---
name: okx-agent-payments-protocol
description: "Unified payment dispatcher covering x402 (`exact` / `aggr_deferred` schemes), MPP (`charge` / `session` intents), and a2a-pay (`a2a_charge` paymentId flow). Detects HTTP 402 protocol from response headers and routes to the matching scheme/intent reference; also handles a2a paymentId mentions without a 402. Loads `references/exact.md` (x402 exact scheme — full EIP-3009 TEE or local-key fallback), `references/aggr_deferred.md` (x402 aggr_deferred scheme — Session Key Ed25519 with sessionCert), `references/charge.md` (MPP one-shot charge in transaction or hash mode, with splits), `references/session.md` (MPP channel: open + voucher loop + topUp + close, with state echo), or `references/a2a_charge.md` (a2a-pay create / pay / auto-poll status). Returns a ready-to-paste authorization header (x402 / MPP) or a tx-hash + status (a2a). Trigger words (English): '402', 'payment required', 'mpp', 'machine payment', 'pay for access', 'payment-gated', 'WWW-Authenticate: Payment', 'x402', 'x402Version', 'PAYMENT-REQUIRED', 'PAYMENT-SIGNATURE', 'X-PAYMENT', 'open channel', 'voucher', 'session payment', 'close channel', 'topup channel', 'top up channel', 'settle channel', 'settle session', 'refund channel', 'channelId', 'channel_id', 'paymentId', 'a2a_', 'a2a payment', 'create payment link', 'payment link', 'payment status'. Trigger words (Chinese): '支付通道', '关闭通道', '关闭会话', '关闭支付通道', '充值通道', '续费通道', '结算通道', '结算会话', '关单', '凭证', '会话支付', '付款链接', '创建支付', '支付状态'. Critical sensitivity rule: any user mention of close / topup / settle / voucher / refund near a `channel_id`, `0x...` channel hash, or 'session' / 'channel' context = MPP mid-session operation — load this skill, jump into `references/session.md`, do NOT search for a separate close/topup tool."
license: MIT
metadata:
  author: okx
  version: "2.0.0"
  homepage: "https://web3.okx.com"
---

# OKX Agent Payments Protocol (Dispatcher)

Unified entry point for three payment paths, distinguished by HTTP signature: **`accepts`-based 402** (challenge in body for v1 or `PAYMENT-REQUIRED` header for v2), **`WWW-Authenticate: Payment` 402** (channel-capable, with `intent="charge"` or `"session"`), and **a2a-pay** (paymentId-based agent-to-agent links, no 402 required). This file owns the shared steps — protocol detection, payload decode, user confirmation gate, wallet status check — then dispatches into the right scheme/intent reference.

> **User-facing terminology — IMPORTANT**
>
> **Rule 1 — Always call it "OKX Agent Payments Protocol", and always render it bolded.** Use the exact English term **OKX Agent Payments Protocol** in user-visible messages regardless of the user's language, and always wrap it in markdown bold (`**OKX Agent Payments Protocol**`) so the user sees it emphasized. Keep it as a fixed English noun phrase even inside otherwise-Chinese sentences. Reserve protocol literals and internal identifiers for CLI invocations, HTTP headers, JSON payloads, and code — never speak them to the user.
>
> **Rule 2 — Do not narrate internal protocol detection.** The dispatch logic (which header was detected, which reference is being loaded, which scheme/intent was selected, TEE vs local-key path) is internal — keep it internal. The user only needs to see: (a) what is being paid, (b) what they need to confirm, (c) the result.
>
> **Rule 3 — Externally-defined protocol literals stay byte-for-byte exact.** The JSON field `x402Version`, the HTTP headers `X-PAYMENT` / `PAYMENT-SIGNATURE` / `PAYMENT-REQUIRED` / `WWW-Authenticate: Payment`, and the reference URL `https://x402.org` MUST appear verbatim wherever the protocol/server requires them — these are externally defined and changing them breaks interop. CLI subcommand names (`onchainos payment pay` / `pay-local` / `charge` / `session ...` / `a2a-pay ...`) are this CLI's own surface and may evolve; refer to them by their current name in CLI invocations and code, but never speak them to the user (Rule 2).
>
> **Example**
>
> (中) `准备通过 **OKX Agent Payments Protocol** 完成本次支付，下面是扣款明细，请确认……`
> (EN) `Preparing a payment via the **OKX Agent Payments Protocol**. Here are the charge details — please confirm before I proceed…`

