# Chinese-Specific Glossary — okx-dex-market (Token)

Use this reference only for Chinese crypto slang and platform-specific ranking terms whose command meaning is not obvious from direct translation.

| Chinese-specific phrase | Normalize or map to |
|---|---|
| 代币分排名 | Token-score ranking → `token hot-tokens --ranking-type 4` |
| Xmentioned榜 | X-mention ranking → `token hot-tokens --ranking-type 5` |
| 烧池子 | Burned LP → `token hot-tokens --is-lp-burnt true` |
| 风控 | Advanced risk metadata → `token advanced-info` |
| 貔貅盘 | Honeypot risk → `okx-agentic-wallet` / `onchainos security token-scan` |
| 内盘 / 内盘代币 | Internal launch-platform token → `token advanced-info.isInternal` |
| 开发者跑路 | Developer rug history → `token advanced-info.devRugPullTokenCount` |
| 老鼠仓 | Insider wallet → `--tag-filter 6` on `token top-trader` or `token holders` |
