// ─── XMTP mock buyer (Web UI) ───
// 启动：UI_PORT=9013 XMTP_WALLET_KEYS=0x... node --env-file=.env dist/index-ui.js
// 浏览器打开 http://localhost:9013 即可点击收发消息。

import http from "node:http";
import { homedir } from "node:os";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Agent, IdentifierKind } from "@xmtp/agent-sdk";
import { createUser, createSigner } from "@xmtp/agent-sdk/user";

type XmtpEnv = "dev" | "production" | "local";
// 先从脚本所在目录自动识别角色（避免忘设 ROLE 环境变量导致端口 / 日志标签冲突）
function detectRole(): "buyer" | "seller" {
  const scriptPath = fileURLToPath(import.meta.url);
  if (scriptPath.includes("xmtp-mock-seller")) return "seller";
  if (scriptPath.includes("xmtp-mock-buyer")) return "buyer";
  return "buyer";
}
const ROLE = (process.env.ROLE ?? detectRole()).toLowerCase();
const TAG = `[mock-${ROLE}]`;
const UI_PORT = Number(process.env.UI_PORT ?? (ROLE === "seller" ? 9014 : 9013));

// 我方 agent 身份（发出 envelope 时放进 sender 字段）
const OWN_AGENT_ID              = process.env.OWN_AGENT_ID ?? "";
const OWN_AGENT_NAME            = process.env.OWN_AGENT_NAME ?? "";
const OWN_AGENT_PROFILE_DESC    = process.env.OWN_AGENT_PROFILE_DESC ?? "";
const OWN_AGENT_PROFILE_PICTURE = process.env.OWN_AGENT_PROFILE_PICTURE ?? "";
const OWN_AGENT_ROLE            = process.env.OWN_AGENT_ROLE ?? "";   // 1=buyer, 2=seller

interface Msg {
  dir: "in" | "out";
  content: string;
  sender: string;   // sender inboxId
  ts: number;
}

// 每个会话的上下文 —— 入站 a2a-agent-chat envelope 里抽出来，用于出站复用
interface ConvCtx {
  peerAddr?: string;
  groupId?: string;
  jobId?: string;
}

const conversations = new Map<string, Msg[]>();        // convId → messages
const convCtxMap    = new Map<string, ConvCtx>();      // convId → context

function requireEnv(name: string): string {
  const v = process.env[name]?.trim();
  if (!v) { console.error(`${TAG} 缺少环境变量: ${name}`); process.exit(1); }
  return v;
}

function recordMsg(convId: string, msg: Msg) {
  let list = conversations.get(convId);
  if (!list) { list = []; conversations.set(convId, list); }
  list.push(msg);
  pushSSE("message", { convId, msg });
}

// ── a2a-agent-chat envelope builder ─────────────────────────────────
// 末尾的 scheme 字段是字段说明元数据，对端 openclaw XMTP 插件里也按这个格式发。
// 对齐它保证协议风格一致（对端如果对 scheme 结构有 assert 就不会 fail）。
function buildEnvelope(args: {
  content: string;
  peerAddr: string;
  groupId: string;
  jobId: string;
  myAddress: string;
}): Record<string, unknown> {
  return {
    msgType: "a2a-agent-chat",
    content: args.content,
    contentType: "text",
    fromXmtpAddress: args.myAddress.toLowerCase(),
    toXmtpAddress:   (args.peerAddr || "").toLowerCase(),
    groupId: args.groupId,
    jobId:   args.jobId,
    sender: {
      agentId:             OWN_AGENT_ID,
      name:                OWN_AGENT_NAME,
      profileDescription:  OWN_AGENT_PROFILE_DESC,
      profilePicture:      OWN_AGENT_PROFILE_PICTURE,
      role:                OWN_AGENT_ROLE ? Number(OWN_AGENT_ROLE) : undefined,
    },
    scheme: {
      msgType:         "消息类型标识，固定为 a2a-agent-chat",
      content:         "消息正文",
      contentType:     "内容类型，固定为 text",
      fromXmtpAddress: "发送方 XMTP 通信地址",
      toXmtpAddress:   "接收方 XMTP 通信地址",
      groupId:         "XMTP 群聊 ID",
      jobId:           "A2A 任务 ID",
      sender:          "发送方 agent 身份信息，包含 agentId / name / profileDescription / profilePicture / role（1=buyer, 2=seller）",
    },
  };
}

