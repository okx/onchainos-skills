# Invariants — rendering rules, id ladder, fields, commands

Load this file when: rendering a card / diff / detail view, resolving `#<id>`, translating CLI labels, or handling `--service` fields.

---

## Lexicon (prose / Q&A / post-success rows when CLI label is absent)

- **Roles:** `user` → **User** / 用户 · `asp` → **ASP** / 服务提供商 · `evaluator` → **Evaluator** / 评审员 — each rendered as its localized label in the conversation language. Never show the raw enum token, never legacy nouns (buyer/seller/arbitrator/仲裁者/仲裁员 or their localized equivalents), never a bilingual parenthetical. Legacy "arbitrator"-family words (in any language) are input aliases — recognize them on input, but always render the localized **Evaluator** label on output (`评审员` in Chinese, `Evaluator` in English).
- **Service type:** A2MCP → **API service** · A2A → **agent to agent**. Gloss once per table: "API service = pay-per-call, fixed price; agent to agent = per-call or monthly-subscription pricing (one or the other)." Never raw A2MCP/A2A.
- **Stars:** render `★ <value>` from CLI's `ratingStars` / `feedbackRate` / `average` **directly** — never divide by 20, never show raw 0–100. Null/0 context-split: **search** rows → `null`=`—`, `0`=`No rating yet`; **list / detail / feedback** → no rating = `No rating yet` (never `—`).
- **Fee:** stored/sent as a plain numeric string (`"10"`); **displayed** as `N USDT` (USDT is implicit — the renderer appends it). Both API service (A2MCP) and agent to agent (A2A) support a `0` fee → an explicit `0` displays as `0 USDT` (a free service). An empty single-purchase `fee` (`""`) means "no per-call price" (a subscription-priced A2A service) → display the Fee row as `—`, with the price in the Subscription row. A2MCP with no fee → `—` (missing required fee — not `free`, since A2MCP requires a fee at create/update). An A2A service with **neither** a fee nor a subscription set (a legacy/backend-anomalous state — current registration always requires exactly one) → `free`.
- **Subscription (A2A only):** the `subscription[]` array carries monthly pricing tiers `{interval:"month", fee:"N"}`. **Displayed** as `N USDT / month` per tier; an empty `[]` displays as `—` (no subscription). A2MCP never has one. Fee and Subscription are **mutually exclusive** on A2A — a service shows **exactly one** of them as a real price and the other as `—` (never both real, never both `—`).
- **Free trial (A2A subscription only):** `freeTrial` is a duration in **hours**; the skill only ever sets the fixed 3-day value `"72"`. **Displayed** as its duration — `3 days` (whole days collapse to a day count; otherwise `<N> hours`) — in the Free trial column/row; absent, single-fee A2A, or A2MCP → `—`. **Address:** lowercase `0x…1234`. **Reviewer** slot = "reviewer", never "creator".

## Legacy role words — rename prompt (Evaluator)

The Evaluator role was previously surfaced with legacy words. When the user's **own input** names it with any legacy role word — **仲裁者 / 仲裁员 / 评估者 / arbiter / arbitrator / assessor**, or the equivalent pre-rename terms in the user's language — recognize it as the **Evaluator** role and, **once per session**, emit the rename prompt (matched to the conversation language) before proceeding, then carry out the request directly without asking for confirmation:

- Chinese: 你说的角色现在叫「评审员」，已按此为你处理。
- English: That role is now called Evaluator — proceeding.
- Other languages: translate the English line, naming the localized new role label (never echoing the legacy word).

Rules:
- Fire **once per session** — do not repeat the prompt on later turns.
- After prompting, **execute directly** — do not wait for the user to re-confirm the rename.
- **Never restate the old role word** in your own output afterward; use the localized **Evaluator** label from then on (`评审员` in Chinese, `Evaluator` in English).
<!-- intent: the trigger list intentionally keeps the legacy role words; they are input aliases and must be recognized. This rule normalizes presentation to the new term without dropping recognition of old input. -->

## Card skeleton (every confirmation / diff / detail card uses THIS)

Two-column pipe table `| Field | Value |`, one row per field. Role row uses localized label (never enum); photo row = uploaded CDN URL or `default` (ASPs require a URL; `default` only for user/evaluator — see register §5) — never a user-pasted link (rejected).

- **Confirmation variant** (create only): ends with `> Reply **1** to confirm and run.` (localized). No bash shown.
  - **ASP exception:** ASP create renders TWO cards in sequence (register.md §7) — the first (Identity) card ends with `> Reply **1** to continue.` instead (advances to Step 2, no CLI call yet); only the second (Service) card is the FINAL card and carries the confirm-and-run footer above.
