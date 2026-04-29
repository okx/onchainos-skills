/**
 * mock-evaluator-ui: 带 Web 界面的仲裁员 mock
 *
 * 启动后打开 http://localhost:9004
 *
 * 架构：每个 convId 一个 EvalSession（TASK_DISPUTED 登记 → evaluator_selected 触发 commit → reveal_started 触发 reveal）
 * mock-api 在最后一个 reveal 后广播 dispute_resolved + reward_claimed 系统通知（事件名对齐 Lark 设计文档）。
 * 自动模式：5s 后自动裁决
 * 手动模式：UI 点击"买家胜"/"卖家胜"按钮
 *
 * 用法:
 *   npm run ui
 */
import http from "node:http";
import { WsMockClient, WsEnvelope, TaskPayload } from "./ws-client.js";

const EVAL_COMM_ADDR = "0xEvaluator00000000000000000000000000001";
const EVAL_AGENT_ID  = "mock-evaluator-agent-001";
const WS_URL        = "ws://127.0.0.1:9000";
const API_BASE_URL  = "http://127.0.0.1:9001";
const UI_PORT       = 9004;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ── 状态 ─────────────────────────────────────────────────────────────────────
interface EvalMessage {
  from: "system" | "eval" | "other";
  type: string;
  content: string;
  ts: number;
}

interface EvalSessionState {
  convId: string;
  jobId: string;
  disputeId?: string;
  phase: "prep" | "committed" | "revealed";
  resolved: boolean;
  verdict?: "buyer" | "seller";
  reason?: string;
  autoMode: boolean;
  messages: EvalMessage[];
}

const sessions = new Map<string, EvalSessionState>();     // convId → session
const jobToConv = new Map<string, string>();              // jobId → dispute convId
const sseClients = new Set<http.ServerResponse>();

function pushSSE(event: string, data: unknown) {
  const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  for (const res of sseClients) res.write(payload);
}

function sessionToView(s: EvalSessionState) {
  return {
    convId: s.convId,
    jobId: s.jobId,
    disputeId: s.disputeId,
    phase: s.phase,
    resolved: s.resolved,
    verdict: s.verdict,
    autoMode: s.autoMode,
    messages: s.messages.slice(-50),
  };
}

// ── WS 客户端 ─────────────────────────────────────────────────────────────────
let client: WsMockClient;

const EVAL_VOTER_ADDR = "0xEvaluator00000000000000000000000000001";

function buildReason(verdict: "buyer" | "seller"): string {
  return verdict === "buyer"
    ? "交付物未完全满足验收标准，支持买家拒绝验收，资金退还买家。"
    : "交付物符合验收标准，买家拒绝理由不充分，资金释放给卖家。";
}

// 记录用户/自动模式选择的裁决,不立刻提交;等 evaluator_selected 事件到达再 commit。
function setVerdict(convId: string, verdict: "buyer" | "seller", reason?: string) {
  const s = sessions.get(convId);
  if (!s || s.resolved) return;
  s.verdict = verdict;
  s.reason = reason ?? buildReason(verdict);
  pushSSE("session_updated", sessionToView(s));
  // 如果 evaluator_selected 已经来过(极端时序),立即 commit
  // (mock-api 先发 TASK_DISPUTED 再 setTimeout 发 evaluator_selected,通常不会提前)
}

async function commitIfReady(s: EvalSessionState): Promise<void> {
  if (s.phase !== "prep" || !s.verdict) return;
  const vote: 1 | 2 = s.verdict === "seller" ? 1 : 2;
  try {
    await callCommitVote(s.jobId, vote, s.reason ?? buildReason(s.verdict));
    s.phase = "committed";
    pushSSE("session_updated", sessionToView(s));
  } catch (e) {
    console.error(`[eval][api] commit error:`, e);
  }
}

