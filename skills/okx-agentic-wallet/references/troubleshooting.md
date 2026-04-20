# Wallet Troubleshooting

> Load this file when a wallet operation fails or an edge case is encountered.

## Edge Cases

### Send (D1)
- **Insufficient balance**: Check balance first. Warn if too low (include gas estimate for EVM).
- **Wrong chain for token**: `--contract-token` must exist on the specified chain.

### History (E)
- **No transactions**: Display "No transactions found" — not an error.
- **Detail mode without chain**: CLI requires `--chain` with `--tx-hash`. Ask user which chain.
- **Detail mode without address**: CLI requires `--address` with `--tx-hash`. Use current account's address.
- **Empty cursor**: No more pages.

### Contract Call (D2)
- **Missing input-data and unsigned-tx**: CLI requires exactly one. Command will fail if neither is provided.
- **Invalid calldata**: Malformed hex causes API error. Help re-encode.
- **Simulation failure**: Show `executeErrorMsg`, do NOT broadcast.
- **Insufficient gas**: Suggest `--gas-limit` for higher limit.

### Common (all sections)
- **Region restriction (error code 50125 or 80001)**: Do NOT show raw error code. Display: "Service is not available in your region. Please switch to a supported region and try again."
- **Not logged in** (`not logged in` error): Session expired or wallet store missing. Tell user to run `wallet login` + `wallet verify`.
- **Confirming response (exit code 2, error code 81362)**: Not an error — backend requires user confirmation. Display the `message` and follow instructions in `next`. Re-run with `--force` (or with Gas Station params) per the scenario.

---

## Gas Station (`wallet send` with insufficient native token)

Load `references/gas-station.md` for the end-to-end flow. This section covers failure modes and how the Agent should respond.

### First `unsignedInfo` call (Step 1)

| Edge Case | How to detect | Agent response |
|---|---|---|
| Pending Gas Station tx blocks new one | `gasStationUsed=true` + `hasPendingTx=true` | "A previous Gas Station transaction is still being processed. Please wait for it to finish, or top up native tokens to send via the normal flow. Check `wallet history` for status." Do NOT retry automatically. |
| All stablecoins insufficient | `gasStationUsed=true` + `insufficientAll=true` + `fromAddr` | "No stablecoin has enough balance to cover the gas fee. Please top up at: {fromAddr}. Accepted: ETH, USDT, USDC, USDG." Do NOT proceed. |
| Backend auto-selected (Scene B/D) | `gasStationUsed=true` + `hash` non-empty + `autoSelectedToken=true` | Silently broadcast. After success, tell user: "Gas fee: {serviceCharge formatted} {serviceChargeSymbol} (via Gas Station). Tx submitted, check history for final status." |
| First-time enable needed (Scene A) | Confirming (exit 2) + `gasStationFirstTimePrompt=true` | Show message + ask user to pick a token. On confirm, re-run with `--gas-token-address --relayer-id --enable-gas-station` (sets default). To enable without default, pass only `--enable-gas-station`. |
| Default token insufficient (Scene C) | Confirming (exit 2) + `gasStationFirstTimePrompt=false` | Show alternatives. Re-run with `--gas-token-address --relayer-id` (this tx only). If user wants to change default, additionally call `wallet gas-station update-default-token` after. |
| Relayer single-tx cap exceeded (100,000 U) | Backend silently returns `gasStationUsed=false` for this specific amount | Do NOT proactively explain. Only if user asks "why can't I use stablecoin for this?": "This transaction exceeds the Gas Station single-transaction limit (100,000 U). Please use native tokens or split into multiple transactions." |
| Unsupported chain | Backend returns `gasStationUsed=false` | Gas Station only supports Ethereum, BNB Chain, Base, Polygon, Arbitrum One, Optimism, X Layer. List these only if user asks. |
| Native token transfer | Backend returns `gasStationUsed=false` when no `contract-token` | Gas Station does not cover native token (ETH/BNB) transfers. Only ERC-20 transfers are supported. |
| Main-chain token sufficient | Backend returns `gasStationUsed=false` even with insufficient stablecoin | Normal flow — no Gas Station needed. No special message. |

### Second `unsignedInfo` call (Step 2, after user chose token)

| Edge Case | How to detect | Agent response |
|---|---|---|
| Backend rejects token selection | Non-2xx response or `gasStationUsed=false` with error "Gas Station not activated by backend for this transaction" | Tell user the selection failed, ask them to retry. Possible causes: balance changed between calls, relayerId expired, token no longer supported. Re-run Step 1 to refresh `tokenList`. |
| Invalid `gasTokenAddress` | Backend returns error | Do NOT fabricate addresses. Rerun Step 1 and use values from `next` field of the Confirming response. |
| Simulation failure (`executeResult=false`) | CLI bails with `transaction simulation failed: <msg>` | Show `<msg>` to user. Do NOT broadcast. Common causes: insufficient token balance for the `amount`, recipient invalid, contract revert. |
| Balance changed between Step 1 and Step 2 | Second-call returns `insufficientAll` or simulation fails | Rerun Step 1 to get updated `tokenList`. Possible cause: another tx consumed the balance. |
| `hash` empty on second call | Parse error / backend bug | Surface backend error. Do NOT attempt to sign. |

