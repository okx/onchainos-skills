---
name: okx-defi-invest
description: "Use this skill to 'invest in DeFi', 'earn yield on USDC', 'deposit into Aave', 'stake ETH on Lido', 'search DeFi products', 'find best APY', 'redeem my DeFi position', 'withdraw from lending', 'claim DeFi rewards', 'borrow USDC', 'repay loan', 'add liquidity to Uniswap V3', 'remove liquidity', or mentions DeFi investing, yield farming, lending, borrowing, staking, liquidity pools, APY/APR across Ethereum, BSC, Avalanche, Sui, Solana, and other supported chains. Supports Aave, Lido, Compound, PancakeSwap, Uniswap, NAVI, Kamino, BENQI, and more. Do NOT use for DEX spot swaps — use okx-dex-swap. Do NOT use for token prices or market data — use okx-dex-market. Do NOT use for wallet token balances — use okx-wallet-portfolio. Do NOT use for viewing DeFi positions/holdings only — use okx-defi-portfolio."
license: Apache-2.0
metadata:
  author: okx
  version: "2.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DeFi Invest

Multi-chain DeFi product discovery and investment execution. The CLI handles precision conversion, multi-step orchestration, and validation internally.

For CLI parameter details, see [references/cli-reference.md](references/cli-reference.md).

## Skill Routing

- For DeFi positions / holdings → use `okx-defi-portfolio`
- For token price/chart → use `okx-dex-market`
- For token search by name/contract → use `okx-dex-token`
- For DEX spot swap execution → use `okx-dex-swap`
- For wallet token balances → use `okx-wallet-portfolio`
- For broadcasting signed transactions → use `okx-onchain-gateway`
- For Agentic Wallet login, balance, contract-call → use `okx-agentic-wallet`

## Command Index

| # | Command | Description |
|---|---------|-------------|
| 1 | `defi list` | List top DeFi products by APY |
| 2 | `defi search --token <tokens> [--platform <names>] [--chain <chain>] [--product-group <group>]` | Search DeFi products |
| 3 | `defi detail --investment-id <id>` | Get full product details |
| 4 | `defi invest --investment-id <id> --address <addr> --token <symbol_or_addr> --amount <minimal_units> [--chain <chain>] [--slippage <pct>] [--tick-lower <n>] [--tick-upper <n>] [--token-id <nft>]` | One-step deposit (CLI handles prepare + precision + calldata) |
| 5 | `defi withdraw --investment-id <id> --address <addr> --chain <chain> [--ratio <0-1>] [--amount <minimal_units>] [--token-id <nft>] [--platform-id <pid>] [--slippage <pct>]` | One-step withdrawal (CLI handles position lookup + calldata) |
| 6 | `defi collect --address <addr> --chain <chain> --reward-type <type> [--investment-id <id>] [--platform-id <pid>] [--token-id <nft>] [--principal-index <idx>]` | One-step reward claim (CLI handles reward check + calldata) |
| 7 | `defi positions --address <addr> --chains <chains>` | List DeFi positions by platform |
| 8 | `defi position-detail --address <addr> --chain <chain> --platform-id <pid>` | Get detailed position info |

## Investment Types

| productGroup | Description |
|-------------|-------------|
| `SINGLE_EARN` | Single-token yield (savings, staking, vaults) |
| `DEX_POOL` | Liquidity pools (Uniswap V2/V3, PancakeSwap, etc.) |
| `LENDING` | Lending / borrowing (Aave, Compound, etc.) |

## Chain Support

CLI resolves chain names automatically (e.g. `ethereum` → `1`, `bsc` → `56`, `solana` → `501`).

## Operation Flow

### Deposit (invest)

