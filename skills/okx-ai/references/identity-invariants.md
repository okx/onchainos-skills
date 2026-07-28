# Invariants — rendering rules, id ladder, fields, commands

Load this file when: rendering a card / diff / detail view, resolving `#<id>`, translating CLI labels, or handling `--service` fields.

---

## Lexicon (prose / Q&A / post-success rows when CLI label is absent)

- **Roles:** `user` → **User** / 用户 · `asp` → **ASP** / 服务提供商 · `evaluator` → **Evaluator** / 仲裁者. Never show the raw enum token, never legacy nouns (buyer/seller), never a bilingual parenthetical.
- **Service type:** A2MCP → **API service** · A2A → **agent to agent**. Gloss once per table: "API service = pay-per-call, fixed price; agent to agent = per-call or monthly-subscription pricing (one or the other)." Never raw A2MCP/A2A.
- **Stars:** render `★ <value>` from CLI's `ratingStars` / `feedbackRate` / `average` **directly** — never divide by 20, never show raw 0–100. Null/0 context-split: **search** rows → `null`=`—`, `0`=`No rating yet`; **list / detail / feedback** → no rating = `No rating yet` (never `—`).
- **Fee:** stored/sent as a plain numeric string (`"10"`); **displayed** as `N USDT` (USDT is implicit — the renderer appends it). Both API service (A2MCP) and agent to agent (A2A) support a `0` fee → an explicit `0` displays as `0 USDT` (a free service). An empty single-purchase `fee` (`""`) means "no per-call price" (a subscription-priced A2A service) → display the Fee row as `—`, with the price in the Subscription row. A2MCP with no fee → `—` (missing required fee — not `free`, since A2MCP requires a fee at create/update).
- **Subscription (A2A only):** the `subscription[]` array carries monthly pricing tiers `{interval:"month", fee:"N"}`. **Displayed** as `N USDT / month` per tier; an empty `[]` displays as `—` (no subscription). A2MCP never has one. Fee and Subscription are **mutually exclusive** on A2A — a service shows **exactly one** of them as a real price and the other as `—` (never both real, never both `—`).
- **Free trial (A2A subscription only):** `freeTrial` is a duration in **hours**; the skill only ever sets the fixed 3-day value `"72"`. **Displayed** as its duration — `3 days` (whole days collapse to a day count; otherwise `<N> hours`) — in the Free trial column/row; absent, single-fee A2A, or A2MCP → `—`. **Address:** lowercase `0x…1234`. **Reviewer** slot = "reviewer", never "creator".

## Card skeleton (every confirmation / diff / detail card uses THIS)

Two-column pipe table `| Field | Value |`, one row per field. Role row uses localized label (never enum); photo row = uploaded CDN URL or `default` (ASPs require a URL; `default` only for user/evaluator — see register §5) — never a user-pasted link (rejected).

- **Confirmation variant** (create only): ends with `> Reply **1** to confirm and run.` (localized). No bash shown.
- **Diff variant** (update only): 3 columns `| Field | Current | New |`; unchanged fields → `(unchanged)`; changed New cell **bold**. Show real before→after values.

## Verbatim-render contract (P0-4)

When CLI returns `card[]` / `cells[]` plus `roleLabel` / `statusLabel` / `approvalLabel` / `ratingStars`, render numeric/star fields **verbatim** — do not hand-map integers, do not divide score/20, never show raw 0–100. **Verbatim applies to numbers/stars/ids/addresses only — NOT to language.** Every string `*Label` field and all surrounding prose/labels are English-canonical and MUST be translated into the SKILL §Language-Lock language before rendering. Fallback: hand-map via Lexicon if `*Label` absent (legacy response).

## CLI output fields — translate before rendering

- `roleLabel` / `statusLabel` / `approvalLabel`
- Service type values: "API service" / "agent to agent"
- Placeholder strings: "(not set)" / "default" / "No rating yet" / "(no comment)" / "free"
- `findings[].issue` and `findings[].fix` — translate the QA guidance text

## #id ladder (P0-3) — resolving `#<id>` after create

1. top-level **`newAgentId`** when its value is a **non-empty string** (PRIMARY — WS push succeeded)
2. else `agent.agentId` from the WS push object
3. `newAgentId` is `null` (WS push timed out) — omit `#<id>` substring, use fallback wording.

