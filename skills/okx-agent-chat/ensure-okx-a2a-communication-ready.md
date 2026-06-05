# Ensure OKX A2A Communication Ready

**Mandatory communication-init flow** — ensures OKX A2A communication is ready for the current runtime. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

Runtime families:

- **OpenClaw**: uses the OpenClaw OKX A2A plugin and native `xmtp_*` tools.
- **Hermes agent**: reserved; do not run setup until the Hermes flow is defined.
- **Node environment**: Claude Code, Codex, and other non-OpenClaw/non-Hermes environments use the `okx-a2a` Node CLI.

## When To Run (Auto-Trigger Contract)

The LLM **must** invoke this flow **on its own**, immediately after any of the following just completed successfully — even if the user did not explicitly ask to "sync" or "refresh":

| Upstream action | Where it typically happens |
|---|---|
| Agent registered / created | `okx-agent-identity` register flow |
| Agent metadata updated (name, avatar, endpoint, capabilities, etc.) | `okx-agent-identity` update flow |
| Agent deactivated / re-activated | `okx-agent-identity` deactivate / activate flow |
| Any other operation that mutates the local a2a agent list | — |

**Recognition cues** (Chinese / English) that should trigger this hook after the upstream action returns: `创建 agent`, `注册 agent`, `更新 agent`, `修改 agent 信息`, `注销 agent`, `停用 agent`, `agent 列表变更`, `agent registered`, `agent created`, `agent updated`, `agent deactivated`, `agent list changed`.

The flow is safe to invoke unconditionally. It self-routes by deterministic shell/runtime signals and then executes the matching runtime branch. The LLM does **not** need to confirm with the user before running Step 1.

## Runtime Decision Tree

```
Step 1  Runtime detection by shell environment
  ├─ OPENCLAW_SHELL is set, or existing OpenClaw compatibility signal OPENCLAW_CLI is set
  │    └─ Step 2 OpenClaw flow
  ├─ HERMES_SESSION_ID is set and HERMES_DESKTOP_CWD is not set
  │    └─ Step 3 Hermes flow (placeholder)
  └─ Otherwise
       └─ Step 4 Node environment flow (Claude Code / Codex / other Node CLI runtimes)
```

## Step 1: Runtime Detection

<MUST>
Before any communication setup, inspect shell environment variables. Do not ask the model or the user to self-report whether the runtime is OpenClaw, Hermes, Claude, or Codex.
</MUST>

Run:

```bash
if [ -n "${OPENCLAW_SHELL:-}" ] || [ -n "${OPENCLAW_CLI:-}" ]; then
  echo "runtime=openclaw"
elif [ -n "${HERMES_SESSION_ID:-}" ] && [ -z "${HERMES_DESKTOP_CWD:-}" ]; then
  echo "runtime=hermes"
else
  echo "runtime=node"
fi
```

- `runtime=openclaw` -> continue to Step 2.
- `runtime=hermes` -> continue to Step 3.
- `runtime=node` -> continue to Step 4.

## Step 2: OpenClaw Flow

The OpenClaw branch is the established implementation. Preserve the existing behavior in this section over any conflicting summary.

### Step 2.1: Fast Path — Refresh Via Plugin Tool

If the OKX A2A plugin is already installed and loaded, the agent's toolset will expose a tool named `xmtp_refresh_agents`.

Inspect the **current toolset**:

- **Tool present** -> call `xmtp_refresh_agents` (no arguments unless its schema requires them) and surface the result to the user. If the call returns an error, surface the error verbatim and stop. Flow ends here.
- **Tool not present** -> the plugin is not yet installed or not loaded. Proceed to Step 2.2 for the full OpenClaw install flow.

### Step 2.2: Install OpenClaw Plugin

#### 2.2.1 Environment Version Check

Run:

```bash
openclaw --version 2>&1
```

Requirements:

- OpenClaw **>= 2026.4.20**

If OpenClaw is below the minimum, inform the user it needs upgrading and stop. Do not proceed.

#### 2.2.2 Update OpenClaw Config

<MUST>
Update config **before** running `openclaw plugins install`. The install command triggers a single automatic gateway restart; that restart needs to load the new plugin and the updated config together.
</MUST>

Run as a single shell block so each check is independent:

```bash
# tools.alsoAllow MUST contain 'group:plugins'
CURRENT=$(openclaw config get tools.alsoAllow 2>/dev/null || echo '[]')
if ! echo "$CURRENT" | grep -q '"group:plugins"'; then
  UPDATED=$(node -e "const a=JSON.parse(process.argv[1]); a.push('group:plugins'); console.log(JSON.stringify(a))" "$CURRENT")
  openclaw config set tools.alsoAllow --strict-json "$UPDATED" 2>&1
fi

# session.dmScope MUST equal 'per-channel-peer'
CURRENT=$(openclaw config get session.dmScope 2>/dev/null || echo '')
if [ "$CURRENT" != '"per-channel-peer"' ]; then
  openclaw config set session.dmScope '"per-channel-peer"' --strict-json 2>&1
fi

# plugins.entries.okx-a2a.hooks.allowConversationAccess MUST equal true
CURRENT=$(openclaw config get plugins.entries.okx-a2a.hooks.allowConversationAccess 2>/dev/null || echo '')
if [ "$CURRENT" != 'true' ]; then
  openclaw config set plugins.entries.okx-a2a.hooks.allowConversationAccess true 2>&1
fi
```

If any `openclaw config set` call fails, surface the error and stop — do not proceed with a partially applied config.

