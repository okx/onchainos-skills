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

The backend decides whether to activate Gas Station and returns a `gasStationStatus` enum. Map each status to the corresponding action:

| gasStationStatus | CLI Output | What to do |
|---|---|---|
| `NOT_APPLICABLE` (or `gasStationUsed=false`) | Normal flow: sign → broadcast → `{ txHash }` | Done. Native token is sufficient or the transaction is outside Gas Station scope (native transfer, unsupported chain). No Gas Station messaging. |
| `READY_TO_USE` with `hash` non-empty (auto-path) | `{ txHash, orderId, gasStationUsed, autoSelectedToken, serviceCharge, serviceChargeSymbol }` | Done. Tell user: "Gas fee: {serviceCharge} {serviceChargeSymbol} (via Gas Station). Transaction submitted. Use orderId {orderId} to query status later." |
| `PENDING_UPGRADE` / `REENABLE_ONLY` (auto-path) | Same shape as above | Same as `READY_TO_USE` auto-path. The 7702 upgrade (if any) is embedded in the same broadcast. |
| `FIRST_TIME_PROMPT` | **Confirming** (exit code 2): message explains Gas Station + shows available tokens | Scene A — walk the 3-option decision tree (see Step 2). |
| `READY_TO_USE` with `hash` empty (Scene C) | **Confirming** (exit code 2): message shows alternative tokens | Scene C — walk the 2-question decision tree (see Step 2). |
| `INSUFFICIENT_ALL` | `{ gasStationUsed, insufficientAll, fromAddr }` | Tell user: "No sufficient balance for gas. Please top up at: {fromAddr}. Accepted: ETH (or native token of this chain), USDT, USDC." |
| `HAS_PENDING_TX` | `{ gasStationUsed, hasPendingTx }` | Tell user: "当前有一笔交易正在处理中，暂时无法通过 Gas 加油站支付 Gas。请等待该笔交易完成后重试。" Note: once Gas Station is enabled, ERC-20 transfers always route through it; topping up native tokens does NOT bypass the pending check. To proceed before the pending TTL expires, the user can disable Gas Station via `wallet gas-station disable`. |

### Step 2 — Skill orchestrates user decisions (Confirming response handler)

Parse the `next` field from the Confirming response. It contains the token list with addresses and relayer IDs. The Skill is responsible for walking the user through the decision tree and assembling the correct flags before re-invoking `wallet send`.

#### Scene A — FIRST_TIME_PROMPT (first-time enable)

Present **three options** to the user and walk the decision tree accordingly:

1. **Enable + pin a default gas token** → Show the token list, ask the user to pick one → Re-run `wallet send` adding `--enable-gas-station --gas-token-address <addr> --relayer-id <id>`. Future transactions will auto-use this token.
2. **Enable without a default** → Re-run `wallet send` adding only `--enable-gas-station`. Backend auto-picks the token with highest balance each time.
3. **Do NOT enable** → Do NOT re-invoke the CLI. Terminate the flow and tell the user: "Your native token balance is insufficient to pay gas. Please top up native token (ETH / BNB / MATIC / etc. for this chain) to `{fromAddr}` and try again." Do not push Gas Station further once the user declined.

