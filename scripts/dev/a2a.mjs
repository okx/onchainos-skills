import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

export function localA2aPath(repoRoot) {
  return path.join(repoRoot, ".codex", "a2a", "node_modules", ".bin", "okx-a2a");
}

export function assertLocalA2a(repoRoot) {
  const executable = localA2aPath(repoRoot);
  const packageDir = path.join(fs.realpathSync(repoRoot), ".codex", "a2a", "node_modules", "@okxweb3", "a2a-node");
  if (!fs.existsSync(executable)
      || !fs.realpathSync(executable).startsWith(`${packageDir}${path.sep}`)) {
    throw new Error("project-local okx-a2a is missing or points outside this project; run npm run setup");
  }
  fs.accessSync(executable, fs.constants.X_OK);
  return executable;
}

export function installLocalA2a(repoRoot) {
  const installDir = path.join(repoRoot, ".codex", "a2a");
  fs.mkdirSync(installDir, { recursive: true, mode: 0o700 });
  // Refresh existing installs too. A2A's doctor --fix only upgrades the global package.
  const result = spawnSync("npm", [
    "install", "--global=false", "--prefix", installDir,
    "--no-save", "--package-lock=false", "@okxweb3/a2a-node@latest",
  ], { cwd: repoRoot, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`local A2A install failed (exit ${result.status})`);
  return assertLocalA2a(repoRoot);
}

export function prepareLocalA2a(repoRoot) {
  const executable = assertLocalA2a(repoRoot);
  const wrapper = path.join(repoRoot, ".codex", "bin", "okx-a2a");
  const realLink = path.join(repoRoot, ".codex", "bin", "okx-a2a.real");
  if (!fs.existsSync(wrapper)
      || fs.realpathSync(wrapper) !== fs.realpathSync(path.join(repoRoot, "scripts", "dev", "okx-a2a.sh"))
      || !fs.existsSync(realLink) || fs.realpathSync(realLink) !== fs.realpathSync(executable)) {
    throw new Error("A2A wrapper does not point to the project package; run npm run setup");
  }
  // This development environment configures Codex. An explicit provider avoids
  // an interactive prompt and replaces a stale provider binding on restart.
  // Do not use doctor --fix: it can install global packages and OS services.
  const restart = spawnSync(wrapper, ["daemon", "restart", "--provider", "codex", "--no-autostart"], {
    cwd: repoRoot, stdio: ["ignore", "inherit", "inherit"],
  });
  if (restart.error) throw restart.error;
  if (restart.status !== 0) throw new Error(`project A2A restart failed (exit ${restart.status})`);

  // The first doctor refreshes identities and waits for XMTP. Its daemon check
  // runs before that wait, so repeat once if it captured the warm-up state.
  let report;
  for (let attempt = 0; attempt < 2; attempt++) {
    const check = spawnSync(wrapper, ["doctor", "--non-interactive", "--json"], {
      cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"],
    });
    if (check.error) throw check.error;
    try { report = JSON.parse(check.stdout); }
    catch { throw new Error(`A2A doctor returned invalid JSON (exit ${check.status})`); }
    if (check.status !== 0 || report.ready !== true) {
      console.error(check.stdout);
      throw new Error("project A2A is not ready; resolve the reported issues and rerun npm run setup && npm run build");
    }
    if (report.checks?.some(item => item.id === "daemon_running" && item.status === "pass")) break;
    if (attempt === 1) {
      console.error(check.stdout);
      throw new Error("project A2A daemon has not finished starting; inspect its log and rerun npm run build");
    }
  }
  console.log(`A2A ready: ${executable} (version ${report.cliVersion})`);
  for (const item of report.checks ?? []) {
    console.log(`  ${item.id}: ${item.status} — ${item.detail}`);
  }
  return report;
}