> **Progress narration counts as user-visible — Rules 1-3 still apply.**
>
> Long-running flows (decode → confirm → wallet check → sign → header assembly → replay) tempt status updates. Every `"正在…"` / `"I'm now…"` line is user-facing. Step labels in this SKILL.md (`Step A3-Accepts`, `Step A3-WWW-Authenticate`) and reference files (`exact` / `aggr_deferred` schemes, `charge` / `session` intents) are internal — do NOT echo them in narration.
>
> | ❌ Don't say | ✅ Say |
> |---|---|
> | "正在处理 `accepts`-based 流程" / "Processing the `accepts`-based path" | "正在通过 **OKX Agent Payments Protocol** 处理本次支付" / "Processing the payment via the **OKX Agent Payments Protocol**" |
> | "CLI 自动选择 `exact` 方案" / "CLI selected the `exact` scheme" / "走 `aggr_deferred` 路径" | "签名完成" / "Signing done" |
> | "组装 `PAYMENT-SIGNATURE` / `X-PAYMENT` 头" / "Assembling the `PAYMENT-SIGNATURE` header" | "正在重放请求" / "Replaying the request" |
> | "检测到 `WWW-Authenticate: Payment` / `PAYMENT-REQUIRED` 协议" / "Detected the channel-based protocol" | _(silent — go straight to the confirmation prompt)_ |
> | "加载 `references/exact.md`" / "Loading the `exact` playbook" | _(silent — internal routing)_ |
> | "进入 `session` 模式 / `charge` 模式" / "Entering `session` intent" | "支付通道已开" / "Channel opened" — describe the user-visible effect, not the internal mode |
> | "TEE 路径 / 本地 key 路径" / "Using TEE signing path" | _(silent — signing path is internal)_ |

> Read `../okx-agentic-wallet/_shared/preflight.md` before any `onchainos` command. EVM only — CAIP-2 `eip155:<chainId>` (run `onchainos wallet chains` for the list).

## Reference map

| Triggered by | Load |
|---|---|
| 402 with `PAYMENT-REQUIRED` header (v2) or `x402Version` body field (v1), CLI returns no `sessionCert` | `references/exact.md` |
| 402 with `PAYMENT-REQUIRED` header (v2) or `x402Version` body field (v1), CLI returns `sessionCert` | `references/aggr_deferred.md` |
| 402 with `WWW-Authenticate: Payment`, `intent="charge"` | `references/charge.md` |
| 402 with `WWW-Authenticate: Payment`, `intent="session"` (also: any mid-session op on a `channel_id`) | `references/session.md` |
| User mentions a paymentId / `a2a_...` link / "create payment link" | `references/a2a_charge.md` |

## Skill Routing

| Intent | Use skill |
|---|---|
| Token prices / charts / wallet PnL / tracker activities | `okx-dex-market` |
| Token search / metadata / holders / cluster analysis | `okx-dex-token` |
| Smart money / whale / KOL signals | `okx-dex-signal` |
| Meme / pump.fun token scanning | `okx-dex-trenches` |
| Token swaps / trades / buy / sell | `okx-dex-swap` |
| Authenticated wallet (balance / send / tx history) | `okx-agentic-wallet` |
| Public address holdings | `okx-wallet-portfolio` |
| Tx broadcasting (`feePayer=false` hash mode) | `okx-onchain-gateway` |
| Security scanning (token / DApp / tx / signature) | `okx-security` |

**Channel mid-session ops** (close / topup / settle / voucher / refund mentioned with an active `channel_id`, regardless of fresh 402) → stay here, jump straight into `references/session.md` at the matching phase. **Do NOT** search for a separate `close-channel` / `topup-channel` / `settle-channel` tool — they're all `onchainos payment session ...` subcommands.

---

# Path A: HTTP 402

## Step A1: Send the original request

Make the HTTP request the user asked for. If status is **not 402**, return the body directly — no payment, no wallet check, no other tool calls.

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

**Both `WWW-Authenticate: Payment` and `PAYMENT-REQUIRED`/`x402Version` indicators present** — STOP and ask the user:

> The server offers two payment options via the **OKX Agent Payments Protocol**:
> 1. **One-shot purchase, or streaming session (multi-request)** (recommended)
> 2. **One-shot purchase**
>
> Which would you like to use?

Internal mapping: option 1 → `WWW-Authenticate: Payment` path, option 2 → `PAYMENT-REQUIRED`/`x402Version` path.

## Step A3-Accepts: Decode

**v2** — payload is in the `PAYMENT-REQUIRED` response **header** (base64-encoded JSON):

```
headerValue = response.headers['PAYMENT-REQUIRED']
decoded     = JSON.parse(atob(headerValue))
```

**v1** — payload is in the response **body** (direct JSON, not base64):

```
decoded = JSON.parse(response.body)
```

Extract:

```
accepts = decoded.accepts          // pass full array to the CLI later
option  = decoded.accepts[0]       // for display only
```