### Broadcast (Step 3, after signing)

Gas Station broadcast is **asynchronous** — `txHash` returns "processing", actual chain status is eventual.

| Edge Case | How to detect | Agent response |
|---|---|---|
| Broadcast returns "processing" | Normal | Tell user: "Transaction submitted via Gas Station. Check `wallet history` in a few minutes for final status." |
| User asks for `txHash` before broadcast completes | Before response returned | "Transaction is being processed on-chain. Please check back in a moment." Do NOT invent a hash. |
| User asks why txHash returns slower than normal tx | After success | "Gas Station transactions are routed through a Relayer, so the hash returns slightly later than a normal direct broadcast." |
| Relayer timeout (10-min TTL) | `wallet history` shows Failed status with Relayer timeout reason | "This Gas Station transaction did not complete within the 10-minute relay window. Your funds are safe — the stablecoin was not spent. Please retry or top up native tokens." |
| 7702 upgrade revert during first Gas Station tx | History shows Failed; cannot distinguish upgrade vs execute from response | "The first-time Gas Station transaction failed during on-chain execution. Your funds are intact. Please retry; if it persists, report with the txHash." See `references/eip7702-upgrade.md`. |
| Broadcast API-level error (code 81362) | Returned as Confirming with warning | Show warning, ask user to confirm. If confirmed, re-run with `--force`. |

### History display (post-broadcast)

| Issue | How to detect | Agent response |
|---|---|---|
| Gas fee shown in ETH instead of stablecoin | Should NOT happen — backend returns actual token | If observed, report as a backend bug. Do NOT manually convert. |
| `from` shows Relayer address, not user | Should NOT happen — backend uses user's address | Report as backend bug. Never tell user the Relayer address is theirs. |
| Tx hash not queryable right after broadcast | Expected due to async relay | "The Relayer is still submitting the transaction. Use `wallet history --order-id <orderId>` as a fallback." |
| Pending > 10 minutes | Tx state in history remains Pending | After 10-min Relayer TTL, backend auto-fails the tx. Tell user their funds are intact and to retry. |

### Management commands

| Command | Failure mode | Agent response |
|---|---|---|
| `wallet gas-station update-default-token` | API error | Show the error message, do NOT retry automatically. Common causes: invalid token address, chain not supported, user not logged in. |
| `wallet gas-station disable` | API error | Show the error message, do NOT retry automatically. Note: disable is DB-only; on-chain 7702 delegation is preserved, so re-enabling later is instant (no new upgrade). |
| User confuses "disable" with "revoke 7702" | User says "撤销 7702" / "revoke 7702" | Agent output must translate to "**关闭 Gas Station** / disable Gas Station". NEVER use "撤销", "revoke", "7702", "授权" in your response. Tell user: "关闭后切换回主网币支付 Gas，随时可重新开启 (链上委托保留，无需重新升级)。" |
| User wants to fully remove on-chain 7702 delegation | PRD does not expose this operation in current scope | Explain: "当前仅支持关闭 Gas Station（DB 开关）。完全撤销链上 7702 委托需通过钱包主端。" Do NOT attempt to invent a command. |

### Blocked scenarios (do NOT proactively mention Gas Station)

Per PRD, when any of these conditions hold, the backend returns `gasStationUsed=false` and the normal flow runs. Agent must NOT suggest enabling Gas Station in these cases:

- A previous Gas Station tx is still pending (7702 upgrade or regular)
- A prior EOA transaction is blocking 7702 upgrade slot on this chain
- Transaction amount exceeds Relayer single-tx cap (100,000 U)
- dApp interaction requires EIP-712 signature (not supported in Phase 1)
- Chain not in supported list
- Transfer is a native token transfer (ETH/BNB/etc.)

If the user explicitly asks "why can't I use stablecoin?", explain the matching reason. Otherwise stay silent.

### Agent output vocabulary (critical)

<MUST>
When responding about Gas Station disable / 7702 revocation, the **output** must use only:
- 中文: "开启 / 关闭 Gas Station"
- English: "enable / disable Gas Station"

**NEVER output** to the user: "撤销 7702", "取消 7702 授权", "revoke 7702", "cancel 7702 upgrade", "EIP-7702", "7702 升级", "授权", "委托".

Users may **input** any of these phrases — recognize them as intent, but translate your response to the unified vocabulary. 7702 is an internal implementation detail, not user-facing terminology. Exception: if user directly asks "什么是 7702", brief explanation is allowed but note it's an internal mechanism.
</MUST>
