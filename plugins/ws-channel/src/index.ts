import type { ChannelPlugin, OpenClawPluginApi, ClawdbotConfig } from "openclaw/plugin-sdk";
import { DEFAULT_ACCOUNT_ID, normalizeAccountId } from "openclaw/plugin-sdk/account-id";
import { setRuntime, getRuntime } from "./runtime.js";
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

// 角色系统提示：身份 + skill 加载规则 + 消息格式模板（agent 每轮都能看到）
function taskSkillRule(role: string, agentId: string): string {
  const tag = role === "buyer" ? "BUYER" : role === "seller" ? "PROVIDER" : "EVALUATOR";
  return `
[最高优先级规则 - 必须在回复前执行]
收到的消息如果包含以下任何信号: jobId、[BUYER]、[PROVIDER]、TASK_ 开头的类型、任务、协商、接单、交付、仲裁,
则该消息属于任务上下文, 必须按以下顺序处理, 完成前不得输出任何回复:
1. 加载 okx-agent-task skill
2. 按 SKILL.md "How to Determine Your Role" 判断你的角色(消息含[BUYER]标签→你是Provider, 含[PROVIDER]标签→你是Client)
3. Read 对应角色文件(provider.md / client.md / evaluator.md)
4. 严格遵守角色文件中的输出格式和行为规则后再回复
不加载 skill 就直接回复 = 格式错误 = 对方无法解析 = 任务流程中断。

[消息格式模板 - 每条回复必须严格照抄此格式]
你的每一条文字回复必须以下面的header开头(纯文本, 禁止markdown/代码块/emoji):
jobId:  {从来源消息的jobId行提取}
来自:   ${agentId} [${tag}]
类型:   REPLY
会话:   {从来源消息的会话行提取}
----------------------------------------
{回复正文}
注意: 上面"来自:"后面是三个空格再接agentId, "类型:"和"会话:"同理。所有冒号都是半角":"不是全角"："。分隔线是40个连字符"-"。`;
}

const ROLE_PRESETS: Record<string, { systemPrompt: (addr: string, agentId: string) => string }> = {
  generic: {
    systemPrompt: (addr, agentId) => `钱包地址: ${addr}
可用工具: xmtp_send, xmtp_get_pending, xmtp_accept, xmtp_close, xmtp_get_messages, xmtp_upload, xmtp_queue_status, identity_register, identity_lookup
${taskSkillRule("generic", agentId)}`,
  },
  buyer: {
    systemPrompt: (addr, agentId) => `角色: 买家(REQUESTER)
agentId: ${agentId}
钱包地址: ${addr}
${taskSkillRule("buyer", agentId)}`,
  },
  seller: {
    systemPrompt: (addr, agentId) => `角色: 卖家(PROVIDER)
agentId: ${agentId}
钱包地址: ${addr}
${taskSkillRule("seller", agentId)}`,
  },
  arbitrator: {
    systemPrompt: (addr, agentId) => `角色: 仲裁者(EVALUATOR)
agentId: ${agentId}
钱包地址: ${addr}
${taskSkillRule("arbitrator", agentId)}`,
  },
};

interface WsMockAccount {
  accountId: string;
  walletAddr: string;
  /** Logical agent identifier used in conv_id. Defaults to walletAddr if not configured. */
  agentId: string;
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
  /** 完整 envelope，供 xmtp_accept dispatch 时使用 */
  envelope?: import("./ws-client.js").WsEnvelope;
}

/** 供 xmtp_accept 等工具在 tool handler 中访问当前 session 配置 */
let activeCfg: ClawdbotConfig | null = null;
let activeSystemPrompt = "";

interface MessageRecord {
  from: string;
  content: string;
  type: string;
  timestamp: number;
}