Save `decoded` for header assembly later — you will need `decoded.x402Version` and `decoded.resource` (v2).

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

Convert `amount` from base units to human-readable using the token's decimals (typically 6 for USDC/USD₮, 18 for native).

## Step A4: Display payment details and STOP

**⚠️ MANDATORY: Display details and STOP to wait for explicit user confirmation. Do NOT call `onchainos wallet status` or any other tool until the user confirms.**

For **`accepts`-based 402** (`PAYMENT-REQUIRED` header v2 / `x402Version` body v1):

> This resource requires payment via the **OKX Agent Payments Protocol**:
> - **Network**: `<chain name>` (`<option.network>`)
> - **Token**: `<token symbol>` (`<option.asset>`)
> - **Amount**: `<human-readable amount>` (from `option.amount` for v2, or `option.maxAmountRequired` for v1; convert from minimal units using token decimals)
> - **Pay to**: `<option.payTo>`
>
> Proceed with payment? (yes / no)

For **`WWW-Authenticate: Payment` 402**:

> This resource requires payment via the **OKX Agent Payments Protocol**:
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

- **User confirms** → Step A5.
- **User declines** → stop. No payment, no wallet check.

## Step A5: Check wallet status (only after the user explicitly confirms)

```bash
onchainos wallet status
```

- **Logged in** → Step A6.
- **Not logged in (`accepts`-based path)** → ask the user to choose between (1) wallet login (TEE signing) or (2) local private key (`onchainos payment pay-local`, `exact` scheme only). Don't read files or check env vars until the user picks.
- **Not logged in (`WWW-Authenticate: Payment` path)** → ask the user to log in via email OTP or AK. **TEE-only — no local-key fallback for this path** (only the `accepts`-based path has one).

## Step A6: Hand off to the scheme/intent reference

| Path | Action |
|---|---|
| **`accepts`-based** (`PAYMENT-REQUIRED` header v2 / `x402Version` body v1) | Run `onchainos payment pay --accepts '<JSON.stringify(decoded.accepts)>'`. When the response comes back, look at whether `sessionCert` is present:<br>• `sessionCert` present → load **`references/aggr_deferred.md`** for header assembly + replay<br>• `sessionCert` absent → load **`references/exact.md`** for header assembly + replay<br>If the user picked the local-key fallback, run `onchainos payment pay-local` instead and load **`references/exact.md`** (only scheme this fallback supports). |
| **`WWW-Authenticate: Payment`, `intent="charge"`** | Load **`references/charge.md`** at "Decide mode". |
| **`WWW-Authenticate: Payment`, `intent="session"`** | Load **`references/session.md`** at "Phase S1: Open Channel" (or jump to S2 / S2b / S3 if the user is mid-session with an active `channel_id`). |

After the reference returns the assembled `X-PAYMENT` / `PAYMENT-SIGNATURE` header or `authorization_header`, replay the original request and surface the response to the user. Suggest follow-ups conversationally — never expose internal field names or skill IDs.

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

The reference contains the full create/pay/status flow including the auto-poll-to-terminal logic and trust-delegation note. Buyer-side trust is delegated to the upstream caller — the buyer signs whatever the on-server challenge declares. Cross-checking the paymentId against the agreed terms is the upstream's responsibility, NOT this dispatcher's.

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

All user-facing amounts in BOTH human and atomic form: `<human> (<atomic>)`, e.g. `0.0004 USDC (400)`, `1.5 ETH (1500000000000000000)`. Compute via `amount / 10^decimals` from the challenge `currency` token.

| Token | Decimals | 1 unit in minimal | Example |
|---|---|---|---|
| USDC | 6 | `1000000` | `1000000` → 1.00 USDC |
| USDT | 6 | `1000000` | `2500000` → 2.50 USDT |
| USDG | 6 | `1000000` | `500000`  → 0.50 USDG |
| ETH | 18 | `1000000000000000000` | `10000000000000000` → 0.01 ETH |

For any symbol not in the table: never assume — query `okx-dex-token` for the token's decimals first. If you cannot resolve them, render `<minimal> <symbol>` and append `unknown decimals — please double-check the seller-provided amount`. Do not block the flow.

## Suggest next steps

After a successful payment + response, suggest conversationally:

| Just completed | Suggest |
|---|---|
| Successful HTTP 402 replay | Check balance impact via `okx-agentic-wallet`; or make another request to the same resource |
| Successful a2a payment | Verify post-payment balance via `okx-agentic-wallet` |
| 402 on replay (expired) | Retry with a fresh signature |
| Channel session in progress | Issue another voucher when the next request arrives; close the channel when done |
