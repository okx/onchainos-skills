# a2a_charge — agent-to-agent payment links (`onchainos payment a2a-pay`)

> **CLI down-sink:** don't self-sleep/poll for status — use
> `onchainos payment a2a-pay status --payment-id <id> --wait` to poll internally
> (3s interval, 60s ceiling) until a terminal state; read `data.terminal` /
> `data.timed_out`. create/pay NL→command routing stays here.

> Loaded from `../SKILL.md` when the user mentions a paymentId, an `a2a_...` link, "create payment link", or asks to check a2a payment status. Unlike the HTTP 402 paths (`accepts`-based and `WWW-Authenticate: Payment`), a2a is **not triggered by an HTTP 402 response** — it's invoked by name, with a paymentId or a seller's create-link request.

Wraps `onchainos payment a2a-pay` for seller (`create`) and buyer (`pay` / `status`) roles. Buyer-side trust is **delegated upstream** (see Trust model below).

## Pre-flight

`create` and `pay` need a live wallet session — the dispatcher's Step B2 already checked it. If you entered here directly, run `onchainos wallet status` first; not logged in → `onchainos wallet login`. Never sign without a live session.

---

## Seller — Create a Payment Link

**Inputs**:
- **Required**: `--amount` (decimal, e.g. `"0.01"`), `--symbol` (e.g. `"USDT"`), `--recipient` (0x... EVM address — seller wallet)
- **Optional**: `--description`, `--realm`, `--expires-in` (seconds, default 1800)

**Steps**:

1. Run pre-flight (see above).
2. Shell out:
   ```bash
   onchainos payment a2a-pay create \
     --amount <amount> --symbol <symbol> --recipient <recipient> \
     [--description <text> --realm <domain> --expires-in <seconds>]
   ```
3. Parse the response — only `payment_id` and `deliveries.url` (optional) are present. The CLI no longer returns `amount` / `currency`; echo the seller's input args back for display.
4. Display:

   > Payment link created.
   > • paymentId: `<id>`
   > • Amount: `<amount input> <symbol input>` (decimal as you submitted)
   > • Recipient: `<recipient input>`
   > • Share with buyer: `<deliveries.url>` (if returned by the server) or `paymentId=<id>`

5. Suggest next: poll status anytime with `onchainos payment a2a-pay status --payment-id <id>` once the buyer is expected to have paid.

---

## Buyer — Pay a Payment Link

**Required input**: `paymentId` only. The CLI fetches the seller-issued challenge from the server and signs whatever amount / currency / recipient the challenge declares.

> **Trust model**: the buyer signs the seller's challenge as-is. Verifying that the challenge matches what the buyer agreed to pay is the **upstream caller's responsibility** — the user (or the upstream skill) MUST cross-check the seller's `paymentId` / `deliveries.url` against their out-of-band agreement (chat, task spec, prior negotiation) **before** calling this skill. Once invoked, the skill signs whatever the on-server challenge declares.

### Step 1 — Sign and submit

The skill does not run its own preview / yes-no gate; trust is delegated upstream. Shell out directly:

```bash
onchainos payment a2a-pay pay --payment-id <paymentId>
```

The CLI fetches the on-server challenge, TEE-signs the EIP-3009 authorization, and submits the credential. Two outcomes:

**Accepted** — `ok:true`, exit 0; `data` carries `payment_id` / `status` / `tx_hash` / `signature`. Proceed to Step 2 (auto-poll).

**Rejected** — server returned `data.success:false` (e.g. `errorReason:"insufficient_balance"`). CLI surfaces it as a hard failure: `ok:false`, exit code 1, message embeds the reason verbatim. A generic rejection is just:

```json
{ "ok": false, "error": "payment a2a_xxx rejected (reason=<errorReason>)" }
```

When the reason is `insufficient_balance`, the CLI returns the standard
structured blocked result under `data` (see Insufficient-balance top-up below):

```json
{
  "ok": false,
  "data": {
    "phase": "funding_required",
    "decision": "blocked",
    "reason": "insufficient_balance",
    "nextAction": [],
    "payload": {
      "operation": "a2a_payment",
      "fundingTarget": {},
      "qr": {},
      "fundingNeed": {}
    }
  }
}
```

