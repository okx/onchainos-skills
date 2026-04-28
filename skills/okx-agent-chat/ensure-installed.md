# Ensure XMTP Plugin Installed

**Mandatory safeguard** — run this every time the agent needs to communicate with another agent, initiate agent commerce, or use XMTP messaging. Verifies environment prerequisites, upgrades device scope, installs the XMTP plugin (`openclaw-a2a-chat-extension`) if missing, and injects the required OpenClaw config entries. After completion, automatically proceeds to `check-version.md` to check for updates.

All steps are idempotent — re-running this flow is safe.

## Command Index

| Step | Command | Description |
|---|---|---|
| 1 | `node --version` / `openclaw --version` | Verify Node >= 22.14 and OpenClaw >= 2026.3.0 |
| 2 | `openclaw devices approve --latest` | Upgrade device scope to admin (first-time only) |
| 3 | `npm list -g openclaw-a2a-chat-extension --depth=0` | Check if the XMTP plugin is installed |
| 4.1 | `openclaw gateway stop` | Stop the gateway before installation |
| 4.2 | `npm install -g openclaw-a2a-chat-extension@latest` | Install the latest version |
| 5 | Config injection + gateway restart (single block) | Inject 4 config entries, then start/restart gateway as needed |

## Why Gateway Restart Is Required

The XMTP extension is an **OpenClaw plugin** (not a standalone channel). It loads via the `plugins.*` config path, registers a channel, daemon, hooks, skills, and services inside its `register()` method, and initializes the daemon on `gateway:startup`. Hot reload cannot re-run `register()`, and config changes under `plugins.*` / `tools.*` / `session.*` only take effect at startup — a full gateway stop/start cycle is required whenever the package is installed/updated or config is mutated.

## Execution Flow

### Step 1: Environment check

<MUST>
Verify the host environment meets plugin prerequisites before continuing.
</MUST>

Run:
```bash
node --version && openclaw --version 2>&1
```

Requirements:
- Node **>= 22.14**
- OpenClaw **>= 2026.3.0**

If either is below the minimum, inform the user which component needs upgrading and stop. Do not proceed to later steps.

### Step 2: Upgrade device scope to admin

<MUST>
Config mutation (Step 5) and plugin registration require admin scope. This is a first-time bootstrap step; subsequent runs no-op cleanly.
</MUST>

Run:
```bash
openclaw devices approve --latest 2>&1
```

- Expected first-run output: `Approved <deviceId> (<requestId>)`.
- If the output says `no pending request` → device is already admin-scoped. Skip and continue.
- On any other error, surface it to the user and stop.

### Step 3: Check if installed

Run:
```bash
npm list -g openclaw-a2a-chat-extension --depth=0 2>/dev/null
```

- If the package appears in the output (shows a version number) → **installed**. Skip Step 4 and proceed to Step 5.
- If the output says `(empty)` or the package is not listed → **not installed**. Proceed to Step 4.

### Step 4: Install the extension

<MUST>
**Gateway must be stopped before installation and started after Step 5.** The plugin's `register()` method runs at gateway startup — installing while the gateway is running will not load the new plugin.
</MUST>

First, stop the gateway:
```bash
openclaw gateway stop
```

Then install:
```bash
npm install -g openclaw-a2a-chat-extension@latest
```

If installation succeeds:
- Inform the user: "XMTP plugin installed successfully."
- Proceed to Step 5 (do **not** start the gateway yet — config must be written first).

If installation fails:
- Display the error message to the user.
- Suggest checking npm permissions (`npm config get prefix`) or network connectivity.
- Start the gateway so it isn't left stopped:
  ```bash
  openclaw gateway start
  ```
- Do not proceed to Step 5.

### Step 5: Inject OpenClaw config and restart gateway

