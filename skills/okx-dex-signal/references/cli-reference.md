# Onchain OS DEX Signal — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for the 2 signal commands.

## 1. onchainos signal chains

Get supported chains for market signals. No parameters required.

```bash
onchainos signal chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `chainName` | String | Human-readable chain name (e.g., `"Ethereum"`, `"Solana"`) |
| `chainLogo` | String | Chain logo image URL |

> Call this first when signal data is needed — confirm chain support before calling `onchainos signal list`.

## 2. onchainos signal list

Get latest buy-direction token signals sorted descending by time.

```bash
onchainos signal list --chain <chain> [options]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chain` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `base`) |
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

## Input / Output Examples

**User says:** "What are smart money wallets buying on Solana?"

```bash
onchainos signal chains   # confirm Solana is supported
onchainos signal list --chain solana --wallet-type 1
# -> Display smart money buy signals with token info
```

**User says:** "Show me whale buys above $10k on Ethereum"

```bash
onchainos signal list --chain ethereum --wallet-type 3 --min-amount-usd 10000
# -> Display whale-only signals, min $10k
```

**User says:** "Filter signals to only show whale buys above $10k"

```bash
onchainos signal list --chain ethereum --wallet-type 3 --min-amount-usd 10000
# -> whale-only signals on Ethereum, min $10k
```
