#!/usr/bin/env node

import { execFile } from "node:child_process";
import { fileURLToPath } from "node:url";
import http from "node:http";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const THIS_DIR = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_A2A_BIN = path.resolve(THIS_DIR, "../../.codex/bin/okx-a2a");
const DEFAULT_ONCHAINOS_BIN = path.resolve(THIS_DIR, "../../.codex/bin/onchainos");
const DEFAULT_AGENT_ID = "11926";
const DEFAULT_BUYER_AGENT_ID = "11040";
const MAX_BODY_BYTES = 256 * 1024;
const MAX_EVENTS = 100;

const SERVICE_SIGNAL_DEFAULTS = new Map([
  ["5ebbf120-a8bf-419f-925b-a94af41cb5fa", {
    serviceName: "姻緣命理解讀",
    signal: [
      "【姻緣命理解讀｜模擬信號】",
      "感情能量：穩中有進，適合坦誠溝通與確認彼此期待。",
      "近期提醒：避免因猜測取代溝通；重要決定宜先觀察再行動。",
      "測試用途：此內容為 ASP 控制台產生的模擬訂閱信號，不構成真實命理解讀。",
    ].join("\n"),
  }],
  ["fe7cd268-98da-4b4f-89b2-108ee7a446c2", {
    serviceName: "每日個人運程",
    signal: [
      "【每日個人運程｜模擬信號】",
      "今日整體：節奏平穩，適合整理優先級並完成積壓事項。",
      "行動提醒：重大決定先核對資訊，溝通時保留耐心。",
      "測試用途：此內容為 ASP 控制台產生的模擬訂閱信號，不構成真實運勢建議。",
    ].join("\n"),
  }],
]);

export function defaultSignalForService(serviceId, fallbackName = "訂閱服務") {
  const configured = SERVICE_SIGNAL_DEFAULTS.get(String(serviceId));
  if (configured) return configured;
  const serviceName = String(fallbackName || "訂閱服務");
  return {
    serviceName,
    signal: [
      `【${serviceName}｜模擬信號】`,
      "這是一則由 ASP 控制台產生的服務專用測試信號。",
      `serviceId: ${serviceId || "unknown"}`,
      "測試用途：simulation=true。",
    ].join("\n"),
  };
}

export function messageEligibilityForStatus(status, verified = true) {
  if (!verified) return { eligible: false, label: "UNVERIFIED — 未能確認 message-eligible" };
  if (Number(status) === 1) {
    return { eligible: true, label: "ELIGIBLE — ACTIVE，可進入 xmtp-send 檢查" };
  }
  return { eligible: false, label: `INELIGIBLE — ${subscriptionStatusLabel(status)}` };
}

export function parsePeerFromSessionKey(sessionKey, agentId) {
  if (typeof sessionKey !== "string") return null;
  const match = /^job:[^:]+:my:([^:]+):to:(.+)$/.exec(sessionKey);
  if (!match || decodeURIComponent(match[1]) !== String(agentId)) return null;
  return decodeURIComponent(match[2]);
}

export function buildSignalMessage({ jobId, signalText }) {
  return [
    `jobId: ${jobId}`,
    "deliverableType: text",
    "- - -",
    "[SIMULATED_SIGNAL]",
    signalText,
    "simulation: true",
    "- - -",
    "[intent:deliver]",
  ].join("\n");
}

export function buildXmtpSendArgs({ jobId, agentId, toAgentId, message }) {
  return [
    "xmtp-send",
    "--job-id",
    jobId,
    "--session-agent-id",
    agentId,
    "--to-agent-id",
    toAgentId,
    "--message",
    message,
    "--json",
  ];
}

export function subscriptionStatusLabel(status, statusName) {
  const labels = new Map([
    [-1, "INIT — 尚未上鏈"],
    [0, "CREATED — 等待 ASP 接受"],
    [1, "ACTIVE — 可以發送訂閱交付"],
    [3, "REJECTED — Buyer 已拒絕本期交付"],
    [4, "DISPUTED — 仲裁中"],
    [6, "COMPLETED — 已完成"],
    [7, "CLOSED — 已關閉"],
    [8, "EXPIRED — 已過期"],
    [9, "FAILED — 已退款"],
  ]);
  return labels.get(Number(status)) || String(statusName || `UNKNOWN_${status}`);
}

function validateId(value, field, pattern) {
  if (typeof value !== "string" || !pattern.test(value)) {
    throw new Error(`invalid ${field}`);
  }
  return value;
}

async function runJson(a2aBin, args, options = {}) {
  const { stdout } = await execFileAsync(a2aBin, args, {
    encoding: "utf8",
    timeout: options.timeout || 20_000,
    maxBuffer: 1024 * 1024,
  });
  const parsed = JSON.parse(stdout.trim());
  if (parsed.ok !== true) {
    throw new Error(parsed.error || `${args[0]} failed`);
  }
  return parsed;
}

