import WebSocket from "ws";

export interface WsEnvelope {
  from: string;
  conversation_id: string;
  payload: TaskPayload;
}

export interface TaskPayload {
  type: string;
  jobId?: string;
  content?: string;
  [key: string]: unknown;
}

type MessageHandler = (envelope: WsEnvelope) => void;

export class WsMockClient {
  private ws: WebSocket | null = null;
  private handler: MessageHandler | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  /** The address used as `from` when sending — updated by register(). */
  private activeAddr: string;

  constructor(
    private readonly serverUrl: string,
    private readonly myAddr: string,
  ) {
    this.activeAddr = myAddr;
  }

  /** The comm_addr this client is registered with (WS routing address). */
  get commAddr(): string { return this.myAddr; }

  /** 连接并等待注册完成，返回后可安全调用 lookupAddr 等方法 */
  connectAndRegister(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.serverUrl);

      this.ws.on("open", () => {
        this.ws!.send(JSON.stringify({ action: "Register", addr: this.myAddr }));
      });

      const onFirstMsg = (data: WebSocket.RawData) => {
        try {
          const msg = JSON.parse(data.toString());
          if (msg.type === "registered") {
            console.log(`[ws-channel] connected as ${this.myAddr}`);
            this.ws!.off("message", onFirstMsg);
            this._attachPermanentHandlers();
            resolve();
          }
        } catch {}
      };
      this.ws.on("message", onFirstMsg);

      this.ws.on("error", (err) => {
        console.error("[ws-channel] error:", err.message);
        reject(err);
      });

