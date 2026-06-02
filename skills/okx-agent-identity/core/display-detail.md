# Display Formats — Agent Detail & Confirmation Cards

> Supplement to `core/display-formats.md`. Contains §2 Agent detail card, §2.5 Multi-agent detail, and §3 Create/Update Diff confirmation card.
> Global rendering rules (service-type Pattern B, URL rule, `#<id>` placeholder, photo/description row rules) are defined in `core/display-formats.md`.

## Table of Contents

| Section | Content |
|---|---|
| **§2** | Agent detail card — rendered after create / update / activate / deactivate / get --agent-ids |
| **§2.5** | Multi-agent detail — when `agent get --agent-ids` returns multiple agents |
| **§3** | Create / Update Diff confirmation card — mandatory before every content-creating write |

---

## 2. Agent detail card — after `create` / `update` / `activate` / `deactivate` / `agent get --agent-ids <id>`

| Field | Value |
|---|---|
| Agent ID | #99 |
| Name | DeFi Analyzer |
| Role | Agent Service Provider (ASP) |
| Status | active |
| Approval status | Listed — eligible for task recommendations |
| Address | 0xabc…1234 |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | <url> |
| Services | [1] TVL Query — API service, 10 USDT, `<user-or-backend-provided-endpoint>` |
| Services | [2] Yield Check — agent-to-agent, free |
| Services | [3] Whale Alert — agent-to-agent, 5 USDT |
| Rating | ★ 4.6 (18 reviews) |
| txHash | 0xabcdef…0f12 |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.

Rules:

- Two-column table. Never the Unicode box-drawing "Field / Value" art.
- Render `Role` using the user-facing label: `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render the raw ERC-8004 enum (`requester / provider / evaluator`).
- Render `Status` using `active / inactive`.
- `Approval status` row: render `approvalDisplayStatus` per `core/ux-lexicon.md §ApprovalDisplayStatus` — never expose the raw integer. When `approvalRemark` is non-empty, append it as a parenthetical. This field is independent of `status` (on-chain publish state); both rows always appear in the card when the field is present.
- Short-form address: `0x`first 4`…`last 4 hex chars. Show the full address only when the user asks.
- **⛔ `Services` rows are provider-only.** Role definitions for `requester` and `evaluator` have no service — when rendering their detail cards, **omit every `Services` row entirely** (no `Services | none` / `Services | —` / `Services | (empty)` placeholders, just drop the rows). This holds even when the backend returns `services: []` or `services: null` (or, by anomaly, a non-empty array for a non-provider role): render Service rows **only when `role == provider`**. Same constraint applies to the §3 Create / Update Diff variants.
- Services — one row per service, numbered `[N]`, single-line format **(provider only — see the rule above; on requester / evaluator skip the rows entirely)**. The **name value** (what the user typed, e.g. `TVL Query`) stays verbatim; the following descriptor uses `Name — Type, Fee, Endpoint` reading order. In practice the single-line format is `<ServiceName> — <Type>, <Fee or free>, <Endpoint>`. **A2A fee handling**: if the backend returned a non-empty `fee` for the A2A service, render it as `<N> USDT` exactly like A2MCP; if `fee` is absent / empty, render the short form `free` (Type=A2A in the same row already gives readers the off-chain-pricing context, so no parenthetical is needed in this compact row). The Endpoint cell is always dropped for A2A regardless (CLI clears it).
- `txHash` row present only when the command produced a tx (absent on read-only commands).
- `Agent ID` row: follow the `#<id>` placeholder rule at the top of this file — omit the row entirely if the id is not available yet (e.g. fresh `create` response), don't render `#` alone.
- **Single source of data — no chain calls.** All rows above (including Services and Reputation aggregate) come from the **one** `agent get --agent-ids <id>` response. The envelope is double-layer (see `core/cli-reference.md §3`): outer `list[*]` is an accountName wrapper, the actual agent row sits at `list[0].agentList[0]` for a single-id detail lookup. Field set on the agent row: `{ agentId, name, role, status, description, picture, address, services: [...], reputation: { score, count }, approvalDisplayStatus, approvalRemark }`. `approvalDisplayStatus` and `approvalRemark` are read-only backend-returned fields — render per the `Approval status` rule above; never pass the raw integer to the user. Do **NOT** chain `agent service-list --agent-id <id>` to "populate" the Services rows — they're already in the response. Do **NOT** chain `agent feedback-list --agent-id <id>` to "populate" the Reputation row — the aggregate `{ score, count }` is already there; individual review entries belong to a separate, user-triggered request (see §Post-detail prompt below).

### Post-detail prompt (after rendering §2)

After the detail card is rendered from a single-agent `agent get`, offer **one** numbered-options prompt asking whether to continue — do not auto-run anything. Follow the numbered-options pattern:

```
Want to see this agent's review details?
  1. Yes, pull the review list
  2. No, I'm good
Reply 1 or 2.
```

- On `1`: run `agent feedback-list --agent-id <id>` once and render §5 (feedback list).
- On `2`: stop. No further calls.
- No other side-queries. `service-list` is **never** triggered from this prompt — services are already shown in the detail card.

