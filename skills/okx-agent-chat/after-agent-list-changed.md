# After: Agent List Changed

**Mandatory post-hook** — keeps the OpenClaw side in sync after the local a2a agent list changes. Designed to be **auto-triggered by the LLM itself**, without waiting for the user to ask.

## When To Run (Auto-Trigger Contract)

The LLM **must** invoke this flow **on its own**, immediately after any of the following just completed successfully — even if the user did not explicitly ask to "sync" or "refresh":

| Upstream action | Where it typically happens |
|---|---|
| Agent registered / created | `okx-agent-identity` register flow |
| Agent metadata updated (name, avatar, endpoint, capabilities, etc.) | `okx-agent-identity` update flow |
| Agent deactivated / re-activated | `okx-agent-identity` deactivate / activate flow |
| Any other operation that mutates the local a2a agent list | — |

**Recognition cues** (Chinese / English) that should trigger this hook after the upstream action returns: `创建 agent`, `注册 agent`, `更新 agent`, `修改 agent 信息`, `注销 agent`, `停用 agent`, `agent 列表变更`, `agent registered`, `agent created`, `agent updated`, `agent deactivated`, `agent list changed`.

The flow is safe to invoke unconditionally: if the user is not in an OpenClaw runtime (Step 0 below), it becomes a **silent no-op**, so there is no risk of doing the wrong thing in a non-OpenClaw context. The LLM does **not** need to confirm with the user before running it.

## What This Flow Does

When the user **is** running inside an OpenClaw runtime:

- If the OKX A2A plugin is **already installed and loaded**, refresh OpenClaw's cached agent list so the new/updated agent becomes visible without a gateway restart.
- If the plugin is **not yet installed**, install it from npm package `@okxweb3/a2a-openclaw`. If the legacy `openclaw-okx-a2a-extension` plugin is still around, uninstall it first.

When the user is **not** running inside an OpenClaw runtime (e.g., they triggered agent creation through Claude Code, Claude Desktop, or another LLM entry), this flow is a silent no-op.

All steps are idempotent — re-running this flow is safe.

## Target Plugin

- **npm package**: `@okxweb3/a2a-openclaw`
- **Install spec**: `@okxweb3/a2a-openclaw` (bare name pulls the `latest` dist-tag).
- **Legacy plugin id to clean up if present**: `openclaw-okx-a2a-extension`

## Config Requirements

- `tools.alsoAllow` MUST contain `'group:plugins'`
- `session.dmScope` MUST equal `'per-channel-peer'`
- `plugins.entries.okx-a2a.hooks.allowConversationAccess` MUST equal `true`

## Decision Tree

```
Step 0  Runtime detection
  └─ Neither OPENCLAW_CLI nor OPENCLAW_SHELL is set → silent no-op
Step 1  Fast path
  └─ Tool `xmtp_refresh_agents` present in current toolset?
       ├─ Yes → call it → done
       └─ No  → enter full install flow (Step 2 onwards)
Step 2  OpenClaw version check
Step 3  Clean up deprecated debug plugins (id contains `xmtp`)
Step 4  Update OpenClaw config (idempotent)
Step 5  (Uninstall legacy openclaw-okx-a2a-extension if present) → install @okxweb3/a2a-openclaw
```

## Why Config Goes Before Install

`openclaw plugins install` automatically restarts the gateway when it succeeds. That auto-restart is the only restart in the flow. Update `tools.alsoAllow`, `session.dmScope`, and the OKX A2A hook config **before** install so the single auto-restart loads the new plugin alongside the updated config in one pass. These config keys are safe to set while the plugin is not yet installed; the plugin-specific hook key prepares the `okx-a2a` entry OpenClaw will use once the plugin is loaded.

## Execution Flow

### Step 0: Runtime detection

<MUST>
Before any other check, confirm the LLM session is running inside an OpenClaw runtime by inspecting environment variables.
</MUST>

```bash
if [ -n "$OPENCLAW_CLI" ] || [ -n "$OPENCLAW_SHELL" ]; then
  echo "in_openclaw=true"
else
  echo "in_openclaw=false"
fi
```

- `in_openclaw=false` → **silently skip** the entire flow. Do not call any tool, do not message the user. The upstream agent change was triggered from another LLM entry (Claude Code, Claude Desktop, etc.) and does not require any OpenClaw-side sync. Stop here.
- `in_openclaw=true` → proceed to Step 1.

### Step 1: Fast path — refresh via plugin tool

If the OKX A2A plugin is already installed and loaded, the agent's toolset will expose a tool named `xmtp_refresh_agents`.

Inspect the **current toolset**:

- **Tool present** → call `xmtp_refresh_agents` (no arguments unless its schema requires them) and surface the result to the user. If the call returns an error, surface the error verbatim and stop. Flow ends here.
- **Tool not present** → the plugin is not yet installed (or not loaded). Proceed to **Step 2** for the full install flow.

### Step 2: Environment version check

```bash
openclaw --version 2>&1
```

Requirements:
- OpenClaw **>= 2026.4.1**

If OpenClaw is below the minimum, inform the user it needs upgrading and stop. Do not proceed.

### Step 3: Clean up deprecated debug plugins

```bash
openclaw plugins list 2>&1
```

Scan the output for any plugin whose **id contains the substring `xmtp`** (case-insensitive). This naming is deprecated — those plugins were debug/test builds and must be removed before installing the official one.