// ── SSE ─────────────────────────────────────────────────────────────
const sseClients = new Set<http.ServerResponse>();
function pushSSE(event: string, data: unknown) {
  const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  for (const res of sseClients) res.write(payload);
}

async function main() {
  const env = (process.env.XMTP_ENV as XmtpEnv | undefined) ?? "dev";
  const walletKey = requireEnv("XMTP_WALLET_KEYS").split(",")[0]!.trim() as `0x${string}`;
  const dbDir = `${homedir()}/.xmtp-mock-${ROLE}`;
  mkdirSync(dbDir, { recursive: true });

  console.log(`${TAG} 连接 XMTP (env=${env})…`);
  const user = createUser(walletKey);
  const signer = createSigner(user);
  const agent = await Agent.create(signer, {
    dbPath: (inboxId: string) => `${dbDir}/${inboxId}-${env}.db3`,
    env,
  });

  const myInboxId = agent.client.inboxId;
  const myAddress = user.account.address;
  console.log(`${TAG} inboxId=${myInboxId} address=${myAddress}`);

  await agent.client.conversations.syncAll();

  agent.on("text", async (ctx) => {
    const m = ctx.message;
    if (m.senderInboxId === myInboxId) return;
    const convId = m.conversationId;
    const content = typeof m.content === "string" ? m.content : JSON.stringify(m.content);
    // 尝试解析 a2a-agent-chat envelope，抽出对话上下文缓存起来，供回复 wrap 复用。
    // UI 不解构 —— 原样 display。
    try {
      const parsed = JSON.parse(content);
      if (parsed && parsed.msgType === "a2a-agent-chat") {
        const ctxRec = convCtxMap.get(convId) ?? {};
        if (typeof parsed.fromXmtpAddress === "string") ctxRec.peerAddr = parsed.fromXmtpAddress;
        if (typeof parsed.groupId === "string")         ctxRec.groupId  = parsed.groupId;
        if (typeof parsed.jobId === "string")           ctxRec.jobId    = parsed.jobId;
        convCtxMap.set(convId, ctxRec);
      }
    } catch { /* 非 JSON 消息忽略解析，直接展示 raw */ }
    recordMsg(convId, { dir: "in", content, sender: m.senderInboxId, ts: Date.now() });
  });

  agent.on("unknownMessage", async (ctx) => {
    const m = ctx.message;
    recordMsg(m.conversationId, {
      dir: "in",
      content: `[unknown contentType=${m.contentType?.typeId ?? "?"}]`,
      sender: m.senderInboxId,
      ts: Date.now(),
    });
  });

  agent.on("start", () => console.log(`${TAG} agent 已启动`));
  agent.start().catch((e: unknown) => console.error(`${TAG} agent 异常:`, e));

  // ── HTTP server ───────────────────────────────────────────────────
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url ?? "/", `http://${req.headers.host}`);

    const sendJson = (code: number, body: unknown) => {
      res.writeHead(code, { "Content-Type": "application/json" });
      res.end(JSON.stringify(body));
    };

    const readBody = (): Promise<string> =>
      new Promise((resolve, reject) => {
        let buf = "";
        req.on("data", (c) => (buf += c));
        req.on("end", () => resolve(buf));
        req.on("error", reject);
      });

    // Static HTML
    if (url.pathname === "/" || url.pathname === "/index.html") {
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
      res.end(HTML);
      return;
    }

    // SSE stream
    if (url.pathname === "/events") {
      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      });
      sseClients.add(res);
      res.write(`event: hello\ndata: ${JSON.stringify({ myInboxId, myAddress, role: ROLE, agentId: OWN_AGENT_ID, agentName: OWN_AGENT_NAME })}\n\n`);
      req.on("close", () => sseClients.delete(res));
      return;
    }

    // State snapshot
    if (url.pathname === "/state") {
      const convs: Array<{ convId: string; peer: string; msgCount: number; lastMsg?: Msg }> = [];
      for (const [convId, list] of conversations) {
        const lastMsg = list[list.length - 1];
        // Peer inferred from last inbound message's sender if any
        const firstInbound = list.find((m) => m.dir === "in");
        convs.push({
          convId,
          peer: firstInbound?.sender ?? "?",
          msgCount: list.length,
          lastMsg,
        });
      }
      sendJson(200, { myInboxId, myAddress, role: ROLE, agentId: OWN_AGENT_ID, agentName: OWN_AGENT_NAME, conversations: convs });
      return;
    }

    // Get messages for a conversation
    if (url.pathname === "/messages") {
      const convId = url.searchParams.get("convId") ?? "";
      const list = conversations.get(convId) ?? [];
      sendJson(200, { convId, messages: list });
      return;
    }

    // POST /send {convId, content}  —— content 是用户输入的纯文本，发出前包成 envelope
    if (req.method === "POST" && url.pathname === "/send") {
      try {
        const body = JSON.parse(await readBody()) as { convId?: string; content?: string };
        if (!body.convId || !body.content) { sendJson(400, { error: "convId + content required" }); return; }
        const conv = await agent.client.conversations.getConversationById(body.convId);
        if (!conv) { sendJson(404, { error: "conversation not found" }); return; }
        const ctxRec = convCtxMap.get(body.convId) ?? {};
        const envelope = buildEnvelope({
          content: body.content,
          peerAddr: ctxRec.peerAddr ?? "",
          groupId:  ctxRec.groupId  ?? "",
          jobId:    ctxRec.jobId    ?? "",
          myAddress,
        });
        const payload = JSON.stringify(envelope);
        await conv.send(payload);
        recordMsg(body.convId, { dir: "out", content: payload, sender: myInboxId, ts: Date.now() });
        sendJson(200, { ok: true });
      } catch (e: any) {
        sendJson(500, { error: String(e?.message ?? e) });
      }
      return;
    }

    // POST /new-dm {peer, content}
    if (req.method === "POST" && url.pathname === "/new-dm") {
      try {
        const body = JSON.parse(await readBody()) as { peer?: string; content?: string };
        if (!body.peer) { sendJson(400, { error: "peer required (address or inboxId)" }); return; }
        const isAddr = body.peer.startsWith("0x") && body.peer.length === 42;
        const conv = isAddr
          ? await agent.client.conversations.newDmWithIdentifier({
              identifier: body.peer,
              identifierKind: IdentifierKind.Ethereum,
            })
          : await agent.client.conversations.newDm(body.peer);
        if (body.content) {
          // 首条消息也 wrap envelope；peer 已知，groupId/jobId 暂无，留空
          const envelope = buildEnvelope({
            content: body.content,
            peerAddr: body.peer,
            groupId:  process.env.DEFAULT_GROUP_ID ?? "",
            jobId:    process.env.DEFAULT_JOB_ID ?? "",
            myAddress,
          });
          const payload = JSON.stringify(envelope);
          await conv.send(payload);
          recordMsg(conv.id, { dir: "out", content: payload, sender: myInboxId, ts: Date.now() });
        } else {
          // Ensure the conversation shows up even before any message is sent
          if (!conversations.has(conv.id)) conversations.set(conv.id, []);
        }
        sendJson(200, { ok: true, convId: conv.id });
      } catch (e: any) {
        sendJson(500, { error: String(e?.message ?? e) });
      }
      return;
    }

    res.writeHead(404); res.end("not found");
  });

  server.listen(UI_PORT, () => {
    console.log(`${TAG} UI → http://localhost:${UI_PORT}`);
  });
}

