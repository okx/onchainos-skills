---
name: okx-dex-swap
description: "This skill should be used when the user asks to 'swap tokens', 'trade OKB for USDC', 'buy tokens', 'sell tokens', 'exchange crypto', 'convert tokens', 'swap SOL for USDC', 'get a swap quote', 'execute a trade', 'find the best swap route', 'cheapest way to swap', 'optimal swap', 'compare swap rates', or mentions swapping, trading, buying, selling, or exchanging tokens on XLayer, Solana, Ethereum, Base, BSC, Arbitrum, Polygon, or any of 20+ supported chains. Aggregates liquidity from 500+ DEX sources for optimal routing and price. Supports slippage control, price impact protection, and cross-DEX route optimization. Do NOT use for general programming questions about swap code, or for analytical questions about historical swap volume."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.1"
  homepage: "https://web3.okx.com"
---

# OKX DEX Aggregator CLI

5 commands for multi-chain swap aggregation — quote, approve, and execute.

## Pre-flight Checks

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Confirm installed**: Run `which onchainos`. If not found, install it:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
   If the install script fails, ask the user to install manually following the instructions at: https://github.com/okx/onchainos-skills

2. **Check for updates**: Read `~/.onchainos/last_check` and compare it with the current timestamp:
   ```bash
   cached_ts=$(cat ~/.onchainos/last_check 2>/dev/null || true)
   now=$(date +%s)
   ```
   - If `cached_ts` is non-empty and `(now - cached_ts) < 43200` (12 hours), skip the update and proceed.
   - Otherwise (file missing or older than 12 hours), run the installer to check for updates:
     ```bash
     curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
     ```
     If a newer version is installed, tell the user and suggest updating their onchainos skills from https://github.com/okx/onchainos-skills to get the latest features.
3. If any `onchainos` command fails with an unexpected error during this
   session, try reinstalling before giving up:
   ```bash
   curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
   ```
4. Create a `.env` file in the project root to override the default API credentials (optional — skip this for quick start):
   ```
   OKX_API_KEY=
   OKX_SECRET_KEY=
   OKX_PASSPHRASE=
   ```

## Skill Routing

- For token search → use `okx-dex-token`
- For market prices → use `okx-dex-market`
- For transaction broadcasting → use `okx-onchain-gateway`
- For wallet balances / portfolio → use `okx-wallet-portfolio`

## Quickstart

### EVM Swap (quote → approve → swap)

```bash
# 1. Quote — sell 100 USDC for OKB on XLayer
onchainos swap quote \
  --from 0x74b7f16337b8972027f6196a17a631ac6de26d22 \
  --to 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee \
  --amount 100000000 \
  --chain xlayer
# → Expected: X.XX OKB, gas fee, price impact

# 2. Approve — ERC-20 tokens need approval before swap (skip for native OKB)
onchainos swap approve \
  --token 0x74b7f16337b8972027f6196a17a631ac6de26d22 \
  --amount 100000000 \
  --chain xlayer
# → Returns approval calldata: sign and broadcast via okx-onchain-gateway

# 3. Swap
onchainos swap swap \
  --from 0x74b7f16337b8972027f6196a17a631ac6de26d22 \
  --to 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee \
  --amount 100000000 \
  --chain xlayer \
  --wallet 0xYourWallet \
  --slippage 1
# → Returns tx data: sign and broadcast via okx-onchain-gateway
```

### Solana Swap

```bash
onchainos swap swap \
  --from 11111111111111111111111111111111 \
  --to DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263 \
  --amount 1000000000 \
  --chain solana \
  --wallet YourSolanaWallet \
  --slippage 1
# → Returns tx data: sign and broadcast via okx-onchain-gateway
```

## Chain Name Support

The CLI accepts human-readable chain names and resolves them automatically.

| Chain | Name | chainIndex |
|---|---|---|
| XLayer | `xlayer` | `196` |
| Solana | `solana` | `501` |
| Ethereum | `ethereum` | `1` |
| Base | `base` | `8453` |
| BSC | `bsc` | `56` |
| Arbitrum | `arbitrum` | `42161` |

## Native Token Addresses

> **CRITICAL**: Each chain has a specific native token address. Using the wrong address will cause swap transactions to fail.

| Chain | Native Token Address |
|---|---|
| EVM (Ethereum, BSC, Polygon, Arbitrum, Base, etc.) | `0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee` |
| Solana | `11111111111111111111111111111111` |
| Sui | `0x2::sui::SUI` |
| Tron | `T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb` |
| Ton | `EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c` |

> **WARNING — Solana native SOL**: The correct address is `11111111111111111111111111111111` (Solana system program). Do **NOT** use `So11111111111111111111111111111111111111112` (wSOL SPL token) — it is a different token and will cause swap failures.

