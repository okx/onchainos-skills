/**
 * Mock API Server — TypeScript port of mock_api.rs
 * Port: 9001  Dashboard: http://127.0.0.1:9001
 */
import http from "node:http";
import fs   from "node:fs";
import path from "node:path";
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

const tasks    = new Map<string, Task>();
const confirms = new Map<string, ProviderConfirm[]>();

// Dispute store (by disputeId)
interface DisputeEvidence {
  submitter: string;
  type: string;
  summary: string;
  fileUrl?: string;
}
interface DisputeRecord {
  disputeId: string;
  jobId: string;
  statusStr: string;
  raiserAddress: string;
  reason: string;
  createTime: string;
  evidences: DisputeEvidence[];
}
const disputes = new Map<string, DisputeRecord>();
let disputeCounter = 1;

// ── Persistence ───────────────────────────────────────────────────────────────
const PERSIST_PATH = process.env.MOCK_API_DB ??
  path.join(path.dirname(new URL(import.meta.url).pathname), "mock-tasks.json");

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
  for (const k of tasks.keys()) {
    const n = parseInt(k, 16) || 0;
    if (n > jobCounter) jobCounter = n;
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────
let jobCounter = 1000;
const genJobId   = () => `0x${(++jobCounter).toString(16)}`;
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
    type: "TASK_CONFIRMED", jobId,
    content: `系统通知：任务 ${jobId} 已上链确认，状态变为 open。`,
    llm: `[TASK_CONFIRMED] 任务 ${jobId} 已上链确认，状态 open。按 client.md Scene 0/1 处理（作为买家：寻找卖家并发起协商）。`,
  });
}

async function notifyApplied(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                              sellerAgentId: string, sellerCommAddr: string, tokenAmount: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "TASK_APPLIED", jobId, sellerAgentId, tokenAmount,
    content: `系统通知：卖家 ${sellerAgentId} 已申请接单（TASK_APPLIED），报价 ${tokenAmount} USDT，jobId=${jobId}。`,
    llm: `[TASK_APPLIED] 卖家 ${sellerAgentId} 已申请接单，报价 ${tokenAmount} USDT，jobId=${jobId}。按 client.md Scene 3 处理（作为买家：确认接单）。`,
  });
}

async function notifyAccepted(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                               sellerCommAddr: string, sellerAgentId: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "TASK_ACCEPTED", jobId, sellerAgentId,
    content: `系统通知：任务 ${jobId} 已接单确认（TASK_ACCEPTED），卖家 ${sellerAgentId}，资金已进入托管。`,
    llm: `[TASK_ACCEPTED] 任务 ${jobId} 已被买家确认接单，资金已托管。按 provider.md Scene 4 处理（作为卖家：执行任务并交付）。`,
  });
}

async function notifySubmitted(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                                sellerAgentId: string, sellerCommAddr: string, deliverable: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "TASK_SUBMITTED", jobId, deliverable,
    content: `系统通知：任务 ${jobId} 交付物已上链（TASK_SUBMITTED），交付物：${deliverable}。`,
    llm: `[TASK_SUBMITTED] 卖家 ${sellerAgentId} 已提交交付物（jobId: ${jobId}），交付物：${deliverable}。按 client.md Scene 5 处理（作为买家：验收交付物）。`,
  });
}

async function notifyRefused(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                              sellerCommAddr: string, sellerAgentId: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "TASK_REFUSED", jobId, buyerAgentId,
    content: `系统通知：买家拒绝了交付物（TASK_REFUSED），jobId=${jobId}。卖家可在 24 小时内发起仲裁，否则资金退还买家。`,
    llm: `TASK_REFUSED jobId=${jobId}`,
  });
}

async function notifyDisputed(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                               sellerCommAddr: string, sellerAgentId: string, reason: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  const evaluators = await lookupRoleAddrs("EVALUATOR");
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr, ...evaluators], {
    type: "TASK_DISPUTED", jobId, buyerAgentId, sellerAgentId, reason,
    content: `系统通知：任务 ${jobId} 进入仲裁（TASK_DISPUTED）。卖家申诉理由：${reason}。`,
  });
}

async function notifyCompleted(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                               sellerCommAddr: string, sellerAgentId: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "TASK_COMPLETED", jobId, sellerAgentId,
    content: `系统通知：任务 ${jobId} 已验收通过（TASK_COMPLETED），资金已释放给卖家 ${sellerAgentId}。`,
    llm: `[Scene 7] 任务 ${jobId} 买家已验收通过（TASK_COMPLETED），资金已释放。任务圆满完成。`,
  });
}