```
1. defi search --token USDC --chain ethereum       → pick investmentId
2. defi detail --investment-id <id>                 → confirm APY/TVL, get underlyingToken[].tokenAddress
3. token search --query <tokenAddress> --chains <chain>  → get decimal (e.g. 6) for amount conversion
4. Ask user for amount → convert: userAmount × 10^decimal (e.g. 100 USDC → 100000000)
5. Check wallet balance (okx-wallet-portfolio) → if insufficient, warn user and stop
6. defi invest --investment-id <id> --address <addr> --token USDC --amount 100000000
   → CLI returns calldata (APPROVE + DEPOSIT steps)
7. User signs and broadcasts each step in order
```

> **Token decimal**: Get `tokenAddress` from `defi detail` → `underlyingToken[].tokenAddress`, then use `token search --query <tokenAddress>` to get `decimal`. Same approach as DEX swap.
> **Balance check is mandatory before invest.** Use `okx-wallet-portfolio` to verify sufficient balance.

### Withdraw

```
1. defi positions --address <addr> --chains ethereum
2. defi position-detail --address <addr> --chain ethereum --platform-id <pid>
   → get investmentId, tokenPrecision, coinAmount (current balance)
3. Full exit:
   defi withdraw --investment-id <id> --address <addr> --chain ethereum --ratio 1 --platform-id <pid>
   Partial exit (convert coinAmount to minimal units: amount × 10^tokenPrecision):
   defi withdraw --investment-id <id> --address <addr> --chain ethereum --amount <minimal_units> --platform-id <pid>
4. User signs and broadcasts
```

> **Partial exit --amount**: position-detail returns `coinAmount` in human-readable (e.g. "2.3792") and `tokenPrecision` (e.g. 6). Convert to minimal units: `floor(2.3792 × 10^6) = 2379200` → `--amount 2379200`.

### Claim Rewards

```
1. defi positions --address <addr> --chains ethereum
2. defi position-detail --address <addr> --chain ethereum --platform-id <pid>
3. defi collect --address <addr> --chain ethereum --reward-type REWARD_INVESTMENT --investment-id <id> --platform-id <pid>
   → CLI returns calldata (or skips if no rewards)
4. User signs and broadcasts
```

### V3 Pool Deposit

```
1. defi search --token USDT --platform PancakeSwap --chain bsc --product-group DEX_POOL
2. defi detail --investment-id <id>
3. Ask user for amount and tick range
4. defi invest --investment-id <id> --address <addr> --token USDT --amount 100000000 --range 5
   → CLI handles calculate-entry internally, returns calldata
5. User signs and broadcasts
```

### Step 3: Sign & Broadcast Calldata

After `invest`/`withdraw`/`collect` returns `dataList`, use `onchainos wallet contract-call` to sign and broadcast each step. Execute steps **strictly in order** — wait for each to complete before proceeding.

**EVM chains** (Ethereum, BSC, Polygon, Arbitrum, Base, etc.):
```bash
onchainos wallet contract-call \
  --to <dataList[N].to> \
  --chain <chainIndex> \
  --input-data <dataList[N].serializedData> \
  --value <value_in_UI_units>
```

**EVM (XLayer)**:
```bash
onchainos wallet contract-call \
  --to <dataList[N].to> \
  --chain 196 \
  --input-data <dataList[N].serializedData> \
  --value <value_in_UI_units>
```

**Solana**:
```bash
onchainos wallet contract-call \
  --to <dataList[N].to> \
  --chain 501 \
  --unsigned-tx <dataList[N].serializedData>
```

**`--value` unit conversion**: `dataList[].value` is in minimal units (wei). `contract-call --value` expects UI units. Convert: `value_UI = value / 10^nativeToken.decimal` (e.g. 18 for ETH/POL, 9 for SOL). If `value` is `""`, `"0"`, or `"0x0"`, use `"0"`.

**`--chain` mapping**: `contract-call` requires `realChainIndex` (e.g. `1` for Ethereum, `137` for Polygon, `56` for BSC, `501` for Solana, `196` for XLayer).

**Execution rules**:
- Execute `dataList[0]` first, then `dataList[1]`, etc. Never in parallel.
- `contract-call` handles TEE signing and broadcasting internally — no separate broadcast step needed.
- If any step fails, stop all remaining steps and report which succeeded/failed.

