---
name: okx-defi-invest
description: "Use this skill to 'invest in DeFi', 'earn yield on USDC', 'deposit into Aave', 'stake ETH on Lido', 'search DeFi products', 'find best APY', 'redeem my DeFi position', 'withdraw from lending', 'claim DeFi rewards', 'borrow USDC', 'repay loan', 'add liquidity to Uniswap V3', 'remove liquidity', '申购理财产品', '赎回DeFi', '领取DeFi奖励', '存款', '借贷', '流动性挖矿', '质押', '最高年化', '追加投资', or mentions DeFi investing, yield farming, lending, borrowing, staking, liquidity pools, APY/APR across Ethereum, BSC, Avalanche, Sui, Solana, and other supported chains. Supports Aave, Lido, Compound, PancakeSwap, Uniswap, NAVI, Kamino, BENQI, and more. Do NOT use for DEX spot swaps — use okx-dex-swap. Do NOT use for token prices or market data — use okx-dex-market. Do NOT use for wallet token balances — use okx-wallet-portfolio. Do NOT use for viewing DeFi positions/holdings only — use okx-defi-portfolio."
license: Apache-2.0
metadata:
  author: okx
  version: "1.0.5"
  homepage: "https://web3.okx.com"
---

# OKX DeFi Invest

8 commands for multi-chain DeFi product discovery and investment execution (deposit, redeem, claim).

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
| 1 | `onchainos defi list [--page-num <n>]` | List top DeFi products by APY (no filters) |
| 2 | `onchainos defi search --token <tokens> [--platform <names>] [--chain <chain>] [--product-group <group>] [--page-num <n>]` | Search DeFi investment products |
| 3 | `onchainos defi detail --investment-id <id>` | Get full product details |
| 4 | `onchainos defi prepare --investment-id <id>` | Pre-investment check (token list, V3 ticks) |
| 5 | `onchainos defi calculate-entry --id <id> --address <addr> --input-token <token_addr> --input-amount <amount> --token-decimal <decimals> [--tick-lower <n>] [--tick-upper <n>]` | Calculate exact token amounts for V3 pool entry |
| 6 | `onchainos defi deposit --investment-id <id> --address <addr> --user-input '<json>' [--slippage <pct>] [--token-id <nft>] [--tick-lower <n>] [--tick-upper <n>]` | Build deposit calldata |
| 7 | `onchainos defi redeem --id <id> --chain <chain> --address <addr> [--token-id <nft>] [--ratio <n>] [--user-input '<json>']` | Build redemption calldata |
| 8 | `onchainos defi claim --address <addr> --chain <chain> --reward-type <type> [--id <id>] [--platform-id <id>] [--token-id <nft>] [--expect-output '<json>']` | Build claim calldata |

## Investment Types

| productGroup | Description |
|-------------|-------------|
| `SINGLE_EARN` | Single-token yield (savings, staking, vaults) |
| `DEX_POOL` | Liquidity pools (Uniswap V2/V3, PancakeSwap, etc.) |
| `LENDING` | Lending / borrowing (Aave, Compound, etc.) |

## Operation Flow

### Step 1: Identify Intent

| User says | Action |
|-----------|--------|
| Browse top products by APY | `defi list` |
| Search / find DeFi products | `defi search --token <kw>` |
| Deposit / stake / invest / earn | search → detail → prepare → deposit |
| Withdraw / redeem / exit | position-detail → `defi redeem` |
| Claim rewards / fees | position-detail → `defi claim` |
| Borrow | search (LENDING) → detail → prepare → deposit |
| Repay | position-detail → `defi redeem` with `--user-input` |
| Add V3 liquidity | search (DEX_POOL) → detail → prepare → calculate-entry → deposit |
| Remove V3 liquidity | position-detail (get tokenId) → redeem with `--token-id` |

For complex flows (V3 pool, borrow/repay, Agentic Wallet), see Cross-Skill Workflows section below.

### Step 2: Collect Parameters

