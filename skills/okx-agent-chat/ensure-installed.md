# Ensure A2A openclaw Plugin Installed

**Mandatory safeguard** — run this every time the agent needs to communicate with another agent, initiate agent commerce, or use agent-to-agent messaging. Verifies the host environment, installs the OKX A2A plugin from a local `~/Downloads/openclaw-okx-a2a-extension-{version}.tgz` package if missing, ensures the required OpenClaw config is in place, and finally prompts the user to restart the gateway when changes were made.

All steps are idempotent — re-running this flow is safe.

## Target Plugin

- **Plugin ID**: `openclaw-okx-a2a-extension`
- **Local package pattern**: `~/Downloads/openclaw-okx-a2a-extension-{version}.tgz`

## Config Requirements

- `tools.alsoAllow` MUST contain `'group:plugins'`
- `session.dmScope` MUST equal `'per-channel-peer'`

## Flow Variables

Track these two booleans across the run. Both default to `false`.

| Variable | Meaning |
|---|---|
| `isPluginInstalled` | The target plugin already appears in `openclaw plugins list` at Step 2 |
| `isConfigChanged` | One or more config entries were mutated during Step 4 |

## Command Index

| Step | Command | Description |
|---|---|---|
| 1 | `node --version` / `openclaw --version` | Verify Node ≥ 22.14 and OpenClaw ≥ 2026.3.0 |
| 2 | `openclaw plugins list` | Check whether `openclaw-okx-a2a-extension` is installed |
| 3 | `openclaw plugins install ~/Downloads/openclaw-okx-a2a-extension-<version>.tgz` | Install the highest-versioned local package |
| 4 | `openclaw config get` / `openclaw config set` | Verify and update `tools.alsoAllow` and `session.dmScope` |
| 5 | (prompt only) `openclaw gateway restart` | Ask the user to restart the gateway when changes were made |

## Why Gateway Restart Is Required

The extension is an **OpenClaw plugin**. It registers a channel, daemon, hooks, skills, and services inside its `register()` method, which only runs at gateway startup. Config under `tools.*` / `session.*` likewise takes effect at startup. So whenever the plugin is freshly loaded or relevant config is mutated, the gateway needs a full restart to apply the change.

## Execution Flow

### Step 1: Environment check

<MUST>
Verify the host environment meets plugin prerequisites before continuing.
</MUST>

```bash
node --version && openclaw --version 2>&1
```

Requirements:
- Node **≥ 22.14**
- OpenClaw **≥ 2026.3.0**

If either is below the minimum, inform the user which component needs upgrading and stop. Do not proceed to later steps.

### Step 2: Check whether the plugin is installed

```bash
openclaw plugins list 2>&1
```

#### 2.1 Clean up deprecated debug plugin

Before checking the target plugin, scan the output for any plugin whose **id contains `xmtp`** (case-insensitive). This naming is deprecated — those plugins were debug/test builds and should be removed.

For **each** matching plugin id:

1. Inform the user, e.g.:

   > ⚠️ 检测到已废弃的调试插件 `<plugin-id>`（命名包含 `xmtp`，已不再使用），即将卸载。

2. Run the uninstall command and wait for it to finish before continuing:

   ```bash
   openclaw plugins uninstall <plugin-id>
   ```

3. If uninstall fails for any deprecated plugin, surface the error and stop — do not proceed to 2.2.

After all deprecated plugins are removed (or if none were found), proceed to 2.2.

#### 2.2 Check the target plugin

Look for `openclaw-okx-a2a-extension` in the output collected at the start of Step 2.

- If found → set `isPluginInstalled = true` and skip to **Step 4**.
- If not found → keep `isPluginInstalled = false` and proceed to **Step 3**.

### Step 3: Install the plugin from `~/Downloads`

<MUST>
This step is reached only when the plugin is not installed (`isPluginInstalled = false`). It must complete successfully before continuing to Step 4.
</MUST>

#### 3.1 Locate the local package

List candidate packages in the user's `~/Downloads` directory:

```bash
ls ~/Downloads/ 2>/dev/null | grep -E '^openclaw-okx-a2a-[0-9]+(\.[0-9]+)*\.tgz$'
```

**Branch A — no matching file found**

If no file matches the pattern, do **not** continue. Send the user the official download link and stop, asking them to download the plugin package first and then re-run this flow:

> 未在 `~/Downloads/` 找到 OKX A2A 插件包（命名格式：`openclaw-okx-a2a-<version>.tgz`）。
> 请先到下面的文档下载最新插件包，下载完成后重新执行本流程：
> https://okg-block.sg.larksuite.com/wiki/JMilw2rFoipWrLkZtSfloqgrgtu#share-DDsIdTHcTog5umxvxzjlXOsVgdD

