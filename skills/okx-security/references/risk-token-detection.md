# Risk Token Detection

`onchainos security token-scan` — batch token risk and honeypot detection across all supported chains.

## 3-Path Decision Tree

### Path 1 — Agentic Wallet (loggedIn: true), scanning own wallet

Two-step flow — always fetch balance first, then scan:

**Step 1**: Fetch authenticated wallet holdings:
```bash
onchainos wallet balance          # all chains
onchainos wallet balance --chain <chain>   # specific chain
```

**Step 2**: Extract non-native ERC-20 / SPL tokens from the response (skip native tokens like ETH/SOL/OKB — they have no contract address). Then scan:
```bash
onchainos security token-scan --tokens "<chainIndex>:<contractAddress>,..."
```

- **Single token by name**: Search with `onchainos token search <name>`, confirm address, then use `--tokens`.
- Fall through to Path 3 if user provides an explicit address directly.

### Path 1b — Agentic Wallet (loggedIn: true), scanning a DIFFERENT address

The target address is not the user's own wallet — use public portfolio query instead:

**Step 1**: Fetch holdings of the target address via `portfolio all-balances`:
```bash
# EVM address
onchainos portfolio all-balances --address <target_evm_addr> --chains "1,56,137,42161,8453,196,43114,10" --filter 1

# Solana address (if applicable)
onchainos portfolio all-balances --address <target_sol_addr> --chains "501" --filter 1
```

Display a summary table of holdings to the user before scanning.

**Step 2**: Extract non-native ERC-20 / SPL tokens, then scan:
```bash
onchainos security token-scan --tokens "<chainIndex>:<contractAddress>,..."
```

### Path 2 — No Agentic Wallet (not logged in), user provides wallet address

Two-step flow — fetch public address balance first, then scan:

**Step 1**: Fetch public address holdings. Query EVM and Solana addresses separately:
```bash
# EVM address (all supported chains)
onchainos portfolio all-balances --address <evm_addr> --chains "1,56,137,42161,8453,196,43114,10" --filter 1

# Solana address (if user has one)
onchainos portfolio all-balances --address <sol_addr> --chains "501" --filter 1
```

Display a summary table of holdings to the user before scanning.

**Step 2**: Extract non-native ERC-20 / SPL tokens, then scan:
```bash
onchainos security token-scan --tokens "<chainIndex>:<contractAddress>,..."
```

If the user wants to create an Agentic Wallet instead, guide through login then use Path 1.

### Path 3 — Explicit chainId:contractAddress

```bash
onchainos security token-scan --tokens "<chainId>:<contractAddress>[,...]"
```

If user provides name/symbol instead, search first with `onchainos token search`, confirm, then use `--tokens`.

## Parameters (explicit `--tokens` mode)

| Param | Required | Description |
|---|---|---|
| `--tokens` | Yes | Comma-separated `chainId:contractAddress` pairs (max 50). Chain can be name or ID (e.g. `ethereum:0x...` or `1:0x...`) |

> **Internal mechanism**: All three modes ultimately call the same `/api/v6/security/token-scan` endpoint with `{tokenList: [{chainId, contractAddress}]}`. The `--address` and no-flags modes first query the balance API to obtain the contract address list, then batch-scan (max 50 per batch, concurrent execution). The `--tokens` mode passes contract addresses directly, skipping the query step. **Native tokens (ETH/BNB/SOL/OKB etc.) are skipped in all modes** because their `tokenContractAddress` is empty.

## Scan Modes

| Mode | When to use | Command |
|------|-------------|---------|
| `--tokens` | **Primary mode** — used after fetching balance in Path 1 / 2 | `onchainos security token-scan --tokens "<chainId>:<addr>[,...]"` |
| No flags | Agentic Wallet shortcut (skips explicit balance step) | `onchainos security token-scan [--chain <chain>]` |
| `--address` | Public address shortcut (skips explicit balance step) | `onchainos security token-scan --address <addr> [--chain <chain>]` |

> **Recommended: use `--tokens` mode.** First fetch holdings via `wallet balance` (logged in) or `portfolio all-balances` (not logged in) to display the portfolio to the user, then construct the `--tokens` parameter from that data. This way the user can see their holdings before scanning.
>
> **Note: Native tokens (ETH / BNB / SOL / OKB etc.) are silently skipped.** Native tokens have no contract address and cannot be scanned by token-scan. Only pass ERC-20 / SPL contract token addresses to `--tokens`. If the user explicitly wants to verify native token safety, use `dapp-scan` or `tx-scan` with the specific transaction data.

## Return Fields

