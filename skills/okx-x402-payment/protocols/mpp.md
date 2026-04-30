# MPP Protocol Playbook

Sign MPP (Machine Payments Protocol) authorizations for OKX's payment-channel-based pay-per-use API access. Covers **charge** (one-shot) and **session** (open / voucher / topUp / close) intents in both transaction and hash modes.

> **Entry point**: this file is loaded by the dispatcher (`../SKILL.md`) when the 402 response carries a `WWW-Authenticate: Payment ...` header with `method="evm"`. Pre-flight checks, skill routing, chain support, and protocol detection are owned by the dispatcher — start here at "Background".

## Background: MPP at a Glance

MPP is OKX's payment protocol for AI agents and machine-to-machine billing. Two intents:

| Intent      | Use case                                                           | Flow                                                                                  |
|-------------|--------------------------------------------------------------------|---------------------------------------------------------------------------------------|
| **charge**  | One-shot purchase (image, API call, file)                          | Sign once → seller settles on-chain → done                                            |
| **session** | Multi-request stream (subscriptions, AI inference, pay-per-second) | Open channel + N vouchers + close. Off-chain vouchers, batched on-chain settlement    |

MPP authorization header: `Authorization: Payment <base64url>` (a single envelope `{challenge, source, payload}`, JCS-canonicalised then base64url-encoded). The CLI commands return a complete `authorization_header` string — the agent just pastes it into the request.

Two delivery modes per intent (set by `methodDetails.feePayer` in the 402 challenge):

- **`feePayer=true` → transaction mode**: CLI TEE-signs an EIP-3009 `transferWithAuthorization` (or `receiveWithAuthorization` for session escrow); seller broadcasts.
- **`feePayer=false` → hash mode**: client/user broadcasts the tx themselves and supplies the resulting tx hash to the CLI via `--tx-hash`. The CLI still TEE-signs the off-chain pieces (e.g. initial voucher).

**MPP method check** — verify `method="evm"` in the WWW-Authenticate header (e.g. `Payment ... method="evm", ...`). This playbook only supports EVM-based MPP. If `method` is `"tempo"`, `"svm"`, `"stripe"`, or other → stop and tell the user this playbook cannot handle that method.

## Command Index

| # | Command                              | Intent  | Purpose                                                                |
|---|--------------------------------------|---------|------------------------------------------------------------------------|
| 1 | `onchainos payment mpp-charge`          | charge  | One-shot charge payment (transaction mode default; hash mode optional) |
| 2 | `onchainos payment mpp-session-open` | session | Open a payment channel (always first in a session)                     |
| 3 | `onchainos payment mpp-session-voucher`      | session | Sign a voucher for each business request                               |
| 4 | `onchainos payment mpp-session-topup`| session | Add more deposit to an open channel (optional)                         |
| 5 | `onchainos payment mpp-session-close`| session | Close the channel and settle                                           |

All five commands return a JSON object with `authorization_header` field — the value to pass back as `Authorization:` when retrying the original request.

**`--base-url` (all commands)** — every payment command accepts `--base-url '<URL>'` to override the backend service URL. Use this when the user explicitly asks to point at a staging / forked / testnet environment and provides the URL. **Always require `https://`** — `http://` triggers a 301 redirect that converts POST→GET and silently drops the request body, surfacing as a `30001 incorrect params` error. If `--base-url` is not provided, the CLI uses the configured production endpoint.

---

# Operation Flow

## Step 1: Decode and Display

Parse the WWW-Authenticate header:

```
Payment id="...", realm="...", method="evm", intent="...", request="<base64url>", expires="..."
```

base64url-decode `request` to get the JSON body. Save these fields:

```
intent              charge | session
amount              base units string (e.g. "1000000")
currency            ERC-20 contract address (token used for payment)
recipient           merchant payee address
methodDetails:
  chainId           EVM chain ID (e.g. 196 for X Layer)
  escrowContract    REQUIRED for session, ABSENT for charge
  feePayer          true (transaction mode) | false (hash mode)
  splits            optional, charge only, max 10 entries [{amount, recipient}]
  minVoucherDelta   optional, session only — min cumulativeAmount delta between vouchers
  channelId         optional, session topUp/voucher only — pre-existing channel
suggestedDeposit    optional, session only — suggested initial deposit
unitType            optional — "request" | "second" | "byte" etc.
```

