---
name: okx-agent-payments-protocol
description: "Use when an agent hits HTTP 402 / payment-required, or the user mentions x402, x402Version, X-PAYMENT, PAYMENT-REQUIRED, PAYMENT-SIGNATURE, WWW-Authenticate: Payment, permit2, upto, metered billing, a payment channel / voucher / session, channelId / channel_id, opening / closing / topping up / settling / refunding a channel, a paymentId or a2a_ link, creating / checking a payment link, A2MCP / an A2MCP endpoint, or sending a request to / calling an Agent's endpoint with a concrete endpoint URL. Covers x402 (exact, exact+Permit2, upto, aggr_deferred), MPP (charge / session), and a2a-pay paymentId flows. Any close / topup / settle / voucher / refund near a channel_id or session is an MPP mid-session op. Two-phase quote/pay: `payment quote`, `payment pay --payment-id`, `decode-receipt`. The full bilingual trigger list (including Chinese) lives in the skill body."
license: MIT
metadata:
  author: okx
  version: "4.2.6"
  homepage: "https://web3.okx.com"
---

# OKX Agent Payments Protocol (Dispatcher)

> **⚠️ READ FIRST — ZERO-TEXT-ON-TRIGGER + NEVER-SKIP-USER-GATES.**
>
> Between detecting a 402 (or any trigger word) and emitting the first user-facing card — the Step A3.5 recommendation card, or the Step A4 confirmation card — output **ZERO** user-visible text. No "received 402", no "triggered OKX Agent Payments Protocol", no "detected N schemes", no enumeration of schemes / networks / tokens / amounts, no "loading skill" — in any language (the same prohibition applies to the equivalent phrases in any other language). The skill-load tool call may run but emits no surrounding prose.
>
> Exactly **one** confirmation card runs per payment: A3.5's recommendation card (2+ candidates and user picks `yes`) OR A4's confirmation card (single candidate, OR user picked an alternative from A3.5's expanded list). Do NOT skip the applicable card under the pretext of "past user preference" / "streamlining" / "already confirmed once" — those preferences do not exist. Do NOT render both cards back-to-back with the same info — after `yes` on A3.5.5, go straight to Step A5. The next user-visible text after detection MUST be one of the two cards.

Three payment paths, distinguished by HTTP signature: **`accepts`-based 402** (challenge in body for v1 or `PAYMENT-REQUIRED` header for v2), **`WWW-Authenticate: Payment` 402** (channel-capable, `intent="charge"` or `"session"`), and **a2a-pay** (paymentId-based, no 402). Shared steps below (detect → decode → confirm → wallet check), then dispatch to a reference.

> **User-facing terminology — IMPORTANT**
>
> **Rule 1 — Always call it "OKX Agent Payments Protocol", and always render it bolded.** Use the exact English term **OKX Agent Payments Protocol** in user-visible messages regardless of the user's language, and always wrap it in markdown bold (`**OKX Agent Payments Protocol**`) so the user sees it emphasized. Keep it as a fixed English noun phrase even inside otherwise-Chinese sentences. Reserve protocol literals and internal identifiers for CLI invocations, HTTP headers, JSON payloads, and code — never speak them to the user.
>
> **Rule 2 — Do not narrate internal protocol detection.** The dispatch logic (which header was detected, which reference is being loaded, which scheme/intent was selected, TEE vs local-key path) is internal — keep it internal. The user only needs to see: (a) what is being paid, (b) what they need to confirm, (c) the result.
>
> **Rule 2 carve-out — narrow, alternatives list only.** Inside Step A3.5, the literals `exact` / `aggr_deferred` / `charge` may be exposed to the user **only** in the expanded **alternatives list** (the list rendered after the user picks "show others"), because at that point the user is explicitly choosing between schemes. They MUST NOT appear in: the default recommendation card, the "N other methods" summary line, status narration, error displays, post-payment summaries, or anywhere else. The recommendation card shows network / token / amount / recipient only — never the scheme name.
>
> **Rule 3 — Externally-defined protocol literals stay byte-for-byte exact.** The JSON field `x402Version`, the HTTP headers `X-PAYMENT` / `PAYMENT-SIGNATURE` / `PAYMENT-REQUIRED` / `WWW-Authenticate: Payment`, and the reference URL `https://x402.org` MUST appear verbatim wherever the protocol/server requires them — these are externally defined and changing them breaks interop. CLI subcommand names (`onchainos payment pay` / `pay-local` / `charge` / `session ...` / `a2a-pay ...`) are this CLI's own surface and may evolve; refer to them by their current name in CLI invocations and code, but never speak them to the user (Rule 2).
>
> **Example**
>
> (EN) `Preparing a payment via the **OKX Agent Payments Protocol**. Here are the charge details — please confirm before I proceed…`
> When narrating in another language, translate this lead line but keep **OKX Agent Payments Protocol** as a bolded English noun phrase.

> **Progress narration counts as user-visible — Rules 1-3 still apply.**
>
> Long-running flows (decode → confirm → wallet check → sign → replay) tempt status updates. Every progress line ("I'm now…", or its Chinese equivalent) is user-facing; Step labels and reference/scheme names are internal — do NOT echo them. The anchors:
>
> | ❌ Don't say | ✅ Say |
> |---|---|
> | "Detected HTTP 402, triggering OKX Agent Payments Protocol" / "Detected `PAYMENT-REQUIRED`, loading `exact`" | _(silent — detection / routing is internal)_ |
> | "CLI selected `exact`, assembling the `PAYMENT-SIGNATURE` header" / "taking the TEE path" | "Signing done, replaying the request" |
> | "Detected 2 schemes: exact (USD₮0), aggr_deferred (USDG)" / "checking balance to filter candidates" | _(silent — enumeration + balance check are internal; only the recommendation card is user-visible)_ |
> | "Entering session / charge mode" | "Channel opened" — describe the user-visible effect, not the internal mode |
> | "Per past preference, paying without re-confirming" | _(forbidden — no such preference; the gate is mandatory every time)_ |
>
> The same rules apply when narrating in any other language — match the intent of these ❌/✅ phrasings, not just the English wording.
>
> **These rules are authoritative and always in force** — when unsure whether a status line leaks internals, match it against the rows above and default to silence.

## Triggers (full list)

- **EN**: `402`, payment required, `x402`, `x402Version`, `X-PAYMENT`, `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`, `WWW-Authenticate: Payment`, `permit2`, `upto`, metered billing, open / close / topup / settle channel, voucher, session payment, `channelId`, `channel_id`, `paymentId`, `a2a_`, create payment link, payment link, payment status
- subscribe / subscription / recurring payment / recurring charge / "pay every month" / cancel subscription / upgrade plan / downgrade plan → `period` scheme (see `references/subscription.md`)
- The same trigger vocabulary applies to its equivalents in any other language (e.g. Chinese subscription / recurring-billing terms route to the `period` scheme the same way).

Any close / topup / settle / voucher / refund near a `channel_id` or session context = MPP mid-session op → `references/session.md`.

## Pre-flight Checks

Read `../okx-agentic-wallet/_shared/preflight.md` (fallback: `_shared/preflight.md`).

## Command Routing & Reference map

Each 402 signal (or paymentId) → CLI command → reference. Detailed gating + decode/confirm steps are in Path A / Path B below.

| Signal | Command | Reference |
|---|---|---|
| 402 + `PAYMENT-REQUIRED` (v2) / body `x402Version` (v1) — one **or many** `accepts[]` schemes (`exact` / `exact`+Permit2 / `upto` / `aggr_deferred`) | **Primary — Path A:** `payment quote <url>` → confirm → `payment pay --payment-id --yes`. Single-scheme and multi-scheme take the **same** quote flow (the CLI decodes, converts, balance-checks, signs, **replays**, and returns the receipt). Even if you already curled the raw 402, re-enter via `payment quote <url>` — never assemble a header by hand and never jump straight to sign-only. **Compat only:** `payment pay --payload [--selected-index]` (sign-only + manual replay) when `quote` is unavailable. | Success path loads **no** reference. `references/accepts-schemes.md` only for: post-pay scheme-specific receipt reading, `Permit2 allowance insufficient` one-time approve, `pay-local`, the `pay --payload` compat path, or legacy x402 v1 (the CLI-output field tells you which scheme — `permit2Authorization` = `upto` / `exact`+Permit2, `sessionCert` = `aggr_deferred`, `authorization` = `exact`) |
| 402 offer with an `accepts[]` entry whose `scheme == "period"` (a.k.a. `permit2_subscription`) — recurring/subscription billing | `payment subscription subscribe/access/change/cancel/cancel-pending/my-subscriptions/allowance-status` | `references/subscription.md` |
| 402 + `WWW-Authenticate: Payment`, `intent="charge"` | `payment charge --challenge` | `references/charge.md` |
| 402 + `WWW-Authenticate: Payment`, `intent="session"` (or mid-session `channel_id`) | `payment session open/voucher/topup/close` | `references/session.md` |
| paymentId / `a2a_…` link / create-or-check payment link | `payment a2a-pay create/pay/status` | `references/a2a_charge.md` |
| A2MCP / 402 endpoint URL, "pay this endpoint", entry A/B payment node | `payment quote <url> [--param k=v ...] [--method GET \| POST \| ...]` | (inline — Path A) |
| User confirmed the quoted payment (currency/amount/scheme chosen) | `payment pay --payment-id <id> [--selected-index <n>] --yes` | (inline — Path A) |
| Need to decode a `PAYMENT-RESPONSE` header or a charge receipt | `payment decode-receipt (--header <b64> \| --receipt <json>)` | (inline — read-only) |

> **Don't load a reference on the success path.** On the primary Path A flow, `onchainos payment pay --payment-id --yes` signs, replays, and returns the settled receipt directly — skip `references/accepts-schemes.md` entirely (this holds for a single `accepts[]` scheme exactly as for multi-scheme). On the compat `pay --payload` path the CLI returns an `authorization_header` you replay yourself — same rule, no reference on success. Load `references/accepts-schemes.md` only on a **failure / legacy** path: `Permit2 allowance insufficient` → `references/accepts-schemes.md` (one-time approve), or a legacy x402 v1 raw proof → its "Legacy: x402 v1" section. `charge` / `session` / `a2a_charge` are always loaded — those are multi-phase flows.

> **Channel mid-session ops** (close / topup / settle / voucher / refund mentioned with an active `channel_id`, regardless of fresh 402) → stay here, jump straight into `references/session.md` at the matching phase. **Do NOT** search for a separate `close-channel` / `topup-channel` / `settle-channel` tool — they're all `onchainos payment session ...` subcommands.

---

# Path A: HTTP 402

## Path A (accepts-based): quote → confirm → pay — PREFERRED 2-round flow

**For an `accepts`-based 402 / A2MCP endpoint, the CLI does all mechanical work.
You do exactly two reasoning rounds.** (For `WWW-Authenticate: Payment` charge /
session challenges, skip this and use the protocol-detection steps below.)

### Step A1 — Extract params (round 1)
From the user prompt (Entry A) or the task payment node (Entry B), extract the
endpoint `url` and any known business params. Do NOT curl, decode, or convert
anything yourself.

### Step A2 — Quote
Run: `onchainos payment quote <url> [--param key=value ...] [--method GET|POST|...]`
The CLI probes the endpoint, parses the 402, checks your wallet balance, ranks
candidates, and writes a `paymentId`.

> **Probe method** — the CLI probes with `GET` by default. When the service
> declaration or the user's intent says the endpoint's initial call is **not GET**
> (e.g. the Bazaar `outputSchema.method` / business mind-map declares `"POST"`, or
> the user says "POST this endpoint"), pass `--method POST` (or the correct verb).
> Known business params then ride in the JSON **body** instead of the query string.
> Probing a POST-only A2MCP endpoint with the default GET can return 405 / a non-402
> response → `endpoint_unreachable` instead of the payment challenge. (The paid replay
> still uses `outputSchema.method` regardless — this flag only fixes the initial probe.)

Read `data`:
- `summary` — the human one-liner. `needsConfirm` is always true here.
- `candidates[]` (with `recommended:true`) and `alternatives[]` — the ranked schemes. Each carries `acceptsIndex` — its position in `accepts[]` (the ranked order differs from `accepts[]`, so never treat a candidate's list position as the index).
- `missingParams[]` + `merchantBody` — params the CLI could not fill; find the rest in `merchantBody`.
- `walletError` — if `login_required`, tell the user to log in, then re-quote.
- `recommended:null` on every candidate ⇒ no balance anywhere; present the list and ask.

### Step A3 — Confirm (round 2)  ⚠ MANDATORY — never skip
Use `AskUserQuestion` to confirm: currency/amount, the chosen scheme, and any
`missingParams`. Pass the chosen candidate's **`acceptsIndex`** as `--selected-index`
(NOT its position in `candidates[]`/`alternatives[]`) so the CLI signs exactly the
entry the user approved. **You MUST stop and confirm before paying — do not auto-pay.**

### Step A4 — Pay
Run: `onchainos payment pay --payment-id <id> --selected-index <n> --yes [--param key=value ...]`
`--yes` is required (the fund-moving confirming gate). `pay` signs the quoted payload,
replays, and returns the receipt — it never re-fetches the 402. Read `data.status`:
- `success` → report `txHash`; (Entry B) the task system marks the node paid.
- `failed` → surface `data.error`; offer retry.
- `pending` → poll / await terminal, then continue.

> To decode a returned `PAYMENT-RESPONSE` header or a charge receipt at any time,
> run `onchainos payment decode-receipt (--header <b64> | --receipt <json>)`.

---

## Step A1: Start from the original response (legacy / WWW-Authenticate detail)

> **⚠️ `accepts`-based 402 → go back to Path A `payment quote`.** The steps below are the **legacy manual path** (decode → assemble → replay yourself) plus the shared decode detail for `WWW-Authenticate: Payment` charge / session challenges. If the 402 you hold is **`accepts`-based** (`PAYMENT-REQUIRED` header v2 / `x402Version` body v1 — `exact` / `exact`+Permit2 / `upto` / `aggr_deferred`, whether a **single** scheme or many), do **not** continue here: discard your raw 402 and re-enter at **Path A** with `payment quote <url>`. The quote flow runs the same mandatory confirm gate and returns the same receipt schema for single- and multi-scheme alike — a single scheme is not a shortcut for skipping `quote`. Continue below **only** for the `WWW-Authenticate: Payment` charge / session detail, or when `payment quote` is genuinely unavailable and you must fall back to the explicit `pay --payload` sign-only compat path.

You already have the original HTTP response. If it is **not 402**, return the body directly. Otherwise → Step A2.

**Capture any request parameters the user's prompt supplies** (e.g. "weather in San Francisco" → `city=San Francisco`, `token=0x…`; "translate to Chinese" → `lang=zh`). Record each as `name → value` for the Step A3-Params plan — values given here are **never re-asked**, just shown in the confirmation card. Keep them even if the first request didn't need them; the seller may require them on the paid replay.

## Step A2: Detect the protocol

```
Priority 1: response.headers['WWW-Authenticate']
  starts with "Payment "        → continue at Step A3-WWW-Authenticate
Priority 2: response.headers['PAYMENT-REQUIRED']
  base64-encoded JSON           → continue at Step A3-Accepts (v2)
Priority 3: response body JSON has "x402Version"
                                → continue at Step A3-Accepts (v1)
Otherwise                       → not a supported payment protocol, stop
```

**Both indicators present** — branch on the WWW-Authenticate intent:

- `intent="session"` offered alongside `accepts`-based options → STOP and ask the user:
  > The server offers two payment styles via the **OKX Agent Payments Protocol**:
  > 1. **Session (multi-request)** — open a channel and issue vouchers per request
  > 2. **One-shot purchase**
  >
  > Which would you like to use?

  Option 1 → continue at Step A3-WWW-Authenticate (session path). Option 2 → drop the session intent and continue at Step A3-Accepts with the accepts options.

- `intent="charge"` offered alongside `accepts`-based options → all options are one-shot; **do not** show the session-vs-one-shot prompt. Decode both protocol families (Step A3-Accepts AND Step A3-WWW-Authenticate), merge the candidates, and let Step A3.5 handle the recommendation.

## Step A3-Accepts: Decode

Decode the 402 payload **yourself** for **display + recommendation only** — no CLI round-trip:

```
raw_402 = response.headers['PAYMENT-REQUIRED']   // v2 (base64-encoded JSON)
       or response.body                          // v1 (already plain JSON)

decoded = JSON.parse(atob(raw_402))              // v2; for v1 it's already JSON: JSON.parse(response.body)
```

Extract for display:

```
accepts = decoded.accepts
option  = decoded.accepts[0]       // for display only
```

**Keep `raw_402` verbatim** — Step A6 passes it straight to `onchainos payment pay --payload` (the CLI re-decodes and signs). The local decode is display-only; never re-encode or assemble anything.

## Step A3-WWW-Authenticate: Decode

Parse the WWW-Authenticate header:

```
Payment id="...", realm="...", method="evm", intent="...", request="<base64url>", expires="..."
```

base64url-decode `request` to get the JSON body. Save:

```
intent              charge | session
amount              base units string (e.g. "1000000")
currency            ERC-20 contract address
recipient           merchant payee address
methodDetails:
  chainId           EVM chain ID (e.g. 196 for X Layer)
  escrowContract    REQUIRED for session, ABSENT for charge
  feePayer          true (transaction mode) | false (hash mode)
  splits            optional, charge only, max 10 entries
  minVoucherDelta   optional, session only
  channelId         optional, session topUp/voucher only — pre-existing channel
suggestedDeposit    optional, session only — suggested initial deposit
unitType            optional — "request" | "second" | "byte" etc.
```

**Method check** — only `method="evm"` is supported here. If `method` is `"tempo"`, `"svm"`, `"stripe"`, etc. → stop and tell the user this dispatcher cannot handle it.

**Challenge expiry** — if `expires=...` (ISO-8601) is in the past, the challenge is dead: re-send the original request to get a fresh 402 before signing. Stale challenges fail with `30001 incorrect params`.

Convert `amount` from base units to human-readable (see `_shared/amount-display.md`).

## Step A3-Params: Build the request-parameter plan

> **Runs after Step A3 decode, before any confirmation card.** Beyond payment terms, the seller may declare which parameters the **paid replay** must carry and how. Build a **param plan** so the user confirms params alongside payment and the replay attaches them correctly.

A param plan is a list of `{ name, value, carrier, required, source }`, `carrier ∈ {query, body, header, path}`. No seller-declared params and none named by the user → **empty plan**; replay unchanged.

### Source 1 — Bazaar `outputSchema.input` (preferred)

If the decoded 402 (or any `accepts[i]`) carries `outputSchema.input`, parse it:

| Field | Use |
|---|---|
| `input.type` | `"http"` → handle here. `"mcp"` → out of scope, skip param assembly. |
| `input.method` | Method to replay with (may differ from the original). `GET`/`HEAD`/`DELETE` → params go in **query**; `POST`/`PUT`/`PATCH` → in **body** (`input.bodyType`: `json`/`form-data`/`text`). |
| `input.queryParams` / `input.body` / `input.pathParams` / `input.headers` | Params for that carrier (query / body / path / header). |

The JSON Schema `properties` + `required` give each param's type and whether it's mandatory. One plan entry per declared param.

### Source 2 — non-Bazaar (conservative)

No `outputSchema.input` → add a param **only** on an explicit seller signal; **never invent one**:

- response **body** lists requirements (`required` / `params` / `parameters` / `fields` / `inputSchema`), OR
- an **error message** names a missing param (e.g. `missing required query param "city"`), OR
- a documented response **header** asks for one.

Ambiguous → add nothing, replay unchanged.

### Fill values

Per entry, resolve `value`: (1) user's prompt (Step A1) → `source=prompt`, don't re-ask; (2) conversation context → `source=context`; (3) still missing **and required** → ask the user, one grouped question for all of them (a legitimate gate, not narration — ZERO-TEXT-ON-TRIGGER doesn't forbid it). Optional + unresolved → drop.

## Step A3.5: Multi-scheme recommendation (when applicable)

**Applies only when** the combined candidate pool contains **2 or more** of `{exact, aggr_deferred, charge, period}`. Otherwise skip straight to Step A4 with the single available candidate.

> When the 402 `accepts[]` contains 2 or more of `{exact, aggr_deferred, charge, period}`, load `references/multi-scheme.md`. Treat `period` as the recurring-billing option: recommend it only when the user intent is an ongoing subscription, not a single call.

When it applies → **load `references/multi-scheme.md`** and follow it end to end. It returns the **selected candidate** and tells you where to resume: Step A4 (user picked an alternative) or straight to Step A6 (user accepted with `yes` — A5's wallet check already satisfied).

## Step A4: Display payment details and STOP

**🟢 Skip this step entirely if** the user accepted the recommendation in A3.5.5 with `yes` (the card already showed network / token / amount / recipient). Go straight to Step A5 (a no-op if A3.5.2 already handled login) → A6.

**🔴 Run this step normally if** either:
- Step A3.5 did not run (single-candidate path), OR
- The user picked an alternative from A3.5's expanded list (the picked candidate still needs full-detail confirmation).

**⚠️ MANDATORY (when run): Display details and STOP to wait for explicit user confirmation. Do NOT call `onchainos wallet status` or any other tool until the user confirms.**

For **`accepts`-based 402** (`PAYMENT-REQUIRED` header v2 / `x402Version` body v1):

> This resource requires payment via the **OKX Agent Payments Protocol**:
> - **Network**: `<chain name>` (`<option.network>`)
> - **Token**: `<token symbol>` (`<option.asset>`)
> - **Amount**: `<human-readable amount>` (from `option.amount` for v2, or `option.maxAmountRequired` for v1; convert from minimal units using token decimals)
> - **Pay to**: `<option.payTo>`
> - **Request parameters** (omit this line entirely if the Step A3-Params plan is empty): one row per param as `<name> = <value>` → `<carrier: query | body | header | path>`
>
> Proceed with payment? (yes / no)

For **`WWW-Authenticate: Payment` 402**:

> This resource requires payment via the **OKX Agent Payments Protocol**:
> - **Payment type**: `<one-shot payment | session (multiple requests)>` (render as "one-shot payment" / "session (multiple requests)" — never "single purchase"; keep the same distinction when translating to another language)
> - **Network**: `<chain name>` (`eip155:<chainId>`)
> - **Token**: `<symbol>` (`<currency address>`)
> - **Amount per request**: `<human-readable>` (atomic: `<amount>`)
> - **Pay to**: `<recipient>`
> - **Who pays gas**: `<server (transaction mode) | you broadcast it yourself (hash mode)>`
> - **Split recipients** (one-shot only, if present): `<N other parties also receive a share>`
> - **Suggested prepaid balance** (session only, if present): `<human-readable>`
> - **Request parameters** (omit this line entirely if the Step A3-Params plan is empty): one row per param as `<name> = <value>` → `<carrier: query | body | header | path>`
>
> Proceed with payment? (yes / no)

- **User confirms** → Step A5.
- **User declines** → stop. No payment, no wallet check.

## Step A5: Check wallet status (only after the user explicitly confirms)

```bash
onchainos wallet status
```

- **Logged in** → Step A6.
- **Not logged in (`accepts`-based path)** → ask the user to choose between (1) wallet login (TEE signing) or (2) local private key (`onchainos payment pay-local`, supports `exact + EIP-3009`, `exact + Permit2`, and `upto` — `aggr_deferred` not supported, requires TEE session key). Don't read files or check env vars until the user picks.
- **Not logged in (`WWW-Authenticate: Payment` path)** → ask the user to log in via email OTP or AK. **TEE-only — no local-key fallback for this path** (only the `accepts`-based path has one).

## Step A6: Hand off to the scheme/intent reference

| Path | Action |
|---|---|
| **`accepts`-based** (`PAYMENT-REQUIRED` header v2 / `x402Version` body v1) | **Primary — Path A:** you should already be on the `payment quote <url>` → confirm → `payment pay --payment-id --yes` flow (top of Path A); it signs, replays, and returns the receipt — no hand-assembly, and no reference load on success. This is identical for a single `accepts[]` scheme and for multi-scheme.<br>**Compat / fallback only** (`quote` unavailable, or an explicit legacy request): run `onchainos payment pay --payload '<raw_402 from Step A3>'`. If Step A3.5 ran and the user picked an accepts-based candidate, add `--selected-index <index in decoded.accepts>` so the CLI signs exactly that entry; omit it for a single candidate (CLI auto-selects). The CLI decodes, signs from the selected account, and returns `{authorization_header, header_name, scheme, wallet}` — **no hand-assembly**; then go to Replay below.<br>If the user picked the local-key fallback, run `onchainos payment pay-local --payload '<raw_402>'` instead (same success rule; supports `exact + EIP-3009`, `exact + Permit2`, and `upto` — `aggr_deferred` is TEE-only).<br>**`Permit2 allowance insufficient` error** (`upto` / `exact`+permit2, first payment for that token) → load **`references/accepts-schemes.md`** for the one-time approve, then retry the pay.<br>**Legacy v1** — CLI returns a raw proof (`signature`+`authorization`, no `authorization_header`) → load **`references/accepts-schemes.md`** and follow its "Legacy: x402 v1" section to assemble the `X-PAYMENT` header. |

| `period` (subscription / `permit2_subscription`) | Load **`references/subscription.md`** at "Decide operation" (subscribe vs access vs change vs cancel). First-time offer → `payment subscription subscribe`; already-active resource → `payment subscription access` (never re-subscribe); upgrade/downgrade → `change`; teardown → `cancel` / `cancel-pending`. |
| **`WWW-Authenticate: Payment`, `intent="charge"`** | Load **`references/charge.md`** at "Decide mode". |
| **`WWW-Authenticate: Payment`, `intent="session"`** | Load **`references/session.md`** at "Phase S1: Open Channel" (or jump to S2 / S2b / S3 if the user is mid-session with an active `channel_id`). |

**Replay (success path — no reference needed):** resend the original request with the returned header (`<header_name>: <authorization_header>`, or the `X-PAYMENT` you assembled for legacy v1), expect `HTTP 200`, and decode any `PAYMENT-RESPONSE` header locally (`echo '<value>' | base64 -d | jq .`) to read `status` / `transaction` / `amount` / `payer`. Surface the settlement details to the user; suggest follow-ups conversationally — never expose internal field names or skill IDs.

---

# Path B: a2a-pay (paymentId-based, no 402)

The user invokes this path explicitly — by mentioning a `paymentId` / `a2a_...` link, asking to "create a payment link", or asking to check a2a payment status.

## Step B1: Identify the role

| User says… | Load | Role |
|---|---|---|
| "create payment link" / "generate payment" / `--amount`/`--recipient` | `references/a2a_charge.md` → "Seller — Create" | Seller |
| Provides a `paymentId` / `a2a_...` to pay | `references/a2a_charge.md` → "Buyer — Pay" | Buyer |
| Provides a `paymentId` and asks for status | `references/a2a_charge.md` → "Status — Query" | Either |

If the user says only "I want to pay" without a paymentId — STOP and ask the user to provide the seller-issued paymentId. Do not attempt anything else.

## Step B2: Wallet status

Both `create` and `pay` require a live wallet session. Run `onchainos wallet status`:

- **Logged in** → proceed (load the reference and follow it).
- **Not logged in** → ask the user to log in via `onchainos wallet login` or `onchainos wallet login <email>`. **Do NOT sign without a live session.**

## Step B3: Hand off to `references/a2a_charge.md`

The reference has the full create/pay/status flow (incl. auto-poll and the trust-delegation note). Buyer-side trust is delegated upstream — the buyer signs whatever the on-server challenge declares.

---

# Cross-cutting

## Reading seller errors (`WWW-Authenticate: Payment` / a2a-pay)

When the seller rejects, do NOT show raw JSON or just the numeric code. Extract the human-readable explanation in priority order, use the first non-empty match:

1. `body.reason` (mppx, OKX TS Session)
2. `body.detail` (RFC 9457 ProblemDetails)
3. `body.message`
4. `body.msg` (OKX SA API)
5. `body.error`
6. `body.title` (RFC 9457 short title — fallback only)
7. fallthrough — format the whole body and add the HTTP status

Format:

> ❌ Seller rejected: `<reason text>` (code `<code if present>`, HTTP `<status>`)

## Amount display

All user-facing amounts in BOTH human and atomic form: `<human> (<atomic>)`, e.g. `0.0004 USDC (400)`. Decimals table + unknown-symbol fallback → `_shared/amount-display.md`.

## Suggest next steps

After a successful payment + response, suggest conversationally:

| Just completed | Suggest |
|---|---|
| `payment quote` returned `needsConfirm:true` | `AskUserQuestion` to confirm, then `payment pay --payment-id <id> --selected-index <n> --yes` |
| `payment pay` returned `status:"success"` | Report `txHash`; if a `PAYMENT-RESPONSE` header is present, `payment decode-receipt --header <b64>` |
| `payment pay` returned `status:"pending"` | `payment a2a-pay status --payment-id <id> --wait` (a2a) or await the facilitator callback |
| Successful HTTP 402 replay | Check balance impact via `okx-agentic-wallet`; or make another request to the same resource |
| Successful a2a payment | Verify post-payment balance via `okx-agentic-wallet` |
| 402 on replay (expired) | Retry with a fresh signature |
| Channel session in progress | Issue another voucher when the next request arrives; close the channel when done |