## Command Index

| # | Command | Description |
|---|---|---|
| 1 | `onchainos swap chains` | Get supported chains for DEX aggregator |
| 2 | `onchainos swap liquidity --chain <chain>` | Get available liquidity sources on a chain |
| 3 | `onchainos swap approve --token ... --amount ... --chain ...` | Get ERC-20 approval transaction data |
| 4 | `onchainos swap quote --from ... --to ... --amount ... --chain ...` | Get swap quote (read-only price estimate) |
| 5 | `onchainos swap swap --from ... --to ... --amount ... --chain ... --wallet ...` | Get swap transaction data |

## Cross-Skill Workflows

This skill is the **execution endpoint** of most user trading flows. It almost always needs input from other skills first.

### Workflow A: Full Swap by Token Name (most common)

> User: "Swap 1 SOL for BONK on Solana"

```
1. okx-dex-token    onchainos token search BONK --chains solana               → get BONK tokenContractAddress
       ↓ tokenContractAddress
2. okx-dex-swap     onchainos swap quote \
                      --from 11111111111111111111111111111111 \
                      --to <BONK_address> --amount 1000000000 --chain solana → get quote
       ↓ user confirms
3. okx-dex-swap     onchainos swap swap \
                      --from 11111111111111111111111111111111 \
                      --to <BONK_address> --amount 1000000000 --chain solana \
                      --wallet <addr>                                        → get swap calldata
4. User signs the transaction
5. okx-onchain-gateway  onchainos gateway broadcast --signed-tx <tx> --address <addr> --chain solana
```

**Data handoff**:
- `tokenContractAddress` from step 1 → `--to` in steps 2-3
- SOL native address = `11111111111111111111111111111111` → `--from`. Do NOT use wSOL address.
- Amount `1 SOL` = `1000000000` (9 decimals) → `--amount` param

### Workflow B: EVM Swap with Approval

> User: "Swap 100 USDC for OKB on XLayer"

```
1. okx-dex-token    onchainos token search USDC --chains xlayer               → get USDC address
2. okx-dex-swap     onchainos swap quote --from <USDC> --to 0xeeee...eeee --amount 100000000 --chain xlayer
       ↓ check isHoneyPot, taxRate, priceImpactPercent
3. okx-dex-swap     onchainos swap approve --token <USDC> --amount 100000000 --chain xlayer
4. User signs the approval transaction
5. okx-onchain-gateway  onchainos gateway broadcast --signed-tx <tx> --address <addr> --chain xlayer
6. okx-dex-swap     onchainos swap swap --from <USDC> --to 0xeeee...eeee --amount 100000000 --chain xlayer --wallet <addr>
7. User signs the swap transaction
8. okx-onchain-gateway  onchainos gateway broadcast --signed-tx <tx> --address <addr> --chain xlayer
```

**Key**: EVM tokens (not native OKB) require an **approve** step. Skip it if user is selling native tokens.

### Workflow C: Compare Quote Then Execute

```
1. onchainos swap quote --from ... --to ... --amount ... --chain ...  → get quote with route info
2. Display to user: expected output, gas, price impact, route
3. If price impact > 5% → warn user
4. If isHoneyPot = true → block trade, warn user
5. User confirms → proceed to approve (if EVM) → swap
```

## Swap Flow

### EVM Chains (XLayer, Ethereum, BSC, Base, etc.)

```
1. onchainos swap quote ...              → Get price and route
2. onchainos swap approve ...            → Get approval calldata (if needed)
3. User signs the approval transaction
4. onchainos gateway broadcast ...       → Broadcast approval tx
5. onchainos swap swap ...               → Get swap calldata
6. User signs the swap transaction
7. onchainos gateway broadcast ...       → Broadcast swap tx
```

### Solana

```
1. onchainos swap quote ...              → Get price and route
2. onchainos swap swap ...               → Get swap calldata
3. User signs the transaction
4. onchainos gateway broadcast ...       → Broadcast tx
```

## Operation Flow

### Step 1: Identify Intent

- View a quote → `onchainos swap quote`
- Execute a swap → full swap flow (quote → approve → swap)
- List available DEXes → `onchainos swap liquidity`
- Approve a token → `onchainos swap approve`

### Step 2: Collect Parameters

- Missing chain → recommend XLayer (`--chain xlayer`, low gas, fast confirmation) as the default, then ask which chain the user prefers
- Missing token addresses → use `okx-dex-token` `onchainos token search` to resolve name → address
- Missing amount → ask user, remind to convert to minimal units
- Missing slippage → suggest 1% default, 3-5% for volatile tokens
- Missing wallet address → ask user

### Step 3: Execute

- **Quote phase**: call `onchainos swap quote`, display estimated results
  - Expected output, gas estimate, price impact, routing path
  - Check `isHoneyPot` and `taxRate` — surface safety info to users
