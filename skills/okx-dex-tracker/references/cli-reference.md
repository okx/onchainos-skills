# OKX DEX Address Tracker — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for the tracker command.

## 1. onchainos tracker trades

Get on-chain trading activity of tracked addresses. Supports filtering by tracker type (KOL, smart money, or multi-address), trade direction, chain, volume, market cap, liquidity, and holder count.

```bash
onchainos tracker trades [options]
```

### Request Parameters

| Param | Required | Default | Description |
|---|---|---|---|
| `--tracker-type` | No | `kol` | Tracker type. Accepts names, abbreviations, or numbers: `smart_money` / `sm` / `1`, `kol` / `2`, `multi_address` / `custom` / `3` |
| `--wallet-address` | Conditional | - | Wallet address(es) to track. **Required** when `--tracker-type` is `multi_address` / `custom` / `3`. Comma-separated, max 20 addresses. |
| `--trade-type` | No | `all` | Trade direction. Accepts names or numbers: `all` / `0`, `buy` / `1`, `sell` / `2` |
| `--chain` | No | all chains | Chain filter: `ethereum` / `eth`, `solana` / `sol`, `bsc` / `bnb`, `base`, `xlayer`, numeric chainIndex, or `all` |
| `--min-volume` | No | - | Minimum trade amount in USD |
| `--max-volume` | No | - | Maximum trade amount in USD |
| `--min-holders` | No | - | Minimum holder count of the traded token |
| `--min-market-cap` | No | - | Minimum token market cap in USD |
| `--max-market-cap` | No | - | Maximum token market cap in USD |
| `--min-liquidity` | No | - | Minimum token liquidity in USD |
| `--max-liquidity` | No | - | Maximum token liquidity in USD |

### Tracker Type Values

| CLI Value | API Value | Description |
|---|---|---|
| `kol` / `2` | `2` | Platform top 100 KOL addresses (default) |
| `smart_money` / `sm` / `1` | `1` | Platform smart money addresses |
| `multi_address` / `custom` / `3` | `3` | Ad-hoc multi-address tracking (requires `--wallet-address`) |

### Trade Type Values

| CLI Value | API Value | Description |
|---|---|---|
| `all` / `0` | `0` | All trades — buys and sells (default) |
| `buy` / `1` | `1` | Buy transactions only |
| `sell` / `2` | `2` | Sell transactions only |

### Return Fields

| Field | Type | Description |
|---|---|---|
| `trades` | Array | List of trade activity entries |
| `trades[].traderAddress` | String | Trader wallet address |
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

# Same as above using numeric aliases
onchainos tracker trades --tracker-type 1 --trade-type 1 --chain solana

# KOL sells on Ethereum, min $50k volume
onchainos tracker trades --tracker-type kol --trade-type sell --chain ethereum --min-volume 50000

# Track specific wallet addresses (ad-hoc multi-address)
onchainos tracker trades --tracker-type multi_address --wallet-address "0xabc...,0xdef..."

# Filter by market cap and liquidity
onchainos tracker trades \
  --min-market-cap 1000000 \
  --max-market-cap 500000000 \
  --min-liquidity 100000

# Smart money buys with min 1000 holders and min $100k market cap
onchainos tracker trades \
  --tracker-type sm \
  --trade-type buy \
  --min-holders 1000 \
  --min-market-cap 100000
```