| Field | Type | Description |
|---|---|---|
| `chainId` | String | Chain ID |
| `tokenAddress` | String | Token contract address |
| `isChainSupported` | Boolean | Whether the chain supports security scanning |
| `isRiskToken` | Boolean | Whether the token is high-risk (composite flag) |
| `buyTaxes` | String\|null | Buy tax percentage (null = unknown) |
| `sellTaxes` | String\|null | Sell tax percentage (null = unknown) |
| `isHoneypot` | Boolean | Honeypot — cannot sell after buying |
| `isRubbishAirdrop` | Boolean | Garbage/spam airdrop token |
| `isAirdropScam` | Boolean | Gas-mint scam — steals gas fees |
| `isHasAssetEditAuth` | Boolean | Privileged address with asset edit authority (Solana) |
| `isLowLiquidity` | Boolean | Low liquidity |
| `isDumping` | Boolean | Dumping — large sell-off detected |
| `isLiquidityRemoval` | Boolean | Liquidity removal detected |
| `isPump` | Boolean | Pump — artificial price inflation |
| `isWash` | Boolean | Wash trading detected |
| `isFakeLiquidity` | Boolean | Fake/artificial liquidity |
| `isWash2` | Boolean | Wash trading detected (third-party vendor) |
| `isFundLinkage` | Boolean | Rugpull gang linkage detected |
| `isVeryLowLpBurn` | Boolean | Very low LP burn ratio |
| `isVeryHighLpHolderProp` | Boolean | LP holder concentration is very high |
| `isHasBlockingHis` | Boolean | Has history of freezing addresses |
| `isOverIssued` | Boolean | Token over-issued beyond stated supply |
| `isCounterfeit` | Boolean | Counterfeit — impersonates a well-known token |
| `isMintable` | Boolean | Token supply can be increased (mintable) |
| `isHasFrozenAuth` | Boolean | Contract has freeze authority |
| `isNotRenounced` | Boolean | Contract ownership not renounced |

## Risk Label Catalog

### Level 4 — Critical Risk (Block buy)

| # | Label | API Field | Description |
|---|---|---|---|
| 4-1 | Honeypot | `isHoneypot` | Token cannot be sold after purchase |
| 4-2 | Garbage Airdrop | `isRubbishAirdrop` | Spam/scam airdrop token |
| 4-3 | Gas Mint Scam | `isAirdropScam` | Steals gas fees via airdrop interaction |

### Level 3 — High Risk (Pause for user confirmation on buy)

| # | Label | API Field | Description |
|---|---|---|---|
| 3-1 | Privileged Address (Solana only) | `isHasAssetEditAuth` | Account has asset edit authority. Only applies to Solana (`chainId: 501`). Ignore on other chains. |
| 3-2 | Low Liquidity | `isLowLiquidity` | Insufficient trading liquidity |
| 3-3 | Dumping | `isDumping` | Large sell-off / insider dumping |
| 3-4 | Liquidity Removal | `isLiquidityRemoval` | LP being removed |
| 3-5 | Pump | `isPump` | Artificial price inflation |
| 3-6 | Wash Trading | `isWash` | Fake volume via wash trading |
| 3-7 | Fake Liquidity | `isFakeLiquidity` | Artificially inflated liquidity |
| 3-8 | Wash Trading v2 | `isWash2` | Wash trading (third-party vendor detection) |
| 3-9 | Rugpull Gang | `isFundLinkage` | Linked to known rugpull addresses |
| 3-10 | Very Low LP Burn | `isVeryLowLpBurn` | Minimal LP tokens burned |
| 3-11 | Very High LP Holder Concentration | `isVeryHighLpHolderProp` | LP held by very few addresses |
| 3-12 | Has Blocking History | `isHasBlockingHis` | Contract has frozen addresses before |
| 3-13 | Over Issued | `isOverIssued` | Token supply exceeds stated amount |
| 3-14 | Counterfeit | `isCounterfeit` | Impersonates a well-known token |

### Level 2 — Medium Risk (Info warning only)

| # | Label | API Field | Description |
|---|---|---|---|
| 2-1 | Mintable | `isMintable` | Supply can be increased |
| 2-2 | Has Freeze Authority | `isHasFrozenAuth` | Contract can freeze accounts |
| 2-3 | Not Renounced | `isNotRenounced` | Contract ownership retained |

### Tax Thresholds (Special Handling)

Tax fields (`buyTaxes`, `sellTaxes`) map to risk levels based on value:

