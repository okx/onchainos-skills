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

  // 链上状态变更通知：同时推送到 main session（用户感知），再 fall-through 到子 session
  const NOTIFY_MAIN_TYPES = new Set([
    "TASK_APPLIED",    // 卖家申请接单
    "TASK_ACCEPTED",   // 接单成功
    "TASK_SUBMITTED",  // 交付物已上链
    "TASK_COMPLETED",  // 验收通过
    "TASK_REFUSED",    // 买家拒绝交付物 → 用户决定是否仲裁
    "TASK_AGREEREFUND",// 卖家同意退款
    "TASK_DISPUTED",   // 进入仲裁
    "DISPUTE_RESOLVED",// 仲裁结果
  ]);
  if (NOTIFY_MAIN_TYPES.has(envelope.payload.type)) {
    const jobId = (envelope.payload as any).jobId ?? envelope.payload.task_id ?? "?";
    log(`[ws-channel] ${envelope.payload.type} jobId=${jobId}，向 main session 推送通知`);
    try {
      const notifyCtx = core.channel.reply.finalizeInboundContext({
        Body: rawBody,
        RawBody: rawBody,
        CommandBody: rawBody,
        From: `ws-mock:system`,
        To: `ws-mock:${myAddr}`,
        SessionKey: route.mainSessionKey,
        AccountId: route.accountId,
        ChatType: "direct",
        SenderName: "system",
        SenderId: "system",
        Provider: "ws-mock",
        Surface: "ws-mock",
        MessageSid: `notify-${envelope.payload.type}-${jobId}-${Date.now()}`,
        Timestamp: Date.now(),
        WasMentioned: true,
        OriginatingChannel: "ws-mock",
        OriginatingTo: `ws-mock:${myAddr}`,
        MsgType: "NOTIFY",
      });
      await core.channel.reply.dispatchReplyWithBufferedBlockDispatcher({
        ctx: notifyCtx,
        cfg,
        dispatcherOptions: { deliver: async (_payload: any) => {} },
      });
    } catch (err) {
      (core.error ?? console.error)(`[ws-channel] ${envelope.payload.type} notify error: ${String(err)}`);
    }
    // 继续 fall-through，投递到子 session
  }

  // 链上状态通知 vs P2P 消息：不同的 body 策略
  const isChainNotify = NOTIFY_MAIN_TYPES.has(envelope.payload.type);
  let body: string;
  if (isChainNotify) {
    // 链上通知：用 content（通用可读文本），不用 llm（含角色专属指令，对另一方无意义）
    body = typeof envelope.payload.content === "string" ? envelope.payload.content : rawBody;
  } else {
    // P2P 消息：优先使用 llm 字段（机器可读摘要+行动指令）
    const llmField = (envelope.payload as any).llm as string | undefined;
    body = llmField ?? rawBody;
  }

  // sub session P2P 消息：注入 skill 加载指令（链上通知不注入，避免误导）
  if (sessionMode === "sub" && !isChainNotify) {
    const skillDirective = `[系统指令] 这是一条任务协商P2P消息。回复前必须先加载 okx-agent-task skill，按 SKILL.md 判断角色（消息含[BUYER]→你是Provider→Read provider.md；含[PROVIDER]→你是Client→Read client.md），严格遵守角色文件中的消息格式和行为规则。不得使用markdown、emoji、代码块。\n\n`;
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