> `invest`/`withdraw`/`collect` only return **unsigned calldata** — they do NOT broadcast. The CLI never holds private keys.

## Displaying Search / List Results

| # | Platform | Chain | investmentId | Name | APY | TVL |
|---|---------|-------|-------------|------|-----|-----|
| 1 | Aave V3 | ETH | 9502 | USDC | 1.89% | $3.52B |

- `investmentId` is **MANDATORY** in every row
- `rate` is decimal → multiply by 100 and append `%`
- `tvl` → format as human-readable USD ($3.52B, $537M)
- Display data as-is — do NOT editorialize on APY values

## rewardType Reference

| rewardType | When to use | Required params |
|------------|-------------|-----------------|
| `REWARD_PLATFORM` | Protocol-level rewards (e.g. AAVE token) | `--platform-id` |
| `REWARD_INVESTMENT` | Product mining/staking rewards | `--investment-id` + `--platform-id` |
| `V3_FEE` | V3 trading fee collection | `--investment-id` + `--token-id` |
| `REWARD_OKX_BONUS` | OKX bonus rewards | `--investment-id` + `--platform-id` |
| `REWARD_MERKLE_BONUS` | Merkle proof-based bonus | `--investment-id` + `--platform-id` |
| `UNLOCKED_PRINCIPAL` | Unlocked principal after lock | `--principal-index` |

## Key Protocol Rules

- **Aave borrow**: uses `callDataType=WITHDRAW` internally — do not expose to user
- **Aave repay**: uses `callDataType=DEPOSIT` internally — do not expose to user
- **V3 Pool exit**: pass `--token-id` + `--ratio` (e.g. `--ratio 1` for full exit)
- **Partial withdrawal (non-V3)**: pass `--amount` for the exit amount
- **Full withdrawal**: `--ratio 1`

## Post-execution Suggestions

| Just completed | Suggest |
|----------------|---------|
| `defi list` / `defi search` | View details → `defi detail`, or start deposit flow |
| `defi detail` | Proceed → `defi invest`, or compare → `defi search` |
| `defi invest` success | View positions → `okx-defi-portfolio`, or search more |
| `defi withdraw` success | Check positions → `okx-defi-portfolio`, or check balance → `okx-wallet-portfolio` |
| `defi collect` success | Check positions → `okx-defi-portfolio`, or swap rewards → `okx-dex-swap` |

## Error Codes

| Code | Scenario | Handling |
|------|----------|----------|
| 84400 | Parameter null | Check required params — partial exit needs `--amount` or `--ratio` |
| 84021 | Asset syncing | "Position data is syncing, please retry shortly" |
| 84023 | Invalid expectOutputList | CLI auto-constructs from position-detail; retry or pass `--platform-id` |
| 84014 | Balance check failed | Insufficient balance — check with `okx-wallet-portfolio` |
| 84018 | Balancing failed | V3 balancing failed — adjust price range or increase slippage |
| 84010 | Token not supported | Check supported tokens via `defi detail` |
| 84001 | Platform not supported | DeFi platform not supported |
| 84016 | Contract execution failed | Check parameters and retry |
| 84019 | Address format mismatch | Address format invalid for this chain |
| 50011 | Rate limit | Wait and retry |

## Global Notes

- `--amount` must be in **minimal units** (integer). Convert: userAmount × 10^tokenPrecision. Example: 0.1 USDC (precision=6) → `--amount 100000`. Get tokenPrecision from `defi detail` or `defi position-detail`
- The wallet address parameter for ALL defi commands is `--address`
- `--slippage` default is `"0.01"` (1%); suggest `"0.03"`–`"0.05"` for volatile V3 pools
- Solana DeFi transactions use base58-encoded VersionedTransaction — sign and broadcast within 60 seconds
- High APY warning: APY > 50% → alert user about elevated risk
- User confirmation required before every invest/withdraw/collect execution
- Address used for calldata generation MUST match the signing address
