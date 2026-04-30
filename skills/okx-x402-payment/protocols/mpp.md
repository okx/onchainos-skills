# MPP Protocol Playbook

Sign MPP authorizations — **charge** (one-shot) and **session** (open / voucher / topUp / close) in transaction or hash mode.

> **Entry point**: loaded by `../SKILL.md` when the 402 response has `WWW-Authenticate: Payment ... method="evm"`. Pre-flight, routing, chain support, and protocol detection are the dispatcher's job — start here at Background.

> **Talk to users in plain language.** Match the user's language (中文用中文,English uses English). Use action-verb phrasing like "issue a voucher / 签发凭证", "top up your balance / 补充余额", "close the channel / 关闭通道", "your prepaid balance / 通道余额" — don't dump bare jargon (`voucher`, `topUp`, `close`, `escrow`, `cumulativeAmount`) on the user. Technical field names are fine in **state echo** (`📋 Channel ... cum ... sig`) since the user copy-pastes those across sessions.

## Background

Two intents: **charge** (one-shot — sign once, seller settles) and **session** (open channel + N vouchers + close — off-chain vouchers, batched on-chain settlement). Authorization header is `Authorization: Payment <base64url>` over the JCS-canonicalised envelope `{challenge, source, payload}`. The CLI returns a ready `authorization_header` — agent just pastes it.

Two delivery modes (per `methodDetails.feePayer` in the 402 challenge):
- **`feePayer=true` → transaction mode**: CLI TEE-signs EIP-3009; seller broadcasts.
- **`feePayer=false` → hash mode**: user broadcasts the tx and passes `--tx-hash`; CLI still TEE-signs off-chain pieces (initial voucher etc.).

**Method check** — only `method="evm"` is supported here. If `method` is `"tempo"`, `"svm"`, `"stripe"`, etc. → stop and tell the user this playbook cannot handle it.

## Command Index

| # | Command                              | Intent  | Purpose                                                                |
|---|--------------------------------------|---------|------------------------------------------------------------------------|
| 1 | `onchainos payment mpp-charge`          | charge  | One-shot charge payment (transaction mode default; hash mode optional) |
| 2 | `onchainos payment mpp-session-open` | session | Open a payment channel (always first in a session)                     |
| 3 | `onchainos payment mpp-session-voucher`      | session | Sign a voucher for each business request                               |
| 4 | `onchainos payment mpp-session-topup`| session | Add more deposit to an open channel (optional)                         |
| 5 | `onchainos payment mpp-session-close`| session | Close the channel and settle                                           |

All five commands return `data.authorization_header` — paste as `Authorization:` when retrying the request.

**`--base-url`** (all commands) overrides the backend URL for staging / forked / testnet endpoints. **Always `https://`** — `http://` causes a 301 POST→GET redirect that drops the body, surfacing as `30001 incorrect params`. Omitted = production default.

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

**Challenge expiry** — if `expires=...` (ISO-8601) is in the past, the challenge is dead: re-send the original request to get a fresh 402 before signing. Stale challenges fail with `30001 incorrect params`.

**MANDATORY STOP — display these details and wait for explicit confirmation:**

> This resource requires payment:
> - **Payment type**: `<one-shot purchase (charge) | streaming session (multi-request)>`
> - **Network**: `<chain name>` (`eip155:<chainId>`)
> - **Token**: `<symbol>` (`<currency address>`)
> - **Amount per request**: `<human-readable>` (atomic: `<amount>`)
> - **Pay to**: `<recipient>`
> - **Who pays gas**: `<server (transaction mode) | you broadcast it yourself (hash mode)>`
> - **Split recipients** (one-shot only, if present): `<N other parties also receive a share>`
> - **Suggested prepaid balance** (session only, if present): `<human-readable>`
>
> Proceed with payment? (yes / no)

**Do not call `onchainos wallet status` or any other tool until the user confirms.**

Confirm → Step 2; decline → stop, no payment made, no wallet check.

## Step 2: Check Wallet Status

After user confirms, run `onchainos wallet status`. Logged in → Step 3. Not logged in → ask:

> You are not logged in. How would you like to authenticate?
> 1. **Email login** — `onchainos wallet login <email>` (OTP)
> 2. **API Key login** — `onchainos wallet login` (uses `OKX_API_KEY` / `OKX_SECRET_KEY` / `OKX_PASSPHRASE` env)
> 3. **Cancel**

Wait for the user. **MPP requires TEE** — local private key signing is not supported (only x402 has that fallback).

