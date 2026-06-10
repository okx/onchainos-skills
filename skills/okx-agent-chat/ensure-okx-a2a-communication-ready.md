# Ensure OKX A2A Communication Ready

**Mandatory communication-init flow** — ensures OKX A2A communication is ready for the current runtime. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

Runtime families:

- **OpenClaw**: uses the OpenClaw OKX A2A plugin and native `xmtp_*` tools.
- **Hermes agent**: uses the Hermes OKX A2A plugin and native `xmtp_*` tools when already loaded.
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

The flow is safe to invoke unconditionally. It first checks whether communication is already ready in the current toolset, then self-routes by deterministic shell/runtime signals only when setup is still required. The LLM does **not** need to confirm with the user before running Step 0.

## Runtime Decision Tree

```
Step 0  Toolset self-check
  ├─ xmtp_refresh_agents is present
  │    └─ Call it directly and end
  └─ Tool is absent
       └─ Step 1 Runtime detection
            ├─ HERMES_SESSION_ID is set and HERMES_DESKTOP_CWD is not set
            │    └─ Step 3 Hermes flow
            ├─ OPENCLAW_SHELL or OPENCLAW_CLI is set
            │    └─ Step 2 OpenClaw flow
            ├─ An OpenClaw process is found in the parent-process chain (up to 8 levels)
            │    └─ Step 2 OpenClaw flow
            └─ Otherwise
                 └─ Step 4 Node environment flow
```

## Step 0: Toolset Self-Check

<MUST>
Inspect the LLM's current toolset before running any shell command. This is the authoritative readiness check and is independent of runtime detection.
</MUST>

- **`xmtp_refresh_agents` is present** -> call it directly (no arguments unless its schema requires them). If it succeeds, surface only user-relevant output and end the flow.
- **`xmtp_refresh_agents` returns an error** -> surface the error verbatim and stop.
- **`xmtp_refresh_agents` is absent** -> continue to Step 1.

Do not run runtime detection, installation checks, or gateway health checks when the tool is already present.

## Step 1: Runtime Detection

<MUST>
When Step 0 does not find `xmtp_refresh_agents`, run the shell function below. Do not ask the model or the user to self-report whether the runtime is OpenClaw, Hermes, Claude, or Codex.
</MUST>

Run:

```bash
detect_runtime() {
  # Hermes first: this signal shape is the most specific.
  if [ -n "${HERMES_SESSION_ID:-}" ] && [ -z "${HERMES_DESKTOP_CWD:-}" ]; then
    echo "hermes"
    return
  fi

  # Preserve legacy OpenClaw environment hints as the cheap path.
  if [ -n "${OPENCLAW_SHELL:-}" ] || [ -n "${OPENCLAW_CLI:-}" ]; then
    echo "openclaw"
    return
  fi

  # Cover newer OpenClaw/Codex launch shapes by walking at most 8 parents.
  pid=$PPID
  for _ in 1 2 3 4 5 6 7 8; do
    if [ -z "$pid" ] || [ "$pid" = "0" ] || [ "$pid" = "1" ]; then
      break
    fi
    comm=$(ps -p "$pid" -o comm= 2>/dev/null | tr -d ' ')
    case "$comm" in
      *openclaw*|*OpenClaw*)
        echo "openclaw"
        return
        ;;
    esac
    pid=$(ps -p "$pid" -o ppid= 2>/dev/null | tr -d ' ')
  done

  echo "node"
}

runtime=$(detect_runtime)
echo "runtime=$runtime"
```

- `runtime=openclaw` -> continue to Step 2.
- `runtime=hermes` -> continue to Step 3.
- `runtime=node` -> continue to Step 4.

The PPID walk inspects process names only. Do not check socket files, use `lsof`, ask the LLM/user to declare the runtime, or use gateway status as runtime detection.

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
if ! command -v openclaw >/dev/null 2>&1; then
  echo "openclaw_cli=missing"
  exit 1
