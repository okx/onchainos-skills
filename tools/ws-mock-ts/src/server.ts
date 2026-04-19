/**
 * WS Mock Server — TypeScript port of server.rs
 *
 * Actions: Register, JoinConversation, Send,
 *          RegisterIdentity, LookupRole, LookupAddr, ListIdentities
 *
 * Port: 9000
 */
import { WebSocketServer, WebSocket } from "ws";

interface IdentityEntry {
  agent_id: string;
  comm_addr: string;
  role: string;
  metadata: unknown;
}

// addr → WebSocket
const registry = new Map<string, WebSocket>();
// convId → participants (comm_addrs)
const conversations = new Map<string, string[]>();
// role → entries
const identities = new Map<string, IdentityEntry[]>();

const wss = new WebSocketServer({ host: "127.0.0.1", port: 9000 });
console.log("[server] listening on ws://127.0.0.1:9000");

wss.on("connection", (ws) => {
  const myAddrs: string[] = [];
  console.log("[server] new connection");

  ws.on("message", (raw) => {
    let msg: Record<string, unknown>;
    try { msg = JSON.parse(raw.toString()); } catch { return; }

    const action = msg.action as string;

    if (action === "Register") {
      const addr = msg.addr as string;
      registry.set(addr, ws);
      if (!myAddrs.includes(addr)) myAddrs.push(addr);
      console.log(`[server] registered: ${addr}`);
      ws.send(JSON.stringify({ type: "registered", addr }));

    } else if (action === "JoinConversation") {
      const convId = msg.conversation_id as string;
      const parts  = msg.participants as string[];
      conversations.set(convId, parts);
      console.log(`[server] join conv ${convId}: [${parts.join(", ")}]`);
      ws.send(JSON.stringify({ type: "conversation_joined", conversation_id: convId }));

    } else if (action === "Send") {
      const convId  = msg.conversation_id as string;
      const payload = msg.payload;
      const from    = myAddrs.at(-1) ?? "unknown";
      const preview = JSON.stringify(payload).slice(0, 120);
      console.log(`[server] ${from.slice(0, 20)} → conv:${convId.slice(-30)}: ${preview}`);

      const parts = conversations.get(convId);
      if (!parts?.length) {
        ws.send(JSON.stringify({ type: "error", msg: `conversation ${convId} not found — call JoinConversation first` }));
        return;
      }
      let delivered = 0;
      for (const p of parts) {
        if (p === from) continue;
        const dest = registry.get(p);
        if (dest?.readyState === WebSocket.OPEN) {
          dest.send(JSON.stringify({ from, conversation_id: convId, payload }));
          delivered++;
        }
      }
      if (!delivered) {
        ws.send(JSON.stringify({ type: "error", msg: `no participants online in ${convId}` }));
      }

    } else if (action === "RegisterIdentity") {
      const { role, agent_id, comm_addr, metadata } = msg as Record<string, string>;
      const entry: IdentityEntry = { agent_id, comm_addr, role, metadata: (msg.metadata ?? null) };
      if (!identities.has(role)) identities.set(role, []);
      identities.get(role)!.push(entry);
      console.log(`[server] identity: role=${role} agent_id=${agent_id} comm_addr=${comm_addr}`);
      ws.send(JSON.stringify({ type: "identity_registered", role, agent_id, comm_addr }));

    } else if (action === "LookupAddr") {
      const addr = msg.addr as string;
      let found: IdentityEntry | undefined;
      for (const entries of identities.values()) {
        found = entries.find(e => e.agent_id === addr || e.comm_addr === addr);
        if (found) break;
      }
      ws.send(JSON.stringify({ type: "addr_lookup", agent_id: addr, identity: found ?? null }));

    } else if (action === "LookupRole") {
      const role = msg.role as string;
      ws.send(JSON.stringify({ type: "identity_lookup", role, agents: identities.get(role) ?? [] }));

    } else if (action === "ListIdentities") {
      const all: IdentityEntry[] = [];
      for (const entries of identities.values()) all.push(...entries);
      ws.send(JSON.stringify({ type: "identity_list", identities: all }));

    } else {
      console.log(`[server] unknown action: ${action}`);
    }
  });

  ws.on("close", () => {
    for (const addr of myAddrs) registry.delete(addr);
    // 同时清理该连接注册的 identity，避免断线后身份残留
    for (const [role, entries] of identities) {
      const before = entries.length;
      const filtered = entries.filter(e => !myAddrs.includes(e.comm_addr));
      identities.set(role, filtered);
      if (filtered.length < before)
        console.log(`[server] identity cleanup: role=${role} removed ${before - filtered.length} entries`);
    }
    if (myAddrs.length) console.log(`[server] disconnected, removed: ${myAddrs.join(", ")}`);
  });

  ws.on("error", (err) => console.error("[server] ws error:", err.message));
});