## Step 3: Sign and Assemble

Branch by `intent`: `charge` → [§ Charge](#charge-flow); `session` → [§ Session](#session-flow).

---

# Charge Flow

One-shot payment. CLI TEE-signs EIP-3009 (or wraps a client-broadcast tx hash) and returns `authorization_header`.

## Charge Step 1: Decide Mode

`methodDetails.feePayer=true` (default) → transaction mode (server pays gas). `feePayer=false` → hash mode (user broadcasts first).

## Charge Step 2a (transaction mode): Sign

```bash
onchainos payment mpp-charge \
  --challenge '<full WWW-Authenticate header value>' \
  [--from '<0xPayer>']
```

CLI auto-detects `methodDetails.splits[]` — no extra flag needed. Output: `{ ok, data: { authorization_header, wallet, ... } }`. Save `data.authorization_header` and skip to [Charge Step 3: Replay](#charge-step-3-replay).

## Charge Step 2b (hash mode): Broadcast First, Then Wrap

When `feePayer=false`, user must broadcast `transferWithAuthorization` themselves first. Ask:

> The seller isn't paying gas, so you need to send the payment transaction on-chain yourself first, then give me the tx hash. How would you like to send it?
> 1. **Help me send it** — switch to `okx-onchain-gateway` (recommended)
> 2. **I'll send it manually** — paste the tx hash when ready

Option 1: delegate to `okx-onchain-gateway`, return here with the hash. Option 2: wait for a 66-char `0x...` hash.

```bash
onchainos payment mpp-charge \
  --challenge '<full WWW-Authenticate header value>' \
  --tx-hash '0x<64-char hex>' \
  [--from '<0xPayer>']
```

Output shape same as transaction mode but `mode: "hash"`. Save `authorization_header`.

## Charge Step 3: Replay

```
<original method> <original url>
Authorization: <authorization_header>
```

Expected: `HTTP 200` with content + `Payment-Receipt` header (on-chain tx hash). Charge complete. Another 402 → see [§ Troubleshooting](#troubleshooting) (replay / expired challenge).

---

# Session Flow

State machine: **open → N vouchers → close** (optional topUp). Each phase has its own CLI command and `Authorization` header.

## Session State to Track

Save these the moment `mpp-session-open` returns and maintain across phases:

| Field             | Source                                                                              |
|-------------------|--------------------------------------------------------------------------------------|
| `channel_id`      | `mpp-session-open` output                                                            |
| `escrow`          | open challenge `methodDetails.escrowContract`                                        |
| `chain_id`        | open challenge `methodDetails.chainId`                                               |
| `currency`        | open challenge `currency`                                                            |
| `payer_addr`      | open output `wallet`                                                                 |
| `current_cum`     | highest signed cum so far (open `--initial-cum` or last issued voucher's cum)        |
| `current_sig`     | last voucher signature (`signature` field of open / voucher / close output)          |
| `estimated_spent` | sum of `unit_amount` across all served business requests since the last fresh sign   |
| `unit_amount`     | latest voucher challenge `amount` (seller is authoritative)                          |
| `deposit`         | open output `deposit` + topup `--additional-deposit`                                 |

Track in conversation context. Across conversations, ask the user to re-supply `channel_id` / `escrow` / `current_cum` / `current_sig` to continue a session.

**Mandatory state echo** — after `mpp-session-open`, after each voucher (sign or reuse), after topup, and immediately before close, end your message with one line:

> 📋 Channel `<channel_id>` · chain `<chain_id>` · escrow `<escrow>` · deposit `<human(deposit)>` (`<deposit>`) · cum `<human(current_cum)>` (`<current_cum>`) · spent~`<human(estimated_spent)>` (`<estimated_spent>`) · sig `<current_sig prefix...>`

**All user-facing amounts in BOTH human and atomic form** — `<human> (<atomic>)`, e.g. `0.0004 USDC (400)`, `1.5 ETH (1500000000000000000)`. Compute via `amount / 10^decimals` from the challenge `currency` token (typically 6 for USDC/USD₮, 18 for native — **never assume**; query `okx-dex-token` if uncertain). Applies everywhere: state echo, confirmation prompts, deposit suggestions, settle / close summaries.

## Phase S1: Open Channel

First step of any session. Decide the **deposit** with the user:

> A streaming session needs you to lock a prepaid balance up front (held in escrow). How much would you like to prepay?
> Suggested: `<human(suggestedDeposit)> (<suggestedDeposit>)` (or `unit_amount × 100` if no suggestion — enough for ~100 requests).
> You can give a human amount like `0.01 USDC` or atomic units (the CLI takes atomic — I'll convert).
> Each request draws from this balance. You can add more later, or close the channel anytime to refund whatever's unused.

Wait for user's amount.

### Optional: Initial Voucher Prepay

Opening a channel signs a baseline voucher with `cumulativeAmount=0` by default. To override:
- `--initial-cum N` — explicit baseline (atomic units).
- `--prepay-first` — use the unit price from `challenge.amount` (silently falls back to 0 if missing/`"0"`).

Pick from user intent: no preference → no flag; "pay first request immediately" → `--prepay-first`; "pre-authorize N" → `--initial-cum N`. Constraint: `initial_cum ≤ deposit` (SDK rejects with `70012` otherwise).

### Mode Branch

Branch by `methodDetails.feePayer`.

**Transaction mode (`feePayer=true`):**
```bash
onchainos payment mpp-session-open \
  --challenge '<full WWW-Authenticate header value>' \
  --deposit '<atomic units>' \
  [--initial-cum '<atomic>' | --prepay-first] \
  [--from '<0xPayer>']
```

CLI TEE-signs EIP-3009 `receiveWithAuthorization` (deposit into escrow) + EIP-712 baseline Voucher (channelId, cum=initial_cum). Output: `data.{authorization_header, channel_id, escrow, chain_id, deposit, wallet}` — save all to session state. Initial `current_cum` = initial-cum value (default `"0"`).

**Hash mode (`feePayer=false`)** — user must send the on-chain "open channel" transaction themselves first (delegate to `okx-onchain-gateway` or manual, same prompt as charge S2b). Then:

```bash
onchainos payment mpp-session-open \
  --challenge '<full WWW-Authenticate header value>' \
  --deposit '<atomic units>' \
  --tx-hash '0x<64-char hex>' \
  [--initial-cum '<atomic>' | --prepay-first] \
  [--from '<0xPayer>']
```

CLI still TEE-signs the initial voucher (EIP-712); only the deposit tx is replaced by the supplied hash.

### Send Open to Seller

```
<original method> <original url>
Authorization: <authorization_header>
```

Outcomes:
- **HTTP 200** — channel open, response carries the first business result. Echo saved state (channel_id / deposit / current_cum). Subsequent requests to the same resource: send without `Authorization` first; seller responds with a voucher 402 → Phase S2.
- **HTTP 402 (fresh `WWW-Authenticate: Payment`)** — channel opened but seller wants the first voucher signed. Go straight to Phase S2.

## Phase S2: Business Request (Voucher Loop)

Run for **each** business request during the session.

**Enter triggers** (when `channel_id` is active): user says "next request" / "again" / "another one" / "再调一次" / "再发一个" / "继续" / "voucher" / "凭证" / "签一个授权"; or user requests the resource again and gets a fresh 402.

### How vouchers actually work

A voucher is a **cumulative authorization**, not a single-request payment. Once signed, the seller keeps deducting until `spent` reaches the signed `cumulativeAmount`. So one voucher with `cum=50` funds 50× `unit_amount=1` requests **without re-signing** — provided the seller supports reuse (mppx / OKX TS Session / OKX Rust SDK ≥ this version). Legacy OKX Rust SDK treats byte-replay as idempotent retry and skips deduct; force re-sign every request if you suspect this.

Per-request job: pick **reuse** vs **sign** based on remaining balance.

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
  --reuse-signature '<saved current_sig>' \
  [--from '<saved payer_addr>']
```

Don't pass `--escrow` / `--chain-id` here — the existing signature already binds them. CLI skips TEE and wraps the existing signature bytes verbatim. `mode = "reuse"`.

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

CLI signs an EIP-712 Voucher(channelId, cum_for_this_call) via TEE. Output `mode` is `"sign"`. Both paths return: `data.{authorization_header, channel_id, cumulative_amount, signature, mode: "reuse"|"sign"}`.

### S2.4: Replay the Business Request

```
<original method> <original url>
Authorization: <authorization_header>
```

Expected: `HTTP 200` with content. **Update state**: `current_cum = cum_for_this_call`, `current_sig = <signature>`, `estimated_spent += unit_amount`. (Reuse path: `current_cum` / `current_sig` unchanged; only `estimated_spent` advances.)

### S2.5: Handle Insufficient-Balance Fallback

When the seller rejects a voucher, extract the reason via [§ Reading Seller Errors](#reading-seller-errors-important-for-ux). If it indicates **insufficient balance** (e.g. `reason: "insufficient balance"`, `detail: "voucher exhausted"`, or OKX Rust SDK private code `70015`), `estimated_spent` drifted. Recover:

1. Surface the seller's reason to the user, e.g. `❌ Seller rejected: insufficient balance — your current authorization is fully used. Signing a new one to continue.`
2. Set `estimated_spent = current_cum` (treat existing voucher as exhausted).
3. Re-enter S2.2 — `remaining = 0`, so **SIGN** is picked.
4. Sign a new voucher with `cum = current_cum + unit_amount` and retry.

**Do NOT loop reuse-on-insufficient-balance** — always escalate to SIGN.

Other rejections: `amount_exceeds_deposit` → topup (S2b); `delta_too_small` → raise cum; `invalid_signature` → check seller logs. Always surface the seller's reason text first, code in parens second.

### S2.6: Loop

Repeat S2.1–S2.4 for each request. Same voucher funds many calls while `remaining ≥ unit_amount`; re-sign only when balance runs out.

> Voucher rejections come from **seller-SDK local validation**, not a backend round-trip. Common: `70000` (cum not increasing), `70004` (invalid signature), `70012` (amount > deposit), `70013` (delta too small), plus `InsufficientBalance` (mppx/OKX TS typed error; OKX Rust SDK private `70015`).

## Phase S2b (Optional): TopUp Mid-Session

If `current_cum + unit_amount > deposit`, the channel needs more funds (seller will refuse with `70012` or pre-emptively send a topUp challenge).

Ask user:

> Your prepaid balance is running low. How much would you like to add (atomic units)?
> Current balance: `<human(deposit)> (<deposit>)` · Used so far: `<human(current_cum)> (<current_cum>)`

Branch by `methodDetails.feePayer` from the topUp challenge (typically same as open).

**Transaction mode:**
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

CLI TEE-signs `receiveWithAuthorization`. EIP-3009 nonce is `keccak256(abi.encode(channelId, additionalDeposit, from, topUpSalt))` — must match the on-chain contract.

**Hash mode** (user sends the on-chain "top-up" transaction themselves first, then):
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

`--currency` is optional in hash mode (CLI doesn't sign EIP-3009; the on-chain tx already covers it).

**After TopUp**: `deposit = deposit + additional_deposit`. Resume Phase S2.

## Phase S3: Close Channel

When the user is done — either says "close the channel / 关闭通道 / end the session", or after the final request. **Always close** when the user is done; otherwise the prepaid balance stays locked on-chain until the seller's timeout (typically 12-24h).

### S3.1: Decide Final cumulativeAmount

`final_cum = current_cum` — the highest voucher cum sent in this session. **Don't add `unit_amount`** — close reuses the last voucher's cum (no new service is being delivered).

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

CLI signs an EIP-712 Voucher(channelId, final_cum) via TEE — same signing path as a regular voucher, just used at close time. Output: `data.{authorization_header, channel_id, cumulative_amount}`.

### S3.3: Send Close to Seller

```
<original method> <original url>     # typically a dedicated close endpoint, e.g. /session/manage
Authorization: <authorization_header>
```

Seller settles on-chain (transfers `final_cum` to merchant, refunds the rest to payer) and returns a receipt. **Clear session state** — channel is closed.

### S3.4: Confirm to User

> ✅ Channel closed. Charged `<human(final_cum)> (<final_cum>)` of your `<human(deposit)> (<deposit>)` prepaid balance. Refund of `<human(deposit - final_cum)> (<deposit - final_cum>)` returned to your wallet.
> On-chain tx: `<reference from response>`

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

Numeric codes (`70004`, `70013`, etc.) are useful **next to** the human reason, never as a substitute. Format errors as:

> ❌ Seller rejected: `<reason text>` (code `<code if present>`, HTTP `<status>`)

Applies in every error path: voucher submission, settle, close, topup, and the initial 402 challenge response.

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

- **TEE-only signing for MPP** — no local private key path (x402 has one; MPP doesn't).
- **Always close sessions** — abandoned deposits stay escrowed until seller closes or the on-chain timeout fires (typically 12–24h).
- **`cumulativeAmount` monotonically increases per channel** — never decrease or reuse across vouchers in the same session.
- **`channelId` is deterministic** — `keccak256(abi.encode(payer, payee, token, salt, authorizedSigner, escrow, chainId))`; identical parameters produce duplicate channelIds and the contract rejects them.

