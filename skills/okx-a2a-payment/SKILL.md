---
name: okx-a2a-payment
description: "Use this skill when the user mentions creating a payment link, paying a paymentId / a2a_... link, or checking a2a payment status. Wraps `onchainos a2a-pay` agent-to-agent payment protocol: seller-side `create`, buyer-side `pay` via EIP-3009 + TEE signing through a confirmation gate, and `status` query. Do NOT use for external HTTP 402 resources — use okx-x402-payment. Do NOT use for wallet balance / transfer / login — use okx-agentic-wallet."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
---

# Onchain OS A2A Payment

Wrap the `onchainos a2a-pay` CLI surface end-to-end for both seller and buyer roles. The skill enforces a hard human confirmation gate before any EIP-3009 signature, displays amounts in human-readable units, and auto-polls payment status to a terminal state after the buyer signs.

## Skill Routing

This skill only covers **internal a2a payments** issued via `onchainos a2a-pay`. If the request fits one of the intents below, route to the corresponding skill instead:

| Intent | Use skill |
|--------|-----------|
| External HTTP 402 payment-gated resource (any non-onchainos URL) | `okx-x402-payment` |
| Wallet balance / transfer / login | `okx-agentic-wallet` |
| Task publish / accept / deliver / verify (business layer) — payment sub-step calls back into this skill via the Workflow A contract | upstream task / agent skill (out of repo) |
| Internal `onchainos a2a-pay` payment link | **this skill** |

## Triggers

Skill activates on user intents that match any of:

- "create payment link", "create a2a payment", "generate payment", "create payment authorization"
- "pay paymentId", "pay a2a_...", "pay this link", "settle this payment"
- "payment status", "a2a payment status", "check payment status", "where is my payment"

## Pre-flight Checks

Both seller (`create`) and buyer (`pay`) require an authenticated wallet session. The CLI calls `ensure_tokens_refreshed` internally and bails on `not logged in`.

Before invoking `create` or `pay`:

```bash
onchainos wallet status
```

- **Logged in** → proceed.
- **Not logged in** → ask the user to log in via `onchainos wallet login` (AK login, no email) or `onchainos wallet login <email>` (OTP login). **Do NOT attempt to sign without a live session.**

`status` does not require additional pre-flight beyond what the CLI itself enforces.

## Operation Flow

### Seller — Create a Payment Link (`a2a-pay create`)

**Inputs**:

- **Required**: `--amount` (decimal token amount, e.g. `"0.01"`), `--symbol` (e.g. `"USDT"`), `--recipient` (0x... EVM address — seller wallet)
- **Optional**: `--description`, `--realm`, `--external-id`, `--expires-in` (seconds, default 1800)

**Steps**:

1. Run pre-flight (see above) — the CLI requires a live session.
2. Shell out:
   ```bash
   onchainos a2a-pay create \
     --amount <amount> --symbol <symbol> --recipient <recipient> \
     [--description <text> --realm <domain> --external-id <id> --expires-in <seconds>]
   ```
3. Parse the response — only `payment_id` and `deliveries.url` (optional) are present. The CLI no longer returns `amount` / `currency`; the skill echoes the seller's input args back for display.
4. Display to the user:

   > Payment link created.
   > • paymentId: `<id>`
   > • Amount: `<amount input> <symbol input>` (decimal as you submitted)
   > • Recipient: `<recipient input>`
   > • Share with buyer: `<deliveries.url>` (if returned by the server) or `paymentId=<id>`

5. Suggest next: poll status anytime with `onchainos a2a-pay status --payment-id <id>` once the buyer is expected to have paid.

### Buyer — Pay a Payment Link (`a2a-pay pay`)

**Required input**: `paymentId` only. The CLI fetches the seller-issued challenge from the server and signs whatever amount / currency / recipient the challenge declares.

> **Trust model**: the buyer signs the seller's challenge as-is. Verifying that the challenge matches what the buyer agreed to pay is the **upstream caller's responsibility**: the user (or the upstream skill) MUST cross-check the seller's `paymentId` / `deliveries.url` against their out-of-band agreement (chat, task spec, prior negotiation) **before** calling this skill. Once the skill is invoked, it will sign the on-server challenge.

#### Step 1 — Confirmation Gate (mandatory)

Display to the user:

> You are about to pay payment ID **`<paymentId>`**.
>
> This will fetch the seller-issued challenge and create an EIP-3009 signature via TEE that authorizes the transfer of whatever amount / currency / recipient the seller declared. Make sure the paymentId matches what you agreed with the seller out-of-band.
>
> Proceed? (yes / no)

**STOP and wait for the user's reply.** A reply of `no` (or anything that is not an explicit `yes`) terminates the flow — no signing, no further CLI calls.