This paymentId is terminal; `pay` is not retried on it. The Funding payload does
not carry the paymentId or other payment-specific state. Keep that rule in this
Reference and use current conversation context when the user later continues.
With `data` present, run Insufficient-balance top-up below; without it, relay
what failed, suggest the obvious remedy, and stop.

### Troubleshooting — insufficient-balance top-up

When `data` carries `phase=funding_required`, `decision=blocked`,
and `reason=insufficient_balance`:

1. Follow the Wallet Skill's common
   [funding.md](../../okx-agentic-wallet/references/funding.md) immediately. Its
   shared Funding-required template displays the balance, shortfall, address,
   and QR in the same response.
2. After shared Funding verifies a sufficient balance, it asks whether to
   continue using the current conversation context. If the user continues the
   payment, require a new seller-issued paymentId/link; the failed paymentId is
   terminal. If the original payment is no longer clear, ask for it.
3. Once a new paymentId arrives, re-enter the ordinary Buyer — Pay/task-creation
   Reference and its existing confirmation rules. The new CLI result replaces
   the old funding result.

### Step 2 — Auto-poll status to terminal

Status classification:

- **Non-terminal** (poll): `pending`, `settling`
- **Terminal** (stop): `completed`, `failed`, `expired`, `cancelled`

If `status` is already terminal → render the result and stop.

If non-terminal → poll every **3 seconds**, up to a **60-second** total budget:

```bash
onchainos payment a2a-pay status --payment-id <paymentId>
```

- As soon as a terminal status is observed → render full result (status + tx_hash + block_number) and stop.
- If 60 seconds elapse and the status is still non-terminal → return the current `status` plus the paymentId, and tell the user: "Status is still `<status>` after 60s; you can run `status` again later."

**Terminal display strings**:

| status | Display |
|---|---|
| `completed` | "✅ Payment confirmed on-chain. tx_hash: `<tx_hash>` block: `<block_number>`" |
| `failed` | "❌ Payment failed. (include the server-provided reason if any)" |
| `expired` | "⌛ Payment link expired before settlement. Ask the seller for a new one." |
| `cancelled` | "🚫 Seller cancelled this payment." |

---

## Status — Query Payment State

**Input**: `paymentId`.

```bash
onchainos payment a2a-pay status --payment-id <paymentId>
```

Map the returned `status` to a human-readable line:

| status | Meaning | Display |
|---|---|---|
| `pending` | Awaiting buyer signature | "⏳ Awaiting buyer signature." |
| `settling` | Credential received, settling on-chain | "🔄 Settling on-chain (credential submitted, awaiting confirmation)." |
| `completed` | Confirmed on-chain | "✅ Confirmed on-chain. tx_hash: `<tx_hash>` block: `<block_number>` fee: `<fee_decimal> <fee_symbol>`" |
| `failed` | Payment failed | "❌ Failed. (include the server-provided reason if any)" |
| `expired` | Expired before settlement | "⌛ Expired before settlement." |
| `cancelled` | Seller cancelled | "🚫 Cancelled by seller." |

**Rendering the fee**: the CLI returns `fee_amount` as a top-level string in minimal units (and `fee_bps` as the basis-points used). To compute `<fee_decimal>`, look up the token decimals (see Amount Display Rules below). For `<fee_symbol>`, reuse the `--symbol` the seller passed to `create` for the same `paymentId` — the upstream caller (or the seller flow that issued the link) is the source of truth; the `status` response itself does not echo it back. If neither is available, display `fee_amount` minimal units as-is.

**Suggest next**:
- `pending` / `settling` → "Check again in a few moments" or wait briefly and re-run `status`.
- `completed` → recommend `okx-agentic-wallet` to verify post-payment balance delta.
- `failed` → recommend checking buyer balance via `okx-agentic-wallet`, and if `tx_hash` is present, inspect it via `okx-agentic-wallet` (`onchainos security tx-scan`).

---

## Amount Display Rules

