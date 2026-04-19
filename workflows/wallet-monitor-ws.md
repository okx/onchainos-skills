# Wallet Monitor (WebSocket)

> Configure and start a background WebSocket monitoring session that runs independently of the conversation.

## Triggers

"后台监控", "挂一个ws盯着", "离线监控", "WebSocket监控", "长期盯着这个钱包", "background monitor"

## Required Skills

okx-dex-ws, okx-dex-token, okx-security

## Input

| Param             | Required | Default |
|-------------------|----------|---------|
| wallet_addresses  | Yes      | Max 10  |
| chain             | No       | Auto    |

**Difference from W8 (Polling):**

| Aspect        | W8 Polling                     | W9 WebSocket                        |
|---------------|--------------------------------|-------------------------------------|
| Runs in       | AI in-session loop             | Background WS session               |
| AI presence   | Required                       | Not needed after setup              |
| Latency       | polling_interval (default 60s) | Real-time push                      |
| Token cost    | Each poll round                | Setup + on-demand poll only         |
| Best for      | Online, real-time discussion   | Background / offline / scripting    |

## Steps

### Step 1 — Check available channels [required] (sequential)

```
onchainos ws channels
onchainos ws channel-info --channel address-tracker-activity
```

> Channel name must match what `ws channel-info` returns.

Present: available channels, subscription parameters for address-tracker-activity

### Step 2 — Start session [required] (sequential)

```
onchainos ws start \
  --channel address-tracker-activity \
  --wallet-addresses "<addr1>,<addr2>" \
  --chain <chain>
```

> `--wallet-addresses` takes comma-separated values (max 200). Do not use `--params` JSON.

Present: session ID, subscription confirmation

### Step 3 — Verify session [required] (sequential)

```
onchainos ws list
```

Present: active sessions list, confirm new session is running

### Step 4 — Show consumption options [required] (sequential)

Manual poll:

```
onchainos ws poll --id <session_id>
```

Scripted poll (example):

```bash
while true; do
  onchainos ws poll --id <session_id> --limit 50
  sleep 30
done
```

When user runs `ws poll` and new events are returned, optionally enrich:

```
onchainos token price-info --address <event_token> --chain <chain>
onchainos security token-scan --tokens "<chainIndex>:<event_token>"
```

## Output Template

```
WS MONITOR STARTED
Session: {session_id}
Channel: address-tracker-activity
Addresses: {addr1}, {addr2}...
Status: Active

To check events:
  onchainos ws poll --id {session_id}

To stop:
  onchainos ws stop --id {session_id}

To list all sessions:
  onchainos ws list
```

## Actions

- → "看看 [symbol]" — triggers Token Research (for tokens seen in poll events)
- → "用 [amount] [native_token] 买 [symbol]" — triggers Safe Swap
- → "停止监控" (`onchainos ws stop --id <session_id>`)

## Follow-up Workflows

Token Research (`workflows/token-research.md`), Safe Swap (`workflows/safe-swap.md`)