Convert `amount` from base units to human-readable using the token's decimals (typically 6 for USDC/USD₮, 18 for native).

**Challenge expiry check** — if the WWW-Authenticate header carries an `expires=...` ISO-8601 timestamp and the current time is past it, the challenge is dead: re-send the original request to obtain a fresh 402 / challenge before signing. Signing against an expired challenge will fail at the seller with `30001 incorrect params` or similar.

**MANDATORY STOP — display these details and wait for explicit confirmation:**

> This resource requires MPP payment:
> - **Intent**: `<charge | session>`
> - **Network**: `<chain name>` (`eip155:<chainId>`)
> - **Token**: `<symbol>` (`<currency address>`)
> - **Amount per unit**: `<human-readable>` (atomic: `<amount>`)
> - **Pay to**: `<recipient>`
> - **Fee mode**: `<server pays gas (transaction) | client broadcasts (hash)>`
> - **Splits** (charge only, if present): `<N split recipients>`
> - **Suggested deposit** (session only, if present): `<human-readable>`
>
> Proceed with payment? (yes / no)

**Do not call `onchainos wallet status` or any other tool until the user confirms.**

- User confirms → proceed to Step 2
- User declines → stop, no payment made, no wallet check

## Step 2: Check Wallet Status

Now that the user has confirmed, check the wallet:

```bash
onchainos wallet status
```

- **Logged in** → proceed to Step 3
- **Not logged in** → ask the user how to log in:

  > You are not logged in. How would you like to authenticate?
  > 1. **Email login** — `onchainos wallet login <email>` (sends OTP, then verify)
  > 2. **API Key login** — `onchainos wallet login` (uses `OKX_API_KEY` / `OKX_SECRET_KEY` / `OKX_PASSPHRASE` env vars)
  > 3. **Cancel** — abort payment

  Wait for user response. **Local private key signing is NOT supported for MPP** (only for x402; MPP requires TEE).

## Step 3: Sign and Assemble

Branch by `intent`:

