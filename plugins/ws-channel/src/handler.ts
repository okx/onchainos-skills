import type { ClawdbotConfig, PluginRuntime } from "openclaw/plugin-sdk";
import type { WsEnvelope } from "./ws-client.js";
import { getRuntime } from "./runtime.js";

export async function handleInboundMessage(params: {
  cfg: ClawdbotConfig;
  accountId: string;
  myAddr: string;
  myAgentId?: string;
  systemPrompt?: string;
  envelope: WsEnvelope;
  reply: (text: string) => void;
  /**
   * "main"  → 路由到 main session（系统通知、TASK_CONFIRMED 等）
   * "sub"   → 路由到子 session（P2P 协商、已激活的会话）
   * 默认 "main"
   */
  sessionMode?: "main" | "sub";
}): Promise<void> {
  const { cfg, accountId, myAddr, myAgentId, systemPrompt, envelope, reply, sessionMode = "main" } = params;
  const core = getRuntime() as PluginRuntime & any;
  const log = core.log ?? console.log;

  log(`[ws-channel] conv:${envelope.conversation_id} from:${envelope.from} type:${envelope.payload.type} mode:${sessionMode}`);

  // 优先使用 llm 字段作为 agent 输入（机器可读摘要+行动指令），否则回退到 content
  const rawBody =
    typeof (envelope.payload as any).llm === "string"
      ? (envelope.payload as any).llm
      : typeof envelope.payload.content === "string"
        ? envelope.payload.content
        : JSON.stringify(envelope.payload);

  // 路由：per-conversation session（用 conversation_id 作为 peer id）
  const route = core.channel.routing.resolveAgentRoute({
    cfg,
    channel: "ws-mock",
    accountId,
    peer: { kind: "direct", id: envelope.conversation_id },
  });

  // TASK_CONFIRMED：路由到 main session，触发 agent turn，由 agent 自行调用 recommend + xmtp_send
  if (envelope.payload.type === "TASK_CONFIRMED") {
    const jobId = envelope.payload.jobId ?? "?";
    const notifyBody = rawBody;
    log(`[ws-channel] TASK_CONFIRMED jobId=${jobId}，触发 main session agent turn`);
    try {
      const notifyCtx = core.channel.reply.finalizeInboundContext({
        Body: notifyBody,
        RawBody: notifyBody,
        CommandBody: notifyBody,
        From: `ws-mock:${envelope.from}`,
        To: `ws-mock:${myAddr}`,
        SessionKey: route.mainSessionKey,
        AccountId: route.accountId,
        ChatType: "direct",
        SenderName: envelope.from,
        SenderId: envelope.from,
        Provider: "ws-mock",
        Surface: "ws-mock",
        MessageSid: `task-confirmed-${jobId}-${Date.now()}`,
        Timestamp: Date.now(),
        WasMentioned: true,
        OriginatingChannel: "ws-mock",
        OriginatingTo: `ws-mock:${myAddr}`,
        MsgType: "TASK_CONFIRMED",
        ...(systemPrompt ? { SystemPrompt: systemPrompt } : {}),
      });
      await core.channel.reply.dispatchReplyWithBufferedBlockDispatcher({
        ctx: notifyCtx,
        cfg,
        dispatcherOptions: {
          deliver: async (payload: any) => {
            if (payload.text) reply(payload.text);
          },
        },
      });
    } catch (err) {
      (core.error ?? console.error)(`[ws-channel] TASK_CONFIRMED dispatch error: ${String(err)}`);
    }
    return;
  }

  const SYSTEM_NOTIFY_TYPES = new Set([
    "TASK_APPLIED", "TASK_ACCEPTED", "TASK_SUBMITTED", "TASK_COMPLETED",
    "TASK_REFUSED", "TASK_REJECTED", "TASK_DISPUTED", "DISPUTE_ASSIGNED",
  ]);

  let body: string = rawBody;
  if (SYSTEM_NOTIFY_TYPES.has(envelope.payload.type)) {
    body = `[系统通知] ${body}`;
  }

  // sub session：注入 skill 加载指令
  if (sessionMode === "sub") {
    const skillDirective = `[系统指令] 回复前必须先加载 okx-agent-task skill，按 SKILL.md 判断角色（消息含[BUYER]→你是Provider→Read provider.md；含[PROVIDER]→你是Client→Read client.md），严格遵守角色文件中的消息格式、行为规则和系统通知角色过滤规则。不得使用markdown、emoji、代码块。\n\n`;
    body = skillDirective + body;
  }

  // sessionMode 决定路由：sub → 子 session（P2P），main → main session
  const sessionKey = sessionMode === "sub" ? route.sessionKey : route.mainSessionKey;

  try {
    const ctxPayload = core.channel.reply.finalizeInboundContext({
      Body: body,
      RawBody: body,
      CommandBody: body,
      From: `ws-mock:${envelope.from}`,
      To: `ws-mock:${myAddr}`,
      SessionKey: sessionKey,
      AccountId: route.accountId,
      ChatType: "direct",
      SenderName: envelope.from,
      SenderId: envelope.from,
      Provider: "ws-mock",
      Surface: "ws-mock",
      MessageSid: `${envelope.conversation_id}-${Date.now()}`,
      Timestamp: Date.now(),
      WasMentioned: true,
      OriginatingChannel: "ws-mock",
      OriginatingTo: `ws-mock:${myAddr}`,
      ConversationId: envelope.conversation_id,
      ...(envelope.payload.task_id ? { TaskId: envelope.payload.task_id } : {}),
      MsgType: envelope.payload.type,
      ...(systemPrompt ? { SystemPrompt: systemPrompt } : {}),
    });

    let replyCount = 0;
    await core.channel.reply.dispatchReplyWithBufferedBlockDispatcher({
      ctx: ctxPayload,
      cfg,
      dispatcherOptions: {
        deliver: async (payload: any) => {
          if (payload.text) {
            replyCount++;
            reply(payload.text);
          }
        },
      },
    });

    log(`[ws-channel] dispatch 完成 (replies=${replyCount} mode=${sessionMode})`);
  } catch (err) {
    (core.error ?? console.error)(`[ws-channel] dispatch error: ${String(err)}`);
  }
}
