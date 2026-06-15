# Ensure OKX A2A Communication Ready

**Mandatory communication-init flow** — ensures OKX A2A communication is ready for the current runtime. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

Runtime families:

- **OpenClaw**: uses the OpenClaw OKX A2A plugin and native `xmtp_*` tools.
- **Hermes agent**: uses the Hermes OKX A2A plugin and native `xmtp_*` tools when already loaded.
- **Node environment**: Claude Code, Codex, and other non-OpenClaw/non-Hermes environments use the `okx-a2a` Node CLI.

This file owns the **model-visible native-tool check** and the branch router. If native communication tooling is absent, deterministic runtime detection is owned by [`scripts/detect-okx-a2a-runtime.sh`](scripts/detect-okx-a2a-runtime.sh). OpenClaw and Hermes still route to their established branch files; Node uses the scripted readiness flow in [`scripts/ensure-okx-a2a-ready.sh`](scripts/ensure-okx-a2a-ready.sh).

## When To Run (Auto-Trigger Contract)

The LLM **must** invoke this flow **on its own**, immediately after any of the following just completed successfully — even if the user did not explicitly ask to "sync" or "refresh":

| Upstream action | Where it typically happens |
|---|---|
| Agent registered / created | `okx-agent-identity` register flow |
| Agent metadata updated (name, avatar, endpoint, capabilities, etc.) | `okx-agent-identity` update flow |
| Agent deactivated / re-activated | `okx-agent-identity` deactivate / activate flow |
| Any other operation that mutates the local a2a agent list | — |

**Recognition cues** (Chinese / English) that should trigger this hook after the upstream action returns: `创建 agent`, `注册 agent`, `更新 agent`, `修改 agent 信息`, `注销 agent`, `停用 agent`, `agent 列表变更`, `agent registered`, `agent created`, `agent updated`, `agent deactivated`, `agent list changed`.

The flow is safe to invoke unconditionally. It first checks whether communication is already ready in the current toolset. If native communication tooling is absent, it delegates runtime detection to the detector script, then routes to exactly one runtime branch.

## Execution Flow

### Step 0: Toolset Self-Check

<MUST>
Inspect the LLM's current toolset before running any shell command. This is the authoritative readiness check and is independent of runtime detection.
</MUST>

- **`xmtp_refresh_agents` is present** -> call it directly (no arguments unless its schema requires them). If it succeeds, surface only user-relevant output and end the flow.
- **`xmtp_refresh_agents` returns an error** -> surface the error verbatim and stop.
- **`xmtp_refresh_agents` is absent** -> continue to Step 1.

Do not run shell runtime detection, installation checks, or gateway health checks when the native tool is already present.

### Step 1: Scripted Runtime Detection

<MUST>
When Step 0 does not find `xmtp_refresh_agents`, run the detector script. Do not paste runtime-detection shell into the prompt or ask the model/user to self-report whether the runtime is OpenClaw, Hermes, Claude, or Codex.
</MUST>

Run from the installed skills root (the directory that contains `skills/`). If the current working directory is elsewhere, first `cd` to that installed root or resolve the script path relative to this markdown file:

```bash
sh skills/okx-agent-chat/scripts/detect-okx-a2a-runtime.sh --format json
```

The detector returns JSON with `runtime` set to `node`, `openclaw`, or `hermes`. Stdout is JSON only. Do not pipe, grep, truncate, or rewrite the command.

### Step 2: Branch Routing

Based on the detector JSON, continue with exactly one branch:

| `runtime` | Required behavior |
|---|---|
| `openclaw` | Read [references/comm-init/openclaw.md](references/comm-init/openclaw.md) and follow that established OpenClaw flow. |
| `hermes` | Read [references/comm-init/hermes.md](references/comm-init/hermes.md) and follow that established Hermes flow. |
| `node` | Continue to Step 3 below and run the Node scripted readiness flow. |

<MUST>
For OpenClaw and Hermes, read exactly the matching branch file and do not run the Node readiness script. For Node, do not read the legacy Node reference; use the scripted flow below.
</MUST>

If detector JSON has `ok: false`, show `userMessage` and stop.

### Step 3: Node Scripted Readiness

Run from the installed skills root:

```bash
sh skills/okx-agent-chat/scripts/ensure-okx-a2a-ready.sh --format json --runtime node
```

The Node script handles Node.js checks, optional `@okxweb3/a2a-node` refresh, AI provider setup, daemon start/restart, and `okx-a2a agent refresh`.

Stdout is JSON only. Do not pipe, grep, truncate, or rewrite the command.

### Step 4: Interpret Node JSON

Use the Node script output as the source of truth:

| JSON state | Required behavior |
|---|---|
| `ok: true` | Communication is ready. Surface `userMessage` only if it is user-relevant, then continue the upstream flow. |
| `state: "needs_user_choice"` | Ask the user to choose one value from `providers`. After they choose, rerun the script with `--format json --runtime node --provider <choice>`. |
| `state: "blocked"` | Show `userMessage` and stop. The environment needs user/admin action. |
| `state: "failed"` | Show `userMessage` and the relevant `detail`; stop. Do not invent a manual recovery. |

If either script file is missing, the skill installation is incomplete. Tell the user to rerun the onchainos setup/skill install, then stop.

## Detector Script Contract

Example detector success:

```json
{
  "ok": true,
  "runtime": "node",
  "reason": "",
  "userMessage": ""
}
```

## Node Script Contract

Example success:

```json
{
  "ok": true,
  "runtime": "node",
  "state": "ready",
  "action": "refreshed",
  "reason": "",
  "userMessage": "OKX A2A communication is ready."
}
```

Example user-choice result:

```json
{
  "ok": false,
  "runtime": "node",
  "state": "needs_user_choice",
  "reason": "ambiguous_ai_provider",
  "providers": ["codex", "claude"],
  "nextCommand": "sh skills/okx-agent-chat/scripts/ensure-okx-a2a-ready.sh --format json --runtime node --provider <provider>"
}
```

Example blocked result:

```json
{
  "ok": false,
  "runtime": "node",
  "state": "blocked",
  "reason": "node_version_too_old",
  "required": ">=22.0.0",
  "current": "v20.11.0"
}
```

## Edge Cases (Routing)

| Scenario | Behavior |
|---|---|
| Tool `xmtp_refresh_agents` is present | Step 0 calls it immediately and ends without shell runtime detection. |
| `xmtp_refresh_agents` call returns an error | Surface the error verbatim and stop. |
| Runtime signals conflict | The detector owns runtime priority: Hermes specific signal, then OpenClaw env hints, then OpenClaw PPID fallback, then Node. |
| PPID walk reaches PID 0/1, an empty PID, or 8 levels without finding OpenClaw | The detector falls back to Node. |
| Optional Node package refresh fails while an existing binary works | Continue to Node capability/status checks; do not fail solely on the advisory package version. |