Never invent or borrow a pre-check id; never emit a bare `# `.
**Non-create intents** (activate/deactivate/update/detail): no `newAgentId` — use the `#N` the user typed or the CLI's direct id.

## Fields-from-user (output-safety invariant)

`name` / `description` / `picture` / `service.*` come from the user's **literal reply this turn** — never pre-filled from userEmail, wallet name, or session metadata. Carve-out: you MAY reformat the user's OWN words into the **numbered service description** on separate lines — non-subscription: `1.` core-capability summary · `2.` what the user must provide · `3.` delivery note; subscription-priced: `1.` core-capability summary · `2.` delivery note ("what the user must provide" is omitted) (illustrate, never invent a capability or metric).

**Name must be a brand, not a person (semantic QA — register §4):** block any agent name that **contains** a celebrity / public-figure name as a substring, even when prefixed or suffixed (e.g. Trump, Musk, CZ, 马斯克, 马云). This is a semantic check, not a CLI mechanical rule.

**Confirmation requirement for any reformat/draft (non-overridable):** reformatting or drafting is a *draft*, never an authorization to commit silently. Whenever you reshape the user's words into the numbered description, you MUST (1) flag every affected row on the confirmation card / diff card with an explicit marker — e.g. ` ✏️ drafted from your words — please review` — so the user can tell Claude-rewritten content from their own verbatim input, and (2) wait for the normal card confirm (Reply **1**) before the write. Never let reformatted/drafted content reach the chain presented as the user's literal input. If the user flags any drafted row as wrong, re-collect that field from their own words and redraw — do not argue or keep your draft.

## Commands (12 `onchainos agent` subcommands — you invoke them, never show them)

`create · pre-check · update · get-my-agents · get-agents · activate · deactivate · upload · search · service-list · feedback-submit · feedback-list`.
(`get` is a hidden dual-mode read alias — prefer `get-my-agents` for list and `get-agents --agent-ids` for detail.)

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
| `serviceDescription` | yes | numbered parts on separate lines, each prefixed `1.` / `2.` / `3.`. Part count follows the pricing model: **non-subscription (A2MCP or per-call A2A) → 3 parts** (`1.` core-capability summary · `2.` what the user must provide · `3.` delivery note); **subscription-priced → 2 parts** (`1.` core-capability summary · `2.` delivery note — "what the user must provide" omitted, since a subscription auto-delivers). Each part ≤200 CJK chars, total ≤600 CJK chars; no example prompts / links / tech-stack / disclaimers / profit guarantees (deterministically caught by the CLI's D9 phrase list). Subscription-priced services (= the trading-signal service, per this skill's convention; non-subscription services are ordinary and skip this): the core-capability part must declare covered markets (only DEX / Polymarket / Hyperliquid) and the delivery note must carry a full-market-name signal example (register §4). Length is counted in **East-Asian display width** (CJK = 2, ASCII = 1) |
| `serviceType` | yes | raw enum `A2MCP` (API service) or `A2A` (agent to agent) — never the localized label |
| `fee` | A2MCP yes / A2A: exactly one real price across `fee` & `subscription` | a **plain number as a JSON string**, e.g. `"10"` (quoted — never a bare number `10`). USDT is the implicit, only currency; **no currency suffix/symbol**, ≤6 dp. `"10 USDT"` / `"5元"` → rejected (P1). Both keys are always transmitted; **exactly one** carries a real price — A2A subscription-priced → send an empty `fee` (`""`) alongside the `subscription` (P2 if neither has a price, P6 if both do) |
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

**Agent-level vs service-level description (most common mix-up):** the *agent* description is the top-level `--description` flag; each *service* description is the `serviceDescription` key **inside** the `--service` JSON. Different field, different place.

**Flag gotchas (case/shape-sensitive — getting these wrong forces a retry):**
- `update` → `--agent-id` (singular); `get` → `--agent-ids` (plural). Don't swap them.
- `activate` → `--preferred-language` is **required** (BCP-47, e.g. `zh-CN` / `en-US`); omit it → `missing required parameter`.
- create role flag is `--role`; `update` has no `--role` (role is fixed at create).