const clients = new Map<string, WsMockClient>();
const pendingConversations = new Map<string, PendingConv>();
const activeConversations = new Set<string>();
const messageHistory = new Map<string, MessageRecord[]>();
/** 当前活跃账户，供工具访问 agentId 等配置 */
let activeAccount: WsMockAccount | null = null;
/** 最近一次 sub-session dispatch 的 convId，供 outbound sendText 兜底 */
let lastDispatchedConvId: string | null = null;

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
  const agentId: string = s.agentId || walletAddr;
  const systemPrompt: string = s.systemPrompt || preset.systemPrompt(walletAddr, agentId);
  return {
    accountId: resolvedAccountId,
    walletAddr,
    agentId,
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
      const roleToErc: Record<string, string> = {
        buyer: "REQUESTER", seller: "PROVIDER", arbitrator: "EVALUATOR",
      };
      try {
        await client.connectAndRegister();
        // 自动注册 ERC-8004 身份（agentId + commAddr）
        const ercRole = roleToErc[account.role];
        if (ercRole) {
          await client.registerIdentity(ercRole, account.agentId, account.walletAddr).catch((e) => {
            ctx.log?.warn?.(`[ws-channel] auto identity register failed: ${e}`);
          });
          ctx.log?.info?.(`[ws-channel] 身份已注册: role=${ercRole} agentId=${account.agentId} commAddr=${account.walletAddr}`);
        } else {
          // 无 role 配置时尝试从服务端查已注册身份
          const identity = await client.lookupAddr(account.walletAddr);
          if (identity?.role) {
            const detectedRole = (identity.role as string).toLowerCase();
            const preset = ROLE_PRESETS[detectedRole];
            if (preset) resolvedSystemPrompt = s.systemPrompt || preset.systemPrompt(account.walletAddr, account.agentId);
            ctx.log?.info?.(`[ws-channel] 自动识别角色: ${detectedRole} | agentId: ${account.agentId}`);
          } else {
            ctx.log?.info?.(`[ws-channel] 身份未注册，使用通用 prompt | agentId: ${account.agentId}`);
          }
        }
      } catch (e) {
        ctx.log?.warn?.(`[ws-channel] 连接失败，将重试: ${e}`);
      }

      // 保存当前 session 配置供工具（xmtp_accept 等）访问
      activeCfg = ctx.cfg;
      activeSystemPrompt = resolvedSystemPrompt;

      // 仅 TASK_CONFIRMED 走 main session（买家启动流程）
      // 其余链上通知走子 session（保持 P2P 上下文连贯）
      const SYSTEM_MSG_TYPES = new Set(["TASK_CONFIRMED"]);

      // per-session 串行队列：保证同一 convId 的消息严格按到达顺序处理
      // 解决 openclaw "queued messages while agent was busy" 导致的乱序问题
      const dispatchQueues = new Map<string, Promise<void>>();
      function enqueueDispatch(key: string, fn: () => Promise<void>): Promise<void> {
        const prev = dispatchQueues.get(key) ?? Promise.resolve();
        const next = prev.then(fn, fn);
        dispatchQueues.set(key, next);
        return next;
      }

      client.start(async (envelope) => {
        const convId = envelope.conversation_id;
        const msgType = envelope.payload.type;

        // 1. 记录消息历史
        const record: MessageRecord = {
          from: envelope.from,
          content: String(envelope.payload.content ?? ""),
          type: msgType,
          timestamp: Date.now(),
        };
        const hist = messageHistory.get(convId) ?? [];
        hist.push(record);
        messageHistory.set(convId, hist);

        // 2. CLI 自回显：CLI 发出消息时会话已建立，标记为活跃 conv，seller 回复走子 session
        if (envelope.from.startsWith("0xCLI-")) {
          if (!activeConversations.has(convId)) {
            ctx.log?.info?.(`[ws-channel] CLI echo: activating conv ${convId} type=${msgType}`);
            activeConversations.add(convId);
          }
          return;
        }

        const makeReply = (cid: string) => (text: string) => {
          ctx.log?.info?.(`[ws-channel] reply via conv:${cid} type=REPLY content=${JSON.stringify(text.slice(0, 200))}`);
          client.sendToConv(cid, { type: "REPLY", content: text });
        };

        // 3. 仅 TASK_CONFIRMED 走 main session
        if (SYSTEM_MSG_TYPES.has(msgType)) {
          ctx.log?.info?.(`[ws-channel] ${msgType} → main session (not sub)`);
          await enqueueDispatch("main", () => handleInboundMessage({
            cfg: ctx.cfg,
            accountId: account.accountId,
            myAddr: account.walletAddr,
            myAgentId: account.agentId,
            systemPrompt: resolvedSystemPrompt,
            envelope,
            sessionMode: "main",
            reply: makeReply(convId),
          }));
          return;
        }

        // 4. P2P 消息：active conv → 子 session；新 conv → 判断是否自动激活
        //    TASK_INQUIRE：seller 收到买家发起的新协商请求，自动激活子 session（无需 xmtp_accept）
        if (msgType === "TASK_INQUIRE" && !activeConversations.has(convId)) {
          ctx.log?.info?.(`[ws-channel] auto-accept TASK_INQUIRE conv=${convId}`);
          activeConversations.add(convId);
        }

        if (activeConversations.has(convId)) {
          lastDispatchedConvId = convId;
          await enqueueDispatch(convId, () => handleInboundMessage({
            cfg: ctx.cfg,
            accountId: account.accountId,
            myAddr: account.walletAddr,
            myAgentId: account.agentId,
            systemPrompt: resolvedSystemPrompt,
            envelope,
            sessionMode: "sub",
            reply: makeReply(convId),
          }));
        } else {
          // 新 conv：存入 pending（带完整 envelope），通知 main session
          const existing = pendingConversations.get(convId);
          pendingConversations.set(convId, {
            conversationId: convId,
            peerAddress: envelope.from,
            jobId: envelope.payload.jobId,
            unreadCount: (existing?.unreadCount ?? 0) + 1,
            lastMessage: String(envelope.payload.content ?? ""),
            timestamp: Date.now(),
            envelope,
          });
          await notifyMainSessionOfPendingConv({
            cfg: ctx.cfg,
            accountId: account.accountId,
            myAddr: account.walletAddr,
            myAgentId: account.agentId,
            systemPrompt: resolvedSystemPrompt,
            envelope,
          });
        }
      });

      clients.set(account.accountId, client);
      activeAccount = account;
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
        log(`[ws-channel] sendText: no conversationId, dropping (main session output not forwarded to P2P)`);
        return { channel: "ws-mock", messageId: "dropped-no-conv" };
      }
      client.sendToConv(conversationId, { type: "REPLY", content: text });
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