> **Hard rule:** this skill always runs its own confirmation gate. Even if an upstream caller claims "the user already confirmed at the business layer", the skill still asks for an explicit yes/no before signing. Upstream callers MUST NOT attempt to bypass this.

#### Step 2 — Sign and Submit

Once the user confirms:

```bash
onchainos a2a-pay pay --payment-id <paymentId>
```

The CLI fetches the on-server challenge, TEE-signs the EIP-3009 authorization, and submits the credential. The successful response shape:

```json
{
  "payment_id": "a2a_xxx",
  "status": "<status>",
  "tx_hash": "<hash or null>",
  "valid_after": 0,
  "valid_before": 1746000000,
  "signature": "0x..."
}
```

#### Step 3 — Auto-poll Status to Terminal

Status classification:

- **Non-terminal** (poll): `pending`, `settling`
- **Terminal** (stop): `completed`, `failed`, `expired`, `cancelled`

If `status` is already terminal → render the result (see table below) and stop.

If non-terminal → poll every **3 seconds**, up to a **60-second** total budget:

```bash
onchainos a2a-pay status --payment-id <paymentId>
```

- As soon as a terminal status is observed → render full result (status + tx_hash + block_number) and stop.
- If 60 seconds elapse and the status is still non-terminal → return the current `status` plus the paymentId, and tell the user: "Status is still `<status>` after 60s; you can run `status` again later."

**Terminal display strings:**

| status | Display |
|--------|---------|
| `completed` | "✅ Payment confirmed on-chain. tx: `<tx_hash>` block: `<block_number>`" |
| `failed`    | "❌ Payment failed. (include the server-provided reason if any)" |
| `expired`   | "⌛ Payment link expired before settlement. Ask the seller for a new one." |
| `cancelled` | "🚫 Seller cancelled this payment." |

### Status — Query Payment State (`a2a-pay status`)

**Input**: `paymentId`.

**Steps**:

1. Run:
   ```bash
   onchainos a2a-pay status --payment-id <paymentId>
   ```
2. Map the returned `status` to a human-readable line:

   | status | Meaning | Display |
   |--------|---------|---------|
   | `pending`   | Awaiting buyer signature | "⏳ Awaiting buyer signature." |
   | `settling`  | Credential received, settling on-chain | "🔄 Settling on-chain (credential submitted, awaiting confirmation)." |
   | `completed` | Confirmed on-chain | "✅ Confirmed on-chain. tx: `<tx_hash>` block: `<block_number>` fee: `<decimal> <symbol>`" |
   | `failed`    | Payment failed | "❌ Failed. (include the server-provided reason if any)" |
   | `expired`   | Expired before settlement | "⌛ Expired before settlement." |
   | `cancelled` | Seller cancelled | "🚫 Cancelled by seller." |

3. If the response includes `fee.amount`, convert it from minimal units to decimal using the same decimals table as Step 1 of the Buyer flow.

4. Suggest next:
   - `pending` / `settling` → "Check again in a few moments" or wait briefly and re-run `status`.
   - `completed` → recommend `okx-agentic-wallet` to verify the buyer's post-payment balance delta.
   - `failed` → recommend checking buyer balance via `okx-agentic-wallet`, and if `tx_hash` is present, inspect it via `okx-security tx-scan`.

## Cross-Skill Workflows

### Workflow A — Sub-skill called from an upstream agent flow (most common)

Applicable upstream callers: any agent-to-agent task / chat / agent flow that holds the seller-issued payment information.

**Contract — upstream MUST hand off `paymentId`** (skill stops and asks the user if missing). Upstream is also responsible for confirming, before invoking this skill, that the `paymentId` matches the buyer's agreed terms — once invoked, the skill signs whatever the on-server challenge declares.

```
1. <upstream caller>     verifies paymentId matches the buyer's agreed terms → hands off paymentId
       ↓
2. okx-a2a-payment (this skill)  confirmation gate → onchainos a2a-pay pay → auto-poll status → display terminal state
       ↓
3. okx-agentic-wallet    optional: onchainos wallet balance to see post-payment delta
```

### Workflow B — Seller manually creates a payment link

```
1. okx-a2a-payment create   → paymentId + deliveries.url
2. Seller shares paymentId (and optionally deliveries.url) with the buyer out-of-band (chat / QR / message)
3. Buyer cross-checks the paymentId / deliveries.url against the seller's quoted terms, then runs Workflow A starting from step 2 with the received paymentId
```

### Workflow C — Payment failure triage

```
1. okx-a2a-payment status                 → expired / failed / cancelled
2. Branch on terminal state:
   - expired   → ask seller to create a new link
   - failed    → check buyer balance via okx-agentic-wallet; inspect tx_hash via okx-security tx-scan if present
   - cancelled → contact seller out-of-band
```

