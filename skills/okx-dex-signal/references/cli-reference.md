# Onchain OS DEX Tracker — CLI Command Reference

Full parameter tables, return field schemas, and usage examples for the 4 tracker watch commands.

---

## 1. `onchainos tracker watch start`

Start a background WebSocket watch session. Returns a session ID immediately; the daemon
connects and authenticates asynchronously.

```bash
onchainos tracker watch start [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--channel` | No | `kol_smartmoney-tracker-activity` | Channel to subscribe. Can be specified multiple times. Values: `kol_smartmoney-tracker-activity`, `address-tracker-activity` |
| `--wallet-addresses` | When channel is `address-tracker-activity` | — | Comma-separated wallet addresses (EVM `0x...` or Solana base58), max 20 |
| `--env` | No | `prod` | Environment: `prod` or `pre` |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `id` | String | Session ID (e.g. `watch_abc123`) — use for poll/stop |
| `status` | String | `"starting"` or `"already_running"` |
| `pid` | Number | Daemon process ID |
| `channels` | Array | Subscribed channel names |
| `wallet_addresses` | Array | Wallet addresses (for `address-tracker-activity`) |
| `env` | String | `"prod"` or `"pre"` |
| `dir` | String | Local storage path for this session |

**Examples**:

```bash
# KOL + smart money channel (no address needed)
onchainos tracker watch start --channel kol_smartmoney-tracker-activity

# Custom address tracking
onchainos tracker watch start \
  --channel address-tracker-activity \
  --wallet-addresses 0xAAA,0xBBB,0xCCC

# Both channels in one session
onchainos tracker watch start \
  --channel kol_smartmoney-tracker-activity \
  --channel address-tracker-activity \
  --wallet-addresses 0xAAA,0xBBB

# Pre environment
onchainos tracker watch start --channel kol_smartmoney-tracker-activity --env pre
```

---

## 2. `onchainos tracker watch poll`

Read incremental trade events from a running watch session. Advances a cursor so each call
only returns events that arrived since the last poll.

```bash
onchainos tracker watch poll --id <session-id> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--id` | Yes | — | Session ID from `watch start` |
| `--channel` | No | First channel in session | Channel to poll |
| `--limit` | No | `20` | Max events to return |
| `--trade-type` | No | all | `buy` / `1` or `sell` / `2` |
| `--min-quote-amount` | No | — | Min quote token amount (e.g. USDT value) |
| `--min-market-cap` | No | — | Min token market cap (USD) |
| `--min-pnl` | No | — | Min realized PnL (USD); use `0` for profit-only trades |
| `--trader` | No | — | Filter by wallet address (exact or prefix match) |
| `--tag` | No | all | `smart_money` / `sm` / `1` or `kol` / `2` |
| `--since` | No | — | Only return events with `tradeTime` ≥ this Unix ms timestamp |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `daemon_status` | String | `running`, `reconnecting`, `disconnected:<reason>`, `crashed` |
| `new_count` | Number | Number of events returned after filtering |
| `last_trade_time` | String | `tradeTime` of the last returned event (Unix ms) |
| `trades` | Array | Trade event objects (see trade event schema below) |

**Trade event schema**:

| Field | Type | Description |
|---|---|---|
| `walletAddress` | String | Wallet address that made the trade |
| `tokenSymbol` | String | Traded token symbol |
| `tokenContractAddress` | String | Token contract address |
| `chainIndex` | String | Chain identifier (e.g. `"1"` = Ethereum, `"501"` = Solana) |
| `tokenPrice` | String | Token price at trade time (USD) |
| `marketCap` | String | Token market cap at trade time (USD) |
| `quoteTokenSymbol` | String | Quote token symbol (e.g. `"USDT"`, `"SOL"`) |
| `quoteTokenAmount` | String | Amount of quote token traded |
| `tradeType` | String | `"1"` = Buy, `"2"` = Sell |
| `tradeTime` | String | Trade timestamp (Unix milliseconds) |
| `realizedPnlUsd` | String | Realized PnL for this trade (USD) |
| `trackerType` | Array\<Number\> | Wallet tags: `1` = Smart Money, `2` = KOL |
| `txHash` | String | Transaction hash (optional) |

**Examples**:

```bash
# Basic poll
onchainos tracker watch poll --id watch_abc123

# Only buy events, limit 50
onchainos tracker watch poll --id watch_abc123 --trade-type buy --limit 50

# Filter: KOL trades, min $5k quote amount
onchainos tracker watch poll --id watch_abc123 --tag kol --min-quote-amount 5000

# Filter: specific wallet prefix
onchainos tracker watch poll --id watch_abc123 --trader 0x1234

# Catch up from a specific timestamp
onchainos tracker watch poll --id watch_abc123 --since 1742700000000
```

---

## 3. `onchainos tracker watch stop`

Stop a running session and clean up its local storage. If `--id` is omitted, all sessions are stopped.

```bash
onchainos tracker watch stop [--id <session-id>] [--flush]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--id` | No | — | Session ID to stop. Omit to stop all sessions |
| `--flush` | No | false | Read and return any unread events before stopping |

**Return fields** (single stop):

| Field | Type | Description |
|---|---|---|
| `id` | String | Stopped session ID |
| `status` | String | `"stopped"` |
| `flushed_count` | Number | Number of events flushed (0 if `--flush` not set) |

**Return fields** (stop all):

| Field | Type | Description |
|---|---|---|
| `stopped` | Array | List of stopped session IDs |

**Examples**:

```bash
# Stop a specific session
onchainos tracker watch stop --id watch_abc123

# Stop and flush remaining events
onchainos tracker watch stop --id watch_abc123 --flush

# Stop all sessions
onchainos tracker watch stop
```

---

## 4. `onchainos tracker watch list`

List all watch sessions with their current status.

```bash
onchainos tracker watch list
```

No parameters. Returns an array of session objects:

| Field | Type | Description |
|---|---|---|
| `id` | String | Session ID |
| `status` | String | `running`, `reconnecting`, `disconnected:<reason>`, `stopped`, `crashed` |
| `pid` | Number | Daemon process ID |
| `channels` | Array | Subscribed channels |
| `env` | String | `"prod"` or `"pre"` |
| `created_at` | String | Session creation timestamp (Unix ms) |

**Example**:

```bash
onchainos tracker watch list
# [
#   { "id": "watch_abc123", "status": "running", "channels": ["kol_smartmoney-tracker-activity"], ... },
#   { "id": "watch_def456", "status": "stopped", "channels": ["address-tracker-activity"], ... }
# ]
```

---

## Daemon Status Reference

| Status | Meaning | Action |
|---|---|---|
| `running` | Connected and streaming | Normal — keep polling |
| `reconnecting` | Temporarily disconnected, retrying (up to 20×, 3s interval) | Wait and poll again |
| `disconnected:<reason>` | Disconnected, reason attached | Check reason; may self-recover |
| `disconnected:max_reconnect_reached` | Exhausted all retries | Stop session and restart |
| `stopped` | Session was stopped | No further polls needed |
| `crashed` | Daemon stopped heartbeating (>60s) | Stop and restart session |
