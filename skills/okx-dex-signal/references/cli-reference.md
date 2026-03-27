# Onchain OS DEX Signal — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for the tracker, signal, leaderboard, and tracker watch commands.

---

## 1. onchainos tracker activities (address tracker)

Get latest DEX activities for tracked addresses. Supports smart money, KOL, or custom multi-address tracking, with filters for trade type, chain, volume, market cap, liquidity, and holder count.

```bash
onchainos tracker activities --tracker-type <type> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--tracker-type` | Yes | - | Tracker type: `smart_money` (or `1`) = platform smart money; `kol` (or `2`) = platform Top 100 KOL addresses; `multi_address` (or `3`) = custom addresses |
| `--wallet-address` | Conditional | - | Required when `--tracker-type multi_address`. Comma-separated wallet addresses, max 20 |
| `--trade-type` | No | `0` (all) | Trade direction: `0`=all, `1`=buy, `2`=sell |
| `--chain` | No | all chains | Chain filter (e.g., `ethereum`, `solana`, `bsc`, `base`, `xlayer`) |
| `--min-volume` | No | - | Minimum trade volume (USD) |
| `--max-volume` | No | - | Maximum trade volume (USD) |
| `--min-holders` | No | - | Minimum number of holding addresses |
| `--min-market-cap` | No | - | Minimum market cap (USD) |
| `--max-market-cap` | No | - | Maximum market cap (USD) |
| `--min-liquidity` | No | - | Minimum liquidity (USD) |
| `--max-liquidity` | No | - | Maximum liquidity (USD) |

**Return fields** (inside `trades` array):

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Transaction hash |
| `walletAddress` | String | Wallet address of the transaction |
| `quoteTokenSymbol` | String | Pricing token symbol (mainnet native token) |
| `quoteTokenAmount` | String | Amount of pricing token traded |
| `tokenSymbol` | String | Trading token symbol |
| `tokenContractAddress` | String | Trading token contract address |
| `chainIndex` | String | Chain identifier where the trading token is located |
| `tokenPrice` | String | Trading price of the token (USD) |
| `marketCap` | String | Market cap at the transaction price (USD) |
| `realizedPnlUsd` | String | Realized PnL of the trading token (USD) |
| `tradeType` | String | Trade direction: `1`=buy, `2`=sell |
| `tradeTime` | String | Transaction time (Unix milliseconds) |
| `trackerType` | Array\<String\> | Tracker type tags for this trade; values: `"1"`=smart_money, `"2"`=kol, `"3"`=multi_address. May be empty `[]` if the API does not populate the field for this trade. |

**Examples**:

```bash
# Latest trades by platform smart money (all chains)
onchainos tracker activities --tracker-type smart_money

# Latest buys by KOL addresses on Solana
onchainos tracker activities --tracker-type kol --chain solana --trade-type 1

# Latest trades for custom wallet addresses
onchainos tracker activities --tracker-type multi_address \
  --wallet-address 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045,0xab5801a7d398351b8be11c439e05c5b3259aec9b

# Smart money buys with volume filter
onchainos tracker activities --tracker-type smart_money --trade-type 1 --min-volume 10000
```

---

## 2. onchainos signal chains

Get supported chains for market signals. No parameters required.

