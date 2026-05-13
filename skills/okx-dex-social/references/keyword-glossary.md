# Keyword Glossary — okx-dex-social

| Chinese | English | Maps To |
|---|---|---|
| 最新新闻 / 最新加密新闻 / 头条 | latest news, headlines, news feed | `onchainos social news-latest` |
| BTC新闻 / ETH新闻 / 某币种新闻 | news for symbol, news on `<coin>` | `onchainos social news-by-symbol --token-symbols <symbols>` |
| 搜索新闻 / 搜新闻 / 全文搜索 | search news, news search, find articles about | `onchainos social news-search --keyword <kw>` |
| 文章详情 / 全文 / 看这篇 | article detail, full body, open article | `onchainos social news-detail --article-id <id>` |
| 新闻平台 / 新闻来源列表 | news platforms, news sources, source list | `onchainos social news-platforms` |
| 情绪 / 市场情绪 | sentiment, market mood | `onchainos social sentiment-symbol` (single coin) / `sentiment-ranking` (top coins) |
| 情绪排行 / 热度榜 / 讨论度榜 | sentiment ranking, mentions ranking, top coins by chatter | `onchainos social sentiment-ranking` |
| 看多 / 看涨 / 多空比 | bullish / bearish / bull-bear ratio | `onchainos social sentiment-symbol` (`bullishRatio`, `bearishRatio`) |
| 趋势 / 走势 / 时间序列 | trend, trendline, time-bucketed | `onchainos social sentiment-symbol --trend-points <N>` (sentiment) or `social vibe-timeline` (vibe) |
| 热度 / vibe / 热度评分 | vibe, hotness score | `onchainos social vibe-timeline` |
| KOL榜 / 头部KOL / 谁在讨论 | top KOLs, who's talking, KOL leaderboard | `onchainos social vibe-top-kols` |
| 首发提及 / 首次提到 | first mention, first to tweet | `onchainos social vibe-top-kols` (`firstMention` field) |
| 重要新闻 / 高重要度 | important / high-importance news | `--importance 1` on news commands |
| 看涨新闻 / 看跌新闻 | bullish / bearish news | `--sentiment 1` (bullish) / `--sentiment 2` (bearish) on `news-by-symbol` / `news-search` |
| 24小时 / 三天 / 一周 / 一个月 | 24h / 72h / 7d / 30d | `--time-frame 1` / `2` / `3` / `4` on sentiment + vibe |

## Period Code Reference

| User phrasing | `--time-frame` |
|---|---|
| today, 24h, 24 小时, 1D | `1` |
| 3 days, 三天, 72h, 3D | `2` |
| this week, 7 days, 一周, 1W | `3` |
| this month, 30 days, 一个月, 1M | `4` |

## Sentiment / Importance / Sort Code Reference

| Field | Code | Meaning |
|---|---|---|
| `--sentiment` | `1` | bullish / 看多 |
| `--sentiment` | `2` | bearish / 看空 |
| `--sentiment` | `3` | neutral / 中性 |
| `--importance` | `1` | high / 高 |
| `--importance` | `2` | medium / 中 |
| `--importance` | `3` | low / 低 |
| `news-by-symbol` / `news-search` `--sort-by` | `1` | latest |
| `news-by-symbol` / `news-search` `--sort-by` | `2` | hot |
| `sentiment-ranking` `--sort-by` | `1` | hot (only value currently supported) |
| `vibe-top-kols` `--sort-by` | `1` | engagement |
| `vibe-top-kols` `--sort-by` | `2` | mentions |
| `vibe-top-kols` `--sort-by` | `3` | impressions |

> **Symbol vs contract address**: news / sentiment commands take coin **symbols** (`BTC`, `ETH`). Vibe commands take a **contract address + chain** — if the user only gave a symbol, resolve to a contract address via `okx-dex-token` `onchainos token search` first; never guess the address.
