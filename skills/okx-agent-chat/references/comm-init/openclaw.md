# Step 2: OpenClaw Flow

> **Precondition**: [ensure-okx-a2a-communication-ready.md](../../ensure-okx-a2a-communication-ready.md) already confirmed `xmtp_refresh_agents` is absent from the toolset, and the runtime detector returned `runtime: "openclaw"`. If the tool is in fact present in the current toolset, go back and call it directly instead — do not run this install flow.

The OpenClaw branch is the established implementation. Preserve the existing behavior in this file over any conflicting summary.

## Step 2.2: Install OpenClaw Plugin

### 2.2.1 Environment Version Checks

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
- Node.js **>= 22.19.0**

If the OpenClaw CLI is missing, tell the user to install the OpenClaw CLI and stop. This includes runtimes identified through the PPID fallback.

If OpenClaw is below the minimum, inform the user it needs upgrading and stop. Do not proceed.

Then run:

```bash
if ! command -v node >/dev/null 2>&1; then
  echo "node=missing"
  exit 1
fi
node --version
node -e "const min=[22,19,0]; const cur=process.versions.node.split('.').map(Number); const ok=cur[0]>min[0] || (cur[0]===min[0] && (cur[1]>min[1] || (cur[1]===min[1] && cur[2]>=min[2]))); if(!ok){console.error('node_version_too_old='+process.versions.node+'; required>=22.19.0'); process.exit(1)} console.log('node_version_ok='+process.versions.node)"
```

If Node.js is missing or below `22.19.0`, inform the user that the OpenClaw OKX A2A plugin requires Node.js `>= 22.19.0`. Tell them the AI environment must upgrade Node.js and then rerun this flow. Stop immediately and do not proceed to config or plugin installation until the check passes. After Node.js is upgraded, rerun this step and continue installation from Step 2.2.2.

### 2.2.2 Update OpenClaw Config

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

### 2.2.3 Install Official OpenClaw Plugin

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

## Edge Cases (OpenClaw)

| Scenario | Behavior |
|---|---|
| OpenClaw is detected by env or PPID but the `openclaw` command is missing | Tell the user to install the OpenClaw CLI and stop. |
| OpenClaw < 2026.4.20 | Inform the user OpenClaw is too old and stop. |
| Node.js is missing or < 22.19.0 | Inform the user Node.js must be upgraded for the OpenClaw OKX A2A plugin, stop, then rerun this flow after upgrade and continue installation. |
| `openclaw config set` fails | Surface the error and stop — do not run install with partial config. |
