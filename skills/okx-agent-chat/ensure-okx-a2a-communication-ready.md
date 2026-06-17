# Ensure OKX A2A Communication Ready

**Mandatory communication-init flow** — ensures OKX A2A communication is ready for the current runtime. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

Runtime readiness is owned by the `okx-a2a` CLI:

- `okx-a2a setup --help` is the lightweight capability check. If the installed CLI does not support `setup`, install the latest stable `@okxweb3/a2a-node` package.
- `okx-a2a setup --json` owns runtime/plugin setup. It detects OpenClaw / Hermes / Node, installs missing OpenClaw or Hermes OKX A2A plugins from npm when needed, and ensures the local `@okxweb3/a2a-node` package is set up.
- `okx-a2a switch-runtime --json` detects the current caller runtime, switches the AI provider/runtime wiring, and returns the machine-readable runtime readiness result.
- `okx-a2a agent refresh --json` refreshes local agent communication identities. It is the CLI replacement for the legacy/native `xmtp_refresh_agents` tool.
- `okx-a2a update` is **not** part of this auto-triggered readiness flow. It is reserved for user-initiated manual package version updates.

This file owns the LLM-visible execution contract and JSON interpretation. Runtime/plugin detection must not be duplicated in markdown or shell snippets; it is handled inside `okx-a2a setup --json` and `okx-a2a switch-runtime --json`.

## When To Run (Auto-Trigger Contract)

The LLM **must** invoke this flow **on its own**, immediately after any of the following just completed successfully — even if the user did not explicitly ask to "sync" or "refresh":

| Upstream action | Where it typically happens |
|---|---|
| Agent registered / created | `okx-agent-identity` register flow |
| Agent metadata updated (name, avatar, endpoint, capabilities, etc.) | `okx-agent-identity` update flow |
| Agent deactivated / re-activated | `okx-agent-identity` deactivate / activate flow |
| Any other operation that mutates the local a2a agent list | — |

**Recognition cues** (Chinese / English) that should trigger this hook after the upstream action returns: `创建 agent`, `注册 agent`, `更新 agent`, `修改 agent 信息`, `注销 agent`, `停用 agent`, `agent 列表变更`, `agent registered`, `agent created`, `agent updated`, `agent deactivated`, `agent list changed`.

The flow is safe to invoke unconditionally. It first verifies Node.js is installed and satisfies the minimum supported version, then bootstraps the `okx-a2a` CLI if missing, verifies the installed CLI supports `setup`, uses `setup --json` for runtime/plugin setup, `switch-runtime --json` for runtime readiness, and `agent refresh --json` for agent communication identity refresh.

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

### Step 1: Bootstrap `okx-a2a` If Missing

Run from the installed skills root, or resolve commands normally from the current shell:

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` exists, continue to Step 2.

If `okx-a2a` is missing, bootstrap the Node CLI package:

```bash
npm install -g @okxweb3/a2a-node@latest
```

Then check again:

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` is still missing, stop and tell the user:

> `okx-a2a` was installed, but the global npm bin directory is not on `PATH`.

If `npm` is missing, stop and tell the user that npm is required to bootstrap OKX A2A communication.

### Step 2: Ensure `setup` Is Supported

Run:

```bash
okx-a2a setup --help >/dev/null 2>&1
```

If this succeeds, continue to Step 3.

If this fails, the installed `@okxweb3/a2a-node` is too old for this flow. Install the latest stable package:

```bash
npm install -g @okxweb3/a2a-node@latest
```

Then re-check:

```bash
okx-a2a setup --help >/dev/null 2>&1
```

If `setup` is still unsupported, stop and tell the user:

> `okx-a2a` is installed, but it does not support `setup`. Please check the global npm installation and PATH.

Do **not** run `okx-a2a update` from this auto-triggered flow. `setup` replaces it for runtime/plugin detection and installation. `update` is reserved for user-initiated manual package version updates.

### Step 3: Setup Runtime And Plugins

Run:

```bash
okx-a2a setup --json
```

This command owns runtime/plugin detection and setup:

