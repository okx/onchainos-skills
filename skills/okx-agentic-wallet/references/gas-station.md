# Gas Station — Detailed Reference

Gas Station enables paying gas fees with stablecoins (USDT/USDC/USDG) when the user lacks native tokens. It uses EIP-7702 to upgrade the wallet and a third-party Relayer to pay gas on behalf of the user.

**Supported chains (12 EVM chains)**: Ethereum, BNB Smart Chain (BSC), Base, Polygon, Arbitrum One, Optimism, Linea, Monad, Scroll, Sonic, Conflux eSpace, X Layer.

Gas Station is **EVM-only**. Non-EVM chains (Solana, Sui, Bitcoin, Ton, Tron, …) are **not supported** — never list them as supported even if the user mentions them.

**Supported gas tokens per chain**:

| Chain | USDT | USDC | USDG |
|---|---|---|---|
| Ethereum | ✓ | ✓ | ✓ |
| BNB Smart Chain (BSC) | ✓ | ✓ | |
| Base | ✓ | ✓ | |
| Polygon | ✓ | ✓ | |
| Arbitrum One | ✓ | ✓ | |
| Optimism | ✓ | ✓ | |
| Linea | ✓ | ✓ | |
| Monad | ✓ | ✓ | |
| Scroll | ✓ | ✓ | |
| Sonic | | ✓ | |
| Conflux eSpace | ✓ | ✓ | |
| X Layer | ✓ | ✓ | |

Always derive the actual set from `gasStationTokenList` in the Phase 1 response (the backend-authoritative list per chain). The table above is a reference for FAQ replies.

**Supported scenarios**: all on-chain transactions — native / ERC-20 transfer, contract calls (approve, DEX swap, DeFi supply / borrow / redeem / claim, cross-chain bridge, and any other contract interaction). Gas Station activates automatically during any of these when native token is insufficient.

**Scope**: every Gas Station state (enable flag, default gas token, and the internal setup) is scoped to `(account, chain)`. Enabling / disabling / switching the default on one chain does not affect any other chain; each chain requires its own first-time activation and carries its own default.

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
- **NEVER proactively push Gas Station as a feature** when the user is just browsing or asking unrelated questions.
- **NEVER tell the user Gas Station is "free"** — there is a service charge paid in the selected stablecoin. Always display the `serviceCharge` + `serviceChargeSymbol` when present.
- **NEVER expose implementation detail to the user** — see "User Intent Recognition" section below for the authoritative output-vocabulary ban list. User-facing text is only about "enable / disable Gas Station" and "which stablecoin to pay gas".
</NEVER>

<MUST>
**Always surface Gas Station when native is insufficient but a stablecoin on the same chain has enough balance to cover the transfer + gas fee**, whether the user is attempting a transfer or asking why a transfer failed. This is the single most valuable suggestion the Agent can make:

- On `wallet send` success-but-insufficient-native: the backend triggers the GS flow (see the status matrix above); CLI handles it automatically or via a Confirming. The Agent must present GS as the primary path, not as one of several equivalent options.
- On `wallet send` failure with an abnormal error (empty error body / non-standard error code / backend glitch) while the user has stablecoin on the chain: tell the user explicitly "Your native token is insufficient, but your {token} balance can pay gas via Gas Station. Want to enable it?" and then follow Step 2 Scene A.
- On the Agent ranking solutions: "enable Gas Station with your stablecoin" comes **first**, "top up native token" comes after, "switch account / switch chain" comes last. Do not present top-up-native as the default when stablecoins are available.
- If Gas Station is already disabled on this chain (confirmed via a prior `wallet gas-station disable`), mention it: "Gas Station is currently disabled on this chain — re-enabling will let your stablecoin pay gas." Do not silently skip this context.
- **Whenever a GS transfer fails or is blocked due to the default gas token's balance, and the account holds other stablecoins on the same chain**: propose switching the gas token before any other fallback. Use `wallet gas-station update-default-token --chain <chain> --gas-token-address <alt_addr>` to change the default, then re-issue the original transfer. This includes: default token short for gas fee, default token balance tight for `transferAmount + gasFee`, user manually asking "can I switch the gas stablecoin?", etc. **Switching gas token is a zero-cost change** — always prefer it over "reduce transfer amount" or "top up default token" when alternatives are available.
</MUST>

---

## Flow (integrated into `wallet send`)

Gas Station is **not** a separate command — it activates automatically during `wallet send` when the backend detects insufficient native token balance. The flow uses the standard **Confirming Response** pattern (exit code 2).

### Authoritative State Matrix (`gasStationStatus`)

`gasStationStatus` is the authoritative enum. The backend returns one of seven values indicating the combined **user (DB) × chain (7702 delegation)** state. CLI / Agent MUST dispatch on this enum — do not infer from other fields.

| Status | Precondition (DB × chain) | CLI request params | Response key fields |
|---|---|---|---|
| `NOT_APPLICABLE` | Not a GS-eligible transaction (native transfer, unsupported chain, native sufficient + GS disabled) | Normal transfer fields (no GS params) | `unsignedTxHash` + `unsignedTx` |
| `FIRST_TIME_PROMPT` | DB no record + chain not delegated | Default request — no `enableGasStation` | `gasStationTokenList` (hash empty) |
| `PENDING_UPGRADE` | Chain not delegated (DB in any state: no record / disabled / enabled) | `enableGasStation=true` + `gasTokenAddress` + `relayerId` | `hash(712)` + `authHashFor7702` + `user712Data` + `user7702Data` + `contractNonce` + `eoaNonce` |
| `REENABLE_ONLY` | DB disabled + chain delegated (DB flip only, no on-chain action) | `enableGasStation=true` + `gasTokenAddress` + `relayerId` (user-picked token; backend overwrites previous default) | `hash(712)` + `contractNonce` (**does NOT return** `authHashFor7702`) |
| `READY_TO_USE` | DB enabled + chain delegated (steady-state normal operation) | Default request — no `enableGasStation` | `hash(712)` + `contractNonce` |
| `INSUFFICIENT_ALL` | All tokens below required balance | Ask user to top up — do not proceed | `gasStationTokenList` (all entries with `sufficient=false`) |
| `HAS_PENDING_TX` | A pending tx is blocking | Wait for the pending tx to clear, then retry | `gasStationStatus=HAS_PENDING_TX` + `hasPendingTx=true` + `gasStationUsed=true` + `autoSelectedToken=false` + `executeResult=true` (no `executeErrorMsg`) |

Note: The full Response fields for `PENDING_UPGRADE` / `REENABLE_ONLY` appear on the Phase 2 call (after CLI sends the corresponding params). The initial diagnostic call (no params) returns only `gasStationTokenList` with hash empty; CLI must re-issue with the right params.

### Phase 1 Response Fields (dispatch reference)

Phase 1 = CLI's initial diagnostic request (no GS params). The backend returns these fields; CLI / Agent dispatches on them to decide the next move.

