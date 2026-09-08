# Capability: Market

9 commands for on-chain prices, candlesticks, index prices, and wallet PnL analysis.

## Commands

| # | Command | Use When |
|---|---|---|
| 1 | `onchainos market price --address <address>` | Single token price (**default for all price queries**) |
| 2 | `onchainos market prices --tokens <tokens>` | Batch price query (multiple tokens at once) |
| 3 | `onchainos market kline --address <address>` | K-line / candlestick chart — **only when the user explicitly mentions chart, candle, K-line, OHLC, or bar data; a timeframe alone is NOT sufficient** |
| 4 | `onchainos market index --address <address>` | Index price — **only when user explicitly asks for aggregate/cross-exchange price** |
| 5 | `onchainos market portfolio-supported-chains` | Check which chains support PnL |
| 6 | `onchainos market portfolio-overview` | Wallet PnL overview (win rate, realized PnL, top 3 tokens) |
| 7 | `onchainos market portfolio-dex-history` | Wallet DEX transaction history |
| 8 | `onchainos market portfolio-recent-pnl` | Recent PnL by token for a wallet |
| 9 | `onchainos market portfolio-token-pnl` | Per-token PnL snapshot (realized/unrealized) |

### `portfolio-dex-history` time window

`onchainos market portfolio-dex-history --address <addr> --chain <chain>` needs a time window: pass **either** `--since 24h`/`--since 7d` (CLI computes it and returns `data.resolvedWindow {begin,end}`) **or** absolute `--begin <ms> --end <ms>`. Supplying neither, or `--since` together with `--begin`/`--end`, returns a structured `invalid_input` error.

**Mandatory routing rules:**

**Index price** → `onchainos market index` only when the user explicitly asks for an aggregate, index, or cross-exchange composite price. For every other price or "how much is X" query, use `onchainos market price`.

**K-line** → `onchainos market kline` only when the user explicitly mentions a chart, candle, candlestick, K-line, OHLC, or bar. A timeframe alone ("5 minutes", "1h", "daily") does NOT trigger kline; default to `onchainos market price`. Examples: "BTC 5-minute candlestick chart" → kline. "BTC 5-minute up/down market" → blocked and routed to Polymarket by the top-level skill. "BTC 5-minute price" → price.

### Step 1: Collect Parameters

- Missing chain → ask the user which chain they want to use before proceeding; for portfolio PnL queries, first call `onchainos market portfolio-supported-chains` to confirm the chain is supported
- Missing token address → use the **Token** capability's `onchainos token search` first to resolve
- K-line requests → confirm bar size and time range with user

### Step 2: Call and Display

- Call directly, return formatted results
- Use appropriate precision: 2 decimals for high-value tokens, significant digits for low-value
- Show USD value alongside
- **Kline field mapping**: The CLI returns named JSON fields using short API names. Always translate to human-readable labels when presenting to users: `ts` → Time, `o` → Open, `h` → High, `l` → Low, `c` → Close, `vol` → Volume, `volUsd` → Volume (USD), `confirm` → Status (0=incomplete, 1=completed). Never show raw field names like `o`, `h`, `l`, `c` to users.

## Data Freshness

Render `requestTime` (Unix ms) as-is. Do NOT chain off a previous response's `requestTime`; for a relative window use `--since` on `portfolio-dex-history`.

## Amount Display Rules

- Always display in UI units (`1.5 ETH`), never base units
- Show USD value alongside (`1.5 ETH ≈ $4,500`)
- Prices are strings — handle precision carefully
