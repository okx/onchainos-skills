import type { ChannelPlugin, OpenClawPluginApi, ClawdbotConfig } from "openclaw/plugin-sdk";
import { DEFAULT_ACCOUNT_ID, normalizeAccountId } from "openclaw/plugin-sdk/account-id";
import { setRuntime } from "./runtime.js";
import { handleInboundMessage } from "./handler.js";
import { WsMockClient } from "./ws-client.js";
import { readFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import { createHash, randomBytes } from "node:crypto";


/**
 * 类比生产 XMTP 插件的 SQLite agentId → communicationAddress 映射。
 *
 * 存储在 ~/.openclaw/ws-mock-addresses.json：{ [accountId]: walletAddr }
 * - accountId 作为 key（gateway context 中可获取的 agent 标识）
 * - 首次启动时从 device.json 的 deviceId 派生确定性地址；若不存在则随机生成
 * - 这样同一台机器上不同 accountId 的 openclaw 实例各有独立地址
 */
function resolveOrCreateWalletAddr(accountId: string): string {
  const stateDir = join(homedir(), ".openclaw");
  const mapPath = join(stateDir, "ws-mock-addresses.json");

  // 读取已持久化的映射表
  let addressMap: Record<string, string> = {};
  if (existsSync(mapPath)) {
    try { addressMap = JSON.parse(readFileSync(mapPath, "utf8")); } catch {}
  }

  // 已有记录直接返回（保持地址稳定，类比 SQLite 已存在的行）
  if (typeof addressMap[accountId] === "string" && addressMap[accountId]) {
    return addressMap[accountId];
  }

  // 首次：从 device.json 的 deviceId + accountId 派生确定性地址
  let addr: string;
  const identityPath = join(stateDir, "identity", "device.json");
  if (existsSync(identityPath)) {
    try {
      const identity = JSON.parse(readFileSync(identityPath, "utf8"));
      if (typeof identity.deviceId === "string" && identity.deviceId) {
        addr = "0x" + createHash("sha256").update(identity.deviceId + accountId).digest("hex").slice(0, 40);
      }
    } catch {}
  }
  addr ??= "0x" + randomBytes(20).toString("hex");

  // 持久化（类比 INSERT INTO comm_addresses）
  addressMap[accountId] = addr;
  mkdirSync(stateDir, { recursive: true });
  writeFileSync(mapPath, JSON.stringify(addressMap, null, 2) + "\n");
  return addr;
}

// 预定义角色配置，切角色只需改 config 里的 role 字段
// walletAddr 字段已移除 —— 运行时由 deriveOrLoadWalletAddr() 动态派生
const ROLE_PRESETS: Record<string, { systemPrompt: (addr: string) => string }> = {
  generic: {
    systemPrompt: (addr) => `你是 OKX AI Economy 任务市场的 Agent，运行在 XMTP mock 通信系统上。

## 身份
- 你的钱包地址：${addr}
- 角色：未注册（可通过 identity_register 注册为买家/卖家/仲裁者）

## 消息格式
每条收到的消息包含：
- [会话: <conversationId>] — 调用 xmtp_send 时必须使用此 ID
- 来自: <发送方钱包地址>
- 消息正文

## 可用工具
- xmtp_send(conversationId, content, taskId?) — 发送消息
- xmtp_get_pending_list() — 查看待处理会话
- xmtp_start_conversation(conversationId) — 激活会话
- xmtp_close_conversation(conversationId, reason?) — 关闭会话
- identity_register(role) — 注册身份（REQUESTER/PROVIDER/EVALUATOR）
- identity_lookup(role) — 查询已注册的同角色 Agent`,
  },
  buyer: {
    systemPrompt: (addr) => `你是 OKX AI Economy 任务市场的买家 Agent，运行在 XMTP mock 通信系统上。

## 身份
- 角色：买家（REQUESTER）
- 你的钱包地址：${addr}

## 消息格式
每条收到的消息包含：
- [会话: <conversationId>] — 调用 xmtp_send 时必须使用此 ID
- 来自: <发送方钱包地址>
- 消息正文

## 可用工具
- xmtp_send(conversationId, content, taskId?) — 发送消息
- xmtp_get_pending_list() — 查看待处理会话
- xmtp_start_conversation(conversationId) — 激活会话
- xmtp_close_conversation(conversationId, reason?) — 关闭会话

## 行为规则
1. 收到卖家询问时，介绍任务详情并用 xmtp_send 回复
2. 收到 TASK_ACCEPT 时：用 xmtp_send 确认接单
3. 收到 TASK_DELIVER 时：评估交付，决定发送 TASK_CONFIRM 或 TASK_REJECT
4. 收到仲裁请求时：配合仲裁者提供信息`,
  },
  seller: {
    systemPrompt: (addr) => `你是 OKX AI Economy 任务市场的卖家 Agent，运行在 XMTP mock 通信系统上。

## 身份
- 角色：卖家（PROVIDER）
- 你的钱包地址：${addr}

## 消息格式
每条收到的消息包含：
- [会话: <conversationId>] — 调用 xmtp_send 时必须使用此 ID
- 来自: <发送方钱包地址>
- 消息正文

## 可用工具
- xmtp_send(conversationId, content, taskId?) — 发送消息
- xmtp_get_pending_list() — 查看待处理会话
- xmtp_start_conversation(conversationId) — 激活会话
- xmtp_close_conversation(conversationId, reason?) — 关闭会话

## 行为规则
1. 收到任务询问（TASK_INQUIRE）时：了解任务详情，评估是否接单
2. 决定接单时：发送 TASK_ACCEPT 消息
3. 完成任务后：发送 TASK_DELIVER 消息并附上交付内容
4. 对交付有争议时：可发起 TASK_DISPUTE 请求仲裁`,
  },
  arbitrator: {
    systemPrompt: (addr) => `你是 OKX AI Economy 任务市场的仲裁者 Agent，运行在 XMTP mock 通信系统上。

## 身份
- 角色：仲裁者（EVALUATOR）
- 你的钱包地址：${addr}

## 消息格式
每条收到的消息包含：
- [会话: <conversationId>] — 调用 xmtp_send 时必须使用此 ID
- 来自: <发送方钱包地址>
- 消息正文

## 可用工具
- xmtp_send(conversationId, content, taskId?) — 发送消息
- xmtp_get_pending_list() — 查看待处理会话
- xmtp_start_conversation(conversationId) — 激活会话
- xmtp_close_conversation(conversationId, reason?) — 关闭会话

## 行为规则
1. 收到 TASK_DISPUTE 时：向买卖双方收集信息
2. 综合评估后：发送 TASK_RESOLVE 裁决（winner: buyer 或 seller）
3. 保持中立，基于证据裁决`,
  },
};

interface WsMockAccount {
  accountId: string;
  walletAddr: string;
  serverUrl: string;
  role: string;
  systemPrompt: string;
  enabled: boolean;
  configured: boolean;
}

interface PendingConv {
  conversationId: string;
  peerAddress: string;
  jobId?: string;
  unreadCount: number;
  lastMessage: string;
  timestamp: number;
}

const clients = new Map<string, WsMockClient>();
const pendingConversations = new Map<string, PendingConv>();
const activeConversations = new Set<string>();

function getDefaultClient(): WsMockClient | undefined {
  return clients.get(normalizeAccountId(DEFAULT_ACCOUNT_ID));
}

function resolveAccount(cfg: ClawdbotConfig, accountId?: string | null): WsMockAccount {
  const s = (cfg as any).channels?.["ws-mock"] ?? {};
  const role: string = s.role ?? "";
  const preset = ROLE_PRESETS[role] ?? ROLE_PRESETS["generic"];
  const resolvedAccountId = normalizeAccountId(accountId ?? DEFAULT_ACCOUNT_ID);
  // 显式 walletAddr 优先，否则按 accountId 查/创建（类比生产 SQLite agentId → communicationAddress）
  const walletAddr: string = s.walletAddr || resolveOrCreateWalletAddr(resolvedAccountId);
  const systemPrompt: string = s.systemPrompt || preset.systemPrompt(walletAddr);
  return {
    accountId: resolvedAccountId,
    walletAddr,
    serverUrl: s.serverUrl ?? "ws://127.0.0.1:9000",
    role,
    systemPrompt,
    enabled: true,
    configured: true,
  };
}

export const wsMockPlugin: ChannelPlugin<WsMockAccount> = {
  id: "ws-mock",

  meta: {
    id: "ws-mock",
    label: "WS Mock",
    selectionLabel: "WS Mock (XMTP placeholder)",
    docsPath: "/channels/ws-mock",
    blurb: "WebSocket mock channel for local development, simulates XMTP.",
  },

  capabilities: {
    chatTypes: ["direct"],
    reply: true,
  },

  config: {
    listAccountIds: (cfg) => {
      const s = (cfg as any).channels?.["ws-mock"];
      // 只要配置了 ws-mock（哪怕只有 serverUrl），即可启动；walletAddr 由 deriveOrLoadWalletAddr() 自动派生
      return s ? [DEFAULT_ACCOUNT_ID] : [];
    },
    resolveAccount: (cfg, accountId) => resolveAccount(cfg, accountId),
    defaultAccountId: () => DEFAULT_ACCOUNT_ID,
    isConfigured: (account) => account.configured,
  },

  setup: {
    applyAccountConfig: ({ cfg, input }) => ({
      ...cfg,
      channels: { ...(cfg as any).channels, "ws-mock": { ...(input as any) } },
    } as ClawdbotConfig),
  },

  gateway: {
    startAccount: async (ctx) => {
      const account = resolveAccount(ctx.cfg, ctx.accountId);

      const s = (ctx.cfg as any).channels?.["ws-mock"] ?? {};
      const client = new WsMockClient(account.serverUrl, account.walletAddr);

      // 若 config 未指定 role，连上后从服务端查角色
      let resolvedSystemPrompt = account.systemPrompt;
      try {
        await client.connectAndRegister();
        if (!s.role) {
          const identity = await client.lookupAddr(account.walletAddr);
          if (identity?.role) {
            const detectedRole = identity.role.toLowerCase();
            const preset = ROLE_PRESETS[detectedRole];
            if (preset) resolvedSystemPrompt = s.systemPrompt || preset.systemPrompt(account.walletAddr);
            ctx.log?.info?.(`[ws-channel] 自动识别角色: ${detectedRole} | 地址: ${account.walletAddr}`);
          } else {
            ctx.log?.info?.(`[ws-channel] 地址未注册，使用通用 prompt | 地址: ${account.walletAddr}`);
          }
        } else {
          ctx.log?.info?.(`[ws-channel] 角色: ${account.role} | 地址: ${account.walletAddr}`);
        }
      } catch (e) {
        ctx.log?.warn?.(`[ws-channel] 连接失败，将重试: ${e}`);
      }

      client.start(async (envelope) => {
        // Track pending conversations
        if (!activeConversations.has(envelope.conversation_id)) {
          const existing = pendingConversations.get(envelope.conversation_id);
          pendingConversations.set(envelope.conversation_id, {
            conversationId: envelope.conversation_id,
            peerAddress: envelope.from,
            jobId: envelope.payload.jobId,
            unreadCount: (existing?.unreadCount ?? 0) + 1,
            lastMessage: String(envelope.payload.content ?? ""),
            timestamp: Date.now(),
          });
        }

        await handleInboundMessage({
          cfg: ctx.cfg,
          accountId: account.accountId,
          myAddr: account.walletAddr,
          systemPrompt: resolvedSystemPrompt,
          envelope,
          reply: (text) => {
            ctx.log?.info?.(`[ws-channel] reply via conv:${envelope.conversation_id}`);
            client.sendToConv(envelope.conversation_id, { type: "REPLY", content: text });
            activeConversations.add(envelope.conversation_id);
            pendingConversations.delete(envelope.conversation_id);
          },
        });
      });

      clients.set(account.accountId, client);
      ctx.log?.info?.(`[ws-channel] 已启动，钱包地址: ${account.walletAddr}`);

      await new Promise<void>((resolve) => {
        ctx.abortSignal.addEventListener("abort", () => {
          client.stop();
          clients.delete(account.accountId);
          resolve();
        });
      });
    },
  },

  outbound: {
    deliveryMode: "direct",
    sendText: async (ctx) => {
      const accountId = (ctx as any).accountId as string;
      const conversationId = (ctx as any).conversationId as string;
      const text = (ctx as any).text as string;
      const log = console.log;
      const client = clients.get(accountId);
      if (!client) {
        log(`[ws-channel] sendText: no client for ${accountId}`);
        return { channel: "ws-mock", messageId: "err-no-client" };
      }
      if (!conversationId) {
        log(`[ws-channel] sendText: missing conversationId, ctx keys=${Object.keys(ctx as any).join(",")}`);
        return { channel: "ws-mock", messageId: "err-no-conv" };
      }
      client.sendToConv(conversationId, { type: "TEXT", content: text });
      return { channel: "ws-mock", messageId: `${Date.now()}` };
    },
  },
};

function toolResult(data: unknown) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
    details: data,
  };
}