```bash
onchainos signal chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `chainName` | String | Human-readable chain name (e.g., `"Ethereum"`, `"Solana"`) |
| `chainLogo` | String | Chain logo image URL |

> Call this first when signal data is needed — confirm chain support before calling `onchainos signal list`.

## 3. onchainos signal list

Get latest buy-direction token signals sorted descending by time.

```bash
onchainos signal list --chain <chain> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chain` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `base`) |
| `--wallet-type` | No | all types | Wallet classification, comma-separated: `1`=Smart Money, `2`=KOL/Influencer, `3`=Whale (e.g., `"1,2"`) |
| `--min-amount-usd` | No | - | Minimum transaction amount in USD |
| `--max-amount-usd` | No | - | Maximum transaction amount in USD |
| `--min-address-count` | No | - | Minimum triggering wallet address count |
| `--max-address-count` | No | - | Maximum triggering wallet address count |
| `--token-address` | No | - | Token contract address (filter signals for a specific token) |
| `--min-market-cap-usd` | No | - | Minimum token market cap in USD |
| `--max-market-cap-usd` | No | - | Maximum token market cap in USD |
| `--min-liquidity-usd` | No | - | Minimum token liquidity in USD |
| `--max-liquidity-usd` | No | - | Maximum token liquidity in USD |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `timestamp` | String | Signal timestamp (Unix milliseconds) |
| `chainIndex` | String | Chain identifier |
| `price` | String | Token price at signal time (USD) |
| `walletType` | String | Wallet classification: `"1"`=Smart Money, `"2"`=KOL/Influencer, `"3"`=Whale |
| `triggerWalletCount` | String | Number of wallets that triggered this signal |
| `triggerWalletAddress` | String | Comma-separated wallet addresses that triggered the signal |
| `amountUsd` | String | Total transaction amount in USD |
| `soldRatioPercent` | String | Percentage of tokens sold (lower = still holding) |
| `token.tokenAddress` | String | Token contract address |
| `token.symbol` | String | Token symbol |
| `token.name` | String | Token name |
| `token.logo` | String | Token logo URL |
| `token.marketCapUsd` | String | Token market cap in USD |
| `token.holders` | String | Number of token holders |
| `token.top10HolderPercent` | String | Percentage of supply held by top 10 holders |

## Input / Output Examples

**User says:** "What are smart money wallets buying on Solana?" (transaction-level)

```bash
onchainos tracker activities --tracker-type smart_money --chain solana --trade-type 1
# -> Display latest smart money buy transactions on Solana
```

**User says:** "Show me smart money buy signal alerts on Solana" (aggregated alerts)

```bash
onchainos signal chains   # confirm Solana is supported
onchainos signal list --chain solana --wallet-type 1
# -> Display aggregated smart money buy signals with token info
```

**User says:** "Show me whale buys above $10k on Ethereum" (signal alerts)

```bash
onchainos signal list --chain ethereum --wallet-type 3 --min-amount-usd 10000
# -> Display whale-only buy signal alerts, min $10k
```

---

## 4. onchainos leaderboard supported-chains


Get supported chains for the leaderboard. No parameters required.

```bash
onchainos leaderboard supported-chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `chainName` | String | Human-readable chain name (e.g., `"Ethereum"`, `"Solana"`) |
| `chainLogo` | String | Chain logo URL |

> Call this first to confirm chain support before calling `onchainos leaderboard list`.

---

## 5. onchainos leaderboard list

Get top trader leaderboard ranked by PnL, win rate, volume, tx count, or ROI. Returns at most 20 entries per request.

```bash
onchainos leaderboard list --chain <chain> --time-frame <tf> --sort-by <sort> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chain` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `base`) |
| `--time-frame` | Yes | - | Statistics window: `1`=1D, `2`=3D, `3`=7D, `4`=1M, `5`=3M |
| `--sort-by` | Yes | - | Sort field: `1`=PnL, `2`=Win Rate, `3`=Tx number, `4`=Volume, `5`=ROI |
| `--wallet-type` | No | all types | Single-select wallet type: `sniper`, `dev`, `fresh`, `pump`, `smartMoney`, `influencer` |
| `--min-realized-pnl-usd` | No | - | Minimum realized PnL (USD) |
| `--max-realized-pnl-usd` | No | - | Maximum realized PnL (USD) |
| `--min-win-rate-percent` | No | - | Minimum win rate % (0–100) |
| `--max-win-rate-percent` | No | - | Maximum win rate % (0–100) |
| `--min-txs` | No | - | Minimum number of transactions |
| `--max-txs` | No | - | Maximum number of transactions |
| `--min-tx-volume` | No | - | Minimum transaction volume (USD) |
| `--max-tx-volume` | No | - | Maximum transaction volume (USD) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `walletAddress` | String | Wallet address |
| `realizedPnlUsd` | String | Cumulative realized PnL (USD) in the selected time frame |
| `realizedPnlPercent` | String | Cumulative realized PnL % in the selected time frame |
| `winRatePercent` | String | Win rate % (profitable tokens / total traded tokens) |
| `avgBuyValueUsd` | String | Average buy value (USD) |
| `topPnlTokenList` | Array | Top 3 tokens by PnL |
| `topPnlTokenList[].tokenContractAddress` | String | Token contract address |
| `topPnlTokenList[].tokenSymbol` | String | Token symbol |
| `topPnlTokenList[].tokenPnLUsd` | String | Token PnL (USD) |
| `topPnlTokenList[].tokenPnLPercent` | String | Token PnL % |
| `txVolume` | String | Total transaction volume (USD) in the selected time frame |
| `txs` | String | Total transaction count in the selected time frame |
| `lastActiveTimestamp` | String | Last active time (Unix milliseconds) |

**Examples**:

```bash
# Top traders on Solana by PnL over last 7D
onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1

# Top smart money on Ethereum by win rate over last 30D
onchainos leaderboard list --chain ethereum --time-frame 4 --sort-by 2 --wallet-type smartMoney

# Top snipers on BSC by volume over last 1D, min 10 txs
onchainos leaderboard list --chain bsc --time-frame 1 --sort-by 4 --wallet-type sniper --min-txs 10
```

---

## 6. `onchainos tracker watch start`

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

## 7. `onchainos tracker watch poll`

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

## 8. `onchainos tracker watch stop`

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

## 9. `onchainos tracker watch list`

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