| Field | One-line description |
|---|---|
| `gasStationStatus` | Authoritative state enum; primary dispatch key (one of the seven values in the state matrix above). |
| `gasStationUsed` | Whether this tx path uses Gas Station; `false` means fall back to normal transfer, ignore the rest of GS fields. |
| `hash` (= `eip712MessageHash`) | Phase 2 signing hash; **non-empty** = backend has a token picked and CLI can silently complete (Scene B auto), **empty** = Confirming needed (user picks token). |
| `authHashFor7702` | On-chain activation hash; present only on `PENDING_UPGRADE` Phase 2 response. CLI must sign both this and `hash` for broadcast. |
| `autoSelectedToken` | Whether backend auto-selected a token; `true` + `hash` non-empty = Scene B auto path; `false` + `hash` empty = Scene C (user picks). |
| `defaultGasTokenAddress` | Current default gas token on this chain (if any); used by the Agent to tell the user "your default is X but its balance is short". |
| `gasStationTokenList[*].feeTokenAddress` + `relayerId` | Per-token address + relayer id — the authoritative values to pass back as `--gas-token-address` / `--relayer-id` in Phase 2. Never fabricate. |
| `gasStationTokenList[*].sufficient` | Whether this token can cover `transferAmount + gasFee`; drives Scene C options and identifies `INSUFFICIENT_ALL` (all false). |
| `serviceCharge` / `serviceChargeSymbol` | Gas service fee amount + token symbol; the Agent must surface this to the user on every successful GS broadcast. |
| `hasPendingTx` | `true` → enter `HAS_PENDING_TX` branch, bail (do NOT auto-retry). |
| `insufficientAll` | `true` → enter `INSUFFICIENT_ALL` branch, ask user to top up. |
| `fromAddr` | User's address on this chain; used in top-up messaging. |
| `contractNonce` / `eoaNonce` | Signing nonces — CLI internal, not user-facing. |

**Dispatch order**: always start from `gasStationStatus`. In degraded environments where the enum is missing, fall back to field combinations: `hasPendingTx` → `insufficientAll` → `hash` empty/non-empty → `autoSelectedToken`.

### Step 1 — First `wallet send` call (no gas station params)

CLI issues the default request (no GS params); the backend returns a diagnostic response. CLI / Agent dispatches on the `gasStationStatus` enum:

| gasStationStatus | CLI local behavior | Agent output |
|---|---|---|
| `NOT_APPLICABLE` (or `gasStationUsed=false`) | Normal flow: sign → broadcast → `{ txHash }` | Done. No Gas Station messaging. |
| `READY_TO_USE` + default matches tokenList + sufficient (Scene B — default path) | **CLI silently completes Phase 2** (re-issues with default token + signs + broadcasts) | "Gas fee: {serviceCharge} {serviceChargeSymbol} (via Gas Station). Transaction submitted. Use orderId {orderId} to query status later." |
| `READY_TO_USE` + **no default** + exactly 1 sufficient token (Scene B — unambiguous fallback) | Same as above — picks the single sufficient token for Phase 2 | Same. Unambiguous auto-path; non-interactive callers (plugins) do not bail. |
| `READY_TO_USE` + **default present but insufficient** (Scene C) | **Confirming** (exit code 2): message shows alternative tokens | Walk the 2-question decision tree (see Step 2 Scene C). |
| `READY_TO_USE` + **no default** + multiple sufficient tokens (Scene C) | **Confirming** (exit code 2): message shows alternative tokens | Same as above. |
| `FIRST_TIME_PROMPT` | **Confirming** (exit code 2): message explains Gas Station and shows the tokenList | Present the combined option list (see Step 2 Scene A): `1` decline / `2+` enable and use this token. Backend pins the picked token as the chain's default. |
| `PENDING_UPGRADE` | **Confirming** (exit code 2): same combined option list as Scene A (`1` decline / `2+` enable and use this token) | See Step 2 Scene A'. Phase 2 re-run includes `--enable-gas-station --gas-token-address --relayer-id`; backend returns signing material for both the transaction and on-chain wallet activation, which CLI signs together in one broadcast. |
| `REENABLE_ONLY` | **Confirming** (exit code 2): the user previously disabled Gas Station, so their explicit intent must be respected — CLI returns Confirming asking whether to re-enable | See Step 2 Scene B'. Same shape as Scene A (`1` decline / `2+` re-enable and use this token). Backend overwrites previous default with the picked token. No on-chain action; Phase 2 carries only transaction signing material. |
| `INSUFFICIENT_ALL` | Bail | **(authoritative user message for this state)** "None of your stablecoins have enough balance to cover the gas fee. Please top up your wallet at: `{fromAddr}`. Accepted deposits: the chain's native token (ETH / BNB / MATIC / etc.), USDT, USDC, USDG." Do NOT proceed. |
| `HAS_PENDING_TX` | Bail | **(authoritative user message for this state)** "There is a Gas Station transaction still processing, which is blocking this new one. Say `check order {orderId}` (with the previous transaction's orderId) or `check my recent transactions` to see its status — once it clears you can retry." Do NOT auto-retry — the backend will block again. Note: once Gas Station is enabled, every ERC-20 transfer routes through GS regardless of native-token balance; topping up native does not bypass the pending check. |

### Step 2 — Skill orchestrates user decisions (Confirming response handler)

Parse the `next` field from the Confirming response. It contains the token list with addresses and relayer IDs. The Skill is responsible for walking the user through the decision tree and assembling the correct flags before re-invoking `wallet send`.

#### Scene A — FIRST_TIME_PROMPT (first-time enable)

Present a **single flat option list**: one "decline" option plus one "enable and use this token" option per `sufficient=true` entry in `gasStationTokenList`. The user completes the whole decision in one pick. Backend pins the chosen token as the chain's default on first-time selection — there is no "just this time" option.

**Step 1 — Route to Gas Station**

Backend has already signaled this is a GS-eligible path (status = `FIRST_TIME_PROMPT`). No prompt; proceed to Step 2.

**Step 2 — Enable and pick a token**

<MUST>
**User-facing template (Scene A — verbatim). Do NOT paraphrase the body text. Only substitute the bracketed slots.** Translate the template to the user's language at output time; the structure and every sentence stays.

```
Your {native_symbol} balance isn't enough to pay gas. You have two ways to proceed:

1. Top up {native_symbol} and pay gas with the native token.
2. Enable Gas Station to pay gas directly with a stablecoin.

About Gas Station: Gas Station aggregates third-party Relayer services, automatically compares rates and picks the best, and pays gas on your behalf. You can pay with USDT, USDC, or USDG — no need to hold {native_symbol}, BNB, or other native tokens. Learn more: https://web3.okx.com/learn/wallet-gas-station
- Once enabled, when your native balance is insufficient the system will automatically pay gas with a stablecoin — no further confirmation needed.
- By default the system uses the stablecoin with the highest balance. You can also pin a specific token as the default gas token for each transaction. Stablecoins supported on this chain: {supported_tokens_on_chain}.

Stablecoins supported on {chain_display_name}: {supported_tokens_on_chain}.

Do you confirm enabling Gas Station and paying this transaction's gas with a stablecoin?
```

**User response parsing (natural language, not numbered picks)**

Users reply conversationally — parse what they said, don't force them into an option number:

