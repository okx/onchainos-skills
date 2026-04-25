/**
 * Mock API Server — TypeScript port of mock_api.rs
 * Port: 9001  Dashboard: http://127.0.0.1:9001
 */
import http   from "node:http";
import https  from "node:https";
import fs     from "node:fs";
import path   from "node:path";
import crypto from "node:crypto";
import { WebSocket } from "ws";

const API_PORT  = 9001;
const WS_URL    = "ws://127.0.0.1:9000";
const CHAIN_ADDR = "0xMockChain000000000000000000000001";

// ── Task status ───────────────────────────────────────────────────────────────
const S_OPEN = 0, S_ACCEPTED = 1, S_SUBMITTED = 2, S_REFUSED = 3;
const S_DISPUTED = 4, S_COMPLETE = 5, S_CLOSE = 7;
const STATUS_STR: Record<number, string> = {
  [-1]:"init", 0:"open", 1:"accepted", 2:"submitted", 3:"refused",
  4:"disputed", 5:"complete", 6:"rejected", 7:"close", 8:"expired",
};

// ── Data model ────────────────────────────────────────────────────────────────
interface Task {
  jobId: string; title: string; description: string; descriptionSummary: string;
  tokenAddress: string; tokenAmount: string;
  paymentType: number | null; openType: number;
  status: number; statusStr: string; chainId: number;
  minCreditScore: number | null; designatedProvider: string | null;
  buyerAgentAddress: string; buyerAgentId: string;
  providerAgentAddress: string | null; providerAgentId: string | null;
  groupId: string | null; evaluatorAddress: string | null;
  expireConfig: unknown; createTime: string; updateTime: string;
}
interface ProviderConfirm {
  providerAddress: string; providerAgentId: string;
  tokenAddress: string; tokenAmount: string;
}
interface DisputeEvidence {
  from: "client" | "provider"; summary: string; url?: string; level: "S"|"A"|"B"|"C"|"D";
}
interface DisputeVote { side: 1 | 2; reason: string; voter: string; at: string; }
interface VoterCommit {
  vote: 1 | 2; salt: string; reason: string;
  committedAt: string; revealedAt?: string;
}
interface Dispute {
  disputeId: string; jobId: string; round: number;
  clientReason: string; providerReason: string;
  qualityStandards: string; deliverableUrl: string;
  evidences: DisputeEvidence[];
  voterCommits: Record<string, VoterCommit>;
  votes: DisputeVote[];
  verdict: "client" | "provider" | null;
  createTime: string;
  commitPhaseStartedAt: string | null;  // evaluator_selected 触发时写入(VotersSelected 上链,CommitPhase 开始)
  resolvedAt: string | null;
}

const tasks    = new Map<string, Task>();
const confirms = new Map<string, ProviderConfirm[]>();
const disputes = new Map<string, Dispute>();

// ── Persistence ───────────────────────────────────────────────────────────────
const PERSIST_PATH = process.env.MOCK_API_DB ??
  path.join(path.dirname(new URL(import.meta.url).pathname), "mock-tasks.json");

// ── Static data fixtures (identity APIs) ─────────────────────────────────────
// 请求时重新读盘，支持热改 JSON 不用重启。放在 ../data/ 便于单独维护。
const DATA_DIR = process.env.MOCK_DATA_DIR ??
  path.resolve(path.dirname(new URL(import.meta.url).pathname), "..", "data");
function loadJsonFixture<T>(filename: string, fallback: T): T {
  try {
    const fp = path.join(DATA_DIR, filename);
    return JSON.parse(fs.readFileSync(fp, "utf8")) as T;
  } catch (e) {
    console.error(`[mock-api] 读取 ${filename} 失败:`, (e as Error).message);
    return fallback;
  }
}

// ── Upstream proxy（没命中 mock 路由的请求透传给真实后端）─────────────────────
// 通过 MOCK_PROXY_UPSTREAM 环境变量覆盖。默认打到 forked-walletmain test env。
// 适用于：auth/login, auth/refresh, wallet/balance 等"真实就好"的接口。
// 设成空字符串（MOCK_PROXY_UPSTREAM=）就退回到纯 mock（未匹配 → 404）。
const UPSTREAM_URL = process.env.MOCK_PROXY_UPSTREAM ??
  "http://okx-defi-walletmain-api.forked-walletmain-swim.swim.env";

function proxyToUpstream(
  req: http.IncomingMessage,
  res: http.ServerResponse,
  originalPath: string,
  urlSearch: string,
): void {
  if (!UPSTREAM_URL) {
    res.writeHead(404); res.end("not found"); return;
  }
  let up: URL;
  try { up = new URL(UPSTREAM_URL); }
  catch {
    console.error(`[proxy] invalid MOCK_PROXY_UPSTREAM: ${UPSTREAM_URL}`);
    res.writeHead(502); res.end("bad upstream config"); return;
  }
  const lib = up.protocol === "https:" ? https : http;
  const forwardHeaders = { ...req.headers, host: up.host };
  // Host 要改成 upstream 的，不然后端可能按 127.0.0.1 路由/鉴权
  const opts: http.RequestOptions = {
    protocol: up.protocol,
    hostname: up.hostname,
    port: up.port || (up.protocol === "https:" ? 443 : 80),
    method: req.method,
    path: originalPath + (urlSearch || ""),
    headers: forwardHeaders,
  };
  console.log(`[proxy] ${req.method} ${originalPath}${urlSearch || ""} → ${up.host}`);
  const upReq = lib.request(opts, (upRes) => {
    res.writeHead(upRes.statusCode ?? 502, upRes.headers);
    upRes.pipe(res);
  });
  upReq.on("error", (e: Error) => {
    console.error(`[proxy] upstream error: ${e.message}`);
    if (!res.headersSent) {
      res.writeHead(502, { "Content-Type": "application/json" });
    }
    res.end(JSON.stringify({ code: -1, msg: `upstream unreachable: ${e.message}` }));
  });
  const raw = (req as any)._rawBody as Buffer | undefined;
  if (raw && raw.length > 0) {
    upReq.end(raw);
  } else if (req.method === "POST" || req.method === "PUT") {
    upReq.end(); // 空 body 也要显式 end
  } else {
    upReq.end();
  }
}

function saveTasks() {
  try {
    const obj: Record<string, Task> = {};
    for (const [k, v] of tasks) obj[k] = v;
    fs.writeFileSync(PERSIST_PATH, JSON.stringify(obj, null, 2));
  } catch (e) { console.error("[mock-api] save error:", e); }
}
function loadTasks() {
  try {
    const raw = fs.readFileSync(PERSIST_PATH, "utf8");
    const obj = JSON.parse(raw) as Record<string, Task>;
    for (const [k, v] of Object.entries(obj)) tasks.set(k, v);
    console.log(`[mock-api] loaded ${tasks.size} task(s) from ${PERSIST_PATH}`);
  } catch { /* first run */ }
  // 从已有 jobId 里找最大的十进制 ID（仅 10 进制计数）。
  // 旧的 0x 前缀 jobId 继续保留可查询，但不参与计数。
  for (const k of tasks.keys()) {
    if (k.startsWith("0x")) continue;
    const n = parseInt(k, 10) || 0;
    if (n > jobCounter) jobCounter = n;
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────
let jobCounter = 100;                              // 起始；下一个任务会从 101 开始
const genJobId   = () => String(++jobCounter);     // "123" 纯十进制
const nowIso     = () => new Date().toISOString();
const mockUop    = () => `0x${Date.now().toString(16).padStart(64, "0")}`;
const ok         = (data: unknown) => ({ code: 0, data });
const errRes     = (code: number, msg: string) => ({ code, msg, data: null });
const setStatus  = (t: Task, s: number) => { t.status = s; t.statusStr = STATUS_STR[s] ?? "unknown"; t.updateTime = nowIso(); };

/** Mock uopData structure matching real wallet-service response */
function mockUopData(extraFields: Record<string, unknown> = {}): Record<string, unknown> {
  const uopHash = mockUop();
  return {
    unsignedTxHash: uopHash,
    unsignedTx: "0x" + "00".repeat(32),
    uopHash,
    hash: uopHash,
    authHashFor7702: "",
    executeErrorMsg: "",
    executeResult: true,
    signType: "eip1559Tx",
    encoding: "hex",
    extraData: {
      nonce: Math.floor(Math.random() * 1000),
      tokenAddress: null,
      coinAmount: "0",
      toAdr: "0x97693439ea2f0ecdeb9135881e49f354656a911c",
      serviceCharge: "0",
      gasPrice: 66147514,
      gasLimit: 44991,
      priorityFee: "66147514",
      inputData: "0x",
      signType: "eip1559Tx",
    },
    ...extraFields,
  };
}

/** Map /priapi/v1/aieco/... → /api/v1/... so CLI paths match mock routes */
function normalizePath(p: string): string {
  return p.replace(/^\/priapi\/v1\/aieco/, "/api/v1");
}

const sleep = (ms: number) => new Promise<void>(r => setTimeout(r, ms));

// ── Event logs (API requests + WS notifications) ────────────────────────────
interface EventLog {
  ts: string;
  kind: "api" | "ws";
  method?: string;
  path?: string;
  status?: number;
  jobId?: string;
  agentId?: string;
  wsType?: string;
  convId?: string;
  detail?: string;
  reqBody?: unknown;
  resBody?: unknown;
  wsPayload?: unknown;
}
const eventLogs: EventLog[] = [];
const MAX_LOGS = 200;
function pushLog(entry: EventLog) {
  eventLogs.unshift(entry);
  if (eventLogs.length > MAX_LOGS) eventLogs.length = MAX_LOGS;
}
function logApi(method: string, path: string, status: number, jobId?: string, detail?: string, reqBody?: unknown, resBody?: unknown, agentId?: string) {
  pushLog({ ts: new Date().toISOString(), kind: "api", method, path, status, jobId, detail, reqBody, resBody, agentId });
}
function logWs(wsType: string, jobId: string, convId: string, detail?: string, wsPayload?: unknown) {
  pushLog({ ts: new Date().toISOString(), kind: "ws", wsType, jobId, convId, detail, wsPayload });
}

/** 通过 WS LookupRole 查找指定角色的所有 comm_addr */
async function lookupRoleAddrs(role: string): Promise<string[]> {
  return new Promise((resolve) => {
    const ws = new WebSocket(WS_URL);
    const timer = setTimeout(() => { ws.terminate(); resolve([]); }, 3000);
    ws.once("open", () => ws.send(JSON.stringify({ action: "LookupRole", role })));
    ws.on("message", (raw) => {
      const msg = JSON.parse(raw.toString()) as Record<string, unknown>;
      if (msg.type === "identity_lookup") {
        clearTimeout(timer);
        const agents = msg.agents as Array<{ comm_addr?: string }> | null;
        ws.close();
        resolve(agents?.map(i => i.comm_addr).filter(Boolean) as string[] ?? []);
      }
    });
    ws.once("error", () => { clearTimeout(timer); resolve([]); });
  });
}

/** 通过 WS LookupAddr 查找 agentId 对应的 comm_addr */
async function lookupCommAddr(agentId: string): Promise<string | null> {
  return new Promise((resolve) => {
    const ws = new WebSocket(WS_URL);
    const timer = setTimeout(() => { ws.terminate(); resolve(null); }, 3000);
    ws.once("open", () => ws.send(JSON.stringify({ action: "LookupAddr", addr: agentId })));
    ws.on("message", (raw) => {
      const msg = JSON.parse(raw.toString()) as Record<string, unknown>;
      if (msg.type === "addr_lookup") {
        clearTimeout(timer);
        const identity = msg.identity as { comm_addr?: string } | null;
        ws.close();
        resolve(identity?.comm_addr ?? null);
      }
    });
    ws.once("error", () => { clearTimeout(timer); resolve(null); });
  });
}

// ── WS notification helper ───────────────────────────────────────────────────
async function wsNotify(convId: string, participants: string[], payload: Record<string, unknown>): Promise<void> {
  const wsType = String(payload.type ?? "?");
  const jobId = String(payload.jobId ?? "?");
  logWs(wsType, jobId, convId, `→ ${participants.filter(p => p !== CHAIN_ADDR).join(", ")}`, payload);
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(WS_URL);
    const timer = setTimeout(() => { ws.terminate(); reject(new Error("wsNotify timeout")); }, 8000);
    ws.once("open", () => ws.send(JSON.stringify({ action: "Register", addr: CHAIN_ADDR })));
    let joined = false;
    ws.on("message", (raw) => {
      const msg = JSON.parse(raw.toString()) as Record<string, unknown>;
      if (!joined && msg.type === "registered") {
        joined = true;
        ws.send(JSON.stringify({ action: "JoinConversation", conversation_id: convId, participants }));
        setTimeout(() => {
          ws.send(JSON.stringify({ action: "Send", conversation_id: convId, payload }));
          setTimeout(() => { clearTimeout(timer); ws.close(); resolve(); }, 200);
        }, 100);
      }
    });
    ws.once("error", (err) => { clearTimeout(timer); reject(err); });
  });
}

