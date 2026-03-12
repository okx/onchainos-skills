# OKX DEX Leaderboard — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for all 2 leaderboard commands.

## 1. onchainos leaderboard supported-chains

Get chains supported by the smart money leaderboard. No parameters required.

```bash
onchainos leaderboard supported-chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Unique chain identifier (e.g., `"1"` for Ethereum, `"501"` for Solana) |
| `chainName` | String | Human-readable chain name |
| `chainLogo` | String | Chain logo image URL |

> Call this first to confirm chain support before calling `onchainos leaderboard list`.

**Example**:

```bash
onchainos leaderboard supported-chains
```

---

## 2. onchainos leaderboard list

Get the smart money leaderboard — top traders ranked by PnL, win rate, transaction count, volume, or ROI. Returns at most **20 entries per request**.

```bash
onchainos leaderboard list --chain <chain> --time-frame <tf> --sort-by <sort> [options]
```

### Request Parameters

| Param | Required | Type | Description |
|---|---|---|---|
| `--chain` | Yes | String | Chain name (e.g., `solana`, `ethereum`) or chainIndex (e.g., `501`). Single chain only. |
| `--time-frame` | Yes | String | Time frame: `1`=1D, `2`=3D, `3`=7D, `4`=1M, `5`=3M |
| `--sort-by` | Yes | String | Sort by: `1`=PnL, `2`=Win Rate, `3`=Tx number, `4`=Volume, `5`=ROI (profit rate) |
| `--wallet-type` | No | String | Single wallet type: `sniper`, `dev`, `fresh`, `pump`, `smartMoney`, `influencer`. If omitted, all types are returned. |
| `--min-realized-pnl-usd` | No | String | Minimum realized PnL in USD |
| `--max-realized-pnl-usd` | No | String | Maximum realized PnL in USD |
| `--min-win-rate-percent` | No | String | Minimum win rate percentage |
| `--max-win-rate-percent` | No | String | Maximum win rate percentage |
| `--min-txs` | No | String | Minimum number of transactions |
| `--max-txs` | No | String | Maximum number of transactions |
| `--min-tx-volume` | No | String | Minimum transaction volume in USD |
| `--max-tx-volume` | No | String | Maximum transaction volume in USD |

### Time Frame Values

| Value | Period |
|---|---|
| `1` | 1 Day |
| `2` | 3 Days |
| `3` | 7 Days |
| `4` | 1 Month |
| `5` | 3 Months |

### Sort By Values

| Value | Sort Field |
|---|---|
| `1` | PnL (realized profit and loss) |
| `2` | Win Rate |
| `3` | Tx number (transaction count) |
| `4` | Volume (transaction volume USD) |
| `5` | ROI (profit rate) |

### Wallet Type Values

| Value | Description |
|---|---|
| `sniper` | Sniper wallets |
| `dev` | Developer wallets |
| `fresh` | Newly-created / fresh wallets |
| `pump` | Pump wallets |
| `smartMoney` | Smart money wallets |
| `influencer` | KOL / influencer wallets |

### Examples

```bash
# Top traders on Solana by PnL over last 7 days
onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1

# Top smart money on Ethereum by win rate over last 1 month
onchainos leaderboard list --chain ethereum --time-frame 4 --sort-by 2 --wallet-type smartMoney

# Top snipers on BSC by volume, last 1 day, min 10 txs
onchainos leaderboard list --chain bsc --time-frame 1 --sort-by 4 --wallet-type sniper --min-txs 10

# Filter by realized PnL range
onchainos leaderboard list --chain solana --time-frame 3 --sort-by 1 \
  --min-realized-pnl-usd 10000 --max-realized-pnl-usd 1000000

# Top influencers on Arbitrum by ROI, last 3 months, min 50% win rate
onchainos leaderboard list --chain arbitrum --time-frame 5 --sort-by 5 \
  --wallet-type influencer --min-win-rate-percent 50
```
