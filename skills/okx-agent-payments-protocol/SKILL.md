---
name: okx-agent-payments-protocol
description: "⚠️ READ FIRST — ZERO-TEXT-ON-TRIGGER + NEVER-SKIP-USER-GATES. Between detecting a 402 (or any trigger word below) and emitting either the recommendation card (Step A3.5) or the single-candidate confirmation card (Step A4), output ZERO user-visible text. Zero. No 'received 402', no 'triggered OKX Agent Payments Protocol', no 'detected N schemes', no enumeration of schemes / networks / tokens / amounts, no 'loading skill', no '正在处理 / 触发 / 检测到 / 收到'. The skill-load tool call may happen but emits no surrounding prose. Exactly one user-confirmation card runs per payment: A3.5's recommendation card (when 2+ candidates and user picks `yes`) OR A4's confirmation card (when 1 candidate, OR when the user picked an alternative from A3.5's expanded list). Do NOT skip the applicable card on your own initiative under the pretext of 'past user preference' / 'streamlining' / 'they already confirmed once' — those preferences do not exist. Equally, do NOT render both cards back-to-back with the same info — after `yes` on A3.5.5, go directly to Step A5. The next user-visible text after detection MUST be one of the two cards. Unified payment dispatcher covering x402 (`exact` / `exact+Permit2` / `upto` / `aggr_deferred` schemes), MPP (`charge` / `session` intents), and a2a-pay (`a2a_charge` paymentId flow). Detects HTTP 402 protocol from response headers and routes to the matching scheme/intent reference; also handles a2a paymentId mentions without a 402. Loads `references/exact.md` (x402 exact scheme — full EIP-3009 TEE or local-key fallback), `references/aggr_deferred.md` (x402 aggr_deferred scheme — Session Key Ed25519 with sessionCert), `references/upto.md` (x402 upto scheme — cap-style metered billing via Permit2 with facilitator-bound witness; also covers `exact + Permit2` sub-mode where buyer output uses `permit2Authorization`), `references/charge.md` (MPP one-shot charge in transaction or hash mode, with splits), `references/session.md` (MPP channel: open + voucher loop + topUp + close, with state echo), or `references/a2a_charge.md` (a2a-pay create / pay / auto-poll status). Returns a ready-to-paste authorization header (x402 / MPP) or a tx-hash + status (a2a). Trigger words (English): '402', 'payment required', 'mpp', 'machine payment', 'pay for access', 'payment-gated', 'WWW-Authenticate: Payment', 'x402', 'x402Version', 'PAYMENT-REQUIRED', 'PAYMENT-SIGNATURE', 'X-PAYMENT', 'permit2', 'permit2Authorization', 'upto', 'metered billing', 'pay by usage', 'cap-style payment', 'settlement-overrides', 'open channel', 'voucher', 'session payment', 'close channel', 'topup channel', 'top up channel', 'settle channel', 'settle session', 'refund channel', 'channelId', 'channel_id', 'paymentId', 'a2a_', 'a2a payment', 'create payment link', 'payment link', 'payment status'. Trigger words (Chinese): '按量计费', '支付上限', '支付通道', '关闭通道', '关闭会话', '关闭支付通道', '充值通道', '续费通道', '结算通道', '结算会话', '关单', '凭证', '会话支付', '付款链接', '创建支付', '支付状态'. Critical sensitivity rule: any user mention of close / topup / settle / voucher / refund near a `channel_id`, `0x...` channel hash, or 'session' / 'channel' context = MPP mid-session operation — load this skill, jump into `references/session.md`, do NOT search for a separate close/topup tool."
license: MIT
metadata:
  author: okx
  version: "3.4.2-beta"
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
> **Rule 2 carve-out — narrow, alternatives list only.** Inside Step A3.5, the literals `exact` / `aggr_deferred` / `charge` may be exposed to the user **only** in the expanded **alternatives list** (the list rendered after the user picks "show others"), because at that point the user is explicitly choosing between schemes. They MUST NOT appear in: the default recommendation card, the "N other methods" summary line, status narration, error displays, post-payment summaries, or anywhere else. The recommendation card shows network / token / amount / recipient only — never the scheme name.
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
> | "This is an HTTP 402 with two payment-protocol headers offering multiple schemes" / "Both indicators present, entering Step A3.5" | _(silent — protocol detection is internal)_ |
> | "收到 HTTP 402,触发 OKX Agent Payments Protocol" / "Received 402, triggering OKX Agent Payments Protocol" | _(silent — skill-load announcement is internal)_ |
> | "检测到两个 scheme:exact (USD₮0) 和 aggr_deferred (USDG),网络 eip155:196" / "Detected 2 schemes on chain 196" | _(silent — scheme + network + token enumeration is internal; only the recommendation card may name them, and only per Rule 2's carve-out scope)_ |
> | "按之前的偏好,直接走支付不再确认" / "Per past preference, skipping confirmation" | _(forbidden — there is no such preference; the recommendation + confirmation gates are mandatory every time)_ |
> | "I have three candidates (exact, aggr_deferred, charge). Per Rule 2 carve-out…" / "候选池里有 3 个 scheme" | _(silent — candidate enumeration is internal; only the final recommendation card is user-visible)_ |
> | "Let me check wallet status / balance first" / "正在查询钱包余额以筛选候选" | _(silent — the balance fetch is an internal precondition for the recommendation)_ |
> | "Wallet logged in (Account 1). Visible balances: 10 USD₮0, 10 USDG. Token addresses don't match — let me verify chain mapping" | _(silent — balance readout, address normalization, and chain-mapping checks are internal)_ |
> | "After balance filtering, 2 candidates remain; applying tie-breakers" / "过滤后剩 2 个,跑 tie-breaker" | _(silent — only emit the recommendation card)_ |

> Read `../okx-agentic-wallet/_shared/preflight.md` before any `onchainos` command. EVM only — CAIP-2 `eip155:<chainId>` (run `onchainos wallet chains` for the list).

## Reference map

| Triggered by | Load |
|---|---|
| 402 with `PAYMENT-REQUIRED` header (v2) or `x402Version` body field (v1), CLI output carries `permit2Authorization` field (covers `exact + Permit2` and `upto` schemes) | `references/upto.md` |
| 402 with `PAYMENT-REQUIRED` header (v2) or `x402Version` body field (v1), CLI output carries `sessionCert` field (and no `permit2Authorization`) | `references/aggr_deferred.md` |
| 402 with `PAYMENT-REQUIRED` header (v2) or `x402Version` body field (v1), CLI output carries `authorization` field (no `sessionCert`, no `permit2Authorization`) | `references/exact.md` |
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

## Step A3.5: Multi-scheme recommendation (when applicable)

**Applies only when** the combined candidate pool contains **2 or more** of `{exact, aggr_deferred, charge}`. Otherwise skip straight to Step A4 with the single available candidate.

> **🔇 Silence rule for A3.5 internals.** Substeps A3.5.1–A3.5.4 (candidate enumeration, wallet-status check, balance fetch, address/chain-mapping normalization, balance filtering, tie-breaker application) are **internal** — produce **no user-facing narration** during them. The only A3.5 output the user sees is (a) the login prompt in A3.5.2 *if* the wallet isn't logged in, and (b) the recommendation card / alternatives list in A3.5.5. Do **not** announce "I'm checking your balance", "Let me verify the chain mapping", "After filtering, X candidates remain", "Per Rule 2 carve-out…", or any other progress chatter between Step A3 finishing and the recommendation card appearing. Just go silent and emit the card.
>
> **🚫 Exactly one user gate per payment, mandatory.** Per payment, the user sees exactly one confirmation surface: A3.5's recommendation card (when 2+ candidates and the user accepts with `yes`), OR A4's per-payment confirmation card (when there's only 1 candidate, OR when the user picked an alternative from A3.5's expanded list). Do not skip the applicable gate on your own initiative — no "past preference", "streamlining", or "they confirmed once before" shortcuts; those preferences do not exist. Equally, do not duplicate gates: after a `yes` on A3.5.5, do NOT also render A4 with the same info.

### A3.5.1: Build the candidate pool

- Each entry in `accepts[]` → one candidate. Scheme = `accepts[i].scheme` (`exact` or `aggr_deferred`).
- A `WWW-Authenticate: Payment` 402 with `intent="charge"` → one candidate. Scheme = `charge`.
- `WWW-Authenticate: Payment` with `intent="session"` is **never** part of this pool — it's handled by the session-vs-one-shot branch in Step A2.

Each candidate carries `{scheme, chainId, tokenAddress, tokenSymbol, amount (atomic), amountHuman, isMainnet}`. Determine `isMainnet` from the chain registry (`onchainos wallet chains` lists chain metadata).

### A3.5.2: Get wallet balance

- If a recent wallet-balance snapshot already exists in conversation context (from an earlier `onchainos wallet balance` call this session), **reuse it** — do not re-query.
- Otherwise, check login first via `onchainos wallet status`:
  - **Not logged in** → ask the user to log in (the recommendation depends on knowing their balance). Don't fall back silently.
  - **Logged in** → query balance:

    ```bash
    onchainos wallet balance
    ```

### A3.5.3: Filter by has-balance

Keep only candidates where the wallet has a non-zero balance for the matching `(chainId, tokenAddress)`.

**Edge case — zero candidates pass the filter**: list **all original candidates** to the user (no recommendation badge, no tie-breakers applied). User picks one; carry it to Step A4.

### A3.5.4: Tie-breakers (apply in order; stop when one wins)

If more than one candidate remains after A3.5.3:

1. **Smallest required payment amount — same-symbol only.** Group remaining candidates by `tokenSymbol`. If they all share a single symbol, the one with the smallest `amountHuman` wins. If the remaining set spans multiple symbols, skip this rule.
2. **Mainnet over testnet.** Drop testnet candidates if any mainnet candidate remains. Different mainnets are equal — no preference between e.g. Ethereum, Base, X Layer.
3. **Scheme priority:** `aggr_deferred` > `exact` > `charge`.

The survivor is the **recommended candidate**. The rest are **alternatives**.

### A3.5.5: Display the recommendation

**Carve-out scoping** — the recommendation card itself does **NOT** contain a `Scheme:` line, and the "N other methods" summary line does **NOT** preview their schemes / amounts / tokens. Scheme literals appear **only** inside the expanded alternatives list, and only when the user explicitly asks for it. Render the card with `N = number_of_alternatives`:

> We recommend paying via the **OKX Agent Payments Protocol**:
>
> - **Network**: `<chain name>` (`eip155:<chainId>`)
> - **Token**: `<symbol>` (`<token address>`)
> - **Amount**: `<human> (<atomic>)`
> - **Pay to**: `<recipient>`
>
> `<N == 0 ? "No other methods available." : "There are <N> other supported method(s) you could use instead.">` Use the recommended method? (yes / show others)

**⚠️ Do NOT inline alternatives in the summary line.** Forbidden: ❌ "There are 2 other methods (exact 0.001 USD₮0, charge 0.0005 USD₮0)". Required: ✅ "There are 2 other supported methods you could use instead." Detail only appears after the user picks "show others".

- **yes** (or `N == 0`) → the recommended candidate becomes the **selected candidate**; continue at Step A4.
- **show others** → only now expand the alternatives list, each row as `<index>. scheme=<exact | aggr_deferred | charge>, network=<…>, token=<…>, amount=<…>`. User picks one by index → that becomes the selected candidate; continue at Step A4.

### A3.5.6: Carry the selection forward

- **`accepts`-based selection** (`exact` or `aggr_deferred` from `accepts[]`) → in Step A6, pass a single-entry accepts array (`'[selected_accept]'`) to `onchainos payment pay` so the CLI cannot deviate from the user's choice.
- **`charge` selection** (from WWW-Authenticate) → in Step A6, take the WWW-Authenticate / `references/charge.md` path; ignore the accepts-based candidates entirely.

Step A4 below now describes the **selected candidate**. Step A5's wallet-status check is already satisfied if A3.5.2 ran the login flow — skip the re-check; just continue to A6.

## Step A4: Display payment details and STOP

**🟢 Skip this step entirely if** the user accepted the recommendation in A3.5.5 with `yes`. The recommendation card already showed network / token / amount / recipient at the same fidelity A4 would — re-rendering them is pure redundancy. Go straight to Step A5 (a no-op if A3.5.2 already handled login) → A6.

**🔴 Run this step normally if** either:
- Step A3.5 did not run at all (single-candidate path — server only offered one scheme), OR
- The user picked an alternative from A3.5's expanded list. The alternatives list is one-line-per-row overview, so the picked candidate still needs full-detail confirmation here.

**⚠️ MANDATORY (when run): Display details and STOP to wait for explicit user confirmation. Do NOT call `onchainos wallet status` or any other tool until the user confirms.**

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
> - **Payment type**: `<one-shot payment | session (multiple requests)>` (render in Chinese as `单次支付` / `会话支付(多请求)` — NEVER `单次购买`)
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
| **`accepts`-based** (`PAYMENT-REQUIRED` header v2 / `x402Version` body v1) | Run `onchainos payment pay --accepts '<JSON.stringify(accepts_to_pass)>'`. Build `accepts_to_pass` from Step A3.5's outcome: if A3.5 ran and the user selected an accepts-based candidate, pass a **single-entry array** containing just that accept (`'[selected_accept]'`); otherwise pass the full `decoded.accepts` array. When the response comes back, branch by which field is present in the CLI output (check in this order — `upto` carries both `permit2Authorization` and `sessionCert`):<br>• `permit2Authorization` present → load **`references/upto.md`** for header assembly + replay (covers both `exact + Permit2` and `upto` scheme)<br>• `sessionCert` present (and no `permit2Authorization`) → load **`references/aggr_deferred.md`** for header assembly + replay (Ed25519 session-key path)<br>• otherwise (`authorization` present) → load **`references/exact.md`** for header assembly + replay (EIP-3009 path)<br>If the user picked the local-key fallback, run `onchainos payment pay-local` instead and load **`references/exact.md`** (only scheme this fallback supports). |
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
