# Cross-Skill Workflows

This file is for **orchestrator agents**. It documents common multi-step workflows spanning the 4 core skills: `okx-dex-market`, `okx-dex-signal`, `okx-dex-token`, `okx-dex-trenches`.

For dynamic orchestration, also read each skill's `## Data Contract` section.

---

## Workflow 1: Research Token Before Buying

> "Tell me about BONK, show me the chart, then buy if it looks good"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-token` | `onchainos token search --query BONK --chains solana` | `tokenAddress`, `chain`, `decimal` |
| 2 | `okx-dex-token` | `onchainos token price-info --address <tokenAddress> --chain <chain>` | `liquidity`, `marketCap`, `priceChange24H` |
| 3 | `okx-dex-token` | `onchainos token holders --address <tokenAddress> --chain <chain>` | holder concentration |
| 4 | `okx-dex-market` | `onchainos market kline --address <tokenAddress> --chain <chain>` | price chart |

**Data handoff**: `tokenAddress` + `chain` from step 1 → reused in steps 2–4.

---

## Workflow 2: Hot Token Discovery → Safety Check

> "Show me the hottest tokens and check if any are safe"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-token` | `onchainos token hot-tokens --ranking-type 4 --chain solana` | `tokenAddress`, `chainIndex` |
| 2 | `okx-dex-token` | `onchainos token price-info --address <tokenAddress> --chain solana` | `liquidity`, `marketCap` |
| 3 | `okx-dex-token` | `onchainos token advanced-info --address <tokenAddress> --chain solana` | `riskControlLevel`, `devHoldingPercent` |
| 4 | `okx-dex-token` | `onchainos token cluster-overview --address <tokenAddress> --chain solana` | `clusterConcentration`, `rugPullPercent` |
| 5 | `okx-dex-market` | `onchainos market kline --address <tokenAddress> --chain solana` | price momentum |

**Stop condition**: `riskControlLevel >= 3` in step 3 or `clusterConcentration = High` in step 4 → warn user.

---

## Workflow 3: Signal-Driven Token Research

> "Show me what smart money is buying on Solana"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-signal` | `onchainos signal list --chain solana --wallet-type 1,2,3` | `tokenAddress`, `chainIndex`, `soldRatioPercent` |
| 2 | `okx-dex-token` | `onchainos token price-info --address <tokenAddress> --chain <chain>` | `liquidity`, `marketCap` |
| 3 | `okx-dex-token` | `onchainos token cluster-overview --address <tokenAddress> --chain <chain>` | `clusterConcentration`, `rugPullPercent` |
| 4 | `okx-dex-market` | `onchainos market kline --address <tokenAddress> --chain <chain>` | price chart |

**Data handoff**: `tokenAddress` + `chainIndex` from step 1 → reused in steps 2–4.

---

## Workflow 4: Meme Token Discovery & Due Diligence

> "Show me new meme tokens and check if any look safe"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-trenches` | `onchainos memepump tokens --chain solana --stage NEW` | `tokenAddress`, `chainIndex` |
| 2 | `okx-dex-trenches` | `onchainos memepump token-details --address <tokenAddress> --chain solana` | `bondingPercent`, `bundlersPercent`, `top10HoldingsPercent` |
| 3 | `okx-dex-trenches` | `onchainos memepump token-dev-info --address <tokenAddress> --chain solana` | `rugPullCount`, `migratedCount`, `devHoldingPercent` |
| 4 | `okx-dex-trenches` | `onchainos memepump token-bundle-info --address <tokenAddress> --chain solana` | `totalBundlers`, `bundlerAthPercent` |
| 5 | `okx-dex-market` | `onchainos market kline --address <tokenAddress> --chain solana` | price chart |

**Stop condition**: `rugPullCount > 0` in step 3 or high `totalBundlers` in step 4 → warn user.

---

## Workflow 5: Signal → Meme Deep Dive

> "A whale signal came in — is it a pump.fun token? Check it out"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-signal` | `onchainos signal list --chain <chain> --wallet-type 3` | `tokenAddress`, `chainIndex` |
| 2 | `okx-dex-trenches` | `onchainos memepump token-details --address <tokenAddress> --chain <chain>` | confirms meme token, audit tags |
| 3 | `okx-dex-trenches` | `onchainos memepump token-dev-info --address <tokenAddress> --chain <chain>` | `rugPullCount` |
| 4 | `okx-dex-trenches` | `onchainos memepump token-bundle-info --address <tokenAddress> --chain <chain>` | verifies signal isn't a bundler |
| 5 | `okx-dex-market` | `onchainos market kline --address <tokenAddress> --chain <chain>` | price momentum |

---

## Workflow 6: Leaderboard → Portfolio Drill-In

> "Show me top traders on Solana and check what they hold"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-signal` | `onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1` | `walletAddress` |
| 2 | `okx-dex-market` | `onchainos market portfolio-overview --address <walletAddress> --chain solana --time-frame 3` | `realizedPnl`, `winRate`, top tokens |

---

## Workflow 7: Wallet PnL Analysis

> "How is my wallet performing? Show me my PnL"

| Step | Skill | Command | Produces |
|---|---|---|---|
| 1 | `okx-dex-market` | `onchainos market portfolio-supported-chains` | confirmed supported chains |
| 2 | `okx-dex-market` | `onchainos market portfolio-overview --address <wallet> --chain <chain> --time-frame 3` | `realizedPnl`, `winRate`, top 3 tokens |
| 3 | `okx-dex-market` | `onchainos market portfolio-recent-pnl --address <wallet> --chain <chain>` | per-token PnL list, `tokenAddress` |
| 4 | `okx-dex-market` | `onchainos market portfolio-token-pnl --address <wallet> --chain <chain> --token <tokenAddress>` | realized/unrealized PnL snapshot |

**Data handoff**: `--address` (wallet) reused across all steps; `tokenAddress` from step 3 → `--token` in step 4.

---

## Data Flow Reference

Key fields passed between skills:

| Field | Produced By | Consumed By |
|---|---|---|
| `tokenContractAddress` | `okx-dex-token` (search, hot-tokens), `okx-dex-market` (portfolio-*), `okx-dex-signal` (tracker activities) | pass as `--address` to all downstream token commands |
| `token.tokenAddress` | `okx-dex-signal` (signal list) — nested field | extract via `token.tokenAddress`; pass as `--address` downstream |
| `tokenAddress` | `okx-dex-trenches` (memepump tokens, token-details) | pass as `--address` to all downstream token commands |
| `chainIndex` | any skill that returns token data (returned as numeric string e.g. `"501"`) | all `--chain` params downstream — pass `chainIndex` directly; CLI accepts numeric IDs. Do NOT use `chainName` (capitalized, not accepted by CLI) |
| `decimal` | `okx-dex-token` (search, info) | amount unit conversion for swap |
| `walletAddress` | `okx-dex-signal` (leaderboard), user input | `okx-dex-market` portfolio commands |
| `rugPullCount` | `okx-dex-trenches` (token-dev-info) | stop condition before proceeding |
| `riskControlLevel` | `okx-dex-token` (advanced-info) | stop condition before proceeding |
| `clusterConcentration` | `okx-dex-token` (cluster-overview) | stop condition before proceeding |
| `soldRatioPercent` | `okx-dex-signal` (signal list) | signal strength assessment |
