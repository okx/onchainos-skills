# OKX DEX Address Tracker — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for the tracker command.

## 1. onchainos tracker trades

Get on-chain trading activity of tracked addresses. Supports filtering by tracker type (KOL, smart money, or custom group), trade direction, chain, volume, market cap, liquidity, and holder count. Returns at most **50 results per request** (default 20).

```bash
onchainos tracker trades [options]
```

### Request Parameters

| Param | Required | Default | Description |
|---|---|---|---|
| `--tracker-type` | No | `kol` | Tracker type: `kol` (platform top 100 KOL addresses), `smart_money` (platform smart money addresses), `group` (user-defined custom group — requires `--group-name`) |
| `--group-name` | Conditional | - | Custom group name. **Required** when `--tracker-type group` |
| `--trade-type` | No | `all` | Trade direction: `all`, `buy`, `sell` |
| `--chain` | No | all chains | Chain filter: `ethereum`, `solana`, `bsc`, `base`, `xlayer`, or `all` |
| `--min-volume` | No | - | Minimum trade amount in USD |
| `--max-volume` | No | - | Maximum trade amount in USD |
| `--min-holders` | No | - | Minimum holder count of the traded token |
| `--min-market-cap` | No | - | Minimum token market cap in USD |
| `--max-market-cap` | No | - | Maximum token market cap in USD |
| `--min-liquidity` | No | - | Minimum token liquidity in USD |
| `--max-liquidity` | No | - | Maximum token liquidity in USD |
| `--limit` | No | `20` | Number of results to return (max 50) |

### Tracker Type Values

| Value | Description |
|---|---|
| `kol` | Platform top 100 KOL addresses (default) |
| `smart_money` | Platform smart money addresses |
| `group` | User-defined custom group (must also pass `--group-name`) |

### Trade Type Values

| Value | Description |
|---|---|
| `all` | All trades — buys and sells (default) |
| `buy` | Buy transactions only |
| `sell` | Sell transactions only |

### Return Fields

| Field | Type | Description |
|---|---|---|
| `trades` | Array | List of trade activity entries |
| `trades[].traderAddress` | String | Trader wallet address |
| `trades[].traderRemark` | String | Address remark / label (if available) |
| `trades[].quoteTokenSymbol` | String | Quote token symbol (e.g., `SOL`, `USDC`) |
| `trades[].quoteTokenAmount` | String | Quote token trade amount |
| `trades[].baseTokenSymbol` | String | Base token symbol (the traded token) |
| `trades[].baseTokenContractAddress` | String | Base token contract address |
| `trades[].baseTokenChainIndex` | String | Chain identifier of the traded token |
| `trades[].tradePrice` | String | Trade price in USD |
| `trades[].marketCap` | String | Token market cap at trade time (USD) |
| `trades[].realizedPnlUsd` | String | Realized profit/loss of the trader (USD) |
| `trades[].tradeType` | String | Trade direction: `buy` or `sell` |
| `trades[].tradeTime` | String | Trade timestamp (Unix milliseconds) |

### Examples

```bash
# Get latest KOL activity across all chains (default)
onchainos tracker trades

# Smart money buys on Solana
onchainos tracker trades --tracker-type smart_money --trade-type buy --chain solana

# KOL sells on Ethereum, min $50k volume
onchainos tracker trades --tracker-type kol --trade-type sell --chain ethereum --min-volume 50000

# Custom group trades
onchainos tracker trades --tracker-type group --group-name "my-whales"

# Filter by market cap and liquidity
onchainos tracker trades \
  --min-market-cap 1000000 \
  --max-market-cap 500000000 \
  --min-liquidity 100000

# Get 50 results
onchainos tracker trades --limit 50

# Smart money buys with min 1000 holders and min $100k market cap
onchainos tracker trades \
  --tracker-type smart_money \
  --trade-type buy \
  --min-holders 1000 \
  --min-market-cap 100000
```
