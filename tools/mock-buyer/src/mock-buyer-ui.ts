/**
 * mock-buyer-ui: 带 Web 界面的买家 mock
 *
 * 启动后打开 http://localhost:9003 即可点击操作
 *
 * 功能:
 *   - 发布新任务（调用 mock-api POST /api/v1/task/create）
 *   - 自动/手动协商（共用 BuyerSession 状态机）
 *   - 自动 confirm-accept（收到 TASK_APPLY）
 *   - 自动 complete（收到 TASK_DELIVER）
 *
 * 用法:
 *   npm run ui
 */
import http from "node:http";
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";
import {
  BuyerSession, callAcceptApi, callCompleteApi,
  BUYER_COMM_ADDR, BUYER_AGENT_ID, WS_URL, API_BASE_URL,
  formatMsg, sleep, MOCK_TASK,
} from "./buyer-session.js";

const UI_PORT = 9003;

// ── display types ──────────────────────────────────────────────────────────────
interface Message {
  from: "buyer" | "seller" | "system";
  type: string;
  content: string;
  ts: number;
}

const sessions   = new Map<string, BuyerSession>();
const uiMessages = new Map<string, Message[]>();
const autoModes  = new Map<string, boolean>(); // convId → autoMode, default true
const sseClients = new Set<http.ServerResponse>();

function pushSSE(event: string, data: unknown) {
  const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  for (const res of sseClients) res.write(payload);
}

function sessionToView(s: BuyerSession) {
  return {
    convId: s.convId,
    jobId: s.jobId,
    step: s.step,
    autoMode: autoModes.get(s.convId) ?? true,
    messages: (uiMessages.get(s.convId) ?? []).slice(-50),
  };
}

function recordMsg(convId: string, msg: Message) {
  let msgs = uiMessages.get(convId);
  if (!msgs) { msgs = []; uiMessages.set(convId, msgs); }
  msgs.push(msg);
}

// ── WS 客户端 ─────────────────────────────────────────────────────────────────
let client: WsMockClient;

function buyerSend(convId: string, payload: Partial<TaskPayload>) {
  console.log(`[buyer] → conv=${convId.slice(-20)} type=${payload.type}`);
  client.sendToConv(convId, payload as TaskPayload);
  recordMsg(convId, { from: "buyer", type: payload.type!, content: String(payload.content ?? ""), ts: Date.now() });
  const s = sessions.get(convId);
  if (s) pushSSE("session_updated", sessionToView(s));
}

async function uiStartNegotiation(jobId: string): Promise<string> {
  // Find PROVIDER with retries (same logic as headless startNegotiation)
  let providers: unknown[] = [];
  for (let attempt = 0; attempt < 5; attempt++) {
    providers = await client.lookupRole("PROVIDER");
    if (providers.length > 0) break;
    console.log(`[buyer] no PROVIDER yet, retrying in 3s... (attempt ${attempt + 1}/5)`);
    await sleep(3000);
  }
  if (providers.length === 0) throw new Error("no PROVIDER registered after retries");

  const seller = providers[0] as { agent_id: string; comm_addr: string };
  const sellerAgentId = seller.agent_id ?? "unknown-seller";
  const sellerCommAddr = seller.comm_addr ?? "";
  const convId = `conv-${jobId}-${BUYER_AGENT_ID}-${sellerAgentId}`;
  console.log(`[buyer] starting negotiation conv=${convId} seller=${sellerAgentId}`);

  client.joinConversation(convId, [BUYER_COMM_ADDR, sellerCommAddr]);
  await sleep(300);

  // UI-aware reply: sends + records + pushes SSE
  const reply = (p: Partial<TaskPayload>) => {
    console.log(`[buyer] → conv=${convId.slice(-30)} type=${p.type}`);
    client.sendToConv(convId, p as TaskPayload);
    recordMsg(convId, { from: "buyer", type: p.type!, content: String(p.content ?? ""), ts: Date.now() });
    const s = sessions.get(convId);
    if (s) pushSSE("session_updated", sessionToView(s));
  };

  const session = new BuyerSession(
    convId, jobId, sellerAgentId, sellerCommAddr, reply,
    () => { pushSSE("session_updated", sessionToView(session)); },
  );
  sessions.set(convId, session);
  uiMessages.set(convId, []);
  autoModes.set(convId, true);
  pushSSE("new_session", sessionToView(session));

  const inquireContent = formatMsg(jobId, convId, "TASK_INQUIRE",
    `你好，我有一个任务（jobId: ${jobId}）想请你来完成，请问你感兴趣吗？`);
  client.sendToConv(convId, { type: "TASK_INQUIRE", jobId, content: inquireContent });
  recordMsg(convId, { from: "buyer", type: "TASK_INQUIRE", content: inquireContent, ts: Date.now() });
  pushSSE("session_updated", sessionToView(session));
  console.log(`[buyer] TASK_INQUIRE sent → ${sellerAgentId}`);

  return convId;
}