export async function listAspAgents({ onchainosBin, run = runJson }) {
  const accounts = [];
  const pageSize = 50;
  for (let page = 1; page <= 20; page += 1) {
    const result = await run(onchainosBin, [
      "agent", "get", "--page", String(page), "--page-size", String(pageSize),
    ]);
    const batch = Array.isArray(result.data?.list) ? result.data.list : [];
    accounts.push(...batch);
    const total = Number(result.data?.total || accounts.length);
    if (accounts.length >= total || batch.length === 0) break;
  }
  return accounts.flatMap((account) => (account.agentList || [])
    .filter((agent) => Number(agent.role) === 2 || agent.roleLabel === "ASP")
    .map((agent) => ({
      agentId: String(agent.agentId),
      name: agent.name || `ASP #${agent.agentId}`,
      status: agent.status,
      statusLabel: agent.statusLabel || null,
      approvalLabel: agent.approvalLabel || null,
      onlineStatus: agent.onlineStatus,
      communicationAddress: agent.communicationAddress || null,
      profileDescription: agent.profileDescription || null,
      profilePicture: agent.profilePicture || null,
    })));
}

export async function listSubscriptionJobs({
  a2aBin,
  onchainosBin,
  agentId,
  buyerAgentId,
  run = runJson,
}) {
  const [activeResult, providerResult, sessionResult] = await Promise.allSettled([
    run(onchainosBin, ["agent", "subscribe-active", "--agent-id", agentId]),
    run(onchainosBin, ["agent", "my-subscriptions", "--role", "provider"]),
    run(a2aBin, [
      "session",
      "query",
      "--my-agent-id",
      agentId,
      "--limit",
      "100",
      "--json",
    ]),
  ]);
  const warnings = [];
  const active = activeResult.status === "fulfilled" && Array.isArray(activeResult.value.data)
    ? activeResult.value.data
    : [];
  if (activeResult.status === "rejected") {
    warnings.push(`訂閱列表查詢失敗：${activeResult.reason?.message || activeResult.reason}`);
  }
  const providerSubscriptions = providerResult.status === "fulfilled"
    ? (providerResult.value.data?.list || []).filter(
      (job) => String(job.providerAgentId) === agentId &&
        String(job.buyerAgentId) === buyerAgentId,
    )
    : [];
  if (providerResult.status === "rejected") {
    warnings.push(`ASP 訂閱詳情列表查詢失敗：${providerResult.reason?.message || providerResult.reason}`);
  }
  const sessions = sessionResult.status === "fulfilled"
    ? (sessionResult.value.sessions || []).filter(
      (session) => String(session.myAgentId) === agentId &&
        String(session.toAgentId) === buyerAgentId,
    )
    : [];
  if (sessionResult.status === "rejected") {
    warnings.push(`A2A session 查詢失敗：${sessionResult.reason?.message || sessionResult.reason}`);
  }
  const sessionByJob = new Map(sessions.map((session) => [String(session.jobId), session]));
  const activeIds = new Set(active.map((job) => String(job.jobId)));
  const jobs = providerSubscriptions.map((job) => {
    const session = sessionByJob.get(String(job.jobId));
    const status = Number(job.status);
    const activeFromBackend = activeIds.has(String(job.jobId)) || status === 1;
    const serviceDefaults = defaultSignalForService(job.serviceId, job.title);
    const eligibility = messageEligibilityForStatus(status);
    return {
      ...job,
      jobId: String(job.jobId),
      toAgentId: session?.toAgentId || buyerAgentId,
      codexSessionId: session?.codexSessionId || null,
      sessionUpdatedAt: session?.updatedAt || null,
      status,
      statusLabel: subscriptionStatusLabel(status, job.statusName),
      serviceName: serviceDefaults.serviceName,
      defaultSignal: serviceDefaults.signal,
      messageEligibility: eligibility,
      source: session ? "provider-subscription + a2a-session" : "provider-subscription",
      sendable: activeFromBackend,
      canAccept: status === 0,
    };
  });
  const knownIds = new Set(providerSubscriptions.map((job) => String(job.jobId)));
  for (const session of sessions) {
    if (knownIds.has(String(session.jobId))) continue;
    jobs.push({
      jobId: String(session.jobId),
      toAgentId: String(session.toAgentId),
      codexSessionId: session.codexSessionId || null,
      sessionUpdatedAt: session.updatedAt || null,
      source: "a2a-session (未確認仍為 ACTIVE 訂閱)",
      sendable: false,
      canAccept: false,
      messageEligibility: messageEligibilityForStatus(null, false),
    });
  }
  const enrichedJobs = await Promise.all(jobs.map(async (job) => {
    if (job.source.startsWith("provider-subscription")) {
      return job;
    }
    try {
      const detail = await run(onchainosBin, [
        "agent",
        "subscribe-detail",
        job.jobId,
        "--format",
        "json",
      ]);
      const statusName = String(detail.data?.statusName || `status_${detail.data?.status ?? "unknown"}`);
      return {
        ...job,
        status: detail.data?.status ?? null,
        statusName,
        statusLabel: subscriptionStatusLabel(detail.data?.status, statusName),
        source: "a2a-session + subscription-detail",
        sendable: Number(detail.data?.status) === 1,
        canAccept: Number(detail.data?.status) === 0,
        messageEligibility: messageEligibilityForStatus(detail.data?.status),
      };
    } catch (error) {
      return {
        ...job,
        statusName: "UNVERIFIED",
        statusLabel: "UNVERIFIED — 無法取得訂閱狀態",
        sendable: false,
        canAccept: false,
        messageEligibility: messageEligibilityForStatus(null, false),
        detailWarning: error instanceof Error ? error.message : String(error),
      };
    }
  }));
  return { jobs: enrichedJobs, warnings };
}

