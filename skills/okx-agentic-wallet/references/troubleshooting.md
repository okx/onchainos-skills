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

## Gas Station — Solana (`wallet send` / `wallet contract-call` with insufficient SOL)

Load `references/gas-station.md` for the end-to-end flow, verbatim product copy, and the scene matrix. This section covers failure modes and how the Agent should respond.

### Phase 1 response dispatch (boolean flags, not enum)

Dispatch on the boolean flag combination — there is no `gasStationStatus` enum on Solana.

| Detection | Scene | Agent / CLI response |
|---|---|---|
| `gasStationUsed=false` | NOT_APPLICABLE | Normal native-gas flow. Covers: native SOL transfer, Jito Bundle transaction, single-tx > 100,000 U, chain ≠ Solana, GS disabled + SOL sufficient. |
| `hasPendingTx=true` | Pending blocking | Render Edge Case 4 copy from `gas-station.md`. Do NOT auto-retry. |
| `insufficientAll=true` | Scene E | Render Scene E verbatim copy. Do NOT proceed. |
| `gasStationFirstTimePrompt=true` | Scene A (first-time) | Render Scene A verbatim copy. User picks → re-run with `--enable-gas-station [+ --gas-token-address --relayer-id]`. |
| `hash` non-empty + `autoSelectedToken=true` | Scene B / Scene D (silent auto) | CLI silently completes Phase 2 + sign + broadcast. Apply Universal Gas Station Success Reply. |
| `hash` empty + `gasStationFirstTimePrompt=false` + `insufficientAll=false` | Scene C (default insufficient) | Render Scene C verbatim copy. User picks alt token + (optional) replace default. |

### Special cases (Solana)

| Scenario | Detection | Agent response |
|---|---|---|
| Relayer single-tx cap exceeded (100,000 U) | Backend silently falls back to `gasStationUsed=false`. | Do NOT proactively explain. Only if the user directly asks why stablecoins cannot pay Gas for this transaction → render Edge Case 1 verbatim copy from `gas-station.md`. |
| Jito Bundle + stablecoin gas requested | Plugin / user supplies `--jito-unsigned-tx`, or user explicitly asks for both | **HARD BLOCK.** Render Edge Case 2 verbatim copy from `gas-station.md`. Never silently substitute. |
| Native SOL transfer | `contractAddr` empty → backend returns `gasStationUsed=false` | Gas Station only supports SPL transfers and contract interactions. If user asks, explain briefly. |
| GS transfer blocked by default-gas-token balance while another stablecoin is available | Phase 1 returns Scene C (default insufficient, another `sufficient=true`) | **Suggest switching the gas token first** via Scene C — zero-cost. If yes, re-run with `--gas-token-address <alt_addr> --relayer-id <id>` (no `--enable-gas-station`). The two echo templates in Scene C control whether default is replaced. |
| Native insufficient + stablecoin sufficient, but backend returned abnormal error | Empty error body or unexpected code, while stablecoin is available | Tell the user: "Your SOL balance is not enough, but your {token} balance can cover Gas via Gas Station. Enable it?" Then walk Scene A. |

### Phase 2 (after CLI fills in params)

| Edge Case | How to detect | Agent response |
|---|---|---|
| Backend rejects token selection | Non-2xx response or `gasStationUsed=false` with error | Tell user the selection failed; ask to retry. Causes: balance changed between calls, `relayerId` expired, token no longer supported. Re-run Phase 1 to refresh `gasStationTokenList`. |
| Invalid `gasTokenAddress` | Backend returns error | Do NOT fabricate. Re-run Phase 1 and use values from `next` field of the Confirming response. |
| Simulation failure (`executeResult=false`) | CLI bails with `transaction simulation failed: <msg>` | Show `<msg>` to user. Do NOT broadcast. Causes: insufficient token balance for `amount`, recipient invalid, program revert. |
| Balance changed between Phase 1 and Phase 2 | Phase 2 returns `insufficientAll` or simulation fails | Re-run Phase 1 to refresh `gasStationTokenList`. |
| `hash` empty on Phase 2 | Backend bug | Surface backend error. Do NOT attempt to sign. |
| `signType` ≠ `multiSignerTx` on a Gas Station response | Backend bug | Treat as fatal — CLI cannot construct the multi-signer transaction. Surface error. |

### Broadcast (asynchronous)

Gas Station broadcast is **asynchronous** — `txHash` returns "processing", actual chain status is eventual.

| Edge Case | How to detect | Agent response |
|---|---|---|
| Broadcast returns "processing" | Normal: `orderId` present, `txHash` empty | Use the Universal Gas Station Success Reply template (gas-station.md). Tell user to ask back later via `check order {orderId}`. |
| User asks for `txHash` before Relayer returns it | `txHash` empty, only `orderId` | Render Edge Case 3 verbatim copy from `gas-station.md`. Never fabricate a hash. Never show the raw CLI command. |
| User asks why txHash is slower than normal tx | After success | Render Edge Case 3 follow-up verbatim copy. One sentence is enough. |
| Relayer timeout (10-min TTL) | `wallet history` shows Failed | Render Edge Case 5 Outcome C copy: funds are intact, stablecoin not deducted, propose retry or SOL fallback. |
| Broadcast API-level error (code 81362) | Returned as Confirming with warning | Show the warning, ask the user to confirm. If confirmed, re-run with `--force`. |

### History display (post-broadcast)

| Issue | How to detect | Agent response |
|---|---|---|
| Gas fee shown in SOL instead of stablecoin | Should NOT happen — backend returns actual token used | If observed, report as a backend bug. Do NOT manually convert. |
| `from` shows Relayer address, not user | Should NOT happen — Per PRD, history only shows user intent and user address | Report as backend bug. |
| Tx hash not queryable right after broadcast | Expected due to async relay | "The Relayer is still submitting the transaction. Please check again shortly — tell me `check order {orderId}`." |
| Pending > 10 minutes | Tx state in history remains Pending | After 10-min Relayer TTL, backend auto-fails the tx. Tell user funds are intact and to retry. |

### Management commands

| Command | Failure mode | Agent response |
|---|---|---|
| `wallet gas-station update-default-token` | API error | Show the error message, do NOT retry automatically. Common causes: invalid token address, chain not supported, user not logged in. |
| `wallet gas-station disable` | API error | Show the error message, do NOT retry automatically. (Agent-internal: disable is DB-only; re-enabling later is instant — never paraphrase to the user. See `gas-station.md` User-Facing Reply Templates.) |
| `wallet gas-station enable` | API error | Show the error message, do NOT retry automatically. |

### Blocked scenarios (do NOT proactively mention Gas Station)

When any of these conditions hold, the backend returns `gasStationUsed=false` and the normal flow runs. Agent must NOT suggest enabling Gas Station in these cases:

- A previous Gas Station tx is still pending
- Transaction amount exceeds Relayer single-tx cap (100,000 U)
- User is sending via Jito Bundle
- Chain is not Solana
- Transfer is a native SOL transfer

If the user explicitly asks "why can't I use stablecoin?", render the matching Edge Case template from `gas-station.md`. Otherwise stay silent.

### Agent output vocabulary

See `gas-station.md` — "User Intent Recognition" MUST block for the authoritative vocabulary rules and ban list. Do not duplicate here.