| User says (examples) | Interpretation | CLI action |
|---|---|---|
| "No", "取消", "先不开启", "算了充 {native_symbol} 吧" | Decline | Do NOT re-run. Tell user balance is insufficient for gas, ask them to top up {native_symbol} at `{fromAddr}`. Terminate. |
| "Yes" / "开启" / "确认开启" (no specific token named) | Confirm, no explicit default | Re-run `wallet send --enable-gas-station --gas-token-address <addr> --relayer-id <id>` using the **first `sufficient=true` entry** from `gasStationTokenList` (backend-ordered). That token gets pinned as default. |
| "确认，预设 USDT 作为 Gas" / "yes, use USDT" / "use {token} as default" | Confirm + pin specific token | Re-run with that token's `feeTokenAddress` / `relayerId`. Pinned as default. |
| Ambiguous or names a token not in tokenList | Ask to clarify | Re-prompt once with the token list. Do not guess. |

**Slot fills**
- `{chain_display_name}`: full chain name (Ethereum, BNB Chain, Base, etc.).
- `{native_symbol}`: the chain's native token symbol (ETH, BNB, MATIC, etc.).
- `{supported_tokens_on_chain}`: comma-separated symbols of every entry in `gasStationTokenList` (regardless of `sufficient`), reflecting what this chain supports (e.g. "USDT, USDC, USDG" on Ethereum; "USDC" on Sonic EVM).
- `{fromAddr}`: the user's address on this chain.

**Never** modify the template body. Never drop the academy link. Never drop the two bullets. Never drop the supported-tokens line. Never reduce the prompt to a bare "yes/no?" without the education paragraph.
</MUST>

**Post-success echo template (verbatim after the Phase 2 broadcast returns successfully)**

