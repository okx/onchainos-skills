# OKX DEX Memepump — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for all 7 memepump commands.

## 1. onchainos memepump chains

Get supported chains and protocols for meme pump. No parameters required.

```bash
onchainos memepump chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].chainIndex` | String | Chain identifier (e.g., `"501"` for Solana, `"56"` for BSC) |
| `data[].chainName` | String | Human-readable chain name |
| `data[].protocolList[].protocolId` | String | Protocol unique ID |
| `data[].protocolList[].protocolName` | String | Protocol display name (e.g., `pumpfun`, `fourmeme`) |

> Currently supports: Solana (501), BSC (56), X Layer (196), TRON (195).

## 2. onchainos memepump tokens

List meme pump tokens with advanced filtering. Returns up to 30 tokens per request.

```bash
onchainos memepump tokens --chain <chain> --stage <stage> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chain` | Yes | - | Chain name (e.g., `solana`, `bsc`) |
| `--stage` | Yes | - | Token stage: `NEW`, `MIGRATING`, or `MIGRATED` |
| `--protocol-id` | No | - | Filter by protocol ID (get IDs from `memepump-chains`) |
| `--sort-by` | No | - | Sort field: `marketCap`, `volume1h`, `txCount1h`, `createdTimestamp`, `bondingPercent` |
| `--sort-order` | No | - | Sort direction: `asc` or `desc` |
| `--min-age` | No | - | Minimum token age in minutes |
| `--max-age` | No | - | Maximum token age in minutes |
| `--min-market-cap` | No | - | Minimum market cap in USD |
| `--max-market-cap` | No | - | Maximum market cap in USD |
| `--min-volume` | No | - | Minimum 1h volume in USD |
| `--max-volume` | No | - | Maximum 1h volume in USD |
| `--min-tx-count` | No | - | Minimum 1h transaction count |
| `--max-tx-count` | No | - | Maximum 1h transaction count |

**Return fields**: Array of token objects (same structure as `memepump-token-details` response).

## 3. onchainos memepump token-details

Get detailed information for a specific meme pump token.

```bash
onchainos memepump token-details --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `protocolId` | String | Protocol numeric ID (e.g., `"120596"` for pumpfun) |
| `quoteTokenAddress` | String | Quote token contract address |
| `tokenAddress` | String | Token contract address |
| `symbol` | String | Token symbol |
| `name` | String | Token name |
| `logoUrl` | String | Token logo URL |
| `creatorAddress` | String | Token creator wallet address |
| `createdTimestamp` | String | Creation timestamp (Unix ms) |
| `migratedBeginTimestamp` | String | Migration start timestamp (Unix ms, empty if not migrating) |
| `migratedEndTimestamp` | String | Migration end timestamp (Unix ms, empty if not migrated) |
| `market.marketCapUsd` | String | Market cap in USD |
| `market.volumeUsd1h` | String | 1-hour volume in USD |
| `market.txCount1h` | String | 1-hour transaction count |
| `market.buyTxCount1h` | String | 1-hour buy transaction count |
| `market.sellTxCount1h` | String | 1-hour sell transaction count |
| `bondingPercent` | String | Bonding curve progress (0-100) |
| `tags.top10HoldingsPercent` | String | Top 10 holders percentage (0-100) |
| `tags.devHoldingsPercent` | String | Dev holdings percentage (0-100) |
| `tags.insidersPercent` | String | Insiders percentage (0-100) |
| `tags.bundlersPercent` | String | Bundlers percentage (0-100) |
| `tags.snipersPercent` | String | Snipers percentage (0-100) |
| `tags.freshWalletsPercent` | String | Fresh wallets percentage (0-100) |
| `tags.suspectedPhishingWalletPercent` | String | Phishing wallet percentage (0-100) |
| `tags.totalHolders` | String | Total holder count |
| `social.x` | String | X (Twitter) URL |
| `social.telegram` | String | Telegram URL |
| `social.website` | String | Website URL |
| `social.dexScreenerPaid` | Boolean | Paid on DexScreener |
| `social.communityTakeover` | Boolean | Community takeover flag |
| `social.liveOnPumpFun` | Boolean | Currently live on Pump.fun |
| `bagsFeeClaimed` | Boolean | Bags fee claimed |
| `aped` | String | Same-car wallet count |

## 4. onchainos memepump token-dev-info

Get developer analysis including rug pull history, migration stats, and holding info.

```bash
onchainos memepump token-dev-info --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `devLaunchedInfo.totalTokens` | String | Total tokens created by this dev |
| `devLaunchedInfo.rugPullCount` | String | Number of rug pulls |
| `devLaunchedInfo.migratedCount` | String | Number of successfully migrated tokens |
| `devLaunchedInfo.goldenGemCount` | String | Number of golden gem tokens |
| `devHoldingInfo.devHoldingPercent` | String | Dev holding percentage (0-100) |
| `devHoldingInfo.devAddress` | String | Developer wallet address |
| `devHoldingInfo.fundingAddress` | String | Funding source address |
| `devHoldingInfo.devBalance` | String | Dev's current balance |
| `devHoldingInfo.lastFundedTimestamp` | String | Last funded timestamp (Unix ms) |

> **Note**: `devHoldingInfo` may be `null` if the creator address is unavailable.

## 5. onchainos memepump similar-tokens

Find similar tokens created by the same developer. Returns at most 2 results.

```bash
onchainos memepump similar-tokens --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].tokenAddress` | String | Similar token contract address |
| `data[].tokenSymbol` | String | Token symbol |
| `data[].tokenLogo` | String | Token logo URL |
| `data[].marketCapUsd` | String | Market cap in USD |
| `data[].lastTxTimestamp` | String | Last transaction timestamp (Unix ms) |
| `data[].createdTimestamp` | String | Creation timestamp (Unix ms) |

## 6. onchainos memepump token-bundle-info

Get bundle/sniper analysis for a token.

```bash
onchainos memepump token-bundle-info --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `bundlerAthPercent` | String | Bundler all-time-high percentage (0-100) |
| `totalBundlers` | String | Total number of bundlers |
| `bundledValueNative` | String | Total bundled value in native token |
| `bundledTokenAmount` | String | Total bundled token amount |

## 7. onchainos memepump aped-wallet

Get the aped (same-car) wallet list for a token.

```bash
onchainos memepump aped-wallet --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].walletAddress` | String | Wallet address |
| `data[].walletType` | String | Wallet type label (e.g., Smart Money, KOL, Whale) |
| `data[].holdingUsd` | String | Holding value in USD |
| `data[].holdingPercent` | String | Holding percentage (0-100) |
| `data[].totalPnl` | String | Total PnL in USD |
| `data[].pnlPercent` | String | PnL percentage |

## Input / Output Examples

**User says:** "Show me new meme tokens on Solana"

```bash
onchainos memepump tokens --chain solana --stage NEW
# -> Display list of new meme pump tokens with market data and audit tags
```

**User says:** "Is this meme token safe? Check the developer"

```bash
onchainos memepump token-dev-info --address <address> --chain solana
# -> Display dev rug pull count, migration count, golden gems, dev holding info
```

**User says:** "Check if this token has bundler activity"

```bash
onchainos memepump token-bundle-info --address <address> --chain solana
# -> Display bundler count, bundled value, bundled token amount
```

**User says:** "Who else has bought this meme token?"

```bash
onchainos memepump aped-wallet --address <address> --chain solana
# -> Display aped wallets with wallet type, holding %, and PnL
```
