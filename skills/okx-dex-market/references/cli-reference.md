# OKX DEX Market — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for all 18 market commands.

## 1. onchainos market price

Get single token price.

```bash
onchainos market price <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `ethereum` | Chain name (e.g., `ethereum`, `solana`, `xlayer`) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `time` | String | Timestamp (Unix milliseconds) |
| `price` | String | Current price in USD |

## 2. onchainos market prices

Batch price query for multiple tokens.

```bash
onchainos market prices <tokens> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<tokens>` | Yes | - | Comma-separated tokens. Format: `chainIndex:address` pairs (e.g., `"1:0xeee...,501:So111..."`) or plain addresses with `--chain` |
| `--chain` | No | `ethereum` | Default chain for tokens without explicit chainIndex prefix |

**Return fields** (per token):

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `time` | String | Timestamp (Unix milliseconds) |
| `price` | String | Current price in USD |

## 3. onchainos market kline

Get K-line / candlestick data.

```bash
onchainos market kline <address> [--bar <bar>] [--limit <n>] [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address |
| `--bar` | No | `1H` | Bar size: `1s`, `1m`, `5m`, `15m`, `30m`, `1H`, `4H`, `1D`, `1W`, etc. |
| `--limit` | No | `100` | Number of data points (max 299) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**: Each data point is an array with the following elements:

| Index | Field | Type | Description |
|---|---|---|---|
| 0 | `ts` | String | Timestamp (Unix milliseconds) |
| 1 | `open` | String | Opening price |
| 2 | `high` | String | Highest price |
| 3 | `low` | String | Lowest price |
| 4 | `close` | String | Closing price |
| 5 | `vol` | String | Trading volume (token units) |
| 6 | `volUsd` | String | Trading volume (USD) |
| 7 | `confirm` | String | `"0"` = uncompleted candle, `"1"` = completed candle |

## 4. onchainos market index

Get index price (aggregated from multiple sources).

```bash
onchainos market index <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (empty string `""` for native token) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `price` | String | Index price (aggregated from multiple sources) |
| `time` | String | Timestamp (Unix milliseconds) |

## 5. onchainos market signal-chains

Get supported chains for market signals. No parameters required.

```bash
onchainos market signal-chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `chainName` | String | Human-readable chain name (e.g., `"Ethereum"`, `"Solana"`) |
| `chainLogo` | String | Chain logo image URL |

> Call this first when signal data is needed — confirm chain support before calling `onchainos market signal-list`.

## 6. onchainos market signal-list

Get latest buy-direction token signals sorted descending by time.

```bash
onchainos market signal-list <chain> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<chain>` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `base`) (positional) |
| `--wallet-type` | No | all types | Wallet classification, comma-separated: `1`=Smart Money, `2`=KOL/Influencer, `3`=Whale (e.g., `"1,2"`) |
| `--min-amount-usd` | No | - | Minimum transaction amount in USD |
| `--max-amount-usd` | No | - | Maximum transaction amount in USD |
| `--min-address-count` | No | - | Minimum triggering wallet address count |
| `--max-address-count` | No | - | Maximum triggering wallet address count |
| `--token-address` | No | - | Token contract address (filter signals for a specific token) |
| `--min-market-cap-usd` | No | - | Minimum token market cap in USD |
| `--max-market-cap-usd` | No | - | Maximum token market cap in USD |
| `--min-liquidity-usd` | No | - | Minimum token liquidity in USD |
| `--max-liquidity-usd` | No | - | Maximum token liquidity in USD |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `timestamp` | String | Signal timestamp (Unix milliseconds) |
| `chainIndex` | String | Chain identifier |
| `price` | String | Token price at signal time (USD) |
| `walletType` | String | Wallet classification: `SMART_MONEY`, `WHALE`, or `INFLUENCER` |
| `triggerWalletCount` | String | Number of wallets that triggered this signal |
| `triggerWalletAddress` | String | Comma-separated wallet addresses that triggered the signal |
| `amountUsd` | String | Total transaction amount in USD |
| `soldRatioPercent` | String | Percentage of tokens sold (lower = still holding) |
| `token.tokenAddress` | String | Token contract address |
| `token.symbol` | String | Token symbol |
| `token.name` | String | Token name |
| `token.logo` | String | Token logo URL |
| `token.marketCapUsd` | String | Token market cap in USD |
| `token.holders` | String | Number of token holders |
| `token.top10HolderPercent` | String | Percentage of supply held by top 10 holders |

## 7. onchainos market memepump-chains

Get supported chains and protocols for meme pump. No parameters required.

```bash
onchainos market memepump-chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data[].chainIndex` | String | Chain identifier (e.g., `"501"` for Solana, `"56"` for BSC) |
| `data[].chainName` | String | Human-readable chain name |
| `data[].protocolList[].protocolId` | String | Protocol unique ID |
| `data[].protocolList[].protocolName` | String | Protocol display name (e.g., `pumpfun`, `fourmeme`) |

