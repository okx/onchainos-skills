#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");

const toolName = process.argv[2];
const rawParams = process.argv[3] || "{}";

function fail(message, code = 1) {
  console.error(message);
  process.exit(code);
}

function parseParams(raw) {
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      fail("params must be a JSON object");
    }
    return parsed;
  } catch (err) {
    fail(`failed to parse params JSON: ${err instanceof Error ? err.message : String(err)}`);
  }
}

function readCommand(envName, fallback) {
  const raw = process.env[envName] || fallback;
  return raw.match(/(?:[^\s"]+|"[^"]*")+/g)?.map((part) => part.replace(/^"|"$/g, "")) || [fallback];
}

function run(command, args) {
  const result = runResult(command, args);
  if (result.error) {
    fail(`${result.bin} failed: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(formatCommandFailure(result));
  }
  return result.stdout.trim();
}

function runResult(command, args) {
  const [bin, ...prefix] = command;
  const result = spawnSync(bin, [...prefix, ...args], {
    encoding: "utf8",
    env: process.env,
  });
  return {
    bin,
    prefix,
    args,
    error: result.error,
    status: result.status,
    stdout: result.stdout || "",
    stderr: result.stderr || "",
  };
}

function formatCommandFailure(result) {
  return [
    `${[result.bin, ...result.prefix, ...result.args].join(" ")} exited with code ${result.status}`,
    result.stdout ? `stdout:\n${result.stdout.trim()}` : "",
    result.stderr ? `stderr:\n${result.stderr.trim()}` : "",
  ].filter(Boolean).join("\n");
}

function parseJsonOutput(raw, label) {
  try {
    return JSON.parse(raw);
  } catch (err) {
    fail(`${label} returned non-JSON output: ${err instanceof Error ? err.message : String(err)}\n${raw}`);
  }
}

function tryParseJson(raw) {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

const OKX_A2A = readCommand("OKX_A2A_BIN", "okx-a2a");
const ONCHAINOS = readCommand("ONCHAINOS_BIN", "onchainos");

function required(params, key) {
  const value = params[key];
  if (typeof value !== "string" || value.length === 0) {
    fail(`${toolName} requires params.${key}`);
  }
  return value;
}

function optionalString(params, key) {
  const value = params[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function optionalPositiveInt(params, key) {
  const value = params[key];
  if (typeof value === "number" && Number.isInteger(value) && value > 0) {
    return value;
  }
  if (typeof value === "string" && /^\d+$/.test(value) && Number(value) > 0) {
    return Number(value);
  }
  return null;
}

function parseModernSessionKey(sessionKey) {
  const parts = sessionKey.split(":");
  if (parts.length !== 6 || parts[0] !== "job" || parts[2] !== "my" || parts[4] !== "to") {
    return null;
  }
  return {
    jobId: decodeURIComponent(parts[1]),
    myAgentId: decodeURIComponent(parts[3]),
    toAgentId: decodeURIComponent(parts[5]),
  };
}

function buildModernSessionKey(jobId, myAgentId, toAgentId) {
  return [
    "job",
    encodeURIComponent(jobId),
    "my",
    encodeURIComponent(myAgentId || "unknown"),
    "to",
    encodeURIComponent(toAgentId || "unknown"),
  ].join(":");
}

function buildLegacyScopeKey(params) {
  const query = new URLSearchParams({
    my: params.myXmtpAddress,
    to: params.toXmtpAddress,
    job: params.jobId,
    gid: params.groupId,
  });
  return `okx-xmtp:${query.toString()}`;
}

function getSession(sessionKey) {
  try {
    const raw = run(OKX_A2A, ["session", "get", "--session-key", sessionKey, "--json"]);
    const parsed = JSON.parse(raw);
    return parsed && parsed.ok ? parsed.session : null;
  } catch {
    return null;
  }
}

function resolveSession(sessionKey) {
  const stored = getSession(sessionKey);
  if (stored) {
    return {
      sessionKey,
      jobId: stored.jobId || stored.job_id || null,
      myAgentId: stored.myAgentId || stored.my_agent_id || null,
      toAgentId: stored.toAgentId || stored.to_agent_id || null,
      groupId: stored.groupId || stored.group_id || null,
      toAddress: stored.toAgentXmtpAddress || stored.to_agent_xmtp_address || null,
    };
  }
  const modern = parseModernSessionKey(sessionKey);
  return modern ? { sessionKey, ...modern } : null;
}

function extractMarker(text, names) {
  for (const name of names) {
    const bracket = new RegExp(`\\[${name}\\s*:\\s*([^\\]]+)\\]`, "i").exec(text);
    if (bracket) {
      return bracket[1].trim();
    }
    const loose = new RegExp(`${name}\\s*[:=]\\s*([^\\s,;]+)`, "i").exec(text);
    if (loose) {
      return loose[1].trim();
    }
  }
  return null;
}

function appendOptional(args, flag, value) {
  if (typeof value === "string" && value.length > 0 && value !== "unknown") {
    args.push(flag, value);
  }
}

function readSessionKey(session) {
  return session?.sessionKey || session?.session_key || null;
}

function normalizeSessionKey(value) {
  return String(value || "").replace(/\s+/g, "");
}

function handleStartConversation(params) {
  const args = [
    "session",
    "create",
    "--job-id", required(params, "jobId"),
    "--my-agent-id", required(params, "myAgentId"),
    "--to-agent-id", required(params, "toAgentId"),
    "--json",
  ];
  appendOptional(args, "--group-id", optionalString(params, "groupId"));
  const raw = run(OKX_A2A, args);
  const parsed = parseJsonOutput(raw, "okx-a2a session create");
  const sessionKey = readSessionKey(parsed.session);
  const groupId = parsed.session?.groupId || parsed.session?.group_id || params.groupId || "(created on first xmtp_send)";
  console.log(`Group chat prepared: sessionKey=${sessionKey}, groupId=${groupId}, jobId=${params.jobId}`);
}

function handleStartEvaluateConversation(params) {
  const args = [
    "session",
    "create",
    "--job-id", required(params, "jobId"),
    "--my-agent-id", required(params, "myAgentId"),
    "--to-agent-id", "_",
    "--group-id", "_",
    "--json",
  ];
  const raw = run(OKX_A2A, args);
  const parsed = parseJsonOutput(raw, "okx-a2a session create");
  const sessionKey = readSessionKey(parsed.session) || buildModernSessionKey(params.jobId, params.myAgentId, "_");
  console.log(`Evaluation session prepared: sessionKey=${sessionKey}, jobId=${params.jobId}`);
}

function handleSend(params) {
  const sessionKey = required(params, "sessionKey");
  const content = required(params, "content");
  const session = resolveSession(sessionKey);
  if (!session?.jobId || !session?.myAgentId) {
    fail(`xmtp_send cannot resolve jobId/myAgentId from sessionKey=${sessionKey}. Run xmtp_start_conversation first or create a matching okx-a2a session.`);
  }
  const args = [
    "xmtp-send",
    "--job-id", session.jobId,
    "--my-agent-id", session.myAgentId,
    "--message", content,
  ];
  if (session.toAgentId && session.toAgentId !== "unknown") {
    args.push("--to-agent-id", session.toAgentId);
  } else if (session.toAddress) {
    args.push("--to-address", session.toAddress);
  } else {
    fail(`xmtp_send cannot resolve toAgentId/toAddress from sessionKey=${sessionKey}`);
  }
  console.log(run(OKX_A2A, args));
}

function handlePromptUser(params) {
  const llmContent = required(params, "llmContent");
  const userContent = required(params, "userContent");
  const args = [
    "user",
    "decision-request",
    "--user-content", userContent,
    "--llm-content", llmContent,
    "--json",
  ];
  appendOptional(args, "--job-id", optionalString(params, "jobId") || extractMarker(llmContent, ["job", "jobId"]));
  appendOptional(args, "--session-key", optionalString(params, "sessionKey") || extractMarker(llmContent, ["session_key", "sessionKey"]));
  console.log(run(OKX_A2A, args));
}

function handleDispatchUser(params) {
  const args = ["user", "notify", "--content", required(params, "content"), "--json"];
  appendOptional(args, "--job-id", optionalString(params, "jobId") || extractMarker(params.content, ["job", "jobId"]));
  appendOptional(args, "--session-key", optionalString(params, "sessionKey") || extractMarker(params.content, ["session_key", "sessionKey"]));
  console.log(run(OKX_A2A, args));
}

function handleDispatchSession(params) {
  const sessionKey = optionalString(params, "sessionKey") || "main";
  const content = required(params, "content");
  console.log(run(OKX_A2A, [
    "session", "send",
    "--session-key", sessionKey,
    "--content", content,
    "--no-wait",
    "--json",
  ]));
}

function handleConversationHistory(params) {
  const raw = run(OKX_A2A, [
    "session", "get",
    "--session-key", required(params, "sessionKey"),
    "--json",
  ]);
  const parsed = parseJsonOutput(raw, "okx-a2a session get");
  const messages = Array.isArray(parsed.file?.messages) ? parsed.file.messages : null;
  if (!messages) {
    console.log(JSON.stringify([], null, 2));
    return;
  }
  const limit = Number.isFinite(Number(params.limit)) && Number(params.limit) > 0
    ? Number(params.limit)
    : undefined;
  const selected = typeof limit === "number" ? messages.slice(-limit) : messages;
  const result = selected.map((message) => ({
    id: message.id,
    senderInboxId: message.senderInboxId || "",
    content: message.rawText || message.content || "",
    sentAt: message.xmtpSentAtMs || message.receivedAtMs || null,
    deliveryStatus: message.deliveryStatus || "",
  }));
  console.log(JSON.stringify(result, null, 2));
}

function handleSessionsQuery(params) {
  const args = ["session", "query", "--json"];
  appendOptional(args, "--job-id", optionalString(params, "jobId"));
  appendOptional(args, "--my-agent-id", optionalString(params, "myAgentId"));
  appendOptional(args, "--to-agent-id", optionalString(params, "toAgentId"));
  const raw = run(OKX_A2A, args);
  const parsed = parseJsonOutput(raw, "okx-a2a session query");
  if (!Array.isArray(parsed.sessions)) {
    console.log(raw);
    return;
  }
  const sessionKeys = parsed.sessions
    .map((session) => readSessionKey(session))
    .filter((value) => typeof value === "string" && value.length > 0);
  console.log(JSON.stringify(sessionKeys, null, 2));
}

function handleSessionStatus() {
  const sessionKey = normalizeSessionKey(process.env.OKX_A2A_CURRENT_SESSION_KEY);
  if (!sessionKey) {
    fail("session_status requires current session context; OKX_A2A_CURRENT_SESSION_KEY is not set");
  }
  const parsed = parseModernSessionKey(sessionKey);
  const jobId = process.env.OKX_A2A_CURRENT_JOB_ID || parsed?.jobId || "";
  const agentId = process.env.OKX_A2A_CURRENT_AGENT_ID || parsed?.myAgentId || "";
  const data = {
    ok: true,
    source: "okx-a2a-legacy-tool-bridge",
    platform: "cli",
    sessionKey,
    scopeKey: sessionKey,
    sessionId: process.env.OKX_A2A_CURRENT_SESSION_ID || process.env.OKX_AGENT_TASK_AI_SESSION_ID || sessionKey,
    aiProvider: process.env.OKX_AGENT_TASK_AI_PROVIDER || "",
    messageId: process.env.OKX_A2A_CURRENT_MESSAGE_ID || "",
    jobId,
    agentId,
    myAgentId: parsed?.myAgentId || agentId,
    toAgentId: parsed?.toAgentId || "",
  };
  console.log(JSON.stringify(data, null, 2));
}

function deleteSessionKey(sessionKey) {
  const result = runResult(OKX_A2A, [
    "session", "delete",
    "--session-key", sessionKey,
    "--json",
  ]);
  if (result.error) {
    return { sessionKey, ok: false, error: result.error.message };
  }
  if (result.status !== 0) {
    const parsed = tryParseJson(result.stdout.trim());
    if (parsed?.ok === false && (parsed.deleted === false || parsed.error === "not_found")) {
      return { sessionKey, ok: false, notFound: true, error: formatCommandFailure(result) };
    }
    return { sessionKey, ok: false, error: formatCommandFailure(result) };
  }
  return { sessionKey, ok: true, stdout: result.stdout.trim() };
}

function handleDeleteConversation(params) {
  const jobId = optionalString(params, "jobId");
  if (!jobId) {
    const result = deleteSessionKey(required(params, "sessionKey"));
    if (!result.ok) {
      fail(result.error);
    }
    console.log(result.stdout);
    return;
  }

  const queryRaw = run(OKX_A2A, ["session", "query", "--job-id", jobId, "--json"]);
  const query = parseJsonOutput(queryRaw, "okx-a2a session query");
  const sessionKeys = Array.isArray(query.sessions)
    ? query.sessions.map((session) => readSessionKey(session)).filter(Boolean)
    : [];
  const matchedSessions = sessionKeys.length;
  const backupKey = `backup:${encodeURIComponent(jobId)}`;
  if (!sessionKeys.includes(backupKey)) {
    sessionKeys.push(backupKey);
  }

  const deleted = [];
  const failed = [];
  for (const sessionKey of sessionKeys) {
    const result = deleteSessionKey(sessionKey);
    if (result.ok) {
      deleted.push(sessionKey);
    } else if (!result.notFound) {
      failed.push({ sessionKey, error: result.error });
    }
  }

  const payload = {
    ok: failed.length === 0,
    jobId,
    matchedSessions,
    deleted,
    failed,
  };
  console.log(JSON.stringify(payload, null, 2));
  if (failed.length > 0) {
    process.exitCode = 1;
  }
}

function extractAgentListData(parsed) {
  return parsed?.data || parsed?.agentList || parsed || {};
}

function extractAgentWrappers(data) {
  if (Array.isArray(data?.list)) {
    return data.list;
  }
  if (Array.isArray(data?.agentList?.list)) {
    return data.agentList.list;
  }
  if (Array.isArray(data)) {
    return data;
  }
  return [];
}

function normalizeAgentRow(agent, wrapper = {}) {
  return {
    agentId: agent.agentId ?? agent.agent_id ?? "",
    name: agent.name ?? "",
    communicationAddress: agent.communicationAddress ?? agent.communication_address ?? agent.agentWalletAddress ?? "",
    role: agent.role ?? "",
    securityRate: agent.securityRate ?? agent.security_rate ?? agent.securityRating ?? "",
    status: agent.status ?? "",
    profileDescription: agent.profileDescription ?? agent.ProfileDescription ?? agent.description ?? "",
    profilePicture: agent.profilePicture ?? agent.picture ?? agent.image ?? "",
    ownerAddress: wrapper.ownerAddress ?? "",
    accountName: wrapper.accountName ?? "",
  };
}

function flattenAgentWrappers(wrappers) {
  const rows = [];
  for (const wrapper of wrappers) {
    const agents = Array.isArray(wrapper?.agentList) ? wrapper.agentList : [wrapper];
    for (const agent of agents) {
      if (agent && typeof agent === "object") {
        rows.push(normalizeAgentRow(agent, wrapper));
      }
    }
  }
  return rows;
}

function handleGetAgentList(params) {
  const agentIds = optionalString(params, "agentIds") || optionalString(params, "agentId");
  const requestedPage = optionalPositiveInt(params, "page");
  const pageSize = optionalPositiveInt(params, "pageSize") || optionalPositiveInt(params, "page_size") || 50;

  if (agentIds) {
    const raw = run(ONCHAINOS, ["agent", "get", "--agent-ids", agentIds]);
    const parsed = parseJsonOutput(raw, "onchainos agent get --agent-ids");
    const wrappers = extractAgentWrappers(extractAgentListData(parsed));
    console.log(JSON.stringify(flattenAgentWrappers(wrappers), null, 2));
    return;
  }

  const startPage = requestedPage || 1;
  const maxPages = requestedPage ? 1 : (optionalPositiveInt(params, "maxPages") || 100);
  const allWrappers = [];
  let total = null;
  let completed = false;

  for (let offset = 0; offset < maxPages; offset += 1) {
    const page = startPage + offset;
    const raw = run(ONCHAINOS, ["agent", "get", "--page", String(page), "--page-size", String(pageSize)]);
    const parsed = parseJsonOutput(raw, `onchainos agent get page ${page}`);
    const data = extractAgentListData(parsed);
    const wrappers = extractAgentWrappers(data);
    if (typeof data.total === "number") {
      total = data.total;
    }
    allWrappers.push(...wrappers);
    if (requestedPage || wrappers.length === 0 || (typeof total === "number" && allWrappers.length >= total)) {
      completed = true;
      break;
    }
  }
  if (!completed) {
    fail(`xmtp_get_agent_list stopped after ${maxPages} pages before confirming the full result. Pass params.maxPages to raise the safety cap.`);
  }

  console.log(JSON.stringify(flattenAgentWrappers(allWrappers), null, 2));
}

function handleGetSessionKey(params) {
  const jobId = required(params, "jobId");
  const groupId = required(params, "groupId");
  const myAgentId = optionalString(params, "myAgentId");
  const toAgentId = optionalString(params, "toAgentId");
  const myXmtpAddress = optionalString(params, "myXmtpAddress");
  const toXmtpAddress = optionalString(params, "toXmtpAddress");
  const args = [
    "session",
    "gen-key",
    "--job-id", jobId,
    "--group-id", groupId,
  ];
  appendOptional(args, "--my-agent-id", myAgentId);
  appendOptional(args, "--to-agent-id", toAgentId);
  appendOptional(args, "--my-xmtp-address", myXmtpAddress);
  appendOptional(args, "--to-xmtp-address", toXmtpAddress);
  console.log(run(OKX_A2A, args));
}

function handleGetPendingList() {
  const raw = run(OKX_A2A, ["task", "requests", "--json"]);
  const parsed = parseJsonOutput(raw, "okx-a2a task requests");
  console.log(JSON.stringify(parsed.payload || [], null, 2));
}

function handleDenyPendingConversation(params) {
  const args = [
    "task",
    "reject",
    "--group-id", required(params, "groupId"),
    "--json",
  ];
  appendOptional(args, "--agent-id", optionalString(params, "agentId"));
  const raw = run(OKX_A2A, args);
  const parsed = parseJsonOutput(raw, "okx-a2a task reject");
  const payload = parsed.payload || {};
  if (payload.denied) {
    console.log(`XMTP group marked as Denied: groupId=${payload.groupId || params.groupId}, ${payload.deniedClientCount || 0} clients`);
    return;
  }
  console.log(`No XMTP group found for groupId=${params.groupId}. Verify the groupId with xmtp_get_pending_list.`);
}

function handleRefreshAgents() {
  const raw = run(OKX_A2A, ["agent", "refresh", "--json"]);
  const parsed = parseJsonOutput(raw, "okx-a2a agent refresh");
  const payload = parsed.payload || {};
  const lines = [];
  if (Array.isArray(payload.added) && payload.added.length > 0) {
    lines.push(`Added: ${payload.added.join(", ")}`);
  }
  if (Array.isArray(payload.removed) && payload.removed.length > 0) {
    lines.push(`Removed: ${payload.removed.join(", ")}`);
  }
  if (lines.length === 0) {
    lines.push("No changes to agent list");
  }
  lines.push(`Active clients: ${payload.activeClients || 0}`);
  console.log(lines.join("\n"));
}

function handleFileUpload(params) {
  const filePath = required(params, "filePath");
  const agentId = required(params, "agentId");
  const jobId = required(params, "jobId");
  const args = [
    "file",
    "upload",
    "--file-path", filePath,
    "--agent-id", agentId,
    "--job-id", jobId,
  ];
  appendOptional(args, "--filename", optionalString(params, "filename"));
  appendOptional(args, "--mime-type", optionalString(params, "mimeType"));
  console.log(run(OKX_A2A, args));
}

function handleFileDownload(params) {
  const fileKey = required(params, "fileKey");
  const agentId = required(params, "agentId");
  const digest = required(params, "digest");
  const salt = required(params, "salt");
  const nonce = required(params, "nonce");
  const secret = required(params, "secret");
  const args = [
    "file",
    "download",
    "--file-key", fileKey,
    "--agent-id", agentId,
    "--digest", digest,
    "--salt", salt,
    "--nonce", nonce,
    "--secret", secret,
  ];
  appendOptional(args, "--filename", optionalString(params, "filename"));
  console.log(run(OKX_A2A, args));
}

const params = parseParams(rawParams);

async function main() {
  switch (toolName) {
    case "xmtp_runtime_env":
      console.log("cli");
      break;
    case "session_status":
      handleSessionStatus();
      break;
    case "xmtp_start_conversation":
      handleStartConversation(params);
      break;
    case "xmtp_send":
      handleSend(params);
      break;
    case "xmtp_prompt_user":
      handlePromptUser(params);
      break;
    case "xmtp_dispatch_user":
      handleDispatchUser(params);
      break;
    case "xmtp_dispatch_session":
      handleDispatchSession(params);
      break;
    case "xmtp_get_conversation_history":
      handleConversationHistory(params);
      break;
    case "xmtp_sessions_query":
      handleSessionsQuery(params);
      break;
    case "xmtp_delete_conversation":
      handleDeleteConversation(params);
      break;
    case "xmtp_start_evaluate_conversation":
      handleStartEvaluateConversation(params);
      break;
    case "xmtp_get_session_key":
      handleGetSessionKey(params);
      break;
    case "xmtp_get_agent_list":
      handleGetAgentList(params);
      break;
    case "xmtp_get_pending_list":
      handleGetPendingList();
      break;
    case "xmtp_deny_pending_conversation":
      handleDenyPendingConversation(params);
      break;
    case "xmtp_refresh_agents":
      handleRefreshAgents();
      break;
    case "xmtp_file_upload":
      await handleFileUpload(params);
      break;
    case "xmtp_file_download":
      await handleFileDownload(params);
      break;
    default:
      fail(`unknown legacy tool: ${toolName}`);
  }
}

main().catch((err) => fail(err instanceof Error ? err.message : String(err)));
