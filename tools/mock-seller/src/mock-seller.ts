/**
 * TypeScript mock seller
 *
 * 每个 convId 独立一个 SellerSession，和买家侧 sub-session 架构对称。
 *
 * 用法:
 *   cd plugins/ws-channel
 *   npx ts-node --esm src/mock-seller.ts
 */
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";

// ── 常量 ─────────────────────────────────────────────────────────────────────
const SELLER_COMM_ADDR = "0xSeller000000000000000000000000000000001";
const SELLER_AGENT_ID  = "mock-seller-agent-001";
const WS_URL           = "ws://127.0.0.1:9000";
const API_BASE_URL     = "http://127.0.0.1:9001";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function formatMsg(jobId: string, convId: string, msgType: string, text: string): string {
  const sep = "-".repeat(40);
  return `jobId:  ${jobId}\n来自:   ${SELLER_AGENT_ID} [SELLER]\n类型:   ${msgType}\n会话:   ${convId}\n${sep}\n${text}`;
}

// ── SellerSession：每个 convId 独立的协商状态机 ──────────────────────────────
//
// step 0 → 收到 TASK_INQUIRE/REPLY → 询问任务详情  → step 1
// step 1 → 收到 REPLY             → 报价           → step 2
// step 2 → 收到 REPLY             → 确认支付方式    → step 3
// step 3 → 收到 REPLY             → 发 TASK_APPLY   → applied
// applied → 收到 TASK_ACCEPTED    → 延迟后发 TASK_DELIVER

type NegStep = 0 | 1 | 2 | 3;

class SellerSession {
  private step: NegStep = 0;
  private applied = false;
  private convId: string;
  private jobId: string;
  private reply: (payload: Partial<TaskPayload>) => void;

  constructor(convId: string, jobId: string, reply: (payload: Partial<TaskPayload>) => void) {
    this.convId = convId;
    this.jobId = jobId;
    this.reply = reply;
    console.log(`[seller][session] new  conv=${convId} jobId=${jobId}`);
  }

  async handle(envelope: WsEnvelope): Promise<void> {
    const type = envelope.payload.type;
    console.log(`[seller][session] recv conv=${this.convId} type=${type} step=${this.step}`);

    // Step 0: 首条消息 → 询问任务详情
    if (this.step === 0 && (type === "TASK_INQUIRE" || type === "REPLY")) {
      await sleep(1000);
      this.reply({
        type: "TASK_REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "TASK_REPLY", "你好！我对这个任务感兴趣，能介绍一下任务详情、验收标准和截止时间吗？"),
      });
      this.step = 1;
      return;
    }

    // Step 1: 收到详情 → 报价
    if (this.step === 1 && type === "REPLY") {
      await sleep(2000);
      this.reply({
        type: "TASK_REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "TASK_REPLY", "了解了任务详情。我的报价是 100 USDT，交付时间 48 小时，请问可以接受吗？"),
      });
      this.step = 2;
      return;
    }

    // Step 2: 价格确认 → 确认支付方式
    if (this.step === 2 && type === "REPLY") {
      await sleep(2000);
      this.reply({
        type: "TASK_REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "TASK_REPLY", "报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。"),
      });
      this.step = 3;
      return;
    }

    // Step 3: 最终确认 → 正式申请接单
    if (this.step === 3 && type === "REPLY" && !this.applied) {
      this.applied = true;
      await sleep(1000);
      this.reply({
        type: "TASK_APPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "TASK_APPLY", "我正式申请接单，报价 100 USDT，支付方式 non_escrow，交付时间 48 小时。"),
      });
      await callApplyApi(this.jobId).catch((e) =>
        console.error(`[seller][api] apply error:`, e),
      );
      return;
    }

    // TASK_ACCEPTED → 延迟后交付
    if (type === "TASK_ACCEPTED") {
      console.log(`[seller][session] task accepted, delivering in 5s...`);
      await sleep(5000);
      const deliverableUrl = `https://mock-deliverable.example.com/${this.jobId}.html`;
      this.reply({
        type: "TASK_DELIVER", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "TASK_DELIVER", "任务已完成，请买家验收。"),
        deliverableUrl,
      });
      await callSubmitApi(this.jobId, deliverableUrl).catch((e) =>
        console.error(`[seller][api] submit error:`, e),
      );
      return;
    }
  }
}

// ── mock-api 调用 ─────────────────────────────────────────────────────────────
async function callApplyApi(jobId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/apply`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ provider_address: SELLER_COMM_ADDR, price_usdt: 100 }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[seller][api] applied job=${jobId}`);
}

async function callSubmitApi(jobId: string, deliverableUrl: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/submit`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ provider_address: SELLER_COMM_ADDR, deliverable_url: deliverableUrl }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[seller][api] submitted job=${jobId}`);
}

// ── main ──────────────────────────────────────────────────────────────────────
async function main() {
  const client = new WsMockClient(WS_URL, SELLER_COMM_ADDR);

  await client.connectAndRegister();
  await client.registerIdentity("PROVIDER", SELLER_AGENT_ID, SELLER_COMM_ADDR);
  console.log(`✓ 身份已注册: role=PROVIDER agentId=${SELLER_AGENT_ID} commAddr=${SELLER_COMM_ADDR}`);
  console.log(`[seller] 无头模式运行中，等待消息...\n`);

  // convId → SellerSession
  const sessions = new Map<string, SellerSession>();

  client.start((envelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const { type } = payload;
    const jobId = String(payload.jobId ?? "");

    // 忽略链上系统通知和自己的回显
    if (type === "TASK_CONFIRMED") return;
    if (from === SELLER_COMM_ADDR) return;

    console.log(`[seller] ← conv=${convId.slice(-30)} from=${from.slice(0, 20)} type=${type}`);

    // 按 convId 路由到独立 session
    if (!sessions.has(convId)) {
      const reply = (p: Partial<TaskPayload>) => {
        console.log(`[seller] → conv=${convId.slice(-30)} type=${p.type}`);
        client.sendToConv(convId, p as TaskPayload);
      };
      sessions.set(convId, new SellerSession(convId, jobId, reply));
    }

    sessions.get(convId)!.handle(envelope).catch((err) =>
      console.error(`[seller][session] error:`, err),
    );
  });

  // keep alive
  await new Promise(() => {});
}

main().catch(console.error);