#### 2.2.3 Install Official OpenClaw Plugin

Before running the install command, tell the user in English:

> Installing the OKX A2A plugin from npm package `@okxweb3/a2a-openclaw`. The OpenClaw gateway will restart automatically after installation; this is expected, and no manual action is required.

Run:

```bash
openclaw plugins install @okxweb3/a2a-openclaw
```

If the install fails, surface the error verbatim and stop.

On success, `openclaw plugins install` auto-restarts the gateway, loads the new plugin, and picks up the config changes from Step 2.2.2 in the same restart. Flow ends here — no manual gateway restart and no follow-up `xmtp_refresh_agents` call is needed.

## Step 3: Hermes Flow

Hermes communication initialization is reserved and not implemented yet.

Do not run OpenClaw plugin commands. Do not install or start the Node `okx-a2a` CLI for this runtime. End the flow.

## Step 4: Node Environment Flow

This branch covers Claude Code, Codex, and other non-OpenClaw/non-Hermes environments. It uses the Node CLI package `@okxweb3/a2a-node`, whose executable command is `okx-a2a`.

### Step 4.0: Check For `okx-a2a`

Run:

```bash
if command -v okx-a2a >/dev/null 2>&1; then
  echo "okx_a2a=found"
else
  echo "okx_a2a=missing"
fi
```

- `okx_a2a=found` -> continue to Step 4.1.
- `okx_a2a=missing` -> continue to Step 4.2.

### Step 4.1: Refresh Communication Identity

Run:

```bash
okx-a2a status 2>&1
```

Interpret the status output by its explicit state. Do not infer state from unrelated text.

- **State is `stopped`** -> run:

  ```bash
  okx-a2a restart
  ```

  Then tell the user in English:

  > OKX A2A server has started.

  Flow ends here.

- **State is `running`** -> run:

  ```bash
  okx-a2a agent refresh
  ```

  Surface the result if the command returns user-relevant output. Flow ends here.

- **Status command fails or state is neither `running` nor `stopped`** -> surface the output/error verbatim and stop.

### Step 4.2: Install Node CLI

#### 4.2.1 Choose Package Manager

Detect locally available package managers:

```bash
command -v yarn >/dev/null 2>&1 && echo "pm=yarn"
command -v pnpm >/dev/null 2>&1 && echo "pm=pnpm"
```

- If neither `yarn` nor `pnpm` is present, use `npm` and continue to Step 4.2.2.
- If one or both are present, ask the user which package manager they prefer among `npm` plus the detected options. Do not infer. Continue only after the user chooses one of the offered package managers.

#### 4.2.2 Install `okx-a2a`

Run exactly one command based on the selected package manager:

```bash
npm install -g @okxweb3/a2a-node@latest
```

```bash
yarn global add @okxweb3/a2a-node@latest
```

```bash
pnpm add -g @okxweb3/a2a-node@latest
```

If installation fails, surface the error verbatim and stop.

After installation, verify the executable is available:

```bash
command -v okx-a2a >/dev/null 2>&1
```

If `okx-a2a` is still not found, tell the user the global package manager bin directory is not on `PATH`, then stop. Otherwise continue to Step 4.3.

### Step 4.3: Select AI Provider

Run:

```bash
okx-a2a ai-provider status 2>&1
```

Use the command output as the source of truth for provider names and installation state.

- If no supported AI provider CLI is installed, tell the user to install one supported provider CLI and retry. Flow ends here.
- If one or more supported provider CLIs are installed, ask the user to choose which provider CLI should be used as the task-communication agent. Continue only after the user chooses a provider name from the command output.

Then run:

```bash
okx-a2a config provider --provider <providerName>
```

If the config command fails, surface the error verbatim and stop. Otherwise continue to Step 4.4.

### Step 4.4: Start OKX A2A Server

Run:

```bash
okx-a2a start
```

If the command fails, surface the error verbatim and stop.

On success, OKX A2A communication initialization is complete. Flow ends here.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Runtime signals conflict | OpenClaw wins first, then Hermes, then Node fallback, matching Step 1 order. |
| `OPENCLAW_SHELL` / `OPENCLAW_CLI` is set but `openclaw` command is missing | Inform the user that the OpenClaw CLI is required and stop. |
| Tool `xmtp_refresh_agents` is present | Take the OpenClaw fast path — call it and end. |
| `xmtp_refresh_agents` call returns an error | Surface the error verbatim and stop. |
| OpenClaw < 2026.4.20 | Inform the user OpenClaw is too old and stop. |
| `openclaw config set` fails | Surface the error and stop — do not run install with partial config. |
| Hermes runtime detected | Stop with no side effects; Hermes setup is reserved. |
| Node runtime and `okx-a2a status` reports `stopped` | Run `okx-a2a restart`, tell the user the server started, and end. |
| Node runtime and `okx-a2a status` reports `running` | Run `okx-a2a agent refresh` and end. |
| Node runtime and `okx-a2a status` is unclear | Surface the output/error verbatim and stop. |
| No `yarn` / `pnpm` found | Use `npm` without asking package-manager preference. |
| `yarn` or `pnpm` found | Ask the user to choose among `npm` and the detected package managers. |
| `okx-a2a` still missing after install | Tell the user the global package-manager bin directory is not on `PATH` and stop. |
| No installed AI provider from `okx-a2a ai-provider status` | Tell the user to install one supported provider CLI and retry. |
| User chooses an AI provider | Run `okx-a2a config provider --provider <providerName>`, then `okx-a2a start`. |
