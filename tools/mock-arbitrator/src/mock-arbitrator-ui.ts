/**
 * mock-arbitrator-ui: 带 Web 界面的仲裁者 mock
 *
 * 启动后打开 http://localhost:9004
 *
 * 架构：每个 convId 一个 ArbSession（收到 TASK_DISPUTED → 在同 convId 内回复 TASK_RESOLVE）
 * 自动模式：5s 后自动裁决
 * 手动模式：UI 点击"买家胜"/"卖家胜"按钮
 *
 * 用法:
 *   npm run ui
 */
import http from "node:http";
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";

const ARB_COMM_ADDR = "0xArbitrator0000000000000000000000000001";
const ARB_AGENT_ID  = "mock-arbitrator-agent-001";
const WS_URL        = "ws://127.0.0.1:9000";
const API_BASE_URL  = "http://127.0.0.1:9001";
const UI_PORT       = 9004;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// ── 状态 ─────────────────────────────────────────────────────────────────────
interface ArbMessage {
  from: "system" | "arb" | "other";
  type: string;
  content: string;
  ts: number;
}

interface ArbSessionState {
  convId: string;
  jobId: string;
  resolved: boolean;
  verdict?: "buyer" | "seller";
  reason?: string;
  autoMode: boolean;
  messages: ArbMessage[];
}

const sessions = new Map<string, ArbSessionState>();
const sseClients = new Set<http.ServerResponse>();

function pushSSE(event: string, data: unknown) {
  const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  for (const res of sseClients) res.write(payload);
}

function sessionToView(s: ArbSessionState) {
  return {
    convId: s.convId,
    jobId: s.jobId,
    resolved: s.resolved,
    verdict: s.verdict,
    autoMode: s.autoMode,
    messages: s.messages.slice(-50),
  };
}

// ── WS 客户端 ─────────────────────────────────────────────────────────────────
let client: WsMockClient;

function arbSend(convId: string, payload: Partial<TaskPayload>) {
  const s = sessions.get(convId);
  if (!s) return;
  console.log(`[arb] → conv=${convId.slice(-20)} type=${payload.type}`);
  client.sendToConv(convId, payload as TaskPayload);
  s.messages.push({ from: "arb", type: payload.type!, content: String(payload.content ?? ""), ts: Date.now() });
  pushSSE("session_updated", sessionToView(s));
}

async function doResolve(convId: string, verdict: "buyer" | "seller", reason?: string) {
  const s = sessions.get(convId);
  if (!s || s.resolved) return;
  s.resolved = true;
  s.verdict = verdict;
  s.reason = reason;

  const defaultReason = verdict === "buyer"
    ? "交付物未完全满足验收标准，支持买家拒绝验收，资金退还买家。"
    : "交付物符合验收标准，买家拒绝理由不充分，资金释放给卖家。";
  const finalReason = reason || defaultReason;

  arbSend(convId, {
    type: "TASK_RESOLVE",
    jobId: s.jobId,
    winner: verdict,
    reason: finalReason,
    content: `⚖️ 仲裁结果：${verdict === "buyer" ? "买家" : "卖家"}胜\n裁决理由：${finalReason}`,
  });

  await callResolveApi(s.jobId, verdict, finalReason).catch((e) =>
    console.error(`[arb][api] resolve error:`, e),
  );
  pushSSE("session_updated", sessionToView(s));
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
          await doResolve(convId, verdict as "buyer" | "seller", reason);
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
  client = new WsMockClient(WS_URL, ARB_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("EVALUATOR", ARB_AGENT_ID, ARB_COMM_ADDR);
  console.log(`✓ 身份已注册: role=EVALUATOR agentId=${ARB_AGENT_ID}`);

  client.start((envelope: WsEnvelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const { type } = payload;
    const jobId = String(payload.jobId ?? "");

    if (from === ARB_COMM_ADDR) return;
    console.log(`[arb] ← conv=${convId.slice(-20)} from=${from.slice(0, 20)} type=${type}`);

    // 创建 session（按 convId）
    if (!sessions.has(convId)) {
      const s: ArbSessionState = { convId, jobId, resolved: false, autoMode: true, messages: [] };
      sessions.set(convId, s);
      pushSSE("new_session", sessionToView(s));
    }

    const s = sessions.get(convId)!;
    const msgFrom: ArbMessage["from"] = from.startsWith("0xMock") ? "system" : "other";
    s.messages.push({ from: msgFrom, type, content: String(payload.content ?? ""), ts: Date.now() });
    pushSSE("session_updated", sessionToView(s));

    // 收到 TASK_DISPUTED → 自动裁决
    if (type === "TASK_DISPUTED" && s.autoMode && !s.resolved) {
      sleep(5000).then(() => doResolve(convId, "buyer")).catch(console.error);
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
<title>Mock Arbitrator</title>
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
      <div class="meta">\${m.from === 'arb' ? '仲裁者' : m.from === 'system' ? '系统' : '参与方'} · \${m.type} · \${new Date(m.ts).toLocaleTimeString()}</div>
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