## Amount Display Rules

When converting `amount` (or `fee.amount`) from minimal units to a decimal display, use the hardcoded decimals table:

| Token | Decimals | "1000000" minimal renders as |
|-------|----------|------------------------------|
| USDC  | 6        | 1.00 USDC                    |
| USDT  | 6        | 1.00 USDT                    |
| USDG  | 6        | 1.00 USDG                    |
| ETH   | 18       | (`1e18` minimal = 1.00 ETH)  |

For any symbol not in the table: render `<minimal> <symbol>` and append the warning `unknown decimals — please double-check the seller-provided amount`. **Do not block** the flow.

## Edge Cases

| Scenario | Handling |
|----------|----------|
| `onchainos wallet status` reports not logged in | Prompt the user to run `onchainos wallet login`. Never attempt to sign without a live session. |
| User provides no `paymentId` | STOP and ask the user for the seller-issued paymentId. |
| Buyer replies anything other than an explicit `yes` at the confirmation gate | Terminate immediately. No signing. No further CLI calls. |
| CLI reports `payment ... not payable` / expired challenge / unsupported intent | Relay the error verbatim and surface it as a **terminal failure** — do NOT retry signing. |
| `paymentId` not found / 404 from server | Relay the error and ask the user to confirm the paymentId with the seller or upstream caller. |
| `pay` succeeded but status is still `pending` / `settling` after the 60s poll budget | Return the current status (verbatim) + paymentId; tell the user `Status is still <status> after 60s; you can run status again later`. |
| Server returns a 5xx | Retry once with a short backoff; if it fails again, surface the error. **Do not silently re-sign.** |
| `--symbol` is not in the hardcoded decimals table | Apply the unknown-decimals fallback (see Amount Display Rules). Do not block. |
| `--expires-in` was set too short and the link is now past its window | `status` returns `expired`; ask the seller to create a new link. |

## Command Index

| # | Command | Role | Purpose |
|---|---------|------|---------|
| 1 | `onchainos a2a-pay create` | Seller | Create a payment link, returns paymentId + deliveries |
| 2 | `onchainos a2a-pay pay`    | Buyer  | Fetch challenge → TEE-sign EIP-3009 → submit credential |
| 3 | `onchainos a2a-pay status` | Either | Query current status (pending / settling / completed / failed / expired / cancelled) |

## CLI Command Reference

### 1. `onchainos a2a-pay create`

```bash
onchainos a2a-pay create \
  --amount <decimal> --symbol <symbol> --recipient <address> \
  [--description <text>] [--realm <domain>] [--external-id <id>] [--expires-in <seconds>]
```

| Param | Required | Default | Description |
|-------|----------|---------|-------------|
| `--amount`      | Yes | - | Decimal token amount (e.g. `"50"` or `"0.01"`) |
| `--symbol`      | Yes | - | ERC-20 token symbol (e.g. `"USDT"`) |
| `--recipient`   | Yes | - | Seller wallet address (= EIP-3009 `to`) |
| `--description` | No  | - | Human-readable description shown to the buyer |
| `--realm`       | No  | - | Seller / provider domain (e.g. `provider.example.com`) |
| `--external-id` | No  | - | External business id (e.g. task id) |
| `--expires-in`  | No  | 1800 | Payment-link expiration window in seconds |

**Return fields**: `payment_id`, `deliveries` (object containing `url` when issued by the server).

### 2. `onchainos a2a-pay pay`

```bash
onchainos a2a-pay pay --payment-id <id>
```

| Param | Required | Default | Description |
|-------|----------|---------|-------------|
| `--payment-id` | Yes | - | Seller-issued paymentId |

**Return fields**: `payment_id`, `status`, `tx_hash` (optional), `valid_after`, `valid_before`, `signature`.

### 3. `onchainos a2a-pay status`

```bash
onchainos a2a-pay status --payment-id <id>
```

| Param | Required | Default | Description |
|-------|----------|---------|-------------|
| `--payment-id` | Yes | - | The paymentId to query |

**Return fields**: `payment_id`, `status`, `tx_hash` (optional), `block_number` (optional), `block_timestamp` (optional), `fee_amount` (optional, minimal units), `fee_bps` (optional).

## Quickstart

```bash
# Seller — create a payment link
onchainos a2a-pay create \
  --amount 0.01 --symbol USDT \
  --recipient 0xSellerWalletAddress
# → { "payment_id": "a2a_xxx", "deliveries": { "url": "..." } }

# Buyer — pay (skill displays confirmation gate before this CLI call)
onchainos a2a-pay pay --payment-id a2a_xxx

# Either side — query status (skill auto-polls this for ~60s after pay if non-terminal)
onchainos a2a-pay status --payment-id a2a_xxx
```
