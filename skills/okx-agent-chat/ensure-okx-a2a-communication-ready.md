# Ensure OKX A2A Communication Ready

**Mandatory communication-init flow** — ensures OKX A2A communication is ready for the current runtime. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

Runtime readiness is owned by the `okx-a2a` CLI:

- The latest stable `@okxweb3/a2a-node` package must be installed before readiness checks, unless the current installed package is a `beta` build. Beta builds are intentionally preserved and must not be replaced with `@latest`.
- `okx-a2a daemon start` ensures the local A2A daemon is running. If the daemon is already running and the `@okxweb3/a2a-node` package version did not change, do not restart it.
- `okx-a2a switch-runtime --json` detects the current caller runtime, switches the AI provider/runtime wiring, and returns the machine-readable runtime readiness result.
- `okx-a2a agent refresh --json` queues or performs local agent communication identity refresh, depending on the installed CLI behavior. It is the CLI replacement for the legacy/native `xmtp_refresh_agents` tool.
- `okx-a2a setup --json` owns final runtime/plugin setup and verification after daemon readiness, runtime switch, and the agent refresh command.
- `okx-a2a update` is **not** part of this auto-triggered readiness flow. Package updates are performed with `npm install -g @okxweb3/a2a-node@latest`, except for beta builds.

This file owns the LLM-visible execution contract and JSON interpretation. Runtime/plugin detection must not be duplicated in markdown or shell snippets; it is handled inside `okx-a2a switch-runtime --json` and `okx-a2a setup --json`.

## When To Run (Auto-Trigger Contract)

The LLM **must** invoke this flow **on its own**, immediately after any of the following just completed successfully — even if the user did not explicitly ask to "sync" or "refresh":

| Upstream action | Where it typically happens |
|---|---|
| Agent registered / created | `okx-agent-identity` register flow |
| Agent metadata updated (name, avatar, endpoint, capabilities, etc.) | `okx-agent-identity` update flow |
| Agent deactivated / re-activated | `okx-agent-identity` deactivate / activate flow |
| Any other operation that mutates the local a2a agent list | — |

**Recognition cues** (Chinese / English) that should trigger this hook after the upstream action returns: `创建 agent`, `注册 agent`, `更新 agent`, `修改 agent 信息`, `注销 agent`, `停用 agent`, `agent 列表变更`, `agent registered`, `agent created`, `agent updated`, `agent deactivated`, `agent list changed`.

The flow is safe to invoke unconditionally. It first verifies Node.js is installed and satisfies the minimum supported version, then ensures the `okx-a2a` CLI is installed at the latest stable version unless the current package is beta. It then ensures the daemon is running: start it if it is not running, restart it if the package was newly installed or actually updated while the daemon was already running, and otherwise leave a running daemon untouched. It then runs `switch-runtime --json`, runs `agent refresh --json`, and finally runs `okx-a2a setup --json`.

## Execution Flow

### Step 0: Check Node.js Version

Run:

```bash
node --version
```

Requirement:

- Node.js `>= 22.14.0`

If `node` is missing, stop and tell the user Node.js and npm are required to bootstrap OKX A2A communication.

If the installed Node.js version is below `22.14.0`, stop and tell the user:

> Node.js must be upgraded to `>= 22.14.0` before OKX A2A communication can be prepared.

Do not proceed to any later step when Node.js is missing or below the minimum version.

### Step 1: Ensure Latest Stable `okx-a2a` CLI Unless Beta

Run from the installed skills root, or resolve commands normally from the current shell:

```bash
npm --version
```

If `npm` is missing, stop and tell the user that npm is required to bootstrap or update OKX A2A communication.

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` is missing, install the Node CLI package:

```bash
npm install -g @okxweb3/a2a-node@latest
```

Treat this as `packageChanged: true`.

Then check again:

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` is still missing, stop and tell the user:

> `okx-a2a` was installed, but the global npm bin directory is not on `PATH`.

If `okx-a2a` already exists, capture the installed package version before any install attempt. Prefer npm's structured package metadata:

```bash
npm list -g @okxweb3/a2a-node --depth=0 --json
```

Read `dependencies["@okxweb3/a2a-node"].version` as `beforeVersion`.