fi
openclaw --version 2>&1
```

Requirements:

- OpenClaw **>= 2026.4.20**

If the OpenClaw CLI is missing, tell the user to install the OpenClaw CLI and stop. This includes runtimes identified through the PPID fallback.

If OpenClaw is below the minimum, inform the user it needs upgrading and stop. Do not proceed.

#### 2.2.2 Update OpenClaw Config

<MUST>
Update config **before** running `openclaw plugins install`. After the install succeeds, you MUST run `openclaw gateway restart` so the new plugin and the updated config are loaded together.
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

> Installing the OKX A2A plugin from npm package `@okxweb3/a2a-openclaw`. After installation succeeds, I will run `openclaw gateway restart` as a required step so the gateway loads the plugin and updated config.

Run:

```bash
openclaw plugins install @okxweb3/a2a-openclaw
```

If the install fails, surface the error verbatim and stop.

After the install succeeds, run:

```bash
openclaw gateway restart
```

If the restart fails, surface the error verbatim and stop.

On success, the gateway loads the new plugin and picks up the config changes from Step 2.2.2. Flow ends here — no follow-up `xmtp_refresh_agents` call is needed.

## Step 3: Hermes Flow

Use this branch when a Hermes user needs to install the `okx-a2a` plugin from the npm package and restart Hermes Gateway.

### Step 3.1: Fast Path — Refresh Via Plugin Tool

If the OKX A2A Hermes plugin is already installed and loaded, the agent's toolset will expose a tool named `xmtp_refresh_agents`.

Inspect the **current toolset**:

- **Tool present** -> call `xmtp_refresh_agents` (no arguments unless its schema requires them) and surface the result to the user. If the call returns an error, surface the error verbatim and stop. Flow ends here.
- **Tool not present** -> the plugin is not yet installed or not loaded. Proceed to Step 3.2 for the full Hermes install flow.

### Step 3.2: Check Node.js Version

Run:

```bash
node --version
```

Requirements:

- Node.js **>= 22.14.0**

If Node.js is below the minimum, inform the user it needs upgrading and stop. Do not proceed.

### Step 3.3: Download The Hermes Plugin Package

Create a temporary working directory and download the latest npm package:

```bash
mkdir -p /tmp/okx-a2a-hermes-install
cd /tmp/okx-a2a-hermes-install
npm pack @okxweb3/a2a-hermes@latest
```

If `npm pack` fails, surface the error verbatim and stop.

Extract the npm package:

```bash
tar -xzf okxweb3-a2a-hermes-*.tgz
```

Extract the Hermes plugin package inside the npm package:

```bash
tar -xzf package/dist/okx-a2a-hermes-plugin-*.tar.gz
cd okx-a2a-hermes-plugin
```

Verify the plugin package contents:

```bash
ls __init__.py plugin.yaml package.json scripts/install-or-upgrade.sh src dist/server.js
```

If any required file or directory is missing, surface the error and stop.

On macOS, remove quarantine attributes from native files bundled in the plugin package before installing:

```bash
xattr -dr com.apple.quarantine .
```

### Step 3.4: Install Or Upgrade The Plugin

Before running the install command, tell the user in English:

> Installing or upgrading the OKX A2A Hermes plugin from npm package `@okxweb3/a2a-hermes`. Hermes Gateway will restart automatically after installation; this is expected, and no manual action is required.

Install or upgrade the plugin into the Hermes **user plugins directory** and automatically restart Hermes Gateway:

```bash
bash scripts/install-or-upgrade.sh --restart --target "$HOME/.hermes/plugins/platforms/okx-a2a"
```

If the install fails, surface the error verbatim and stop.

Always pass `--target "$HOME/.hermes/plugins/platforms/okx-a2a"` so the plugin lands in the Hermes user plugins directory (`~/.hermes/plugins/`), not inside the Hermes git clone.

User-installed plugins are gated by the `plugins.enabled` allow-list in `~/.hermes/config.yaml`. The install script adds the entry automatically; if it prints `ACTION REQUIRED` (an existing `plugins:` section it will not modify), ask the user to add this to `~/.hermes/config.yaml` and restart the gateway:

```yaml
plugins:
  enabled:
    - okx-a2a
```

On success, Hermes communication initialization is complete. Flow ends here.

### Step 3.5: Manual Gateway Restart Fallback

If the install script should not restart Hermes Gateway automatically, install only (same `--target` requirement as Step 3.4):

```bash
bash scripts/install-or-upgrade.sh --target "$HOME/.hermes/plugins/platforms/okx-a2a"
```

Then restart Hermes Gateway manually:

```bash
hermes gateway restart
```

If the current Hermes CLI does not provide a `restart` command, run:

```bash
hermes gateway stop
hermes gateway run --replace
```

If Gateway still reports that a `.node` native file cannot be opened after restart, remove quarantine attributes from the installed plugin directory and restart Gateway again:

```bash
cd ~/.hermes/plugins/platforms/okx-a2a
xattr -dr com.apple.quarantine .
hermes gateway restart
```

If a legacy copy exists at `~/.hermes/hermes-agent/plugins/platforms/okx-a2a`, remove it so only the user-plugins copy is loaded.

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

### Step 4.4: Start OKX A2A Daemon

This step is only reached after Step 4.2 installed missing `okx-a2a`. Do not show the bypass-permission prompt from this step in Step 4.1 when `okx-a2a` was already installed.

Run:

```bash
okx-a2a restart
```

If the command fails, surface the error verbatim and stop.

After the daemon restart succeeds, tell the user in English:

> Bypass permission mode is on by default to skip per-step confirmations. To approve actions manually, just tell the agent to run "okx-a2a agent bypass off"

On success, OKX A2A communication initialization is complete. Flow ends here.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Tool `xmtp_refresh_agents` is present | Step 0 calls it immediately and ends without shell runtime detection. |
| `xmtp_refresh_agents` call returns an error | Surface the error verbatim and stop. |
| Runtime signals conflict | Hermes' specific signal shape wins first, then OpenClaw env hints, then the OpenClaw PPID fallback, then Node. |
| OpenClaw is detected by env or PPID but the `openclaw` command is missing | Tell the user to install the OpenClaw CLI and stop. |
| PPID walk reaches PID 0/1, an empty PID, or 8 levels without finding OpenClaw | Fall back to Node. |
| OpenClaw < 2026.4.20 | Inform the user OpenClaw is too old and stop. |
| `openclaw config set` fails | Surface the error and stop — do not run install with partial config. |
| Hermes runtime and tool `xmtp_refresh_agents` is missing | Continue to the Hermes install flow. |
| Node runtime and `okx-a2a status` reports `stopped` | Run `okx-a2a restart`, tell the user the server started, and end. |
| Node runtime and `okx-a2a status` reports `running` | Run `okx-a2a agent refresh` and end. |
| Node runtime and `okx-a2a status` is unclear | Surface the output/error verbatim and stop. |
| No `yarn` / `pnpm` found | Use `npm` without asking package-manager preference. |
| `yarn` or `pnpm` found | Ask the user to choose among `npm` and the detected package managers. |
| `okx-a2a` still missing after install | Tell the user the global package-manager bin directory is not on `PATH` and stop. |
| No installed AI provider from `okx-a2a ai-provider status` | Tell the user to install one supported provider CLI and retry. |
| User chooses an AI provider after installing missing `okx-a2a` | Run `okx-a2a config provider --provider <providerName>`, then `okx-a2a restart`; after restart succeeds, show the bypass-permission prompt from Step 4.4. |