// ── Notification senders ─────────────────────────────────────────────────────
async function notifyConfirmed(jobId: string, buyerCommAddr: string) {
  const convId = `conv-task-confirmed-${jobId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr], {
    type: "job_created", jobId,
    content: `系统通知：任务 ${jobId} 已上链确认，状态变为 open。`,
  });
}

async function notifyApplied(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                              sellerAgentId: string, sellerCommAddr: string, tokenAmount: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "provider_applied", jobId, sellerAgentId, tokenAmount,
    content: `系统通知：卖家 ${sellerAgentId} 已申请接单（provider_applied），报价 ${tokenAmount} USDT，jobId=${jobId}。`,
  });
}

async function notifyAccepted(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                               sellerCommAddr: string, sellerAgentId: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "job_accepted", jobId, sellerAgentId,
    content: `系统通知：任务 ${jobId} 已接单确认（job_accepted），卖家 ${sellerAgentId}，资金已进入托管。`,
  });
}

async function notifySubmitted(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                                sellerAgentId: string, sellerCommAddr: string, deliverable: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "job_submitted", jobId, deliverable,
    content: `系统通知：任务 ${jobId} 交付物已上链（job_submitted），交付物：${deliverable}。`,
  });
}

async function notifyRefused(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                              sellerCommAddr: string, sellerAgentId: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "job_refused", jobId, buyerAgentId,
    content: `系统通知：买家拒绝了交付物（job_refused），jobId=${jobId}。卖家可在 24 小时内发起仲裁，否则资金退还买家。`,
  });
}

// 查询 ws-mock identity registry 里所有 EVALUATOR 角色的 comm_addr
async function lookupEvaluators(): Promise<Array<{ agent_id: string; comm_addr: string }>> {
  return new Promise((resolve) => {
    const ws = new WebSocket(WS_URL);
    const timer = setTimeout(() => { ws.terminate(); resolve([]); }, 3000);
    ws.once("open", () => ws.send(JSON.stringify({ action: "Register", addr: `${CHAIN_ADDR}-lookup-${Date.now()}` })));
    ws.on("message", (raw) => {
      const msg = JSON.parse(raw.toString()) as Record<string, unknown>;
      if (msg.type === "registered") {
        ws.send(JSON.stringify({ action: "LookupRole", role: "EVALUATOR" }));
      } else if (msg.type === "identity_lookup") {
        const agents = (msg.agents as Array<{ agent_id: string; comm_addr: string }>) ?? [];
        clearTimeout(timer); ws.close(); resolve(agents);
      }
    });
    ws.once("error", () => { clearTimeout(timer); resolve([]); });
  });
}

// Commit/Reveal tx 回执与窗口事件。事件名对齐 Lark 设计文档 event 枚举：
// VotersSelected 上链 → evaluator_selected（evaluator 被选中，CommitPhase 已开）
// commit tx 上链 → vote_committed；RevealStarted 上链 → reveal_started
// reveal tx 上链 → vote_revealed；DisputeSettled 上链 → dispute_resolved
// DisputeInvalidated → round_failed；VoterStaking.Slashed → slashed
// claimRewards tx 上链 → reward_claimed
const EVALUATOR_SELECTED_DELAY_MS = Number(process.env.MOCK_EVALUATOR_SELECTED_MS ?? 3000);
const REVEAL_WINDOW_DELAY_MS      = Number(process.env.MOCK_REVEAL_WINDOW_MS      ?? 3000);

// 所有 evaluator 生命周期事件共用同一个 dispute sub session conv（= notifyDisputed 用的 convId）。
// evaluator_selected 激活 sub session 后，后续 reveal_started / dispute_resolved / reward_claimed
// 自动复用同一 sub，在里面拉 context、跑 CLI、notify_main → 用户只在主 session 看到干净的最终通知。
const MOCK_EVAL_AGENT_ID = "mock-evaluator-agent-001";
function disputeConvId(t: Task): string {
  const sellerAgentId = t.providerAgentId ?? "mock-seller-agent-001";
  return `conv-arb-${t.jobId}-${t.buyerAgentId}-${sellerAgentId}-${MOCK_EVAL_AGENT_ID}`;
}

async function notifyEvaluatorSelected(t: Task, disputeId: string, evaluatorAddrs: string[]) {
  const convId = disputeConvId(t);
  const participants = Array.from(new Set([CHAIN_ADDR, ...evaluatorAddrs]));
  await wsNotify(convId, participants, {
    type: "evaluator_selected", jobId: t.jobId, disputeId,
    content: `⚖️ 你被选为本轮陪审 (disputeId=${disputeId})。CommitPhase 已开，请查证据 + commit vote。`,
  }).catch(e => console.error("[mock-api] evaluator_selected notify error:", e));
}

async function notifyVoteCommitted(jobId: string, disputeId: string, voter: string) {
  const convId = `conv-vote-committed-${jobId}-${voter}`;
  await wsNotify(convId, [CHAIN_ADDR, voter], {
    type: "vote_committed", jobId, disputeId, voter, status: "success",
    content: `📝 投票承诺已上链 (disputeId=${disputeId})。等待 reveal 窗口开启。`,
  }).catch(e => console.error("[mock-api] vote_committed notify error:", e));
}

async function notifyRevealStarted(t: Task, disputeId: string, evaluatorAddrs: string[]) {
  const convId = disputeConvId(t);
  const participants = Array.from(new Set([CHAIN_ADDR, ...evaluatorAddrs]));
  await wsNotify(convId, participants, {
    type: "reveal_started", jobId: t.jobId, disputeId,
    content: `🔓 Reveal 窗口开启 (disputeId=${disputeId})。投票者可 reveal。`,
  }).catch(e => console.error("[mock-api] reveal_started notify error:", e));
}

async function notifyVoteRevealed(jobId: string, disputeId: string, voter: string) {
  const convId = `conv-vote-revealed-${jobId}-${voter}`;
  await wsNotify(convId, [CHAIN_ADDR, voter], {
    type: "vote_revealed", jobId, disputeId, voter, status: "success",
    content: `✅ 投票披露已上链 (disputeId=${disputeId})。`,
  }).catch(e => console.error("[mock-api] vote_revealed notify error:", e));
}

// 结算广播:dispute_resolved + reward_claimed 都发到 dispute sub session conv，
// 让 evaluator sub session 在同一个会话里接着跑 "拉 context → claim → notify_main" 流程。
// 买家/卖家的仲裁结果通知走 notifyArbitrationResult（TASK_COMPLETED / TASK_REJECTED）。
// dispute_resolved = DisputeSettled 上链；reward_claimed = claimRewards tx 回执（mock 直接一并广播）。
async function broadcastSettlement(t: Task, winner: "buyer" | "seller", disputeId?: string) {
  const evaluators = await lookupEvaluators();
  const evalAddrs = Array.from(new Set(evaluators.map(e => e.comm_addr)));
  const allEvalAddrs = evalAddrs.length > 0 ? evalAddrs : ["0xEvaluator00000000000000000000000000001"];
  const convId = disputeConvId(t);
  const participants = Array.from(new Set([CHAIN_ADDR, ...allEvalAddrs]));

  await wsNotify(convId, participants, {
    type: "dispute_resolved", jobId: t.jobId, disputeId: disputeId ?? null, winner,
    content: `⚖️ 任务 ${t.jobId} 仲裁结果:${winner === "buyer" ? "买家胜,资金退回" : "卖家胜,资金释放"}。`,
  }).catch(e => console.error("[mock-api] dispute_resolved notify error:", e));

  await wsNotify(convId, participants, {
    type: "reward_claimed", jobId: t.jobId, disputeId: disputeId ?? null, status: "success",
    content: `💰 任务 ${t.jobId} 结算完成,奖金已入账。`,
  }).catch(e => console.error("[mock-api] reward_claimed notify error:", e));
}

async function notifyDisputed(jobId: string, disputeId: string, buyerCommAddr: string, buyerAgentId: string,
                               sellerCommAddr: string, sellerAgentId: string, reason: string) {
  // 动态查询所有已注册的 EVALUATOR，把他们都放进参与者列表(广播给所有仲裁候选)
  const evaluators = await lookupEvaluators();
  // 去重:同一 comm_addr 的重复注册(来自 openclaw 多次重连)只算一个
  const evalAddrs = Array.from(new Set(evaluators.map(e => e.comm_addr)));
  const fallbackEval = "0xEvaluator00000000000000000000000000001";
  const evalAgentId  = "mock-evaluator-agent-001";
  // 兜底:若没有任何 EVALUATOR 注册(服务还没起），仍发给默认 mock-evaluator 地址
  const allEvalAddrs = evalAddrs.length > 0 ? evalAddrs : [fallbackEval];
  const convId = `conv-arb-${jobId}-${buyerAgentId}-${sellerAgentId}-${evalAgentId}`;
  const participants = Array.from(new Set([CHAIN_ADDR, buyerCommAddr, sellerCommAddr, ...allEvalAddrs]));
  console.log(`[mock-api] dispute broadcast: evaluators=${allEvalAddrs.length} convId=${convId}`);
  await wsNotify(convId, participants, {
    type: "job_disputed", jobId, disputeId, buyerAgentId, sellerAgentId, reason,
    content: `⚖️ 任务 ${jobId} 进入仲裁 (disputeId=${disputeId})。\n买家拒绝验收，卖家申诉：${reason}\n\n请仲裁者查阅证据后裁决。`,
    llm: `job_disputed jobId=${jobId} disputeId=${disputeId} reason=${reason}`,
  });
}

async function notifyCompleted(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                               sellerCommAddr: string, sellerAgentId: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "job_completed", jobId, sellerAgentId,
    content: `系统通知：任务 ${jobId} 已验收通过（job_completed），资金已释放给卖家 ${sellerAgentId}。`,
  });
}

async function notifyRejected(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                              sellerCommAddr: string, sellerAgentId: string, reason: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "confirm_refund", jobId, sellerAgentId, reason,
    content: `系统通知：任务 ${jobId} 卖家同意退款（confirm_refund），原因：${reason}。资金已退还买家。`,
  });
}

async function notifyArbitrationResult(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                                        sellerCommAddr: string, sellerAgentId: string, winner: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  const evaluators = await lookupRoleAddrs("EVALUATOR");
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr, ...evaluators], {
    type: "dispute_resolved", jobId, sellerAgentId, buyerAgentId, winner,
    content: winner === "provider"
      ? `系统通知：任务 ${jobId} 仲裁完成，卖家 ${sellerAgentId} 胜诉（dispute_resolved）。资金判给卖家。`
      : `系统通知：任务 ${jobId} 仲裁完成，买家 ${buyerAgentId} 胜诉（dispute_resolved）。资金已退还买家。`,
  });
}

// ── Route helpers ─────────────────────────────────────────────────────────────
function parseBody(req: http.IncomingMessage): Promise<unknown> {
  if ((req as any)._parsedBody !== undefined) return Promise.resolve((req as any)._parsedBody);
  return new Promise((resolve) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => {
      const buf = Buffer.concat(chunks);
      (req as any)._rawBody = buf;
      try {
        const p = JSON.parse(buf.toString("utf8"));
        (req as any)._parsedBody = p;
        resolve(p);
      } catch {
        (req as any)._parsedBody = {};
        resolve({});
      }
    });
  });
}

function matchPath(pattern: string, pathname: string): Record<string, string> | null {
  const pp = pattern.split("/"), sp = pathname.split("/");
  if (pp.length !== sp.length) return null;
  const params: Record<string, string> = {};
  for (let i = 0; i < pp.length; i++) {
    if (pp[i].startsWith(":")) params[pp[i].slice(1)] = sp[i];
    else if (pp[i] !== sp[i]) return null;
  }
  return params;
}

function send(res: http.ServerResponse, status: number, body: unknown) {
  (res as any)._logBody = body;
  const json = JSON.stringify(body);
  res.writeHead(status, { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" });
  res.end(json);
}
function sendOk(res: http.ServerResponse, data: unknown) { send(res, 200, ok(data)); }
function sendErr(res: http.ServerResponse, code: number, msg: string) {
  send(res, code === 2001 ? 404 : 400, errRes(code, msg));
}

// ── Request handler ───────────────────────────────────────────────────────────
const server = http.createServer(async (req, res) => {
  const url    = new URL(req.url!, `http://localhost`);
  const method = req.method!.toUpperCase();
  const originalPath = url.pathname;
  const path_  = normalizePath(url.pathname);

  // OPTIONS preflight
  if (method === "OPTIONS") { res.writeHead(204, { "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Methods": "*", "Access-Control-Allow-Headers": "*" }); res.end(); return; }

  // ── Pre-parse body for logging ─────────────────────────────────────────────
  if (method === "POST" || method === "PUT") {
    await parseBody(req);
  }

  // ── API request logging (fire after response completes) ──────────────────
  if ((path_.startsWith("/api/v1/") && path_ !== "/api/v1/logs" && path_ !== "/api/v1/tasks/all") || path_.startsWith("/ui/notify/")) {
    res.on("finish", () => {
      const reqBody = (req as any)._parsedBody as Record<string, unknown> | undefined;
      const resBody = (res as any)._logBody;
      const agentId = String(reqBody?.buyerAgentId ?? reqBody?.provider_agent_id ?? reqBody?.agentId ?? "");
      if (path_.startsWith("/ui/notify/")) {
        const parts = path_.split("/");
        const jobId = parts[4];
        const t = tasks.get(jobId);
        const who = t ? (t.buyerAgentId || t.providerAgentId || "") : "";
        logApi(method, originalPath, res.statusCode, jobId, `ui:${parts[3]}`, reqBody, resBody, who || undefined);
      } else {
        const jobMatch = path_.match(/task\/(0x[0-9a-f]+)/i);
        // task/create 没有路径 jobId，从响应体提取
        const jobId = jobMatch?.[1] ?? (resBody as any)?.data?.jobId;
        // 身份优先级：请求 body → X-Agent-Id header
        const headerAgent = String(req.headers["x-agent-id"] ?? "");
        let who = agentId || headerAgent;
        // 仅在特定 action 能明确归属时 fallback 到任务的 buyer/provider
        if (!who && jobId) {
          const t = tasks.get(jobId);
          const action = path_.split("/").pop();
          if (t) {
            if (action === "apply" || action === "submit") who = t.providerAgentId ?? "";
            else if (action === "accept" || action === "complete" || action === "refuse") who = t.buyerAgentId ?? "";
          }
        }
        logApi(method, originalPath, res.statusCode, jobId, path_.split("/").pop(), reqBody, resBody, who || undefined);
      }
    });
  }

  // ── Dashboard ──────────────────────────────────────────────────────────────
  if (method === "GET" && (path_ === "/" || path_ === "/index.html")) {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    res.end(DASHBOARD_HTML);
    return;
  }

  // ── Event logs API ─────────────────────────────────────────────────────────
  if (method === "GET" && path_ === "/api/v1/logs") {
    const kind = url.searchParams.get("kind");
    const limit = Math.min(Number(url.searchParams.get("limit") ?? 100), MAX_LOGS);
    const filtered = kind ? eventLogs.filter(e => e.kind === kind) : eventLogs;
    sendOk(res, { logs: filtered.slice(0, limit) }); return;
  }

  // ── Identity APIs (priapi/v5/wallet/agentic/*) ─────────────────────────────
  // 其他 priapi/v5/wallet/agentic/* 路由（auth/init、auth/verify、auth/refresh 等）
  // 没命中 mock 则由末尾的 proxyToUpstream() 透传到真实后端。
  // 对应 cli/src/commands/agent_commerce/identity/queries.rs 的 get() 入口
  // GET /priapi/v5/wallet/agentic/agent/agent-list
  //   query: chainIndex (忽略) / agentIdList="209,213" (逗号分隔) / page / pageSize
  //   返回 {code:0, data:[{list, page, pageSize, total}]} —— 外层数组是后端真实格式
  if (method === "GET" && path_ === "/priapi/v5/wallet/agentic/agent/agent-list") {
    const agentIdListRaw = url.searchParams.get("agentIdList") ?? "";
    const page = Math.max(1, Number(url.searchParams.get("page") ?? 1) || 1);
    const pageSize = Math.max(1, Number(url.searchParams.get("pageSize") ?? 20) || 20);

    const agents = loadJsonFixture<any[]>("agents.json", []);
    let filtered = agents;
    if (agentIdListRaw) {
      const wanted = new Set(
        agentIdListRaw.split(",").map(s => s.trim()).filter(Boolean),
      );
      filtered = agents.filter(a => wanted.has(String(a.agentId)));
    }
    const total = filtered.length;
    const start = (page - 1) * pageSize;
    const list = filtered.slice(start, start + pageSize);
    console.log(
      `[mock-api] agent-list: agentIdList="${agentIdListRaw}" page=${page} pageSize=${pageSize} → ${list.length}/${total}`,
    );
    sendOk(res, [{ list, page, pageSize, total }]);
    return;
  }

  // ── Static routes ──────────────────────────────────────────────────────────
  if (method === "DELETE" && path_ === "/api/v1/reset") {
    const n = tasks.size; tasks.clear(); confirms.clear(); saveTasks();
    console.log(`[mock-api] reset: removed ${n} tasks`);
    sendOk(res, { removed: n }); return;
  }
  if (method === "GET" && path_ === "/api/v1/tasks/all") {
    const list = [...tasks.values()].sort((a, b) => b.createTime.localeCompare(a.createTime));
    sendOk(res, { total: list.length, list }); return;
  }
  // Agent profile mock
  if (method === "GET" && path_ === "/api/v1/agent/list") {
    sendOk(res, {
      total: 2, page: 1, pageSize: 20,
      list: [
        {
          agentId: "10001",
          status: 1,
          ownerAddress: "0x2381...",
          name: "My DeFi Agent",
          profilePicture: "https://cdn.example.com/agent/avatar1.png",
          profileDescription: "A DeFi trading agent",
        },
      ],
    });
    return;
  }
  if (method === "GET" && path_ === "/api/v1/task/list") {
    const status = url.searchParams.get("status");
    let list = [...tasks.values()].filter(t => t.openType === 1 && (!status || t.statusStr === status));
    list.sort((a, b) => b.createTime.localeCompare(a.createTime));
    const page = parseInt(url.searchParams.get("page") ?? "1");
    const size = parseInt(url.searchParams.get("pageSize") ?? "20");
    const slice = list.slice((page - 1) * size, page * size);
    sendOk(res, { total: list.length, page, pageSize: size, list: slice }); return;
  }
  if (method === "GET" && path_ === "/api/v1/tasks/my") {
    const role = url.searchParams.get("role") ?? "";
    const addr = url.searchParams.get("agent_address") ?? url.searchParams.get("agentAddress") ?? "";
    if (role !== "client" && role !== "provider") { sendErr(res, 1001, "role must be client or provider"); return; }
    let list = [...tasks.values()].filter(t =>
      role === "client" ? t.buyerAgentAddress === addr : t.providerAgentAddress === addr
    );
    list.sort((a, b) => b.updateTime.localeCompare(a.updateTime));
    sendOk(res, { total: list.length, list }); return;
  }
  if (method === "GET" && path_ === "/api/v1/task/hasInProgress") {
    const addr = url.searchParams.get("agent_address") ?? url.searchParams.get("agentAddress") ?? "";
    const has = [...tasks.values()].some(t =>
      (t.buyerAgentAddress === addr || t.providerAgentAddress === addr) && t.status >= S_OPEN && t.status <= S_DISPUTED
    );
    sendOk(res, { hasInProgress: has }); return;
  }
  if (method === "POST" && path_ === "/api/v1/task/create") {
    const body = await parseBody(req) as Record<string, unknown>;
    const title = String(body.title ?? "");
    const desc  = String(body.description ?? "");
    if (!title || title.length > 256) { sendErr(res, 1001, "title required, max 256 chars"); return; }
    if (!desc) { sendErr(res, 1001, "description required"); return; }
    const jobId = genJobId();
    const task: Task = {
      jobId, title, description: desc,
      descriptionSummary: String(body.descriptionSummary ?? desc.slice(0, 200)),
      tokenAddress:  String(body.paymentTokenAddress ?? "0xUSDT0000000000000000000000000000000001"),
      tokenAmount:   String(body.paymentTokenAmount ?? body.tokenAmount ?? "100"),
      paymentType:   body.paymentType != null ? Number(body.paymentType) : null,
      openType:      Number(body.visibility ?? 0),
      status: S_OPEN, statusStr: "open",
      chainId:       Number(body.chainId ?? 196),
      minCreditScore: body.minCreditScore != null ? Number(body.minCreditScore) : null,
      designatedProvider: body.designatedProvider != null ? String(body.designatedProvider) : null,
      buyerAgentAddress: String(body.buyerAgentAddress ?? "0xMockBuyer00000000000000000000000000001"),
      buyerAgentId:      String(body.buyerAgentId ?? "mock-buyer-agent-001"),
      providerAgentAddress: null, providerAgentId: null, groupId: null, evaluatorAddress: null,
      expireConfig: body.expireConfig ?? { openExpireSec: 86400, acceptedExpireSec: 259200 },
      createTime: nowIso(), updateTime: nowIso(),
    };
    tasks.set(jobId, task);
    saveTasks();
    console.log(`[mock-api] task created: ${jobId} "${title}"`);
    const buyerAddr = task.buyerAgentAddress;
    setTimeout(async () => {
      console.log(`[mock-api] sending job_created for job=${jobId}`);
      await notifyConfirmed(jobId, buyerAddr).catch(e => console.error("[mock-api] confirmed error:", e));
      console.log(`[mock-api] job_created sent for job=${jobId}`);
    }, 8000);
    sendOk(res, { jobId, uopData: { uopHash: mockUop(), extraData: {} }, status: "pending", msg: "任务已提交，等待上链确认" }); return;
  }

  // ── Parameterized routes ───────────────────────────────────────────────────
  let m: Record<string, string> | null;

  if (method === "GET" && (m = matchPath("/api/v1/task/:jobId/providerConfirmStatus", path_))) {
    const { jobId } = m;
    if (!tasks.has(jobId)) { sendErr(res, 2001, "task not found"); return; }
    const cs = confirms.get(jobId);
    const agentId = url.searchParams.get("providerAgentId") ?? url.searchParams.get("provider_agent_id");
    const c = agentId ? cs?.find(x => x.providerAgentId === agentId) : cs?.[0];
    sendOk(res, c ? { confirmed: true, ...c } : { confirmed: false, providerAddress: null, providerAgentId: null, tokenAddress: null, tokenAmount: null });
    return;
  }
  // GET dispute info（必须在 /api/v1/task/:jobId 之前匹配，否则 "dispute" 会被当成 jobId）
  if (method === "GET" && (m = matchPath("/api/v1/task/dispute/:disputeId", path_))) {
    const d = disputes.get(m.disputeId);
    if (!d) { sendErr(res, 2001, "dispute not found"); return; }
    sendOk(res, d); return;
  }
  if (method === "GET" && (m = matchPath("/api/v1/task/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    sendOk(res, { task: t }); return;
  }
  // Buyer 获取支付预信息（confirm-accept 前准备链上参数）
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/prePayTaskInfo", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const tokenSymbol = String(body.tokenSymbol ?? "USDT").toUpperCase();
    const currencyMap: Record<string, string> = {
      USDT: "0xUSDT0000000000000000000000000000000001",
      USDG: "0xUSDG0000000000000000000000000000000001",
    };
    const providerAddr = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    sendOk(res, {
      currency: currencyMap[tokenSymbol] ?? currencyMap.USDT,
      recipient: providerAddr,
      receiver: providerAddr,
      evaluator: "0x1234567890abcdef1234567890abcdef12345678",
      submitWindow: "86400",
      disputeWindow: "172800",
      evaluateWindow: "86400",
      completedWindow: "259200",
      hook: "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
      hookData: "0x",
      salt: jobId,
      expiredAt: String(Math.floor(Date.now() / 1000) + 86400),
    });
    return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/apply", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_OPEN) { sendErr(res, 2002, "task status must be OPEN"); return; }
    const body = await parseBody(req) as Record<string, unknown>;

    // provider 身份来源优先级: header > body > 默认值
    const hdrAgentId = req.headers["x-agent-id"] as string | undefined;
    const hdrAddr    = req.headers["x-wallet-address"] as string | undefined;
    const sellerAgent = String(hdrAgentId ?? body.providerAgentId ?? body.provider_agent_id ?? "mock-seller-agent-001");
    const sellerAddr  = String(hdrAddr ?? body.providerAddress ?? body.provider_address ?? "0xSeller000000000000000000000000000000001");
    // tokenAmount: "0" 表示接受原价，>0 表示议价
    const rawAmount = String(body.tokenAmount ?? body.price_usdt ?? "0");
    const amount    = rawAmount === "0" ? t.tokenAmount : rawAmount;
    const symbol    = String(body.tokenSymbol ?? "USDT");

    // 通过 agentId 查找 WS 通信地址，找不到则回退到钱包地址
    const sellerCommAddr = await lookupCommAddr(sellerAgent) ?? sellerAddr;
    console.log(`[mock-api] provider applied: job=${jobId} provider=${sellerAgent} walletAddr=${sellerAddr} commAddr=${sellerCommAddr} amount=${amount} ${symbol}`);

    const confirm: ProviderConfirm = { providerAddress: sellerAddr, providerAgentId: sellerAgent, tokenAddress: "0xUSDT0000000000000000000000000000000001", tokenAmount: amount };
    if (!confirms.has(jobId)) confirms.set(jobId, []);
    confirms.get(jobId)!.push(confirm);
    // 延迟发送通知，模拟链上确认时间，确保 agent 的文本回复先到达买家
    sleep(8000).then(() =>
      notifyApplied(jobId, t.buyerAgentAddress, t.buyerAgentId, sellerAgent, sellerCommAddr, amount)
    ).catch(e => console.error("[mock-api] apply notify error:", e));
    // 向 openclaw 推系统通知（provider_applied 等）由卖家 mock UI 手动触发，
    // 不再从 mock-api 自动推送。
    // 返回标准 uopData 结构（CLI 的 task_sign_and_broadcast 期望此格式）
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/accept", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_OPEN) { sendErr(res, 2002, "task status must be OPEN"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    t.providerAgentId      = String(body.providerAgentId ?? body.provider_agent_id ?? "mock-seller-agent-001");
    // 从 apply 阶段已存的 confirms 里查 commAddr，不依赖买家传地址
    const matchConfirm = (confirms.get(jobId) ?? []).find(c => c.providerAgentId === t.providerAgentId);
    t.providerAgentAddress = matchConfirm?.providerAddress
      ?? String(body.providerAddress ?? body.provider_address ?? "0xSeller000000000000000000000000000000001");
    if (body.groupId) t.groupId = String(body.groupId);
    // 状态推进交给 /broadcast（看 bizType=7 JobAccept），endpoint 只做参数记录
    console.log(`[mock-api] /accept staged (waiting for broadcast): job=${jobId} provider=${t.providerAgentAddress}`);
    const { buyerAgentAddress, buyerAgentId, providerAgentId } = t;
    setTimeout(async () => {
      const sellerComm = await lookupCommAddr(providerAgentId!) ?? t.providerAgentAddress!;
      await notifyAccepted(jobId, buyerAgentAddress, buyerAgentId, sellerComm, providerAgentId!).catch(e => console.error("[mock-api] accepted notify error:", e));
    }, 5000);
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/submit", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_ACCEPTED) { sendErr(res, 2002, "task status must be ACCEPTED"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const deliverable = String(body.deliverable ?? body.deliverable_url ?? `https://mock-deliverable.example.com/${jobId}.html`);
    // 状态推进交给 /broadcast（bizType=8 JobSubmit）
    console.log(`[mock-api] /submit staged (waiting for broadcast): job=${jobId}`);
    const { buyerAgentAddress, buyerAgentId, providerAgentId } = t;
    setTimeout(async () => {
      const sellerComm = await lookupCommAddr(providerAgentId!) ?? t.providerAgentAddress!;
      await notifySubmitted(jobId, buyerAgentAddress, buyerAgentId, providerAgentId!, sellerComm, deliverable).catch(e => console.error("[mock-api] submit notify error:", e));
    }, 3000);
    sendOk(res, { uopData: mockUopData(), status: "pending", msg: "交付物已提交，等待上链确认" }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/complete", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_SUBMITTED) { sendErr(res, 2002, "task status must be SUBMITTED"); return; }
    // 状态推进交给 /broadcast（bizType=9 JobComplete）
    console.log(`[mock-api] /complete staged (waiting for broadcast): job=${jobId}`);
    const { buyerAgentAddress: ba, buyerAgentId: bi, providerAgentId: pi } = t;
    setTimeout(async () => {
      const sellerComm = await lookupCommAddr(pi!) ?? t.providerAgentAddress!;
      await notifyCompleted(jobId, ba, bi, sellerComm, pi!).catch(e => console.error("[mock-api] completed notify error:", e));
    }, 3000);
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/refuse", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_SUBMITTED) { sendErr(res, 2002, "task status must be SUBMITTED"); return; }
    // 状态推进交给 /broadcast（bizType=10 JobRefuse）
    console.log(`[mock-api] /refuse staged (waiting for broadcast): job=${jobId}`);
    const pid = t.providerAgentId ?? "mock-seller-agent-001";
    (async () => {
      const sellerComm = await lookupCommAddr(pid) ?? t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
      await notifyRefused(jobId, t.buyerAgentAddress, t.buyerAgentId, sellerComm, pid);
    })().catch(e => console.error("[mock-api] refused notify error:", e));
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/close", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_OPEN) { sendErr(res, 2002, "task status must be OPEN"); return; }
    // 状态推进交给 /broadcast（bizType=16 JobClose）
    console.log(`[mock-api] /close staged (waiting for broadcast): job=${jobId}`);
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/setVisibility", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_OPEN) { sendErr(res, 2002, "task status must be OPEN"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    t.openType = Number(body.visibility ?? 1); t.updateTime = nowIso();
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/dispute", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_REFUSED) { sendErr(res, 2002, "task status must be REFUSED"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const reason = String(body.reason ?? "");
    // 生成 disputeId（简化:确定性公式 d-{jobId}-r{round}）
    const existingRounds = [...disputes.values()].filter(d => d.jobId === jobId).length;
    const round = existingRounds + 1;
    const disputeId = `d-${jobId}-r${round}`;
    const dispute: Dispute = {
      disputeId, jobId, round,
      clientReason: "买家拒绝验收,未满足验收标准",
      providerReason: reason,
      qualityStandards: t.description.split("验收标准：")[1] ?? "未明确验收标准",
      deliverableUrl: `https://mock-deliverable.example.com/${jobId}.html`,
      evidences: [
        { from: "client", summary: "买家认为交付物未满足验收标准", level: "C" },
        { from: "provider", summary: "卖家声称交付物符合协商约定", level: "C" },
      ],
      voterCommits: {},
      votes: [],
      verdict: null,
      createTime: nowIso(),
      commitPhaseStartedAt: null,
      resolvedAt: null,
    };
    disputes.set(disputeId, dispute);
    // 状态推进交给 /broadcast（bizType=2 DisputeCreate）；dispute 记录此处 staged
    console.log(`[mock-api] /dispute staged (waiting for broadcast): job=${jobId} disputeId=${disputeId} reason=${reason}`);
    const { buyerAgentAddress, buyerAgentId, providerAgentAddress, providerAgentId } = t;
    notifyDisputed(jobId, disputeId, buyerAgentAddress, buyerAgentId, providerAgentAddress ?? "0xSeller000000000000000000000000000000001", providerAgentId ?? "mock-seller-agent-001", reason)
      .catch(e => console.error("[mock-api] dispute notify error:", e));
    // 模拟 Preparation → VoterSelection → CommitPhase:查出 evaluator 候选,推 evaluator_selected + 标记 commitPhaseStartedAt
    setTimeout(async () => {
      const evaluators = await lookupEvaluators();
      const evalAddrs = Array.from(new Set(evaluators.map(e => e.comm_addr)));
      const targets = evalAddrs.length > 0 ? evalAddrs : ["0xEvaluator00000000000000000000000000001"];
      dispute.commitPhaseStartedAt = nowIso();
      notifyEvaluatorSelected(t, disputeId, targets);
    }, EVALUATOR_SELECTED_DELAY_MS);
    // 跟其他 task endpoints 一致返回 uopData(CLI 的 sign_uop_and_broadcast 期望此结构)
    sendOk(res, { uopData: mockUopData(), disputeId }); return;
  }
  // 仲裁证据:文本 + 图片清单(真后端 /priapi/v1/aieco/task/{jobId}/evidence)
  if (method === "GET" && (m = matchPath("/api/v1/task/:jobId/evidence", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const dispute = [...disputes.values()].find(d => d.jobId === jobId);
    if (!dispute) { sendErr(res, 2001, "dispute not found"); return; }
    const qs = t.description.split("验收标准：")[1] ?? dispute.qualityStandards;
    sendOk(res, {
      jobId,
      disputeId: dispute.disputeId,
      round: dispute.round,
      qualityStandards: qs,
      clientReason: dispute.clientReason,
      providerReason: dispute.providerReason,
      deliverableUrl: dispute.deliverableUrl,
      evidences: [
        { from: "client",   kind: "text",  content: "买家证据文字:交付物未完全满足验收标准,缺少单元测试。", level: "C" },
        { from: "client",   kind: "image", name: "client-screenshot.png",  level: "C" },
        { from: "provider", kind: "text",  content: "卖家证据文字:交付物符合协商约定,已附带完整文档。", level: "C" },
        { from: "provider", kind: "image", name: "provider-delivery.png",  level: "C" },
      ],
    });
    return;
  }
  // 证据图片下载(真后端 /priapi/v1/aieco/task/{jobId}/evidence/download)
  if (method === "GET" && (m = matchPath("/api/v1/task/:jobId/evidence/download", path_))) {
    const { jobId } = m;
    if (!tasks.has(jobId)) { sendErr(res, 2001, "task not found"); return; }
    const name = url.searchParams.get("name");
    if (!name) { sendErr(res, 1001, "name required"); return; }
    // mock:返回 1x1 透明 PNG(67 bytes)
    const MOCK_PNG = Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==",
      "base64",
    );
    res.writeHead(200, {
      "Content-Type": "image/png",
      "Content-Disposition": `attachment; filename="${name}"`,
      "Access-Control-Allow-Origin": "*",
    });
    res.end(MOCK_PNG);
    return;
  }
  // Commit-Reveal Phase 1:提交投票承诺(后端生成 salt,mock 真后端都这样)
  // evaluator CLI 走 signing flow:X-Wallet-Address = voter;返回 uopData 供 CLI 签名广播
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/vote/commit", path_))) {
    const { jobId } = m;
    const dispute = [...disputes.values()].find(d => d.jobId === jobId && !d.resolvedAt);
    if (!dispute) { sendErr(res, 2001, "active dispute not found"); return; }
    if (!dispute.commitPhaseStartedAt) { sendErr(res, 2002, "commit phase not started (voters not yet selected)"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const vote = Number(body.vote);
    if (vote !== 1 && vote !== 2) { sendErr(res, 1001, "vote must be 1 (provider) or 2 (client)"); return; }
    // 真后端（Lark §11175）commit body 仅 `{ vote }`，reason 不在 API schema。
    // mock 这里仍保留字段占位（可选），方便做本地分析/dashboard 显示，但不强制。
    const reason = String(body.reason ?? "");
    // voter = header 优先（CLI 带身份头），回退到 body.voter（老 mock 调用）
    const voter = String(req.headers["x-wallet-address"] ?? body.voter ?? "evaluator-unknown");
    if (dispute.voterCommits[voter]) { sendErr(res, 2002, "voter has already committed"); return; }
    const salt = crypto.randomBytes(16).toString("hex");
    const commitHash = "0x" + crypto.createHash("sha256")
      .update(`${dispute.disputeId}|${vote}|${salt}`).digest("hex");
    dispute.voterCommits[voter] = {
      vote: vote as 1 | 2, salt, reason, committedAt: nowIso(),
    };
    console.log(`[mock-api] vote committed: disputeId=${dispute.disputeId} voter=${voter} vote=${vote}`);
    // tx 回执:vote_committed 立即推(真后端是 commit tx 上链后)
    notifyVoteCommitted(jobId, dispute.disputeId, voter);
    // 模拟 commit 窗口结束:reveal_started 延后推(真后端 18H,mock 3s 可调)。
    // 发送到 dispute sub session conv，让 evaluator sub session 复用同一会话跑 reveal。
    setTimeout(async () => {
      const t2 = tasks.get(jobId);
      if (!t2) return;
      const evaluators = await lookupEvaluators();
      const evalAddrs = Array.from(new Set(evaluators.map(e => e.comm_addr)));
      const targets = evalAddrs.length > 0 ? evalAddrs : [voter];
      notifyRevealStarted(t2, dispute.disputeId, targets);
    }, REVEAL_WINDOW_DELAY_MS);
    sendOk(res, { uopData: mockUopData(), disputeId: dispute.disputeId, commitHash }); return;
  }
  // Commit-Reveal Phase 2:披露承诺。按真后端 spec（Lark §11348），voter 传入 vote，
  // 后端从 task_dispute_voter 读 salt，组装 revealVote(jobId, vote, salt) calldata。
  // mock 这里做一致性校验：body.vote 必须与 commit 时存的 vote 相同，否则模拟链上 revert。
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/vote/reveal", path_))) {
    const { jobId } = m;
    const dispute = [...disputes.values()].find(d => d.jobId === jobId && !d.resolvedAt);
    if (!dispute) { sendErr(res, 2001, "active dispute not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const voter = String(req.headers["x-wallet-address"] ?? body.voter ?? "evaluator-unknown");
    const commit = dispute.voterCommits[voter];
    if (!commit) { sendErr(res, 2002, "voter has not committed"); return; }
    if (commit.revealedAt) { sendErr(res, 2002, "voter has already revealed"); return; }
    // 校验 reveal vote 与 commit vote 一致（真后端靠链上 commitHash 比对，mock 直接查表）
    const revealVote = Number(body.vote);
    if (revealVote !== 1 && revealVote !== 2) {
      sendErr(res, 1001, "vote must be 1 (provider) or 2 (client)"); return;
    }
    if (revealVote !== commit.vote) {
      sendErr(res, 2012, `reveal vote (${revealVote}) does not match commit vote (${commit.vote}); on-chain commitHash would not verify`);
      return;
    }
    commit.revealedAt = nowIso();
    dispute.votes.push({ side: commit.vote, reason: commit.reason, voter, at: commit.revealedAt });
    console.log(`[mock-api] vote revealed: disputeId=${dispute.disputeId} voter=${voter} vote=${commit.vote}`);
    // tx 回执:vote_revealed
    notifyVoteRevealed(jobId, dispute.disputeId, voter);
    // mock 简化:单投票者,reveal 完就结算
    const allCommitters = Object.keys(dispute.voterCommits);
    const allRevealed = allCommitters.every(v => dispute.voterCommits[v].revealedAt);
    let settled = false;
    let winner: "buyer" | "seller" | undefined;
    if (allRevealed) {
      winner = commit.vote === 1 ? "seller" : "buyer";
      dispute.verdict = commit.vote === 1 ? "provider" : "client";
      dispute.resolvedAt = nowIso();
      const t = tasks.get(dispute.jobId);
      if (t && t.status === S_DISPUTED) {
        setStatus(t, S_COMPLETE); saveTasks();
        broadcastSettlement(t, winner, dispute.disputeId)
          .catch(e => console.error("[mock-api] settlement broadcast error:", e));
      }
      settled = true;
      console.log(`[mock-api] dispute settled: disputeId=${dispute.disputeId} winner=${winner}`);
    }
    sendOk(res, {
      uopData: mockUopData(), disputeId: dispute.disputeId,
      revealedVote: commit.vote, settled,
      ...(settled ? { winner, verdict: dispute.verdict } : {}),
    });
    return;
  }
  // Read-only:查询指定 voter 是否可以进入 reveal 阶段
  // voter 从 query 参数或 X-Wallet-Address header 读取
  if (method === "GET" && (m = matchPath("/api/v1/task/:jobId/vote/canReveal", path_))) {
    const { jobId } = m;
    const dispute = [...disputes.values()].find(d => d.jobId === jobId && !d.resolvedAt);
    if (!dispute) { sendErr(res, 2001, "active dispute not found"); return; }
    const voter = url.searchParams.get("voter")
      ?? String(req.headers["x-wallet-address"] ?? "");
    if (!voter) { sendErr(res, 1001, "voter required"); return; }
    const commit = dispute.voterCommits[voter];
    if (!commit) { sendOk(res, { canReveal: false, reason: "not committed" }); return; }
    if (commit.revealedAt) { sendOk(res, { canReveal: false, reason: "already revealed" }); return; }
    // mock 简化:committed 即可 reveal。真后端此处门控 commit 窗口是否结束。
    sendOk(res, { canReveal: true, reason: "ok" }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/claim", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const claimer = String(req.headers["x-wallet-address"] ?? "unknown");
    console.log(`[mock-api] reward claim: job=${m.jobId} claimer=${claimer}`);
    sendOk(res, { uopData: mockUopData(), jobId: m.jobId, amount: t.tokenAmount, currency: "USDT", msg: "奖金已领取(mock stub)" }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/agreeRefund", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_REFUSED) { sendErr(res, 2002, "task status must be REFUSED"); return; }
    setStatus(t, 6 /* REJECTED */); saveTasks();
    console.log(`[mock-api] task agreeRefund: job=${jobId}`);
    const { buyerAgentAddress: ba4, buyerAgentId: bi4, providerAgentId: pi4 } = t;
    (async () => {
      const sellerComm = await lookupCommAddr(pi4!) ?? t.providerAgentAddress!;
      await notifyRejected(jobId, ba4, bi4, sellerComm, pi4!, "卖家同意退款");
    })().catch(e => console.error("[mock-api] agreeRefund notify error:", e));
    sendOk(res, { uopData: mockUopData() }); return;
  }
  // multipart/form-data 链下证据上传（必须在 /evidence 之前匹配）
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/evidence/upload", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_DISPUTED) { sendErr(res, 2002, "task status must be DISPUTED"); return; }

    // 简易 multipart 解析：从预解析的 _rawBody 里提取 text 字段和 images 的 filename 列表
    const rawBuf = ((req as any)._rawBody as Buffer) ?? Buffer.alloc(0);
    const raw = rawBuf.toString("latin1");
    const textMatch = raw.match(/name="text"\r?\n\r?\n([\s\S]*?)\r?\n--/);
    const text = textMatch ? Buffer.from(textMatch[1], "latin1").toString("utf8") : "";
    const imageMatches = [...raw.matchAll(/name="images"; filename="([^"]+)"/g)];
    const imageFiles = imageMatches.map(mm => mm[1]);

    if (!text && imageFiles.length === 0) {
      sendErr(res, 1001, "text or images required"); return;
    }

    const submitter = String(req.headers["x-wallet-address"] ?? "");
    const from: "client" | "provider" = submitter.toLowerCase() === (t.providerAgentAddress ?? "").toLowerCase()
      ? "provider" : "client";
    const dispute = [...disputes.values()].filter(d => d.jobId === jobId).pop();
    if (dispute) {
      if (text) dispute.evidences.push({ from, summary: text, level: "C" });
      for (const f of imageFiles) {
        dispute.evidences.push({ from, summary: `(image) ${f}`, url: `https://mock-cdn.example.com/evidence/${jobId}/${f}`, level: "C" });
      }
    }
    console.log(`[mock-api] evidence uploaded (multipart): job=${jobId} text="${text.slice(0, 60)}" images=${imageFiles.length}`);
    sendOk(res, null); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/evidence", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_DISPUTED) { sendErr(res, 2002, "task status must be DISPUTED"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const text = String(body.text ?? body.summary ?? "");
    const submitter = String(req.headers["x-wallet-address"] ?? "");
    const from: "client" | "provider" = submitter.toLowerCase() === (t.providerAgentAddress ?? "").toLowerCase()
      ? "provider" : "client";
    // Append to latest dispute for this job
    const dispute = [...disputes.values()].filter(d => d.jobId === jobId).pop();
    if (dispute) {
      dispute.evidences.push({ from, summary: text, level: "C" });
    }
    console.log(`[mock-api] evidence uploaded: job=${jobId} text="${text.slice(0, 80)}"`);
    sendOk(res, { uopData: mockUopData() }); return;
  }
  // ── Broadcast (CLI task_sign_and_broadcast final step) ────────────────────
  // 真实链上语义：广播即上链。状态推进集中在这里，按 bizContext.bizType 区分场景。
  // 之前在 /accept、/submit、/complete、/reject、/close、/dispute/raise 这些
  // endpoint 内部 setStatus 是错的——那只是链下准备阶段。
  if (method === "POST" && path_ === "/api/v1/task/broadcast") {
    const body = (req as any)._parsedBody as Record<string, unknown> | undefined;
    const bizCtx = body?.bizContext as { jobId?: string; bizType?: number } | undefined;
    if (bizCtx?.jobId && typeof bizCtx?.bizType === "number") {
      const t = tasks.get(bizCtx.jobId);
      if (t) {
        const before = t.statusStr;
        // BizContext 枚举对齐 cli/src/commands/agent_commerce/task/signing.rs
        switch (bizCtx.bizType) {
          case 7:  // JobAccept    : open → accepted
            if (t.status === S_OPEN)       setStatus(t, S_ACCEPTED);
            break;
          case 8:  // JobSubmit    : accepted → submitted
            if (t.status === S_ACCEPTED)   setStatus(t, S_SUBMITTED);
            break;
          case 9:  // JobComplete  : submitted → completed
            if (t.status === S_SUBMITTED)  setStatus(t, S_COMPLETE);
            break;
          case 10: // JobRefuse    : submitted → refused
            if (t.status === S_SUBMITTED)  setStatus(t, S_REFUSED);
            break;
          case 16: // JobClose     : open → close
            if (t.status === S_OPEN)       setStatus(t, S_CLOSE);
            break;
          case 2:  // DisputeCreate: refused → disputed
            if (t.status === S_REFUSED)    setStatus(t, S_DISPUTED);
            break;
          // 其他 bizType（JobApply=15 / SetVisibility=17 / SetPaymentMode=18 / Stake=11/19 / ...）不改 task 状态
        }
        if (t.statusStr !== before) {
          saveTasks();
          console.log(`[mock-api] broadcast bizType=${bizCtx.bizType} job=${bizCtx.jobId}: ${before} → ${t.statusStr}`);
        }
      }
    }
    sendOk(res, [{ txHash: mockUop() }]); return;
  }

  // ── Admin: force task to specific status (mock-only test backdoor) ─────────
  // 用于"快速跳转到任意状态"测试场景：跳过状态机校验直接 PATCH。
  // POST /admin/task/:jobId/force-status  body { statusStr, providerAgentAddress?, providerAgentId? }
  // providerAgent* 可选——force-jump 跳过 /apply 时 task.providerAgent* 为空，导致后续 dispute/upload
  // 等需要校验钱包归属的 CLI 命令报错。允许调用方一起把这俩 stitch 进去。
  if (method === "POST" && (m = matchPath("/admin/task/:jobId/force-status", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const target = String(body?.statusStr ?? "");
    const statusMap: Record<string, number> = {
      open: S_OPEN, accepted: S_ACCEPTED, submitted: S_SUBMITTED,
      refused: S_REFUSED, disputed: S_DISPUTED,
      complete: S_COMPLETE, completed: S_COMPLETE,
      rejected: 6, refunded: 6, close: S_CLOSE, expired: 8,
    };
    const newStatus = statusMap[target];
    if (newStatus === undefined) {
      sendErr(res, 4000, `unknown statusStr: ${target}（accepted: ${Object.keys(statusMap).join("/")}）`);
      return;
    }
    const before = t.statusStr;
    setStatus(t, newStatus);
    if (typeof body?.providerAgentAddress === "string" && body.providerAgentAddress) {
      t.providerAgentAddress = String(body.providerAgentAddress);
    }
    if (typeof body?.providerAgentId === "string" && body.providerAgentId) {
      t.providerAgentId = String(body.providerAgentId);
    }
    saveTasks();
    console.log(`[mock-api] FORCE-STATUS job=${m.jobId}: ${before} → ${t.statusStr}`
      + (t.providerAgentAddress ? ` provider=${t.providerAgentAddress}/${t.providerAgentId ?? "?"}` : ""));
    sendOk(res, { jobId: m.jobId, before, after: t.statusStr,
      providerAgentAddress: t.providerAgentAddress ?? null,
      providerAgentId: t.providerAgentId ?? null });
    return;
  }

  // ── Staking: evaluator onboarding (Lark §8.2 /staking/stake) ──────────────
  // agentId 从 X-Agent-Id header 获取；amount 是 OKB UI 单位（string，不带精度）。
  // 真后端返回仅 uopHash（文档 §8.2 示例），但 CLI 走通用 sign_uop_and_broadcast 需要
  // uopData（UnsignedInfoResponse）。mock 这里按 CLI 约定返回 {uopData, uopHash}。
  if (method === "POST" && path_ === "/api/v1/task/staking/stake") {
    const agentId = String(req.headers["x-agent-id"] ?? "");
    if (!agentId) { sendErr(res, 4000, "X-Agent-Id header required"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const amountStr = String(body.amount ?? "");
    const amount = Number(amountStr);
    if (!amountStr || !Number.isFinite(amount) || amount <= 0) {
      sendErr(res, 1001, "amount must be positive OKB number (UI unit, no precision)"); return;
    }
    // 首次质押最低 100 OKB（mock 不区分首次/补充，统一要求）
    if (amount < 100) { sendErr(res, 1001, "first stake amount must be >= 100 OKB"); return; }
    console.log(`[mock-api] staking/stake: agentId=${agentId} amount=${amountStr} OKB`);
    sendOk(res, { uopData: mockUopData() }); return;
  }

  // Provider 主动拉取推荐的 Public 任务（必须在 /api/v1/task/:jobId/match 之前匹配）
  if (method === "POST" && path_ === "/api/v1/task/job/match") {
    const openPublic = [...tasks.values()].filter(
      (t) => t.status === S_OPEN && t.openType === 1,
    );
    const picks = openPublic.slice(0, 5).map((t) => ({
      jobId: t.jobId,
      title: t.title,
      description: t.description,
      tokenAddress: t.tokenAddress,
      tokenAmount: t.tokenAmount,
      minCreditScore: t.minCreditScore ?? 0,
      createTime: t.createTime,
    }));
    sendOk(res, { tasks: picks });
    return;
  }

  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/match", path_))) {
    if (!tasks.has(m.jobId)) { sendErr(res, 2001, "task not found"); return; }
    sendOk(res, { recommendations: [
      { providerAddress: "0xSeller000000000000000000000000000000001", providerAgentId: "mock-seller-agent-001", matchScore: 92.5, creditScore: 88, capabilitySummary: "专注 Solidity 审计和 DeFi 协议开发，完成率 96%", completedTaskCount: 42 },
      { providerAddress: "0xSeller000000000000000000000000000000002", providerAgentId: "mock-seller-agent-002", matchScore: 85.0, creditScore: 79, capabilitySummary: "全栈区块链开发，擅长 Rust 和 EVM 合约", completedTaskCount: 18 },
    ] }); return;
  }

  // ── UI notify endpoints ────────────────────────────────────────────────────
  if (method === "POST" && (m = matchPath("/ui/notify/confirmed/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    notifyConfirmed(m.jobId, t.buyerAgentAddress).then(() => console.log(`[mock-api] manual job_created sent for job=${m!.jobId}`)).catch(console.error);
    sendOk(res, { triggered: "job_created", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/applied/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    notifyApplied(m.jobId, t.buyerAgentAddress, t.buyerAgentId, "mock-seller-agent-001", "0xSeller000000000000000000000000000000001", "100")
      .then(() => console.log(`[mock-api] manual provider_applied sent`)).catch(console.error);
    sendOk(res, { triggered: "provider_applied", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/accepted/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    notifyAccepted(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si).catch(console.error);
    sendOk(res, { triggered: "job_accepted", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/submitted/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const deliverable = String(body.deliverable ?? `https://mock-deliverable.example.com/${m.jobId}.html`);
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    notifySubmitted(m.jobId, t.buyerAgentAddress, t.buyerAgentId, si, sa, deliverable).catch(console.error);
    sendOk(res, { triggered: "job_submitted", jobId: m.jobId, deliverable }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/disputed/:jobId", path_))) {
    const jobId = m.jobId;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    // 若无现有 dispute 记录,创建一条(方便 UI 手动触发场景)
    const existingRounds = [...disputes.values()].filter(d => d.jobId === jobId).length;
    const round = existingRounds + 1;
    const disputeId = `d-${jobId}-r${round}`;
    if (!disputes.has(disputeId)) {
      disputes.set(disputeId, {
        disputeId, jobId, round,
        clientReason: "手动触发:买家拒绝验收",
        providerReason: "手动触发仲裁通知",
        qualityStandards: t.description.split("验收标准：")[1] ?? "未明确验收标准",
        deliverableUrl: `https://mock-deliverable.example.com/${jobId}.html`,
        evidences: [],
        voterCommits: {},
        votes: [],
        verdict: null,
        createTime: nowIso(),
        commitPhaseStartedAt: null,
        resolvedAt: null,
      });
    }
    notifyDisputed(jobId, disputeId, t.buyerAgentAddress, t.buyerAgentId, sa, si, "手动触发仲裁通知").catch(console.error);
    sendOk(res, { triggered: "TASK_DISPUTED", jobId, disputeId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/completed/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    notifyCompleted(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si).catch(console.error);
    sendOk(res, { triggered: "job_completed", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/resolved/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const winner = String(body.winner ?? "provider");
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    notifyArbitrationResult(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si, winner).catch(console.error);
    sendOk(res, { triggered: "dispute_resolved", jobId: m.jobId, winner }); return;
  }

  // ── x402 pay (mock) ──────────────────────────────────────────────────────
  if (method === "POST" && path_ === "/api/v1/x402/pay") {
    const body = await parseBody(req) as Record<string, unknown>;
    const jobId = String(body.jobId ?? "");
    const endpoint = String(body.endpoint ?? "");
    const amount = Number(body.amount ?? 0);
    if (!jobId) { sendErr(res, 4000, "missing jobId"); return; }
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    t.status = S_SUBMITTED; t.statusStr = "submitted"; t.updateTime = new Date().toISOString();
    saveTasks();
    console.log(`[mock-api] x402 pay: jobId=${jobId} endpoint=${endpoint} amount=${amount} → status=submitted`);
    sendOk(res, { jobId, endpoint, amount, receipt: `x402-receipt-${Date.now()}`, status: "paid" });
    return;
  }

  // 未匹配任何 mock 路由 —— 转发到真实后端（UPSTREAM_URL）
  proxyToUpstream(req, res, originalPath, url.search);
});

// ── Seed tasks ────────────────────────────────────────────────────────────────
function seedTasks() {
  const seeds: Task[] = [
    { jobId: "task-001", title: "Solidity 合约安全审计", description: "审计目标合约地址 0xABC123...，重点检查重入攻击、权限控制和整数溢出漏洞。要求提交详细的审计报告，包含风险评级和修复建议。", descriptionSummary: "EVM 合约安全审计，重点重入攻击和权限控制检查", tokenAddress: "0xUSDT0000000000000000000000000000000001", tokenAmount: "500", paymentType: 0, openType: 1, status: S_OPEN, statusStr: "open", chainId: 196, minCreditScore: 70, designatedProvider: null, buyerAgentAddress: "0xMockBuyer00000000000000000000000000001", buyerAgentId: "mock-buyer-agent-001", providerAgentAddress: null, providerAgentId: null, groupId: null, evaluatorAddress: null, expireConfig: { openExpireSec: 172800, acceptedExpireSec: 604800 }, createTime: "2026-04-15T08:00:00Z", updateTime: "2026-04-15T08:00:00Z" },
    { jobId: "task-002", title: "DEX 套利机器人开发", description: "开发跨链 DEX 套利机器人，支持 Uniswap V3 和 PancakeSwap，使用 Rust 实现。要求完整的回测报告、单元测试和部署文档。", descriptionSummary: "Rust DEX 套利机器人，支持 Uni V3 和 PCS", tokenAddress: "0xUSDT0000000000000000000000000000000001", tokenAmount: "2000", paymentType: 0, openType: 1, status: S_OPEN, statusStr: "open", chainId: 196, minCreditScore: 80, designatedProvider: null, buyerAgentAddress: "0xMockBuyer00000000000000000000000000001", buyerAgentId: "mock-buyer-agent-001", providerAgentAddress: null, providerAgentId: null, groupId: null, evaluatorAddress: null, expireConfig: { openExpireSec: 172800, acceptedExpireSec: 604800 }, createTime: "2026-04-15T09:00:00Z", updateTime: "2026-04-15T09:00:00Z" },
    { jobId: "task-003", title: "XLayer 链上数据索引服务", description: "为 XLayer 构建链上事件索引服务，监听指定合约的 Transfer/Swap 事件，写入 PostgreSQL，并提供 REST API 查询接口。", descriptionSummary: "XLayer 事件索引 + REST API，支持历史回扫", tokenAddress: "0xUSDT0000000000000000000000000000000001", tokenAmount: "800", paymentType: 0, openType: 1, status: S_OPEN, statusStr: "open", chainId: 196, minCreditScore: 60, designatedProvider: null, buyerAgentAddress: "0xMockBuyer00000000000000000000000000002", buyerAgentId: "mock-buyer-agent-002", providerAgentAddress: null, providerAgentId: null, groupId: null, evaluatorAddress: null, expireConfig: { openExpireSec: 259200, acceptedExpireSec: 432000 }, createTime: "2026-04-15T10:00:00Z", updateTime: "2026-04-15T10:00:00Z" },
  ];
  for (const t of seeds) { if (!tasks.has(t.jobId)) tasks.set(t.jobId, t); }
}

// ── Start ─────────────────────────────────────────────────────────────────────
loadTasks();
seedTasks();

server.listen(API_PORT, "127.0.0.1", () => {
  console.log(`[mock-api] HTTP server listening on http://127.0.0.1:${API_PORT}`);
  console.log(`[mock-api] task db: ${PERSIST_PATH}`);
  console.log(`[mock-api] 已预置示例任务: task-001 (合约审计), task-002 (套利机器人), task-003 (链上索引)`);
});

// ── Dashboard HTML ─────────────────────────────────────────────────────────────
const DASHBOARD_HTML = `<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<title>Mock API Dashboard</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:monospace;background:#0d1117;color:#c9d1d9;padding:16px;font-size:13px}
h1{color:#58a6ff;font-size:1.1em;margin-bottom:16px;display:flex;align-items:center;gap:8px}
h2{color:#8b949e;font-size:0.9em;text-transform:uppercase;letter-spacing:.08em;margin:16px 0 8px}
.grid{display:grid;grid-template-columns:1fr 340px;gap:16px}
table{width:100%;border-collapse:collapse;font-size:12px}
th{background:#161b22;color:#58a6ff;padding:6px 8px;text-align:left;border-bottom:1px solid #30363d}
td{padding:5px 8px;border-bottom:1px solid #21262d;vertical-align:middle}
tr:hover td{background:#161b22}
.badge{padding:1px 7px;border-radius:10px;font-size:11px;white-space:nowrap}
.s-open{background:#1c3a4a;color:#79c0ff}.s-accepted{background:#12372a;color:#3fb950}
.s-submitted{background:#3a2d00;color:#e3b341}.s-complete{background:#0d2818;color:#56d364}
.s-close{background:#282828;color:#8b949e}.s-refused{background:#3a1a1a;color:#f85149}
.s-disputed{background:#3a1c00;color:#ffa657}.s-init{background:#1c1c2c;color:#8b949e}
.btn{cursor:pointer;padding:2px 8px;border:1px solid #30363d;border-radius:4px;font-size:11px;
  font-family:monospace;background:#21262d;color:#c9d1d9;transition:background .15s}
.btn:hover{background:#30363d}.btn:disabled{opacity:.4;cursor:default}
.btn-confirm{border-color:#1f6feb;color:#58a6ff}.btn-confirm:hover{background:#1c2e4a}
.btn-applied{border-color:#388bfd;color:#79c0ff}.btn-applied:hover{background:#1a2f4a}
.btn-accept{border-color:#238636;color:#3fb950}.btn-accept:hover{background:#12341e}
.btn-submit{border-color:#bb8009;color:#e3b341}.btn-submit:hover{background:#2d2000}
.btn-complete{border-color:#1a7f37;color:#56d364}.btn-complete:hover{background:#0d1f12}
.btn-dispute{border-color:#bb5500;color:#ffa657}.btn-dispute:hover{background:#2d1800}
.btn-reset{border-color:#f85149;color:#ff7b72}.btn-reset:hover{background:#3a1a1a}
.panel{background:#161b22;border:1px solid #30363d;border-radius:6px;padding:12px}
.panel h2{margin-top:0}
.api-list{list-style:none}
.api-list li{padding:3px 0;display:flex;gap:6px;align-items:baseline}
.method{font-weight:bold;min-width:36px;font-size:11px}
.get{color:#3fb950}.post{color:#ffa657}.delete{color:#f85149}
.path{color:#8b949e;word-break:break-all}
.log-grid{margin-top:16px}
.log-box{background:#0d1117;border:1px solid #21262d;border-radius:4px;padding:8px;
  max-height:420px;overflow-y:auto;font-size:11px}
.log-row{padding:2px 0;color:#8b949e;border-bottom:1px solid #161b22;display:flex;gap:6px;flex-wrap:wrap}
.log-row .ts{color:#484f58;min-width:70px}
.log-row .tag{padding:0 4px;border-radius:3px;font-size:10px;font-weight:bold}
.log-row .t-get{background:#0d2818;color:#3fb950}.log-row .t-post{background:#2d1800;color:#ffa657}
.log-row .t-delete{background:#3a1a1a;color:#f85149}
.log-row .t-ws{background:#1c1c3a;color:#bc8cff}
.log-row .job{color:#79c0ff}.log-row .detail{color:#8b949e}
.log-row .s-ok{color:#3fb950}.log-row .s-err{color:#f85149}
.log-row.clickable{cursor:pointer}.log-row.clickable:hover{background:#1c2230}
.job-clickable{cursor:pointer;text-decoration:underline dotted}
.job-clickable:hover{color:#a5d6ff}
.filter-bar{display:flex;align-items:center;gap:8px;margin-bottom:8px;font-size:11px}
.filter-bar .clear-btn{cursor:pointer;background:#21262d;border:1px solid #30363d;border-radius:4px;
  color:#c9d1d9;padding:2px 8px;font-family:monospace;font-size:11px}
.filter-bar .clear-btn:hover{background:#30363d}
.modal-overlay{display:none;position:fixed;inset:0;background:rgba(0,0,0,.7);z-index:100;align-items:center;justify-content:center}
.modal-overlay.show{display:flex}
.modal{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:16px;max-width:720px;width:92%;max-height:82vh;overflow-y:auto}
.modal h3{color:#58a6ff;font-size:13px;margin-bottom:12px;display:flex;justify-content:space-between;align-items:center}
.modal pre{background:#0d1117;border:1px solid #21262d;border-radius:4px;padding:10px;font-size:11px;overflow-x:auto;white-space:pre-wrap;word-break:break-all;color:#c9d1d9;margin-bottom:10px;max-height:300px;overflow-y:auto}
.modal label{color:#8b949e;font-size:10px;text-transform:uppercase;letter-spacing:.05em;display:block;margin-bottom:4px;margin-top:8px}
.modal .close-btn{cursor:pointer;background:none;border:1px solid #30363d;border-radius:4px;color:#8b949e;padding:2px 10px;font-family:monospace;font-size:11px}
.modal .close-btn:hover{color:#c9d1d9;border-color:#58a6ff}
.modal .meta{color:#58a6ff;font-size:12px;margin-bottom:4px}
.status-bar{display:flex;gap:16px;font-size:11px;color:#8b949e;margin-bottom:12px}
.status-bar span{display:flex;align-items:center;gap:4px}
.dot{width:7px;height:7px;border-radius:50%;background:#3fb950}
#tasks-count{color:#58a6ff}
</style>
</head>
<body>
<h1>🔧 Mock API Dashboard <span style="font-size:.75em;color:#8b949e">http://127.0.0.1:9001</span></h1>
<div class="status-bar">
  <span><span class="dot" id="api-dot"></span>mock-api :9001</span>
  <span><span class="dot" id="ws-dot" style="background:#e3b341"></span>ws-mock :9000</span>
  <span>Tasks: <span id="tasks-count">-</span></span>
  <span style="margin-left:auto"><button class="btn btn-reset" onclick="resetAll()">🗑 Reset All Tasks</button></span>
</div>
<div class="grid">
<div>
  <h2>任务列表</h2>
  <table id="task-table">
    <thead><tr>
      <th>JobId</th><th>Title</th><th>Status</th><th>Buyer</th><th>Provider</th><th>通知操作</th>
    </tr></thead>
    <tbody id="task-body"><tr><td colspan="6" style="color:#8b949e;text-align:center">加载中...</td></tr></tbody>
  </table>
</div>
<div>
  <div class="panel" style="margin-bottom:12px">
    <h2>API 接口</h2>
    <ul class="api-list">
      <li><span class="method post">POST</span><span class="path">/api/v1/task/create</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/apply</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/accept</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/submit</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/complete</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/refuse</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/dispute</span></li>
      <li><span class="method get">GET</span><span class="path">/api/v1/task/:id/evidence</span></li>
      <li><span class="method get">GET</span><span class="path">/api/v1/task/:id/evidence/download?name=</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/vote/commit</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/vote/reveal</span></li>
      <li><span class="method get">GET</span><span class="path">/api/v1/task/:id/vote/canReveal?voter=</span></li>
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/match</span></li>
      <li><span class="method get">GET</span><span class="path">/api/v1/task/:id</span></li>
      <li><span class="method get">GET</span><span class="path">/api/v1/tasks/my</span></li>
      <li><span class="method get">GET</span><span class="path">/api/v1/task/list</span></li>
      <li><span class="method delete">DEL</span><span class="path">/api/v1/reset</span></li>
      <li style="margin-top:8px;border-top:1px solid #30363d;padding-top:8px">
        <span class="method post" style="color:#ff7b72">POST</span><span class="path">/ui/notify/confirmed/:id</span>
      </li>
      <li><span class="method post" style="color:#ff7b72">POST</span><span class="path">/ui/notify/applied/:id</span></li>
      <li><span class="method post" style="color:#ff7b72">POST</span><span class="path">/ui/notify/accepted/:id</span></li>
      <li><span class="method post" style="color:#ff7b72">POST</span><span class="path">/ui/notify/submitted/:id</span></li>
      <li><span class="method post" style="color:#ff7b72">POST</span><span class="path">/ui/notify/disputed/:id</span></li>
    </ul>
  </div>
  </div>
</div>
<div class="log-grid">
  <div class="panel">
    <h2>事件记录（API 请求 + WS 系统通知）</h2>
    <div class="filter-bar">
      <span>过滤：</span>
      <span id="filter-label" style="color:#79c0ff">全部</span>
      <button class="clear-btn" id="clear-filter-btn" onclick="clearFilter()" style="display:none">✕ 清除过滤</button>
    </div>
    <div id="event-log" class="log-box"></div>
  </div>
</div>
<div id="detail-modal" class="modal-overlay" onclick="if(event.target===this)this.classList.remove('show')">
  <div class="modal">
    <h3><span id="detail-title">请求详情</span><button class="close-btn" onclick="document.getElementById('detail-modal').classList.remove('show')">ESC</button></h3>
    <div id="detail-content"></div>
  </div>
</div>
<script>
document.addEventListener('keydown',e=>{if(e.key==='Escape')document.getElementById('detail-modal').classList.remove('show');});
let allEvents=[], jobFilter=null;
function fmtTs(iso) { if(!iso) return ''; try { const d=new Date(iso); return [d.getHours(),d.getMinutes(),d.getSeconds()].map(n=>String(n).padStart(2,'0')).join(':'); } catch(e) { return iso; } }
function renderEventRow(e, i) {
  if (e.kind === 'api') {
    const m = (e.method||'GET').toUpperCase();
    const cls = m==='GET'?'t-get':m==='POST'?'t-post':'t-delete';
    const scode = (e.status||0) < 400 ? 's-ok' : 's-err';
    const hasBody = e.reqBody || e.resBody;
    const agent = e.agentId ? \`<span style="color:#d2a8ff">\${e.agentId}</span>\` : '';
    return \`<div class="log-row\${hasBody?' clickable':''}" \${hasBody?'onclick="showDetail('+i+')"':''}><span class="ts">\${fmtTs(e.ts)}</span><span class="tag \${cls}">\${m}</span><span class="\${scode}">\${e.status}</span><span class="job">\${e.jobId||''}</span>\${agent}<span class="detail">\${e.path||''}</span></div>\`;
  } else {
    const hasPayload = !!e.wsPayload;
    return \`<div class="log-row\${hasPayload?' clickable':''}" \${hasPayload?'onclick="showDetail('+i+')"':''}><span class="ts">\${fmtTs(e.ts)}</span><span class="tag t-ws">\${e.wsType||'?'}</span><span class="job">\${e.jobId||''}</span><span class="detail">\${e.detail||''}</span></div>\`;
  }
}
function esc(s) { const d=document.createElement('div');d.textContent=s;return d.innerHTML; }
function showDetail(index) {
  const e = allEvents[index];
  if(!e) return;
  const modal = document.getElementById('detail-modal');
  const title = document.getElementById('detail-title');
  const content = document.getElementById('detail-content');
  let html = '';
  if (e.kind === 'api') {
    title.textContent = \`\${e.method} \${e.path}  [\${e.status}]\`;
    html += \`<div class="meta">\${e.ts}  |  jobId: \${e.jobId||'-'}</div>\`;
    if(e.reqBody && Object.keys(e.reqBody).length) {
      html += \`<label>Request Body</label><pre>\${esc(JSON.stringify(e.reqBody,null,2))}</pre>\`;
    }
    html += \`<label>Response Body</label><pre>\${esc(JSON.stringify(e.resBody,null,2))}</pre>\`;
  } else {
    title.textContent = \`WS: \${e.wsType}\`;
    html += \`<div class="meta">\${e.ts}  |  jobId: \${e.jobId||'-'}  |  conv: \${e.convId||'-'}</div>\`;
    html += \`<label>Detail</label><pre>\${esc(e.detail||'-')}</pre>\`;
    if(e.wsPayload) {
      html += \`<label>WS Payload</label><pre>\${esc(JSON.stringify(e.wsPayload,null,2))}</pre>\`;
    }
  }
  content.innerHTML = html;
  modal.classList.add('show');
}
function renderEventLog() {
  const filtered = jobFilter ? allEvents.filter(e => e.jobId === jobFilter) : allEvents;
  const el = document.getElementById('event-log');
  el.innerHTML = filtered.length
    ? filtered.map((e,i) => renderEventRow(e, allEvents.indexOf(e))).join('')
    : '<div class="log-row">暂无记录</div>';
}
function filterByJob(jobId) {
  jobFilter = jobId;
  document.getElementById('filter-label').textContent = 'jobId = ' + jobId;
  document.getElementById('filter-label').style.color = '#79c0ff';
  document.getElementById('clear-filter-btn').style.display = 'inline-block';
  renderEventLog();
}
function clearFilter() {
  jobFilter = null;
  document.getElementById('filter-label').textContent = '全部';
  document.getElementById('clear-filter-btn').style.display = 'none';
  renderEventLog();
}
async function loadLogs() {
  try {
    const [apiRes, wsRes] = await Promise.all([
      fetch('/api/v1/logs?kind=api&limit=50'),
      fetch('/api/v1/logs?kind=ws&limit=50')
    ]);
    const apiData = await apiRes.json();
    const wsData = await wsRes.json();
    const apiLogs = (apiData.data?.logs || []).map(e => ({...e, kind: 'api'}));
    const wsLogs = (wsData.data?.logs || []).map(e => ({...e, kind: 'ws'}));
    allEvents = [...apiLogs, ...wsLogs].sort((a,b) => (b.ts||'').localeCompare(a.ts||''));
    renderEventLog();
  } catch(e) {}
}
const statusBadge = s => {
  const cls = {'open':'s-open','accepted':'s-accepted','submitted':'s-submitted',
    'complete':'s-complete','close':'s-close','refused':'s-refused',
    'disputed':'s-disputed','init':'s-init'}[s] || 's-init';
  return \`<span class="badge \${cls}">\${s}</span>\`;
};
const actionBtns = (jobId, status) => {
  const b = (cls, label, fn) => \`<button class="btn \${cls}" onclick="\${fn}('\${jobId}')">\${label}</button>\`;
  const btns = [];
  if (status === 'open') {
    btns.push(b('btn-confirm','📡 Confirmed', 'sendConfirmed'));
    btns.push(b('btn-applied','📬 Applied', 'sendApplied'));
    btns.push(b('btn-accept','✅ Accepted', 'sendAccepted'));
  }
  if (status === 'accepted') btns.push(b('btn-submit','📦 Submitted', 'sendSubmitted'));
  if (status === 'refused')  btns.push(b('btn-dispute','⚖️ Disputed', 'sendDisputed'));
  return btns.join(' ') || '<span style="color:#6e7681">-</span>';
};
async function loadTasks() {
  try {
    const res = await fetch('/api/v1/tasks/all');
    if (!res.ok) throw new Error(res.status);
    const data = await res.json();
    const tasks = data.data?.list || [];
    document.getElementById('tasks-count').textContent = tasks.length;
    document.getElementById('api-dot').style.background = '#3fb950';
    const tbody = document.getElementById('task-body');
    if (!tasks.length) { tbody.innerHTML = '<tr><td colspan="6" style="color:#8b949e;text-align:center">暂无任务</td></tr>'; return; }
    tbody.innerHTML = tasks.map(t => \`<tr>
      <td><code class="job-clickable" style="color:#79c0ff" onclick="filterByJob('\${t.jobId}')" title="点击过滤该 jobId 的记录">\${t.jobId}</code></td>
      <td style="max-width:160px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="\${t.title}">\${t.title}</td>
      <td>\${statusBadge(t.statusStr)}</td>
      <td style="color:#8b949e;font-size:11px">\${t.buyerAgentId}</td>
      <td style="color:#8b949e;font-size:11px">\${t.providerAgentId||'-'}</td>
      <td>\${actionBtns(t.jobId, t.statusStr)}</td>
    </tr>\`).join('');
  } catch(e) { document.getElementById('api-dot').style.background = '#f85149'; }
}
async function uiNotify(type, jobId, body={}) {
  try {
    const res = await fetch(\`/ui/notify/\${type}/\${jobId}\`, {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    const data = await res.json();
    loadTasks(); loadLogs();
  } catch(e) {}
}
const sendConfirmed = id => uiNotify('confirmed', id);
const sendApplied   = id => uiNotify('applied',   id);
const sendAccepted  = id => uiNotify('accepted',  id);
const sendSubmitted = id => uiNotify('submitted', id, {deliverable:\`https://mock-deliverable.example.com/\${id}.html\`});
const sendDisputed  = id => uiNotify('disputed',  id);
async function resetAll() {
  if (!confirm('确认重置所有任务？')) return;
  const res = await fetch('/api/v1/reset', {method:'DELETE'});
  await res.json();
  loadTasks(); loadLogs();
}
loadTasks(); loadLogs();
setInterval(() => { loadTasks(); loadLogs(); }, 3000);
</script>
</body>
</html>`;