- **Diff variant** (update only): 3 columns `| Field | Current | New |`; unchanged fields → `(unchanged)`; changed New cell **bold**. Show real before→after values.

## Verbatim-render contract (P0-4)

When CLI returns `card[]` / `cells[]` plus `roleLabel` / `statusLabel` / `approvalLabel` / `ratingStars`, render numeric/star fields **verbatim** — do not hand-map integers, do not divide score/20, never show raw 0–100. **Verbatim applies to numbers/stars/ids/addresses only — NOT to language.** Every string `*Label` field and all surrounding prose/labels are English-canonical and MUST be translated into the SKILL §Language-Lock language before rendering. Fallback: hand-map via Lexicon if `*Label` absent (legacy response).

## CLI output fields — translate before rendering

- `roleLabel` / `statusLabel` / `approvalLabel`
- Service type values: "API service" / "agent to agent"
- Placeholder strings: "(not set)" / "default" / "No rating yet" / "(no comment)" / "free"
- `findings[].message` — the unified per-rule user-facing message text (English-canonical), and the ONLY rule-message field in the output. Translate it into the SKILL §Language-Lock language, then render **as-is** on the offending field row. Each finding also carries `code` (fine-grained diagnostic, e.g. `U1`/`N1`) for grouping/diagnostics only, **never shown to the user** (the canonical rule id is deliberately NOT serialized). Several sub-checks on the SAME field can carry the SAME `message`: **de-duplicate by (`field`, `message`) and render that sentence ONCE per field** (do not repeat the identical sentence per sub-check). (Legacy responses may still carry `issue`/`fix` instead of `message` — if so, translate and render `<issue> → <fix>` as before.)

> **NEVER expose rule identifiers to the user (P0).** The `FE-01`…`FE-23` rule numbers and the internal `code` tokens (`U1`/`N1`/`S1`/`T1`/`P1`/`D1`…) used throughout these skill docs are **authoring / internal references only**. Never print, echo, translate, or otherwise surface an `FE-xx` id or a diagnostic `code` in anything shown to the user — the user only ever sees the plain-language `message` (translated) and your drafted correction. When a rule below says "show its message text", that means the plain-language message, NOT the rule number.

## #id ladder (P0-3) — resolving `#<id>` after create

1. top-level **`newAgentId`** when its value is a **non-empty string** (PRIMARY — WS push succeeded)
2. else `agent.agentId` from the WS push object
3. `newAgentId` is `null` (WS push timed out) — omit `#<id>` substring, use fallback wording.

Never invent or borrow a pre-check id; never emit a bare `# `.
**Non-create intents** (activate/deactivate/update/detail): no `newAgentId` — use the `#N` the user typed or the CLI's direct id.

## Fields-from-user (output-safety invariant)

`name` / `description` / `picture` / `service.*` come from the user's **literal reply this turn** — never pre-filled from userEmail, wallet name, or session metadata. Carve-out: you MAY reformat the user's OWN words into the **service description**, one part per line (numbering required for A2MCP, optional for A2A) — A2MCP: the request-description four parts (`1.` service description · `2.` parameter spec · `3.` request method · `4.` CURL request example — see §A2MCP `serviceDescription` structure); A2A (any pricing model): `1.` core-capability summary (required) · `2.` what the user must provide (optional) · `3.` delivery note (optional) (illustrate, never invent a capability or metric — and never synthesize an absent part `2.`/`3.` out of nothing).

**Name must be a brand, not a person (semantic QA — register §4):** flag any agent name that **contains** a celebrity / public-figure name as a substring, even when prefixed or suffixed (e.g. Trump, Musk, CZ — in any language or script). This is a skill semantic check, not a CLI mechanical rule, so it follows the uniform skill-rule handling: show its message text (never a rule number), draft a neutral brand-name alternative marked ` ✏️ drafted from your words — please review`, and have the user confirm a compliant name — never carry a celebrity / personal name onto the §7 confirmation card.

**Confirmation requirement for any reformat/draft (non-overridable):** reformatting or drafting is a *draft*, never an authorization to commit silently. Whenever you reshape the user's words into the multi-line description, you MUST (1) flag every affected row on the confirmation card / diff card with an explicit marker — e.g. ` ✏️ drafted from your words — please review` — so the user can tell Claude-rewritten content from their own verbatim input, and (2) wait for the normal card confirm (Reply **1**) before the write. Never let reformatted/drafted content reach the chain presented as the user's literal input. If the user flags any drafted row as wrong, re-collect that field from their own words and redraw — do not argue or keep your draft.