function containsServiceId(value, serviceId) {
  if (Array.isArray(value)) return value.some((item) => containsServiceId(item, serviceId));
  if (!value || typeof value !== "object") return false;
  if (String(value.serviceId || "") === serviceId) return true;
  return Object.values(value).some((item) => containsServiceId(item, serviceId));
}

export async function acceptSubscription({ onchainosBin, jobId, agentId, run = runJson }) {
  const detail = await run(onchainosBin, [
    "agent",
    "subscribe-detail",
    jobId,
    "--format",
    "json",
  ]);
  const subscription = detail.data || {};
  if (String(subscription.jobId || "") !== jobId) {
    throw new Error("subscription detail returned a different jobId");
  }
  if (String(subscription.providerAgentId || "") !== agentId) {
    throw new Error(`job is assigned to ASP #${subscription.providerAgentId || "unknown"}, not #${agentId}`);
  }
  const status = Number(subscription.status);
  if (status === 1) {
    return { ok: true, duplicate: true, detail: subscription };
  }
  if (status !== 0) {
    throw new Error(`subscription cannot be accepted from ${subscriptionStatusLabel(status, subscription.statusName)}`);
  }
  const serviceId = String(subscription.serviceId || "");
  if (!serviceId) throw new Error("subscription serviceId is missing");
  const services = await run(onchainosBin, [
    "agent",
    "service-list",
    "--agent-id",
    agentId,
    "--service-id",
    serviceId,
  ]);
  if (!containsServiceId(services.data, serviceId)) {
    throw new Error(`designated service ${serviceId} is not registered under ASP #${agentId}`);
  }
  const result = await run(onchainosBin, [
    "agent",
    "accept-subscription",
    jobId,
    "--agent-id",
    agentId,
  ], { timeout: 120_000 });
  return { ok: true, duplicate: false, serviceId, result };
}

export async function resolvePeerAgentId({
  a2aBin,
  jobId,
  agentId,
  sessionKey,
  toAgentId,
}) {
  if (toAgentId) {
    return validateId(String(toAgentId), "toAgentId", /^\d+$/);
  }

  const fromSession = parsePeerFromSessionKey(sessionKey, agentId);
  if (fromSession) return validateId(fromSession, "toAgentId", /^\d+$/);

  const result = await runJson(a2aBin, [
    "session",
    "query",
    "--job-id",
    jobId,
    "--my-agent-id",
    agentId,
    "--limit",
    "20",
    "--json",
  ]);
  const peers = [
    ...new Set(
      (result.sessions || [])
        .map((session) => session.toAgentId)
        .filter((value) => typeof value === "string" && /^\d+$/.test(value)),
    ),
  ];
  if (peers.length !== 1) {
    throw new Error(
      `cannot resolve one peer for job ${jobId}; found ${peers.length}. Pass toAgentId explicitly.`,
    );
  }
  return peers[0];
}

export async function sendSignal({
  a2aBin,
  jobId,
  agentId,
  toAgentId,
  signalText,
  dryRun,
}) {
  const message = buildSignalMessage({ jobId, signalText });
  if (dryRun) {
    return { ok: true, dryRun: true, jobId, agentId, toAgentId, message };
  }
  return runJson(
    a2aBin,
    buildXmtpSendArgs({ jobId, agentId, toAgentId, message }),
  );
}

async function readJsonBody(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_BODY_BYTES) throw new Error("request body too large");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
}

function writeJson(response, statusCode, body) {
  response.writeHead(statusCode, {
    "cache-control": "no-store",
    "content-type": "application/json; charset=utf-8",
  });
  response.end(`${JSON.stringify(body)}\n`);
}

