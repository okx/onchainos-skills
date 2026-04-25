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

Load `references/gas-station.md` for the end-to-end flow and the authoritative `gasStationStatus` state matrix. This section covers failure modes and how the Agent should respond per status.

### First `unsignedInfo` call (Step 1) — per `gasStationStatus`

| gasStationStatus | Detection | Agent / CLI response |
|---|---|---|
| `NOT_APPLICABLE` | `gasStationUsed=false` + normal transfer fields (unsignedTxHash + unsignedTx) | Normal flow — no Gas Station needed, no special messaging. Covers: native-token transfer, unsupported chain, native sufficient + GS disabled. Note: once GS is enabled, a sufficient native balance does **not** cause a revert to NOT_APPLICABLE; the account stays on the GS path until explicitly disabled. |
| `FIRST_TIME_PROMPT` | Confirming (exit 2) + `gasStationFirstTimePrompt=true` + tokenList returned | Present one combined prompt — `1` decline / `2+` enable and use tokenList[N-2] to pay gas. Backend pins the picked token as the chain's default. `1`: do not re-run, tell the user balance is insufficient for gas at `{fromAddr}`. `N ≥ 2`: re-run `wallet send --enable-gas-station --gas-token-address <addr> --relayer-id <id>` with the picked token. **Never** call `wallet gas-station disable` as a follow-up — that is a different intent. See `gas-station.md` Step 2 Scene A. |
| `PENDING_UPGRADE` (Phase 1 diagnostic) | Confirming (exit 2); Phase 1 returns tokenList with hash empty (chain not yet delegated) | Same combined prompt as Scene A (`1` decline / `2+` enable and use this token). Phase 2 sends `--enable-gas-station --gas-token-address --relayer-id`; the response carries both transaction and activation signing material, which CLI signs together in one broadcast. See `gas-station.md` Step 2 Scene A'. |
| `REENABLE_ONLY` (DB disabled + chain delegated) | Phase 1 diagnostic (hash empty) | **Do NOT silently re-enable** — the user explicitly disabled Gas Station, so CLI returns Confirming and asks first. Same combined prompt shape as Scene A (`1` decline / `2+` re-enable and use this token). Backend overwrites the previous default with the picked token. Phase 2 response carries only transaction signing material. See `gas-station.md` Step 2 Scene B'. |
| `READY_TO_USE` + default matches + sufficient (Scene B) | `autoSelectedToken=true` + `hash` non-empty | **CLI silently completes** Phase 2 + sign + broadcast. Tell the user: "Gas fee: {serviceCharge} {serviceChargeSymbol} (via Gas Station). orderId {orderId}." |
| `READY_TO_USE` + default insufficient / multiple sufficient (Scene C) | Confirming (exit 2) + `hash` empty + `gasStationFirstTimePrompt=false` | Walk the 2-question dialog: (1) Pick alternative token; (2) Replace default? — No → re-run with `--gas-token-address --relayer-id` only; Yes → same re-run, then call `wallet gas-station update-default-token` after the tx completes. See `gas-station.md` Step 2 Scene C. |
| `INSUFFICIENT_ALL` | `insufficientAll=true` + `fromAddr` + all tokenList entries `sufficient=false` | Use the authoritative user message in `gas-station.md` Step 1 table `INSUFFICIENT_ALL` row. Do NOT proceed. |
| `HAS_PENDING_TX` | `hasPendingTx=true` + `gasStationUsed=true` + `autoSelectedToken=false` + `executeResult=true` (no `executeErrorMsg`) | Use the authoritative user message in `gas-station.md` Step 1 table `HAS_PENDING_TX` row. Do NOT auto-retry. |

### Special cases