## Commands (11 `onchainos agent` subcommands — you invoke them, never show them)

`create · pre-check · update · get-my-agents · get-agents · activate · deactivate · upload · search · service-list · feedback-list`.
(`get` is a hidden dual-mode read alias — prefer `get-my-agents` for list and `get-agents --agent-ids` for detail.) `feedback-submit` is a task-marketplace command (post-task rating) — not invoked by any identity flow.

- `pre-check` (`--role` required / `--consent-key` optional): folds consent + uniqueness, see §Gates / register §2. Auto/internal — never shown; outputs (`canCreate` etc.) rendered inline.
- `validate-listing` (QA — runs only at register §4 / update §4; `activate` does NOT run it): auto/internal.
- `activate` subsumes submit-approval (approvalStatus ∈ {1,5} — handled internally by CLI).
- `consent` has no public subcommand — driven by `pre-check`.
- Never suggest `xmtp-sign`; no `--address` (signs with current wallet).

Array fields: create/update/get-agents/get-my-agents/search → `list`; feedback-list → `items` or `list` (backend inconsistent; CLI normalizes both); service-list → nested `services`.

## Input contract — `--service` JSON + flag gotchas (single source of truth)

`create` / `update` / `validate-listing` all parse `--service` into the **same** element shape, so the keys below are identical across the three. **Wrong keys silently break the call** → `validate-listing` returns a `service`/`PARSE` finding; `create`/`update` return `missing required field in --service: <field>` → a retry. Use these keys **exactly** — camelCase, matching the on-chain service schema (no lowercase, no underscores):