function registerTools(api: OpenClawPluginApi): void {
  if (typeof (api as any).registerTool !== "function") {
    console.warn("[ws-channel] registerTool not available on api, skipping tool registration");
    return;
  }

  api.registerTool((_ctx) => ({
    name: "xmtp_get_pending_list",
    label: "XMTP Pending List",
    description: "获取当前待回复的沟通会话列表（按最新消息时间排序）。用于查看有哪些卖家/仲裁者发来了未处理的消息。",
    parameters: {
      type: "object" as const,
      properties: {
        limit: { type: "number", description: "返回数量上限，默认全部" },
      },
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { limit?: number };
      const list = Array.from(pendingConversations.values())
        .sort((a, b) => b.timestamp - a.timestamp);
      return toolResult(p.limit ? list.slice(0, p.limit) : list);
    },
  }));

  api.registerTool((_ctx) => ({
    name: "xmtp_start_conversation",
    label: "XMTP Start Conversation",
    description: "接受 pending 队列中优先级最高的沟通请求，激活为当前会话。无需参数，daemon 自动从队列头部取出。激活后使用 xmtp_send 向对方发消息。",
    parameters: {
      type: "object" as const,
      properties: {},
    },
    async execute(_toolCallId: string, _params: unknown) {
      // 按时间戳取队列最早的（先进先出）
      const next = Array.from(pendingConversations.values())
        .sort((a, b) => a.timestamp - b.timestamp)[0];
      if (!next) {
        return toolResult({ status: "empty", message: "当前没有待处理的会话" });
      }
      activeConversations.add(next.conversationId);
      pendingConversations.delete(next.conversationId);
      return toolResult({ status: "active", conversationId: next.conversationId, peerAddress: next.peerAddress, jobId: next.jobId });
    },
  }));

  api.registerTool((_ctx) => ({
    name: "xmtp_send",
    label: "XMTP Send",
    description: "向指定 XMTP 会话发送消息。conversationId 在收到消息时上下文中的 [会话: ...] 字段里。",
    parameters: {
      type: "object" as const,
      properties: {
        conversationId: { type: "string", description: "目标会话 ID（消息上下文 [会话: ...] 中的值）" },
        content: { type: "string", description: "消息内容" },
        jobId: { type: "string", description: "关联任务 ID（可选）" },
      },
      required: ["conversationId", "content"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { conversationId: string; content: string; jobId?: string };
      const client = getDefaultClient();
      if (!client) return toolResult({ error: "ws-mock client not connected" });
      if (!p.conversationId) return toolResult({ error: "conversationId is required" });
      client.sendToConv(p.conversationId, {
        type: "TEXT",
        content: p.content,
        ...(p.jobId ? { jobId: p.jobId } : {}),
      });
      return toolResult({ messageId: `msg-${Date.now()}`, sentAt: new Date().toISOString() });
    },
  }));

  api.registerTool((_ctx) => ({
    name: "xmtp_close_conversation",
    label: "XMTP Close Conversation",
    description: "结束一个沟通会话，释放槽位。任务完成、拒绝或超时时调用。",
    parameters: {
      type: "object" as const,
      properties: {
        conversationId: { type: "string", description: "要关闭的会话 ID" },
        reason: { type: "string", enum: ["completed", "rejected", "timeout", "cancelled"], description: "关闭原因" },
      },
      required: ["conversationId"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { conversationId: string; reason?: string };
      activeConversations.delete(p.conversationId);
      pendingConversations.delete(p.conversationId);
      return toolResult({ status: "closed", conversationId: p.conversationId, reason: p.reason ?? "completed" });
    },
  }));

  api.registerTool((_ctx) => ({
    name: "register_address",
    label: "Register Address",
    description: "向 WS mock 服务器注册新的钱包地址，用于切换钱包或添加多账户（买家/卖家各有独立地址时使用）。注册后该地址立即可以收发消息，并成为后续发送的 from 地址。",
    parameters: {
      type: "object" as const,
      properties: {
        addr: { type: "string", description: "要注册的钱包地址（如切换后的新钱包地址）" },
      },
      required: ["addr"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { addr: string };
      const client = getDefaultClient();
      if (!client) return toolResult({ error: "ws-mock client not connected" });
      try {
        await client.register(p.addr);
        return toolResult({ addr: p.addr, registered: true });
      } catch (e) {
        return toolResult({ error: String(e) });
      }
    },
  }));

  api.registerTool((_ctx) => ({
    name: "identity_register",
    label: "Identity Register",
    description: "向身份系统注册 Agent 身份（模拟 ERC-8004）。role 为 REQUESTER/PROVIDER/EVALUATOR。addr 可选，不传则由系统自动生成 TEE 通信地址。注册成功后返回分配的钱包地址。",
    parameters: {
      type: "object" as const,
      properties: {
        role: { type: "string", enum: ["REQUESTER", "PROVIDER", "EVALUATOR"], description: "Agent 角色" },
        addr: { type: "string", description: "指定钱包地址（可选，不传则自动生成）" },
        metadata: { type: "object", description: "附加元数据（可选）" },
      },
      required: ["role"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { role: string; addr?: string; metadata?: Record<string, unknown> };
      const client = getDefaultClient();
      if (!client) return toolResult({ error: "ws-mock client not connected" });
      try {
        const assignedAddr = await client.registerIdentity(p.role, p.addr, p.metadata);
        return toolResult({ role: p.role, addr: assignedAddr, registered: true });
      } catch (e) {
        return toolResult({ error: String(e) });
      }
    },
  }));

  api.registerTool((_ctx) => ({
    name: "identity_lookup",
    label: "Identity Lookup",
    description: "按角色查询已注册的 Agent 列表。role 为 REQUESTER/PROVIDER/EVALUATOR。用于在发起任务前找到对应角色的 Agent 钱包地址。",
    parameters: {
      type: "object" as const,
      properties: {
        role: { type: "string", enum: ["REQUESTER", "PROVIDER", "EVALUATOR"], description: "要查询的角色" },
      },
      required: ["role"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { role: string };
      const client = getDefaultClient();
      if (!client) return toolResult({ error: "ws-mock client not connected" });
      try {
        const agents = await client.lookupRole(p.role);
        return toolResult({ role: p.role, agents });
      } catch (e) {
        return toolResult({ error: String(e) });
      }
    },
  }));

  console.log("[ws-channel] 已注册 XMTP mock tools: xmtp_get_pending_list, xmtp_start_conversation, xmtp_send, xmtp_close_conversation, register_address, identity_register, identity_lookup");
}

const plugin = {
  id: "ws-mock",
  name: "WS Mock Channel",
  description: "WebSocket mock channel，用于本地开发（XMTP 占位）",
  register(api: OpenClawPluginApi) {
    setRuntime(api.runtime);
    (api as any).registerChannel({ plugin: wsMockPlugin });
    registerTools(api);
  },
};

export default plugin;