// ── HTTP 服务 ─────────────────────────────────────────────────────────────────
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url!, `http://localhost`);

  // SSE
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

  // GET /sessions
  if (url.pathname === "/sessions" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify([...sessions.values()].map(sessionToView)));
    return;
  }

  // POST /create-task  { title, description, budget }
  if (url.pathname === "/create-task" && req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", async () => {
      try {
        const { title, description, budget } = JSON.parse(body);
        const apiRes = await fetch(`${API_BASE_URL}/api/v1/task/create`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            title: title || MOCK_TASK.title,
            description: description || MOCK_TASK.description,
            tokenAmount: String(budget || MOCK_TASK.budget),
            tokenSymbol: "USDT",
            deadlineOpen: 172800,
            deadlineSubmit: 86400,
            buyerAgentAddress: BUYER_COMM_ADDR,
            buyerAgentId: BUYER_AGENT_ID,
          }),
        });
        const data = await apiRes.json() as { data?: { jobId?: string } };
        const jobId = data?.data?.jobId;
        if (!jobId) throw new Error("no jobId returned: " + JSON.stringify(data));
        console.log(`[buyer][api] task created jobId=${jobId}`);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true, jobId }));
      } catch (e) {
        res.writeHead(400); res.end(String(e));
      }
    });
    return;
  }

  // POST /start-negotiation  { jobId }
  if (url.pathname === "/start-negotiation" && req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", async () => {
      try {
        const { jobId } = JSON.parse(body);
        const convId = await uiStartNegotiation(jobId);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true, convId }));
      } catch (e) {
        res.writeHead(400); res.end(String(e));
      }
    });
    return;
  }

  // POST /action  { convId, action, content? }
  if (url.pathname === "/action" && req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", async () => {
      try {
        const { convId, action, content } = JSON.parse(body);
        const session = sessions.get(convId);
        if (!session) { res.writeHead(404); res.end("session not found"); return; }

        if (action === "toggle_auto") {
          autoModes.set(convId, !(autoModes.get(convId) ?? true));
        } else if (action === "send") {
          const formatted = formatMsg(session.jobId, session.convId, "REPLY", content);
          buyerSend(convId, { type: "REPLY", jobId: session.jobId, content: formatted });
        } else if (action === "accept") {
          if (!session.accepted) {
            session.accepted = true;
            await callAcceptApi(session.jobId, session.sellerAgentId).catch(console.error);
            session.step = 4;
          }
        } else if (action === "complete") {
          if (!session.completed) {
            session.completed = true;
            await callCompleteApi(session.jobId).catch(console.error);
            session.step = 6;
          }
        }
        pushSSE("session_updated", sessionToView(session));
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true }));
      } catch (e) {
        res.writeHead(400); res.end(String(e));
      }
    });
    return;
  }

  // GET / → HTML UI
  if (url.pathname === "/" || url.pathname === "/index.html") {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    res.end(HTML);
    return;
  }

  res.writeHead(404); res.end("not found");
});