async function revealIfCommitted(s: EvalSessionState): Promise<void> {
  if (s.phase !== "committed") return;
  try {
    await callRevealVote(s.jobId);
    s.phase = "revealed";
    s.resolved = true;
    pushSSE("session_updated", sessionToView(s));
  } catch (e) {
    console.error(`[eval][api] reveal error:`, e);
  }
}

async function callCommitVote(jobId: string, vote: 1 | 2, reason: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/vote/commit`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ vote, reason, voter: EVAL_VOTER_ADDR }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[eval][api] committed job=${jobId} vote=${vote}`);
}

async function callRevealVote(jobId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/vote/reveal`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ voter: EVAL_VOTER_ADDR }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  console.log(`[eval][api] revealed job=${jobId}`);
}

// ── HTTP 服务 ─────────────────────────────────────────────────────────────────
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url!, "http://localhost");

  if (url.pathname === "/events") {
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Connection": "keep-alive",
      "Access-Control-Allow-Origin": "*",
    });
    sseClients.add(res);
    for (const s of sessions.values()) {
      res.write(`event: session_updated\ndata: ${JSON.stringify(sessionToView(s))}\n\n`);
    }
    req.on("close", () => sseClients.delete(res));
    return;
  }

  if (url.pathname === "/sessions" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify([...sessions.values()].map(sessionToView)));
    return;
  }

  // POST /action  { convId, action, verdict?, reason? }
  if (url.pathname === "/action" && req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", async () => {
      try {
        const { convId, action, verdict, reason } = JSON.parse(body);
        const s = sessions.get(convId);
        if (!s) { res.writeHead(404); res.end("session not found"); return; }

        if (action === "toggle_auto") {
          s.autoMode = !s.autoMode;
          pushSSE("session_updated", sessionToView(s));
        } else if (action === "resolve") {
          // 手动模式:用户点击买家/卖家胜 → 设裁决;如已到 evaluator_selected 会在下一刻 commit
          setVerdict(convId, verdict as "buyer" | "seller", reason);
          await commitIfReady(s);
        }
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true }));
      } catch (e) {
        res.writeHead(400); res.end(String(e));
      }
    });
    return;
  }

  if (url.pathname === "/" || url.pathname === "/index.html") {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    res.end(HTML);
    return;
  }

  res.writeHead(404); res.end("not found");
});

// ── main ──────────────────────────────────────────────────────────────────────
async function main() {
  client = new WsMockClient(WS_URL, EVAL_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("EVALUATOR", EVAL_AGENT_ID, EVAL_COMM_ADDR);
  console.log(`✓ 身份已注册: role=EVALUATOR agentId=${EVAL_AGENT_ID}`);

  client.start((envelope: WsEnvelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const { type } = payload;
    const jobId = String(payload.jobId ?? "");
    const disputeId = String(payload.disputeId ?? "");

    if (from === EVAL_COMM_ADDR) return;
    console.log(`[eval] ← conv=${convId.slice(-20)} from=${from.slice(0, 20)} type=${type}`);

    // 非 dispute 入口的事件到达时,把消息挂到已存在的 dispute session(按 jobId 查)
    // 兼容大写 TASK_DISPUTED（mock-api 当前发的）和小写 task_disputed（mingtao.gan rename 方向）
    const isDisputed = type === "TASK_DISPUTED" || type === "task_disputed";
    const disputeConvId = isDisputed ? convId : jobToConv.get(jobId);
    if (!disputeConvId) return;  // 未见过此 jobId 的 dispute,忽略

    if (isDisputed && !sessions.has(convId)) {
      const s: EvalSessionState = {
        convId, jobId, disputeId,
        phase: "prep", resolved: false, autoMode: true, messages: [],
      };
      sessions.set(convId, s);
      jobToConv.set(jobId, convId);
      pushSSE("new_session", sessionToView(s));
    }

    const s = sessions.get(disputeConvId);
    if (!s) return;
    const msgFrom: EvalMessage["from"] = from.startsWith("0xMock") ? "system" : "other";
    s.messages.push({ from: msgFrom, type, content: String(payload.content ?? ""), ts: Date.now() });
    pushSSE("session_updated", sessionToView(s));

    switch (type) {
      case "TASK_DISPUTED":
      case "task_disputed": {
        // auto 模式:5s 模拟审查后登记默认裁决(commit 仍由 evaluator_selected 触发)
        if (s.autoMode && !s.verdict) {
          sleep(5000).then(() => {
            setVerdict(disputeConvId, "buyer");
            commitIfReady(s).catch(console.error);
          }).catch(console.error);
        }
        return;
      }
      case "evaluator_selected": {
        commitIfReady(s).catch(console.error);
        return;
      }
      case "reveal_started": {
        revealIfCommitted(s).catch(console.error);
        return;
      }
    }
  });

  server.listen(UI_PORT, () => {
    console.log(`\n🌐 UI: http://localhost:${UI_PORT}\n`);
  });

  await new Promise(() => {});
}

