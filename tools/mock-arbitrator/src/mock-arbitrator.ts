/**
 * TypeScript mock arbitrator (headless)
 *
 * 架构：每个 convId 一个 ArbSession（和系统通知 sub-session 对称）
 * 流程：收到 TASK_DISPUTED → 延迟 5s → 发 TASK_RESOLVE（在同一 convId 内回复）
 *
 * 用法:
 *   cd tools/mock-arbitrator
 *   npm install && npm start
 *
 *   VERDICT=seller npm start   # 裁定卖家胜（默认买家胜）
 */
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";

const ARB_COMM_ADDR = "0xArbitrator0000000000000000000000000001";
const ARB_AGENT_ID  = "mock-arbitrator-agent-001";
const WS_URL        = "ws://127.0.0.1:9000";
const API_BASE_URL  = "http://127.0.0.1:9001";

const DEFAULT_VERDICT: "buyer" | "seller" =
  process.env.VERDICT === "seller" ? "seller" : "buyer";

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ── ArbSession：一个 convId 对应一次仲裁 ─────────────────────────────────────
class ArbSession {
  private resolved = false;

  constructor(
    private convId: string,
    private jobId: string,
    private verdict: "buyer" | "seller",
    private reply: (payload: Partial<TaskPayload>) => void,
  ) {
    console.log(`[arb][session] new  conv=${convId.slice(-40)} jobId=${jobId} verdict=${verdict}`);
  }

  async handle(envelope: WsEnvelope): Promise<void> {
    if (this.resolved) return;
    const { type } = envelope.payload;

    if (type === "TASK_DISPUTED") {
      this.resolved = true;
      // 延迟 5s 模拟审查
      await sleep(5000);
      const reason = this.verdict === "buyer"
        ? "交付物未完全满足验收标准，支持买家拒绝验收，资金退还买家。"
        : "交付物符合验收标准，买家拒绝理由不充分，资金释放给卖家。";

      await callResolveApi(this.jobId, this.verdict, reason).catch((e) =>
        console.error(`[arb][api] resolve error:`, e),
      );
    }
  }
}

async function callResolveApi(jobId: string, winner: string, reason: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/resolve`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ winner, reason }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[arb][api] resolved job=${jobId} winner=${winner}`);
}

// ── main ──────────────────────────────────────────────────────────────────────
async function main() {
  const client = new WsMockClient(WS_URL, ARB_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("EVALUATOR", ARB_AGENT_ID, ARB_COMM_ADDR);
  console.log(`✓ 身份已注册: role=EVALUATOR agentId=${ARB_AGENT_ID}`);
  console.log(`[arb] 无头模式，默认裁决=${DEFAULT_VERDICT}，等待 TASK_DISPUTED...\n`);

  const sessions = new Map<string, ArbSession>();

  client.start((envelope: WsEnvelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const jobId = String(payload.jobId ?? "");

    if (from === ARB_COMM_ADDR) return;
    console.log(`[arb] ← conv=${convId.slice(-30)} from=${from.slice(0, 20)} type=${payload.type}`);

    if (!sessions.has(convId)) {
      const reply = (p: Partial<TaskPayload>) => {
        console.log(`[arb] → conv=${convId.slice(-30)} type=${p.type}`);
        client.sendToConv(convId, p as TaskPayload);
      };
      sessions.set(convId, new ArbSession(convId, jobId, DEFAULT_VERDICT, reply));
    }

    sessions.get(convId)!.handle(envelope).catch((err) =>
      console.error(`[arb][session] error:`, err),
    );
  });

  await new Promise(() => {});
}

main().catch(console.error);
