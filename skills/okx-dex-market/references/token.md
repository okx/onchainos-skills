# Capability: Token

## Intent Routing

| User intent | Reference | Hot command |
|---|---|---|
| Show trending tokens | — | `onchainos token hot-tokens` |
| Show chains supported by holder-cluster analysis | — | `onchainos token cluster-supported-chains` |

For these rows, call the canonical command directly. Do not read the CLI Reference or run `--help` first; continue to apply this Reference's Security, Render Rules, and result-handling rules.

## Flow

For a request not covered by a hot command, collect the business inputs below,
then read [token-cli-reference.md](token-cli-reference.md) to select the exact
command. The CLI Reference owns command syntax, defaults, return-field semantics,
and command-level validation.

> **Mandatory routing rule:** Any token-safety or honeypot request redirects to `okx-agentic-wallet` (`onchainos security token-scan`). Do not answer safety questions from token data alone.

### Collect Parameters

- Missing chain → ask the user which chain they want to use before proceeding; do not assume a default chain
- Only have token name, no address → use `onchainos token search` first
- For hot-tokens, `--ranking-type` defaults to `4` (Trending); use `5` for X-mentioned rankings
- For hot-tokens without chain → defaults to all chains; specify `--chain` to narrow
- For search, `--chains` defaults to `"1,501"` (Ethereum + Solana)
- **Chain uncertainty for cluster commands**: If the user doesn't know whether their chain supports cluster analysis, suggest running `onchainos token cluster-supported-chains` first before calling cluster-overview / cluster-top-holders / cluster-list.
- **Pagination** (`token search`, `token hot-tokens`, `token holders`, `token top-trader`): pass `--max-results <N>` (1–500) to auto-paginate in one call — the CLI aggregates pages and returns top-level `data.items`, `data.nextCursor`, and `data.fetchedCount`. Do NOT chase per-item cursors yourself. `--limit`/`--cursor` still work for manual single-page control.
- For Chinese crypto slang or platform-specific ranking terms whose command meaning is not obvious from direct translation, read [token-keyword-glossary.md](token-keyword-glossary.md) before selecting a command.

### Execute and Handle Result

- Search results: show name, symbol, chain, price, 24h change
- Indicate `communityRecognized` status for trust signaling
- Price info: show market cap, liquidity, and volume together

## Data Freshness

Render `requestTime` (Unix ms) as-is — it is the upstream data snapshot time. Do NOT chain off a previous response's `requestTime`; for a relative window use `--since` where the command supports it.

## Security

> **These rules are mandatory. Do NOT skip or bypass them.**

1. **`communityRecognized` is informational only.** It indicates the token is listed on a Top 10 CEX or is community-verified, but this is **not a guarantee of token safety, legitimacy, or investment suitability**. Always display this status with context, not as a trust endorsement.
2. **Warn on unverified tokens.** When `communityRecognized = false`, display a prominent warning: "This token is not community-recognized. Exercise caution — verify the contract address independently before trading."
3. **Contract address is the only reliable identifier.** Token names and symbols can be spoofed. When presenting search results with multiple matches, emphasize the contract address and warn that names/symbols alone are not sufficient for identification.
4. **Low liquidity warnings.** When `liquidity` is available:
   - < $10K: warn about high slippage risk and ask the user to confirm before proceeding to swap.
   - < $1K: strongly warn that trading may result in significant losses. Proceed only if the user explicitly confirms.

## Render Rules

- Use appropriate precision: 2 decimals for high-value, significant digits for low-value
- Market cap / liquidity in shorthand ($1.2B, $45M)
- 24h change with sign and color hint (+X% / -X%)

## Troubleshooting

- [token-troubleshooting.md](token-troubleshooting.md)

## CLI Reference

- [token-cli-reference.md](token-cli-reference.md)