/** 新 P2P 会话到达时，向 main session 推送通知，提示 agent 调用 xmtp_accept */
async function notifyMainSessionOfPendingConv(params: {
  cfg: ClawdbotConfig;
  accountId: string;
  myAddr: string;
  myAgentId?: string;
  systemPrompt?: string;
  envelope: import("./ws-client.js").WsEnvelope;
}): Promise<void> {
  const { cfg, accountId, myAddr, systemPrompt, envelope } = params;
  const core = getRuntime() as any;
  const convId = envelope.conversation_id;
  const jobId = envelope.payload.jobId ?? "?";
  const peerFrom = envelope.from;
  const content = String(envelope.payload.content ?? "");

  const notifyBody =
    `[新沟通请求] 来自 ${peerFrom}（jobId: ${jobId}）\n` +
    `消息：${content}\n\n` +
    `⚠️ 必须调用工具 xmtp_accept，参数 conversationId="${convId}"，激活会话后再用 xmtp_send 回复卖家。禁止调用 CLI 命令。`;

  try {
    const route = core.channel.routing.resolveAgentRoute({
      cfg, channel: "ws-mock", accountId,
      peer: { kind: "direct", id: convId },
    });
    const notifyCtx = core.channel.reply.finalizeInboundContext({
      Body: notifyBody, RawBody: notifyBody, CommandBody: notifyBody,
      From: `ws-mock:${peerFrom}`, To: `ws-mock:${myAddr}`,
      SessionKey: route.mainSessionKey,
      AccountId: route.accountId,
      ChatType: "direct",
      SenderName: peerFrom, SenderId: peerFrom,
      Provider: "ws-mock", Surface: "ws-mock",
      MessageSid: `pending-notify-${convId}-${Date.now()}`,
      Timestamp: Date.now(),
      WasMentioned: true,
      OriginatingChannel: "ws-mock", OriginatingTo: `ws-mock:${myAddr}`,
      MsgType: "PENDING_NOTIFY",
      ...(systemPrompt ? { SystemPrompt: systemPrompt } : {}),
    });
    await core.channel.reply.dispatchReplyWithBufferedBlockDispatcher({
      ctx: notifyCtx, cfg,
      dispatcherOptions: {
        deliver: async (payload: any) => {
          if (payload.text) {
            (core.log ?? console.log)(`[ws-channel] main session ack pending: ${payload.text.slice(0, 100)}`);
          }
        },
      },
    });
  } catch (err) {
    (core.error ?? console.error)(`[ws-channel] notifyMainSession error: ${String(err)}`);
  }
}