For **each** matching plugin id:

1. Inform the user, e.g.:

   > ⚠️ 检测到已废弃的调试插件 `<plugin-id>`（命名包含 `xmtp`，已不再使用），即将卸载。

2. Run the uninstall command and wait for it to finish:

   ```bash
   openclaw plugins uninstall <plugin-id>
   ```

3. If uninstall fails, surface the error and stop.

After all deprecated plugins are removed (or none were found), proceed to Step 4.

### Step 4: Update OpenClaw config (idempotent, runs before install)

<MUST>
Update config **before** running `openclaw plugins install`. The install command triggers a single automatic gateway restart; that restart needs to load the new plugin and the updated config together.
</MUST>

Run as a single shell block so each check is independent:

```bash
# 4.1 — tools.alsoAllow MUST contain 'group:plugins'
CURRENT=$(openclaw config get tools.alsoAllow 2>/dev/null || echo '[]')
if ! echo "$CURRENT" | grep -q '"group:plugins"'; then
  UPDATED=$(node -e "const a=JSON.parse(process.argv[1]); a.push('group:plugins'); console.log(JSON.stringify(a))" "$CURRENT")
  openclaw config set tools.alsoAllow --strict-json "$UPDATED" 2>&1
fi

# 4.2 — session.dmScope MUST equal 'per-channel-peer'
CURRENT=$(openclaw config get session.dmScope 2>/dev/null || echo '')
if [ "$CURRENT" != '"per-channel-peer"' ]; then
  openclaw config set session.dmScope '"per-channel-peer"' --strict-json 2>&1
fi

# 4.3 — plugins.entries.okx-a2a.hooks.allowConversationAccess MUST equal true
CURRENT=$(openclaw config get plugins.entries.okx-a2a.hooks.allowConversationAccess 2>/dev/null || echo '')
if [ "$CURRENT" != 'true' ]; then
  openclaw config set plugins.entries.okx-a2a.hooks.allowConversationAccess true 2>&1
fi
```

If any `openclaw config set` call fails, surface the error and stop — do not proceed to Step 5 with a partially applied config.

### Step 5: Uninstall legacy plugin (if present), then install the new one

<MUST>
Before running the install command, you **must** tell the user the gateway will auto-restart afterwards. This sets expectations so the user does not mistake the restart for a bug.
</MUST>

Tell the user (translate to the user's language as needed):

> 即将安装 OKX A2A 插件（npm 包 `@okxweb3/a2a-openclaw`）。安装完成后 openclaw gateway 将自动重启，这是预期行为，请稍候即可，无需手动操作。

#### 5.1 — Detect and remove the legacy plugin (only when present)

Reuse the `openclaw plugins list` output captured in Step 3 (or re-run it). Check whether plugin id `openclaw-okx-a2a-extension` appears.

- **Present** → uninstall it (go through the OpenClaw CLI — do **not** `rm -rf` the extension dir, plugin hooks/daemons/config bindings must be cleaned up properly):

  ```bash
  openclaw plugins uninstall openclaw-okx-a2a-extension
  ```

  If uninstall fails, surface the error and stop.

- **Not present** → skip; nothing to clean up.

#### 5.2 — Install the new package

```bash
openclaw plugins install @okxweb3/a2a-openclaw
```

If the install fails, surface the error verbatim and stop.

On success, the gateway auto-restarts, loads the new plugin, and picks up the config changes from Step 4 in the same restart. The newly created/updated agent will be visible in the refreshed agent list once the gateway is back up. Flow ends here — no further actions needed, no manual restart, no follow-up `xmtp_refresh_agents` call.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Neither `OPENCLAW_CLI` nor `OPENCLAW_SHELL` is set | Silent no-op. The user is not running inside an OpenClaw runtime. |
| Tool `xmtp_refresh_agents` already present in toolset | Take the fast path — call it and end. |
| `xmtp_refresh_agents` call returns an error | Surface the error verbatim and stop. |
| OpenClaw < 2026.4.1 | Inform the user OpenClaw is too old, stop. |
| `openclaw plugins list` fails | Surface the error and stop — cannot determine plugin state. |
| Plugin id contains `xmtp` (deprecated debug build) | Inform the user it is a deprecated test plugin and run `openclaw plugins uninstall <id>` for each match before continuing. |
| `openclaw plugins uninstall <deprecated-id>` fails | Surface the error and stop — do not proceed with stale debug plugin still loaded. |
| `openclaw config get` returns empty / errors | Treat the value as missing and apply the required setting. |
| `openclaw config set` fails | Surface the error and stop — do not run install with partial config. |
| Legacy `openclaw-okx-a2a-extension` not present in `openclaw plugins list` | Skip Step 5.1, go straight to 5.2. |
| `openclaw plugins uninstall openclaw-okx-a2a-extension` fails | Surface the error and stop — do not install while the legacy plugin is half-removed. |
| `openclaw plugins install @okxweb3/a2a-openclaw` fails | Surface the error verbatim and stop. |
| `openclaw` command not found despite `OPENCLAW_*` env var | Inform the user that the OpenClaw CLI is required (rare — Step 0 already confirmed runtime). |
| Plugin already installed and config already in place | Step 1 fast path covers it — single `xmtp_refresh_agents` call, done. |