- **Pre-check `isInvestable`** → call `defi detail`, if `false` stop and inform user
- **Missing amount** → MUST ask user explicitly. Display amounts in human-readable form to users, but pass **minimal units** (integer) in `--user-input` coinAmount with `tokenPrecision` (see Global Notes)
- **Zero amount** → if user inputs 0 or empty amount, reject immediately. Do NOT call deposit/redeem/claim. CLI will also reject coinAmount=0.
- **Missing token address** → use `okx-dex-token` to resolve
- **Missing wallet address** → ask user
- **Missing investmentId** → run `defi search` or `defi list` first
- **Missing slippage** → default `"0.01"` (1%); suggest `"0.03"`–`"0.05"` for volatile V3 pools
- **MUST call `defi prepare` before `defi deposit`** → for ALL product types. Returns `investWithTokenList` (tokenAddress, chainIndex, tokenPrecision)
- **V3 Pool: MUST call `defi calculate-entry` after `defi prepare`** → returns exact dual-token amounts for `--user-input`
- **V3 tick range** → from prepare get `currentTick`, `tickSpacing`; use `=` syntax for negatives: `--tick-lower=-11`

### Pre-Deposit Gate (MANDATORY)

Before calling `defi deposit`, ALL checks must pass:
1. **Balance Check** → `okx-wallet-portfolio` verify sufficient balance for ALL input tokens
2. **V3 Only** → `defi calculate-entry` called, exact amounts obtained

### Pre-Redeem Gate (MANDATORY)

Before calling `defi redeem`:
- MUST call `defi position-detail` first to get `investmentId` + underlying token info
- **Balance Check**: compare user's redeem amount against `position-detail` token balance (`coinAmount`). If redeem amount > position balance → warn user "Redeem amount exceeds current position balance" and STOP. Do NOT call `defi redeem`.
- Full exit → `--ratio 1` (+ `--user-input` preferred); Partial exit → `--user-input` MUST; V3 → `--token-id` + `--ratio`

### Pre-Claim Gate (MANDATORY)

Before calling `defi claim`:
- MUST call `defi position-detail` first to get `investmentId`, `rewardType`, reward token info (`rewardDefiTokenInfo`)
- If reward token info available → ALWAYS pass `--expect-output` directly (do NOT rely on auto-fetch)

### Step 3: Execute

- **Show product details before every transaction** (protocol, APY, TVL, fee rate, chain, investmentId)
- **Get explicit user confirmation** before signing
- **Display calldata summary** after `deposit`/`redeem`/`claim` returns `dataList`:
  ```
  Calldata ready! N transaction(s) to execute:
  Step 1/N: APPROVE  to: 0x7251... chain: Ethereum  value: 0x0
  Step 2/N: DEPOSIT  to: 0x7251... chain: Ethereum  value: 0x48da...
  ```
- **Display sign data block** — `serializedData` in its own fenced code block (never inline in JSON)
- **Append next-step instructions**:
  ```
  Next Steps:
  - External Wallet: sign each step -> onchainos gateway broadcast --signed-tx <tx> --address <addr> --chain <chain>
  - Agentic Wallet:  onchainos wallet contract-call --to <to> --chain <chainId> --input-data <serializedData>
  ```
- **Execute strictly in order** — only proceed after current step confirms success

If any command returns an error code, see Error Codes section below.

### Signing Path Selection

**How to determine wallet type:**
1. Address from `onchainos wallet balance/addresses` → **Agentic Wallet**
2. Address pasted by user or local signing scripts → **External Wallet**
3. Ambiguous → ask user

**Path A: External Wallet**

Display sign data → user signs → `gateway broadcast` → `gateway orders` poll until `txStatus=2`.

**Path B: Agentic Wallet**

| Chain | Command |
|-------|---------|
| **EVM** | `onchainos wallet contract-call --to <to> --chain <chainId> --input-data <serializedData> [--value <value_UI>]` |
| **EVM (XLayer)** | `onchainos wallet contract-call --to <to> --chain 196 --input-data <serializedData> [--value <value_UI>]` |
| **Solana** | `onchainos wallet contract-call --to <to> --chain 501 --unsigned-tx <serializedData>` |

