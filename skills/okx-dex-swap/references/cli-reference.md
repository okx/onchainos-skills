# Onchain OS DEX Swap — CLI Command Reference

Detailed parameter tables, return field schemas, and usage examples for all 5 swap commands.

## 1. onchainos swap chains

Get supported chains for DEX aggregator. No parameters required.

```bash
onchainos swap chains
```

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `chainIndex` | String | Chain identifier (e.g., `"1"`, `"501"`) |
| `chainName` | String | Human-readable chain name |
| `dexTokenApproveAddress` | String | DEX router address for token approvals on this chain |

## 2. onchainos swap liquidity

Get available liquidity sources on a chain.

```bash
onchainos swap liquidity --chain <chain>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--chain` | Yes | - | Chain name (e.g., `ethereum`, `solana`, `xlayer`) |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `id` | String | Liquidity source ID |
| `name` | String | Liquidity source name (e.g., `"Uniswap V3"`, `"CurveNG"`) |
| `logo` | String | Liquidity source logo URL |

## 3. onchainos swap approve

Get ERC-20 approval transaction data.

```bash
onchainos swap approve --token <address> --amount <amount> --chain <chain>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--token` | Yes | - | Token contract address to approve |
| `--amount` | Yes | - | Amount in minimal units |
| `--chain` | Yes | - | Chain name |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `data` | String | Approval calldata (hex) — use as tx `data` field |
| `dexContractAddress` | String | Spender address (already encoded in `data`). **NOT** the tx `to` — send tx to the token contract |
| `gasLimit` | String | Estimated gas limit for the approval tx |
| `gasPrice` | String | Recommended gas price |

## 4. onchainos swap quote

Get swap quote (read-only price estimate).

```bash
onchainos swap quote --from <address> --to <address> --amount <amount> --chain <chain> [--swap-mode <mode>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Source token contract address |
| `--to` | Yes | - | Destination token contract address |
| `--amount` | Yes | - | Amount in minimal units (sell amount if exactIn, buy amount if exactOut) |
| `--chain` | Yes | - | Chain name |
| `--swap-mode` | No | `exactIn` | `exactIn` or `exactOut` |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `toTokenAmount` | String | Expected output amount in minimal units |
| `fromTokenAmount` | String | Input amount in minimal units |
| `estimateGasFee` | String | Estimated gas fee (native token units) |
| `tradeFee` | String | Trade fee estimate in USD |
| `priceImpactPercent` | String | Price impact as percentage (e.g., `"0.05"`) |
| `router` | String | Router type used |
| `dexRouterList[]` | Array | DEX routing path details |
| `dexRouterList[].dexName` | String | DEX name in the route |
| `dexRouterList[].percentage` | String | Percentage of amount routed through this DEX |
| `fromToken.isHoneyPot` | Boolean | `true` = source token is a honeypot (cannot sell) |
| `fromToken.taxRate` | String | Source token buy/sell tax rate |
| `fromToken.decimal` | String | Source token decimals |
| `fromToken.tokenUnitPrice` | String | Source token unit price in USD |
| `toToken.isHoneyPot` | Boolean | `true` = destination token is a honeypot (cannot sell) |
| `toToken.taxRate` | String | Destination token buy/sell tax rate |
| `toToken.decimal` | String | Destination token decimals |
| `toToken.tokenUnitPrice` | String | Destination token unit price in USD |

## 5. onchainos swap swap

Get swap transaction data (quote -> sign -> broadcast).

```bash
onchainos swap swap --from <address> --to <address> --amount <amount> --chain <chain> --wallet <address> [--slippage <pct>] [--gas-level <level>] [--swap-mode <mode>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Source token contract address |
| `--to` | Yes | - | Destination token contract address |
| `--amount` | Yes | - | Amount in minimal units |
| `--chain` | Yes | - | Chain name |
| `--wallet` | Yes | - | User's wallet address |
| `--slippage` | No | autoSlippage | Slippage tolerance in percent (e.g., `"1"` for 1%). Omit to use autoSlippage. |
| `--gas-level` | No | `average` | Gas priority: `slow`, `average`, `fast` |
| `--swap-mode` | No | `"exactIn"` | `exactIn` or `exactOut` |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `routerResult` | Object | Same structure as quote return (see swap quote above) |
| `tx.from` | String | Sender address |
| `tx.to` | String | Contract address to send the transaction to |
| `tx.data` | String | Transaction calldata (hex) |
| `tx.gas` | String | Gas limit for the transaction |
| `tx.gasPrice` | String | Gas price |
| `tx.value` | String | Native token value to send (in minimal units) |
| `tx.minReceiveAmount` | String | Minimum receive amount after slippage (minimal units) |
| `tx.maxSpendAmount` | String | Maximum spend amount (for exactOut mode) |
| `tx.slippagePercent` | String | Applied slippage tolerance percentage |

## Input / Output Examples

**User says:** "Swap 100 USDC for OKB on XLayer"

```bash
# 1. Quote
onchainos swap quote --from 0x74b7f16337b8972027f6196a17a631ac6de26d22 --to 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --amount 100000000 --chain xlayer
# -> Expected output: 3.2 OKB, Gas fee: ~$0.001, Price impact: 0.05%

# 2. Approve (ERC-20 token needs approval)
onchainos swap approve --token 0x74b7f16337b8972027f6196a17a631ac6de26d22 --amount 100000000 --chain xlayer
# -> Returns approval calldata -> sign & broadcast via wallet contract-call

# 3. Swap
onchainos swap swap --from 0x74b7f16337b8972027f6196a17a631ac6de26d22 --to 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --amount 100000000 --chain xlayer --wallet <local_wallet_addr>
# -> Returns swap calldata -> sign & broadcast via wallet contract-call
```

**User says:** "What DEXes are available on XLayer?"

```bash
onchainos swap liquidity --chain xlayer
# -> Display: CurveNG, XLayer DEX, ... (DEX sources on XLayer)
```