| Tax Value | Risk Level | Agent Behavior |
|---|---|---|
| ≥ 50% | Level 4 | Block buy transaction |
| ≥ 21% and < 50% | Level 3 | Pause, require user confirmation for buy |
| > 0% and < 21% | Level 2 | Show tax info, do not pause |
| 0% or null | — | No tax risk (null = tax data unavailable, do not display) |

## Risk Level Computation

When the Agent receives a token-scan response, compute the **effective risk level** as follows:

1. **Collect triggered labels**: Iterate all boolean fields. For each `true` value, record its risk level from the catalog above. For `isHasAssetEditAuth`, only count as Level 3 when `chainId == 501` (Solana).
2. **Evaluate tax thresholds**: Parse `buyTaxes` and `sellTaxes` as numbers. Map each to a risk level per the tax threshold table. If `null`, empty, or non-numeric, treat as unavailable — skip tax evaluation and do not display.
3. **Determine effective level**: Take the **maximum** risk level across all triggered labels and tax thresholds. This is the token's effective risk level. **Fallback**: If `isRiskToken: true` but the computed effective level is Level 1 (no individual boolean labels triggered AND no tax thresholds triggered), promote to Level 2 (info warning). In the fallback case, display: `[L2] Risk flagged by API (isRiskToken)` — no specific label identified. This preserves the API's composite judgment as a safety net.

> **Direction-agnostic tax rule**: Both `buyTaxes` and `sellTaxes` contribute to the effective level regardless of operation direction; the buy/sell distinction only affects the *action* (block vs. warn), not the level computation. Rationale: a high sell tax indicates the token may be functionally a honeypot (users can buy but cannot sell without heavy loss), so both taxes factor into the risk level regardless of direction.

> **`isRiskToken` note**: `isRiskToken` is a server-side composite flag that may incorporate signals not exposed as individual boolean fields (e.g., off-chain intelligence, ML models). The skill computes its own level from individual labels for explainability, and uses `isRiskToken` only as a fallback.

4. **Apply action matrix**: Use the effective risk level + operation type (buy/sell) to determine the Agent action per the matrix below.

## Risk Level Action Matrix

| Effective Level | Buy Action | Sell Action |
|---|---|---|
| **4** | `action: block` — Refuse to execute. Display: "This token has triggered [label names], posing critical risk. Buy blocked." | `action: warn` — Display risk labels, allow sell to continue. Display: "This token has triggered [label names], posing critical risk. Please trade with caution." |
| **3** | `action: warn` + **pause** — Display risk labels, halt execution, wait for explicit user confirmation (yes/no). Display: "This token has triggered [label names], posing high risk. Continue buying? (yes/no)" | `action: warn` — Display risk labels, allow sell to continue. |
| **2** | `action: warn` — Display risk labels as informational notice, do not pause. | `action: warn` — Display risk labels as informational notice, do not pause. |
| **1** (no labels triggered) | `action: ""` — Safe, proceed normally. | `action: ""` — Safe, proceed normally. |

### Determining Buy vs. Sell

- **Buy**: The target token (the token being received / `--to` in swap) is the one being scanned. User is acquiring this token.
- **Sell**: The source token (the token being spent / `--from` in swap) is the one being scanned. User is disposing of this token.
- **Standalone scan** (no swap context): Display all triggered labels with their risk levels. Do not apply buy/sell action logic — just present the risk assessment.

### Display Format

When reporting risk scan results to the user:

```
Token: <symbol or contract address> on <chain>
Risk Level: <CRITICAL|HIGH|MEDIUM|LOW> (Level <4|3|2|1>)
Triggered Labels:
  - [L4] Garbage Airdrop (isRubbishAirdrop)
  - [L3] Low Liquidity (isLowLiquidity)
  - [L3] Pump activity (isPump)
Buy Tax: <value>% | Sell Tax: <value>%    <!-- Omit entirely if both null; show only non-null if one is available -->
Action: <BLOCK / WARN — requires confirmation / WARN — info only / Safe>
```

> If symbol is unknown (e.g., raw address via Path 3), display the contract address instead, or look up the symbol via `onchainos token search` first.

## Edge Cases

| Scenario | Handling |
|---|---|
| `isChainSupported: false` | Skip detection. Append warning: "This chain does not support token security scanning." Do not block the trade. |
| API timeout / request failure | **Swap context**: Append warning: "Token security scan is temporarily unavailable. Please trade with caution." Continue flow (overrides general fail-safe). **Standalone context**: Follow the general fail-safe principle — ask user whether to retry or proceed. |
| All labels `false` and no tax risk | Level 1 — safe to proceed. |
| `buyTaxes`/`sellTaxes` is `null` | Tax data unavailable. Do not display tax info. Do not treat as risk. |

## Result Interpretation (Quick Reference)