**`--value` conversion**: calldata `value` is in wei. `contract-call --value` expects UI units. Convert: `value_UI = value / 10^nativeTokenDecimal` (get from `defi prepare` → `investWithTokenList[]` where `isBaseToken=true` → `tokenPrecision`). If `value` is `""`, `"0"`, or `"0x0"`, omit or use `"0"`.

**Execution rules (both paths)**:
1. Execute `dataList` steps strictly in order
2. **MUST wait for on-chain confirmation before next step** — `txHash` returned only means broadcast success, NOT on-chain confirmation:
   - **External Wallet**: poll `gateway orders --order-id <id>` until `txStatus=2` (confirmed on-chain)
   - **Agentic Wallet**: after `contract-call` returns `txHash`, poll `gateway orders --address <addr> --chain <chain>` until the tx is confirmed on-chain. Do NOT immediately execute the next step — the previous tx may still be in mempool
3. If any step fails → stop all remaining, report which succeeded/failed
4. Never broadcast in parallel

**Result messaging (Agentic Wallet)**: Use business-level language ("Deposit complete", "Withdrawal complete"). Do NOT say "Transaction broadcast".

**Pre-requisite**: Agentic Wallet requires login. Check `onchainos wallet status` first.

> **Important**: `defi deposit`/`defi redeem`/`defi claim` only return **unsigned calldata** — they do NOT broadcast. The CLI never holds private keys.

## Security Rules

> **Mandatory. Never skip or bypass.**

1. **User confirmation** before every `deposit`/`redeem`/`claim`. Display full details and receive explicit confirmation.
2. **Always call `position-detail` before `redeem` or `claim`** — enforced by Pre-Redeem/Pre-Claim Gates above.
3. **Solana blockhash validity ~60 seconds.** Sign and broadcast immediately. If expired, re-fetch calldata.
4. **Minimize approve amounts.** Default to exact-amount. No unlimited approve unless user explicitly requests.
5. **High APY warning.** APY > 50% → "High yield alert: APY over 50% may carry elevated risk."
6. **Lending health rate.** `healthRate < 1.5` → strong liquidation warning before proceeding.
7. **V3 narrow range.** `isNarrow=true` → warn about high impermanent loss risk.
8. **Sequential execution.** `dataList` must execute in order. Never parallel.
9. **No auto-retry.** Report errors clearly. Do not automatically retry transactions.
10. **Large tx simulation.** >$1000 → recommend `onchainos gateway simulate` first.
11. **Address consistency.** The address used in `deposit`/`redeem`/`claim` (calldata generation) MUST be the same address used for signing and broadcasting. Never sign/broadcast with a different wallet than the one used to generate calldata — the calldata contains address-specific data (permit signatures, allowances) that will revert if executed from a different address.

## Key Protocol Rules

- **Aave borrow**: `callDataType=WITHDRAW` — internal semantics, do not expose to user
- **Aave repay**: `callDataType=DEPOSIT` — internal semantics, do not expose to user
- **V3 Pool entry**: MUST use `calculate-entry` to get exact dual-token amounts; use both in `--user-input`
- **V3 Pool exit**: MUST pass `--token-id` + `--ratio` (e.g. `--token-id <nft> --ratio 1` for full exit)
- **Redeem non-V3**: full exit → `--ratio 1`; partial → `--user-input` with underlying token (NOT aToken)
- **Repay**: always `--user-input` with exact amount

## callDataType Reference

| callDataType | Description | Encoding |
|--------------|-------------|----------|
| `APPROVE` | ERC-20 authorization | EVM hex |
| `DEPOSIT` | Deposit to protocol (also Aave repay) | EVM hex / Solana base58 / Sui base64 |
| `SWAP,DEPOSIT` | Swap then deposit (V3 legacy) | EVM hex |
| `WITHDRAW` | Withdraw from protocol (also Aave borrow) | EVM hex / Solana base58 |
| `WITHDRAW,SWAP` | Withdraw then swap | EVM hex |
| `EMPTY` | No calldata (Solana full tx) | Solana base58 |