function writeHtml(response, body) {
  response.writeHead(200, {
    "cache-control": "no-store",
    "content-type": "text/html; charset=utf-8",
    "x-content-type-options": "nosniff",
  });
  response.end(body);
}

function dashboardHtml() {
  return `<!doctype html>
<html lang="zh-Hant">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>ASP Runtime 唯讀觀察台</title>
  <style>
    :root { color-scheme: dark; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; background:#0b1020; color:#e6edf7; }
    body { margin:0; padding:24px; }
    header { display:flex; gap:16px; align-items:center; justify-content:space-between; margin-bottom:18px; }
    h1 { margin:0; font:700 22px system-ui,sans-serif; }
    .muted { color:#8b9bb4; font:13px system-ui,sans-serif; }
    .grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(220px,1fr)); gap:12px; margin-bottom:18px; }
    .card, details { border:1px solid #27324a; border-radius:10px; background:#11182a; }
    .card { padding:14px; }
    h2 { margin:0 0 12px; font:650 17px system-ui,sans-serif; }
    .jobs { display:grid; gap:10px; margin-bottom:18px; }
    .asps { display:grid; grid-template-columns:repeat(auto-fit,minmax(260px,1fr)); gap:10px; }
    .service-group { border:1px solid #3a4764; border-radius:10px; padding:13px; background:#10192d; }
    .service-head { margin-bottom:10px; }
    details.service-group > summary { display:flex; justify-content:space-between; gap:12px; padding:2px; border:0; }
    details.service-group[open] > summary { border:0; margin-bottom:12px; }
    .service-counts { flex:0 0 auto; color:#8b9bb4; font-size:12px; }
    .asp { display:flex; gap:12px; align-items:center; border:1px solid #27324a; border-radius:8px; padding:12px; background:#0d1425; }
    .asp img { width:48px; height:48px; border-radius:50%; object-fit:cover; background:#17213a; }
    .job { border:1px solid #27324a; border-radius:8px; padding:12px; background:#0d1425; }
    .job-head { display:flex; gap:10px; justify-content:space-between; align-items:start; margin-bottom:9px; }
    .job-id { overflow-wrap:anywhere; color:#93c5fd; }
    textarea, input { box-sizing:border-box; width:100%; border:1px solid #35415b; background:#090e1a; color:#e6edf7; border-radius:7px; padding:9px; font:inherit; }
    textarea, input, .send-row, .card > .job, .service-group > .label { display:none; }
    textarea { min-height:72px; resize:vertical; margin:8px 0; }
    .send-row { display:flex; gap:8px; align-items:center; }
    .send-status { color:#8b9bb4; font-size:12px; overflow-wrap:anywhere; }
    .label { color:#8b9bb4; font-size:12px; margin-bottom:6px; }
    .value { overflow-wrap:anywhere; }
    #events { display:grid; gap:12px; }
    details { overflow:hidden; }
    summary { cursor:pointer; display:grid; grid-template-columns:170px 100px 1fr; gap:12px; padding:13px 15px; align-items:center; }
    details[open] summary { border-bottom:1px solid #27324a; }
    .body { padding:14px; display:grid; gap:12px; }
    .section { min-width:0; }
    pre { margin:5px 0 0; padding:12px; overflow:auto; white-space:pre-wrap; word-break:break-word; background:#090e1a; border-radius:7px; color:#cbd7ea; }
    .ok { color:#4ade80; } .error { color:#fb7185; } .processing { color:#fbbf24; } .deduplicated { color:#60a5fa; }
    button { border:1px solid #35415b; background:#17213a; color:#e6edf7; border-radius:7px; padding:8px 12px; cursor:pointer; }
    button:disabled, textarea:disabled { cursor:not-allowed; opacity:.45; }
    @media (max-width:700px) { body{padding:14px} summary{grid-template-columns:1fr}.muted{display:none} }
  </style>
</head>
<body>
  <header><div><h1>ASP Runtime 唯讀觀察台</h1><div class="muted">只讀取身份、訂閱、session 與歷史紀錄；不接受訂閱、不生成交付、不發送 XMTP。</div></div><button id="refresh">立即刷新</button></header>
  <div class="grid" id="config"></div>
  <section class="card" style="margin-bottom:18px"><h2>本帳戶的 ASP</h2><div class="label">來源：onchainos agent get。標記「本控制台」的 ASP 會處理下方訂閱。</div><div class="asps" id="asps"><div class="muted">載入中…</div></div></section>
  <section class="card" style="margin-bottom:18px"><h2>ASP 訂閱 Jobs</h2><div class="label">正式生命週期由 ASP runtime 自主處理；本頁只顯示後端與 A2A session 狀態。</div><input id="auth" type="password"><div class="job"><input id="manual-job"><textarea data-message="manual"></textarea><div class="send-row"><button data-send="manual"></button><span data-status="manual"></span></div></div><div id="job-warning" class="error"></div><div class="jobs" id="jobs"><div class="muted">載入中…</div></div></section>
  <h2>歷史紀錄</h2>
  <div id="events"><div class="card muted">尚未收到訊息</div></div>
  <script>
    const pretty = value => JSON.stringify(value, null, 2);
    let runtimeConfig = {};
    let knownJobs = new Map();
    let knownServices = new Map();
    const text = value => String(value ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
    function section(label, value) { return '<div class="section"><div class="label">'+text(label)+'</div><pre>'+text(typeof value === "string" ? value : pretty(value))+'</pre></div>'; }
    async function refreshAsps() {
      const response = await fetch('/api/asps', { cache:'no-store' });
      const data = await response.json();
      document.querySelector('#asps').innerHTML = data.asps.length ? data.asps.map(asp => '<div class="asp">'+(asp.profilePicture?'<img src="'+text(asp.profilePicture)+'" alt="">':'<div></div>')+'<div><div class="value">'+text(asp.name)+' · #'+text(asp.agentId)+(asp.agentId===data.configuredAgentId?' <span class="ok">（本控制台）</span>':'')+'</div><div class="label">'+text(asp.statusLabel || 'unknown')+' · '+text(asp.approvalLabel || '')+' · '+(Number(asp.onlineStatus)===1?'在線':'離線')+'</div><div class="muted">'+text(asp.profileDescription || '')+'</div></div></div>').join('') : '<div class="muted">目前帳戶沒有 ASP。</div>';
    }
    async function refreshJobs() {
      const response = await fetch('/api/jobs', { cache:'no-store' });
      const data = await response.json();
      knownJobs = new Map(data.jobs.map(job => [job.jobId, job]));
      knownServices = new Map();
      data.jobs.forEach((job, index) => {
        job.uiIndex = index;
        const serviceId = job.serviceId || 'unknown-service';
        if (!knownServices.has(serviceId)) knownServices.set(serviceId, []);
        knownServices.get(serviceId).push(job);
      });
      document.querySelector('#job-warning').textContent = (data.warnings || []).join('；');
      const jobHtml = job => '<div class="job"><div class="job-head"><div><div class="job-id">'+text(job.title || '未命名訂閱')+' · '+text(job.jobId)+'</div><div class="value '+(job.sendable?'ok':'processing')+'">'+text(job.statusLabel || job.statusName || 'UNVERIFIED')+'</div><div class="label">message-eligible：'+text(job.messageEligibility?.label || 'UNVERIFIED')+'</div><div class="label">ASP #'+text(job.providerAgentId || runtimeConfig.aspAgentId)+' → Buyer #'+text(job.toAgentId)+' · '+text(job.source)+(job.codexSessionId?' · Codex '+text(job.codexSessionId):'')+'</div></div></div><div class="send-row">'+(job.canAccept?'<button data-accept="'+job.uiIndex+'" data-job="'+text(job.jobId)+'">ASP 接受訂閱</button>':'')+'<span class="send-status" data-action-status="'+job.uiIndex+'"></span></div><textarea data-message="'+job.uiIndex+'" placeholder="單 Job 信號；留空時可使用上方服務群組信號" '+(job.sendable?'':'disabled')+'></textarea><div class="send-row"><button data-send="'+job.uiIndex+'" data-job="'+text(job.jobId)+'" data-to="'+text(job.toAgentId)+'" '+(job.sendable?'':'disabled')+'>只發送此 Job</button><span class="send-status" data-status="'+job.uiIndex+'">'+(job.sendable?'':'只有 ACTIVE 訂閱可以發送')+'</span></div></div>';
      document.querySelector('#jobs').innerHTML = data.jobs.length ? [...knownServices.entries()].map(([serviceId, jobs], groupIndex) => '<details class="service-group" '+(jobs.some(job => job.sendable || job.canAccept)?'open':'')+'><summary><div><div class="value">服務：'+text(jobs[0].serviceName || jobs[0].title || '未命名服務')+'</div><div class="job-id">serviceId: '+text(serviceId)+'</div></div><div class="service-counts">'+jobs.length+' Jobs · '+jobs.filter(job => job.sendable).length+' 可通過 · '+jobs.filter(job => job.canAccept).length+' 待接受</div></summary><div class="label">此 serviceId 的預設模擬信號（可編輯）</div><textarea data-service-message="'+groupIndex+'" placeholder="輸入此 serviceId 專用的模擬信號">'+text(jobs[0].defaultSignal || '')+'</textarea><div class="send-row"><button data-send-service="'+groupIndex+'" data-service-id="'+text(serviceId)+'" '+(jobs.some(job => job.sendable)?'':'disabled')+'>推送到本服務所有 ACTIVE 訂閱</button><span class="send-status" data-service-status="'+groupIndex+'">'+(jobs.some(job => job.sendable)?'會逐筆執行 okx-a2a message-eligible':'目前沒有 ACTIVE job，無法通過 message-eligible')+'</span></div><div class="jobs">'+jobs.map(jobHtml).join('')+'</div></details>').join('') : '<div class="muted">目前沒有可用的訂閱。</div>';
    }
    function requestHeaders() {
      const headers = { 'content-type':'application/json' };
      const token = document.querySelector('#auth').value;
      if (token) headers.authorization = 'Bearer '+token;
      return headers;
    }
    async function postSignal(job, signalText) {
      const response = await fetch('/signal', { method:'POST', headers:requestHeaders(), body:JSON.stringify({ jobId:job.jobId, toAgentId:job.toAgentId, signalText, messageId:crypto.randomUUID() }) });
      const result = await response.json();
      if (!response.ok || result.ok !== true) throw new Error(result.error || 'HTTP '+response.status);
      return result;
    }
    async function sendService(button) {
      const index = button.dataset.sendService;
      const status = document.querySelector('[data-service-status="'+index+'"]');
      const signalText = document.querySelector('[data-service-message="'+index+'"]').value.trim();
      if (!signalText) { status.textContent = '請先輸入此服務的信號內容'; return; }
      const jobs = (knownServices.get(button.dataset.serviceId) || []).filter(job => job.sendable);
      button.disabled = true; status.textContent = '正在推送 '+jobs.length+' 個 ACTIVE 訂閱…';
      const results = await Promise.allSettled(jobs.map(job => postSignal(job, signalText)));
      const success = results.filter(result => result.status === 'fulfilled').length;
      const failed = results.length - success;
      status.textContent = '完成：'+success+' 成功'+(failed?'，'+failed+' 失敗':'');
      button.disabled = false; await refresh();
    }
    async function acceptFromJob(button) {
      const index = button.dataset.accept;
      const status = document.querySelector('[data-action-status="'+index+'"]');
      if (!confirm('確認由 ASP #'+runtimeConfig.aspAgentId+' 接受訂閱 '+button.dataset.job+'？這會簽署並廣播鏈上交易。')) return;
      button.disabled = true; status.textContent = '接受中，等待鏈上確認…';
      try {
        const response = await fetch('/api/subscriptions/accept', { method:'POST', headers:requestHeaders(), body:JSON.stringify({ jobId:button.dataset.job, agentId:runtimeConfig.aspAgentId }) });
        const result = await response.json();
        if (!response.ok || result.ok !== true) throw new Error(result.error || 'HTTP '+response.status);
        status.textContent = result.result?.duplicate ? '已經是 ACTIVE' : '接受成功，等待狀態刷新';
        await Promise.all([refresh(),refreshJobs()]);
      } catch (error) { status.textContent = '接受失敗：'+error.message; button.disabled = false; }
    }
    async function sendFromJob(button) {
      const index = button.dataset.send;
      const ownSignal = document.querySelector('[data-message="'+index+'"]').value.trim();
      const serviceSignal = button.closest('.service-group')?.querySelector('[data-service-message]')?.value.trim() || '';
      const signalText = ownSignal || serviceSignal;
      const status = document.querySelector('[data-status="'+index+'"]');
      if (!signalText) { status.textContent = '請先輸入信號內容'; return; }
      const jobId = button.dataset.job || document.querySelector('#manual-job').value.trim();
      const toAgentId = button.dataset.to || runtimeConfig.buyerAgentId;
      if (!jobId) { status.textContent = '請先輸入 Job ID'; return; }
      const knownJob = knownJobs.get(jobId);
      if (knownJob && !knownJob.sendable) { status.textContent = knownJob.statusLabel+'，不可發送'; return; }
      button.disabled = true; status.textContent = '發送中…';
      try {
        const result = await postSignal({ jobId, toAgentId }, signalText);
        status.textContent = '已送出'+(result.result?.messageId ? ' · '+result.result.messageId : '');
        await refresh();
      } catch (error) { status.textContent = '失敗：'+error.message; }
      finally { button.disabled = false; }
    }
    async function refresh() {
      const response = await fetch('/api/events', { cache:'no-store' });
      const data = await response.json();
      runtimeConfig = data.config;
      document.querySelector('#config').innerHTML = Object.entries(data.config).map(([k,v]) => '<div class="card"><div class="label">'+text(k)+'</div><div class="value">'+text(v)+'</div></div>').join('');
      document.querySelector('#events').innerHTML = data.events.length ? data.events.map((event, index) => '<details '+(index===0?'open':'')+'><summary><span>'+text(event.receivedAt)+'</span><strong class="'+text(event.status)+'">'+text(event.status)+'</strong><span>'+text(event.endpoint)+' · '+text(event.jobId || 'unknown job')+'</span></summary><div class="body">'+section('HTTP / dispatcher input',event.input)+section('Resolved routing',event.routing)+section('okx-a2a xmtp-send argv',event.xmtp?.args || [])+section('XMTP message payload',event.xmtp?.message || '')+section('Send result',event.result || event.error || 'processing')+'</div></details>').join('') : '<div class="card muted">尚未收到訊息</div>';
    }
    document.querySelector('#jobs').addEventListener('click', event => { const sendButton=event.target.closest('[data-send]'); if(sendButton) sendFromJob(sendButton); const serviceButton=event.target.closest('[data-send-service]'); if(serviceButton) sendService(serviceButton); const acceptButton=event.target.closest('[data-accept]'); if(acceptButton) acceptFromJob(acceptButton); });
    document.querySelector('[data-send="manual"]').addEventListener('click', event => sendFromJob(event.currentTarget));
    document.querySelector('#refresh').addEventListener('click', () => Promise.all([refresh(),refreshAsps(),refreshJobs()]));
    Promise.all([refresh(),refreshAsps(),refreshJobs()]).catch(console.error); setInterval(() => refresh().catch(console.error), 2000);
  </script>
</body>
</html>`;
}