Convert `amount` / `fee_amount` per **`../_shared/amount-display.md`**.

**a2a exception (unlisted symbol):** a2a delegates trust upstream, so do **NOT** query `okx-dex-market` and do **NOT** block — use the unknown-decimals fallback (`<atomic> <symbol>` + "double-check") directly.

---

## Edge cases

| Scenario | Handling |
|---|---|
| Insufficient-balance intent | Enter the Wallet Skill's common [funding.md](../../okx-agentic-wallet/references/funding.md) immediately and render its shared balance, address, and QR template. |
| `onchainos wallet status` reports not logged in | Prompt user to run `onchainos wallet login`. Never attempt to sign without a live session. |
| User provides no `paymentId` | STOP and ask the user for the seller-issued paymentId. |
| CLI reports `payment ... not payable` / expired challenge / unsupported intent | Relay the error verbatim and surface as a **terminal failure** — do NOT retry signing. |
| CLI reports `payment ... rejected (reason=<errorReason>)` (post-signing credential refusal — `insufficient_balance`, etc.) | **Terminal for this paymentId**. Structured `insufficient_balance` data → enter shared Funding immediately and show its balance, address, and QR template. Other reasons → relay and ask the seller for a new link. **Do NOT retry `pay`** — burns a fresh nonce + signature without changing the outcome. |
| `paymentId` not found / 404 from server | Relay the error and ask the user to confirm the paymentId with the seller or upstream caller. |
| `pay` succeeded but status still `pending` / `settling` after 60s poll budget | Return the current status verbatim + paymentId; tell the user `Status is still <status> after 60s; you can run status again later`. |
| Server returns 5xx | Surface status code and any `errorMessage` verbatim. **Do not auto-retry `pay`** — every retry produces a fresh EIP-3009 nonce + signature; let the upstream decide. `status` is read-only and safe to retry manually. |
| `--symbol` is not in the hardcoded decimals table | Apply the unknown-decimals fallback (see Amount Display Rules). Do not block. |
| `--expires-in` was set too short and the link is now past its window | `status` returns `expired`; ask the seller to create a new link. |

---

## CLI Reference

### `onchainos payment a2a-pay create`

```bash
onchainos payment a2a-pay create \
  --amount <decimal> --symbol <symbol> --recipient <address> \
  [--description <text>] [--realm <domain>] [--expires-in <seconds>]
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--amount` | Yes | - | Decimal token amount (e.g. `"50"` or `"0.01"`) |
| `--symbol` | Yes | - | ERC-20 token symbol (e.g. `"USDT"`) |
| `--recipient` | Yes | - | Seller wallet address (= EIP-3009 `to`) |
| `--description` | No | - | Human-readable description shown to the buyer |
| `--realm` | No | - | Seller / provider domain (e.g. `provider.example.com`) |
| `--expires-in` | No | 1800 | Payment-link expiration window in seconds |

**Return fields**: `payment_id`, `deliveries` (object containing `url` when issued by the server).

### `onchainos payment a2a-pay pay`

```bash
onchainos payment a2a-pay pay --payment-id <id>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--payment-id` | Yes | - | Seller-issued paymentId |

**Return fields**: `payment_id`, `status`, `tx_hash` (optional), `valid_after`, `valid_before`, `signature`.

### `onchainos payment a2a-pay status`

```bash
onchainos payment a2a-pay status --payment-id <id>
```

| Param | Required | Default | Description |
|---|---|---|---|
| `--payment-id` | Yes | - | The paymentId to query |

**Return fields**: `payment_id`, `status`, `tx_hash` (optional), `block_number` (optional), `block_timestamp` (optional), `fee_amount` (optional, minimal units), `fee_bps` (optional).

## Quickstart

```bash
onchainos payment a2a-pay create --amount 0.01 --symbol USDT --recipient 0xSeller   # → { "payment_id": "a2a_xxx", "deliveries": {...} }
onchainos payment a2a-pay pay    --payment-id a2a_xxx                                # buyer signs on-server challenge as-is
onchainos payment a2a-pay status --payment-id a2a_xxx                                # auto-polled ~60s after pay if non-terminal
```
