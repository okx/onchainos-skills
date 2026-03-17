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
| `buyTaxes` | String | Buy tax percentage |
| `sellTaxes` | String | Sell tax percentage |
| `isRiskToken` | Boolean | Whether the token is high-risk |

## Result Interpretation

| Field | Value | Agent Behavior |
|---|---|---|
| `isChainSupported` | `false` | Chain not supported for scanning. Inform user, do not block trade. |
| `isRiskToken` | `false` | Low risk. Safe to trade. |
| `isRiskToken` | `true` | High risk. Block buy. Recommend avoiding. |

## Suggest Next Steps

| Result | Suggest |
|---|---|
| Safe (`isRiskToken: false`) | 1. Swap the token 2. Check market data |
| Risky (`isRiskToken: true`) | Warn user. Do NOT suggest buying. |

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
   Risk Level: LOW (1)
   Buy Tax: 0%, Sell Tax: 0%
   Verdict: Safe to trade.
```

**User says:** "Is this token safe to buy?" (provides address directly)

```bash
onchainos security token-scan --tokens "1:0xdAC17F958D2ee523a2206206994597C13D831ec7"
# -> Display:
#   Token: USDT on Ethereum
#   Risk Level: LOW (1)
#   Buy Tax: 0%, Sell Tax: 0%
#   Verdict: Safe to trade.
```

## Cross-Skill Workflow: Token Safety -> Swap -> TX Scan -> Broadcast

> User: "Is PEPE safe? If so, swap 1 ETH for it"

```
1. (okx-dex-token) onchainos token search PEPE      -> find contract address
2. Confirm which token with user
3. onchainos security token-scan --tokens "<chainId>:<addr>"
       -> check honeypot / high risk
4. If safe: (okx-dex-swap) onchainos swap quote --from ... --to ... --chain ethereum
       -> get quote (price, impact, gas)
5. (okx-dex-swap) onchainos swap approve --token <fromToken> --amount <amount> --chain ethereum
       -> get approve calldata (skip if selling native token)
6. onchainos security tx-scan --chain ethereum --from <addr> --to <token_contract_address> --data <approve_calldata>
       -> check SPENDER_ADDRESS_BLACK, approve_eoa, phishing risks on the approve calldata
       -> If action is "block", or scan fails: STOP — do NOT execute approval, show risk details, abort workflow
       -> If action is "warn": show risk details, require explicit user confirmation before continuing
7. Execute approval (only if tx-scan passed):
   Path A (user-provided wallet): user signs approve calldata externally -> onchainos gateway broadcast
   Path B (Agentic Wallet):      onchainos wallet contract-call --to <token_contract_address> --chain eth --input-data <approve_calldata>
8. (okx-dex-swap) onchainos swap swap --from ... --to ... --amount ... --chain ethereum --wallet <addr>
       -> get swap calldata (tx.to, tx.data, tx.value)
9. onchainos security tx-scan --chain ethereum --from <addr> --to <tx.to> --data <tx.data> --value <tx.value>
       -> check risk level
       -> If action is "block", or scan fails: STOP — do NOT execute swap, show risk details, abort workflow
       -> If action is "warn": show risk details, require explicit user confirmation before continuing
10. If safe or user confirmed after warn, execute swap:
   Path A (user-provided wallet): user signs externally -> onchainos gateway broadcast --signed-tx <tx> --address <addr> --chain ethereum
   Path B (Agentic Wallet):      onchainos wallet contract-call --to <tx.to> --chain eth --value <value_in_UI_units> --input-data <tx.data>
```