// ── main ──────────────────────────────────────────────────────────────────────
async function main() {
  client = new WsMockClient(WS_URL, BUYER_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("REQUESTER", BUYER_AGENT_ID, BUYER_COMM_ADDR);
  console.log(`✓ 身份已注册: role=REQUESTER agentId=${BUYER_AGENT_ID} commAddr=${BUYER_COMM_ADDR}`);

  client.start((envelope: WsEnvelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const type = String(payload.type ?? "");
    const jobId = String(payload.jobId ?? "");

    if (from === BUYER_COMM_ADDR) return;
    console.log(`[buyer] ← conv=${convId.slice(-20)} from=${from.slice(0, 20)} type=${type}`);

    // TASK_CONFIRMED: 自动开始协商
    if (type === "TASK_CONFIRMED" && jobId) {
      console.log(`[buyer] TASK_CONFIRMED jobId=${jobId}，自动启动协商...`);
      uiStartNegotiation(jobId).catch(console.error);
      return;
    }

    const session = sessions.get(convId);
    if (!session) { console.log(`[buyer] unknown conv=${convId.slice(-20)}, ignoring`); return; }

    // Record incoming message for display
    const msgFrom: Message["from"] = from.startsWith("0xMock") ? "system" : "seller";
    recordMsg(convId, { from: msgFrom, type, content: String(payload.content ?? ""), ts: Date.now() });
    pushSSE("session_updated", sessionToView(session));

    // Delegate to shared BuyerSession state machine if in auto mode
    if (autoModes.get(convId) !== false) {
      session.handle(envelope).catch((e) => console.error(`[buyer][session] error:`, e));
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
<title>Mock Buyer</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: monospace; background: #0d1117; color: #c9d1d9; display: flex; flex-direction: column; height: 100vh; overflow: hidden; }
#topbar { display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: #161b22; border-bottom: 1px solid #30363d; flex-shrink: 0; }
#topbar h1 { font-size: 14px; color: #58a6ff; flex: 1; }
#create-form { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
#create-form input { background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 5px 8px; border-radius: 5px; font-size: 12px; font-family: monospace; }
#create-form input[name=title] { width: 220px; }
#create-form input[name=budget] { width: 70px; }
#create-form input[name=jobId] { width: 120px; }
#workspace { display: flex; flex: 1; overflow: hidden; }
#sidebar { width: 240px; border-right: 1px solid #30363d; overflow-y: auto; display: flex; flex-direction: column; }
#sidebar h2 { padding: 10px 14px; font-size: 12px; color: #8b949e; border-bottom: 1px solid #30363d; }
.session-item { padding: 10px 14px; cursor: pointer; border-bottom: 1px solid #21262d; }
.session-item:hover, .session-item.active { background: #161b22; }
.session-item .job { font-size: 11px; color: #58a6ff; }
.session-item .step { font-size: 11px; color: #8b949e; margin-top: 2px; display: flex; align-items: center; gap: 6px; }
.badge { display: inline-block; padding: 1px 6px; border-radius: 10px; font-size: 10px; }
.badge.auto { background: #1a4731; color: #3fb950; }
.badge.manual { background: #3d1f00; color: #f0883e; }
#main { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
#conv-header { padding: 10px 14px; border-bottom: 1px solid #30363d; display: flex; align-items: center; gap: 10px; }
#conv-header h3 { font-size: 12px; flex: 1; color: #8b949e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
#messages { flex: 1; overflow-y: auto; padding: 14px; display: flex; flex-direction: column; gap: 8px; }
.msg { max-width: 72%; padding: 8px 12px; border-radius: 8px; font-size: 12px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }
.msg.buyer { background: #0d419d; align-self: flex-end; }
.msg.seller { background: #161b22; border: 1px solid #30363d; align-self: flex-start; }
.msg.system { background: #1a2332; border: 1px solid #30363d; align-self: center; color: #8b949e; font-size: 11px; }
.msg .meta { font-size: 10px; color: #8b949e; margin-bottom: 3px; }
#toolbar { padding: 10px 14px; border-top: 1px solid #30363d; display: flex; flex-direction: column; gap: 8px; flex-shrink: 0; }
#toolbar .btns { display: flex; gap: 8px; flex-wrap: wrap; }
button { padding: 5px 11px; border-radius: 6px; border: none; cursor: pointer; font-size: 12px; font-family: monospace; }
.btn-primary { background: #1f6feb; color: white; }
.btn-success { background: #1a4731; color: #3fb950; }
.btn-warn { background: #3d1f00; color: #f0883e; }
.btn-neutral { background: #21262d; color: #c9d1d9; }
#input-area { display: flex; gap: 8px; }
#msg-input { flex: 1; background: #161b22; border: 1px solid #30363d; color: #c9d1d9; padding: 6px 10px; border-radius: 6px; font-size: 12px; font-family: monospace; }
#empty { flex: 1; display: flex; align-items: center; justify-content: center; color: #30363d; font-size: 14px; }
.step-bar { display: flex; gap: 0; margin-bottom: 4px; font-size: 10px; }
.step-dot { width: 16px; height: 16px; border-radius: 50%; background: #21262d; border: 2px solid #30363d; margin-right: 4px; display: flex; align-items: center; justify-content: center; font-size: 9px; color: #8b949e; }
.step-dot.done { background: #1a4731; border-color: #3fb950; color: #3fb950; }
.step-dot.active { background: #1f3a5f; border-color: #58a6ff; color: #58a6ff; }
</style>
</head>
<body>
<div id="topbar">
  <h1>Mock Buyer</h1>
  <div id="create-form">
    <input name="title" placeholder="任务标题 (可选)" />
    <input name="budget" placeholder="预算" value="100" />
    <button class="btn-primary" onclick="createTask()">发布任务</button>
    <span style="color:#8b949e;font-size:12px">或手动:</span>
    <input name="jobId" placeholder="jobId (e.g. 0x3e9)" />
    <button class="btn-neutral" onclick="startManual()">联系卖家</button>
  </div>
</div>
<div id="workspace">
  <div id="sidebar">
    <h2>Sessions</h2>
    <div id="session-list"></div>
  </div>
  <div id="main">
    <div id="empty">发布任务或输入 jobId 以开始</div>
    <div id="conv-view" style="display:none; flex:1; flex-direction:column; overflow:hidden;">
      <div id="conv-header">
        <h3 id="conv-title"></h3>
        <span id="step-indicator"></span>
        <button class="btn-neutral" id="btn-auto" onclick="toggleAuto()">切换自动</button>
      </div>
      <div id="messages"></div>
      <div id="toolbar">
        <div class="btns">
          <button class="btn-primary" onclick="quickReply(0)">发送任务详情</button>
          <button class="btn-primary" onclick="quickReply(1)">接受报价</button>
          <button class="btn-primary" onclick="quickReply(2)">确认支付方式</button>
          <button class="btn-success" onclick="doAccept()">Confirm Accept</button>
          <button class="btn-warn" onclick="doComplete()">Complete</button>
        </div>
        <div id="input-area">
          <input id="msg-input" type="text" placeholder="自定义消息..." onkeydown="if(event.key==='Enter') sendCustom()" />
          <button class="btn-neutral" onclick="sendCustom()">发送 REPLY</button>
        </div>
      </div>
    </div>
  </div>
</div>

<script>
let currentConvId = null;
const sessions = {};
const STEP_LABELS = ['等待卖家询问','已发任务详情','已接受报价','已确认支付','等待交付','交付中','已完成'];
const REPLIES = [
  '任务标题：开发一个 Python 脚本监控链上交易。\\n描述：实时输出以太坊主网大额交易，支持按金额过滤，有完整注释。\\n预算：100 USDT。\\n验收标准：代码有注释，支持以太坊主网，交付可运行脚本。',
  '好的，我接受你的报价 100 USDT，交付时间 48 小时，请继续。',
  '确认，我接受报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。请正式提交申请接单。',
];

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
      <div class="step">
        \${STEP_LABELS[Math.min(s.step, STEP_LABELS.length-1)]}
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
  // Step indicator
  const stepNames = ['询问', '详情', '报价', '支付', 'Apply', 'Accept', 'Done'];
  document.getElementById('step-indicator').innerHTML = stepNames.map((n, i) =>
    \`<span class="step-dot \${i < s.step ? 'done' : i === s.step ? 'active' : ''}">\${i+1}</span>\`
  ).join('');
  document.getElementById('btn-auto').textContent = s.autoMode ? '⚡ 自动' : '🖐 手动';
  const msgs = document.getElementById('messages');
  msgs.innerHTML = s.messages.map(m => \`
    <div class="msg \${m.from}">
      <div class="meta">\${m.from === 'buyer' ? '买家' : m.from === 'seller' ? '卖家' : '系统'} · \${m.type} · \${new Date(m.ts).toLocaleTimeString()}</div>
      \${escHtml(m.content)}
    </div>
  \`).join('');
  msgs.scrollTop = msgs.scrollHeight;
}

function escHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

async function createTask() {
  const title = document.querySelector('input[name=title]').value.trim() || '开发一个 Python 脚本监控链上交易';
  const budget = document.querySelector('input[name=budget]').value.trim() || '100';
  const res = await fetch('/create-task', { method: 'POST', headers: {'Content-Type':'application/json'},
    body: JSON.stringify({ title, budget: parseInt(budget) }) });
  const data = await res.json();
  if (data.ok) alert('任务已创建 jobId=' + data.jobId + '\\n等待 TASK_CONFIRMED (约8s) 后自动联系卖家');
  else alert('创建失败: ' + JSON.stringify(data));
}

async function startManual() {
  const jobId = document.querySelector('input[name=jobId]').value.trim();
  if (!jobId) { alert('请输入 jobId'); return; }
  const res = await fetch('/start-negotiation', { method: 'POST', headers: {'Content-Type':'application/json'},
    body: JSON.stringify({ jobId }) });
  const data = await res.json();
  if (!data.ok) alert('失败: ' + JSON.stringify(data));
}

function toggleAuto() {
  if (!currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'toggle_auto' }) });
}

function quickReply(step) {
  if (!currentConvId) return;
  const content = REPLIES[step];
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'send', content }) });
}

function doAccept() {
  if (!currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'accept' }) });
}

function doComplete() {
  if (!currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'complete' }) });
}

function sendCustom() {
  const input = document.getElementById('msg-input');
  const content = input.value.trim();
  if (!content || !currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'send', content }) });
  input.value = '';
}
</script>
</body>
</html>`;
