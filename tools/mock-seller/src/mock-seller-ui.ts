/**
 * mock-seller-ui: 带 Web 界面的卖家 mock
 *
 * 启动后打开 http://localhost:9002 即可点击操作
 *
 * 用法:
 *   npm run mock-seller-ui
 */
import http from "node:http";
import { WsMockClient, WsEnvelope, TaskPayload } from "../../../plugins/ws-channel/src/ws-client.js";

const SELLER_COMM_ADDR = "0xSeller000000000000000000000000000000001";
const SELLER_AGENT_ID  = "mock-seller-agent-001";
const WS_URL           = "ws://127.0.0.1:9000";
const API_BASE_URL     = "http://127.0.0.1:9001";
const UI_PORT          = 9002;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function formatMsg(jobId: string, convId: string, msgType: string, text: string): string {
  const sep = "-".repeat(40);
  return `jobId:  ${jobId}\n来自:   ${SELLER_AGENT_ID} [SELLER]\n类型:   ${msgType}\n会话:   ${convId}\n${sep}\n${text}`;
}

// ── 状态 ─────────────────────────────────────────────────────────────────────
interface Message {
  from: "buyer" | "seller" | "system";
  type: string;
  content: string;
  ts: number;
}

interface Session {
  convId: string;
  jobId: string;
  step: number;      // 协商进度
  messages: Message[];
  autoMode: boolean; // 自动回复
}

const sessions = new Map<string, Session>();
// SSE 客户端列表（浏览器订阅）
const sseClients = new Set<http.ServerResponse>();
// 新会话的默认模式(全局,可通过 UI 切换)
let defaultAutoMode = true;

function pushSSE(event: string, data: unknown) {
  const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  for (const res of sseClients) {
    res.write(payload);
  }
}

// ── WS 客户端 ─────────────────────────────────────────────────────────────────
let client: WsMockClient;

function sellerSend(convId: string, payload: Partial<TaskPayload>) {
  const session = sessions.get(convId);
  if (!session) return;
  console.log(`[seller] → conv=${convId.slice(-20)} type=${payload.type}`);
  client.sendToConv(convId, payload as TaskPayload);
  session.messages.push({ from: "seller", type: payload.type!, content: String(payload.content ?? ""), ts: Date.now() });
  pushSSE("session_updated", sessionToView(session));
}

async function autoReply(session: Session, msgType: string) {
  if (!session.autoMode) return;

  const fmt = (t: string, text: string) => formatMsg(session.jobId, session.convId, t, text);
  if (session.step === 0 && (msgType === "TASK_INQUIRE" || msgType === "REPLY")) {
    await sleep(800);
    sellerSend(session.convId, { type: "TASK_REPLY", jobId: session.jobId, content: fmt("TASK_REPLY", "你好！我对这个任务感兴趣，能介绍一下任务详情、验收标准和截止时间吗？") });
    session.step = 1;
  } else if (session.step === 1 && msgType === "REPLY") {
    await sleep(1500);
    sellerSend(session.convId, { type: "TASK_REPLY", jobId: session.jobId, content: fmt("TASK_REPLY", "了解了任务详情。我的报价是 100 USDT，交付时间 48 小时，请问可以接受吗？") });
    session.step = 2;
  } else if (session.step === 2 && msgType === "REPLY") {
    await sleep(1500);
    sellerSend(session.convId, { type: "TASK_REPLY", jobId: session.jobId, content: fmt("TASK_REPLY", "报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。") });
    session.step = 3;
  } else if (session.step === 3 && msgType === "REPLY") {
    await sleep(800);
    sellerSend(session.convId, { type: "TASK_APPLY", jobId: session.jobId, content: fmt("TASK_APPLY", "我正式申请接单，报价 100 USDT，支付方式 non_escrow，交付时间 48 小时。") });
    session.step = 4;
    await callApplyApi(session.jobId).catch(console.error);
  } else if (msgType === "TASK_ACCEPTED") {
    await sleep(3000);
    const url = `https://mock-deliverable.example.com/${session.jobId}.html`;
    sellerSend(session.convId, { type: "TASK_DELIVER", jobId: session.jobId, content: fmt("TASK_DELIVER", "任务已完成，请买家验收。"), deliverableUrl: url });
    session.step = 5;
    await callSubmitApi(session.jobId, url).catch(console.error);
  }
}