export function createRequestHandler(options = {}) {
  const agentId = String(options.agentId || process.env.ASP_AGENT_ID || DEFAULT_AGENT_ID);
  const buyerAgentId = String(
    options.buyerAgentId || process.env.BUYER_AGENT_ID || DEFAULT_BUYER_AGENT_ID,
  );
  const defaultJobId = String(options.jobId || process.env.ASP_JOB_ID || "");
  const a2aBin = options.a2aBin || process.env.OKX_A2A_BIN || DEFAULT_A2A_BIN;
  const onchainosBin = options.onchainosBin || process.env.ONCHAINOS_BIN || DEFAULT_ONCHAINOS_BIN;
  const authToken = options.authToken ?? process.env.SIMULATOR_AUTH_TOKEN ?? "";
  const dryRun = options.dryRun ?? process.env.DRY_RUN === "1";
  const defaultSignal =
    options.signalText ||
    process.env.SIMULATED_SIGNAL_TEXT ||
    "這是一則由 ASP #11926 發送的模擬訂閱消息。";
  const resolvePeer = options.resolvePeer || resolvePeerAgentId;
  const send = options.send || sendSignal;
  const listAsps = options.listAsps || (() => listAspAgents({ onchainosBin }));
  const listJobs = options.listJobs || (() => listSubscriptionJobs({
    a2aBin,
    onchainosBin,
    agentId,
    buyerAgentId,
  }));
  const accept = options.accept || ((input) => acceptSubscription({
    onchainosBin,
    ...input,
  }));
  const processed = new Map();
  const events = [];
  const config = {
    mode: "read-only",
    aspAgentId: agentId,
    buyerAgentId,
    defaultJobId: defaultJobId || "由每次請求提供",
    xmtpBinary: a2aBin,
    onchainosBinary: onchainosBin,
  };

  function addEvent(event) {
    events.unshift(event);
    if (events.length > MAX_EVENTS) events.length = MAX_EVENTS;
    options.onEvent?.(event);
    return event;
  }

  return async (request, response) => {
    let event;
    try {
      if (request.method === "GET" && request.url === "/") {
        writeHtml(response, dashboardHtml());
        return;
      }
      if (request.method === "GET" && request.url === "/api/events") {
        writeJson(response, 200, { ok: true, config, events });
        return;
      }
      if (request.method === "GET" && request.url === "/api/asps") {
        writeJson(response, 200, { ok: true, configuredAgentId: agentId, asps: await listAsps() });
        return;
      }
      if (request.method === "GET" && request.url === "/api/jobs") {
        writeJson(response, 200, { ok: true, ...await listJobs() });
        return;
      }
      if (request.method === "GET" && request.url === "/health") {
        writeJson(response, 200, {
          ok: true,
          agentId,
          buyerAgentId,
          defaultJobId: defaultJobId || null,
          dryRun,
        });
        return;
      }
      if (
        request.method === "POST" &&
        ["/dispatch", "/signal", "/api/subscriptions/accept"].includes(request.url)
      ) {
        writeJson(response, 405, {
          ok: false,
          error: "read-only dashboard: ASP lifecycle actions are handled by the agent runtime",
        });
        return;
      }
      const isDispatch = request.method === "POST" && request.url === "/dispatch";
      const isSignal = request.method === "POST" && request.url === "/signal";
      const isAccept = request.method === "POST" && request.url === "/api/subscriptions/accept";
      if (!isDispatch && !isSignal && !isAccept) {
        writeJson(response, 404, { ok: false, error: "not found" });
        return;
      }
      if (authToken && request.headers.authorization !== `Bearer ${authToken}`) {
        writeJson(response, 401, { ok: false, error: "unauthorized" });
        return;
      }

      const body = await readJsonBody(request);
      event = addEvent({
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        receivedAt: new Date().toISOString(),
        endpoint: request.url,
        status: "processing",
        input: body,
      });
      if (isAccept) {
        const acceptedAgentId = validateId(String(body.agentId || ""), "agentId", /^\d+$/);
        if (acceptedAgentId !== agentId) {
          throw new Error(`ASP #${acceptedAgentId} is not configured for this console`);
        }
        const acceptedJobId = validateId(
          String(body.jobId || ""),
          "jobId",
          /^[A-Za-z0-9._:-]{1,160}$/,
        );
        Object.assign(event, {
          jobId: acceptedJobId,
          routing: { jobId: acceptedJobId, agentId: acceptedAgentId, action: "accept-subscription" },
          command: {
            executable: onchainosBin,
            args: ["agent", "accept-subscription", acceptedJobId, "--agent-id", acceptedAgentId],
          },
        });
        const result = await accept({ jobId: acceptedJobId, agentId: acceptedAgentId });
        Object.assign(event, { status: "ok", result, completedAt: new Date().toISOString() });
        writeJson(response, 200, { ok: true, jobId: acceptedJobId, agentId: acceptedAgentId, result });
        return;
      }
      const receivedAgentId = validateId(
        String(isDispatch ? body.agentId || "" : agentId),
        "agentId",
        /^\d+$/,
      );
      if (receivedAgentId !== agentId) {
        throw new Error(`agentId ${receivedAgentId} is not configured ASP ${agentId}`);
      }
      const jobId = validateId(
        String(body.jobId || defaultJobId),
        "jobId",
        /^[A-Za-z0-9._:-]{1,160}$/,
      );
      const messageId = body.messageId ? String(body.messageId) : "";
      if (messageId && processed.has(messageId)) {
        const cached = processed.get(messageId);
        Object.assign(event, {
          status: "deduplicated",
          jobId,
          result: cached,
          completedAt: new Date().toISOString(),
        });
        writeJson(response, 200, cached);
        return;
      }

      const toAgentId = await resolvePeer({
        a2aBin,
        jobId,
        agentId,
        sessionKey: body.sessionKey,
        toAgentId: body.toAgentId || (isSignal ? buyerAgentId : undefined),
      });
      if (toAgentId !== buyerAgentId) {
        throw new Error(`toAgentId ${toAgentId} is not configured Buyer ${buyerAgentId}`);
      }
      const signalText = typeof body.signalText === "string" && body.signalText.trim()
        ? body.signalText.trim()
        : defaultSignal;
      const message = buildSignalMessage({ jobId, signalText });
      const args = buildXmtpSendArgs({ jobId, agentId, toAgentId, message });
      Object.assign(event, {
        jobId,
        routing: { jobId, agentId, toAgentId, messageId: messageId || null, dryRun },
        xmtp: { executable: a2aBin, args, message },
      });
      const result = await send({
        a2aBin,
        jobId,
        agentId,
        toAgentId,
        signalText,
        dryRun,
      });
      const reply = { ok: true, jobId, agentId, toAgentId, dryRun, result };
      if (messageId) processed.set(messageId, reply);
      Object.assign(event, {
        status: "ok",
        result,
        completedAt: new Date().toISOString(),
      });
      writeJson(response, 200, reply);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (event) {
        Object.assign(event, {
          status: "error",
          error: message,
          completedAt: new Date().toISOString(),
        });
      } else {
        addEvent({
          id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
          receivedAt: new Date().toISOString(),
          completedAt: new Date().toISOString(),
          endpoint: request.url,
          status: "error",
          input: null,
          error: message,
        });
      }
      writeJson(response, 400, {
        ok: false,
        error: message,
      });
    }
  };
}

export function startServer(options = {}) {
  const host = options.host || process.env.HOST || "127.0.0.1";
  const port = Number(options.port || process.env.PORT || 4319);
  const server = http.createServer(createRequestHandler(options));
  server.listen(port, host, () => {
    const address = server.address();
    const actualPort = typeof address === "object" && address ? address.port : port;
    process.stderr.write(
      `[asp-xmtp-simulator] listening on http://${host}:${actualPort} for ASP #${options.agentId || process.env.ASP_AGENT_ID || DEFAULT_AGENT_ID}\n`,
    );
  });
  return server;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  startServer();
}
