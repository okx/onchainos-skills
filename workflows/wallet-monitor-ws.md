# Wallet Monitor (WebSocket)

> Configure and start a background WebSocket monitoring session that runs independently of the conversation.

## Keyword Glossary

> If the user's query contains Chinese text, read `references/keyword-glossary.md` for trigger mappings.

## Triggers

"background monitor", "offline monitor", "WebSocket monitor", "monitor in background", "long-running wallet watch"

## Required Skills

okx-dex-ws, okx-dex-token, okx-security

## Input

| Param            | Required | Default |
|------------------|----------|---------|
| wallet_addresses | Yes      | Max 10  |
| chain            | No       | Auto    |

**Difference from Polling:**

| Aspect       | Polling                        | WebSocket                        |
|--------------|--------------------------------|----------------------------------|
| Runs in      | AI in-session loop             | Background WS session            |
| AI presence  | Required                       | Not needed after setup           |
| Latency      | polling_interval (default 60s) | Real-time push                   |
| Token cost   | Each poll round                | Setup + on-demand poll only      |
| Best for     | Online, real-time discussion   | Background / offline / scripting |

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
  --chain-index <chainIndex>
```

> `--wallet-addresses` takes comma-separated values (max 200). `--chain-index` accepts comma-separated chain indexes (e.g. `501` for Solana, `1` for Ethereum). Do not use `--params` JSON.

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

- → "research [symbol]" — Token Research (`token-research.md`) (for tokens seen in poll events)
- → "stop monitoring" (`onchainos ws stop --id <session_id>`)
