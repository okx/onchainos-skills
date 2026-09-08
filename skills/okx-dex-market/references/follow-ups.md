# Result Follow-ups

Use this reference only after a DEX command succeeds. Present next actions conversationally and never expose command paths to the user.

## Workflow Hints

When the completed command matches a row, show the workflow hint after displaying the result:

> You can also try out our **[workflow name]** workflow for more comprehensive results. Would you like to try it?

| Command | Workflow | File |
|---|---|---|
| `token info`, `token price-info`, `token report`, `token holders`, `token cluster-overview`, `token top-trader` | Token Research | `~/.onchainos/workflows/token-research.md` |
| `token hot-tokens` | Daily Brief | `~/.onchainos/workflows/daily-brief.md` |
| `token advanced-info` | New Token Screening | `~/.onchainos/workflows/new-token-screening.md` |
| `token price-info` | Portfolio Check | `~/.onchainos/workflows/portfolio-check.md` |
| `market prices`, `market kline` | Daily Brief | `~/.onchainos/workflows/daily-brief.md` |
| `market portfolio-overview`, `market portfolio-recent-pnl` | Wallet Analysis | `~/.onchainos/workflows/wallet-analysis.md` |
| `market portfolio-overview`, `market portfolio-token-pnl` | Portfolio Check | `~/.onchainos/workflows/portfolio-check.md` |
| `signal list` | Smart Money Signals; Daily Brief | `~/.onchainos/workflows/smart-money-signals.md`; `~/.onchainos/workflows/daily-brief.md` |
| `signal list --token-address` | Token Research | `~/.onchainos/workflows/token-research.md` |
| `tracker activities` | Wallet Analysis; Wallet Monitor | `~/.onchainos/workflows/wallet-analysis.md`; `~/.onchainos/workflows/wallet-monitor.md` |
| `memepump tokens` | New Token Screening | `~/.onchainos/workflows/new-token-screening.md` |
| `memepump tokens --stage MIGRATED` | Daily Brief | `~/.onchainos/workflows/daily-brief.md` |
| `memepump token-dev-info`, `memepump token-bundle-info` | Smart Money Signals | `~/.onchainos/workflows/smart-money-signals.md` |
| `memepump token-details`, `memepump token-dev-info`, `memepump token-bundle-info` | Token Research | `~/.onchainos/workflows/token-research.md` |
| `ws start`, `ws poll`, `ws stop` | Wallet Monitor (WebSocket) | `~/.onchainos/workflows/wallet-monitor-ws.md` |

## Next Actions

| After | Suggest |
|---|---|
| `token search` | `token price-info`, `token holders` |
| `token info` | `token price-info`, `token holders` |
| `token price-info` | `token holders`, `market kline`, `swap execute` |
| `token holders` | `token advanced-info`, `token top-trader` |
| `token liquidity` | `token holders`, `token advanced-info` |
| `token hot-tokens` | `token price-info`, `token liquidity`, `token advanced-info` |
| `token advanced-info` | `token holders`, `token top-trader`, `token cluster-overview` |
| `token top-trader` | `token advanced-info`, `token trades` |
| `token trades` | `token top-trader`, `token advanced-info` |
| `token cluster-supported-chains` | `token cluster-overview` |
| `token cluster-overview` | `token cluster-top-holders`, `token cluster-list`, `token advanced-info` |
| `token cluster-top-holders` | `token cluster-list`, `token holders` |
| `token cluster-list` | `token top-trader`, `token advanced-info` |
| `market price` | `market kline`, `token price-info`, `swap execute` |
| `market kline` | `token price-info`, `token holders`, `swap execute` |
| `market prices` | `market kline`, `market price` |
| `market index` | `market price`, `market kline` |
| `market portfolio-supported-chains` | `market portfolio-overview` |
| `market portfolio-overview` | `market portfolio-dex-history`, `market portfolio-recent-pnl`, `swap execute` |
| `market portfolio-dex-history` | `market portfolio-token-pnl`, `market kline` |
| `market portfolio-recent-pnl` | `market portfolio-token-pnl`, `token price-info` |
| `market portfolio-token-pnl` | `market portfolio-dex-history`, `market kline` |
| `signal chains` | `signal list` |
| `tracker activities` | `market price`, `token price-info`, `swap execute` |
| `signal list` | `tracker activities`, `market kline`, `token price-info`, `swap execute` |
| `leaderboard list` | `market portfolio-overview`, `portfolio all-balances`, `tracker activities --tracker-type multi_address` |
| `news-latest`, `news-by-symbol`, `news-search` | `news-detail`; `sentiment-symbol`; `market price` |
| `news-detail` | `news-by-symbol`; `sentiment-symbol` |
| `news-platforms` | `news-search`; `news-by-symbol --platform` |
| `sentiment-ranking` | `sentiment-symbol`; `news-by-symbol`; `token hot-tokens` |
| `sentiment-symbol` | `news-by-symbol`; `vibe-top-kols` when a contract address is known; `market kline` |
| `vibe-timeline` | `vibe-top-kols`, `token advanced-info`, `market kline` |
| `vibe-top-kols` | `vibe-timeline`, `token holders`, `swap execute` |
| `memepump chains` | `memepump tokens` |
| `memepump tokens` | `memepump token-details`, `memepump token-dev-info` |
| `memepump token-details` | `memepump token-dev-info`, `memepump similar-tokens`, `memepump token-bundle-info` |
| `memepump token-dev-info` | `memepump token-bundle-info`, `market kline` |
| `memepump similar-tokens` | `memepump token-details` |
| `memepump token-bundle-info` | `memepump aped-wallet` |
| `memepump aped-wallet` | `token advanced-info`, `market kline`, `swap execute` |