> Currently supports: Solana (501), BSC (56), X Layer (196), TRON (195).

## 8. onchainos market memepump-tokens

List meme pump tokens with advanced filtering. Returns up to 30 tokens per request.

```bash
onchainos market memepump-tokens <chain> --stage <stage> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<chain>` | Yes | - | Chain name (e.g., `solana`, `bsc`) (positional) |
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

## 9. onchainos market memepump-token-details

Get detailed information for a specific meme pump token.

```bash
onchainos market memepump-token-details <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
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

## 10. onchainos market memepump-token-dev-info

Get developer analysis including rug pull history, migration stats, and holding info.

```bash
onchainos market memepump-token-dev-info <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
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

## 11. onchainos market memepump-similar-tokens

Find similar tokens created by the same developer. Returns at most 2 results.

```bash
onchainos market memepump-similar-tokens <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
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

## 12. onchainos market memepump-token-bundle-info

Get bundle/sniper analysis for a token.

```bash
onchainos market memepump-token-bundle-info <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
| `--chain` | No | `solana` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `bundlerAthPercent` | String | Bundler all-time-high percentage (0-100) |
| `totalBundlers` | String | Total number of bundlers |
| `bundledValueNative` | String | Total bundled value in native token |
| `bundledTokenAmount` | String | Total bundled token amount |

## 13. onchainos market memepump-aped-wallet

Get the aped (same-car) wallet list for a token.

```bash
onchainos market memepump-aped-wallet <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `<address>` | Yes | - | Token contract address (positional) |
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

## 14. onchainos market portfolio-supported-chains

Get the list of chains supported by the portfolio PnL endpoints.

```bash
onchainos market portfolio-supported-chains
```

No parameters required.

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Unique identifier of the chain |
| `chainName` | String | Chain name |
| `chainLogo` | String | Chain logo URL |

## 15. onchainos market portfolio-overview

Get wallet portfolio PnL overview: realized/unrealized PnL, win rate, Top 3 tokens, buy/sell stats.

```bash
onchainos market portfolio-overview --address <address> --chain <chain> --time-frame <n>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID (e.g. `ethereum`, `solana`) |
| `--time-frame` | Yes | - | Statistical range: `1`=1D, `2`=3D, `3`=7D, `4`=1M, `5`=3M |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `realizedPnlUsd` | String | Realized PnL (USD) |
| `top3PnlTokenSumUsd` | String | Total PnL of Top 3 tokens (USD) |
| `top3PnlTokenPercent` | String | Top 3 tokens PnL percentage |
| `topPnlTokenList` | Array | Top 3 PnL token list |
| `topPnlTokenList[].tokenContractAddress` | String | Token contract address |
| `topPnlTokenList[].tokenSymbol` | String | Token symbol |
| `topPnlTokenList[].tokenPnLUsd` | String | Token PnL (USD) |
| `topPnlTokenList[].tokenPnLPercent` | String | Token PnL percentage |
| `winRate` | String | Win rate |
| `tokenCountByPnlPercent` | Object | Token count grouped by PnL range |
| `tokenCountByPnlPercent.over500Percent` | String | Tokens with PnL > 500% |
| `tokenCountByPnlPercent.zeroTo500Percent` | String | Tokens with PnL 0%–500% |
| `tokenCountByPnlPercent.zeroToMinus50Percent` | String | Tokens with PnL -50%–0% |
| `tokenCountByPnlPercent.overMinus50Percent` | String | Tokens with PnL < -50% |
| `buyTxCount` | String | Number of buy transactions |
| `buyTxVolume` | String | Buy transaction volume (USD) |
| `sellTxCount` | String | Number of sell transactions |
| `sellTxVolume` | String | Sell transaction volume (USD) |
| `avgBuyValueUsd` | String | Average buy value (USD) |
| `preferredMarketCap` | String | Preferred market cap range |
| `buysByMarketCap` | Array | Buy counts grouped by market cap range |
| `buysByMarketCap[].marketCapRange` | String | Market cap range label |
| `buysByMarketCap[].buyCount` | String | Buy count in that range |

## 16. onchainos market portfolio-dex-history

Get DEX transaction history for a wallet in reverse chronological order (up to 1000 records, 100 per request).

```bash
onchainos market portfolio-dex-history --address <address> --chain <chain> --begin <ms> --end <ms> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--begin` | Yes | - | Start timestamp (Unix milliseconds) |
| `--end` | Yes | - | End timestamp (Unix milliseconds) |
| `--limit` | No | `20` | Records per page (max 100) |
| `--cursor` | No | - | Pagination cursor from previous response |
| `--token` | No | - | Filter by token contract address |
| `--tx-type` | No | - | Transaction type: `1`=BUY, `2`=SELL, `3`=Transfer In, `4`=Transfer Out (comma-separated) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `transactionList` | Array | List of transactions |
| `transactionList[].type` | String | Transaction type (1=BUY, 2=SELL, 3=Transfer In, 4=Transfer Out) |
| `transactionList[].chainIndex` | String | Chain identifier |
| `transactionList[].tokenContractAddress` | String | Token contract address |
| `transactionList[].tokenSymbol` | String | Token symbol |
| `transactionList[].valueUsd` | String | Transaction value (USD) |
| `transactionList[].amount` | String | Token amount |
| `transactionList[].price` | String | Transaction price |
| `transactionList[].marketCap` | String | Market cap at time of tx |
| `transactionList[].pnlUsd` | String | PnL (USD) |
| `transactionList[].time` | String | Transaction timestamp (milliseconds) |
| `cursor` | String | Pagination cursor for next page |

