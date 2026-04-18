/**
 * TypeScript mock buyer (headless)
 *
 * 每个 jobId 独立一个 BuyerSession，和卖家侧 SellerSession 对称。
 *
 * 流程:
 *   收到 TASK_CONFIRMED → 查询卖家 → 创建会话 → 发 TASK_INQUIRE
 *   收到 TASK_REPLY (3 轮) → 自动回复协商
 *   收到 TASK_APPLY → 调用 accept API
 *   收到 TASK_DELIVER → 调用 complete API
 *
 * 用法:
 *   cd tools/mock-buyer
 *   npm install && npm start
 *
 *   # 或者直接指定 jobId，跳过等待 TASK_CONFIRMED
 *   JOB_ID=0x3e9 npm start
 */
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";

// ── 常量 ─────────────────────────────────────────────────────────────────────
const BUYER_COMM_ADDR  = "0xBuyer000000000000000000000000000000001";
const BUYER_AGENT_ID   = "mock-buyer-agent-001";
const WS_URL           = "ws://127.0.0.1:9000";
const API_BASE_URL     = "http://127.0.0.1:9001";

const MOCK_TASK = {
  title: "开发一个 Python 脚本监控链上交易",
  description: "实时输出以太坊主网的大额交易，支持按金额过滤，有完整注释",
  budget: 100,
  qualityStandards: "代码有注释，支持以太坊主网，交付可运行脚本",
};

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function formatMsg(jobId: string, convId: string, msgType: string, text: string): string {
  const sep = "-".repeat(40);
  return `jobId:  ${jobId}\n来自:   ${BUYER_AGENT_ID} [BUYER]\n类型:   ${msgType}\n会话:   ${convId}\n${sep}\n${text}`;
}

// ── BuyerSession：每个 convId 独立的协商状态机 ────────────────────────────────
//
// step 0 → 收到 TASK_REPLY (卖家询问详情) → 发送任务描述    → step 1
// step 1 → 收到 TASK_REPLY (卖家报价)      → 接受价格        → step 2
// step 2 → 收到 TASK_REPLY (卖家确认支付)   → 请卖家正式申请  → step 3
// step 3 → 收到 TASK_APPLY               → 调用 accept API  → accepted
// accepted → 收到 TASK_DELIVER           → 调用 complete API → done

type BuyStep = 0 | 1 | 2 | 3;

class BuyerSession {
  private step: BuyStep = 0;
  private accepted = false;
  private completed = false;
  private convId: string;
  private jobId: string;
  private reply: (payload: Partial<TaskPayload>) => void;

  constructor(convId: string, jobId: string, reply: (payload: Partial<TaskPayload>) => void) {
    this.convId = convId;
    this.jobId = jobId;
    this.reply = reply;
    console.log(`[buyer][session] new  conv=${convId} jobId=${jobId}`);
  }

  async handle(envelope: WsEnvelope): Promise<void> {
    const type = envelope.payload.type;
    console.log(`[buyer][session] recv conv=${this.convId} type=${type} step=${this.step}`);

    // Step 0: 卖家询问详情 → 发送任务描述
    if (this.step === 0 && type === "TASK_REPLY") {
      await sleep(1000);
      this.reply({
        type: "REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "REPLY",
          `任务标题：${MOCK_TASK.title}。\n描述：${MOCK_TASK.description}。\n预算：${MOCK_TASK.budget} USDT。\n验收标准：${MOCK_TASK.qualityStandards}。`),
      });
      this.step = 1;
      return;
    }

    // Step 1: 卖家报价 → 接受
    if (this.step === 1 && type === "TASK_REPLY") {
      await sleep(1500);
      this.reply({
        type: "REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "REPLY",
          "好的，我接受你的报价 100 USDT，交付时间 48 小时，请继续。"),
      });
      this.step = 2;
      return;
    }

    // Step 2: 卖家确认支付方式 → 请正式申请
    if (this.step === 2 && type === "TASK_REPLY") {
      await sleep(1500);
      this.reply({
        type: "REPLY", jobId: this.jobId,
        content: formatMsg(this.jobId, this.convId, "REPLY",
          "确认，我接受报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。请正式提交申请接单。"),
      });
      this.step = 3;
      return;
    }

    // Step 3: 收到 TASK_APPLY → 调用 accept API
    if (this.step === 3 && type === "TASK_APPLY" && !this.accepted) {
      this.accepted = true;
      console.log(`[buyer][session] TASK_APPLY received, calling accept API...`);
      await callAcceptApi(this.jobId).catch((e) =>
        console.error(`[buyer][api] accept error:`, e),
      );
      return;
    }

    // TASK_DELIVER → 调用 complete API（只调一次）
    if ((type === "TASK_DELIVER" || type === "TASK_SUBMITTED") && !this.completed) {
      this.completed = true;
      const url = String(envelope.payload.deliverableUrl ?? "");
      console.log(`[buyer][session] deliverable received url=${url}, calling complete API...`);
      await sleep(1000);
      await callCompleteApi(this.jobId).catch((e) =>
        console.error(`[buyer][api] complete error:`, e),
      );
      return;
    }
  }
}