- For OpenClaw, it detects whether the OKX A2A OpenClaw plugin is installed and configured; if missing, it installs the plugin from npm, applies required runtime config, and may restart the OpenClaw gateway once.
- For Hermes, it detects whether the OKX A2A Hermes plugin is installed; if missing, it pulls the plugin package from npm, installs it into the Hermes user plugins directory, and may restart the Hermes gateway once.
- For Node, it ensures the local `@okxweb3/a2a-node` setup is present.

Stdout must be JSON. Do not pipe, grep, truncate, or rewrite the command.

Use the `setup --json` output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` | Runtime/plugin setup is ready. Surface `userMessage` only if it is user-relevant, then continue to Step 4. |
| `state: "needs_user_action"` or `state: "blocked"` | Show `userMessage` and stop. The environment needs user/admin action. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

If `setup --json` fails because the first-time OpenClaw or Hermes gateway restart failed, treat the setup as failed even if the plugin installation itself completed. Surface the exact error/output to the user and stop. Do not run `okx-a2a switch-runtime --json` or `okx-a2a agent refresh --json` after a gateway restart failure.

If `setup --json` exits non-zero or prints invalid JSON, show the command error/output and stop. The AI should handle the setup failure at this point by reporting the failure and any CLI-provided next action; it must not continue to later readiness steps.

### Step 4: Switch Runtime

Run:

```bash
okx-a2a switch-runtime --json
```

This command owns runtime detection and provider/runtime switching.

Stdout must be JSON. Do not pipe, grep, truncate, or rewrite the command.

### Step 5: Interpret `switch-runtime --json`

Use the `switch-runtime --json` output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` | Runtime/provider wiring is ready. Surface `userMessage` only if it is user-relevant, then continue to Step 6. |
| `state: "needs_user_choice"` | Ask the user to choose one value from `providers`. After they choose, rerun the command indicated by `nextCommand`, or rerun `okx-a2a switch-runtime --json` with the supported provider selection option if the CLI prints one. |
| `state: "blocked"` | Show `userMessage` and stop. The environment needs user/admin action. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

If `switch-runtime --json` exits non-zero or prints invalid JSON, show the command error/output and stop.

### Step 6: Refresh Agent Communication Identities

Run:

```bash
okx-a2a agent refresh --json
```

This command is the CLI replacement for legacy/native `xmtp_refresh_agents`.

Stdout must be JSON. Do not pipe, grep, truncate, or rewrite the command. If it exits non-zero or prints invalid JSON, show the command error/output and stop.

Use the refresh output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` | Communication is ready. Surface `userMessage` only if it is user-relevant, then continue the upstream flow. |
| `ok: false` or `state: "blocked"` | Show `userMessage` and stop. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

## JSON Contract

Example setup success:

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

Example refresh success:

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
| `okx-a2a` is missing | Bootstrap with `npm install -g @okxweb3/a2a-node@latest`, then re-check PATH. |
| `npm` is missing | Stop and tell the user npm is required. |
| `okx-a2a` exists but `setup` is unsupported | Install latest version `@okxweb3/a2a-node`, then re-check `okx-a2a setup --help`. |
| `setup` remains unsupported after install | Tell the user the global npm install or PATH is inconsistent and stop. |
| `okx-a2a setup --json` installs a missing OpenClaw/Hermes plugin | Continue to `okx-a2a switch-runtime --json` after setup returns `ok: true`. |
| `okx-a2a setup --json` is blocked or fails | Surface the setup output and stop. |
| `okx-a2a switch-runtime --json` succeeds | Continue to `okx-a2a agent refresh --json`; communication readiness is decided by refresh. |
| `okx-a2a agent refresh --json` fails | Surface the error and stop. |
| Current runtime is OpenClaw / Hermes Gateway | Do not run manual restart/install commands from markdown. `okx-a2a setup --json` owns plugin setup behavior. |
| Runtime signals conflict | Do not resolve in markdown. `okx-a2a setup --json` and `okx-a2a switch-runtime --json` own runtime detection. |
| Legacy `ensure-okx-a2a-ready.sh` exists in this skill directory | It is a compatibility wrapper only; this markdown flow calls `okx-a2a` directly. |