If npm cannot return the package metadata but `okx-a2a` is on `PATH`, fall back to:

```bash
okx-a2a --version
```

Read the exact version string printed by the CLI as `beforeVersion`. Do not use npm install logs, human-readable summaries, or warning text to decide whether the package changed.

If `beforeVersion` contains `beta`, continue to Step 2 without updating.

Treat this as `packageChanged: false`.

If the installed package is not beta, install the latest stable package:

```bash
npm install -g @okxweb3/a2a-node@latest
```

After install completes, capture the installed package version again using the same source used for `beforeVersion`:

```bash
npm list -g @okxweb3/a2a-node --depth=0 --json
```

or, only if npm metadata was unavailable before:

```bash
okx-a2a --version
```

Read this as `afterVersion`.

Compare only the normalized version strings:

- If `afterVersion` is different from `beforeVersion`, treat this as `packageChanged: true`.
- If `afterVersion` is the same as `beforeVersion`, treat this as `packageChanged: false`.

If `afterVersion` cannot be determined, do not guess from npm output. Stop and report that the installed package version could not be verified.

Then check again:

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` is still missing, stop and tell the user:

> `okx-a2a` was installed, but the global npm bin directory is not on `PATH`.

Do **not** run `okx-a2a update` from this auto-triggered flow.

### Step 2: Ensure `okx-a2a` Daemon Is Running

First check daemon status:

```bash
okx-a2a daemon status
```

Use the status result and Step 1's `packageChanged` value:

- If the daemon is not running, run `okx-a2a daemon start` regardless of `packageChanged`.
- If the daemon is already running and `packageChanged: false`, do **not** restart it. Continue directly to Step 3.
- If the daemon is already running and `packageChanged: true`, run `okx-a2a daemon restart` so the running daemon uses the updated package version.

Run when startup is needed:

```bash
okx-a2a daemon start
```

Run when restart is needed:

```bash
okx-a2a daemon restart
```

If the command exits successfully, continue to Step 3.

If the daemon is already running and the CLI treats that as success or a harmless already-running message, continue to Step 3.

If daemon startup or restart fails, show the command error/output and stop. Do not run `switch-runtime`, `agent refresh`, or `okx-a2a setup` after daemon startup/restart failure.

### Step 3: Switch Runtime

Run:

```bash
okx-a2a switch-runtime --json
```

This command owns runtime detection and provider/runtime switching.

Stdout must be JSON. Do not pipe, grep, truncate, or rewrite the command.

Use the `switch-runtime --json` output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` | Runtime/provider wiring is ready. Surface `userMessage` only if it is user-relevant, then continue to Step 4. |
| `state: "needs_user_choice"` | Ask the user to choose one value from `providers`. After they choose, rerun the command indicated by `nextCommand`, or rerun `okx-a2a switch-runtime --json` with the supported provider selection option if the CLI prints one. |
| `state: "blocked"` | Show `userMessage` and stop. The environment needs user/admin action. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

If `switch-runtime --json` exits non-zero or prints invalid JSON, show the command error/output and stop.

### Step 4: Refresh Agent Communication Identities

Run:

```bash
okx-a2a agent refresh --json
```

This command is the CLI replacement for legacy/native `xmtp_refresh_agents`.

Stdout must be JSON. Do not pipe, grep, truncate, or rewrite the command. If it exits non-zero or prints invalid JSON, show the command error/output and stop.

Use the refresh output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` and `payload.queued: true` | Agent communication refresh was accepted by the daemon queue. Continue to Step 5. |
| `ok: true` with a completed refresh payload | Agent communication identities are refreshed. Surface `userMessage` only if it is user-relevant, then continue to Step 5. |
| `ok: false` or `state: "blocked"` | Show `userMessage` and stop. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

Current `okx-a2a` builds may return a queued-command JSON for `agent refresh --json` instead of waiting for the daemon to finish the refresh. This flow treats queue acceptance as the readiness handoff. If a future caller needs confirmed refreshed identities or active client counts before continuing, the CLI must provide a wait/status command and this document must be updated to use it.

### Step 5: Run Final A2A Setup

Run:

```bash
okx-a2a setup --json
```

This command owns final runtime/plugin setup and verification.

Stdout must be JSON. Do not pipe, grep, truncate, or rewrite the command.

Use the `okx-a2a setup --json` output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` | Communication is ready. Surface `userMessage` only if it is user-relevant, then continue the upstream flow. |
| `state: "needs_user_action"` or `state: "blocked"` | Show `userMessage` and stop. The environment needs user/admin action. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