| Scenario | Detection | Agent response |
|---|---|---|
| Relayer single-tx cap exceeded (100,000 U) | Backend silently falls back to `gasStationUsed=false` for that amount | Do not proactively explain. If the user asks "why can't I use stablecoin for this?": "This transaction exceeds the Gas Station single-transaction limit (100,000 U). Please use native tokens or split into multiple transactions." |
| Native insufficient + stablecoin sufficient, but `wallet send` returned abnormal error (empty error / unexpected code) | User has stablecoins on the same chain that could cover transfer + ~gas fee, but the GS dispatch glitched (backend error / CLI bug) | Tell the user: "Your native token is insufficient, but your {token} balance can pay gas via Gas Station. Want to enable it?" Do NOT default-suggest "top up native" first. Follow Step 2 Scene A. |
| Native insufficient + GS was previously disabled on this chain | User had run `wallet gas-station disable` earlier and now hits native-insufficient | Tell the user explicitly: "Gas Station is currently disabled on this chain. Re-enabling will let your stablecoin pay gas." Then walk Step 2 Scene A (or B' if backend returns REENABLE_ONLY). |
| GS transfer blocked by default-gas-token balance while another stablecoin is available on the same chain | Any scenario where the default gas token's balance is the obstacle (cannot cover gas fee, or too tight to cover `transferAmount + gasFee` in the same token), and the account holds another stablecoin on this chain. Also applies when the user explicitly asks "can I switch the gas stablecoin?". | **Suggest switching the gas token first.** Example: "You have {alt_token} {balance} available on this chain — switch the gas payment to {alt_token}?" If yes, run `wallet gas-station update-default-token --chain <chain> --gas-token-address <alt_addr>` then re-issue the transfer. If the user has no alternatives, the fallback is to reduce the transfer amount or top up. **Do not default-suggest "reduce amount" or "top up" when alternative stablecoins are available.** |

### Second `unsignedInfo` call (Step 2, after CLI fills in params)

Phase 2 request params vary by status (see the authoritative matrix in `gas-station.md`):
- `PENDING_UPGRADE` → `enableGasStation=true + gasTokenAddress + relayerId`; response carries transaction + activation signing material; CLI signs both.
- `REENABLE_ONLY` → `enableGasStation=true` + `gasTokenAddress` + `relayerId` (user-picked token; backend overwrites previous default); response carries only transaction signing material; CLI signs once.
- `READY_TO_USE` (Scene B/C) → `gasTokenAddress + relayerId` (no `enable`); response carries only transaction signing material; CLI signs once.

| Edge Case | How to detect | Agent response |
|---|---|---|
| Backend rejects token selection | Non-2xx response or `gasStationUsed=false` with error "Gas Station not activated by backend for this transaction" | Tell user the selection failed, ask them to retry. Possible causes: balance changed between calls, relayerId expired, token no longer supported. Re-run Step 1 to refresh `tokenList`. |
| Invalid `gasTokenAddress` | Backend returns error | Do NOT fabricate addresses. Rerun Step 1 and use values from `next` field of the Confirming response. |
| Simulation failure (`executeResult=false`) | CLI bails with `transaction simulation failed: <msg>` | Show `<msg>` to user. Do NOT broadcast. Common causes: insufficient token balance for the `amount`, recipient invalid, contract revert. |
| Balance changed between Step 1 and Step 2 | Second-call returns `insufficientAll` or simulation fails | Rerun Step 1 to get updated `tokenList`. Possible cause: another tx consumed the balance. |
| `hash` empty on second call | Parse error / backend bug | Surface backend error. Do NOT attempt to sign. |
| PENDING_UPGRADE Phase 2 missing `--enable-gas-station` | CLI sends `enableGasStation=false` + gasTokenAddress + relayerId; broadcast returns code=10004 (empty msg) | CLI bug — PENDING_UPGRADE must always set `enableGasStation=true`. Retry with the flag added. |
| broadcast `msgForSign` missing user712 signature | Only `authSignatureFor7702` + `sessionCert` + `sessionSignature`, no user712 signature; backend returns code=81358 "empty signedTx" | CLI bug — when the Phase 2 response includes `eip712MessageHash`, CLI must sign it and attach to `msgForSign`. Verify the branch at [transfer.rs:855](cli/src/commands/agentic_wallet/transfer.rs#L855) is executed. |

### Broadcast (Step 3, after signing)

Gas Station broadcast is **asynchronous** — `txHash` returns "processing", actual chain status is eventual.

| Edge Case | How to detect | Agent response |
|---|---|---|
| Broadcast returns "processing" | Normal: `orderId` present, `txHash` empty | Tell user: "Transaction submitted via Gas Station. Query status with `wallet history --chain <chain> --order-id <orderId>` in a few minutes." |
| User asks for `txHash` before broadcast completes | `txHash` empty, only `orderId` | Tell the user: "The transaction is being confirmed on-chain. Please ask me again in a moment — just say `check order {orderId}`." Never fabricate a hash. Never show the raw CLI command to the user. |
| User asks why txHash returns slower than normal tx | After success | Reply with one sentence: "This transaction uses Gas Station, so the hash returns slightly later than a regular transaction." Do not expand into relayer / on-chain-setup details. |
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
| `wallet gas-station disable` | API error | Show the error message, do NOT retry automatically. (Internal note, Agent-only: disable is DB-only; on-chain state is preserved, so re-enabling later is instant — **never paraphrase this to the user**. For the success wording see `gas-station.md` User-Facing Reply Templates.) |
| User confuses "disable" with "revoke 7702" | User says "revoke 7702" / "cancel authorization" or equivalent (in any language) | See `gas-station.md` User Intent Recognition row "disable gas station" for the handling, and User-Facing Reply Templates for the reply wording. |
| User wants to fully remove on-chain delegation | Not exposed by the current CLI | Explain: "The current CLI only disables Gas Station (switches this chain back to native-token gas). For deeper wallet cleanup, please use the main wallet portal." Do not invent a command. Never mention internal mechanism terms — see `gas-station.md` User Intent Recognition MUST block for the ban list. |

### Blocked scenarios (do NOT proactively mention Gas Station)

Per PRD, when any of these conditions hold, the backend returns `gasStationUsed=false` and the normal flow runs. Agent must NOT suggest enabling Gas Station in these cases:

- A previous Gas Station tx is still pending (7702 upgrade or regular)
- A prior EOA transaction is blocking 7702 upgrade slot on this chain
- Transaction amount exceeds Relayer single-tx cap (100,000 U)
- dApp interaction requires EIP-712 signature (not supported in Phase 1)
- Chain not in supported list
- Transfer is a native token transfer (ETH/BNB/etc.)

If the user explicitly asks "why can't I use stablecoin?", explain the matching reason. Otherwise stay silent.

### Agent output vocabulary

See `gas-station.md` — "User Intent Recognition" MUST block (above the intent table) for the authoritative vocabulary rules and ban list. Do not duplicate here.
