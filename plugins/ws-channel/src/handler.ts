import type { ClawdbotConfig, PluginRuntime } from "openclaw/plugin-sdk";
import type { WsEnvelope } from "./ws-client.js";
import { getRuntime } from "./runtime.js";

const ACCEPT_KEYWORDS = ["接单", "我接了", "i'll take it", "accepted"];

function isAcceptMessage(envelope: WsEnvelope): boolean {
  if (envelope.payload.type === "TASK_ACCEPT") return true;
  const lower = String(envelope.payload.content ?? "").toLowerCase();
  return ACCEPT_KEYWORDS.some((kw) => lower.includes(kw));
}

export async function handleInboundMessage(params: {
  cfg: ClawdbotConfig;
  accountId: string;
  myAddr: string;
  systemPrompt?: string;
  envelope: WsEnvelope;
  reply: (text: string) => void;
}): Promise<void> {
  const { cfg, accountId, myAddr, systemPrompt, envelope, reply } = params;
  const core = getRuntime() as PluginRuntime & any;
  const log = core.log ?? console.log;

  log(`[ws-channel] conv:${envelope.conversation_id} from:${envelope.from} type:${envelope.payload.type}`);

  const rawBody =
    typeof envelope.payload.content === "string"
      ? envelope.payload.content
      : JSON.stringify(envelope.payload);

  // 路由：per-conversation session（用 conversation_id 作为 peer id）
  const route = core.channel.routing.resolveAgentRoute({
    cfg,
    channel: "ws-mock",
    accountId,
    peer: { kind: "direct", id: envelope.conversation_id },
  });

  // 检测接单消息：向 main session 推送通知
  if (isAcceptMessage(envelope)) {
    log(`[ws-channel] 检测到接单消息，向 main session 推送通知`);
    try {
      const notifyBody = `🔔 系统通知：卖家 ${envelope.from} 已接单 (task: ${envelope.payload.task_id ?? "?"}, conv: ${envelope.conversation_id.slice(0, 20)}...)。`;
      const notifyCtx = core.channel.reply.finalizeInboundContext({
        Body: notifyBody,
        RawBody: notifyBody,
        CommandBody: notifyBody,
        From: `ws-mock:system`,
        To: `ws-mock:${myAddr}`,
        SessionKey: route.mainSessionKey,
        AccountId: route.accountId,
        ChatType: "direct",
        SenderName: "system",
        SenderId: "system",
        Provider: "ws-mock",
        Surface: "ws-mock",
        MessageSid: `notify-${envelope.from}-${Date.now()}`,
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
      (core.error ?? console.error)(`[ws-channel] notify error: ${String(err)}`);
    }
  }

  // 格式化消息 body，包含 conversationId 供 AI 在 skill 调用时使用
  try {
    const envelopeOptions = core.channel.reply.resolveEnvelopeFormatOptions(cfg);
    const body = core.channel.reply.formatAgentEnvelope({
      channel: "WS Mock",
      from: envelope.from,
      timestamp: new Date(),
      envelope: envelopeOptions,
      body: `[会话: ${envelope.conversation_id}]\n来自: ${envelope.from}\n${rawBody}`,
    });

    const ctxPayload = core.channel.reply.finalizeInboundContext({
      Body: body,
      RawBody: rawBody,
      CommandBody: rawBody,
      From: `ws-mock:${envelope.from}`,
      To: `ws-mock:${myAddr}`,
      SessionKey: route.sessionKey,
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

    log(`[ws-channel] dispatch 完成 (replies=${replyCount})`);
  } catch (err) {
    (core.error ?? console.error)(`[ws-channel] dispatch error: ${String(err)}`);
  }
}