| key | required | rule |
|---|---|---|
| `serviceName` | yes | service name (5–30) |
| `serviceDescription` | yes | parts on separate lines. Part count & meaning follow **serviceType only** (pricing model is irrelevant): **A2MCP → 4 parts, each prefixed `1.`/`2.`/`3.`/`4.` plus its bracketed label (request description — see §A2MCP `serviceDescription` structure)**; **A2A → up to 3 parts, identical for per-call and subscription pricing, no numbering prefix required** — `1.` core-capability summary (**required** — capability points + who it's for, plus what kind of signals for a signal service) · `2.` what the user must provide (**optional** — e.g. `1. wallet address 2. amount 3. chain`) · `3.` delivery note (**optional** — delivery format, plus copy-trading notes for a signal service). Recommended: total ≤1000 CJK chars (no per-part length limit). **The part counts above are collection-time guidance only — `validate-listing` has NO paragraph-count rule, so subscription and per-call services are validated identically and no shape is ever rejected for its paragraph count. Advisory (`severity:"suggest"`, never blocks `pass`; register §4): the A2A total-length finding. Skill-layer A2A semantic suggestions fire ONLY when the description lacks a core-capability introduction; otherwise no semantic description suggestion is shown. Still blocking: a test marker and an empty description (every service type), and a URL (A2A only — an A2MCP request description must carry the endpoint URL of its `curl` example); A2MCP additionally uses the blocking request-description check (§A2MCP `serviceDescription` structure).** Length is counted in **East-Asian display width** (CJK = 2, ASCII = 1) |
| `serviceType` | yes | raw enum `A2MCP` (API service) or `A2A` (agent to agent) — never the localized label |
| `fee` | A2MCP yes / A2A: exactly one real price across `fee` & `subscription` | a **plain number as a JSON string**, e.g. `"10"` (quoted — never a bare number `10`). USDT is the implicit, only currency; **no currency suffix/symbol**, ≤6 dp. `"10 USDT"` / a fee carrying any localized currency word or symbol → rejected (P1). Both keys are always transmitted; **exactly one** carries a real price — A2A subscription-priced → send an empty `fee` (`""`) alongside the `subscription` (P2 if neither has a price, P6 if both do) |
| `subscription` | **A2A only** | array of monthly tiers `[{"interval":"month","fee":"10"}]`. `interval` currently limited to `"month"` (P4 otherwise); each tier `fee` follows the same plain-number rule (P5 otherwise). Empty `[]` = no subscription. **Forbidden on A2MCP** (P3). An A2A service carries **exactly one** of `fee` XOR a non-empty `subscription` — never neither (P2), never both (P6). |
| `freeTrial` | **A2A subscription only, optional** | free-trial duration in **hours** as a plain-number string. The skill offers a **fixed 3-day** trial → send `"72"` when the user opts in, **omit entirely** when they don't (never `""`, never `"0"`). Only valid alongside a non-empty `subscription` — **forbidden on a single-fee A2A and on A2MCP** (P7); must be a positive integer (P8). |
| `endpoint` | A2MCP only | `https://…`; **omit entirely for A2A** |
| `operation` | **`update` flow only** | one of `create` / `update` / `delete` — the per-service delta directive (see update.md §6). **Omit entirely on `create` / register** (services there are all new). |
| `id` | optional | the existing service's id (from `agent service-list`) — used to target an existing service in the `update` flow. |

Example (register / `create` — no `id`, no `operation`): `--service '[{"serviceName":"…","serviceDescription":"…","serviceType":"A2MCP","fee":"10","endpoint":"https://…"}]'`
Example (`update` delta — modify one service): `--service '[{"operation":"update","id":"<existing-id>","serviceName":"…","serviceDescription":"…","serviceType":"A2MCP","fee":"10","endpoint":"https://…"}]'`
Example (A2A, per-call only): `--service '[{"serviceName":"…","serviceDescription":"…","serviceType":"A2A","fee":"0.11"}]'`
Example (A2A, subscription-priced — empty `fee` for the single price): `--service '[{"serviceName":"…","serviceDescription":"…","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}]}]'`
Example (A2A, subscription + 3-day free trial): `--service '[{"serviceName":"…","serviceDescription":"…","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}],"freeTrial":"72"}]'`

### A2MCP `serviceDescription` structure (request description) — type-split

When `serviceType == "A2MCP"`, the four numbered storage lines carry a **request description** so buyers and the sandbox know how to call the service. A2A semantics are unchanged (see the `serviceDescription` row above).

| Line | A2MCP meaning | A2A meaning (unchanged) |
|---|---|---|
| `1.` | `[Service Description]` — what the service does | Core-capability summary |
| `2.` | `[Parameter Spec]` — **all** key parameters on **one line**, separated by `;` (full-width `；` for CJK), each in the **strict format** `<name>(<type>, required/optional): <meaning>` (see the *Parameter-spec strict format* bullet below) | What the user must provide (optional) |
| `3.` | `[Request Method]` — **only** an HTTP-protocol verb (`POST` / `GET` / `PUT` / `PATCH` / `DELETE` / …) **or** a bare MCP tool name. **No URL, no path, no other text** — the callable address already lives in the separate `endpoint` field (see the *Request-method — verb / tool-name only* bullet below) | Delivery note (optional) |
| `4.` | `[Request Example]` — a working `curl` command showing a complete call to the service (see the *Request-example* bullet below) | *(A2A does not use this line)* |

- **Blocking, not advisory.** All four A2MCP items must be present by meaning (not literal keywords). Any missing → the Skill **blocks** the flow at register §4 / update §4 (wherever `validate-listing` runs; `activate` does not re-run QA). This differs from A2A advisory description findings: the CLI total-length finding and the skill-layer missing-core-capability suggestion never block `pass`; a URL or a test marker still blocks, and paragraph count is never checked at all. **Contract-address exemption:** if the `serviceDescription` contains a contract address (`0x` followed by 40 hex characters, e.g. `0xAbCd…1234`), skip the four-item completeness check entirely and accept the description as-is.
- **Reformat rule.** Whatever shape the user gives their input in, reformat it into the `1./2./3./4.` numbered-line storage structure (with the label prefix per the next bullet) — never store loose raw phrasing verbatim when it doesn't already match. Storage format is unchanged. (No copyable fill template is shown to the user during register or update — the inline four-part prompt plus the rejection reason + user suggestion below are the only guidance.)
- **Label prefix (each storage line carries its bracketed tag — supplement only when absent).** Each of the four numbered storage lines is prefixed with its section label: `[Service Description]` (line `1.`) · `[Parameter Spec]` (line `2.`) · `[Request Method]` (line `3.`) · `[Request Example]` (line `4.`) — localized to the conversation language (translate the four bracket labels; keep the bracket form). **If the user already typed the label, keep THEIRS verbatim — never duplicate it; if they omitted it, supplement it.** A stored line therefore reads e.g. `1. [Service Description] Translates input text into a target language` / `2. [Parameter Spec] text (string, required): source text to translate; target_lang (string, optional): target language code, default en` / `3. [Request Method] POST` / `4. [Request Example] curl -X POST https://… -H "Content-Type: application/json" -d '{"text":"hello","target_lang":"zh"}'`.
- **Parameter-spec strict format (line `2.`).** Write ALL key parameters on **one line**, separating adjacent parameters with `;`, each parameter as `<name>(<type>, <required|optional>): <meaning>` — for an **optional** parameter, append its default value to the meaning: `<name>(<type>, optional): <meaning>, <default>`. `<type>` is the value type (`string` / `number` / `boolean` / `object` / …); `<required|optional>` is the required/optional marker. Render the punctuation and the marker words in the user's current language — ASCII `(` `,` `)` `:` `;` for Latin (e.g. `text (string, required): source text to translate; target_lang (string, optional): target language code, default en`); CJK conversations use the full-width equivalents `（` `，` `）` `：` `；` and the localized required/optional marker words, with the parameter meanings written in the conversation language.
- **Proactively normalize a malformed param spec, then confirm — never silently store it.** If the user's parameter-spec input is **present but not in the strict format** above (e.g. free prose like "needs a text and a target language", or missing the `<type>` or the required/optional marker), the Skill MUST proactively rewrite it into the strict one-line `;`-separated format (localized punctuation), SHOW the rewritten version to the user, and ask them to confirm (or correct) it **before** it is stored. This normalization is separate from the completeness block: the block (above) fires only when the parameter spec is **entirely absent**; a present-but-loosely-worded spec is normalized-and-confirmed, not rejected.
- **Overflow tie-break.** When a full per-parameter enumeration cannot fit the total ≤1000 CJK cap, concisely listing the key parameters (each still in the strict `<name>(<type>, required/optional): <meaning>` format, `;`-separated on the one line) satisfies line `2.`; never block solely because not every parameter is enumerated (length limits are unchanged).
- **Request-method — verb / tool-name only (line `3.`).** Line `3.` may contain **only** an HTTP-protocol verb (`POST` / `GET` / `PUT` / `PATCH` / `DELETE` / …) **or** a bare MCP tool name (e.g. `translate`) — **nothing else**. **No URL, no protocol, no domain, no path, no query string** — the callable address already lives in the separate `endpoint` field, so any address on line `3.` is redundant. **Strip on store:** if the user's input includes a URL or path (e.g. `POST https://api.example.com/okx/scan` or `POST /okx/scan`), keep only the leading verb (or, for the MCP form, the tool name) and drop everything after it → store `POST`. If the user gave a path with **no** verb, default the verb to `POST` (or ask if genuinely ambiguous). This is a normalization, done silently at store time — it needs no confirmation card.
- **Request-example (line `4.`).** A working `curl` command that a buyer can copy-and-run to call the service. Must use the service's real endpoint (the `https://…` URL the user provided in the endpoint field) and include a realistic request body / query params that exercise the parameters described in line `2.`. If the user's input is a non-`curl` example (e.g. Python snippet, pseudo-code), reformat it into `curl` form and confirm with the user before storing. If the user provides a `curl` that uses a placeholder hostname (e.g. `localhost`, `example.com`, `<your-endpoint>`) instead of their declared endpoint, point out the mismatch and ask them to use the real endpoint.

Canonical block copy — register §4 and update §4 both display THIS on a blocking failure (single source; render prose in the user's current language, keep machine values like `POST` verbatim). **Do NOT show any copyable fill template** — only the reason + suggestion below:

- **Rejection reason:** "The request description is incomplete — it is missing one or more of: what the service does, the parameter specification, the request method, or the CURL request example. Buyers and the sandbox cannot determine how to call this service."
- **User suggestion:** "In the request description, include all four: (1) what the service does, (2) each key parameter — all on one line, separated by `;`, in the format `name(type, required/optional): meaning` (append the default value for an optional parameter), (3) the request method (POST/GET or tool name), (4) a working CURL example using the real endpoint."

**Agent-level vs service-level description (most common mix-up):** the *agent* description is the top-level `--description` flag; each *service* description is the `serviceDescription` key **inside** the `--service` JSON. Different field, different place.

**Flag gotchas (case/shape-sensitive — getting these wrong forces a retry):**
- `update` → `--agent-id` (singular); `get` → `--agent-ids` (plural). Don't swap them.
- `activate` → `--preferred-language` is **required** (BCP-47, e.g. `zh-CN` / `en-US`); omit it → `missing required parameter`.
- create role flag is `--role`; `update` has no `--role` (role is fixed at create).
