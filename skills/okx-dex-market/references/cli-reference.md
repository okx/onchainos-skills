# Onchain OS DEX Market — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for all 10 market commands.

## 1. onchainos market price

Get single token price.

```bash
onchainos market price --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
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
onchainos market prices --tokens <tokens> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--tokens` | Yes | - | Comma-separated tokens. Format: `chainIndex:address` pairs (e.g., `"1:0xeee...,501:So111..."`) or plain addresses with `--chain` |
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
onchainos market kline --address <address> [--bar <bar>] [--limit <n>] [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address |
| `--bar` | No | `1H` | Bar size: `1s`, `1m`, `5m`, `15m`, `30m`, `1H`, `4H`, `1D`, `1W`, etc. |
| `--limit` | No | `100` | Number of data points (max 299) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**: Each data point is now a named JSON object (transformed from the API's raw array `[ts,o,h,l,c,vol,volUsd,confirm]`):

| Field | Type | Description |
|---|---|---|
| `ts` | String | Opening time (Unix milliseconds) |
| `o` | String | Open price |
| `h` | String | Highest price |
| `l` | String | Lowest price |
| `c` | String | Close price |
| `vol` | String | Trading volume (base currency unit) |
| `volUsd` | String | Trading volume (USD) |
| `confirm` | String | `"0"` = uncompleted candle, `"1"` = completed candle |

## 4. onchainos market index

Get index price (aggregated from multiple sources).

```bash
onchainos market index --address <address> [--chain <chain>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--address` | Yes | - | Token contract address (empty string `""` for native token) |
| `--chain` | No | `ethereum` | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier |
| `tokenContractAddress` | String | Token contract address |
| `price` | String | Index price (aggregated from multiple sources) |
| `time` | String | Timestamp (Unix milliseconds) |

## 5. onchainos market portfolio-supported-chains

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

## 6. onchainos market portfolio-overview

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

## 7. onchainos market portfolio-dex-history

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

## 8. onchainos market portfolio-recent-pnl

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

## 9. onchainos market portfolio-token-pnl

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
onchainos market price --address 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --chain xlayer
# -> Display: OKB current price $XX.XX
```

**User says:** "Show me hourly candles for USDC on XLayer"

```bash
onchainos market kline --address 0x74b7f16337b8972027f6196a17a631ac6de26d22 --chain xlayer --bar 1H
# -> Display candlestick data (open/high/low/close/volume)
```

**User says:** "How is my Ethereum wallet performing this week?"

```bash
onchainos market portfolio-supported-chains   # confirm Ethereum supported
onchainos market portfolio-overview --address <wallet> --chain ethereum --time-frame 3
# -> Display 7D PnL overview: realized PnL, win rate, top 3 tokens
```

**User says:** "Show my DEX trade history on Ethereum for the last 30 days"

```bash
# compute begin/end timestamps first
onchainos market portfolio-dex-history --address <wallet> --chain ethereum \
  --begin <start_ms> --end <end_ms>
# -> Display paginated DEX transaction list
```

---

## 10. onchainos market address-tracker-activities

Get latest DEX activities for tracked addresses. Supports smart money, KOL, or custom multi-address tracking, with filters for trade type, chain, volume, market cap, liquidity, and holder count.

```bash
onchainos market address-tracker-activities --tracker-type <type> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--tracker-type` | Yes | - | Tracker type: `smart_money` (or `1`) = platform smart money; `kol` (or `2`) = platform Top 100 KOL addresses; `multi_address` (or `3`) = custom addresses |
| `--wallet-address` | Conditional | - | Required when `--tracker-type multi_address`. Comma-separated wallet addresses, max 20 |
| `--trade-type` | No | `0` (all) | Trade direction: `0`=all, `1`=buy, `2`=sell |
| `--chain` | No | all chains | Chain filter (e.g., `ethereum`, `solana`, `bsc`, `base`, `xlayer`) |
| `--min-volume` | No | - | Minimum trade volume (USD) |
| `--max-volume` | No | - | Maximum trade volume (USD) |
| `--min-holders` | No | - | Minimum number of holding addresses |
| `--min-market-cap` | No | - | Minimum market cap (USD) |
| `--max-market-cap` | No | - | Maximum market cap (USD) |
| `--min-liquidity` | No | - | Minimum liquidity (USD) |
| `--max-liquidity` | No | - | Maximum liquidity (USD) |

**Return fields** (inside `trades` array):

| Field | Type | Description |
|---|---|---|
| `txHash` | String | Transaction hash |
| `walletAddress` | String | Wallet address of the transaction |
| `quoteTokenSymbol` | String | Pricing token symbol (mainnet native token) |
| `quoteTokenAmount` | String | Amount of pricing token traded |
| `tokenSymbol` | String | Trading token symbol |
| `tokenContractAddress` | String | Trading token contract address |
| `chainIndex` | String | Chain identifier where the trading token is located |
| `tokenPrice` | String | Trading price of the token (USD) |
| `marketCap` | String | Market cap at the transaction price (USD) |
| `realizedPnlUsd` | String | Realized PnL of the trading token (USD) |
| `tradeType` | String | Trade direction: `1`=buy, `2`=sell |
| `tradeTime` | String | Transaction time (Unix milliseconds) |
| `trackerType` | Array\<String\> | Tracker type tags for this trade; values: `"1"`=smart_money, `"2"`=kol, `"3"`=multi_address. May contain multiple values if the address matches more than one tracker type. |

**Examples**:

```bash
# Latest trades by platform smart money (all chains)
onchainos market address-tracker-activities --tracker-type smart_money

# Latest buys by KOL addresses on Solana
onchainos market address-tracker-activities --tracker-type kol --chain solana --trade-type 1

# Latest trades for custom wallet addresses
onchainos market address-tracker-activities --tracker-type multi_address \
  --wallet-address 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045,0xab5801a7d398351b8be11c439e05c5b3259aec9b

# Smart money buys with volume filter
onchainos market address-tracker-activities --tracker-type smart_money --trade-type 1 --min-volume 10000
```
