#!/usr/bin/env node
// ─── notify-openclaw ──────────────────────────────────────────────────────
// 往 openclaw agent session 里灌一条消息（模拟"系统通知"或任意外部注入）。
// 复用 openclaw 框架的 GatewayClient，走同一条 WS RPC。
//
// 用法：
//   node notify.js --session-key "<完整 sessionKey>" --message "[系统通知] provider_applied jobId=101 ..."
//   node notify.js --session-key "<key>" --message "..." --method sessions.send   # 默认，触发 AI
//   node notify.js --session-key "<key>" --message "..." --method chat.inject     # 静默注入，不触发 LLM
//
// sessionKey 从 openclaw UI 会话下拉里复制，例如：
//   agent:main:xmtp:group:okx-xmtp:my=0x...&to=0x...&job=101&gid=fef1...
//
// 两种 RPC 的区别：
//   sessions.send  → 塞一条"user 消息"+触发 AI 推理（模拟 agent 收到对端/系统消息）
//   chat.inject    → 塞一条"assistant 消息"+持久化到 transcript，不触发推理（纯记录）

"use strict";

const { randomUUID } = require("node:crypto");

// ── 解析参数 ────────────────────────────────────────────────────────────────
function parseArgs(argv) {
  const out = { method: "sessions.send" };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--session-key" || a === "-k") out.sessionKey = argv[++i];
    else if (a === "--message" || a === "-m") out.message = argv[++i];
    else if (a === "--method") out.method = argv[++i];
    else if (a === "--label") out.label = argv[++i];
    else if (a === "-h" || a === "--help") out.help = true;
  }
  return out;
}

const args = parseArgs(process.argv);
if (args.help || !args.sessionKey || !args.message) {
  console.error([
    "Usage:",
    "  node notify.js --session-key <key> --message <text> [--method sessions.send|chat.inject] [--label <label>]",
    "",
    "Examples:",
    "  node notify.js \\",
    "    --session-key 'agent:main:xmtp:group:okx-xmtp:my=0x...&to=0x...&job=101&gid=fef1...' \\",
    "    --message '[系统通知] provider_applied jobId=101'",
    "",
    "  node notify.js -k '<key>' -m '...' --method chat.inject",
  ].join("\n"));
  process.exit(args.help ? 0 : 1);
}

// ── 定位 openclaw 包 ────────────────────────────────────────────────────────
// openclaw CLI 通常安装在 homebrew 全局目录，Node 默认查不到全局 node_modules
// 需要显式按绝对路径 require。
function loadGatewayClient() {
  const candidates = [
    "/opt/homebrew/lib/node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js",
    "/usr/local/lib/node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js",
    // 兜底：如果你在 repo 根装了 openclaw 就能找到
    require("node:path").join(process.cwd(), "node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js"),
  ];
  for (const p of candidates) {
    try { return require(p).GatewayClient; } catch { /* try next */ }
  }
  console.error("✗ 找不到 openclaw GatewayClient。检查 openclaw 是否已安装（`openclaw --version`）。");
  console.error("  搜过的路径:");
  for (const p of candidates) console.error("    " + p);
  process.exit(1);
}

const GatewayClient = loadGatewayClient();

// ── 调用 ────────────────────────────────────────────────────────────────────
function callGateway({ method, params, timeoutMs = 10_000 }) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let ignoreClose = false;
    const stop = (err, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (err) reject(err); else resolve(value);
    };

    const client = new GatewayClient({
      instanceId: randomUUID(),
      // 照抄 plugin 用的 clientName —— gateway schema 对这个字段有严格约束
      clientName: "gateway-client",
      clientDisplayName: "notify-openclaw",
      mode: "backend",
      role: "operator",
      scopes: ["operator.admin"],
      minProtocol: 3,
      maxProtocol: 3,
      onHelloOk: async () => {
        try {
          const result = await client.request(method, params, { timeoutMs });
          ignoreClose = true;
          stop(undefined, result);
          client.stop();
        } catch (err) {
          ignoreClose = true;
          client.stop();
          stop(err);
        }
      },
      onClose: (code, reason) => {
        if (settled || ignoreClose) return;
        ignoreClose = true;
        stop(new Error(`gateway closed (${code}): ${reason || "no reason"}`));
      },
      onConnectError: (err) => {
        ignoreClose = true;
        try { client.stop(); } catch {}
        stop(err instanceof Error ? err : new Error(String(err)));
      },
    });

    const timer = setTimeout(() => {
      ignoreClose = true;
      try { client.stop(); } catch {}
      stop(new Error(`gateway timeout after ${timeoutMs}ms`));
    }, timeoutMs);

    client.start();
  });
}

// 两种 RPC 的 params 结构不一样
function buildParams({ method, sessionKey, message, label }) {
  if (method === "chat.inject") {
    return { sessionKey, message, ...(label ? { label } : {}) };
  }
  if (method === "sessions.send") {
    // sessions.send 需要 idempotencyKey + key（不是 sessionKey）
    return { key: sessionKey, message, idempotencyKey: randomUUID() };
  }
  throw new Error(`unsupported method: ${method}`);
}

// ── 主流程 ──────────────────────────────────────────────────────────────────
const params = buildParams(args);
console.log(`→ ${args.method}  sessionKey=${args.sessionKey.slice(0, 60)}…  message=${args.message.slice(0, 60)}…`);

callGateway({ method: args.method, params })
  .then((res) => {
    console.log("✓ ok:", JSON.stringify(res, null, 2));
  })
  .catch((err) => {
    console.error("✗ failed:", err.message || err);
    process.exit(1);
  });