**Per-chain signing**:
- **EVM**: `serializedData` as `tx.data`, `to` as target contract
- **Solana**: base58 decode → skip 65-byte sig → sign → broadcast within 60s
- **Sui**: base64 decode → prepend `[0,0,0]` → blake2b → sign

## Multi-step Transaction Patterns

```
Deposit:  [APPROVE] → [DEPOSIT]    or    [DEPOSIT] (native token)
Redeem:   [WITHDRAW]    or    [APPROVE, WITHDRAW]    or    [WITHDRAW,SWAP]
```

## rewardType Reference

| rewardType | When to use | Required params |
|------------|-------------|-----------------|
| `REWARD_PLATFORM` | Protocol-level rewards (e.g. AAVE token) | `--platform-id` |
| `REWARD_INVESTMENT` | Product mining/staking rewards | `--id` + `--platform-id` |
| `V3_FEE` | V3 trading fee collection | `--id` + `--token-id` |
| `REWARD_OKX_BONUS` | OKX bonus rewards | `--id` + `--platform-id` |
| `REWARD_MERKLE_BONUS` | Merkle proof-based bonus | `--id` + `--platform-id` |
| `UNLOCKED_PRINCIPAL` | Unlocked principal after lock | `--principal-index` |

**expectOutputList rule**: If reward token info from `position-detail` is in context → ALWAYS pass `--expect-output` directly. Only rely on auto-fetch (via `--platform-id`) as fallback.

## Displaying Search / List Results

| # | Platform | Chain | investmentId | Name | APY | TVL |
|---|---------|-------|-------------|------|-----|-----|
| 1 | Aave V3 | ETH | 9502 | USDC | 1.89% | $3.52B |

- `investmentId` is **MANDATORY** in every row
- `rate` is decimal → multiply by 100 and append `%`
- `tvl` → format as human-readable USD ($3.52B, $537M)
- Display data as-is — do NOT editorialize on APY values

## Post-execution Suggestions

| Just completed | Suggest |
|----------------|---------|
| `defi list` / `defi search` | View details → `defi detail`, or start deposit flow |
| `defi detail` | Proceed → `defi prepare` + `defi deposit`, or compare → `defi search` |
| `defi deposit` success | View positions → `okx-defi-portfolio`, or search more |
| `defi redeem` success | Check positions → `okx-defi-portfolio`, or check balance → `okx-wallet-portfolio` |
| `defi claim` success | Check positions → `okx-defi-portfolio`, or swap rewards → `okx-dex-swap` |

## Global Notes

- `--user-input` coinAmount MUST be **minimal units** (integer) with `tokenPrecision` field. CLI converts to decimal internally.
  Format: `[{"tokenAddress":"0x...","chainIndex":"137","coinAmount":"500000","tokenPrecision":"6"}]`
  AI conversion: user says "0.5 USDC" → `floor(0.5 × 10^6) = 500000` → `coinAmount: "500000"`, `tokenPrecision: "6"`
  CLI will **reject** decimal coinAmount (e.g. `"0.5"`) and missing tokenPrecision with a clear error.
  `tokenPrecision` source: `defi prepare` → `investWithTokenList[].tokenPrecision` (deposit) or `position-detail` → `assetsTokenList[].tokenPrecision` (redeem/claim)
- The wallet address parameter for ALL defi commands is `--address`
- The CLI resolves chain names automatically (`ethereum` → `1`, `bsc` → `56`)
- `--slippage` default is `"0.01"` (1%)
- Solana DeFi transactions use base58-encoded VersionedTransaction

## Cross-Skill Workflows

For all workflows, execute signing per "Signing Path Selection" above.

### Workflow A: Find Best Yield and Deposit

> User: "Help me earn yield on my 1000 USDC on Ethereum"

