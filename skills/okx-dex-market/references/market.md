# Capability: Market

## Flow

For a non-hot Market request, collect the business inputs below, then read
[market-cli-reference.md](market-cli-reference.md) to select the exact command.
The CLI Reference owns command syntax, defaults, and command-level validation.

**Mandatory routing rules:**

**Index price** → select the index-price command only when the user explicitly
asks for an aggregate, index, or cross-exchange composite price. For every
other price or "how much is X" query, select the single-token price command.

**K-line** → select the K-line command only when the user explicitly mentions a
chart, candle, candlestick, K-line, OHLC, or bar. A timeframe alone ("5 minutes",
"1h", "daily") does not trigger K-line; default to a single-token price. For
example, "BTC 5-minute candlestick chart" is K-line; "BTC 5-minute up/down
market" is routed to Polymarket by the Root Skill; "BTC 5-minute price" is price.

### Collect Parameters

- Missing chain → ask the user which chain they want to use before proceeding; for portfolio PnL queries, first call `onchainos market portfolio-supported-chains` to confirm the chain is supported
- Missing token address → use the **Token** capability's `onchainos token search` first to resolve
- K-line requests → confirm bar size and time range with user
- For non-literal Chinese market terminology, read [market-keyword-glossary.md](market-keyword-glossary.md) before selecting a command.

### Execute and Handle Result

- Call directly, return formatted results
- Use appropriate precision: 2 decimals for high-value tokens, significant digits for low-value
- Show USD value alongside
- **Kline field mapping**: The CLI returns named JSON fields using short API names. Always translate to human-readable labels when presenting to users: `ts` → Time, `o` → Open, `h` → High, `l` → Low, `c` → Close, `vol` → Volume, `volUsd` → Volume (USD), `confirm` → Status (0=incomplete, 1=completed). Never show raw field names like `o`, `h`, `l`, `c` to users.

## Data Freshness

Render `requestTime` (Unix ms) as-is. Do NOT chain off a previous response's `requestTime`; for a relative window use `--since` on `portfolio-dex-history`.

## Render Rules

- Always display in UI units (`1.5 ETH`), never base units
- Show USD value alongside (`1.5 ETH ≈ $4,500`)
- Prices are strings — handle precision carefully

## Troubleshooting

- [market-troubleshooting.md](market-troubleshooting.md)

## CLI Reference

- [market-cli-reference.md](market-cli-reference.md)