<MUST>
After `wallet send` completes successfully via Gas Station in Scene A, the Agent MUST echo back a confirmation to the user using this template (translated to the user's language, structure verbatim):

```
Gas Station enabled. This transaction will use {chosen_token} to pay gas. Future transactions on {chain_display_name} will automatically use {chosen_token} when the native balance is low — no further prompt.
```

Slot fills: `{chosen_token}` = the stablecoin the user picked (symbol); `{chain_display_name}` = full chain name. If the user did not name a token and the system auto-picked the first sufficient entry, substitute that token's symbol.

**Never** drop the "automatically use … no further prompt" sentence — it sets the right expectation so the user isn't surprised by the silent behavior next time.
</MUST>

**Key semantics**

| Intent | CLI effect | Persistent state change |
|---|---|---|
| Pick a stablecoin at Step 2 | Enables Gas Station + pins the picked token as this chain's default | Gas Station ON, default = picked token |
| Disable Gas Station (separate command) | Flips Gas Station OFF on the chain | Gas Station OFF |

**Notes**

- At least one `sufficient=true` token is required for Step 2. If `gasStationTokenList` has none sufficient, it's actually `INSUFFICIENT_ALL` — route to that handler, do not present Scene A.
- **Never surface** internal mechanism terms to the user — see "User Intent Recognition" MUST block for the ban list. Speak only in terms of "enable Gas Station" and "which stablecoin to pay gas".

#### Scene A' — PENDING_UPGRADE (chain not yet delegated, diagnostic returned Confirming)

State meaning: the DB may have no record, be disabled, or be enabled, but the on-chain delegation required for Gas Station has not been completed. The difference from FIRST_TIME_PROMPT is that the DB may already carry a record (e.g., the user enabled GS before and then disabled, or enabled but the on-chain step never succeeded); this ERC-20 transfer is the first one to trigger the on-chain setup.

For the user, the first on-chain activation is an irreversible action, so it **cannot be done silently**. CLI returns Confirming; the Agent presents the **same single combined option list as Scene A** (`1` decline / `2+` enable and use this token, one option per `sufficient=true` entry). Phrase everything in terms of "enable Gas Station" and "which stablecoin to pay gas". Never mention on-chain delegation / upgrade / signing to the user.

After the user's pick, re-run `wallet send` with `--enable-gas-station --gas-token-address <addr> --relayer-id <id>`. Backend pins the picked token as this chain's default.

Signing note for the CLI maintainer (not user-facing): the Phase 2 response carries both `eip712MessageHash` (the transaction hash) and `authHashFor7702` (the activation hash). Both must be signed and attached to the broadcast request; omitting either causes the backend to reject the broadcast.

#### Scene B' — REENABLE_ONLY (DB disabled + chain already delegated)

State meaning: the user enabled Gas Station previously and completed the on-chain step, later ran `wallet gas-station disable` to flip the DB flag off, and is now sending an ERC-20 transfer with insufficient native token again.

**The user explicitly turned Gas Station off** — do NOT silently re-enable. CLI MUST return Confirming and ask the user first. Flow uses the **same single combined option list as Scene A**, worded to surface the fact that Gas Station was turned off previously. Backend overwrites the previous default with the picked token on re-enable.

**Step 2 — Re-enable and pick a token**

<MUST>
**User-facing template (Scene B' — verbatim). Do NOT paraphrase the body text. Only substitute the bracketed slots.** Translate the template to the user's language at output time; the structure and every sentence stays.

```
Gas Station is currently disabled on {chain_display_name}, so your {native_symbol} balance isn't enough to pay gas. You have two ways to proceed:

1. Top up {native_symbol} and pay gas with the native token (keep Gas Station disabled).
2. Re-enable Gas Station to pay gas with a stablecoin again.

Your previous default gas token was {prev_token_symbol_or_none}.
- Re-enabling with a different stablecoin will overwrite the previous default.
- Once re-enabled, when your native balance is insufficient the system will automatically use the picked stablecoin — no further confirmation needed.

Stablecoins supported on {chain_display_name}: {supported_tokens_on_chain}.

Do you confirm re-enabling Gas Station and paying this transaction's gas with a stablecoin?
```

**User response parsing (natural language)**

| User says (examples) | Interpretation | CLI action |
|---|---|---|
| "No", "不开启", "先充 {native_symbol}" | Decline | Do NOT re-run. Tell user to top up {native_symbol} at `{fromAddr}` (or switch chain / account). Terminate. |
| "Yes" / "确认再开" (no token named) | Re-enable, keep the previous default if any | Re-run `wallet send --enable-gas-station --gas-token-address <prev> --relayer-id <prev_relayer>` using the previous default's row from `gasStationTokenList` if `sufficient=true`; otherwise fall back to the first `sufficient=true` entry. Surface which token was used in the success reply. |
| "确认，用 USDT" / "yes, switch to USDC" | Re-enable + overwrite default | Re-run with that token's `feeTokenAddress` / `relayerId`. Backend overwrites the previous default. |
| Ambiguous or names a token not in tokenList | Ask to clarify | Re-prompt once with the supported stablecoins. Do not guess. |

**Slot fills**
- `{chain_display_name}`, `{native_symbol}`, `{supported_tokens_on_chain}`, `{fromAddr}`: same as Scene A.
- `{prev_token_symbol_or_none}`: symbol of the token whose address matches `defaultGasTokenAddress` in the response; if `defaultGasTokenAddress` is empty, render "none saved".

Never modify the template body. Never drop the "Your previous default…" line. Never drop the two bullets. Never drop the supported-tokens line. Never surface "7702" / "delegation" / "on-chain" — only "disable / re-enable Gas Station" and "stablecoin to pay gas".
</MUST>

**Post-success echo template (verbatim after the Phase 2 broadcast returns successfully)**

<MUST>
After `wallet send` completes successfully via Gas Station in Scene B', the Agent MUST echo back a confirmation to the user using this template (translated to the user's language, structure verbatim):

```
Gas Station re-enabled. This transaction will use {chosen_token} to pay gas. Future transactions on {chain_display_name} will automatically use {chosen_token} when the native balance is low — no further prompt.
```

If `{chosen_token}` differs from the previous default, prepend one short sentence: "The default gas token was updated from {prev_token_symbol_or_none} to {chosen_token}."
</MUST>

No on-chain action is required — the Phase 2 response for this state carries only transaction signing material (no activation signing), since the chain is already delegated.

#### Scene C — READY_TO_USE with default token insufficient (or no default + multiple sufficient tokens)

Gas Station is already enabled; either the current default token's balance can't cover this tx's gas fee, or no default is pinned and multiple tokens are sufficient. Present a **single flat option list** — one cancel option plus two sub-options per `sufficient=true` token (this time only / replace default).

**Step 2 — Pick a token (and whether to replace the default)**

Ask the user:

> "Your default gas token {prev_token} doesn't have enough balance to cover the gas fee on this transaction. Pick one:"
>
> 1. Cancel — top up {prev_token} or switch chains / accounts
> 2. Use {tokenList[0].symbol} for this transaction only (keep {prev_token} as default)
> 3. Use {tokenList[0].symbol} and set it as the new default
> 4. Use {tokenList[1].symbol} for this transaction only
> 5. Use {tokenList[1].symbol} and set it as the new default
> … (two options per `sufficient=true` entry, starting from 2)

If there is no current default (`defaultGasTokenAddress` empty), omit the "keep {prev_token} as default" clause in the "this time only" options and drop option `1`'s "top up {prev_token}" phrasing.

| Pick | CLI action |
|---|---|
| `1` | Do NOT re-run. Tell the user to top up the default token at `{fromAddr}` or switch chains / accounts. Terminate. |
| "this time only" option | Re-run `wallet send --gas-token-address <addr> --relayer-id <id>` (no `--enable-gas-station`; no follow-up `update-default-token`). |
| "set as new default" option | Same re-run as above, **additionally** after the transaction completes call `wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>`. |

Scene C does not pass `--enable-gas-station` — Gas Station is already on; only the gas token changes.

### Step 3 — Second `wallet send` call completes

Sign 712 hash → broadcast → `{ txHash, orderId, serviceCharge, serviceChargeSymbol, serviceChargeUsd, ... }`. Then use the **Universal Gas Station Success Reply** below.

---

## Universal Gas Station Success Reply (applies to ALL commands)

Whenever **any** transaction is paid via Gas Station — regardless of which command triggered it (`wallet send`, `wallet contract-call`, `swap`, `bridge`, any DeFi plugin) — the Agent's user-facing reply MUST include the four elements below. Detecting that Gas Station was used: the response contains `gasStationUsed=true` or a non-empty `serviceCharge` + `serviceChargeSymbol` pair (or the preceding CLI invocation carried `--enable-gas-station` / `--gas-token-address`).

<MUST>
After any successful Gas Station broadcast, the reply MUST contain all four elements:

1. **Gas Station acknowledgment** — state plainly that this transaction's gas was paid via Gas Station, not with native token. Phrase it in a single sentence; never imply the transaction was "free".
2. **Service charge (stablecoin amount + USD equivalent)** — show both the raw amount + symbol (`{serviceCharge} {serviceChargeSymbol}`) and the USD equivalent (`≈ ${serviceChargeUsd}` if the backend returned it, otherwise derive from the token's price). Example: "Network fee: 0.13 USDC (≈ $0.13)".
3. **orderId** — copy verbatim from the broadcast response. Never omit it, never truncate it.
4. **Natural-language follow-up prompt** — an exact sentence the user can type back in this chat to get the final status:
   - Template: "You can tell me: **check order `{orderId}`**"
   - When replying in a non-English language, translate the sentence faithfully; keep the `check order {orderId}` idiom unchanged so the Agent can recognize the user's reply later.

**When `txHash` is empty** (relayer returns hash asynchronously — almost always empty on first response): additionally state "Transaction submitted; the relayer returns the on-chain hash asynchronously — ask me again in a moment."

**NEVER** in this reply:
- Do NOT show raw CLI commands (`onchainos wallet history ...`, `--order-id`, etc.) — the user must not be sent to a terminal.
- Do NOT fabricate a `txHash` when it's empty — only show it once returned.
- Do NOT call Gas Station "free" or hide the service charge.

**Consistency check before sending**: if the reply contains `orderId` but no natural-language prompt, the reply is incomplete — add the prompt. If the reply contains "Gas Station" but no stablecoin amount + USD equivalent, the reply is incomplete — add the fee line.
</MUST>

### Example replies

**Scenario: swap (okx-dex-swap) that used Gas Station**

```
Swap executed.

- From: 0.5 USDC
- To: ~2.231 CRV
- Price impact: 0.44%
- Network fee: 0.08 USDC (~ $0.08, paid via Gas Station)
- orderId: ord_abc123xyz
- txHash: relayer is returning it asynchronously; ask me again in a moment

You can tell me: check order ord_abc123xyz
```

**Scenario: DeFi supply (okx-defi-invest) that used Gas Station**

```
Supplied 0.01 USDT to Aave V3.

- Network fee: 0.12 USDT (~ $0.12, paid via Gas Station)
- orderId: ord_def456uvw
- txHash: relayer is returning it asynchronously; ask me again in a moment

You can tell me: check order ord_def456uvw
```

**Scenario: plain wallet send that used Gas Station**

```
Sent 5 USDT to 0x1234...abcd on Ethereum.

- Network fee: 0.13 USDT (~ $0.13, paid via Gas Station)
- orderId: ord_ghi789rst
- txHash: relayer is returning it asynchronously; ask me again in a moment

You can tell me: check order ord_ghi789rst
```

**Purpose**: prevent the user from thinking the transaction failed and resubmitting, keep the status-check conversation inside this chat (not in a terminal), and make the gas-in-stablecoin cost explicit so the user can budget it.

### Checking the order later

When the user replies with something matching "check order xxx" / "what's the status of order xxx" / "check my recent transactions" / any equivalent (in any language), the Agent runs `wallet history --chain <chain> --order-id <orderId>` internally (NOT shown to user) and relays:
- If completed: final `txHash`, on-chain status, final gas fee (may differ slightly from estimate).
- If still pending: tell the user it's still processing and to ask again shortly.
- If failed / timed out: explain "funds are intact" (GS broadcast is atomic — failure means stablecoin was not deducted), propose retry or native-gas fallback.

---

## Passive Response Templates (blocked scenarios)

Per PRD, these scenarios make Gas Station unavailable for the tx. **Do NOT proactively push Gas Station as an option in these scenarios.** The default flow (native gas or normal error) runs as usual. Only when the user **directly asks** a variant of "can I pay gas with stablecoin?" / "why can't I use Gas Station for this?" does the Agent respond with the matching template below.

### Template 1 — HAS_PENDING_TX (a prior Gas Station tx is still pending)

**Trigger**: user asks "can I pay gas with stablecoin?" and `hasPendingTx=true` in the latest diagnostic response (or the prior tx's orderId has not cleared yet).

<MUST>
**User-facing template (verbatim). Do NOT paraphrase.**

```
A previous transaction is still processing, so Gas Station can't handle a new one yet. Please wait for it to finish and try again, or top up {native_symbol} and continue with native gas.

(If you want to check the prior transaction, tell me: **check order {prev_orderId}**.)
```

**Slot fills**
- `{native_symbol}`: chain native token (ETH, BNB, etc.).
- `{prev_orderId}`: the orderId from the previous Gas Station broadcast, if known from conversation context. If unknown, omit the parenthetical line.

Do NOT auto-retry `wallet send`. Do NOT mention "relayer" / "7702" / internal mechanisms.
</MUST>

### Template 2 — Relayer single-tx cap exceeded (100,000 USD)

**Trigger**: user asks "can I pay gas with stablecoin?" and the tx amount exceeds 100,000 USD equivalent. Backend silently falls back to normal flow (`gasStationUsed=false`) for these txs; CLI will not emit a GS Confirming, so Agent must detect amount vs cap from the user's intent or the router estimate.

<MUST>
**User-facing template (verbatim). Do NOT paraphrase.**

```
This transaction exceeds the Gas Station single-transaction cap (100,000 USD equivalent), so you can't pay gas with a stablecoin on this one. Two options:

1. Top up {native_symbol} and continue paying gas with the native token.
2. Split this transaction into smaller ones below the cap and retry.
```

**Slot fills**
- `{native_symbol}`: chain native token.

Do NOT disclose any per-relayer numeric details, do NOT say "relayer" / "Phase 1" / "7702" / internal terms.
</MUST>

### Template 3 — txHash asked before relayer returns hash

**Trigger**: the user asks for the tx hash or recent tx detail, but the most recent Gas Station broadcast's `txHash` is still empty (the relayer hasn't published the on-chain hash yet). The orderId is known; the hash is not.

<MUST>
**User-facing template (verbatim — first response). Do NOT paraphrase.**

```
The transaction is still being confirmed on chain. Please ask me again in a moment — just say **check order {orderId}** and I'll pull the latest status.
```

**Follow-up template if the user asks "why does my other tx return a hash immediately but this one doesn't?"**

```
This transaction used Gas Station, so its hash comes back a little later than a regular transaction.
```

**Slot fills**
- `{orderId}`: the orderId returned by the most recent broadcast.

Do NOT fabricate a `txHash`. Do NOT show `onchainos wallet history ...` or any raw CLI command. Do NOT explain the relayer/async mechanism beyond the one sentence above.
</MUST>

### Template 4 — orderId status query (user asks "check order xxx")

**Trigger**: the user replies with any phrasing equivalent to "check order {orderId}" / "what's the status of order {orderId}" / "is that transaction done" (in any language). Agent runs `wallet history --chain <chain> --order-id {orderId}` internally (NOT shown to user) and renders one of the three outcome templates below.

<MUST>
**Outcome A — completed (on-chain success, txHash returned).** User-facing template (verbatim):

```
✅ Order {orderId} completed.

- txHash: {txHash}
- Status: success
- Chain: {chain_display_name}
- Gas fee (actual): {serviceCharge} {serviceChargeSymbol} (≈ ${serviceChargeUsd}, paid via Gas Station)
```

**Outcome B — still processing (relayer hasn't returned on-chain hash; txHash empty, status pending).** User-facing template (verbatim):

```
Order {orderId} is still being processed by the relayer. Please ask me again in a moment — just say **check order {orderId}** and I'll pull the latest status.
```

**Outcome C — failed / timed out (status = Failed; typically caused by 10-minute relayer TTL).** User-facing template (verbatim):

```
⚠️ Order {orderId} did not complete.

The transaction was broadcast via Gas Station but the relayer didn't finalize it on chain within the 10-minute window, so it has been marked as failed.

Your assets were NOT moved — the stablecoin gas fee was not deducted, and the {amount} {tokenSymbol} you were trying to send is still in your account.

You can retry the transaction now, or top up {native_symbol} to pay gas with the native token instead.
```

**Slot fills**
- `{orderId}`, `{txHash}`, `{chain_display_name}`, `{serviceCharge}`, `{serviceChargeSymbol}`, `{serviceChargeUsd}`: from the `wallet history` response entry for this order.
- `{amount}`, `{tokenSymbol}`: the original transfer amount + token (from the history entry or conversation context).
- `{native_symbol}`: chain native token symbol.

Never fabricate txHash. Never show `wallet history` as a command to the user. Never expose "relayer" beyond the one-sentence mention in Outcome C. Never expose "7702" / "delegation" / internal status enum names.
</MUST>

---

## Confirming Response — How Agent Should Handle

When `wallet send` returns a **Confirming** response, identify which scene by the `gasStationStatus` enum (or `message` content as fallback) and follow Step 2's decision tree for that scene.

General principles:

1. **Display** the `message` to the user verbatim (contains token list with balances and fees)
2. **Walk the decision tree** for the matching scene:
   - Scene A (FIRST_TIME_PROMPT): one flat prompt — `1` decline / `2+` enable and use this token. Each pick pins that token as the chain's default.
   - Scene A' (PENDING_UPGRADE): same shape as Scene A.
   - Scene B' (REENABLE_ONLY): same shape as Scene A, worded as "re-enable" (user explicitly disabled GS earlier). Pick overwrites the previous default.
   - Scene C (READY_TO_USE + default insufficient): two questions — which alternative token? → replace the default?
3. **Assemble flags** based on user decisions:
   - `--gas-token-address` = picked token's `feeTokenAddress` (Scene A / A' / B' / C)
   - `--relayer-id` = same token item's `relayerId`
   - `--enable-gas-station` = Scene A / A' / B' (non-enabled states that need flipping)
4. **Re-run** the same `wallet send` with the assembled flags.
5. **Scene A / A' / B' user declined (pick = `1`)** → Do not re-invoke the CLI. Tell the user balance is insufficient for gas, ask them to top up native token at `{fromAddr}` (or switch to a chain with enough native). Terminate.
6. **Scene C + user chose to replace default** → After the transaction completes, call `wallet gas-station update-default-token`.

---

## User Intent Recognition

Users may express Gas Station-related needs in various ways. The Agent should recognize these intents:

<MUST>
**Agent output vocabulary (authoritative — referenced by Critical Rules, troubleshooting.md, eip7702-upgrade.md, cli-reference.md)**: speak only of "enable / disable Gas Station" and "which stablecoin to pay gas". Translate to the user's language at output time, but keep the Skill prose in English.

**Never surface** in user-facing replies:
- `7702` / `EIP-7702` / `delegation` / `upgrade` / `authorization` / `revoke` (in any language — the Agent must never utter the local-language equivalents of these terms either)
- Status enum names: `FIRST_TIME_PROMPT`, `PENDING_UPGRADE`, `REENABLE_ONLY`, `READY_TO_USE`, `NOT_APPLICABLE`, `INSUFFICIENT_ALL`, `HAS_PENDING_TX`
- Internal fields / flow labels: `unsignedInfo`, `broadcast`, `Phase 1` / `Phase 2`, `DB flag` / `database flag`, `on-chain setup` (in any language)
- Error codes: `code=10004`, `code=81358`, `code=81362`, `code=50125`, `code=80001`, or any numeric error code
- Debug references: debug flags, log file paths, audit log paths

Users may **input** any equivalent phrasing in any language (e.g., "revoke 7702", "cancel the 7702 upgrade") — recognize the intent, but respond using only the sanctioned vocabulary. Exception: if the user directly asks what the mechanism is, a brief neutral explanation is acceptable, but flag it as an internal detail and keep it short.
</MUST>

| User says | Intent | Action |
|---|---|---|
| "I don't have ETH for gas" / "no gas" / "insufficient gas" / "can't afford gas" / "not enough for fees" | Wants to send but lacks native token | Proceed with `wallet send` — Gas Station activates automatically. |
| "Can I pay gas with USDC?" / "use stablecoin for gas" / "pay fee with USDT" / "pay network fee with stablecoin" | Asks about Gas Station capability | Explain Gas Station briefly, then proceed with the transaction if the user provides one. |
| "What is Gas Station?" / "how does gas station work" / "explain gas station" | FAQ | Answer from the FAQ section below. |
| "Change my default gas token" / "switch gas payment to USDC" / "set USDT as gas default" | Change the default gas token | Call `wallet gas-station update-default-token`. |
| "enable gas station" / "turn on gas station" / "open gas station" / "reactivate gas station" | Enable Gas Station | **Ask the user which chain first** — the API requires `--chain`. Then call `wallet gas-station enable --chain <chain>`. For the pre-confirmation and success reply wording, use the templates in "User-Facing Reply Templates (Management Commands)". If the chain was never activated, the backend returns a message — relay it verbatim, do NOT paraphrase with "7702" / "delegation" / "DB". |
| "disable gas station" / "turn off gas station" / "stop using stablecoin for gas" / "revoke 7702" / "cancel 7702 upgrade" / "cancel gas station authorization" | Disable Gas Station | **Ask the user which chain first** — the API requires `--chain`. Then call `wallet gas-station disable --chain <chain>`. Use the confirmation and success templates in "User-Facing Reply Templates (Management Commands)". **NEVER mention** "DB flag" / "7702" / "delegation" / "authorization" / "revoke" / "on-chain setup" in your reply (in any language) — regardless of the phrasing the user used to trigger the intent. If the user only wants to change the gas-payment token, suggest `wallet gas-station update-default-token` instead of disabling. |
| "Why can't I use stablecoin for gas?" / "gas station not working" / "why no gas station option" | Blocked-scenario inquiry | Check: pending tx? amount too large? unsupported chain? native-token transfer? Explain the matching reason. |
| "What's the tx hash?" / "hash for my last transaction" / "show me the hash" | User wants the txHash while the relayer has not returned it yet | Reply: "The transaction is being confirmed on-chain. Please ask me again in a moment — just say `check order {orderId}`." Never fabricate a hash. Never show the raw CLI command to the user. |
| "Why can I get hash immediately for other txs but not this one?" / "why is hash slow" | User questions why the hash is delayed | Reply: "This transaction uses Gas Station, so the hash returns slightly later than a regular transaction." One sentence is enough — do not expand into relayer or on-chain-setup details. |
| "Check my last transaction" / "transaction status" / "where's my tx" | User wants the status of the recent transaction | Read the orderId from conversation context and call `wallet history --chain <chain> --order-id <orderId>`. Prefer orderId lookup; show the txHash only when it has returned. |

---

## Management Commands

| Command | Usage | Internal Notes (NOT user-facing) |
|---|---|---|
| Change default gas token | `onchainos wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>` | Takes effect on next Gas Station transaction. |
| Enable Gas Station | `onchainos wallet gas-station enable --chain <chain>` | DB flag flip only. Requires the chain to have been delegated earlier via a first-time `wallet send` flow. If the chain was never delegated, backend returns a msg — surface the backend msg verbatim, do NOT paraphrase with "7702" / "delegation". |
| Disable Gas Station | `onchainos wallet gas-station disable --chain <chain>` | DB flag flip only, no on-chain action. On-chain state preserved so re-enabling later is instant. |
| **Pre-flight (read-only)** | `onchainos wallet gas-station status --chain <chain> [--from <addr>]` | Probes Phase 1 diagnostic via a 0-amount native self-transfer. Returns `recommendation` (READY / ENABLE_GAS_STATION / REENABLE_GAS_STATION / PENDING_UPGRADE / INSUFFICIENT_ALL / HAS_PENDING_TX) + `tokenList` + `gasStationActivated`. Never broadcasts; safe to call repeatedly. **Used by third-party plugin pre-flight** — see `SKILL.md` "Third-Party Plugin Pre-flight". |
| **Standalone activation** | `onchainos wallet gas-station setup --chain <chain> --gas-token-address <addr> --relayer-id <id>` | Internally drives a 1-minimal-unit self-transfer of the picked gas token with `--enable-gas-station --force` (pre-condition: agent has already obtained user consent via Scene A). Backend returns signing material for both 712 and (when needed) `authHashFor7702`; CLI signs and broadcasts in one tx. Idempotent: same default → `alreadyActivated=true`; different default → switches via `update-default-token`. |

<MUST>
The "Internal Notes" column is **Agent-internal background only** — it describes why the CLI behaves as it does. **Never copy these notes verbatim into a user-facing reply.** Do NOT mention "DB flag" / "7702" / "delegation" / "on-chain setup" to the user (in any language). Use the user-facing templates below.
</MUST>

### Activation via standalone `gas-station setup`

When the agent has obtained user consent through Scene A / B' / A' (whether triggered by a direct `wallet send` Confirming OR by third-party plugin pre-flight), it MUST activate Gas Station via:

```bash
onchainos wallet gas-station setup \
    --chain <CHAIN> \
    --gas-token-address <picked tokenList[N-2].feeTokenAddress> \
    --relayer-id <picked tokenList[N-2].relayerId>
```

This is the recommended path for activations driven by **third-party plugin pre-flight** (see `SKILL.md` → "Third-Party Plugin Pre-flight"). Activations bundled with `wallet send` (the legacy path that returns a Confirming on FIRST_TIME_PROMPT) continue to work but are reserved for direct user `wallet send` calls.

After successful `setup`, subsequent `wallet send` / `wallet contract-call` (including those issued by third-party plugins) on that chain will transparently use Gas Station — no further user prompts. The third-party plugin will succeed on its very next invocation without any change to its CLI args.

### User-Facing Reply Templates (Management Commands)

Use these sanctioned phrasings when relaying command results to the user. Translate to the user's language at output time; the semantic content must not drift.

**Before running `wallet gas-station disable` (confirmation prompt)**

> "Turning it off means transactions on {chain} will pay gas with the native token ({native_symbol}) again; you can re-enable any time. If you only want to switch the gas-payment token, use 'change default gas token' instead of disabling. Disable now?"

**After `wallet gas-station disable` succeeds**

> "Gas Station is now off on {chain}. This chain will pay gas with {native_symbol} from here on; you can re-enable any time."

**Before running `wallet gas-station enable`**

> "Enable Gas Station on {chain}? Once enabled you can pay gas with a stablecoin."

**After `wallet gas-station enable` succeeds**

> "Gas Station is now on for {chain}. This chain will pay gas with a stablecoin from here on."

**After `wallet gas-station update-default-token` succeeds**

> "Default gas token on {chain} changed to {new_token}. This chain will use {new_token} to pay gas by default."

**NEVER in user-facing replies**: see the authoritative vocabulary ban list in "User Intent Recognition" MUST block above. If the user directly asks about the mechanism, load `references/eip7702-upgrade.md` "User asks what is 7702" row for a neutral short answer — otherwise stay silent about internals.

> For 7702 upgrade details, signing flow, and edge cases: read `references/eip7702-upgrade.md` (Agent-internal reference).

---

## Plugin Bail Recovery (Agent Orchestration)

Third-party plugins like `aave-v3-plugin` invoke `onchainos wallet contract-call` as a subprocess. When CLI returns a **CliConfirming** (exit code 2) for Scene A / Scene C / FIRST_TIME_PROMPT, the plugin's subprocess wrapper typically treats non-zero exit as a failure and `bail!`s out of its own flow. The **Agent (Claude Code)** can catch this, resolve the Gas Station setup, and re-invoke the **same plugin command** — plugin will re-organize its calldata from scratch and this time CLI will hit the auto path.

### When does this pattern trigger

The Agent detects a plugin bail with a **structured Confirming in stdout**:

```
Error: <plugin step> failed
Caused by: onchainos exited with status 2: stderr=... stdout={"confirming": true, "message": "...", "next": "..."}
```

The key markers:
- Exit code **2** (non-zero, subprocess marked as failure)
- stdout contains a JSON with `"confirming": true`

If these match → this is a Gas Station CliConfirming, not a real failure — recoverable.

### Recovery decision tree

Parse `message` from the Confirming JSON to identify the Scene, then:

| Scene marker in `message` | Recovery action |
|---|---|
| `"gasStationStatus: FIRST_TIME_PROMPT"` / `"gasStationStatus: PENDING_UPGRADE"` / `"first-time activation required"` | **Scene 6**: Walk Scene A (`1` decline / `2+` enable and use this token). On pick `N ≥ 2`, trigger activation via a small self-transfer with the picked token (see below). Then re-invoke the original plugin command. Never surface `7702` / `upgrade` / `delegation` to the user. |
| `"gasStationStatus: REENABLE_ONLY"` | **Scene 6'**: Walk Scene B' (`1` decline / `2+` re-enable and use this token). If declined, terminate the plugin flow and tell the user to top up native. On pick `N ≥ 2`, run the small self-transfer to re-enable (no on-chain upgrade needed), then re-invoke the original plugin command. |
| `"Gas Station is active on this chain. Choose a token to pay gas"` + multiple sufficient tokens in the list | **Scene C (plugin bail, no default)**: Show a flat list — `1` cancel / `2+` use `{tokenList[N-2].symbol}` to pay gas (sets as default). On `N ≥ 2`, run `onchainos wallet gas-station update-default-token --chain {chain} --gas-token-address {picked}` first, then re-invoke plugin. |
| `"Your default gas token has insufficient balance"` + alternatives in list | **Scene C (plugin bail, default insufficient)**: Flat list — `1` cancel / `2+` use `{tokenList[N-2].symbol}` (replaces {defaultToken} as default). On `N ≥ 2`, run `wallet gas-station update-default-token --chain {chain} --gas-token-address {picked}` first, then re-invoke plugin. Plugin bail always replaces default (no "this time only" path) because the plugin re-runs multiple sub-calls that all need the same default. |
| `insufficientAll` in structured response | **Scene 4**: Tell user "Not enough balance in any stablecoin to pay gas. Please top up." **Do NOT retry.** |
| `hasPendingTx` in structured response | **Scene 5**: Tell user "A pending Gas Station tx is blocking this. Wait for it to confirm (usually within a few minutes) or run `wallet gas-station disable --chain {chain}` to bypass. Then retry." **Do NOT retry automatically.** |

### Scene 6 activation — self-transfer trigger

To activate Gas Station + 7702 upgrade on a chain, the Agent can run a tiny self-transfer (transferring a stablecoin to the user's own address is idempotent at value level, only acts as a carrier for the 7702 upgrade):

```bash
onchainos wallet send \
  --chain {chain} \
  --contract-token {picked_token_address} \
  --readable-amount 0.01 \
  --recipient {user_own_address} \
  --gas-token-address {picked_token_address} \
  --relayer-id {picked_relayer_id} \
  --enable-gas-station
```

`{picked_token_address}` / `{picked_relayer_id}` come from the `next` field's tokenList JSON. `{user_own_address}` = current account's address on this chain.

Wait for the self-transfer to confirm (check `orderId` via `wallet history`), then re-invoke the original plugin command. Now the account is 7702-upgraded and has a default gas token — subsequent plugin CLI calls will hit Scene B (default matched + sufficient) auto path.

### Why re-invoke the plugin (not reconstruct its calldata)

The plugin owns its business logic:
- Calculates approve + supply / borrow / other calldata
- Knows protocol-specific addresses (Aave Pool, Uniswap Router, etc.)
- Manages step sequencing (approve → wait confirm → supply)

Agent only operates at the **wallet layer** (update default token, trigger 7702 upgrade). After Agent's wallet-layer fix, **re-running the same plugin command** lets the plugin re-organize its own calldata. All plugin's sub-calls to `wallet contract-call` now succeed because GS state is set up.

Because the first bail was at Phase 1 (unsignedInfo diagnostic, before broadcast), **no on-chain state was mutated** — re-running is fully idempotent.

### Recovery principles

<MUST>
- **Always parse the Confirming JSON structure** (exit code 2 + `"confirming": true` in stdout) before deciding it's a recoverable case. Real failures (bad calldata, invalid address, insufficient balance) return different structures — do NOT treat all exit-2 as recoverable.
- **Always ask user consent** for Scene 6 (first-time enable), Scene 6' (re-enable), and Scene C plugin-bail (token selection) — CLI refuses to decide these silently on purpose. Auto-retrying without user consent violates the user-preference contract.
- **Do NOT retry Scene 4 (insufficientAll) or Scene 5 (hasPendingTx)** — these require external action (top up / wait).
- **Re-invoke the same plugin command verbatim** after recovery. Do not try to replicate the plugin's internal flow by hand — the plugin is the domain expert for its own calldata.
</MUST>

### Example end-to-end (aave-v3-plugin supply, account not yet on Gas Station)

```
User → Agent: "supply 0.01 USDT to aave on ethereum"
Agent → aave-v3-plugin supply --asset USDT --amount 0.01 --chain 1 --confirm
  Plugin step 1 (approve):
    Plugin → onchainos wallet contract-call --chain 1 --to USDT --input-data 0x095ea7b3... --from X --force
      CLI: Phase 1 returns FIRST_TIME_PROMPT
      CLI: returns CliConfirming (exit 2, stdout = {"confirming": true, "message": "Gas Station first-time activation required..."})
    Plugin: run_cmd sees exit 2 → bail!
  Plugin returns Err to Agent

Agent inspects err.stdout, sees "first-time activation required" → Scene 6
Agent → User: "You don't have enough ETH on Ethereum to pay gas. Enable Gas Station and pick a stablecoin to pay gas with:
  1. No, don't enable
  2. Yes, enable and use USDT to pay gas
  3. Yes, enable and use USDC to pay gas"
User → Agent: "2"

Agent → onchainos wallet send --chain 1 --contract-token USDT_ADDR --readable-amount 0.01 --recipient X --gas-token-address USDT_ADDR --relayer-id RELAYER_ID --enable-gas-station
  CLI: Phase 2, signs, broadcasts → orderId
Agent waits for confirmation (wallet history --order-id)

Agent → aave-v3-plugin supply --asset USDT --amount 0.01 --chain 1 --confirm
  Plugin step 1 (approve):
    Plugin → wallet contract-call ...
      CLI: Phase 1 now returns READY_TO_USE, default matches USDT, sufficient → auto Phase 2 → broadcast ✅
  Plugin step 2 (supply):
    Plugin → wallet contract-call ...
      CLI: same auto path ✅
  Plugin returns Ok

Agent → User: "Supplied 0.01 USDT to Aave V3. approveTx={h1}, supplyTx={h2}. Gas paid in USDT via Gas Station."
```

---

## Edge Cases

<MUST>
Handle these edge cases explicitly — do NOT fall through to generic error handling:
</MUST>

| Edge Case | How to detect | Agent response |
|---|---|---|
| **Pending transaction** | `hasPendingTx: true` in response | Use the authoritative user message in the Step 1 table `HAS_PENDING_TX` row. See also the "Passive Response Templates" section below for the user-asked variant ("can I pay gas with stablecoin?"). |
| **All tokens insufficient** | `insufficientAll: true` in response | Use the authoritative user message in the Step 1 table `INSUFFICIENT_ALL` row. |
| **Relayer amount cap exceeded** | Backend silently falls back to normal flow (`gasStationUsed=false`) when the single-tx amount exceeds 100,000 USD equivalent. Do NOT proactively surface Gas Station in this case. | See "Passive Response Templates" section below. Only respond with the cap explanation when the user directly asks why stablecoin gas is unavailable for this tx. |
| **Native token transfer** | Backend returns gasStationUsed=false for transfers without contractAddr | Gas Station only works for ERC-20 token transfers, not native token (ETH/BNB) transfers. If user asks, explain this. |
| **Unsupported chain** | Backend returns gasStationUsed=false | Gas Station is only available on: Ethereum, BNB Chain, Base, Arbitrum One, Polygon, Optimism, Conflux eSpace, Linea, Scroll, Monad, Sonic EVM. If user asks about other chains, list the supported chains. X Layer doesn't need GS — it charges zero gas fees natively. |
| **Gas Station tx result** | After broadcast, txHash is returned but result is async | Always remind: "Transaction submitted. Gas Station transactions are processed by a Relayer and may take a few minutes. Check `wallet history` for the final status." |
| **7702 upgrade / disable issues** | See `references/eip7702-upgrade.md` Edge Cases | Load eip7702-upgrade.md for: upgrade in progress, third-party delegation, re-enable shortcut |
| **User asks about gas fee after Gas Station tx** | In transaction history, gas fee shows in stablecoin | Display the gas fee in the actual token used (e.g. "Gas fee: 0.13 USDT"), not in native token. |

---

## FAQ

All answers below are **user-facing reply templates** — render them verbatim (translate to the user's language, but keep the structure and sentences). Never paraphrase. Never expose "7702" / "delegation" / "upgrade" / internal mechanism terms. When a user directly asks about the underlying mechanism, keep the answer short and generic.

<MUST>
**"Gas Station" always means the OKX Agentic Wallet feature shipped by this CLI + skill.** When the user asks any FAQ-style question:

- DO NOT pull from general training knowledge about ERC-4337, Paymaster, Biconomy, Gelato, Pimlico, Alchemy Account Kit, meta-transactions, or any third-party gas-abstraction protocol.
- DO NOT answer in a "category explainer" style (e.g., "Gas Station is a category of services including X, Y, Z...").
- DO NOT list alternative/competing protocols unless the user explicitly asks for comparisons.
- DO use the verbatim templates below — they describe OKX's Gas Station specifically, not a generic category.

If the question doesn't match any Q below, keep the answer grounded in the templates and do NOT fall back to generic web3 knowledge.
</MUST>

### Q: What is Gas Station?

```
Gas Station aggregates third-party payment services, automatically compares their rates, and picks the best one to pay gas on your behalf. You can pay with USDT, USDC, or USDG — no need to hold native tokens like ETH or BNB.

Supported networks and stablecoins:
- Ethereum: USDT, USDC, USDG
- BNB Smart Chain (BSC), Base, Polygon, Arbitrum One, Optimism, Linea, Monad, Scroll, Conflux eSpace, X Layer: USDT, USDC
- Sonic: USDC

Learn more: https://web3.okx.com/learn/wallet-gas-station
```

### Q: How does Gas Station work?

<MUST>
**Verbatim answer — reproduce the three numbered steps and the supported-chains line exactly. Translate to the user's language at output time; the structure and every sentence stays.**

```
Here's how Gas Station works under the hood:
1. Your wallet is upgraded to a smart contract wallet via EIP-7702, giving it the ability to pay gas with ERC-20 tokens.
2. A third-party Relayer service pays the native-token gas required on chain on your behalf.
3. In the same transaction, your chosen stablecoin automatically repays the Relayer.

Supported chains: Ethereum, BNB Smart Chain (BSC), Base, Polygon, Arbitrum One, Optimism, Linea, Monad, Scroll, Sonic, Conflux eSpace, and X Layer.
```
</MUST>

### Q: Does enabling Gas Station cost extra?

```
The first time you enable Gas Station on a chain, there is a one-time on-chain setup that produces a small additional gas cost. That cost is bundled into your first Gas Station transaction on that chain — there is no separate charge. Each supported chain requires this one-time setup when first used.
```

### Q: Which tokens can I use to pay gas?

```
USDT, USDC, and USDG. USDG is only supported on Ethereum; Sonic EVM supports only USDC; all other supported chains accept USDT and USDC. When you have multiple eligible stablecoins, the system picks the one with the highest balance by default.
```

### Q: Does each chain need its own setup?

```
Yes. Each supported chain goes through a one-time setup the first time you use Gas Station there. It happens automatically and is included in the first transaction's gas fee on that chain.
```

### Q: Which transaction types does Gas Station support?

```
Gas Station works for all on-chain transactions — transfers, contract interactions (approvals, swaps, deposits, withdrawals, claims, borrows, repayments, cross-chain bridging, and more). Whenever your native token isn't enough to pay gas, Gas Station kicks in automatically.
```
