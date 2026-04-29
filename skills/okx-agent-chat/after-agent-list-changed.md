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
- If the plugin is **not yet installed**, install it from a local `~/Downloads/openclaw-okx-a2a-extension-<version>.tgz` package and set the required OpenClaw config; the auto-restart triggered by `openclaw plugins install` then loads both in a single pass.

When the user is **not** running inside an OpenClaw runtime (e.g., they triggered agent creation through Claude Code, Claude Desktop, or another LLM entry), this flow is a silent no-op.

All steps are idempotent — re-running this flow is safe.

## Target Plugin

- **Plugin ID**: `openclaw-okx-a2a-extension`
- **Local package pattern**: `~/Downloads/openclaw-okx-a2a-extension-<version>.tgz`

## Config Requirements

- `tools.alsoAllow` MUST contain `'group:plugins'`
- `session.dmScope` MUST equal `'per-channel-peer'`

## Decision Tree

```
Step 0  Runtime detection
  └─ Neither OPENCLAW_CLI nor OPENCLAW_SHELL is set → silent no-op
Step 1  Fast path
  └─ Tool `xmtp_refresh_agents` present in current toolset?
       ├─ Yes → call it → done
       └─ No  → enter full install flow (Step 2 onwards)
Step 2  Node + OpenClaw version check
Step 3  Clean up deprecated debug plugins (id contains `xmtp`)
Step 4  Locate the .tgz package in ~/Downloads
Step 5  Update OpenClaw config (idempotent)
Step 6  Notify user, then `openclaw plugins install` (auto-restart)
```

## Why Config Goes Before Install

`openclaw plugins install` automatically restarts the gateway when it succeeds. That auto-restart is the only restart in the flow. Update `tools.alsoAllow` and `session.dmScope` **before** install so the single auto-restart loads the new plugin alongside the updated config in one pass. Both config keys are general (not coupled to any specific plugin id), so it is safe to set them while the plugin is not yet installed.

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
node --version && openclaw --version 2>&1
```

Requirements:
- Node **≥ 22.14**
- OpenClaw **≥ 2026.3.0**

If either is below the minimum, inform the user which component needs upgrading and stop. Do not proceed.

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

### Step 4: Locate the local package

```bash
ls ~/Downloads/ 2>/dev/null | grep -E '^openclaw-okx-a2a-extension-[0-9]+(\.[0-9]+)*\.tgz$'
```

**Branch A — no matching file**

If no file matches, send the user the official download link and stop:

> 未在 `~/Downloads/` 找到 OKX A2A 插件包（命名格式：`openclaw-okx-a2a-extension-<version>.tgz`）。
> 请先到下面的文档下载最新插件包，下载完成后重新执行本流程：
> https://okg-block.sg.larksuite.com/wiki/JMilw2rFoipWrLkZtSfloqgrgtu#share-DDsIdTHcTog5umxvxzjlXOsVgdD

(Translate the prompt to match the user's language; keep the URL unchanged.)

**Branch B — one or more matching files**

Pick the highest semantic version:

```bash
PLUGIN_PATH=$(
  ls ~/Downloads/ 2>/dev/null \
    | grep -E '^openclaw-okx-a2a-extension-[0-9]+(\.[0-9]+)*\.tgz$' \
    | sort -V \
    | tail -n 1
)
PLUGIN_PATH="$HOME/Downloads/$PLUGIN_PATH"
echo "Selected plugin package: $PLUGIN_PATH"
```

Confirm with the user before continuing:

> 在 `~/Downloads/` 找到插件包：`openclaw-okx-a2a-extension-<version>.tgz`
> 是否使用此包进行安装？(yes / no)

- User declines → stop. Ask them to place the desired version into `~/Downloads/` and re-run.
- User confirms → proceed to Step 5.

### Step 5: Update OpenClaw config (idempotent, runs before install)

<MUST>
Update config **before** running `openclaw plugins install`. The install command triggers a single automatic gateway restart; that restart needs to load the new plugin and the updated config together.
</MUST>

Run as a single shell block so each check is independent:

```bash
# 5.1 — tools.alsoAllow MUST contain 'group:plugins'
CURRENT=$(openclaw config get tools.alsoAllow 2>/dev/null || echo '[]')
if ! echo "$CURRENT" | grep -q '"group:plugins"'; then
  UPDATED=$(node -e "const a=JSON.parse(process.argv[1]); a.push('group:plugins'); console.log(JSON.stringify(a))" "$CURRENT")
  openclaw config set tools.alsoAllow --strict-json "$UPDATED" 2>&1
fi

# 5.2 — session.dmScope MUST equal 'per-channel-peer'
CURRENT=$(openclaw config get session.dmScope 2>/dev/null || echo '')
if [ "$CURRENT" != '"per-channel-peer"' ]; then
  openclaw config set session.dmScope '"per-channel-peer"' --strict-json 2>&1
fi
```

If any `openclaw config set` call fails, surface the error and stop — do not proceed to Step 6 with a partially applied config.

### Step 6: Notify user, then run install

<MUST>
Before running the install command, you **must** tell the user that the gateway will auto-restart afterwards. This sets expectations so the user does not mistake the restart for a bug.
</MUST>

Tell the user (translate to the user's language as needed):

> 即将安装 OKX A2A 插件。安装完成后 openclaw gateway 将自动重启，这是预期行为，请稍候即可，无需手动操作。

Then run:

```bash
openclaw plugins install "$PLUGIN_PATH"
```

If the install fails, surface the error and stop.

On success, the gateway auto-restarts, loads the new plugin, and picks up the config changes from Step 5 in the same restart. The newly created agent will be visible in the refreshed agent list once the gateway is back up. Flow ends here — no further actions needed, no manual restart, no follow-up `xmtp_refresh_agents` call.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Neither `OPENCLAW_CLI` nor `OPENCLAW_SHELL` is set | Silent no-op. The user is not running inside an OpenClaw runtime. |
| Tool `xmtp_refresh_agents` already present in toolset | Take the fast path — call it and end. |
| `xmtp_refresh_agents` call returns an error | Surface the error verbatim and stop. |
| Node < 22.14 or OpenClaw < 2026.3.0 | Inform the user which component is too old, stop. |
| `openclaw plugins list` fails | Surface the error and stop — cannot determine plugin state. |
| Plugin id contains `xmtp` (deprecated debug build) | Inform the user it is a deprecated test plugin and run `openclaw plugins uninstall <id>` for each match before continuing. |
| `openclaw plugins uninstall <deprecated-id>` fails | Surface the error and stop — do not proceed with stale debug plugin still loaded. |
| No matching `.tgz` in `~/Downloads/` | Send the user the download link (https://okg-block.sg.larksuite.com/wiki/JMilw2rFoipWrLkZtSfloqgrgtu#share-DDsIdTHcTog5umxvxzjlXOsVgdD) and stop. |
| Multiple matching `.tgz` files | Pick the highest semantic version (`sort -V \| tail -n 1`), confirm with the user before installing. |
| User declines the candidate `.tgz` | Stop. Ask the user to place the desired version into `~/Downloads/` and re-run. |
| `openclaw config get` returns empty / errors | Treat the value as missing and apply the required setting. |
| `openclaw config set` fails | Surface the error and stop — do not run install with partial config. |
| `openclaw plugins install` fails | Surface the error and stop. |
| `openclaw` command not found despite `OPENCLAW_*` env var | Inform the user that the OpenClaw CLI is required (rare — Step 0 already confirmed runtime). |
| Plugin already installed and config already in place | Step 1 fast path covers it — single `xmtp_refresh_agents` call, done. |