- `charge` → [§ Charge](#charge-flow)
- `session` → [§ Session](#session-flow)

---

# Charge Flow

One-shot payment. The CLI TEE-signs an EIP-3009 authorization (or wraps a client-broadcast tx hash) and returns a complete `authorization_header`.

## Charge Step 1: Decide Mode

Read `methodDetails.feePayer` from the decoded request:

- `feePayer=true` (default) → **transaction mode** (server pays gas)
- `feePayer=false` → **hash mode** (user must broadcast first)

## Charge Step 2a (transaction mode): Sign

```bash
onchainos payment mpp-charge \
  --challenge '<full WWW-Authenticate header value>' \
  [--from '<0xPayer>']
```

CLI auto-detects `methodDetails.splits[]` — no extra flag needed. Output:

```json
{
  "ok": true,
  "data": {
    "protocol": "mpp",
    "action": "mpp_pay",
    "mode": "transaction",
    "authorization_header": "Payment eyJjaGFsbGVuZ2UiOnsi...",
    "wallet": "0x...",
    "challenge": { "id": "...", "realm": "..." }
  }
}
```

Save `data.authorization_header`. Skip to [Charge Step 3: Replay](#charge-step-3-replay).

## Charge Step 2b (hash mode): Broadcast First, Then Wrap

When `feePayer=false`, the user must broadcast `transferWithAuthorization` themselves first. Ask:

> The seller does not pay gas. You need to broadcast the transferWithAuthorization yourself and supply the tx hash. How would you like to broadcast?
> 1. **Help me broadcast** — switch to `okx-onchain-gateway` skill (recommended)
> 2. **I'll broadcast manually** — paste the tx hash when ready

For Option 1, delegate to `okx-onchain-gateway` to construct + broadcast the transferWithAuthorization tx. When they return with a tx hash, continue.

For Option 2, wait for the user to provide a 66-char `0x...` hash.

Then:

```bash
onchainos payment mpp-charge \
  --challenge '<full WWW-Authenticate header value>' \
  --tx-hash '0x<64-char hex>' \
  [--from '<0xPayer>']
```

Output is the same shape as transaction mode but `mode: "hash"`. Save `authorization_header`.

## Charge Step 3: Replay

Send the original request with the signed authorization header:

```
<original method> <original url>
Authorization: <authorization_header>
```

Expected: `HTTP 200` with the requested content + a `Payment-Receipt` header containing the on-chain tx hash. Charge complete.

If the response is again 402, see [§ Troubleshooting](#troubleshooting) (signature replay / expired challenge).

---

# Session Flow

Multi-step state machine: **open → N vouchers → close** (with optional topUp). Each phase has its own CLI command and Authorization header.

## Session State to Track

The agent MUST maintain the following state across all phases of the session. Save these the moment `mpp-session-open` returns:

| Field              | Source                                  | Used by                              |
|--------------------|------------------------------------------|--------------------------------------|
| `channel_id`       | `mpp-session-open` output                | voucher / topup / close              |
| `escrow`           | open challenge `methodDetails.escrowContract` | voucher / topup / close         |
| `chain_id`         | open challenge `methodDetails.chainId`   | voucher / topup / close              |
| `currency`         | open challenge `currency`                | topup (transaction mode)             |
| `payer_addr`       | open output `wallet`                     | All commands `--from`                |
| `current_cum`      | highest signed cum so far (open `--initial-cum` or last issued voucher's cum) | reuse decisions, close      |
| `current_sig`      | last voucher signature (`signature` field of open / voucher / close output) | `--reuse-signature` for next voucher |
| `estimated_spent`  | sum of `unit_amount` across all served business requests since the last fresh sign | reuse decisions      |
| `unit_amount`      | latest voucher challenge `amount` (seller is authoritative) | next voucher cum & remaining calc |
| `deposit`          | open output `deposit` + topup `--additional-deposit` | reuse decisions, close   |

**Tracking strategy** — within a single conversation, track these in your context (e.g., conversation memory). Across conversations, ask the user to provide the channel_id, escrow, current_cum, and current_sig if they want to continue an open session.

**Mandatory state echo** — every time you respond after `mpp-session-open`, after each voucher (sign or reuse), after topup, and immediately before close, **end your message with a one-line state echo**:

> 📋 Channel `<channel_id>` · chain `<chain_id>` · escrow `<escrow>` · deposit `<human(deposit)>` (`<deposit>`) · cum `<human(current_cum)>` (`<current_cum>`) · spent~`<human(estimated_spent)>` (`<estimated_spent>`) · sig `<current_sig prefix...>`

**All amounts shown to users MUST be in BOTH human-readable form AND atomic units**, in the format `<human> (<atomic>)`. Examples:
- `0.0004 USDC (400)` — 6 decimals
- `1.5 ETH (1500000000000000000)` — 18 decimals
- `10 USD₮ (10000000)`

Compute human-readable using the token's decimals (`amount / 10^decimals`). Decimals come from the challenge's `currency` token — typically 6 for USDC/USD₮, 18 for native, but **never assume**: if uncertain, ask the user or query the token contract via `okx-dex-token`. The token symbol comes from the challenge `currency` field's known address mapping or token metadata; if neither is available, fall back to `units` literally.

**This applies everywhere agent talks numbers to user**: state echo, payment confirmation prompts (Step 1), deposit suggestions (Phase S1), settle / close summaries (Phase S3.4), `current_cum` updates after each voucher. Atomic units alone are unreadable to humans.

This lets the user copy-paste it back if the conversation is interrupted, and protects against agents silently losing channel state mid-session.

## Phase S1: Open Channel

Always the first step in any session. Decide the **deposit** with the user:

> Session requires you to lock up a deposit in the escrow contract. How much would you like to deposit?
> Suggested: `<human(suggestedDeposit)> (<suggestedDeposit>)` (if present, else suggest `<human(unit_amount × 100)> (<unit_amount × 100>)` or similar — N×100 covers ~100 unit requests)
>
> Provide either a human amount (e.g. "0.01 USDC", "5") or atomic units. The CLI takes atomic; convert before passing.
>
> Each voucher consumes from this deposit; you can topUp later or close to refund unused.

Wait for user's deposit amount.

### Optional: Initial Voucher Prepay

By default, opening a channel signs a baseline voucher with `cumulativeAmount=0` (no prepay). The user can opt for a non-zero baseline:

- **`--initial-cum N`** → explicit baseline cumulativeAmount (atomic units)
- **`--prepay-first`** → use one unit price (`challenge.amount`) automatically; falls back silently to 0 if challenge.amount is "0" or missing

Decide based on user intent:

| User says                                         | Use flag                              |
|---------------------------------------------------|---------------------------------------|
| "Just open the channel" / no preference           | (no flag, default = 0)                |
| "Open and pay first request immediately"          | `--prepay-first`                      |
| "Open and pre-authorize N atomic units"           | `--initial-cum N`                     |

Constraint: `initial_cum ≤ deposit`. The SDK rejects with `70012` if violated.

### Mode Branch

Read `methodDetails.feePayer`:

#### Transaction mode (`feePayer=true`)

```bash
onchainos payment mpp-session-open \
  --challenge '<full WWW-Authenticate header value>' \
  --deposit '<atomic units>' \
  [--initial-cum '<atomic>' | --prepay-first] \
  [--from '<0xPayer>']
```

CLI TEE-signs an EIP-3009 `receiveWithAuthorization` to deposit funds into the escrow contract, plus an EIP-712 Voucher (channelId, cumulativeAmount=initial_cum) as the baseline.

Output:

```json
{
  "ok": true,
  "data": {
    "protocol": "mpp",
    "action": "session_open",
    "mode": "transaction",
    "authorization_header": "Payment eyJjaGFsbGVuZ2UiOnsi...",
    "channel_id": "0x...",
    "escrow": "0x...",
    "chain_id": 196,
    "deposit": "10000",
    "wallet": "0x..."
  }
}
```

Save `data.channel_id`, `data.escrow`, `data.chain_id`, `data.wallet` to session state. Initial `current_cum` = the initial-cum value (default "0").

#### Hash mode (`feePayer=false`)

User must broadcast the open tx (escrow `openWithAuthorization`) themselves first. Same delegation choice as charge hash mode — offer `okx-onchain-gateway` or manual broadcast.

When you have the tx hash:

```bash
onchainos payment mpp-session-open \
  --challenge '<full WWW-Authenticate header value>' \
  --deposit '<atomic units>' \
  --tx-hash '0x<64-char hex>' \
  [--initial-cum '<atomic>' | --prepay-first] \
  [--from '<0xPayer>']
```

The CLI still TEE-signs the initial voucher (EIP-712); only the on-chain deposit tx is replaced by the hash you supplied.

### Send Open to Seller

```
<original method> <original url>
Authorization: <authorization_header>
```

Two possible outcomes:

- **HTTP 200** — channel is open and the response carries the FIRST business response (e.g. the requested resource). Echo the saved session state to the user (channel_id / deposit / current_cum), then for any subsequent request to the same resource, send the request without `Authorization` first; seller will respond with a voucher 402 → enter Phase S2.
- **HTTP 402 with a fresh `WWW-Authenticate: Payment`** — channel opened but the seller is asking you to sign the first voucher (intent stays `session`, but `cumulativeAmount` is now expected). Proceed directly to Phase S2.

## Phase S2: Business Request (Voucher Loop)

For **each** business request during the session:

> **When to enter this phase**: any of the following user signals while a `channel_id` is active in session state:
> - "再调一次" / "再发一个请求" / "下一个请求" / "继续" / "next request" / "another request" / "do it again"
> - User issues a request to the same resource URL and gets a fresh 402
> - User explicitly says "voucher" / "凭证" / "签一个 voucher"

### How vouchers actually work (read this once, then internalise)

A voucher is a **cumulative authorisation**, not a single-request payment. Once signed, the seller can keep deducting from it until `spent` reaches the signed `cumulativeAmount`. So a single voucher with `cum=50` can fund 50× `unit_amount=1` requests **without re-signing**, as long as the seller supports voucher reuse:

- **mppx**, **OKX TS Session**, **OKX Rust SDK ≥ this version** → reuse is supported (resending the same `(cum, signature)` bytes lets the seller keep deducting).
- **OKX Rust SDK < this version** (legacy) → byte-replay was treated as idempotent network retry and skipped deduct. If you suspect the seller is on legacy SDK, force re-sign every request.

The agent's job per request: pick **reuse** vs **sign** based on remaining balance under the current voucher.

### S2.1: Send the Request

If you don't have a fresh challenge yet, send the business request. Seller responds with HTTP 402 and a fresh `WWW-Authenticate: Payment` header — this is a **voucher challenge** for the new request. Decode `request` to extract `amount` (the seller-quoted unit price).

### S2.2: Decide Reuse vs Sign

```
unit_amount = <amount from this voucher challenge>      // seller is authoritative
remaining   = current_cum - estimated_spent             // headroom under existing voucher

if current_sig is set AND remaining >= unit_amount:
    strategy = REUSE         # spend remaining headroom under existing voucher
    cum_for_this_call = current_cum                     # unchanged
else:
    strategy = SIGN          # need a higher cum
    cum_for_this_call = current_cum + unit_amount

# Hard guards (apply regardless of strategy)
if cum_for_this_call > deposit:
    → Phase S2b (TopUp) first, then re-evaluate
if methodDetails.minVoucherDelta is set AND strategy == SIGN:
    ensure (cum_for_this_call - current_cum) >= minVoucherDelta
```

`unit_amount` always comes from the **current** voucher challenge, never from a cached value — the seller can adjust pricing between requests and the latest 402 wins.

### S2.3a: Reuse path (no TEE)

```bash
onchainos payment mpp-session-voucher \
  --challenge '<fresh WWW-Authenticate from this 402>' \
  --channel-id '<saved channel_id>' \
  --cumulative-amount '<current_cum>' \
  --escrow '<saved escrow>' \
  --chain-id '<saved chain_id>' \
  --reuse-signature '<saved current_sig>' \
  [--from '<saved payer_addr>']
```

The CLI skips TEE signing and emits a fresh `authorization_header` that wraps the existing signature bytes verbatim. Output `mode` is `"reuse"`.

### S2.3b: Sign path (TEE)

```bash
onchainos payment mpp-session-voucher \
  --challenge '<fresh WWW-Authenticate from this 402>' \
  --channel-id '<saved channel_id>' \
  --cumulative-amount '<cum_for_this_call>' \
  --escrow '<saved escrow>' \
  --chain-id '<saved chain_id>' \
  [--from '<saved payer_addr>']
```

CLI signs an EIP-712 Voucher(channelId, cum_for_this_call) via TEE. Output `mode` is `"sign"`. Both paths return:

```json
{
  "ok": true,
  "data": {
    "protocol": "mpp",
    "action": "voucher",
    "mode": "reuse" | "sign",
    "authorization_header": "Payment eyJjaGFsbGVuZ2UiOnsi...",
    "channel_id": "0x...",
    "cumulative_amount": "<cum_for_this_call>",
    "signature": "0x<65-byte hex>"
  }
}
```

### S2.4: Replay the Business Request

```
<original method> <original url>
Authorization: <authorization_header>
```

Expected: `HTTP 200` with content. **Update session state**:

- `current_cum  = cum_for_this_call`
- `current_sig  = <signature from output>`
- `estimated_spent = estimated_spent + unit_amount`

(In the reuse path `current_cum` and `current_sig` are unchanged; only `estimated_spent` advances.)

### S2.5: Handle Insufficient-Balance Fallback

When seller rejects a voucher request, **first** extract the human-readable reason via the priority list in [§ Reading Seller Errors](#reading-seller-errors-important-for-ux). If the extracted reason mentions **insufficient balance** (e.g. `reason: "insufficient balance"`, `detail: "voucher exhausted"`, or — when the seller is the OKX Rust SDK — its private SDK code `70015` accompanies the message), the agent's `estimated_spent` drifted (cross-conversation, or another client consumed balance). Recover by:

1. **Surface the seller's reason to the user** in human form, e.g. `❌ Seller rejected the voucher: insufficient balance — existing voucher exhausted. Signing a new voucher to continue.`
2. Re-set `estimated_spent = current_cum` (assume the existing voucher is exhausted)
3. Go back to S2.2 — `remaining` now becomes 0, so the agent picks **SIGN**
4. Sign a new voucher with `cum = current_cum + unit_amount` and retry

Do **not** loop reuse-on-insufficient-balance — always escalate to sign.

For other voucher rejection reasons (`amount_exceeds_deposit` → topup; `delta_too_small` → raise cum; `invalid_signature` → check seller logs), surface the reason similarly and route per [§ Troubleshooting](#troubleshooting). Always show the user the seller's reason text first, the protocol code in parentheses second.

### S2.6: Loop

For another request to the same resource: repeat S2.1–S2.4. The same voucher can fund many requests as long as `remaining ≥ unit_amount`; a re-sign happens only when balance runs out.

> **Note**: voucher rejection errors come from **seller-side SDK local validation**, not from a network round-trip to MPP backend. Common ones: `70000 invalid_params` (cum not strictly increasing), `70004 invalid_signature`, `70012 amount_exceeds_deposit`, `70013 voucher_delta_too_small`, plus `InsufficientBalance` (no protocol code — emitted by mppx / OKX TS as a typed error; OKX Rust SDK uses private code `70015`).

## Phase S2b (Optional): TopUp Mid-Session

If `current_cum + unit_amount > deposit`, the channel needs more funds. The seller will typically refuse the next voucher with "70012 amount exceeds deposit" or pre-emptively send a topUp challenge.

Ask user:

> The channel deposit is running low. Top up by how much (atomic units)?
> Current deposit: `<deposit>`
> Current spent (highest voucher): `<current_cum>`

### Mode Branch

Read `methodDetails.feePayer` from the topUp challenge (typically same as open).

#### Transaction mode

```bash
onchainos payment mpp-session-topup \
  --challenge '<WWW-Authenticate for topUp>' \
  --channel-id '<saved channel_id>' \
  --additional-deposit '<atomic units>' \
  --escrow '<saved escrow>' \
  --chain-id '<saved chain_id>' \
  --currency '<saved currency>' \
  [--from '<saved payer_addr>']
```

CLI TEE-signs `receiveWithAuthorization` for the additional deposit. The EIP-3009 nonce is derived deterministically as `keccak256(abi.encode(channelId, additionalDeposit, from, topUpSalt))` — must match what the on-chain contract expects.

Output includes `authorization_header`. Send to seller's topUp endpoint.

#### Hash mode

User broadcasts the topUp tx (escrow `topUpWithAuthorization`) themselves. Then:

```bash
onchainos payment mpp-session-topup \
  --challenge '<WWW-Authenticate for topUp>' \
  --channel-id '<saved channel_id>' \
  --additional-deposit '<atomic units>' \
  --escrow '<saved escrow>' \
  --chain-id '<saved chain_id>' \
  --tx-hash '0x<64-char hex>' \
  [--from '<saved payer_addr>']
```

`--currency` is optional in hash mode (CLI doesn't sign EIP-3009; the on-chain tx already contains everything).

### After TopUp

**Update session state**: `deposit = deposit + additional_deposit`. Then resume Phase S2 (voucher loop).

## Phase S3: Close Channel

When the session is done (user-initiated or after the final business request).

### S3.1: Decide Final cumulativeAmount

```
final_cum = current_cum   // = highest voucher cumulativeAmount sent in this session
```

If you tracked `current_cum` correctly through Phase S2, this is just the last voucher's amount. **Don't add unit_amount here** — close uses the same cum as the last voucher, not a new one (no service is being delivered for close itself).

### S3.2: Sign Close Voucher

```bash
onchainos payment mpp-session-close \
  --challenge '<WWW-Authenticate for close, or a fresh 402 if seller issues one>' \
  --channel-id '<saved channel_id>' \
  --cumulative-amount '<final_cum>' \
  --escrow '<saved escrow>' \
  --chain-id '<saved chain_id>' \
  [--from '<saved payer_addr>']
```

CLI signs an EIP-712 Voucher(channelId, final_cum) via TEE — same signing path as a regular voucher, just used at close time.

Output:

```json
{
  "ok": true,
  "data": {
    "protocol": "mpp",
    "action": "session_close",
    "authorization_header": "Payment eyJjaGFsbGVuZ2UiOnsi...",
    "channel_id": "0x...",
    "cumulative_amount": "100"
  }
}
```

### S3.3: Send Close to Seller

```
<original method> <original url>     # typically a dedicated close endpoint, e.g. /session/manage
Authorization: <authorization_header>
```

Seller settles on-chain (transfers `final_cum` from escrow to merchant, refunds the rest to payer) and returns a final receipt. **Clear session state** — channel is closed.

### S3.4: Confirm to User

> ✅ Session closed. Settled `<human(final_cum)> (<final_cum>)` of `<human(deposit)> (<deposit>)`. Refund: `<human(deposit - final_cum)> (<deposit - final_cum>)` returned to your wallet.
> On-chain tx: `<reference from response>`

---

# Hash Mode Workflow Detail

When `feePayer=false`, the user/agent must broadcast the on-chain transaction themselves. This applies to:

- Charge in hash mode → broadcast `transferWithAuthorization`
- Session open in hash mode → broadcast escrow `openWithAuthorization`
- Session topUp in hash mode → broadcast escrow `topUpWithAuthorization`
- Session close → seller broadcasts; client never broadcasts a close tx

**Recommended path**: delegate to `okx-onchain-gateway` skill — that skill handles tx construction, gas estimation, broadcasting, and waiting for the receipt. Once the user has the tx hash, return here and pass it via `--tx-hash`.

**Manual path**: user broadcasts with their own tooling (e.g. ethers.js, foundry, Metamask). They paste the 66-char `0x...` hash into the next CLI command.

The CLI does NOT verify the tx hash itself — the seller's MPP backend will verify on-chain when the seller submits the credential. If the tx is invalid (wrong contract, wrong sender, wrong amount), the seller will reject with an error code.

---

# Reading Seller Errors (Important for UX)

When the seller returns an error response (HTTP 4xx / 5xx, or even HTTP 200 with an `error` field), **do not show the user the raw JSON or the protocol code alone**. Different MPP server implementations use different field names for the human-readable explanation. Extract and surface the most readable string by checking these fields **in priority order**, and use the **first non-empty match**:

```
1. body.reason          ← mppx, OKX TS Session (e.g. "voucher amount below current")
2. body.detail          ← RFC 9457 ProblemDetails (mpp-rs, OKX Rust SDK via to_problem_details)
3. body.message         ← generic, some Java backends
4. body.msg             ← OKX SA API native shape
5. body.error           ← example servers / lightweight handlers
6. body.title           ← RFC 9457 short title (less specific than detail; use only as fallback)
7. fallthrough          ← if none of the above, format the whole body and add the HTTP status
```

Numeric codes (`70004`, `70013`, etc.) are useful **next to** the human reason, never as a substitute. Format errors to the user as:

> ❌ Seller rejected: `<reason text>` (code `<code if present>`, HTTP `<status>`)

Examples of good vs bad messaging:

| ❌ Don't say | ✅ Say instead |
|---|---|
| "Got 70013" | "Seller rejected the voucher: voucher cumulative not strictly increasing (delta ≤ 0). The new cum must be strictly higher than the last accepted voucher." |
| "Error response: `{...}`" | "Seller returned: insufficient balance — the existing voucher has only 50 units of 400 left. Need to sign a new voucher with higher cum, or topup the channel." |
| "70015" | "Seller says the channel balance is exhausted. Sign a new voucher with `cum = current_cum + unit_amount` to continue." |

This applies in every error path: voucher submission, settle, close, topup, and the initial 402 challenge response.

---

# Troubleshooting

| Symptom                                              | Likely cause                                   | Fix                                                            |
|------------------------------------------------------|------------------------------------------------|----------------------------------------------------------------|
| `not logged in` / `session expired`                  | Wallet session missing or expired              | `onchainos wallet login` or `onchainos wallet login <email>`   |
| Voucher rejected: `70012 amount_exceeds_deposit`     | cumulativeAmount > channel deposit             | Phase S2b TopUp first                                          |
| Voucher rejected: `70000 invalid_params` (cum not strictly increasing) | new_cum ≤ current_cum     | Increase strictly; ensure you're tracking current_cum          |
| Voucher rejected: `70013 voucher_delta_too_small`    | Delta below `minVoucherDelta`                  | Raise cumulativeAmount by at least the minimum                 |
| Voucher rejected: `InsufficientBalance` (HTTP 402; OKX Rust SDK private code `70015`) | seller's spent + new_amount > highest_voucher (often hit during reuse when `estimated_spent` drifted) | Set `estimated_spent = current_cum`, fall through to SIGN path with `cum = current_cum + unit_amount` (S2.5) |
| Open fails: `chain not found`                        | Unsupported chainId or chain entry missing     | `onchainos wallet chains` to list supported chains             |
| `--tx-hash` rejected: `must be 0x + 64 hex chars`    | Malformed hash                                 | Copy full 66-char hash (with `0x` prefix)                      |
| Session 402 keeps repeating after voucher sent       | channel_id / escrow / chain_id mismatch        | Re-check saved session state; all three must match the open    |
| `30001 incorrect params`                             | Wrong field set, wrong base URL, http→https redirect | Verify `MPP_SA_URL` is `https://...` (not `http://`)      |
| `70004 invalid signature`                            | EIP-3009 typename mismatch, wrong nonce, wrong domain | Check seller logs; usually means CLI is older than spec   |
| `70008 channel finalized`                            | Channel was already closed on-chain            | Session is done; do not retry close                            |
| `70010 channel not found`                            | Wrong channel_id, or seller has no record      | Verify channel_id against open response                        |
| Seller returns ETIMEOUT or hangs                     | SA backend down or slow                        | Wait + retry; SDK has 30s timeout                              |

---

# Security Notes

- **TEE signing is the only supported signing path for MPP** — the private key stays inside the Trusted Execution Environment, not accessible to this CLI, the host OS, or the agent. Local private key signing is **NOT** supported for MPP (only x402 supports the local-key fallback).
- **Hash mode reveals the tx hash publicly** (standard blockchain behavior). Verify the `to` address (escrow contract for session, recipient for charge) before broadcasting.
- **Session deposits are escrowed** — if you abandon a session without closing, your deposit remains locked until the seller closes it or the on-chain timeout fires (typically 12-24 hours). **Always close** when done.
- **`cumulativeAmount` is monotonically increasing per channel** — never reuse or decrease across vouchers in the same session. Each channel has its own counter starting from the initial-cum baseline (default 0).
- **`channelId` is bytes32, not random** — it's `keccak256(abi.encode(payer, payee, token, salt, authorizedSigner, escrow, chainId))`. Two opens with the same parameters produce the same channelId — the on-chain contract rejects duplicates.

---

# EIP-712 Voucher Signing — Single Source of Truth

For developers integrating with MPP at the protocol level. The voucher EIP-712 typed data is the **single source of truth** shared by client SDK, seller SDK, and on-chain contract:

```
domain:
  name: "EVM Payment Channel"
  version: "1"
  chainId: <runtime>
  verifyingContract: <escrow address>

Voucher:
  bytes32 channelId
  uint128 cumulativeAmount
```

The CLI computes this internally for `mpp-session-voucher`, the initial voucher in `mpp-session-open`, and the close voucher in `mpp-session-close`. The seller SDK uses the identical typed data to verify signatures locally — no MPP server round-trip for voucher verification (`/session/voucher` is not part of the protocol).

If the merchant has forked the escrow contract with a different `name` or `version`, they configure the seller SDK via `with_domain_meta(name, version)` to match.

---

# Notes on the Open Payload (initial voucher fields)

When the CLI sends `mpp-session-open`, the credential payload carries the initial voucher (`cumulativeAmount` + EIP-712 signature) so the seller SDK can verify and store the baseline locally. **The voucher signature field name is mode-dependent** — there's no `signature` collision in transaction mode because that key is taken by the EIP-3009 deposit signature:

```
# transaction mode (feePayer=true)
payload.signature          // EIP-3009 deposit signature — SA needs this
payload.cumulativeAmount   // initial voucher amount, e.g. "0"  (SDK-only)
payload.voucherSignature   // EIP-712 voucher signature        (SDK-only)

# hash mode (feePayer=false)
payload.hash               // tx hash of the on-chain open — SA needs this
payload.cumulativeAmount   // initial voucher amount, e.g. "0"  (SDK-only)
payload.signature          // EIP-712 voucher signature        (SDK-only — hash mode has no EIP-3009 sig, so the voucher sig directly occupies `signature`)
```

The "SDK-only" fields are read by the seller's MPP SDK to verify and store the baseline voucher in its `ChannelRecord`, then **stripped before the credential is forwarded to SA**. The strip set is type-dependent:

- transaction → strip `cumulativeAmount` + `voucherSignature` (keep `signature` because it's the EIP-3009 deposit sig)
- hash → strip `cumulativeAmount` + `signature` (the entire `signature` is the voucher sig)

Agents and integrators don't need to do anything special; both `--initial-cum N` and `--prepay-first` flags handle this end-to-end.