// ── mock-api 调用 ─────────────────────────────────────────────────────────────
async function callAcceptApi(jobId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/accept`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      provider_address: "0xSeller000000000000000000000000000000001",
      provider_agent_id: "mock-seller-agent-001",
    }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[buyer][api] accepted job=${jobId}`);
}

async function callCompleteApi(jobId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/complete`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[buyer][api] completed job=${jobId}`);
}

// ── 启动对话（买家主动联系卖家）─────────────────────────────────────────────────
async function startNegotiation(
  client: WsMockClient,
  jobId: string,
  sessions: Map<string, BuyerSession>,
): Promise<void> {
  // 查找在线卖家
  const providers = await client.lookupRole("PROVIDER");
  if (providers.length === 0) {
    console.error(`[buyer] no PROVIDER registered, waiting...`);
    return;
  }
  const seller = providers[0] as unknown as { agent_id: string; comm_addr: string };
  const sellerAgentId = seller.agent_id ?? "unknown-seller";
  const sellerCommAddr = seller.comm_addr;

  const convId = `conv-${jobId}-${BUYER_AGENT_ID}-${sellerAgentId}`;
  console.log(`[buyer] starting negotiation conv=${convId} seller=${sellerAgentId}`);

  // 加入会话
  client.joinConversation(convId, [BUYER_COMM_ADDR, sellerCommAddr]);
  await sleep(300);

  // 注册 session
  const reply = (p: Partial<TaskPayload>) => {
    console.log(`[buyer] → conv=${convId.slice(-30)} type=${p.type}`);
    client.sendToConv(convId, p as TaskPayload);
  };
  sessions.set(convId, new BuyerSession(convId, jobId, reply));

  // 发送 TASK_INQUIRE
  client.sendToConv(convId, {
    type: "TASK_INQUIRE",
    jobId,
    content: formatMsg(jobId, convId, "TASK_INQUIRE",
      `你好，我有一个任务（jobId: ${jobId}）想请你来完成，请问你感兴趣吗？`),
  });
  console.log(`[buyer] TASK_INQUIRE sent → ${sellerAgentId}`);
}

// ── main ──────────────────────────────────────────────────────────────────────
async function main() {
  const client = new WsMockClient(WS_URL, BUYER_COMM_ADDR);

  await client.connectAndRegister();
  await client.registerIdentity("REQUESTER", BUYER_AGENT_ID, BUYER_COMM_ADDR);
  console.log(`✓ 身份已注册: role=REQUESTER agentId=${BUYER_AGENT_ID} commAddr=${BUYER_COMM_ADDR}`);
  console.log(`[buyer] 无头模式运行中，等待 TASK_CONFIRMED...\n`);

  // convId → BuyerSession
  const sessions = new Map<string, BuyerSession>();

  // 如果直接指定 jobId（绕过等待 TASK_CONFIRMED）
  const envJobId = process.env.JOB_ID;
  if (envJobId) {
    console.log(`[buyer] JOB_ID=${envJobId}，直接启动协商...`);
    await startNegotiation(client, envJobId, sessions);
  }

  client.start((envelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const { type } = payload;
    const jobId = String(payload.jobId ?? "");

    // 忽略自己的回显
    if (from === BUYER_COMM_ADDR) return;

    console.log(`[buyer] ← conv=${convId.slice(-30)} from=${from.slice(0, 20)} type=${type}`);

    // TASK_CONFIRMED: 链上确认，开始联系卖家
    if (type === "TASK_CONFIRMED" && jobId) {
      console.log(`[buyer] TASK_CONFIRMED jobId=${jobId}，启动协商...`);
      startNegotiation(client, jobId, sessions).catch(console.error);
      return;
    }

    // 按 convId 路由到独立 session
    const session = sessions.get(convId);
    if (!session) {
      console.log(`[buyer] unknown conv=${convId.slice(-20)}, ignoring type=${type}`);
      return;
    }

    session.handle(envelope).catch((err) =>
      console.error(`[buyer][session] error:`, err),
    );
  });

  // keep alive
  await new Promise(() => {});
}

main().catch(console.error);