main().catch((e) => { console.error(`${TAG} fatal:`, e); process.exit(1); });

// ── Inline HTML UI ────────────────────────────────────────────────────
const HTML = `<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="UTF-8">
<title>XMTP Mock</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: ui-monospace, monospace; background: #0d1117; color: #c9d1d9; display: flex; flex-direction: column; height: 100vh; overflow: hidden; }
#topbar { display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: #161b22; border-bottom: 1px solid #30363d; flex-shrink: 0; font-size: 12px; }
#topbar h1 { font-size: 14px; color: #58a6ff; flex: 1; }
#topbar .meta { color: #8b949e; }
#topbar .meta b { color: #c9d1d9; font-weight: normal; }
#newdm { display: flex; gap: 6px; }
#newdm input { background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 5px 8px; border-radius: 5px; font-size: 12px; font-family: inherit; }
#newdm input[name=peer] { width: 300px; }
#newdm input[name=init] { width: 200px; }
#newdm button { background: #238636; border: none; color: white; padding: 5px 12px; border-radius: 5px; cursor: pointer; font-size: 12px; }
#newdm button:hover { background: #2ea043; }
#workspace { display: flex; flex: 1; overflow: hidden; }
#sidebar { width: 280px; border-right: 1px solid #30363d; overflow-y: auto; }
#sidebar h2 { padding: 8px 14px; font-size: 11px; color: #8b949e; border-bottom: 1px solid #30363d; text-transform: uppercase; letter-spacing: .05em; position: sticky; top: 0; background: #0d1117; }
.conv-item { padding: 8px 14px; cursor: pointer; border-bottom: 1px solid #21262d; font-size: 11px; }
.conv-item:hover { background: #161b22; }
.conv-item.active { background: #1f3a5f; border-left: 2px solid #58a6ff; }
.conv-item .peer { color: #58a6ff; font-weight: bold; word-break: break-all; }
.conv-item .last { color: #8b949e; font-size: 10px; margin-top: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.empty-hint { padding: 12px 14px; color: #484f58; font-size: 11px; text-align: center; }
#main { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
#conv-header { padding: 10px 14px; border-bottom: 1px solid #30363d; font-size: 12px; color: #8b949e; }
#conv-header b { color: #c9d1d9; }
#messages { flex: 1; overflow-y: auto; padding: 14px; display: flex; flex-direction: column; gap: 8px; }
.msg { max-width: 72%; padding: 8px 12px; border-radius: 8px; font-size: 12px; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }
.msg.out { background: #0d419d; align-self: flex-end; }
.msg.in { background: #161b22; border: 1px solid #30363d; align-self: flex-start; }
.msg .meta { font-size: 10px; color: #8b949e; margin-bottom: 3px; }
#action-toolbar { border-top: 1px solid #30363d; padding: 8px 14px; display: none; }
#action-toolbar.visible { display: block; }
#action-toolbar .btns { display: flex; gap: 6px; flex-wrap: wrap; }
#action-toolbar button { padding: 5px 11px; border-radius: 6px; border: none; cursor: pointer; font-size: 12px; font-family: inherit; }
#action-toolbar .btn-reply { background: #1f6feb; color: white; }
#action-toolbar .btn-reply:hover { background: #388bfd; }
#action-toolbar .btn-success { background: #1a4731; color: #3fb950; }
#action-toolbar .btn-success:hover { background: #1f5a3a; }
#action-toolbar .btn-apply { background: #0d419d; color: #79c0ff; }
#action-toolbar .btn-apply:hover { background: #1158c7; }
#action-toolbar .btn-deliver { background: #3a2d00; color: #e3b341; }
#action-toolbar .btn-deliver:hover { background: #4a3a00; }
#action-toolbar .btn-warn { background: #3d1f00; color: #f0883e; }
#action-toolbar .btn-warn:hover { background: #5a2f00; }
#input-bar { padding: 10px 14px; border-top: 1px solid #30363d; display: flex; gap: 8px; }
#input-bar input { flex: 1; background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 8px 12px; border-radius: 5px; font-family: inherit; font-size: 13px; }
#input-bar input:focus { outline: none; border-color: #58a6ff; }
#input-bar button { background: #1f6feb; border: none; color: white; padding: 8px 18px; border-radius: 5px; cursor: pointer; font-size: 13px; }
#input-bar button:hover { background: #388bfd; }
#input-bar button:disabled { background: #30363d; cursor: not-allowed; color: #8b949e; }
</style>
</head>
<body>
<div id="topbar">
  <h1 id="title">XMTP Mock</h1>
  <div class="meta">agentId: <b id="me-agent">—</b></div>
  <div class="meta">inbox: <b id="me-inbox">—</b></div>
  <div class="meta">addr: <b id="me-addr">—</b></div>
  <form id="newdm">
    <input name="peer" placeholder="对端 address / inboxId" />
    <input name="init" placeholder="首条消息（可选）" />
    <button type="submit">+ New DM</button>
  </form>
</div>
<div id="workspace">
  <div id="sidebar">
    <h2>会话</h2>
    <div id="conv-list">
      <div class="empty-hint">还没有会话。发起 New DM 或等对方先来消息。</div>
    </div>
  </div>
  <div id="main">
    <div id="conv-header">未选中会话</div>
    <div id="messages"></div>
    <div id="action-toolbar">
      <!-- Buyer 快捷动作（协商 3 步 + 链上 3 动作）-->
      <div class="btns" id="btns-buyer" style="display:none;">
        <button class="btn-reply" data-preset="buyer-details">发送任务详情</button>
        <button class="btn-reply" data-preset="buyer-accept-quote">接受报价</button>
        <button class="btn-reply" data-preset="buyer-confirm-pay">确认支付方式</button>
        <button class="btn-success" data-preset="buyer-confirm-accept">Confirm Accept</button>
        <button class="btn-success" data-preset="buyer-complete">Complete</button>
        <button class="btn-warn" data-preset="buyer-refuse">Refuse</button>
      </div>
      <!-- Seller 快捷动作（协商 3 步 + Apply / Deliver）-->
      <div class="btns" id="btns-seller" style="display:none;">
        <button class="btn-reply" data-preset="seller-inquire">询问详情</button>
        <button class="btn-reply" data-preset="seller-quote">报价</button>
        <button class="btn-reply" data-preset="seller-confirm-pay">确认支付</button>
        <button class="btn-apply" data-preset="seller-apply">TASK_APPLY</button>
        <button class="btn-deliver" data-preset="seller-deliver">TASK_DELIVER</button>
      </div>
    </div>
    <div id="input-bar">
      <input id="input-msg" placeholder="输入消息，回车或点发送" disabled />
      <button id="btn-send" disabled>发送</button>
    </div>
  </div>
</div>
<script>
const state = { me: null, convs: new Map(), activeConvId: null };
const $ = (id) => document.getElementById(id);

// 协议消息预设 —— 发送到 XMTP 对端后，由 openclaw skill 解析 + 触发链上动作
const PRESETS = {
  // Buyer 协商话术
  "buyer-details": "任务标题：开发一个 Python 脚本监控链上交易。\\n描述：实时输出以太坊主网大额交易，支持按金额过滤，有完整注释。\\n预算：100 USDT。\\n验收标准：代码有注释，支持以太坊主网，交付可运行脚本。",
  "buyer-accept-quote": "好的，我接受你的报价 100 USDT，交付时间 24 小时，请继续。",
  "buyer-confirm-pay": "确认，我接受报价：100 USDT，支付方式：non_escrow，交付时间 24 小时。请正式提交申请接单。",
  // Buyer 链上动作（语义消息，由对端 skill 接住后调 onchainos）
  "buyer-confirm-accept": "我已确认接单方，资金已入 escrow，请开始交付。[ACTION: CONFIRM_ACCEPT]",
  "buyer-complete": "我已验收完成，请放款。[ACTION: COMPLETE]",
  "buyer-refuse": "我拒绝此次交付，理由请你补充证据。[ACTION: REFUSE]",
  // Seller 协商话术
  "seller-inquire": "你好！我对这个任务感兴趣，能介绍一下任务详情吗？",
  "seller-quote": "我的报价是 100 USDT，交付时间 48 小时，请问可以接受吗？",
  "seller-confirm-pay": "报价：100 USDT，支付方式：non_escrow，交付时间 48 小时。",
  // Seller 链上动作
  "seller-apply": "我已提交申请接单，等待链上确认。[ACTION: APPLY]",
  "seller-deliver": "任务已完成，交付物（链接或附件）：<请填>。[ACTION: DELIVER]",
};

async function quickSend(key) {
  const content = PRESETS[key];
  if (!content || !state.activeConvId) return;
  await fetch("/send", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ convId: state.activeConvId, content }),
  });
}

function wireToolbar() {
  document.querySelectorAll("[data-preset]").forEach((b) => {
    b.addEventListener("click", () => quickSend(b.getAttribute("data-preset")));
  });
}

function toggleToolbarForRole(role) {
  const toolbar = $("action-toolbar");
  const buyerBtns = $("btns-buyer");
  const sellerBtns = $("btns-seller");
  if (role === "seller") {
    sellerBtns.style.display = "flex";
    buyerBtns.style.display = "none";
  } else {
    buyerBtns.style.display = "flex";
    sellerBtns.style.display = "none";
  }
  // 仅当有活跃会话时显示工具栏
  toolbar.classList.toggle("visible", !!state.activeConvId);
}

wireToolbar();

function renderSidebar() {
  const list = $("conv-list");
  if (state.convs.size === 0) {
    list.innerHTML = '<div class="empty-hint">还没有会话。发起 New DM 或等对方先来消息。</div>';
    return;
  }
  const items = [];
  for (const [convId, c] of state.convs) {
    const lastText = c.lastMsg ? (c.lastMsg.dir === "out" ? "→ " : "← ") + c.lastMsg.content.slice(0, 50) : "(no messages)";
    const active = convId === state.activeConvId ? "active" : "";
    items.push(
      '<div class="conv-item ' + active + '" data-conv="' + convId + '">' +
      '<div class="peer">' + c.peer.slice(0, 24) + '…</div>' +
      '<div class="last">' + escapeHtml(lastText) + '</div>' +
      '</div>'
    );
  }
  list.innerHTML = items.join("");
  for (const el of list.querySelectorAll(".conv-item")) {
    el.addEventListener("click", () => selectConv(el.getAttribute("data-conv")));
  }
}

function renderMessages(convId) {
  const c = state.convs.get(convId);
  const box = $("messages");
  if (!c) { box.innerHTML = ""; return; }
  box.innerHTML = (c.messages || []).map(m => {
    const time = new Date(m.ts).toTimeString().slice(0, 8);
    const who = m.dir === "out" ? state.me.myInboxId.slice(0, 8) + "(me)" : m.sender.slice(0, 12) + "…";
    return '<div class="msg ' + m.dir + '">' +
      '<div class="meta">' + who + ' · ' + time + '</div>' +
      escapeHtml(m.content) +
      '</div>';
  }).join("");
  box.scrollTop = box.scrollHeight;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

async function selectConv(convId) {
  state.activeConvId = convId;
  $("conv-header").innerHTML = 'conv: <b>' + convId.slice(0, 16) + '…</b>';
  $("input-msg").disabled = false;
  $("btn-send").disabled = false;
  toggleToolbarForRole(state.me?.role ?? "buyer");
  $("input-msg").focus();
  // Fetch full messages
  const resp = await fetch("/messages?convId=" + encodeURIComponent(convId));
  const { messages } = await resp.json();
  const c = state.convs.get(convId) || { peer: "?", messages: [] };
  c.messages = messages;
  c.lastMsg = messages[messages.length - 1];
  state.convs.set(convId, c);
  renderSidebar();
  renderMessages(convId);
}

async function refreshState() {
  const resp = await fetch("/state");
  const data = await resp.json();
  state.me = data;
  $("me-agent").textContent = data.agentId ? (data.agentId + (data.agentName ? " (" + data.agentName + ")" : "")) : "(未配置 OWN_AGENT_ID)";
  $("me-inbox").textContent = data.myInboxId.slice(0, 16) + "…";
  $("me-addr").textContent = data.myAddress;
  $("title").textContent = "XMTP Mock (" + data.role + ")";
  state.convs.clear();
  for (const c of data.conversations) {
    state.convs.set(c.convId, { peer: c.peer, msgCount: c.msgCount, lastMsg: c.lastMsg, messages: [] });
  }
  if (state.activeConvId && state.convs.has(state.activeConvId)) {
    await selectConv(state.activeConvId);
  } else {
    renderSidebar();
  }
}

// SSE
const ev = new EventSource("/events");
ev.addEventListener("hello", (e) => {
  const info = JSON.parse(e.data);
  state.me = info;
  $("me-agent").textContent = info.agentId ? (info.agentId + (info.agentName ? " (" + info.agentName + ")" : "")) : "(未配置 OWN_AGENT_ID)";
  $("me-inbox").textContent = info.myInboxId.slice(0, 16) + "…";
  $("me-addr").textContent = info.myAddress;
  $("title").textContent = "XMTP Mock (" + info.role + ")";
  refreshState();
});
ev.addEventListener("message", (e) => {
  const { convId, msg } = JSON.parse(e.data);
  let c = state.convs.get(convId);
  if (!c) { c = { peer: msg.sender, msgCount: 0, messages: [] }; state.convs.set(convId, c); }
  if (c.peer === "?") c.peer = msg.sender;
  c.messages.push(msg);
  c.lastMsg = msg;
  c.msgCount++;
  renderSidebar();
  if (convId === state.activeConvId) renderMessages(convId);
});

// Send
$("btn-send").addEventListener("click", send);
$("input-msg").addEventListener("keydown", (e) => { if (e.key === "Enter") send(); });
async function send() {
  const input = $("input-msg");
  const text = input.value.trim();
  if (!text || !state.activeConvId) return;
  input.value = "";
  const resp = await fetch("/send", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ convId: state.activeConvId, content: text }),
  });
  if (!resp.ok) {
    const err = await resp.json().catch(() => ({}));
    alert("发送失败: " + (err.error || resp.status));
  }
}

// New DM
$("newdm").addEventListener("submit", async (e) => {
  e.preventDefault();
  const form = e.target;
  const peer = form.peer.value.trim();
  const init = form.init.value.trim();
  if (!peer) return;
  const resp = await fetch("/new-dm", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ peer, content: init || undefined }),
  });
  const data = await resp.json();
  if (!resp.ok) {
    alert("新建 DM 失败: " + (data.error || resp.status));
    return;
  }
  form.peer.value = "";
  form.init.value = "";
  await refreshState();
  await selectConv(data.convId);
});
</script>
</body>
</html>`;