```
1. onchainos defi search --token USDC --chain ethereum
      → get product list (rate, tvl, investmentId)
2. Display top options to user, ask which product to invest
      ↓ user selects investmentId
3. onchainos defi detail --investment-id <investmentId>
      → display rate, tvl, fee rate, isInvestable
4. **Ask user: "How much <token> would you like to deposit?"** — MUST get explicit amount
      ↓ user provides amount (e.g. "1000")
5. onchainos defi prepare --investment-id <investmentId>
      → get investWithTokenList (tokenAddress, chainIndex, tokenPrecision)
--- Pre-Deposit Gate ---
6. okx-wallet-portfolio   Check wallet balance for the input token
      → if amount > balance, warn and ask to adjust
--- Gate passed ---
7. onchainos defi deposit \
                --investment-id <id> --address <addr> \
                --user-input '[{"tokenAddress":"0xa0b8...","chainIndex":"1","coinAmount":"1000000000","tokenPrecision":"6"}]'
      → returns ordered dataList (e.g. [APPROVE, DEPOSIT])
8. Execute dataList per Signing Path Selection
9. Report: "1000 USDC deposited into Aave V3 (Ethereum)"
```

**Data handoff**: `investmentId` step 1 → steps 3–5; `tokenAddress` + `chainIndex` + `tokenPrecision` from step 5 → step 7 `--user-input`

### Workflow B: Check Holdings and Redeem

> User: "Redeem all my USDC from Aave"

```
1. onchainos defi positions --address <addr> --chains ethereum
      → display platform list, get analysisPlatformId
2. onchainos defi position-detail --address <addr> --platform-id <pid> --chain ethereum
      → get investmentId, underlying tokenAddress, chainIndex, tokenPrecision
3. onchainos defi redeem:
   Full exit:  --id <investmentId> --chain ethereum --address <addr> --ratio 1 \
               --user-input '[{"tokenAddress":"<underlying>","chainIndex":"<id>","coinAmount":"<balance_minimal>","tokenPrecision":"<p>"}]'
   Partial:    --id <investmentId> --chain ethereum --address <addr> \
               --user-input '[{"tokenAddress":"<underlying>","chainIndex":"<id>","coinAmount":"<amount_minimal>","tokenPrecision":"<p>"}]'
4. Execute dataList per Signing Path Selection
5. Report: "Redemption successful"
```

**Redeem rules**: Full exit → `--ratio 1` (+ `--user-input` preferred); Partial → `--user-input` MUST with underlying token; V3 Pool → `--token-id` + `--ratio` (e.g. `--ratio 1` for full exit).

### Workflow C: Claim Rewards

> User: "Claim my rewards on Compound"

```
1. onchainos defi positions --address <addr> --chains ethereum
      → get analysisPlatformId
2. onchainos defi position-detail --address <addr> --platform-id <pid> --chain ethereum
      → get investmentId, rewardDefiTokenInfo
3. onchainos defi claim \
                --address <addr> --chain ethereum \
                --reward-type REWARD_INVESTMENT --id <investmentId> \
                --platform-id <pid> \
                --expect-output '[{"tokenAddress":"<addr>","chainIndex":"<id>","coinAmount":"<amount>"}]'
4. Execute dataList per Signing Path Selection
5. Report: "Rewards claimed successfully"
```

### Workflow D: V3 Pool Liquidity

> User: "Add USDT liquidity on PancakeSwap V3"

```
1. onchainos defi search --token USDT --platform PancakeSwap --chain bsc --product-group DEX_POOL
2. onchainos defi detail --investment-id <id>  → check isInvestable
3. onchainos defi prepare --investment-id <id>  → get ticks, tokenPrecision
4. Ask user for input amount
--- Pre-Deposit Gate ---
5. onchainos defi calculate-entry --id <id> --address <addr> --input-token <token> --input-amount <amt> --token-decimal <dec> --tick-lower=<tl> --tick-upper=<tu>
      → returns exact amounts for BOTH tokens
6. okx-wallet-portfolio   Check balance for BOTH tokens
7. If one insufficient → Option A (reduce) or Option B (swap to acquire)
--- Gate passed ---
8. onchainos defi deposit --investment-id <id> --address <addr> \
      --user-input '[{"tokenAddress":"<A>","chainIndex":"56","coinAmount":"<minA>","tokenPrecision":"<pA>"},
                     {"tokenAddress":"<B>","chainIndex":"56","coinAmount":"<minB>","tokenPrecision":"<pB>"}]' \
      --tick-lower=<tl> --tick-upper=<tu>
9. Execute dataList per Signing Path Selection
10. Report: "Liquidity added, LP NFT received"
```

