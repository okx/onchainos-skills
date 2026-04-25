// ─── XMTP mock buyer (Web UI) ───
// 启动：UI_PORT=9013 XMTP_WALLET_KEYS=0x... node --env-file=.env dist/index-ui.js
// 浏览器打开 http://localhost:9013 即可点击收发消息。

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { homedir } from "node:os";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { randomUUID } from "node:crypto";
import { Agent, IdentifierKind } from "@xmtp/agent-sdk";
import { createUser, createSigner } from "@xmtp/agent-sdk/user";

const requireFromEsm = createRequire(import.meta.url);

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

// mock-api 的地址（仅 buyer 端用，seller 端不调任务创建）
const MOCK_API_URL = process.env.MOCK_API_URL ?? "http://127.0.0.1:9001";

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

// ── Gateway RPC bridge（手动推系统通知到 openclaw agent sub session）─────
// 复用 openclaw 全局包里的 GatewayClient。用 POST /notify-openclaw 触发。
let _GatewayClient: any = null;
let _gatewayInitTried = false;
function loadGatewayClient(): any {
  if (_gatewayInitTried) return _GatewayClient;
  _gatewayInitTried = true;
  const candidates = [
    "/opt/homebrew/lib/node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js",
    "/usr/local/lib/node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js",
  ];
  for (const p of candidates) {
    try {
      _GatewayClient = requireFromEsm(p).GatewayClient;
      console.log(`${TAG} [notify] GatewayClient loaded from ${p}`);
      return _GatewayClient;
    } catch {}
  }
  console.log(`${TAG} [notify] GatewayClient not found; openclaw notifications disabled`);
  return null;
}

function findSessionKeyForJob(jobId: string, myAddress?: string): string | null {
  try {
    const p = path.join(homedir(), ".openclaw/agents/main/sessions/sessions.json");
    if (!fs.existsSync(p)) return null;
    const sessions = JSON.parse(fs.readFileSync(p, "utf8")) as Record<string, unknown>;
    for (const key of Object.keys(sessions)) {
      const idx = key.indexOf("okx-xmtp:");
      if (idx < 0) continue;
      const qs = new URLSearchParams(key.slice(idx + "okx-xmtp:".length));
      if (qs.get("job") !== jobId) continue;
      // sessions.json 是 openclaw 视角：my=<openclaw agent>，to=<对话对端即我们 mock>
      // 所以按 `to === myAddress`（mock 自己的地址）匹配，找出"openclaw 给我发的那条 session"
      if (myAddress) {
        const to = (qs.get("to") ?? "").toLowerCase();
        if (to !== myAddress.toLowerCase()) continue;
      }
      return key;
    }
  } catch (err) {
    console.log(`${TAG} [notify] sessions.json 读取失败:`, (err as Error).message);
  }
  return null;
}

async function callGatewaySessionsSend(sessionKey: string, message: string): Promise<void> {
  const GC = loadGatewayClient();
  if (!GC) throw new Error("GatewayClient not available");
  await new Promise<void>((resolve, reject) => {
    let settled = false;
    let stopped = false;
    const stop = (err?: unknown) => {
      if (settled) return;
      settled = true;
      if (err) reject(err instanceof Error ? err : new Error(String(err)));
      else resolve();
    };
    const client: any = new GC({
      instanceId: randomUUID(),
      clientName: "gateway-client",
      clientDisplayName: `${TAG}:notify`,
      mode: "backend",
      role: "operator",
      scopes: ["operator.admin"],
      minProtocol: 3,
      maxProtocol: 3,
      onHelloOk: async () => {
        try {
          await client.request(
            "sessions.send",
            { key: sessionKey, message, idempotencyKey: randomUUID() },
            { timeoutMs: 10_000 },
          );
          stopped = true;
          try { client.stop(); } catch {}
          stop();
        } catch (err) {
          stopped = true;
          try { client.stop(); } catch {}
          stop(err);
        }
      },
      onClose: (code: number, reason: string) => {
        if (stopped) return;
        stop(new Error(`gateway closed (${code}): ${reason || "no reason"}`));
      },
      onConnectError: (err: unknown) => {
        if (settled) return;
        stop(err);
      },
    });
    setTimeout(() => {
      if (settled) return;
      try { client.stop(); } catch {}
      stop(new Error("gateway timeout"));
    }, 10_000);
    client.start();
  });
}

