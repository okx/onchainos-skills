/**
 * reset.ts — 重置 openclaw sessions，清除所有对话历史和 ws-mock 子会话
 *
 * npm run reset        → 清理 sessions.json + SQLite 子会话（无需重启 gateway）
 * npm run reset:gw     → 同上 + 重启 gateway（等待 ws-channel 就绪）
 * node dist/reset.js   → 同上
 */
import fs   from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import os   from "node:os";

const HOME         = os.homedir();
const SESSIONS_JSON = path.join(HOME, ".openclaw/agents/main/sessions/sessions.json");
const FLOWS_DB     = path.join(HOME, ".openclaw/flows/registry.sqlite");
const GATEWAY_LOG  = path.join(HOME, ".openclaw/logs/gateway.log");
const GATEWAY_SVC  = "ai.openclaw.gateway";

// ── 项目根目录（reset.ts 在 tools/ws-mock-ts/src/，往上三级） ────────────────
const REPO_ROOT = path.resolve(import.meta.dirname ?? path.dirname(new URL(import.meta.url).pathname), "../../..");

/** Sync skill files and plugin files to their deployed locations. */
function syncAssets(): void {
  const synced: string[] = [];
  const failed: string[] = [];

  const copy = (src: string, dst: string) => {
    try {
      fs.writeFileSync(dst, fs.readFileSync(src));
      synced.push(path.basename(src));
    } catch (e) {
      failed.push(`${path.basename(src)}: ${e}`);
    }
  };

  // skills → ~/.agents/skills/okx-agent-task/
  const skillSrc = path.join(REPO_ROOT, "skills/okx-agent-task");
  const skillDst = path.join(HOME, ".agents/skills/okx-agent-task");
  for (const f of ["SKILL.md", "client.md", "provider.md", "evaluator.md"]) {
    copy(path.join(skillSrc, f), path.join(skillDst, f));
  }

  // plugin → ~/openclaw-plugins/ws-channel/src/
  const pluginSrc = path.join(REPO_ROOT, "plugins/ws-channel/src");
  const pluginDst = path.join(HOME, "openclaw-plugins/ws-channel/src");
  for (const f of ["index.ts", "handler.ts", "ws-client.ts", "runtime.ts"]) {
    copy(path.join(pluginSrc, f), path.join(pluginDst, f));
  }

  if (synced.length) console.log(`✓ assets synced (${synced.join(", ")})`);
  if (failed.length) console.log(`⚠ sync failed:\n  ${failed.join("\n  ")}`);
}

// ── helpers ───────────────────────────────────────────────────────────────────

/** Clear main session history by writing empty object. */
function clearSessions(): boolean {
  try {
    fs.writeFileSync(SESSIONS_JSON, "{}");
    return true;
  } catch { return false; }
}

/** Delete ws-mock sub-session rows from flows/registry.sqlite. */
function clearFlowSessions(): number {
  try {
    const out = execSync(
      `sqlite3 "${FLOWS_DB}" "DELETE FROM flow_runs WHERE owner_key LIKE '%ws-mock:direct%'; SELECT changes();"`,
      { stdio: "pipe" }
    ).toString().trim();
    return parseInt(out, 10) || 0;
  } catch { return -1; }
}

function currentLogSize(): number {
  try { return fs.statSync(GATEWAY_LOG).size; } catch { return 0; }
}

/** Poll gateway log for ws-channel registration line appearing after `offset` bytes. */
function waitForGateway(offset: number, timeoutMs = 20_000): "ready" | "timeout" {
  const deadline = Date.now() + timeoutMs;
  let dots = 0;

  while (Date.now() < deadline) {
    try {
      // 用 Buffer 读取以确保 offset 按字节切片（日志含中文，字符数 ≠ 字节数）
      const buf = fs.readFileSync(GATEWAY_LOG);
      const newContent = buf.subarray(offset).toString("utf8");
      if (/ws-channel.*已注册/i.test(newContent)) return "ready";
    } catch { /* log not yet written */ }

    if (dots % 2 === 0) process.stdout.write(".");
    dots++;

    const until = Date.now() + 500;
    while (Date.now() < until) { /* spin */ }
  }
  return "timeout";
}

// ── main ──────────────────────────────────────────────────────────────────────

const withGateway = process.argv.includes("--gateway") || process.argv.includes("-g");

console.log("────────────────────────────────────────");
console.log("  openclaw reset");
console.log("────────────────────────────────────────");

// Step 1: sync skill + plugin assets
syncAssets();

// Step 2: clear session history
const ok = clearSessions();
console.log(ok ? "✓ sessions cleared" : "⚠ sessions: file not found (ok if first run)");

if (!withGateway) {
  // SQLite is locked by running gateway; skip sub-session cleanup
  console.log("  (pass --gateway / -g to also clear sub-sessions and restart gateway)");
  console.log("────────────────────────────────────────");
  process.exit(0);
}

// Step 2 (gateway mode): stop gateway so SQLite is released
try {
  const uid = execSync("id -u").toString().trim();
  execSync(`launchctl stop gui/${uid}/${GATEWAY_SVC}`, { stdio: "pipe" });
  // Wait for process to fully exit and release db locks
  const until = Date.now() + 1500;
  while (Date.now() < until) { /* spin */ }
} catch { /* already stopped */ }

// Step 3: clear ws-mock sub-session rows (gateway stopped, db now writable)
const m = clearFlowSessions();
if (m >= 0) console.log(`✓ flow sub-sessions cleared (${m} row${m !== 1 ? "s" : ""})`);
else        console.log(`⚠ flow sub-sessions: sqlite3 failed`);

// Step 4: restart gateway
const logOffset = currentLogSize();
process.stdout.write("  gateway restarting");
try {
  const uid = execSync("id -u").toString().trim();
  execSync(`launchctl kickstart -k gui/${uid}/${GATEWAY_SVC}`, { stdio: "pipe" });
} catch (e) {
  console.log(`\n✗ launchctl failed: ${e}`);
  process.exit(1);
}

// Step 4: wait for ws-channel plugin to register
const result = waitForGateway(logOffset);
if (result === "ready") {
  console.log(" ✓ ready");
} else {
  console.log(" ⚠ timed out (20s)");
  console.log(`  check: tail -f ${GATEWAY_LOG}`);
}

console.log("────────────────────────────────────────");