### Workflow E: Borrow and Repay

> User: "Borrow 100 USDC on Aave V3"

```
--- Borrow ---
1. onchainos defi search --token USDC --platform Aave --chain avalanche --product-group LENDING
2. onchainos defi detail --investment-id <id>  → check healthRate < 1.5 → warn
3. onchainos defi prepare --investment-id <id>
4. onchainos defi deposit --investment-id <id> --address <addr> \
      --user-input '[{"tokenAddress":"<USDC>","chainIndex":"43114","coinAmount":"100000000","tokenPrecision":"6"}]'
      → callDataType=WITHDRAW (normal Aave borrow semantics)
5. Execute → Report: "Borrowed 100 USDC"

--- Repay ---
6. Check balance for repay token
7. onchainos defi redeem --id <id> --chain avalanche --address <addr> \
      --user-input '[{"tokenAddress":"<USDC>","chainIndex":"43114","coinAmount":"100001000","tokenPrecision":"6"}]'
      → callDataType=DEPOSIT (normal Aave repay semantics)
8. Execute → Report: "Repayment successful"
```

### Workflow F: Agentic Wallet Deposit

> User: "Log in and deposit 50 USDC into Aave"

```
1. onchainos wallet status → if not logged in, guide login
2. onchainos wallet balance --chain 1 → get EVM address, verify balance
3. onchainos defi search → detail → prepare
4. onchainos defi deposit --investment-id <id> --address <wallet_addr> \
      --user-input '[{"tokenAddress":"0xa0b8...","chainIndex":"1","coinAmount":"50000000","tokenPrecision":"6"}]'
5. Execute via contract-call:
   Step 1: onchainos wallet contract-call --to <approve.to> --chain 1 --input-data <approve.serializedData>
   Step 2: onchainos wallet contract-call --to <deposit.to> --chain 1 --input-data <deposit.serializedData>
6. Report: "50 USDC deposited into Aave V3"
```

## Error Codes

> For known codes, show friendly message. For unknown: `Operation failed (code={code}): {msg}`

| Code | Scenario | Handling |
|------|----------|----------|
| 84400 | Parameter null | `userInputList` missing — partial exit needs `--user-input`; full exit use `--ratio 1` |
| 84021 | Asset syncing | "Position data is syncing, please retry shortly" |
| 84023 | Invalid expectOutputList | Pass `--expect-output` from `position-detail`; or use `--platform-id` auto-fetch |
| 84014 | Balance check failed | Insufficient balance — check with `okx-wallet-portfolio` |
| 84018 | Balancing failed | V3 balancing failed — adjust price range or increase slippage |
| 84010 | Token not supported | Run `defi prepare` to check supported tokens |
| 84001 | Platform not supported | DeFi platform not supported |
| 84003 | Protocol not supported | DeFi protocol not supported |
| 84007 | Product not supported | Investment product not supported |
| 84016 | Contract execution failed | Check parameters and retry |
| 84019 | Address format mismatch | Address format invalid for this chain |
| 50011 | Rate limit | Wait and retry |
| 50111 | Invalid API key | Check API Key configuration |
| 50113 | Invalid signature | Check API secret configuration |
| 50125 / 80001 | Region restriction | "Service not available in your region" |

## Edge Cases

| Scenario | Handling |
|----------|----------|
| `isInvestable=false` | Stop, inform user, link to protocol's official site |
| `isSupportRedeem=false` | Inform user redemption not supported |
| V3 exit missing tokenId | Run `position-detail` first to get NFT tokenId |
| Aave `WITHDRAW` for borrow | Normal — do not expose to user |
| Aave `DEPOSIT` for repay | Normal — do not expose to user |
| Search empty results | Suggest relaxing filters |
| Solana blockhash expired | Re-fetch calldata, sign within 60 seconds |
| Multi-step middle step fails | Stop all subsequent steps, report status |