// 按 a2a system envelope 拼 message 文本（见 SKILL.md Priority 1.5）
//
// 字段语义（不要再搞反！）：
// - `event`     = 触发本通知的事件名（provider_applied / job_accepted / ...）
//                 即"这次发生了什么"
// - `jobStatus` = 任务在状态机里此刻的真实状态（open / accepted / submitted / completed / ...）
//                 跟 mock-api 的 task.statusStr 一致；不是事件名
function buildSystemEnvelope(args: {
  agentId: string;
  jobId: string;
  event: string;       // 必填——具体事件
  jobStatus: string;   // 必填——任务真实状态（caller 应从 mock-api 实时拉）
  description?: string;
}): string {
  return JSON.stringify({
    agentId: args.agentId,
    message: {
      event: args.event,
      jobStatus: args.jobStatus,
      description: args.description ?? "",
      source: "system",
      jobId: args.jobId,
      timestamp: Math.floor(Date.now() / 1000),
    },
  });
}

// 实时拉 jobStatus —— 从 mock-api task detail 的 task.statusStr 取
async function fetchJobStatus(jobId: string): Promise<string> {
  try {
    const resp = await fetch(`${MOCK_API_URL}/priapi/v1/aieco/task/${encodeURIComponent(jobId)}`);
    const body: any = await resp.json();
    const status = body?.data?.task?.statusStr ?? body?.data?.statusStr ?? body?.statusStr;
    if (typeof status === "string" && status) return status;
  } catch (e) {
    console.error(`${TAG} [notify] 拉 jobStatus 失败:`, (e as Error)?.message ?? e);
  }
  return "unknown";
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

    // POST /new-dm {peer, content, jobId?}
    // jobId 存在 → 创建 Group（groupName=a2a-<jobId>，对齐 openclaw xmtp_start_conversation 协议）
    // jobId 缺省 → 退化到 DM（适合纯文本调试）
    if (req.method === "POST" && url.pathname === "/new-dm") {
      try {
        const body = JSON.parse(await readBody()) as { peer?: string; content?: string; jobId?: string };
        if (!body.peer) { sendJson(400, { error: "peer required (address or inboxId)" }); return; }
        const isAddr = body.peer.startsWith("0x") && body.peer.length === 42;
        const jobId = body.jobId ?? process.env.DEFAULT_JOB_ID ?? "";

        let conv: any;
        if (jobId) {
          // 建 Group —— 插件把 group 消息解析成 a2a-agent-chat envelope 走任务流程
          if (isAddr) {
            conv = await (agent.client.conversations as any).newGroupWithIdentifiers(
              [{ identifier: body.peer, identifierKind: IdentifierKind.Ethereum }],
              { groupName: `a2a-${jobId}` },
            );
          } else {
            conv = await (agent.client.conversations as any).newGroup(
              [body.peer],
              { groupName: `a2a-${jobId}` },
            );
          }
          console.log(`${TAG} 创建 Group: groupId=${conv.id} jobId=${jobId} peer=${body.peer}`);
        } else {
          conv = isAddr
            ? await agent.client.conversations.newDmWithIdentifier({
                identifier: body.peer,
                identifierKind: IdentifierKind.Ethereum,
              })
            : await agent.client.conversations.newDm(body.peer);
          console.log(`${TAG} 创建 DM: convId=${conv.id} peer=${body.peer}`);
        }
        // 缓存会话上下文，供后续发送时自动填回 envelope
        const ctxRec = convCtxMap.get(conv.id) ?? {};
        if (jobId) ctxRec.jobId = jobId;
        ctxRec.peerAddr = body.peer;
        // 对 Group，groupId = conv.id；对 DM，此字段为空
        ctxRec.groupId = jobId ? conv.id : "";
        convCtxMap.set(conv.id, ctxRec);

        if (body.content) {
          const envelope = buildEnvelope({
            content: body.content,
            peerAddr: body.peer,
            groupId:  ctxRec.groupId ?? "",
            jobId,
            myAddress,
          });
          const payload = JSON.stringify(envelope);
          await conv.send(payload);
          recordMsg(conv.id, { dir: "out", content: payload, sender: myInboxId, ts: Date.now() });
        } else {
          if (!conversations.has(conv.id)) conversations.set(conv.id, []);
        }
        sendJson(200, { ok: true, convId: conv.id });
      } catch (e: any) {
        sendJson(500, { error: String(e?.message ?? e) });
      }
      return;
    }

    // POST /create-task {title, budget, currency?}  —— 调 mock-api 创建任务
    if (req.method === "POST" && url.pathname === "/create-task") {
      try {
        const body = JSON.parse(await readBody()) as { title?: string; budget?: string; currency?: string };
        if (!body.title || !body.budget) { sendJson(400, { error: "title + budget required" }); return; }
        const tokenSymbol = (body.currency ?? "USDT").toUpperCase();
        const tokenAddress = tokenSymbol === "USDG"
          ? "0xUSDG0000000000000000000000000000000001"
          : "0xUSDT0000000000000000000000000000000001";
        const apiBody = {
          title: body.title,
          description: body.title,
          descriptionSummary: body.title,
          tokenAddress,
          tokenAmount: body.budget,
          paymentType: 0,
          openType: 1,
          chainId: 1,
          minCreditScore: 0,
          buyerAgentId: OWN_AGENT_ID,
          buyerAgentAddress: myAddress,
          expireConfig: { openExpireSec: 86400, acceptedExpireSec: 86400 },
        };
        const up = await fetch(`${MOCK_API_URL}/api/v1/task/create`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(apiBody),
        });
        const data = await up.json() as any;
        if (data.code !== 0 && data.code !== "0") {
          sendJson(500, { error: data.msg ?? "task create failed" });
          return;
        }
        sendJson(200, { ok: true, jobId: data.data?.jobId, raw: data });
      } catch (e: any) {
        sendJson(500, { error: String(e?.message ?? e) });
      }
      return;
    }

    // GET /sellers  —— 查 agent-list，筛 role=2 + status=1
    if (req.method === "GET" && url.pathname === "/sellers") {
      try {
        const up = await fetch(`${MOCK_API_URL}/priapi/v5/wallet/agentic/agent/agent-list?chainIndex=196`);
        const data = await up.json() as any;
        const list: any[] = data?.data?.[0]?.list ?? [];
        const sellers = list
          .filter((a) => a.role === 2 && a.status === 1)
          .map((a) => ({
            agentId:  a.agentId,
            name:     a.name,
            commAddr: a.communicationAddress,
            desc:     a.profileDescription,
          }));
        sendJson(200, { sellers });
      } catch (e: any) {
        sendJson(500, { error: String(e?.message ?? e), sellers: [] });
      }
      return;
    }

    // POST /notify-openclaw —— 手动推系统通知到 openclaw agent session
    if (req.method === "POST" && url.pathname === "/notify-openclaw") {
      try {
        const body = JSON.parse(await readBody()) as {
          jobId?: string;
          event?: string;
          description?: string;
        };
        if (!body.jobId || !body.event) {
          sendJson(400, { error: "jobId + event required" });
          return;
        }
        const sessionKey = findSessionKeyForJob(body.jobId, myAddress);
        if (!sessionKey) {
          sendJson(404, {
            error: `未找到 jobId=${body.jobId} + my=${myAddress} 对应的 session（openclaw 是否已建 group？）`,
          });
          return;
        }
        const jobStatus = await fetchJobStatus(body.jobId);
        const message = buildSystemEnvelope({
          agentId: OWN_AGENT_ID || "unknown",
          jobId: body.jobId,
          event: body.event,
          jobStatus,
          description: body.description,
        });
        await callGatewaySessionsSend(sessionKey, message);
        console.log(
          `${TAG} [notify] ✓ event=${body.event} jobStatus=${jobStatus} jobId=${body.jobId} → ${sessionKey.slice(0, 70)}…`,
        );
        sendJson(200, { ok: true, sessionKey, message });
      } catch (e: any) {
        console.error(`${TAG} [notify] ✗ failed:`, e?.message ?? e);
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
#create-task { display: none; align-items: center; gap: 6px; font-size: 12px; }
#create-task input { background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 5px 8px; border-radius: 5px; font-family: inherit; }
#create-task input[name=title]  { width: 200px; }
#create-task input[name=budget] { width: 70px; }
#create-task button { background: #1f6feb; border: none; color: white; padding: 5px 12px; border-radius: 5px; cursor: pointer; font-size: 12px; }
#create-task button:hover { background: #388bfd; }
#current-task { display: inline-block; color: #3fb950; font-size: 11px; margin-left: 8px; }
.sidebar-section { border-bottom: 2px solid #30363d; }
#sellers-section { display: none; }
#sellers-section h2 { display: flex; align-items: center; justify-content: space-between; }
#sellers-section button.refresh { background: #21262d; border: none; color: #c9d1d9; padding: 1px 6px; border-radius: 4px; font-size: 10px; cursor: pointer; }
.seller-item { padding: 8px 14px; cursor: pointer; border-bottom: 1px solid #21262d; font-size: 11px; }
.seller-item:hover { background: #161b22; }
.seller-item .sid { color: #58a6ff; font-weight: bold; }
.seller-item .desc { color: #c9d1d9; font-size: 10px; margin-top: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.seller-item .addr { color: #8b949e; font-size: 10px; margin-top: 2px; }
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
#notify-bar { padding: 8px 14px; border-top: 1px solid #30363d; background: #161b22; font-size: 11px; }
#notify-bar .bar-title { color: #8b949e; margin-bottom: 6px; }
#notify-bar .bar-row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
#notify-bar label { display: flex; align-items: center; gap: 4px; color: #8b949e; }
#notify-bar input, #notify-bar select { background: #0d1117; border: 1px solid #30363d; color: #c9d1d9; padding: 3px 8px; border-radius: 4px; font-family: inherit; font-size: 11px; }
#notify-bar input { width: 80px; }
#notify-bar button { background: #3a2d00; color: #e3b341; border: 1px solid #3a2d00; padding: 4px 12px; border-radius: 4px; cursor: pointer; font-size: 11px; font-family: inherit; }
#notify-bar button:hover { background: #4a3a00; }
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
  <form id="create-task">
    <input name="title" placeholder="任务标题" />
    <input name="budget" placeholder="预算" value="100" />
    <span style="color:#8b949e">USDT</span>
    <button type="submit">发布任务</button>
    <span id="current-task"></span>
  </form>
  <form id="newdm">
    <input name="peer" placeholder="对端 address / inboxId" />
    <input name="init" placeholder="首条消息（可选）" />
    <button type="submit">+ New DM</button>
  </form>
</div>
<div id="workspace">
  <div id="sidebar">
    <div id="sellers-section" class="sidebar-section">
      <h2>可接任务的卖家 <button class="refresh" id="btn-refresh-sellers">↻</button></h2>
      <div id="seller-list"><div class="empty-hint">加载中…</div></div>
    </div>
    <div class="sidebar-section">
      <h2>会话</h2>
      <div id="conv-list">
        <div class="empty-hint">还没有会话。发起 New DM 或等对方先来消息。</div>
      </div>
    </div>
  </div>
  <div id="main">
    <div id="conv-header">未选中会话</div>
    <div id="messages"></div>

    <!-- 手动推系统通知（模拟链事件）-->
    <div id="notify-bar">
      <div class="bar-title">📡 发送系统通知 (模拟链事件，推进自己 agent 的状态机)</div>
      <div class="bar-row">
        <label>jobId<input id="notify-jobid" type="text" placeholder="auto" /></label>
        <label>event
          <select id="notify-event">
            <option value="provider_applied">provider_applied</option>
            <option value="job_accepted">job_accepted</option>
            <option value="job_submitted">job_submitted</option>
            <option value="job_completed">job_completed</option>
            <option value="job_refused">job_refused</option>
            <option value="job_disputed">job_disputed</option>
            <option value="confirm_refund">confirm_refund</option>
            <option value="dispute_resolved">dispute_resolved</option>
          </select>
        </label>
        <button id="btn-notify" type="button">发送通知</button>
      </div>
    </div>

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
const state = { me: null, convs: new Map(), activeConvId: null, currentJobId: "", currentTaskTitle: "", currentTaskBudget: "" };
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

function prettifyIfJson(s) {
  // 尝试把 JSON 字符串展开成缩进格式；失败就原样返回。
  try {
    const parsed = JSON.parse(s);
    if (parsed && typeof parsed === "object") return JSON.stringify(parsed, null, 2);
  } catch {}
  return s;
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
      escapeHtml(prettifyIfJson(m.content)) +
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
  toggleBuyerUI(info.role);
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

// ── Buyer-only: 发布任务 + 联系卖家 ─────────────────────────────────
$("create-task").addEventListener("submit", async (e) => {
  e.preventDefault();
  const form = e.target;
  const title = form.title.value.trim();
  const budget = form.budget.value.trim();
  if (!title || !budget) { alert("title + budget required"); return; }
  const resp = await fetch("/create-task", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title, budget, currency: "USDT" }),
  });
  const data = await resp.json();
  if (!resp.ok) { alert("发布失败: " + (data.error || resp.status)); return; }
  state.currentJobId = data.jobId;
  state.currentTaskTitle = title;
  state.currentTaskBudget = budget;
  $("current-task").textContent = "jobId: " + data.jobId + " (" + title + ", " + budget + " USDT)";
  form.title.value = "";
  loadSellers();
});

async function loadSellers() {
  try {
    const resp = await fetch("/sellers");
    const data = await resp.json();
    const list = $("seller-list");
    const sellers = data.sellers || [];
    if (sellers.length === 0) {
      list.innerHTML = '<div class="empty-hint">没有 role=2 的在线卖家</div>';
      return;
    }
    list.innerHTML = sellers.map(s =>
      '<div class="seller-item" data-addr="' + s.commAddr + '">' +
      '<div class="sid">' + s.agentId + ' (' + escapeHtml(s.name) + ')</div>' +
      '<div class="desc">' + escapeHtml(s.desc || '') + '</div>' +
      '<div class="addr">' + s.commAddr.slice(0, 10) + '…' + s.commAddr.slice(-6) + '</div>' +
      '</div>'
    ).join("");
    for (const el of list.querySelectorAll(".seller-item")) {
      el.addEventListener("click", () => contactSeller(el.getAttribute("data-addr")));
    }
  } catch (e) {
    $("seller-list").innerHTML = '<div class="empty-hint">加载失败: ' + e + '</div>';
  }
}

async function contactSeller(addr) {
  if (!state.currentJobId) {
    alert('请先发布任务');
    return;
  }
  const content = "你好，我有一个任务（jobId: " + state.currentJobId +
    "，标题: " + state.currentTaskTitle +
    "，预算: " + state.currentTaskBudget + " USDT），请问你感兴趣吗？";
  const resp = await fetch("/new-dm", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ peer: addr, content, jobId: state.currentJobId }),
  });
  const data = await resp.json();
  if (!resp.ok) { alert("联系失败: " + (data.error || resp.status)); return; }
  await refreshState();
  await selectConv(data.convId);
}

$("btn-refresh-sellers").addEventListener("click", loadSellers);

// ── 发送系统通知（模拟链事件）─────────────────────────────────────
$("btn-notify").addEventListener("click", async () => {
  // jobId 默认来自当前会话 context
  let jobId = $("notify-jobid").value.trim();
  if (!jobId && state.activeConvId) {
    const c = state.convs.get(state.activeConvId);
    // 从收到的消息里扒 jobId（envelope.jobId）
    for (const m of (c?.messages || [])) {
      try {
        const env = JSON.parse(m.content);
        if (env && typeof env.jobId === "string" && env.jobId) { jobId = env.jobId; break; }
      } catch {}
    }
  }
  if (!jobId) { alert("请填 jobId 或先选中一个含 jobId 的会话"); return; }
  const event = $("notify-event").value;
  const resp = await fetch("/notify-openclaw", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jobId, event }),
  });
  const data = await resp.json();
  if (!resp.ok) {
    alert("推送失败: " + (data.error || resp.status));
    return;
  }
  console.log("[notify-openclaw] ✓ event=" + event, "jobId=" + jobId, "→", data.sessionKey?.slice(0, 60) + "…");
  $("notify-jobid").value = "";
});

// 仅在 buyer 角色下显示"发布任务"表单和卖家列表
function toggleBuyerUI(role) {
  const showBuyer = role === "buyer";
  $("create-task").style.display = showBuyer ? "flex" : "none";
  $("sellers-section").style.display = showBuyer ? "block" : "none";
  if (showBuyer) loadSellers();
}
</script>
</body>
</html>`;