main().catch(console.error);

// ── HTML ──────────────────────────────────────────────────────────────────────
const HTML = `<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="UTF-8">
<title>Mock Evaluator</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: monospace; background: #0d1117; color: #c9d1d9; display: flex; height: 100vh; overflow: hidden; }
#sidebar { width: 260px; border-right: 1px solid #30363d; overflow-y: auto; display: flex; flex-direction: column; }
#sidebar h2 { padding: 12px 16px; font-size: 13px; color: #8b949e; border-bottom: 1px solid #30363d; }
.session-item { padding: 10px 16px; cursor: pointer; border-bottom: 1px solid #21262d; }
.session-item:hover, .session-item.active { background: #161b22; }
.session-item .job { font-size: 11px; color: #e3b341; }
.session-item .status { font-size: 11px; color: #8b949e; margin-top: 2px; display: flex; align-items: center; gap: 6px; }
.badge { display: inline-block; padding: 1px 6px; border-radius: 10px; font-size: 10px; }
.badge.auto { background: #1a4731; color: #3fb950; }
.badge.manual { background: #3d1f00; color: #f0883e; }
.badge.pending { background: #3a2d00; color: #e3b341; }
.badge.buyer { background: #0d2818; color: #3fb950; }
.badge.seller { background: #1c2e4a; color: #58a6ff; }
#main { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
#conv-header { padding: 12px 16px; border-bottom: 1px solid #30363d; display: flex; align-items: center; gap: 12px; }
#conv-header h3 { font-size: 12px; flex: 1; color: #8b949e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
#messages { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
.msg { max-width: 75%; padding: 8px 12px; border-radius: 8px; font-size: 12px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }
.msg.arb { background: #2d1f00; border: 1px solid #e3b341; align-self: flex-end; }
.msg.system { background: #1a2332; border: 1px solid #30363d; align-self: center; color: #8b949e; font-size: 11px; }
.msg.other { background: #161b22; border: 1px solid #30363d; align-self: flex-start; }
.msg .meta { font-size: 10px; color: #8b949e; margin-bottom: 3px; }
#toolbar { padding: 12px 16px; border-top: 1px solid #30363d; display: flex; flex-direction: column; gap: 8px; }
#toolbar .btns { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
button { padding: 6px 14px; border-radius: 6px; border: none; cursor: pointer; font-size: 12px; font-family: monospace; }
.btn-buyer { background: #1a4731; color: #3fb950; font-weight: bold; }
.btn-seller { background: #1c2e4a; color: #58a6ff; font-weight: bold; }
.btn-neutral { background: #21262d; color: #c9d1d9; }
.btn-neutral:disabled { opacity: 0.4; cursor: default; }
#reason-input { flex: 1; background: #161b22; border: 1px solid #30363d; color: #c9d1d9; padding: 6px 10px; border-radius: 6px; font-size: 12px; font-family: monospace; }
#empty { flex: 1; display: flex; align-items: center; justify-content: center; color: #30363d; font-size: 14px; }
</style>
</head>
<body>
<div id="sidebar">
  <h2>⚖️ 仲裁 Sessions</h2>
  <div id="session-list"></div>
</div>
<div id="main">
  <div id="empty">等待仲裁请求...</div>
  <div id="conv-view" style="display:none; flex:1; flex-direction:column; overflow:hidden;">
    <div id="conv-header">
      <h3 id="conv-title"></h3>
      <span id="verdict-badge"></span>
      <button class="btn-neutral" id="btn-auto" onclick="toggleAuto()">切换自动</button>
    </div>
    <div id="messages"></div>
    <div id="toolbar">
      <div class="btns">
        <button class="btn-buyer" id="btn-buyer" onclick="resolve('buyer')">⚖️ 买家胜</button>
        <button class="btn-seller" id="btn-seller" onclick="resolve('seller')">⚖️ 卖家胜</button>
        <input id="reason-input" type="text" placeholder="裁决理由（可选）" />
      </div>
    </div>
  </div>
</div>

<script>
let currentConvId = null;
const sessions = {};

const es = new EventSource('/events');
es.addEventListener('new_session', e => {
  const s = JSON.parse(e.data);
  sessions[s.convId] = s;
  renderSidebar();
  if (!currentConvId) selectSession(s.convId);
});
es.addEventListener('session_updated', e => {
  const s = JSON.parse(e.data);
  sessions[s.convId] = s;
  renderSidebar();
  if (currentConvId === s.convId) renderConv(s);
});

function renderSidebar() {
  const list = document.getElementById('session-list');
  list.innerHTML = Object.values(sessions).map(s => \`
    <div class="session-item \${s.convId === currentConvId ? 'active' : ''}" onclick="selectSession('\${s.convId}')">
      <div class="job">jobId: \${s.jobId || '?'}</div>
      <div class="status">
        \${s.resolved
          ? \`<span class="badge \${s.verdict}">\${s.verdict === 'buyer' ? '买家胜' : '卖家胜'}</span>\`
          : \`<span class="badge pending">待裁决</span>\`}
        <span class="badge \${s.autoMode ? 'auto' : 'manual'}">\${s.autoMode ? '自动' : '手动'}</span>
      </div>
    </div>
  \`).join('');
}

function selectSession(convId) {
  currentConvId = convId;
  document.getElementById('empty').style.display = 'none';
  document.getElementById('conv-view').style.display = 'flex';
  renderConv(sessions[convId]);
  renderSidebar();
}

function renderConv(s) {
  document.getElementById('conv-title').textContent = s.convId;
  const vb = document.getElementById('verdict-badge');
  vb.innerHTML = s.resolved
    ? \`<span class="badge \${s.verdict}">\${s.verdict === 'buyer' ? '已裁：买家胜' : '已裁：卖家胜'}</span>\`
    : \`<span class="badge pending">待裁决</span>\`;
  document.getElementById('btn-auto').textContent = s.autoMode ? '⚡ 自动' : '🖐 手动';
  document.getElementById('btn-buyer').disabled = s.resolved;
  document.getElementById('btn-seller').disabled = s.resolved;

  const msgs = document.getElementById('messages');
  msgs.innerHTML = s.messages.map(m => \`
    <div class="msg \${m.from}">
      <div class="meta">\${m.from === 'eval' ? '仲裁者' : m.from === 'system' ? '系统' : '参与方'} · \${m.type} · \${new Date(m.ts).toLocaleTimeString()}</div>
      \${escHtml(m.content)}
    </div>
  \`).join('');
  msgs.scrollTop = msgs.scrollHeight;
}

function escHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function toggleAuto() {
  if (!currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'},
    body: JSON.stringify({ convId: currentConvId, action: 'toggle_auto' }) });
}

function resolve(verdict) {
  if (!currentConvId) return;
  const reason = document.getElementById('reason-input').value.trim();
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'},
    body: JSON.stringify({ convId: currentConvId, action: 'resolve', verdict, reason: reason || undefined }) });
}
</script>
</body>
</html>`;
