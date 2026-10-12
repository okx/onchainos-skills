# Capability: Signal

## Intent Routing

| User intent | Reference | Hot command |
|---|---|---|
| Show chains supported by signal alerts | — | `onchainos signal chains` |
| Show chains supported by the top-trader leaderboard | — | `onchainos leaderboard supported-chains` |

For these rows, call the canonical command directly. Do not read the CLI Reference or run `--help` first; continue to apply this Reference's result-handling rules.

## Flow

For a request not covered by a hot command, collect the business inputs below,
then read [signal-cli-reference.md](signal-cli-reference.md) to select the exact
command. The CLI Reference owns command syntax, defaults, and command-level
validation.

> **Mandatory routing rule:** If the user wants actual transaction-level trades, including sells, use tracker. If the user wants tokens that triggered buy alerts across multiple wallets, use signal list.

### Collect Parameters

**Address Tracker:**
- `--tracker-type` is required: `smart_money`, `kol`, or `multi_address`
- `--wallet-address` is required when `--tracker-type multi_address`; omit for smart_money/kol
- `--trade-type` defaults to `0` (all); use `1` for buy-only, `2` for sell-only
- `--chain` is optional — omit to get results across all chains
- Optional token filters (use when user wants to narrow results by token quality or size):
  - `--min-volume` / `--max-volume` — trade volume range (USD)
  - `--min-market-cap` / `--max-market-cap` — token market cap range (USD)
  - `--min-liquidity` / `--max-liquidity` — token liquidity range (USD)
  - `--min-holders` — minimum number of token holders

**Signal:**
- Missing chain → always call `onchainos signal chains` first to confirm the chain is supported
- Signal filter params (`--wallet-type`, `--min-amount-usd`, etc.) → ask user for preferences if not specified; default to no filter (returns all signal types)
- `--token-address` is optional — omit to get all signals on the chain; include to filter for a specific token
- **`--wallet-type` is multi-select** (comma-separated integers: `1`=Smart Money, `2`=KOL/Influencer, `3`=Whale) — e.g. `--wallet-type 1,3` returns both Smart Money and Whale signals
- **Pagination**: `signal list` supports `--limit` (default `20`, max `100`) and `--cursor`. Each response item includes a `cursor` field; pass the **last item's `cursor`** as `--cursor` on the next call to page forward.

**Leaderboard:**
- Missing chain → call `onchainos leaderboard supported-chains` to confirm support; default to `solana` if user doesn't specify
- `--time-frame` and `--sort-by` are required by the CLI but the agent should infer them from user language before asking — use the mappings below. Only prompt the user if intent is genuinely ambiguous.
- Missing `--time-frame` → map "today/1D" → `1`, "3 days/3D" → `2`, "7 days/1W/7D" → `3`, "1 month/30D" → `4`, "3 months/3M" → `5`
- Missing `--sort-by` → map "PnL" → `1`, "win rate" → `2`, "transaction count" → `3`, "volume" → `4`, "ROI" → `5`
- **`--wallet-type` is single-select only** (one value at a time: `sniper`, `dev`, `fresh`, `pump`, `smartMoney`, `influencer`) — do NOT pass comma-separated values or it will error; if omitted, all types are returned
- For non-literal Chinese signal or leaderboard terminology, read [signal-keyword-glossary.md](signal-keyword-glossary.md) before selecting a command.

### Execute and Handle Result

**Address Tracker:**
- Present as a transaction feed table: time, wallet address (truncated), token symbol, trade direction (Buy/Sell), amount USD, price, realized PnL
- Translate `tradeType`: `1` → "Buy", `2` → "Sell"

**Signal:**
- Present signals in a readable table: token symbol, wallet type, amount USD, trigger wallet count, price at signal time
- Translate `walletType` values: `"1"` → "Smart Money", `"2"` → "KOL/Influencer", `"3"` → "Whale"
- Show `soldRatioPercent` — lower means the wallet is still holding (bullish signal)

**Leaderboard:**
- Returns at most 20 entries per request
- Present as a ranked table: rank, wallet address (truncated), PnL, win rate, tx count, volume
- Translate field names — never dump raw JSON keys to the user

## Data Freshness

### `requestTime` Field

When a response includes a `requestTime` field (Unix milliseconds), display it alongside results so the user knows when the snapshot was taken. When chaining commands (e.g., showing trade details after a signal), use the `requestTime` from the most recent response as the reference point for any time-based parameters.

## Troubleshooting

- [signal-troubleshooting.md](signal-troubleshooting.md)

## CLI Reference

- [signal-cli-reference.md](signal-cli-reference.md)
