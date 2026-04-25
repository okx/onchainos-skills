/**
 * TypeScript mock evaluator (headless)
 *
 * 事件驱动仲裁流程(事件名对齐 Lark 设计文档 event 枚举):
 *   TASK_DISPUTED          → 记录 verdict,等被选为陪审
 *   evaluator_selected     → 调 /vote/commit (VotersSelected 上链,CommitPhase 已开)
 *   vote_committed         → (日志)commit tx 已回执
 *   reveal_started         → 调 /vote/reveal (RevealStarted 上链)
 *   vote_revealed          → (日志)reveal tx 已回执
 *   dispute_resolved       → (日志)最终裁决广播
 *   round_failed           → (日志)本轮无效,等下一轮
 *   slashed                → (日志)stake 被罚没
 *   reward_claimed         → (日志)奖励已入账
 *
 * Vote 语义:1=Approve(Provider/seller wins), 2=Reject(Client/buyer wins)。
 *
 * 用法:
 *   cd tools/mock-evaluator
 *   npm install && npm start
 *
 *   VERDICT=seller npm start   # 裁定卖家胜(默认买家胜)
 */
import { WsMockClient, WsEnvelope } from "./ws-client.js";

const EVAL_COMM_ADDR = "0xEvaluator00000000000000000000000000001";
const EVAL_AGENT_ID  = "mock-evaluator-agent-001";
const WS_URL         = "ws://127.0.0.1:9000";
const API_BASE_URL   = "http://127.0.0.1:9001";

const DEFAULT_VERDICT: "buyer" | "seller" =
  process.env.VERDICT === "seller" ? "seller" : "buyer";

// ── 按 jobId 跟踪仲裁进度 ────────────────────────────────────────────────────
interface EvalState {
  jobId: string;
  disputeId: string;
  verdict: "buyer" | "seller";
  reason: string;
  phase: "prep" | "committed" | "revealed";
}
const states = new Map<string, EvalState>();

function buildReason(verdict: "buyer" | "seller"): string {
  return verdict === "buyer"
    ? "交付物未完全满足验收标准，支持买家拒绝验收，资金退还买家。"
    : "交付物符合验收标准，买家拒绝理由不充分，资金释放给卖家。";
}

async function commitIfPrep(jobId: string): Promise<void> {
  const s = states.get(jobId);
  if (!s || s.phase !== "prep") return;
  const vote: 1 | 2 = s.verdict === "seller" ? 1 : 2;
  try {
    await callCommitVote(jobId, vote, s.reason);
    s.phase = "committed";
  } catch (e) {
    console.error(`[eval][api] commit error (job=${jobId}):`, e);
  }
}

async function revealIfCommitted(jobId: string): Promise<void> {
  const s = states.get(jobId);
  if (!s || s.phase !== "committed") return;
  try {
    await callRevealVote(jobId);
    s.phase = "revealed";
  } catch (e) {
    console.error(`[eval][api] reveal error (job=${jobId}):`, e);
  }
}

async function callCommitVote(jobId: string, vote: 1 | 2, reason: string): Promise<void> {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/vote/commit`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ vote, reason, voter: EVAL_COMM_ADDR }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[eval][api] committed job=${jobId} vote=${vote}`);
}

async function callRevealVote(jobId: string): Promise<void> {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/vote/reveal`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ voter: EVAL_COMM_ADDR }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[eval][api] revealed job=${jobId}`);
}

// ── main ──────────────────────────────────────────────────────────────────────
async function main() {
  const client = new WsMockClient(WS_URL, EVAL_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("EVALUATOR", EVAL_AGENT_ID, EVAL_COMM_ADDR);
  console.log(`✓ 身份已注册: role=EVALUATOR agentId=${EVAL_AGENT_ID}`);
  console.log(`[eval] 无头模式,默认裁决=${DEFAULT_VERDICT},等待 evaluator_selected...\n`);

  client.start((envelope: WsEnvelope) => {
    const { from, payload } = envelope;
    if (from === EVAL_COMM_ADDR) return;
    const type = payload.type;
    const jobId = String(payload.jobId ?? "");
    const disputeId = String(payload.disputeId ?? "");
    console.log(`[eval] ← from=${from.slice(0, 20)} type=${type} job=${jobId}`);

    if (!jobId) return;

    switch (type) {
      case "TASK_DISPUTED":
      case "task_disputed": {
        if (!states.has(jobId)) {
          const verdict = DEFAULT_VERDICT;
          states.set(jobId, {
            jobId, disputeId, verdict,
            reason: buildReason(verdict),
            phase: "prep",
          });
          console.log(`[eval] recorded dispute job=${jobId} verdict=${verdict}; 等 evaluator_selected...`);
        }
        return;
      }
      case "evaluator_selected": {
        commitIfPrep(jobId);
        return;
      }
      case "reveal_started": {
        revealIfCommitted(jobId);
        return;
      }
      case "vote_committed":
      case "vote_revealed":
      case "dispute_resolved":
      case "round_failed":
      case "slashed":
      case "reward_claimed":
        // 回执/结算/罚没:仅日志(上面已打印)
        return;
      default:
        return;
    }
  });

  await new Promise(() => {});
}

main().catch(console.error);