function registerTools(api: OpenClawPluginApi): void {
  if (typeof (api as any).registerTool !== "function") {
    console.warn("[ws-channel] registerTool not available on api, skipping tool registration");
    return;
  }

  // ── xmtp_get_pending ────────────────────────────────────────────────────────
  // 对齐通信组接口：查询当前待回复的沟通请求列表，按信誉分排序
  api.registerTool((_ctx) => ({
    name: "xmtp_get_pending",
    label: "XMTP Get Pending",
    description: "查询当前待回复的沟通请求列表，按最新消息时间排序。用于查看有哪些对手方发来了未处理的消息。",
    parameters: {
      type: "object" as const,
      properties: {
        agentId: { type: "string", description: "指定 agentId 过滤（可选，默认当前 agent）" },
        since: { type: "string", description: "ISO 8601 时间，只返回该时间之后的（可选）" },
        limit: { type: "number", description: "返回数量上限，默认全部" },
      },
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { agentId?: string; since?: string; limit?: number };
      let list = Array.from(pendingConversations.values())
        .sort((a, b) => b.timestamp - a.timestamp);
      if (p.since) {
        const sinceTs = new Date(p.since).getTime();
        list = list.filter((c) => c.timestamp >= sinceTs);
      }
      return toolResult(p.limit ? list.slice(0, p.limit) : list);
    },
  }));

  // ── xmtp_send ───────────────────────────────────────────────────────────────
  // 对齐通信组接口：向指定会话发送消息。
  // main session 调用时：派生 conv_id，joinConversation，发送消息（创建新会话）。
  // subsession（已有 conv）调用时：直接发送。
  api.registerTool((_ctx) => ({
    name: "xmtp_send",
    label: "XMTP Send",
    description: "向任务对手方发送 P2P 消息。main session 中调用时自动创建会话；会话内调用时直接发送。",
    parameters: {
      type: "object" as const,
      properties: {
        toAgentId: { type: "string", description: "接收方 agentId" },
        fromAgentId: { type: "string", description: "发送方 agentId（可选，默认当前 agent）" },
        taskId: { type: "string", description: "任务 ID" },
        content: { type: "string", description: "消息正文" },
        contentType: { type: "string", enum: ["text", "markdown"], description: "内容类型，默认 text" },
        payload: { type: "object", description: "扩展元数据（可选）" },
      },
      required: ["toAgentId", "taskId", "content"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { toAgentId: string; fromAgentId?: string; taskId: string; content: string; contentType?: string; payload?: Record<string, unknown> };
      const client = getDefaultClient();
      if (!client) return toolResult({ error: "ws-mock client not connected" });

      // 查询对方身份，获取规范 agentId 和 commAddr
      const identity = await client.lookupAddr(p.toAgentId).catch(() => null);
      const peerAgentId = identity?.agent_id ?? p.toAgentId;
      const peerCommAddr = identity?.comm_addr ?? p.toAgentId;
      const myAgentId = activeAccount?.agentId ?? client.commAddr;
      const myCommAddr = client.commAddr;

      console.log(`[ws-channel] xmtp_send lookup: toAgentId=${p.toAgentId} identity=${JSON.stringify(identity)} peerCommAddr=${peerCommAddr}`);

      if (!identity) {
        console.warn(`[ws-channel] xmtp_send: identity lookup failed for ${p.toAgentId}, peerCommAddr falls back to agentId — delivery may fail`);
      }

      // conv_id 与 task_contact_seller、mock_api 保持一致
      const convId = `conv-${p.taskId}-${myAgentId}-${peerAgentId}`;

      // 新会话：先 joinConversation 建立参与者（等同于 main session 发起）
      if (!activeConversations.has(convId)) {
        client.joinConversation(convId, [myCommAddr, peerCommAddr]);
        activeConversations.add(convId);
      }

      console.log(`[ws-channel] xmtp_send to=${peerAgentId}(${peerCommAddr}) from=${myAgentId}(${myCommAddr}) convId=${convId} content=${JSON.stringify(p.content.slice(0, 100))}`);

      client.sendToConv(convId, {
        type: "REPLY",
        content: p.content,
        jobId: p.taskId,
        ...(p.payload ?? {}),
      });

      return toolResult({ messageId: `msg-${Date.now()}`, sentAt: new Date().toISOString(), convId });
    },
  }));

  // ── xmtp_get_messages ───────────────────────────────────────────────────────
  // 对齐通信组接口：获取指定会话的历史消息
  api.registerTool((_ctx) => ({
    name: "xmtp_get_messages",
    label: "XMTP Get Messages",
    description: "获取指定会话的历史消息。",
    parameters: {
      type: "object" as const,
      properties: {
        conversationId: { type: "string", description: "会话 ID" },
        limit: { type: "number", description: "返回数量上限，默认全部" },
        cursor: { type: "string", description: "分页游标（可选）" },
      },
      required: ["conversationId"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { conversationId: string; limit?: number; cursor?: string };
      const hist = messageHistory.get(p.conversationId) ?? [];
      const messages = p.limit ? hist.slice(-p.limit) : hist;
      return toolResult({ conversationId: p.conversationId, messages, total: hist.length });
    },
  }));

  // ── xmtp_accept ─────────────────────────────────────────────────────────────
  // 对齐通信组接口：接受一个排队中的沟通请求，激活为当前会话
  api.registerTool((_ctx) => ({
    name: "xmtp_accept",
    label: "XMTP Accept",
    description: "接受一个排队中的沟通请求，激活为当前会话。先调用 xmtp_get_pending 获取 conversationId，再调用此工具激活。激活后使用 xmtp_send 向对方发消息。",
    parameters: {
      type: "object" as const,
      properties: {
        conversationId: { type: "string", description: "要接受的会话 ID（从 xmtp_get_pending 返回的列表中获取）" },
      },
      required: ["conversationId"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { conversationId: string };
      const conv = pendingConversations.get(p.conversationId);
      if (!conv) {
        return toolResult({ error: `会话 ${p.conversationId} 不存在或已激活` });
      }

      // 标记为活跃子 session
      activeConversations.add(p.conversationId);
      pendingConversations.delete(p.conversationId);

      // 将缓存的 envelope dispatch 到子 session
      if (conv.envelope && activeCfg) {
        const client = getDefaultClient();
        if (client) {
          const envelope = conv.envelope;
          const convId = p.conversationId;
          handleInboundMessage({
            cfg: activeCfg,
            accountId: normalizeAccountId(DEFAULT_ACCOUNT_ID),
            myAddr: activeAccount?.walletAddr ?? "",
            myAgentId: activeAccount?.agentId,
            systemPrompt: activeSystemPrompt,
            envelope,
            sessionMode: "sub",
            reply: (text) => {
              console.log(`[ws-channel] xmtp_accept reply conv:${convId} content=${JSON.stringify(text.slice(0, 100))}`);
              client.sendToConv(convId, { type: "REPLY", content: text });
            },
          }).catch((err) => console.error(`[ws-channel] xmtp_accept dispatch error: ${err}`));
        }
      }

      return toolResult({ status: "active", conversationId: p.conversationId, peerAddress: conv.peerAddress, jobId: conv.jobId });
    },
  }));

  // ── xmtp_close ──────────────────────────────────────────────────────────────
  // 对齐通信组接口：结束或拒绝一个沟通会话，释放槽位
  api.registerTool((_ctx) => ({
    name: "xmtp_close",
    label: "XMTP Close",
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
      messageHistory.delete(p.conversationId);
      return toolResult({ status: "closed", conversationId: p.conversationId, reason: p.reason ?? "completed" });
    },
  }));

  // ── xmtp_upload ─────────────────────────────────────────────────────────────
  // 对齐通信组接口：上传交付产物或仲裁文件到 CDN，返回可访问 URL
  api.registerTool((_ctx) => ({
    name: "xmtp_upload",
    label: "XMTP Upload",
    description: "上传交付产物或仲裁文件，返回可访问 URL。mock 环境直接返回占位 URL，无需真实上传。",
    parameters: {
      type: "object" as const,
      properties: {
        filename: { type: "string", description: "文件名" },
        mimeType: { type: "string", description: "MIME 类型，如 text/html、image/png" },
        data: { type: "string", description: "base64 编码的文件内容" },
        purpose: { type: "string", enum: ["deliverable", "arbitration", "attachment"], description: "用途" },
      },
      required: ["filename", "mimeType", "data"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { filename: string; mimeType: string; data: string; purpose?: string };
      // mock：返回占位 URL，不做真实上传
      const mockUrl = `https://mock-cdn.ws-mock.local/${Date.now()}-${p.filename}`;
      console.log(`[ws-channel] xmtp_upload mock: ${p.filename} (${p.mimeType}) purpose=${p.purpose ?? "deliverable"}`);
      return toolResult({ url: mockUrl, filename: p.filename, mimeType: p.mimeType, purpose: p.purpose ?? "deliverable" });
    },
  }));

  // ── xmtp_queue_status ───────────────────────────────────────────────────────
  // 对齐通信组接口：查询当前并发会话数、队列深度等状态
  api.registerTool((_ctx) => ({
    name: "xmtp_queue_status",
    label: "XMTP Queue Status",
    description: "查询当前并发会话数、队列深度等状态。",
    parameters: {
      type: "object" as const,
      properties: {
        agentId: { type: "string", description: "指定 agentId（可选，默认当前 agent）" },
      },
    },
    async execute(_toolCallId: string, _params: unknown) {
      return toolResult({
        activeCount: activeConversations.size,
        pendingCount: pendingConversations.size,
        agentId: activeAccount?.agentId ?? "unknown",
      });
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
    description: "向身份系统注册 Agent 身份（模拟 ERC-8004）。agentId 为逻辑标识（用于 conv_id），commAddr 自动使用当前连接地址。",
    parameters: {
      type: "object" as const,
      properties: {
        role: { type: "string", enum: ["REQUESTER", "PROVIDER", "EVALUATOR"], description: "Agent 角色" },
        agentId: { type: "string", description: "逻辑 agentId（可选，不传则使用 commAddr）" },
        metadata: { type: "object", description: "附加元数据（可选）" },
      },
      required: ["role"],
    },
    async execute(_toolCallId: string, params: unknown) {
      const p = params as { role: string; agentId?: string; metadata?: Record<string, unknown> };
      const client = getDefaultClient();
      if (!client) return toolResult({ error: "ws-mock client not connected" });
      const commAddr = client.commAddr;
      const agentId = p.agentId ?? commAddr;
      try {
        await client.registerIdentity(p.role, agentId, commAddr, p.metadata);
        return toolResult({ role: p.role, agentId, commAddr, registered: true });
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

  console.log("[ws-channel] 已注册 XMTP mock tools: xmtp_send, xmtp_get_pending, xmtp_accept, xmtp_close, xmtp_get_messages, xmtp_upload, xmtp_queue_status, xmtp_start_conversation, register_address, identity_register, identity_lookup");
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
