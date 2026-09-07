# Chinese-Specific Glossary — okx-dex-market (Market)

Use this reference only for non-literal Chinese market terminology. Apply the Market capability's routing rules after normalization.

| Chinese-specific phrase | Normalize or map to |
|---|---|
| K线 | K-line / candlestick data → `market kline` |
| 清仓 | A fully exited position → `portfolio-recent-pnl` where `unrealizedPnlUsd = "SELL_ALL"` |
| 画像 / 钱包画像 | Wallet profile → `portfolio-overview` |