## 17. onchainos market portfolio-recent-pnl

Get recent PnL list for a wallet in reverse chronological order (up to 1000 records, 100 per request).

```bash
onchainos market portfolio-recent-pnl --address <address> --chain <chain> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--limit` | No | `20` | Records per page (max 100) |
| `--cursor` | No | - | Pagination cursor from previous response |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `pnlList` | Array | PnL record list |
| `pnlList[].chainIndex` | String | Chain identifier |
| `pnlList[].tokenContractAddress` | String | Token contract address |
| `pnlList[].tokenSymbol` | String | Token symbol |
| `pnlList[].lastActiveTimestamp` | String | Last active timestamp (milliseconds) |
| `pnlList[].unrealizedPnlUsd` | String | Unrealized PnL (USD); `SELL_ALL` if all sold |
| `pnlList[].unrealizedPnlPercent` | String | Unrealized PnL percentage |
| `pnlList[].realizedPnlUsd` | String | Realized PnL (USD) |
| `pnlList[].realizedPnlPercent` | String | Realized PnL percentage |
| `pnlList[].totalPnlUsd` | String | Total PnL (USD) |
| `pnlList[].totalPnlPercent` | String | Total PnL percentage |
| `pnlList[].tokenBalanceUsd` | String | Token balance value (USD) |
| `pnlList[].tokenBalanceAmount` | String | Token balance amount |
| `pnlList[].tokenPositionPercent` | String | Token position percentage |
| `pnlList[].tokenPositionDuration.holdingTimestamp` | String | Holding start timestamp (milliseconds) |
| `pnlList[].tokenPositionDuration.sellOffTimestamp` | String | Sell-off timestamp; empty if still holding |
| `pnlList[].buyTxCount` | String | Number of buy transactions |
| `pnlList[].buyTxVolume` | String | Buy transaction volume |
| `pnlList[].buyAvgPrice` | String | Average buy price |
| `pnlList[].sellTxCount` | String | Number of sell transactions |
| `pnlList[].sellTxVolume` | String | Sell transaction volume |
| `pnlList[].sellAvgPrice` | String | Average sell price |

## 18. onchainos market portfolio-token-pnl

Get the latest PnL snapshot for a specific token in a wallet.

```bash
onchainos market portfolio-token-pnl --address <address> --chain <chain> --token <token>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Wallet address |
| `--chain` | Yes | - | Chain name or ID |
| `--token` | Yes | - | Token contract address |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `totalPnlUsd` | String | Total PnL (USD) |
| `totalPnlPercent` | String | Total PnL percentage |
| `unrealizedPnlUsd` | String | Unrealized PnL (USD) |
| `unrealizedPnlPercent` | String | Unrealized PnL percentage |
| `realizedPnlUsd` | String | Realized PnL (USD) |
| `realizedPnlPercent` | String | Realized PnL percentage |
| `isPnlSupported` | Boolean | Whether PnL calculation is supported for this token |

## Input / Output Examples

**User says:** "Check the current price of OKB on XLayer"

```bash
onchainos market price 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer
# -> Display: OKB current price $XX.XX
```

**User says:** "Show me hourly candles for USDC on XLayer"

```bash
onchainos market kline 0x74b7f16337b8972027f6196a17a631ac6de26d22 --chain xlayer --bar 1H
# -> Display candlestick data (open/high/low/close/volume)
```

**User says:** "What are smart money wallets buying on Solana?"

```bash
onchainos market signal-list solana --wallet-type 1
# -> Display smart money buy signals with token info
```

**User says:** "Show me whale buys above $10k on Ethereum"

```bash
onchainos market signal-list ethereum --wallet-type 3 --min-amount-usd 10000
# -> Display whale-only signals, min $10k
```

**User says:** "Show me new meme tokens on Solana"

```bash
onchainos market memepump-tokens solana --stage NEW
# -> Display list of new meme pump tokens with market data and audit tags
```

**User says:** "Is this meme token safe? Check the developer"

```bash
onchainos market memepump-token-dev-info <address> --chain solana
# -> Display dev rug pull count, migration count, golden gems, dev holding info
```

**User says:** "Check if this token has bundler activity"

```bash
onchainos market memepump-token-bundle-info <address> --chain solana
# -> Display bundler count, bundled value, bundled token amount
```