async function notifyRejected(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                              sellerCommAddr: string, sellerAgentId: string, reason: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr], {
    type: "TASK_REJECTED", jobId, sellerAgentId, reason,
    content: `系统通知：任务 ${jobId} 已终止（TASK_REJECTED），原因：${reason}。资金已退还买家。`,
    llm: `[系统通知] 任务 ${jobId} 已终止（TASK_REJECTED），原因：${reason}。资金已退还买家。任务结束。`,
  });
}

async function notifyArbitrationResult(jobId: string, buyerCommAddr: string, buyerAgentId: string,
                                        sellerCommAddr: string, sellerAgentId: string, winner: string) {
  const convId = `conv-${jobId}-${buyerAgentId}-${sellerAgentId}`;
  const evaluators = await lookupRoleAddrs("EVALUATOR");
  if (winner === "provider") {
    await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr, ...evaluators], {
      type: "TASK_COMPLETED", jobId, sellerAgentId, buyerAgentId, arbitration: true,
      content: `系统通知：任务 ${jobId} 仲裁完成，卖家 ${sellerAgentId} 胜诉（TASK_COMPLETED）。资金判给卖家。`,
      llm: `[系统通知] 仲裁结果：卖家胜诉（TASK_COMPLETED），任务 ${jobId} 资金已释放给卖家。`,
    });
  } else {
    await wsNotify(convId, [CHAIN_ADDR, buyerCommAddr, sellerCommAddr, ...evaluators], {
      type: "TASK_REJECTED", jobId, sellerAgentId, buyerAgentId, arbitration: true,
      content: `系统通知：任务 ${jobId} 仲裁完成，买家 ${buyerAgentId} 胜诉（TASK_REJECTED）。资金已退还买家。`,
      llm: `[系统通知] 仲裁结果：买家胜诉（TASK_REJECTED），任务 ${jobId} 资金已退还买家。任务结束。`,
    });
  }
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
      console.log(`[mock-api] sending TASK_CONFIRMED for job=${jobId}`);
      await notifyConfirmed(jobId, buyerAddr).catch(e => console.error("[mock-api] confirmed error:", e));
      console.log(`[mock-api] TASK_CONFIRMED sent for job=${jobId}`);
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
    setStatus(t, S_ACCEPTED);
    console.log(`[mock-api] task accepted: job=${jobId} provider=${t.providerAgentAddress}`);
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
    setStatus(t, S_SUBMITTED);
    console.log(`[mock-api] task submitted: job=${jobId}`);
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
    setStatus(t, S_COMPLETE); saveTasks();
    console.log(`[mock-api] task completed: job=${jobId}`);
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
    setStatus(t, S_REFUSED); saveTasks();
    console.log(`[mock-api] task refused: job=${jobId}`);
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
    setStatus(t, S_CLOSE); saveTasks();
    console.log(`[mock-api] task closed: job=${jobId}`);
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
    setStatus(t, S_DISPUTED); saveTasks();
    // Create dispute record
    const disputeId = `dispute-${disputeCounter++}`;
    const raiser = String(req.headers["x-wallet-address"] ?? t.providerAgentAddress ?? "");
    disputes.set(disputeId, {
      disputeId, jobId,
      statusStr: "disputed",
      raiserAddress: raiser,
      reason, createTime: nowIso(),
      evidences: [],
    });
    console.log(`[mock-api] task disputed: job=${jobId} disputeId=${disputeId} reason=${reason}`);
    const pid2 = t.providerAgentId ?? "mock-seller-agent-001";
    (async () => {
      const sellerComm = await lookupCommAddr(pid2) ?? t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
      await notifyDisputed(jobId, t.buyerAgentAddress, t.buyerAgentId, sellerComm, pid2, reason);
    })().catch(e => console.error("[mock-api] dispute notify error:", e));
    sendOk(res, { uopData: mockUopData() }); return;
  }
  if (method === "POST" && (m = matchPath("/api/v1/task/:jobId/resolve", path_))) {
    const { jobId } = m;
    const t = tasks.get(jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    if (t.status !== S_DISPUTED) { sendErr(res, 2002, "task status must be DISPUTED"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const winner = String(body.winner ?? "buyer");
    const finalStatus = winner === "provider" ? S_COMPLETE : 6 /* REJECTED */;
    setStatus(t, finalStatus); saveTasks();
    console.log(`[mock-api] task resolved: job=${jobId} winner=${winner} status=${STATUS_STR[finalStatus]}`);
    const { buyerAgentAddress: ba3, buyerAgentId: bi3, providerAgentId: pi3 } = t;
    setTimeout(async () => {
      const sellerComm = await lookupCommAddr(pi3!) ?? t.providerAgentAddress!;
      await notifyArbitrationResult(jobId, ba3, bi3, sellerComm, pi3!, winner).catch(e => console.error("[mock-api] resolve notify error:", e));
    }, 3000);
    sendOk(res, { uopData: mockUopData(), winner }); return;
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
    const dispute = [...disputes.values()].filter(d => d.jobId === jobId).pop();
    if (dispute) {
      if (text) dispute.evidences.push({ submitter, type: "text", summary: text });
      for (const f of imageFiles) {
        dispute.evidences.push({ submitter, type: "image", summary: `(image) ${f}`, fileUrl: `https://mock-cdn.example.com/evidence/${jobId}/${f}` });
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
    // Append to latest dispute for this job
    const dispute = [...disputes.values()].filter(d => d.jobId === jobId).pop();
    if (dispute) {
      dispute.evidences.push({ submitter, type: "text", summary: text });
    }
    console.log(`[mock-api] evidence uploaded: job=${jobId} text="${text.slice(0, 80)}"`);
    sendOk(res, { uopData: mockUopData() }); return;
  }
  // ── Broadcast (CLI task_sign_and_broadcast final step) ────────────────────
  if (method === "POST" && path_ === "/api/v1/task/broadcast") {
    // CLI sends { signedTx } or { uopHash, signature } — we mock the txHash
    sendOk(res, [{ txHash: mockUop() }]); return;
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
    notifyConfirmed(m.jobId, t.buyerAgentAddress).then(() => console.log(`[mock-api] manual TASK_CONFIRMED sent for job=${m!.jobId}`)).catch(console.error);
    sendOk(res, { triggered: "TASK_CONFIRMED", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/applied/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    notifyApplied(m.jobId, t.buyerAgentAddress, t.buyerAgentId, "mock-seller-agent-001", "0xSeller000000000000000000000000000000001", "100")
      .then(() => console.log(`[mock-api] manual TASK_APPLIED sent`)).catch(console.error);
    sendOk(res, { triggered: "TASK_APPLIED", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/accepted/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    notifyAccepted(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si).catch(console.error);
    sendOk(res, { triggered: "TASK_ACCEPTED", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/submitted/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const deliverable = String(body.deliverable ?? `https://mock-deliverable.example.com/${m.jobId}.html`);
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    notifySubmitted(m.jobId, t.buyerAgentAddress, t.buyerAgentId, si, sa, deliverable).catch(console.error);
    sendOk(res, { triggered: "TASK_SUBMITTED", jobId: m.jobId, deliverable }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/disputed/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    notifyDisputed(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si, "手动触发仲裁通知").catch(console.error);
    sendOk(res, { triggered: "TASK_DISPUTED", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/completed/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    notifyCompleted(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si).catch(console.error);
    sendOk(res, { triggered: "TASK_COMPLETED", jobId: m.jobId }); return;
  }
  if (method === "POST" && (m = matchPath("/ui/notify/resolved/:jobId", path_))) {
    const t = tasks.get(m.jobId);
    if (!t) { sendErr(res, 2001, "task not found"); return; }
    const body = await parseBody(req) as Record<string, unknown>;
    const winner = String(body.winner ?? "provider");
    const si = t.providerAgentId ?? "mock-seller-agent-001";
    const sa = t.providerAgentAddress ?? "0xSeller000000000000000000000000000000001";
    notifyArbitrationResult(m.jobId, t.buyerAgentAddress, t.buyerAgentId, sa, si, winner).catch(console.error);
    sendOk(res, { triggered: winner === "provider" ? "TASK_COMPLETED" : "TASK_REJECTED", jobId: m.jobId, winner }); return;
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

  res.writeHead(404); res.end("not found");
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
      <li><span class="method post">POST</span><span class="path">/api/v1/task/:id/resolve</span></li>
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
