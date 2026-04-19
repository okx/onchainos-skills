/**
 * buyer-session.ts — 买家协商核心逻辑（无头和 UI 共用）
 */
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";

// ── 常量 ─────────────────────────────────────────────────────────────────────
export const BUYER_COMM_ADDR = "0xBuyer000000000000000000000000000000001";
export const BUYER_AGENT_ID  = "mock-buyer-agent-001";
export const WS_URL          = "ws://127.0.0.1:9000";
export const API_BASE_URL    = "http://127.0.0.1:9001";

export const MOCK_TASK = {
  title: "开发一个 Python 脚本监控链上交易",
  description: "实时输出以太坊主网的大额交易，支持按金额过滤，有完整注释",
  budget: 100,
  qualityStandards: "代码有注释，支持以太坊主网，交付可运行脚本",
};

export const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export function formatMsg(jobId: string, convId: string, msgType: string, text: string): string {
  const sep = "-".repeat(40);
  return `jobId:  ${jobId}\n来自:   ${BUYER_AGENT_ID} [BUYER]\n类型:   ${msgType}\n会话:   ${convId}\n${sep}\n${text}`;
}


// ── mock-api 调用 ─────────────────────────────────────────────────────────────
export async function callAcceptApi(jobId: string, providerAgentId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/accept`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ provider_agent_id: providerAgentId }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[buyer][api] accepted job=${jobId} provider=${providerAgentId}`);
}

export async function callCompleteApi(jobId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/complete`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[buyer][api] completed job=${jobId}`);
}

// ── BuyerSession 核心状态机 ───────────────────────────────────────────────────
export class BuyerSession {
  step = 0;
  accepted = false;
  completed = false;
  readonly convId: string;
  readonly jobId: string;
  readonly sellerAgentId: string;
  readonly sellerCommAddr: string;
  private readonly reply: (payload: Partial<TaskPayload>) => void;
  private readonly onStateChange?: () => void;

  constructor(
    convId: string,
    jobId: string,
    sellerAgentId: string,
    sellerCommAddr: string,
    reply: (payload: Partial<TaskPayload>) => void,
    onStateChange?: () => void,
  ) {
    this.convId = convId;
    this.jobId = jobId;
    this.sellerAgentId = sellerAgentId;
    this.sellerCommAddr = sellerCommAddr;
    this.reply = reply;
    this.onStateChange = onStateChange;
    console.log(`[buyer][session] new  conv=${convId} jobId=${jobId} seller=${sellerAgentId}`);
  }

  async handle(envelope: WsEnvelope): Promise<void> {
    const payload = envelope.payload as Record<string, unknown>;
    const type = String(payload.type ?? "");
    console.log(`[buyer][session] recv conv=${this.convId} type=${type} step=${this.step}`);

    if (this.step === 0 && (type === "TASK_REPLY" || type === "REPLY")) {
      await sleep(1000);
      this.reply({
        type: "REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "REPLY",
          `任务标题：${MOCK_TASK.title}。\n描述：${MOCK_TASK.description}。\n预算：${MOCK_TASK.budget} USDT。\n验收标准：${MOCK_TASK.qualityStandards}。`),
      });
      this.step = 1; this.onStateChange?.(); return;
    }

    if (this.step === 1 && (type === "TASK_REPLY" || type === "REPLY")) {
      await sleep(1500);
      this.reply({
        type: "REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "REPLY",
          "好的，我接受你的报价 100 USDT，交付时间 48 小时，请继续。"),
      });
      this.step = 2; this.onStateChange?.(); return;
    }

    if (this.step === 2 && (type === "TASK_REPLY" || type === "REPLY")) {
      await sleep(1500);
      this.reply({
        type: "REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "REPLY",
          "确认，我接受报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。请正式提交申请接单。"),
      });
      this.step = 3; this.onStateChange?.(); return;
    }

    // 收到 TASK_APPLY / TASK_APPLIED → accept（不限 step，防重复）
    if ((type === "TASK_APPLY" || type === "TASK_APPLIED") && !this.accepted) {
      this.accepted = true;
      const agentId = String(payload.sellerAgentId ?? payload.providerAgentId ?? this.sellerAgentId);
      console.log(`[buyer][session] TASK_APPLY received, calling accept API seller=${agentId}...`);
      await callAcceptApi(this.jobId, agentId).catch((e) =>
        console.error(`[buyer][api] accept error:`, e));
      this.step = 4; this.onStateChange?.(); return;
    }

    // TASK_DELIVER / TASK_SUBMITTED → complete（只调一次）
    if ((type === "TASK_DELIVER" || type === "TASK_SUBMITTED") && !this.completed) {
      this.completed = true;
      const url = String(payload.deliverableUrl ?? "");
      console.log(`[buyer][session] deliverable received url=${url}, calling complete API...`);
      await sleep(1000);
      await callCompleteApi(this.jobId).catch((e) =>
        console.error(`[buyer][api] complete error:`, e));
      this.step = 6; this.onStateChange?.(); return;
    }
  }
}

// ── startNegotiation：查卖家、建 conv、发 TASK_INQUIRE ────────────────────────
export async function startNegotiation(
  client: WsMockClient,
  jobId: string,
  sessions: Map<string, BuyerSession>,
  onNewSession?: (session: BuyerSession) => void,
): Promise<BuyerSession | null> {
  let providers: unknown[] = [];
  for (let attempt = 0; attempt < 5; attempt++) {
    providers = await client.lookupRole("PROVIDER");
    if (providers.length > 0) break;
    console.log(`[buyer] no PROVIDER yet, retrying in 3s... (attempt ${attempt + 1}/5)`);
    await sleep(3000);
  }
  if (providers.length === 0) {
    console.error(`[buyer] no PROVIDER registered after retries, giving up for jobId=${jobId}`);
    return null;
  }

  const seller = providers[0] as { agent_id: string; comm_addr: string };
  const sellerAgentId = seller.agent_id ?? "unknown-seller";
  const sellerCommAddr = seller.comm_addr ?? "";
  const convId = `conv-${jobId}-${BUYER_AGENT_ID}-${sellerAgentId}`;
  console.log(`[buyer] starting negotiation conv=${convId} seller=${sellerAgentId}`);

  client.joinConversation(convId, [BUYER_COMM_ADDR, sellerCommAddr]);
  await sleep(300);

  const reply = (p: Partial<TaskPayload>) => {
    console.log(`[buyer] → conv=${convId.slice(-30)} type=${p.type}`);
    client.sendToConv(convId, p as TaskPayload);
  };

  const session = new BuyerSession(convId, jobId, sellerAgentId, sellerCommAddr, reply,
    () => onNewSession?.(session));
  sessions.set(convId, session);
  onNewSession?.(session);

  const inquireContent = formatMsg(jobId, convId, "TASK_INQUIRE",
    `你好，我有一个任务（jobId: ${jobId}）想请你来完成，请问你感兴趣吗？`);
  client.sendToConv(convId, {
    type: "TASK_INQUIRE", jobId,
    content: inquireContent,
  });
  console.log(`[buyer] TASK_INQUIRE sent → ${sellerAgentId}`);
  return session;
}
