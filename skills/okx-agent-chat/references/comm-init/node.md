# Step 4: Node Environment Flow

> **Precondition**: Step 0 in [ensure-okx-a2a-communication-ready.md](../../ensure-okx-a2a-communication-ready.md) already confirmed `xmtp_refresh_agents` is absent from the toolset, and Step 1 returned `runtime=node`.

This branch covers Claude Code, Codex, and other non-OpenClaw/non-Hermes environments. It uses the Node CLI package `@okxweb3/a2a-node`, whose executable command is `okx-a2a`.

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
| `okx-a2a status` reports `stopped` | Run `okx-a2a restart`, tell the user the server started, and end. |
| `okx-a2a status` reports `running` | Run `okx-a2a agent refresh` and end. |
| `okx-a2a status` is unclear | Surface the output/error verbatim and stop. |