If `okx-a2a setup --json` fails because the first-time OpenClaw or Hermes gateway restart failed, treat the setup as failed even if the plugin installation itself completed. Surface the exact error/output to the user and stop.

If `okx-a2a setup --json` exits non-zero or prints invalid JSON, show the command error/output and stop. The AI should handle the setup failure at this point by reporting the failure and any CLI-provided next action.

## JSON Contract

Example final setup success:

```json
{
  "ok": true,
  "runtime": "openclaw",
  "state": "ready",
  "action": "setup_verified",
  "installed": [],
  "userMessage": "OKX A2A runtime setup is ready."
}
```

Example switch-runtime success:

```json
{
  "ok": true,
  "runtime": "node",
  "state": "ready",
  "action": "switched",
  "reason": "",
  "userMessage": "OKX A2A runtime is ready."
}
```

Example refresh queued:

```json
{
  "id": "cmd_123",
  "ok": true,
  "completedAt": 1760000000000,
  "payload": {
    "queued": true
  }
}
```

Example completed refresh success:

```json
{
  "ok": true,
  "payload": {
    "added": [],
    "removed": [],
    "activeClients": 2
  },
  "userMessage": "OKX A2A communication is ready."
}
```

Example blocked result:

```json
{
  "ok": false,
  "runtime": "node",
  "state": "blocked",
  "action": "none",
  "reason": "okx_a2a_not_on_path",
  "userMessage": "okx-a2a was installed, but the global npm bin directory is not on PATH."
}
```

## Edge Cases

| Scenario | Behavior |
|---|---|
| `node` is missing | Stop and tell the user Node.js and npm are required. |
| Node.js version is below `22.14.0` | Stop and tell the user Node.js must be upgraded to `>= 22.14.0`. Do not proceed. |
| `okx-a2a` is missing | Install with `npm install -g @okxweb3/a2a-node@latest`, then re-check PATH. |
| `npm` is missing | Stop and tell the user npm is required. |
| Installed `okx-a2a` package is beta and daemon is running | Do not update it and do not restart the daemon. Continue to `switch-runtime --json`. |
| Installed `okx-a2a` package is beta and daemon is not running | Do not update it. Run `okx-a2a daemon start`, then continue to `switch-runtime --json`. |
| Installed package version cannot be determined before/after install | Do not infer from npm logs. Stop and report that `@okxweb3/a2a-node` version verification failed. |
| Installed `okx-a2a` package is not beta, already latest, and daemon is running | Do not restart the daemon. Continue to `switch-runtime --json`. |
| Installed `okx-a2a` package is not beta, already latest, and daemon is not running | Run `okx-a2a daemon start`, then continue to `switch-runtime --json`. |
| Installed `okx-a2a` package is not beta and updates to a different version | Re-check PATH, then ensure the daemon is running on the updated package. If it was already running, run `okx-a2a daemon restart`. |
| `okx-a2a daemon start` or `okx-a2a daemon restart` fails after a package install/update | Surface the daemon output and stop. |
| `okx-a2a switch-runtime --json` succeeds | Continue to `okx-a2a agent refresh --json`. |
| `okx-a2a agent refresh --json` fails | Surface the error and stop. |
| `okx-a2a setup --json` installs a missing OpenClaw/Hermes plugin | Communication is ready only after `okx-a2a setup --json` returns `ok: true`. |
| `okx-a2a setup --json` is blocked or fails | Surface the setup output and stop. |
| Current runtime is OpenClaw / Hermes Gateway | Do not run manual restart/install commands from markdown. `okx-a2a setup --json` owns plugin setup behavior. |
| Runtime signals conflict | Do not resolve in markdown. `okx-a2a switch-runtime --json` and `okx-a2a setup --json` own runtime detection. |
| Legacy `ensure-okx-a2a-ready.sh` exists in this skill directory | It is a compatibility wrapper only; this markdown flow calls `okx-a2a` directly. |
