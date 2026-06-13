# Step 4: Node Environment Flow

> **Precondition**: Step 0 in [ensure-okx-a2a-communication-ready.md](../../ensure-okx-a2a-communication-ready.md) already confirmed `xmtp_refresh_agents` is absent from the toolset, and Step 1 returned `runtime=node`.

This branch covers Claude Code, Codex, and other non-OpenClaw/non-Hermes environments. It uses the Node CLI package `@okxweb3/a2a-node`, whose executable command is `okx-a2a`.

## Node.js Version Gate

Run this check before calling any `okx-a2a` command or running the Node CLI install flow:

```bash
if ! command -v node >/dev/null 2>&1; then
  echo "node=missing"
  exit 1
fi
node --version
node -e "const min=[22,0,0]; const cur=process.versions.node.split('.').map(Number); const ok=cur[0]>min[0] || (cur[0]===min[0] && (cur[1]>min[1] || (cur[1]===min[1] && cur[2]>=min[2]))); if(!ok){console.error('node_version_too_old='+process.versions.node+'; required>=22.0.0'); process.exit(1)} console.log('node_version_ok='+process.versions.node)"
```

If Node.js is missing or below `22.0.0`, inform the user that the OKX A2A Node CLI requires Node.js `>= 22.0.0`. Tell them the AI environment must upgrade Node.js and then rerun this flow. Stop immediately and do not proceed to `okx-a2a` checks or installation until the check passes. After Node.js is upgraded, rerun this branch and continue from Step 4.0.

## Step 4.0: Check For `okx-a2a`

Run:

```bash
if command -v okx-a2a >/dev/null 2>&1; then
  echo "okx_a2a=found"
else
  echo "okx_a2a=missing"
fi
```

- `okx_a2a=found` -> continue to Step 4.1 below.
- `okx_a2a=missing` -> read [node-install.md](node-install.md) and continue from its Step 4.2. Do **not** read `node-install.md` when `okx-a2a` is already installed.

## Step 4.1: Refresh Communication Identity

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

Do not show the bypass-permission prompt from `node-install.md` Step 4.4 in this step when `okx-a2a` was already installed.

## Edge Cases (Node, daily path)

| Scenario | Behavior |
|---|---|
| Node.js is missing or < 22.0.0 | Inform the user Node.js must be upgraded for the OKX A2A Node CLI, stop, then rerun this flow after upgrade and continue. |
| `okx-a2a status` reports `stopped` | Run `okx-a2a restart`, tell the user the server started, and end. |
| `okx-a2a status` reports `running` | Run `okx-a2a agent refresh` and end. |
| `okx-a2a status` is unclear | Surface the output/error verbatim and stop. |