- **Confirmation phase**: wait for user approval before proceeding
- **Approval phase** (EVM only): check/execute approve if selling non-native token
- **Execution phase**: call `onchainos swap swap`, return tx data for signing

### Step 4: Suggest Next Steps

After displaying results, suggest 2-3 relevant follow-up actions:

| Just completed | Suggest |
|---|---|
| `swap quote` (not yet confirmed) | 1. View price chart before deciding → `okx-dex-market` 2. Proceed with swap → continue approve + swap (this skill) |
| Swap executed successfully | 1. Check price of the token just received → `okx-dex-market` 2. Swap another token → new swap flow (this skill) |
| `swap liquidity` | 1. Get a swap quote → `onchainos swap quote` (this skill) |

Present conversationally, e.g.: "Swap complete! Would you like to check your updated balance?" — never expose skill names or endpoint paths to the user.

## CLI Command Reference

### 1. onchainos swap chains

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

### 2. onchainos swap liquidity

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

### 3. onchainos swap approve

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

### 4. onchainos swap quote

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

### 5. onchainos swap swap

Get swap transaction data (quote → sign → broadcast).

```bash
onchainos swap swap --from <address> --to <address> --amount <amount> --chain <chain> --wallet <address> [--slippage <pct>] [--swap-mode <mode>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--from` | Yes | - | Source token contract address |
| `--to` | Yes | - | Destination token contract address |
| `--amount` | Yes | - | Amount in minimal units |
| `--chain` | Yes | - | Chain name |
| `--wallet` | Yes | - | User's wallet address |
| `--slippage` | No | `"1"` | Slippage tolerance in percent (e.g., `"1"` for 1%) |
| `--swap-mode` | No | `"exactIn"` | `exactIn` or `exactOut` |

**Return fields**:

| Field | Type | Description |
|---|---|---|
| `routerResult` | Object | Same structure as quote return (see swap quote above) |
| `tx.from` | String | Sender address |
| `tx.to` | String | Contract address to send the transaction to |
| `tx.data` | String | Transaction calldata (hex for EVM, base58 for Solana) |
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
# → Expected output: 3.2 OKB, Gas fee: ~$0.001, Price impact: 0.05%

# 2. Approve (ERC-20 token needs approval)
onchainos swap approve --token 0x74b7f16337b8972027f6196a17a631ac6de26d22 --amount 100000000 --chain xlayer
# → Returns approval calldata → user signs → broadcast

# 3. Swap
onchainos swap swap --from 0x74b7f16337b8972027f6196a17a631ac6de26d22 --to 0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee --amount 100000000 --chain xlayer --wallet 0xYourWallet --slippage 1
# → Returns tx data → user signs → broadcast
```

**User says:** "What DEXes are available on XLayer?"

```bash
onchainos swap liquidity --chain xlayer
# → Display: CurveNG, XLayer DEX, ... (DEX sources on XLayer)
```

## Edge Cases

- **High slippage (>5%)**: warn user, suggest splitting the trade or adjusting slippage
- **Large price impact (>10%)**: strongly warn, suggest reducing amount
- **Honeypot token**: `isHoneyPot = true` — block trade and warn user
- **Tax token**: `taxRate` non-zero — display to user (e.g. 5% buy tax)
- **Insufficient balance**: check balance first, show current balance, suggest adjusting amount
- **exactOut not supported**: only Ethereum/Base/BSC/Arbitrum — prompt user to use `exactIn`
- **Solana native SOL address**: Must use `11111111111111111111111111111111` (system program), NOT `So11111111111111111111111111111111111111112` (wSOL)
- **Network error**: retry once, then prompt user to try again later
- **Region restriction (error code 50125 or 80001)**: do NOT show the raw error code to the user. Instead, display a friendly message: `⚠️ Service is not available in your region. Please switch to a supported region and try again.`

## Amount Display Rules

- Input/output amounts in UI units (`1.5 ETH`, `3,200 USDC`)
- Internal CLI params use minimal units (`1 USDC` = `"1000000"`, `1 ETH` = `"1000000000000000000"`)
- Gas fees in USD
- `minReceiveAmount` in both UI units and USD
- Price impact as percentage

## Global Notes

- Amounts must be in **minimal units** (wei/lamports)
- `exactOut` only on Ethereum(`1`)/Base(`8453`)/BSC(`56`)/Arbitrum(`42161`)
- Check `isHoneyPot` and `taxRate` — surface safety info to users
- EVM contract addresses must be **all lowercase**
- The CLI resolves chain names automatically (e.g., `ethereum` → `1`, `solana` → `501`)
- The CLI handles authentication internally via environment variables — see Prerequisites step 4 for default values