---

## 2.5. Multi-agent detail — `agent get --agent-ids <id1>,<id2>,…` with multiple ids

When the response contains more than one agent — i.e. `sum(list[*].agentList.length) > 1` after walking all accountName wrappers — render **one §2 detail card per agent** in response order (iterate wrappers, then `agentList[*]` within each), separating consecutive cards with a `---` divider line. The same data-source / no-chain rule applies per card (services and reputation already in the response — never chain `service-list` / `feedback-list` to "populate" rows that are already there).

> ⚠️ **Do NOT trigger on `list.length > 1` alone** — `list[*]` now counts accountName wrappers, not agents. `agent get --agent-ids 42,58` may land both ids inside the same wrapper's `agentList` (when both belong to one derived wallet), in which case `list.length == 1` but two agents are present. Trigger this multi-card path off the **flattened agent count**, not the wrapper count.

After all cards, render a **single multi-select Post-detail prompt** at the end (not per card):

```
Which agents' review details do you want to see?
  0. None
  1. #<id1>
  2. #<id2>
  …
Reply with matching numbers (comma-separated, e.g. "1,3").
```

- On `0` → stop. No further calls.
- Otherwise → run `agent feedback-list --agent-id <id>` **once per selected agent**, render §5 for each, separated by `---`. Never run `service-list` from this prompt.
- If the user already named which subset of returned agents they want reviews for ("show me reviews for 42 and 58"), skip the prompt entirely and go directly to those ids' `feedback-list`.

---

## 3. Create / Update Diff confirmation card

Used before executing any write that modifies fields (`create`, `update`). Three columns on `update`; two columns on `create` (nothing to diff against). Unchanged fields on `update` show `(unchanged)`.

> ⛔ **`Service [N]` rows are provider-only — applies to both Create variant and Update Diff variant.** When the role being created / updated is `requester` or `evaluator`, **do NOT** render any `Service [N] ...` row in the confirmation card (no `Service [1] | (none)`, no placeholder dash — **drop the rows entirely**). Only renders when `role == provider`. This mirrors the §2 detail-card rule above and is the canonical guard against the "buyer confirmation card shows a Service field" hallucination. Note: even on `update`, the role of the target agent (resolved from the mandatory `agent get --agent-ids <id>` pre-step) decides this — if you are editing a `requester` agent, the Update Diff card has no Service rows; if you are editing a `provider` agent, it does.

### Create variant (no current values to compare)

| Field | Value |
|---|---|
| Role | Agent Service Provider (ASP) |
| Name | DeFi Analyzer |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | default |
| Service [1] Name | TVL Query |
| Service [1] Type | API service |
| Service [1] Fee | 10 USDT |
| Service [1] Endpoint | `<user-provided-endpoint>` |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.

Service-field label mapping (user-facing labels ↔ CLI JSON keys the skill sends to `--service`):

| CLI JSON key | User-facing label |
|---|---|
| `name` | Name |
| `servicedescription` | Description |
| `servicetype` | Type |
| `fee` | Fee |
| `endpoint` | Endpoint |

Left column is the exact JSON key sent on the wire inside the `--service` payload (new lowercase schema). The right column is the user-facing label rendered in cards and Q&A prompts — never leak the raw JSON key into user-visible text.

### Update variant (diff)

| Field | Current | New |
|---|---|---|
| Name | DeFi Analyzer | (unchanged) |
| Description | On-chain data analysis. | **On-chain data analysis with yield simulation.** |
| Profile photo | <old URL> | **<new URL>** |
| Service [1] Fee | 10 USDT | (unchanged) |

> This update changes Description and Profile photo; everything else stays as-is.
> Reply "execute" to run.

Rules:

- **Three columns for update**: label them `Field / Current / New`. Unchanged rows show `(unchanged)` in the new-value column — never empty, never repeated value.
- Changed rows: bold the new-value cell so the diff reads at a glance.
- For each service entry, always list all sub-fields — easy to spot accidental drops.
- **Do NOT show the bash command in this card.** If the user asks "show me the command", render it as a separate code block afterward; otherwise omit.
- **Maintainer note (wholesale `--service` replacement, internal — do NOT surface to user):** the `--service` flag wire-level **replaces the full services list**, not a per-field patch. When only one sub-field of one service changes (e.g. only `Service [1] Fee`), the skill MUST construct the new `--service` JSON by **starting from the current full services list** (from the mandatory `agent get` pre-step) and applying the diff in memory — then send the **complete** list. Sending only the changed entry would silently delete every other service. This is a wire-level concern; do not mention `--service` in the user-visible card footer (Red line 2).
- End every diff card with exactly one line: `Reply "execute" to run.`. Do NOT use any verb like "dispatch" / "send" in this footer — see the SKILL.md "no narration between confirmation and result" rule for why.
- Source of truth for costs: `core/cost-disclosure.md`. ⛔ **Never fabricate cost items** (no "platform service fee", no "agent dispatch fee", no "review fee", no "Estimated cost" row).

---
