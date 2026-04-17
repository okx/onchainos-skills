# Gas Station — Detailed Reference

Gas Station enables paying gas fees with stablecoins (USDT/USDC/USDG) when the user lacks native tokens. It uses EIP-7702 to upgrade the wallet and a third-party Relayer to pay gas on behalf of the user.

**Supported chains**: Ethereum, BNB Chain, Base, Polygon, Arbitrum One, Optimism, X Layer
**Supported gas tokens**: USDT, USDC, USDG (USDG only on Ethereum and X Layer)

---

## Critical Rules

<MUST>
**Gas Station is fully automatic** — the Agent does NOT need to manually check native token balance or decide whether to use Gas Station. The backend makes this decision. The Agent's only job is to:
1. Call `wallet send` as normal
2. Handle the response according to the scenario table below
3. If a Confirming response is returned, show the message and ask the user to choose a token
</MUST>

<NEVER>
- **NEVER pass `--gas-token-address`, `--relayer-id`, or `--enable-gas-station` on the FIRST call** to `wallet send`. These are only for the second-phase call after the user has chosen a token from a Confirming response.
- **NEVER fabricate token addresses or relayer IDs** — always use the exact values from the Confirming response's `next` field (which contains the tokenList JSON).
- **NEVER proactively suggest Gas Station** when the user has not attempted a transaction. Gas Station activates only when `wallet send` detects insufficient native balance.
- **NEVER tell the user Gas Station is "free"** — there is a service charge paid in the selected stablecoin. Always display the `serviceCharge` + `serviceChargeSymbol` when present.
</NEVER>

---

## Flow (integrated into `wallet send`)

Gas Station is **not** a separate command — it activates automatically during `wallet send` when the backend detects insufficient native token balance. The flow uses the standard **Confirming Response** pattern (exit code 2).

### Step 1 — First `wallet send` call (no gas station params)

The backend decides whether to activate Gas Station. Six outcomes:

| Scenario | CLI Output | What to do |
|---|---|---|
| Not Gas Station | Normal flow: sign → broadcast → `{ txHash }` | Done |
| B/D: Auto-selected token | `{ txHash, gasStationUsed, serviceCharge, serviceChargeSymbol }` | Done — tell user: "Gas fee: {serviceCharge} {serviceChargeSymbol} (via Gas Station). Transaction submitted, check history for final status." |
| A: First-time prompt | **Confirming** (exit code 2): message explains Gas Station + shows available tokens | Show message to user, ask which token to use |
| C: Default token insufficient | **Confirming** (exit code 2): message shows alternatives | Show message to user, ask which token to use |
| E: All tokens insufficient | `{ gasStationUsed, insufficientAll, fromAddr }` | Tell user: "No sufficient balance for gas. Please top up at: {fromAddr}. Accepted: ETH, USDT, USDC." |
| Pending tx | `{ gasStationUsed, hasPendingTx }` | Tell user: "A transaction is still processing. Please wait for it to complete before sending another." |

### Step 2 — User chooses token (Confirming response handler)

Parse the `next` field from the Confirming response. It contains the token list with addresses and relayer IDs. Re-run the **same** `wallet send` command with additional params:

- **Scene A** (first-time enable): add `--gas-token-address <addr> --relayer-id <id> --enable-gas-station`
- **Scene C** (use alternative): add `--gas-token-address <addr> --relayer-id <id>`

### Step 3 — Second `wallet send` call completes

Sign 712 hash → broadcast → `{ txHash, serviceCharge, serviceChargeSymbol }`

<MUST>
After a successful Gas Station broadcast, always tell the user:
- The gas fee amount and token: "{serviceCharge} {serviceChargeSymbol}"
- That the transaction is submitted and they can check transaction history for the final status (Gas Station transactions are processed asynchronously by the Relayer)
</MUST>

---

## Confirming Response — How Agent Should Handle

When `wallet send` returns a **Confirming** response with Gas Station token selection:

1. **Display** the `message` to the user (it contains Gas Station explanation + available token list with balances and fees)
2. **Ask** the user which token to use for gas payment
3. **Re-run** the same `wallet send` command, appending the gas station flags from the `next` field:
   - `--gas-token-address` = the chosen token's `feeTokenAddress`
   - `--relayer-id` = the chosen token's `relayerId`
   - `--enable-gas-station` = only for first-time activation (Scene A)
4. If user declines, inform them they can top up native tokens instead
5. If user asks to set the chosen token as default for future transactions, additionally call `wallet gas-station update-default-token` after the transaction completes

---

## User Intent Recognition

Users may express Gas Station-related needs in various ways. The Agent should recognize these intents:

| User says (EN) | User says (CN) | Intent | Action |
|---|---|---|---|
| "I don't have ETH for gas" / "no gas" / "insufficient gas" / "can't afford gas" / "not enough for fees" | "没有 ETH" / "没有主网币" / "Gas 不够" / "手续费不够" / "没钱付 Gas" | Wants to send but lacks native token | Proceed with `wallet send` — Gas Station activates automatically |
| "Can I pay gas with USDC?" / "use stablecoin for gas" / "pay fee with USDT" / "pay network fee with stablecoin" | "可以用稳定币支付Gas吗" / "用 USDT 付手续费" / "能用 USDC 付 Gas 吗" / "稳定币付网络费" | Asks about Gas Station capability | Explain Gas Station, then proceed with the transaction if user provides one |
| "What is Gas Station?" / "how does gas station work" / "what is gas 加油站" / "explain gas station" | "什么是 Gas 加油站" / "加油站原理" / "Gas 加油站是什么" / "怎么用加油站" | FAQ | Answer from FAQ section below |
| "Change my default gas token" / "switch gas payment to USDC" / "set USDT as gas default" | "修改默认 Gas 代币" / "改成 USDC 付 Gas" / "把默认改成 USDT" / "换个代币付 Gas" | Change default | Call `wallet gas-station update-default-token` |
| "Revoke 7702" / "disable gas station" / "cancel 7702 upgrade" / "turn off gas station" / "stop using stablecoin for gas" | "撤销 7702" / "关闭加油站" / "取消 7702 升级" / "停用加油站" / "不用稳定币付 Gas 了" | Revoke 7702 | **First warn**: revoking disables Gas Station, re-enabling later triggers a new upgrade. Ask if they just want to change the default token instead. Only revoke if user confirms. |
| "Why can't I use stablecoin for gas?" / "gas station not working" / "why no gas station option" | "为什么不能用稳定币付 Gas" / "加油站怎么用不了" / "为什么没有加油站选项" | Blocked scenario inquiry | Check: pending tx? amount too large? unsupported chain? native token transfer? Explain the relevant reason. |

---

## Management Commands

| Command | Usage | Notes |
|---|---|---|
| Change default gas token | `onchainos wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>` | Takes effect on next Gas Station transaction |
| Revoke 7702 | `onchainos wallet gas-station revoke-7702 --chain <chain>` | Disables Gas Station. Requires native token balance. |

> For 7702 upgrade details, signing flow, revocation warnings, and edge cases: read `references/eip7702-upgrade.md`

---

## Edge Cases

<MUST>
Handle these edge cases explicitly — do NOT fall through to generic error handling:
</MUST>

| Edge Case | How to detect | Agent response |
|---|---|---|
| **Pending transaction** | `hasPendingTx: true` in response | "A previous transaction is still being processed. Please wait for it to complete before sending a new one. You can check the status with `wallet history`." |
| **All tokens insufficient** | `insufficientAll: true` in response | "None of your stablecoins have sufficient balance to pay gas. Please top up your wallet at: {fromAddr}. You can deposit ETH, USDT, or USDC." |
| **Relayer amount cap exceeded** | Backend silently falls back to normal flow (gasStationUsed=false). User may ask why. | "This transaction exceeds the Gas Station single-transaction limit (100,000 USD equivalent). Please use native tokens to pay gas for this transaction." — only explain if user asks. |
| **Native token transfer** | Backend returns gasStationUsed=false for transfers without contractAddr | Gas Station only works for ERC-20 token transfers, not native token (ETH/BNB) transfers. If user asks, explain this. |
| **Unsupported chain** | Backend returns gasStationUsed=false | Gas Station is only available on: Ethereum, BNB Chain, Base, Polygon, Arbitrum One, Optimism, X Layer. If user asks about other chains, list the supported chains. |
| **Gas Station tx result** | After broadcast, txHash is returned but result is async | Always remind: "Transaction submitted. Gas Station transactions are processed by a Relayer and may take a few minutes. Check `wallet history` for the final status." |
| **7702 upgrade / revocation issues** | See `references/eip7702-upgrade.md` Edge Cases | Load eip7702-upgrade.md for: upgrade in progress, third-party delegation, revoke with no native token |
| **User asks about gas fee after Gas Station tx** | In transaction history, gas fee shows in stablecoin | Display the gas fee in the actual token used (e.g. "Gas fee: 0.13 USDT"), not in native token. |

---

## FAQ

**Q: What is Gas Station?**

A: Gas Station aggregates third-party Relayer services, automatically compares rates, and pays gas on your behalf. You can use USDT, USDC, or USDG to pay gas fees without holding native tokens like ETH or BNB. Supported networks: Ethereum, BNB Chain, Base, Polygon, Arbitrum One, Optimism.

**Q: How does Gas Station work?**

A: 1) Your wallet is upgraded to a smart contract wallet via EIP-7702. 2) A third-party Relayer pays the native gas on-chain. 3) In the same transaction, your chosen stablecoin repays the Relayer.

**Q: Does the 7702 upgrade cost extra?**

A: Yes, but the one-time upgrade fee is included in the first Gas Station transaction — no separate payment needed. Each supported chain requires one upgrade on first use.

**Q: Which tokens can I use to pay gas?**

A: USDT, USDC, and USDG. USDG is only available on Ethereum and X Layer. The system prioritizes by balance (highest first), with ties broken by USDT > USDC > USDG.

**Q: Does each chain need a separate upgrade?**

A: Yes. Each supported network requires a one-time 7702 upgrade on first use of Gas Station on that chain.