| Effective Level | Agent Behavior |
|---|---|
| Level 4 | Block buy. Warn on sell. |
| Level 3 | Pause buy for confirmation. Warn on sell. |
| Level 2 | Info warning. Continue. |
| Level 1 | Safe. No action needed. |

## Suggest Next Steps

| Result | Suggest |
|---|---|
| Safe (Level 1) | 1. Swap the token 2. Check market data |
| Level 2 (info) | 1. Note risk labels 2. Swap with awareness |
| Level 3 (confirm) | 1. Review risk details 2. Decide whether to proceed |
| Level 4 (block buy) | Warn user. Do NOT suggest buying. If user holds the token, suggest selling. |

## Examples

**User says:** "Is PEPE safe to buy?" (token name, no address)

```
Agent workflow:
1. Search:  onchainos token search PEPE
   -> Returns multiple results across chains
2. Ask user: "I found these tokens matching 'PEPE':
   1. PEPE on Ethereum (0x6982508145454Ce325dDbE47a25d4ec3d2311933)
   2. PEPE on BSC (0x25d887Ce7a35172C62FeBFD67a1856F20FaEbB00)
   Which one do you want to check?"
3. User confirms: "The first one"
4. Scan:   onchainos security token-scan --tokens "1:0x6982508145454Ce325dDbE47a25d4ec3d2311933"
5. Display:
   Token: PEPE on Ethereum
   Risk Level: LOW (Level 1)
   Triggered Labels: None
   Buy Tax: 0%, Sell Tax: 0%
   Action: Safe to trade.
```

**User says:** "Is this token safe to buy?" (provides address directly)

```bash
onchainos security token-scan --tokens "1:0xdAC17F958D2ee523a2206206994597C13D831ec7"
# -> Display:
#   Token: USDT on Ethereum
#   Risk Level: LOW (Level 1)
#   Triggered Labels: None
#   Buy Tax: 0%, Sell Tax: 0%
#   Action: Safe to trade.
```

**Example: Multi-label risk token (from API response)**

```
API returns:
  isRubbishAirdrop: true    → Level 4
  isHasFrozenAuth: true     → Level 2
  isLowLiquidity: true      → Level 3
  isMintable: true           → Level 2
  isPump: true               → Level 3
  isRiskToken: true
  buyTaxes: null, sellTaxes: null

Effective risk level: Level 4 (max of 4, 3, 3, 2, 2)

Display:
  Token: <address> on Solana
  Risk Level: CRITICAL (Level 4)
  Triggered Labels:
    - [L4] Garbage Airdrop (isRubbishAirdrop)
    - [L3] Low Liquidity (isLowLiquidity)
    - [L3] Pump activity (isPump)
    - [L2] Mintable (isMintable)
    - [L2] Has Freeze Authority (isHasFrozenAuth)
  Action: BLOCK — buy is prohibited due to critical risk labels.
```

## Cross-Skill Workflow: Token Safety -> Swap -> TX Scan -> Broadcast

> User: "Is PEPE safe? If so, swap 1 ETH for it"

```
1. (okx-dex-token) onchainos token search PEPE      -> find contract address
2. Confirm which token with user
3. onchainos security token-scan --tokens "<chainId>:<addr>"
       -> compute effective risk level from all labels
       -> Level 4 buy: BLOCK — abort workflow, inform user
       -> Level 3 buy: PAUSE — show labels, wait for yes/no
       -> Level 2 buy: WARN — show labels, continue
       -> Level 1: safe, continue
4. If safe/confirmed: (okx-dex-swap) onchainos swap quote --from ... --to ... --chain ethereum
       -> get quote (price, impact, gas)
5. (okx-dex-swap) onchainos swap approve --token <fromToken> --amount <amount> --chain ethereum
       -> get approve calldata (skip if selling native token)
6. onchainos security tx-scan --chain ethereum --from <addr> --to <token_contract_address> --data <approve_calldata>
       -> check SPENDER_ADDRESS_BLACK, approve_eoa, phishing risks on the approve calldata
       -> If action is "block", or scan fails: STOP — do NOT execute approval, show risk details, abort workflow
       -> If action is "warn": show risk details, require explicit user confirmation before continuing
7. Execute approval (only if tx-scan passed):
   Path A (user-provided wallet): user signs approve calldata externally -> onchainos gateway broadcast
   Path B (Agentic Wallet):      onchainos wallet contract-call --to <token_contract_address> --chain 1 --input-data <approve_calldata>
8. (okx-dex-swap) onchainos swap execute --from ... --to ... --amount ... --chain ethereum --wallet <addr>
```