Notes:
- In FIRST_TIME_PROMPT, picking a token in option 1 implicitly sets it as the default (backend writes to DB). There is no "use this token but don't save as default" semantics at first-time enable.
- `{fromAddr}` comes from the sender address of the current `wallet send` (the current account's EVM address on this chain).

#### Scene C — READY_TO_USE with default token insufficient

Gas Station is already enabled with a default, but the default token's balance is too low for this transaction. Ask two questions:

1. **Which alternative token should be used?** (Pick one from the list)

2. **Replace the default with the chosen token?**
   - **No, use only for this transaction** → Re-run adding `--gas-token-address <addr> --relayer-id <id>` (without `--enable-gas-station`)
   - **Yes, replace the default** → Same re-run as above, **additionally** after transaction completes call `wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>`

### Step 3 — Second `wallet send` call completes

Sign 712 hash → broadcast → `{ txHash, orderId, serviceCharge, serviceChargeSymbol, ... }`

<MUST>
After a successful Gas Station broadcast, always tell the user:
- The gas fee amount and token: "{serviceCharge} {serviceChargeSymbol}"
- The `orderId` returned by the broadcast (copy verbatim)

**When `txHash` is empty (relayer returns hash asynchronously — almost always empty on first response):**
- Explicitly tell the user: "The transaction is submitted on-chain. The relayer is paying gas asynchronously. Query the status later with the orderId."
- Offer two query paths:
  1. **Natural language** (keep asking inside this conversation): "Check the status of my last transaction" or "Query order {orderId}"
  2. **Direct CLI command** (so the user can run it in another window or outside the Skill):
     ```bash
     onchainos wallet history --chain <chain> --order-id <orderId>
     ```

**Purpose:** Prevent the user from thinking the transaction failed and resubmitting, and avoid locking the user into the current chat window.
</MUST>

---

## Confirming Response — How Agent Should Handle

When `wallet send` returns a **Confirming** response, identify which scene by the `message` content (Scene A talks about "first time enable", Scene C talks about "default token insufficient") and follow Step 2's decision tree for that scene.

General principles:

1. **Display** the `message` to the user verbatim (contains token list with balances and fees)
2. **Walk the decision tree** for the matching scene (see Step 2):
   - Scene A: three options — enable + pin default / enable without default / do NOT enable
   - Scene C: two questions — which alternative token? → replace the default?
3. **Assemble flags** based on user decisions:
   - `--gas-token-address` = chosen token's `feeTokenAddress` (only if user picked one)
   - `--relayer-id` = same token item's `relayerId` (paired with gasTokenAddress)
   - `--enable-gas-station` = only in Scene A (first-time activation)
4. **Re-run** the same `wallet send` with the assembled flags
5. **Scene A option 3 (do NOT enable)** → Do not re-invoke the CLI. Tell the user to top up native token to `{fromAddr}` and retry. Do not push Gas Station further.
6. **Scene C + user chose to replace default** → After the transaction completes, call `wallet gas-station update-default-token`

---

## User Intent Recognition

Users may express Gas Station-related needs in various ways. The Agent should recognize these intents:

<MUST>
**Agent output language convention**: Always use "**开启 Gas Station / 关闭 Gas Station**" (EN: "**enable / disable Gas Station**") when talking to users.

**NEVER say in output**: "撤销 7702"、"取消 7702 授权"、"revoke 7702"、"cancel 7702 upgrade"、"EIP-7702 revocation"、"7702 授权"、"授权".

Users may **input** any of these phrases — recognize them as intent, but **your response must translate to the unified vocabulary** (开启/关闭 Gas Station / enable/disable Gas Station). Do NOT expose 7702 as a separate concept to the user.
</MUST>

| User says (EN) | User says (CN) | Intent | Action |
|---|---|---|---|
| "I don't have ETH for gas" / "no gas" / "insufficient gas" / "can't afford gas" / "not enough for fees" | "没有 ETH" / "没有主网币" / "Gas 不够" / "手续费不够" / "没钱付 Gas" | Wants to send but lacks native token | Proceed with `wallet send` — Gas Station activates automatically |
| "Can I pay gas with USDC?" / "use stablecoin for gas" / "pay fee with USDT" / "pay network fee with stablecoin" | "可以用稳定币支付Gas吗" / "用 USDT 付手续费" / "能用 USDC 付 Gas 吗" / "稳定币付网络费" | Asks about Gas Station capability | Explain Gas Station, then proceed with the transaction if user provides one |
| "What is Gas Station?" / "how does gas station work" / "what is gas 加油站" / "explain gas station" | "什么是 Gas 加油站" / "加油站原理" / "Gas 加油站是什么" / "怎么用加油站" | FAQ | Answer from FAQ section below |
| "Change my default gas token" / "switch gas payment to USDC" / "set USDT as gas default" | "修改默认 Gas 代币" / "改成 USDC 付 Gas" / "把默认改成 USDT" / "换个代币付 Gas" | Change default | Call `wallet gas-station update-default-token` |
| "enable gas station" / "turn on gas station" / "open gas station" / "reactivate gas station" | "开启加油站" / "打开加油站" / "启用 Gas Station" / "重新开启 Gas Station" | **开启 Gas Station** (Enable) | **MUST ask the user which chain first** — the API requires `--chain`. Then call `wallet gas-station enable --chain <chain>`. This is a DB-flag flip only; it requires that the chain already has Gas Station delegated on-chain (from an earlier first-time enable). If the chain was never delegated, backend returns a message — relay that message to the user (e.g., "this chain hasn't been activated yet; please send an ERC-20 transaction first to go through the first-time activation"). Do NOT mention 7702 / delegation / authorization terms in the user-facing reply. |
| "disable gas station" / "turn off gas station" / "stop using stablecoin for gas" / "Revoke 7702" / "cancel 7702 upgrade" / "取消 gas station 授权" / "取消 gas station" | "关闭加油站" / "停用加油站" / "不用稳定币付 Gas 了" / "撤销 7702" / "取消 7702 升级" / "取消 Gas Station 授权" / "取消授权" | **关闭 Gas Station** (Disable) | **MUST ask the user which chain first** — the API requires `--chain`. Then call `wallet gas-station disable --chain <chain>`. **Respond using "关闭 Gas Station" / "disable Gas Station" only** — do NOT mention 7702, revoke, or authorization. Tell user: 关闭后切换回主网币支付 Gas，后续可随时重新开启。如果只是想换支付代币，建议用"修改默认 Gas 代币"。 |
| "Why can't I use stablecoin for gas?" / "gas station not working" / "why no gas station option" | "为什么不能用稳定币付 Gas" / "加油站怎么用不了" / "为什么没有加油站选项" | Blocked scenario inquiry | Check: pending tx? amount too large? unsupported chain? native token transfer? Explain the relevant reason. |
| "What's the tx hash?" / "hash for my last transaction" / "show me the hash" | "刚刚那笔交易的 Hash" / "查下 Hash" / "交易 Hash 是多少" | User wants the txHash while the relayer has not returned it yet | Reply: "The transaction is being confirmed on-chain. Please query again shortly." Then provide the natural-language template and the `wallet history --chain <chain> --order-id <orderId>` command. Never fabricate a hash. |
| "Why can I get hash immediately for other txs but not this one?" / "why is hash slow" | "为什么其他交易可以立刻返回 Hash" / "Hash 怎么这么慢" / "为什么拿不到 Hash" | User questions why the hash is delayed | Reply: "This transaction uses Gas Station, so the hash returns slightly later than a regular transaction." One sentence is enough — do not expand into relayer / 7702 technical details. |
| "Check my last transaction" / "transaction status" / "where's my tx" | "查下刚刚那笔交易" / "查看下刚刚那笔交易的交易历史" / "我的交易到哪里了" | User wants status of the recent transaction | Read the orderId from conversation context and call `wallet history --chain <chain> --order-id <orderId>`. Prefer orderId for the lookup; show the txHash only when it is returned. |

---

## Management Commands

| Command | Usage | Notes |
|---|---|---|
| Change default gas token | `onchainos wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>` | Takes effect on next Gas Station transaction |
| Enable Gas Station | `onchainos wallet gas-station enable --chain <chain>` | DB flag only. Requires 7702 delegation already on-chain (first-time enable must have been done previously via `wallet send`). If the chain was never delegated, backend returns a msg — surface it to the user. |
| Disable Gas Station | `onchainos wallet gas-station disable --chain <chain>` | DB flag only, no on-chain action. On-chain 7702 delegation preserved. |

> For 7702 upgrade details, signing flow, revocation warnings, and edge cases: read `references/eip7702-upgrade.md`

---

## Edge Cases

<MUST>
Handle these edge cases explicitly — do NOT fall through to generic error handling:
</MUST>

| Edge Case | How to detect | Agent response |
|---|---|---|
| **Pending transaction** | `hasPendingTx: true` in response | Use PRD wording: "当前有一笔交易正在处理中，暂时无法通过 Gas 加油站支付 Gas。请等待该笔交易完成后重试。" Do NOT silently re-invoke `wallet send` — the backend will block it again. Once Gas Station is enabled for an account/chain, every ERC-20 transfer is routed through Gas Station regardless of native-token balance; topping up native tokens does NOT bypass the pending check. To proceed before the pending TTL expires, the user can disable Gas Station via `wallet gas-station disable`. |
| **All tokens insufficient** | `insufficientAll: true` in response | "None of your stablecoins have sufficient balance to pay gas. Please top up your wallet at: {fromAddr}. You can deposit ETH, USDT, or USDC." |
| **Relayer amount cap exceeded** | Backend silently falls back to normal flow (gasStationUsed=false). User may ask why. | "This transaction exceeds the Gas Station single-transaction limit (100,000 USD equivalent). Please use native tokens to pay gas for this transaction." — only explain if user asks. |
| **Native token transfer** | Backend returns gasStationUsed=false for transfers without contractAddr | Gas Station only works for ERC-20 token transfers, not native token (ETH/BNB) transfers. If user asks, explain this. |
| **Unsupported chain** | Backend returns gasStationUsed=false | Gas Station is only available on: Ethereum, BNB Chain, Base, Polygon, Arbitrum One, Optimism, X Layer. If user asks about other chains, list the supported chains. |
| **Gas Station tx result** | After broadcast, txHash is returned but result is async | Always remind: "Transaction submitted. Gas Station transactions are processed by a Relayer and may take a few minutes. Check `wallet history` for the final status." |
| **7702 upgrade / disable issues** | See `references/eip7702-upgrade.md` Edge Cases | Load eip7702-upgrade.md for: upgrade in progress, third-party delegation, re-enable shortcut |
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