      setTimeout(() => reject(new Error("connect timeout")), 10000);
    });
  }

  start(onMessage: MessageHandler): void {
    this.handler = onMessage;
    // If already connected via connectAndRegister(), reuse the connection.
    // _attachPermanentHandlers() already references this.handler via optional chaining.
    if (this.ws?.readyState === WebSocket.OPEN) return;
    this.connect();
  }

  private _attachPermanentHandlers(): void {
    this.ws!.on("message", (data) => {
      try {
        const msg = JSON.parse(data.toString());
        if (!msg.from) return;
        this.handler?.(msg as WsEnvelope);
      } catch (e) {
        console.error("[ws-channel] parse error:", e);
      }
    });

    this.ws!.on("close", () => {
      console.log("[ws-channel] disconnected, reconnecting in 3s...");
      this.reconnectTimer = setTimeout(() => this.connect(), 3000);
    });
  }

  private connect(): void {
    this.ws = new WebSocket(this.serverUrl);

    this.ws.on("open", () => {
      console.log(`[ws-channel] connected as ${this.myAddr}`);
      this.ws!.send(JSON.stringify({ action: "Register", addr: this.myAddr }));
    });

    this.ws.on("message", (data) => {
      try {
        const msg = JSON.parse(data.toString());
        if (!msg.from) return; // skip acks (registered, conversation_joined, error)
        this.handler?.(msg as WsEnvelope);
      } catch (e) {
        console.error("[ws-channel] parse error:", e);
      }
    });

    this.ws.on("close", () => {
      console.log("[ws-channel] disconnected, reconnecting in 3s...");
      this.reconnectTimer = setTimeout(() => this.connect(), 3000);
    });

    this.ws.on("error", (err) => {
      console.error("[ws-channel] error:", err.message);
    });
  }

  /**
   * Dynamically register an additional wallet address on the same WS connection.
   * Useful when the user switches wallets after startup.
   * The new address becomes the active `from` address for subsequent sends.
   */
  register(addr: string): Promise<void> {
    return new Promise((resolve, reject) => {
      if (this.ws?.readyState !== WebSocket.OPEN) {
        reject(new Error("[ws-channel] not connected"));
        return;
      }
      const onMsg = (data: WebSocket.RawData) => {
        try {
          const msg = JSON.parse(data.toString());
          if (msg.type === "registered" && msg.addr === addr) {
            this.ws!.off("message", onMsg);
            this.activeAddr = addr;
            resolve();
          }
        } catch {}
      };
      this.ws.on("message", onMsg);
      this.ws.send(JSON.stringify({ action: "Register", addr }));
      setTimeout(() => { this.ws?.off("message", onMsg); reject(new Error("register timeout")); }, 5000);
    });
  }

  /** Join a conversation (register participants on server) */
  joinConversation(conversationId: string, participants: string[]): void {
    if (this.ws?.readyState !== WebSocket.OPEN) {
      console.error("[ws-channel] not connected, cannot join conversation");
      return;
    }
    this.ws.send(JSON.stringify({ action: "JoinConversation", conversation_id: conversationId, participants }));
  }

  /** Send a message to a conversation */
  sendToConv(conversationId: string, payload: TaskPayload): void {
    if (!conversationId) {
      console.error("[ws-channel] sendToConv called with empty conversationId — dropping message");
      return;
    }
    if (this.ws?.readyState !== WebSocket.OPEN) {
      console.error("[ws-channel] not connected, cannot send");
      return;
    }
    this.ws.send(JSON.stringify({ action: "Send", conversation_id: conversationId, payload }));
  }

  /** Register agent identity with mock identity system.
   * @param role     ERC-8004 role: REQUESTER / PROVIDER / EVALUATOR
   * @param agentId  Logical agent identifier used in conv_id
   * @param commAddr WS routing address (used for message delivery)
   */
  registerIdentity(role: string, agentId: string, commAddr: string, metadata?: Record<string, unknown>): Promise<void> {
    return new Promise((resolve, reject) => {
      if (this.ws?.readyState !== WebSocket.OPEN) {
        reject(new Error("[ws-channel] not connected"));
        return;
      }
      const onMsg = (data: WebSocket.RawData) => {
        try {
          const msg = JSON.parse(data.toString());
          if (msg.type === "identity_registered" && msg.role === role) {
            this.ws!.off("message", onMsg);
            resolve();
          }
        } catch {}
      };
      this.ws.on("message", onMsg);
      const payload: Record<string, unknown> = { action: "RegisterIdentity", role, agent_id: agentId, comm_addr: commAddr };
      if (metadata) payload.metadata = metadata;
      this.ws.send(JSON.stringify(payload));
      setTimeout(() => { this.ws?.off("message", onMsg); reject(new Error("identity_register timeout")); }, 5000);
    });
  }

  /** Look up the registered role for a given address */
  lookupAddr(addr: string): Promise<{ addr: string; role: string; metadata: unknown } | null> {
    return new Promise((resolve, reject) => {
      if (this.ws?.readyState !== WebSocket.OPEN) {
        reject(new Error("[ws-channel] not connected"));
        return;
      }
      const onMsg = (data: WebSocket.RawData) => {
        try {
          const msg = JSON.parse(data.toString());
          if (msg.type === "addr_lookup" && msg.addr === addr) {
            this.ws!.off("message", onMsg);
            resolve(msg.identity ?? null);
          }
        } catch {}
      };
      this.ws.on("message", onMsg);
      this.ws.send(JSON.stringify({ action: "LookupAddr", addr }));
      setTimeout(() => { this.ws?.off("message", onMsg); resolve(null); }, 3000);
    });
  }

  /** Look up agents registered with a given role */
  lookupRole(role: string): Promise<Array<{ addr: string; role: string; metadata: unknown }>> {
    return new Promise((resolve, reject) => {
      if (this.ws?.readyState !== WebSocket.OPEN) {
        reject(new Error("[ws-channel] not connected"));
        return;
      }
      const onMsg = (data: WebSocket.RawData) => {
        try {
          const msg = JSON.parse(data.toString());
          if (msg.type === "identity_lookup" && msg.role === role) {
            this.ws!.off("message", onMsg);
            resolve(msg.agents ?? []);
          }
        } catch {}
      };
      this.ws.on("message", onMsg);
      this.ws.send(JSON.stringify({ action: "LookupRole", role }));
      setTimeout(() => { this.ws?.off("message", onMsg); reject(new Error("lookup_role timeout")); }, 5000);
    });
  }

  stop(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.ws?.close();
  }
}