(Translate the prompt to match the user's language; keep the URL unchanged.)

**Branch B — one or more matching files found**

Pick the file with the **highest semantic version**:

```bash
PLUGIN_PATH=$(
  ls ~/Downloads/ 2>/dev/null \
    | grep -E '^openclaw-okx-a2a-[0-9]+(\.[0-9]+)*\.tgz$' \
    | sort -V \
    | tail -n 1
)
PLUGIN_PATH="$HOME/Downloads/$PLUGIN_PATH"
echo "Selected plugin package: $PLUGIN_PATH"
```

Then **confirm with the user** before installing. Show the selected path and ask for explicit approval, e.g.:

> 在 `~/Downloads/` 找到插件包：`openclaw-okx-a2a-<version>.tgz`
> 是否使用此包进行安装？(yes / no)

- If the user declines or wants to use a different version → stop. Ask them to place the desired `openclaw-okx-a2a-<version>.tgz` into `~/Downloads/` and re-run this flow.
- If the user confirms → proceed to **3.2**.

#### 3.2 Install the package

Run the install command and **wait for it to finish** before moving on. Do not proceed to Step 4 until the command exits cleanly.

```bash
openclaw plugins install "$PLUGIN_PATH"
```

If the install fails, surface the error to the user and stop. Otherwise, proceed to **Step 4**.

### Step 4: Verify and update OpenClaw config

<MUST>
Each sub-step is independent and idempotent. For any entry that already matches the requirement, do nothing. For any entry that does not match, run the corresponding `openclaw config set` command and set `isConfigChanged = true`.
</MUST>

Run this block as a single shell invocation so the `isConfigChanged` flag persists into the Step 5 decision:

```bash
isConfigChanged=false

# 4.1 — tools.alsoAllow MUST contain 'group:plugins'
CURRENT=$(openclaw config get tools.alsoAllow 2>/dev/null || echo '[]')
if ! echo "$CURRENT" | grep -q '"group:plugins"'; then
  UPDATED=$(node -e "const a=JSON.parse(process.argv[1]); a.push('group:plugins'); console.log(JSON.stringify(a))" "$CURRENT")
  openclaw config set tools.alsoAllow --strict-json "$UPDATED" 2>&1
  isConfigChanged=true
fi

# 4.2 — session.dmScope MUST equal 'per-channel-peer'
CURRENT=$(openclaw config get session.dmScope 2>/dev/null || echo '')
if [ "$CURRENT" != '"per-channel-peer"' ]; then
  openclaw config set session.dmScope '"per-channel-peer"' --strict-json 2>&1
  isConfigChanged=true
fi

echo "isConfigChanged=$isConfigChanged"
```

If any `openclaw config set` call fails, surface the error to the user and stop — do not proceed to Step 5 with a partially applied config.

### Step 5: Prompt the user to restart the gateway

Decision rule:

> If `isPluginInstalled === true` **OR** `isConfigChanged === true`, prompt the user to run `openclaw gateway restart` to apply the changes.
> Otherwise, inform the user that everything is up to date and no restart is needed.

Prompt template (translate to the user's language as needed):

> ✅ A2A plugin and config are ready. Please run the following command to apply the changes:
> ```
> openclaw gateway restart
> ```

Flow ends here. The agent does **not** restart the gateway automatically — the restart is the user's explicit action.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Node < 22.14 or OpenClaw < 2026.3.0 | Inform the user which component is too old, stop. Do not attempt install. |
| `openclaw plugins list` fails | Surface the error and stop — cannot determine plugin state. |
| Plugin id contains `xmtp` (deprecated debug build) | Inform the user it is a deprecated test plugin and run `openclaw plugins uninstall <id>` for each match before checking the target plugin. |
| `openclaw plugins uninstall <deprecated-id>` fails | Surface the error and stop — do not proceed to install / config steps with stale debug plugin still loaded. |
| No `openclaw-okx-a2a-*.tgz` file in `~/Downloads/` | Send the user the download link (https://okg-block.sg.larksuite.com/wiki/JMilw2rFoipWrLkZtSfloqgrgtu#share-DDsIdTHcTog5umxvxzjlXOsVgdD) and stop. |
| Multiple matching `.tgz` files | Pick the highest semantic version (`sort -V \| tail -n 1`), then confirm with the user before installing. |
| User declines the candidate `.tgz` | Stop. Ask the user to place the desired version into `~/Downloads/` and re-run. |
| `openclaw plugins install` fails | Surface the error and stop. Do not proceed to Step 4. |
| `openclaw config get` returns empty / errors | Treat the value as missing and apply the required setting. |
| `openclaw config set` fails | Surface the error and stop. Do not prompt for restart. |
| Plugin already installed and config already in place | Both flags stay `false` → no restart prompt; flow ends with an "all good" message. |
| `openclaw` command not found | Inform the user that the OpenClaw CLI is required. |