async function callApplyApi(jobId: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/apply`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ provider_address: SELLER_COMM_ADDR, price_usdt: 100 }),
  });
  if (!res.ok) throw new Error(`apply ${res.status}`);
  console.log(`[seller][api] applied job=${jobId}`);
}

async function callSubmitApi(jobId: string, url: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/submit`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ provider_address: SELLER_COMM_ADDR, deliverable_url: url }),
  });
  if (!res.ok) throw new Error(`submit ${res.status}`);
  console.log(`[seller][api] submitted job=${jobId}`);
}

async function callDisputeApi(jobId: string, reason: string) {
  const res = await fetch(`${API_BASE_URL}/api/v1/task/${jobId}/dispute`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ reason }),
  });
  if (!res.ok) throw new Error(`dispute ${res.status}`);
  const data = await res.json() as { data?: { disputeId?: string } };
  console.log(`[seller][api] dispute raised job=${jobId} disputeId=${data.data?.disputeId ?? "?"}`);
}

function sessionToView(s: Session) {
  return { convId: s.convId, jobId: s.jobId, step: s.step, autoMode: s.autoMode, messages: s.messages.slice(-50) };
}

// ── HTTP 服务 ─────────────────────────────────────────────────────────────────
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url!, `http://localhost`);

  // SSE: 浏览器订阅实时更新
  if (url.pathname === "/events") {
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache",
      "Connection": "keep-alive",
      "Access-Control-Allow-Origin": "*",
    });
    sseClients.add(res);
    // 立即推送当前状态
    for (const s of sessions.values()) res.write(`event: session_updated\ndata: ${JSON.stringify(sessionToView(s))}\n\n`);
    req.on("close", () => sseClients.delete(res));
    return;
  }

  // GET /sessions
  if (url.pathname === "/sessions" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify([...sessions.values()].map(sessionToView)));
    return;
  }

  // GET /default-mode
  if (url.pathname === "/default-mode" && req.method === "GET") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ defaultAutoMode }));
    return;
  }

  // POST /default-mode { autoMode: bool }
  if (url.pathname === "/default-mode" && req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      try {
        const { autoMode } = JSON.parse(body);
        defaultAutoMode = !!autoMode;
        console.log(`[seller] 默认模式切换为: ${defaultAutoMode ? "⚡ 自动" : "🖐 手动"}`);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true, defaultAutoMode }));
      } catch (e) { res.writeHead(400); res.end(String(e)); }
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
          session.autoMode = !session.autoMode;
        } else if (action === "send") {
          sellerSend(convId, { type: "TASK_REPLY", jobId: session.jobId, content });
          session.step = Math.min(session.step + 1, 3);
        } else if (action === "apply") {
          sellerSend(convId, { type: "TASK_APPLY", jobId: session.jobId, content: content || "我正式申请接单，报价 100 USDT，支付方式 non_escrow，交付时间 48 小时。" });
          session.step = 4;
          await callApplyApi(session.jobId).catch(console.error);
        } else if (action === "deliver") {
          const delivUrl = `https://mock-deliverable.example.com/${session.jobId}.html`;
          sellerSend(convId, { type: "TASK_DELIVER", jobId: session.jobId, content: content || "任务已完成，请买家验收。", deliverableUrl: delivUrl });
          session.step = 5;
          await callSubmitApi(session.jobId, delivUrl).catch(console.error);
        } else if (action === "dispute_raise") {
          const reason = content || "买家拒绝验收，卖家不认可，申请仲裁";
          await callDisputeApi(session.jobId, reason).catch(console.error);
          session.step = 6;
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
  client = new WsMockClient(WS_URL, SELLER_COMM_ADDR);
  await client.connectAndRegister();
  await client.registerIdentity("PROVIDER", SELLER_AGENT_ID, SELLER_COMM_ADDR);
  console.log(`✓ 身份已注册: role=PROVIDER agentId=${SELLER_AGENT_ID}`);

  client.start((envelope) => {
    const { conversation_id: convId, from, payload } = envelope;
    const { type } = payload;
    const jobId = String(payload.jobId ?? "");

    if (type === "TASK_CONFIRMED" || from === SELLER_COMM_ADDR) return;
    console.log(`[seller] ← conv=${convId.slice(-20)} from=${from.slice(0, 20)} type=${type}`);

    if (!sessions.has(convId)) {
      sessions.set(convId, { convId, jobId, step: 0, messages: [], autoMode: defaultAutoMode });
      pushSSE("new_session", sessionToView(sessions.get(convId)!));
    }
    const session = sessions.get(convId)!;
    session.messages.push({ from: "buyer", type, content: String(payload.content ?? ""), ts: Date.now() });
    pushSSE("session_updated", sessionToView(session));

    autoReply(session, type).catch(console.error);
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
<title>Mock Seller</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: monospace; background: #0d1117; color: #c9d1d9; display: flex; height: 100vh; overflow: hidden; }
#sidebar { width: 260px; border-right: 1px solid #30363d; overflow-y: auto; display: flex; flex-direction: column; }
#sidebar h2 { padding: 12px 16px; font-size: 13px; color: #8b949e; border-bottom: 1px solid #30363d; }
.session-item { padding: 10px 16px; cursor: pointer; border-bottom: 1px solid #21262d; }
.session-item:hover, .session-item.active { background: #161b22; }
.session-item .job { font-size: 11px; color: #58a6ff; }
.session-item .step { font-size: 11px; color: #8b949e; margin-top: 2px; }
.badge { display: inline-block; padding: 1px 6px; border-radius: 10px; font-size: 10px; }
.badge.auto { background: #1a4731; color: #3fb950; }
.badge.manual { background: #3d1f00; color: #f0883e; }
#main { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
#conv-header { padding: 12px 16px; border-bottom: 1px solid #30363d; display: flex; align-items: center; gap: 12px; }
#conv-header h3 { font-size: 13px; flex: 1; color: #8b949e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
#messages { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
.msg { max-width: 70%; padding: 8px 12px; border-radius: 8px; font-size: 12px; line-height: 1.5; }
.msg.buyer { background: #161b22; border: 1px solid #30363d; align-self: flex-start; }
.msg.seller { background: #0d419d; align-self: flex-end; }
.msg.system { background: #1a2332; border: 1px solid #30363d; align-self: center; color: #8b949e; font-size: 11px; }
.msg .meta { font-size: 10px; color: #8b949e; margin-bottom: 3px; }
#toolbar { padding: 12px 16px; border-top: 1px solid #30363d; display: flex; flex-direction: column; gap: 8px; }
#toolbar .btns { display: flex; gap: 8px; flex-wrap: wrap; }
button { padding: 6px 12px; border-radius: 6px; border: none; cursor: pointer; font-size: 12px; font-family: monospace; }
.btn-auto { background: #21262d; color: #c9d1d9; }
.btn-reply { background: #1f6feb; color: white; }
.btn-apply { background: #1a4731; color: #3fb950; }
.btn-deliver { background: #3d1f00; color: #f0883e; }
.btn-dispute { background: #3a1c00; color: #ffa657; }
#input-area { display: flex; gap: 8px; }
#msg-input { flex: 1; background: #161b22; border: 1px solid #30363d; color: #c9d1d9; padding: 6px 10px; border-radius: 6px; font-size: 12px; font-family: monospace; }
#empty { flex: 1; display: flex; align-items: center; justify-content: center; color: #30363d; font-size: 14px; }
</style>
</head>
<body>
<div id="sidebar">
  <h2>Sessions</h2>
  <div style="padding: 10px 16px; border-bottom: 1px solid #30363d; font-size: 11px; color: #8b949e;">
    新会话默认:<br>
    <label style="cursor:pointer;margin-right:8px"><input type="radio" name="def-mode" value="auto" onchange="setDefaultMode(true)" checked> ⚡ 自动</label>
    <label style="cursor:pointer"><input type="radio" name="def-mode" value="manual" onchange="setDefaultMode(false)"> 🖐 手动</label>
  </div>
  <div id="session-list"></div>
</div>
<div id="main">
  <div id="empty">等待会话...</div>
  <div id="conv-view" style="display:none; flex:1; flex-direction:column; overflow:hidden;">
    <div id="conv-header">
      <h3 id="conv-title"></h3>
      <span id="step-badge"></span>
      <button class="btn-auto" id="btn-auto" onclick="toggleAuto()">切换自动</button>
    </div>
    <div id="messages"></div>
    <div id="toolbar">
      <div class="btns">
        <button class="btn-reply" onclick="quickAction('send', '你好！我对这个任务感兴趣，能介绍一下任务详情吗？')">询问详情</button>
        <button class="btn-reply" onclick="quickAction('send', '我的报价是 100 USDT，交付时间 48 小时，请问可以接受吗？')">报价</button>
        <button class="btn-reply" onclick="quickAction('send', '报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。')">确认支付</button>
        <button class="btn-apply" onclick="quickAction('apply', '')">TASK_APPLY</button>
        <button class="btn-deliver" onclick="quickAction('deliver', '')">TASK_DELIVER</button>
        <button class="btn-dispute" onclick="quickAction('dispute_raise', '')">⚖️ Dispute Raise</button>
      </div>
      <div id="input-area">
        <input id="msg-input" type="text" placeholder="自定义消息内容..." onkeydown="if(event.key==='Enter') sendCustom()">
        <button class="btn-reply" onclick="sendCustom()">发送 REPLY</button>
      </div>
    </div>
  </div>
</div>

<script>
let currentConvId = null;
const sessions = {};
const STEP_LABELS = ['等待消息','已询问详情','已报价','已确认支付','已申请接单','已交付'];

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
        \${STEP_LABELS[Math.min(s.step, 5)]}
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
  document.getElementById('step-badge').textContent = STEP_LABELS[Math.min(s.step, 5)];
  document.getElementById('btn-auto').textContent = s.autoMode ? '⚡ 自动' : '🖐 手动';
  const msgs = document.getElementById('messages');
  msgs.innerHTML = s.messages.map(m => \`
    <div class="msg \${m.from}">
      <div class="meta">\${m.from === 'buyer' ? '买家' : m.from === 'seller' ? '卖家' : '系统'} · \${m.type} · \${new Date(m.ts).toLocaleTimeString()}</div>
      \${m.content}
    </div>
  \`).join('');
  msgs.scrollTop = msgs.scrollHeight;
}

function toggleAuto() {
  if (!currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'toggle_auto' }) });
}

function quickAction(action, content) {
  if (!currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action, content }) });
}

function sendCustom() {
  const input = document.getElementById('msg-input');
  const content = input.value.trim();
  if (!content || !currentConvId) return;
  fetch('/action', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ convId: currentConvId, action: 'send', content }) });
  input.value = '';
}

function setDefaultMode(autoMode) {
  fetch('/default-mode', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({ autoMode }) });
}

fetch('/default-mode').then(r => r.json()).then(d => {
  const val = d.defaultAutoMode ? 'auto' : 'manual';
  const el = document.querySelector('input[name="def-mode"][value="' + val + '"]');
  if (el) el.checked = true;
});
</script>
</body>
</html>`;