<MUST>
All four config entries are required for the XMTP plugin to load and expose its tools. Each sub-step is idempotent — safe to re-run if an earlier attempt left the config in a partial state. The entire block MUST run in a single shell invocation so that the `CONFIG_CHANGED` flag persists through to the gateway restart decision.
</MUST>

Run this entire block as **one command** (do not split into separate shell invocations):

```bash
CONFIG_CHANGED=0

# 5.1 — Add xmtp to the plugin allow-list (idempotent JSON-array append)
CURRENT=$(openclaw config get plugins.allow 2>/dev/null || echo '[]')
if ! echo "$CURRENT" | grep -q '"xmtp"'; then
  UPDATED=$(node -e "const a=JSON.parse(process.argv[1]); a.push('xmtp'); console.log(JSON.stringify(a))" "$CURRENT")
  openclaw config set plugins.allow --strict-json "$UPDATED" 2>&1
  CONFIG_CHANGED=1
fi

# 5.2 — Enable the plugin entry (set-if-different)
CURRENT=$(openclaw config get plugins.entries.xmtp.enabled 2>/dev/null || echo '')
if [ "$CURRENT" != "true" ]; then
  openclaw config set plugins.entries.xmtp.enabled true --strict-json 2>&1
  CONFIG_CHANGED=1
fi

# 5.3 — Expose plugin tools to the LLM (idempotent JSON-array append)
CURRENT=$(openclaw config get tools.alsoAllow 2>/dev/null || echo '[]')
if ! echo "$CURRENT" | grep -q '"group:plugins"'; then
  UPDATED=$(node -e "const a=JSON.parse(process.argv[1]); a.push('group:plugins'); console.log(JSON.stringify(a))" "$CURRENT")
  openclaw config set tools.alsoAllow --strict-json "$UPDATED" 2>&1
  CONFIG_CHANGED=1
fi

# 5.4 — Set session isolation policy (set-if-different)
CURRENT=$(openclaw config get session.dmScope 2>/dev/null || echo '')
if [ "$CURRENT" != '"per-channel-peer"' ]; then
  openclaw config set session.dmScope '"per-channel-peer"' --strict-json 2>&1
  CONFIG_CHANGED=1
fi

# 6 — Gateway restart decision
if [ "$FRESH_INSTALL" = "1" ]; then
  # Gateway was stopped in Step 4 — start it
  openclaw gateway start
  echo "openclaw gateway started."
elif [ "$CONFIG_CHANGED" = "1" ]; then
  # Config mutated on an already-installed setup — full restart needed
  openclaw gateway stop && openclaw gateway start
  echo "openclaw gateway restarted to apply config changes."
else
  echo "XMTP plugin already installed and configured — no restart needed."
fi
```

Before running this block, set `FRESH_INSTALL=1` if Step 4 was executed (fresh install path), or `FRESH_INSTALL=0` if Step 4 was skipped (already installed).

### Step 6: Proceed to version check

After the plugin is installed and config is in place, automatically load and follow `check-version.md` to check for available updates.

## Edge Cases

| Scenario | Behavior |
|---|---|
| Node < 22.14 or OpenClaw < 2026.3.0 | Inform user which component is too old, stop. Do not attempt install. |
| `openclaw devices approve --latest` output = `no pending request` | Device already admin-scoped; continue normally |
| `openclaw devices approve --latest` fails with other error | Surface error, stop — config mutation will fail without admin scope |
| `plugins.allow` is unset (returns error/empty) | Treat as `[]` and append `"xmtp"` |
| npm not found | Inform the user that Node.js/npm is required. Suggest installing via https://nodejs.org |
| Permission denied on npm install -g | Suggest `sudo npm install -g` or fixing npm global prefix |
| openclaw command not found | Inform the user that OpenClaw CLI is required |
| Gateway stop fails | Display error, attempt install anyway — the gateway may not have been running |
| Gateway start fails | Display error, suggest manual start |
| Already installed, config already in place | Skip install and skip gateway restart, proceed directly to version check |
