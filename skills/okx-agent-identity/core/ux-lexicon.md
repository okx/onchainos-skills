# UX Lexicon — Internal terms → user-facing translation

Every AI user-visible message MUST follow the **per-section rendering rule** below. For Role / Status / Field sections that means using the canonical user-facing wording; for the multi-form Service-type section that means using the form prescribed by the section's own pattern selector (long form for Pattern A teaching contexts, short form + footnote for Pattern B cell contexts). Never leak the left-column internal literal (wire-level enum / CLI flag / JSON key) into chat output. Internal reasoning, tool arguments, CLI invocations, and maintainer-facing doc blocks may use those left-column literals freely — the constraint applies only to text the user sees.

## Role

The raw `requester` / `provider` / `evaluator` enum is wire-only.

| Internal (CLI key / API field) | User-facing label |
|---|---|
| `requester` (CLI `--role` value, alias `1` / `buyer` / `requestor`) | **User Agent** |
| `provider` (CLI `--role` value, alias `2`) | **Agent Service Provider (ASP)** — the abbreviation `ASP` is acceptable after first mention in the same conversation |
| `evaluator` (CLI `--role` value, alias `3`) | **Evaluator Agent** |

The raw `requester` / `provider` / `evaluator` enum is wire-only and should not reach user-visible text (this is what causes the "buy agent" confusion). They're wire-only on the CLI `--role` flag.

**Carve-out:** if the user themselves typed `provider` / `requester` / `evaluator` in their message, the AI MAY echo their wording in the immediate reply — but the next system-initiated mention should drift back to the canonical localized term so subsequent prompts stay consistent.

## Service-type

| Internal (`servicetype`) | Long form (for "teaching" contexts, gloss inlined) | Short form (for card cells / labels) | Standalone gloss (used as footnote) |
|---|---|---|---|
| `A2MCP` | "**API-interface service** (pay-per-call, fixed price)" | "API service" | pay-per-call, fixed price |
| `A2A` | "**agent-to-agent service** (negotiated / off-chain pricing)" | "agent-to-agent" | negotiated / off-chain pricing |

⛔ **Raw `A2MCP` / `A2A` enum NEVER appears in user-visible text — period.** The raw form is the wire-level CLI `--service` payload value only; user output uses one of the two localized forms above.

### Two acceptable rendering patterns (both deliver the gloss on first occurrence)

Both patterns satisfy the "user must see the gloss on first encounter" requirement; the choice is **context-driven**, not preferential. The skill MUST use exactly one of these patterns whenever serviceType reaches user-visible text:

- **Pattern A — Inline parenthetical (long form)**: render the **long form** verbatim — the gloss sits in the parenthetical attached to the name. Used in: Q&A prompts that teach the user the choice (provider registration type-choice numbered options), error messages explaining the constraint, free-form explanations in chat. Example:
  > Which type is this service?
  >   1. API-interface service (pay-per-call, fixed price, standard MCP (standard call protocol) interface)
  >   2. agent-to-agent service (negotiated / off-chain pricing; price defaults to off-chain negotiation, with an optional on-chain reference price)

- **Pattern B — Short form + footnote below table** (preferred in cells / tables where space is tight): the **short form** sits in the cell; **on first occurrence in the conversation**, append a one-line gloss footnote below the table. Used in: detail cards, confirmation cards, service-list, search results, anywhere `serviceType` appears as a cell value. Example:
  > | TVL Query | API service | 10 USDT | ... |
  > | Yield Check | agent-to-agent | free | ... |
  >
  > Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.

### Subsequent reuse in the same conversation

After the user has seen the gloss (either via Pattern A or Pattern B), subsequent renderings in the same conversation MAY use the **short form alone** — no further gloss / footnote needed. The skill MUST still NEVER render the raw enum.

This framework is the single source of truth for service-type localization; all templates must stay aligned with it.

## Status

| Internal (`status` int) | User-facing label |
|---|---|
| `1` | active (CN: listed/published — NOT "deactivated/offline") |
| `2` | not listed (CN: not yet published — NOT "taken down") |
| `3` | This agent is currently unavailable |
| `4` | This agent is currently unavailable |
| `5` | This agent is currently unavailable |

⛔ Never render the raw integer. Always translate. Values `3` / `4` / `5` all render as the same "unavailable" copy — do NOT distinguish the reason (security / risk-control / manual) to the user.

## ApprovalDisplayStatus

Rendering rules by `status`:

| Agent `status` | Approval status cell |
|---|---|
| `1` (active / listed) | `Approved` — translate to user language |
| `2` (inactive / not listed) | Translate `approvalDisplayStatus` per table below |
| other | `—` |

`approvalDisplayStatus` translation table (used only when `status == 2`):

| `approvalDisplayStatus` | User-facing label |
|---|---|
| `1` | Not submitted for review |
| `2` | Under review, please wait |
| `4` | Approved — eligible for task recommendations |
| `5` | Review failed |
| `7` | This agent is currently unavailable |

Row label: `Approval status`.

⛔ Never render the raw integer. Always translate. When `approvalRemark` is non-empty and `approvalDisplayStatus` is `5`, append it as a parenthetical: "Review failed (reason: xxx)". This applies to **both** the detail card and the agent list view (`core/display-formats.md §1`).

## Field

| Internal (CLI JSON key) | User-facing label |
|---|---|
| `agentId` | "#N" or "Agent ID #N" (keep `#` prefix) |
| `ownerAddress` | owner wallet |
| `address` (agent record `address` field) | on-chain address |
| `chainIndex` | (don't mention — XLayer is default and the only chain) |
| `name` (agent or service) | name |
| `description` (agent) | description |
| `picture` | profile photo |
| `servicedescription` | service description |
| `servicetype` | service type |
| `fee` | price / fee |
| `endpoint` | endpoint |
| `reputation.score` | (do NOT render raw — always convert to `★ <stars>` via `score / 20`, up to 2 decimal places) |
| `reputation.count` | review count |
| `txHash` | tx hash |
| `creator-id` | (do NOT expose the literal `creator-id`; phrase as "your agent #N will be the reviewer") |
| `--agent-id` flag value | (don't expose the flag; AI fills it itself) |
| `--score` flag value | (don't expose the flag; "X stars") |

⛔ The carve-out: `Agent ID` as a column header in cards / `#<N>` as a row value is allowed (it's a stable identifier the user will see again on explorer). Everywhere else, translate.

**agentId exposure rule**: only surface `agentId` (`#N`) in user-visible output when it is directly relevant (e.g. confirmation card, post-success line, detail card). When a counterparty only needs the `address` (e.g. for payments or cross-skill references), provide `address` only — do not proactively volunteer `agentId`.

**A2A service rendering when fee is empty**: when a service of type `A2A` carries an empty / missing `fee`, render the user-facing value as `free / (skipped — negotiated directly)` — do NOT echo the wire-level empty string, and do NOT use the older "off-chain negotiation" wording (that phrasing was changed to emphasize that pricing happens **between the two parties directly**, not on some "external chain").

**EVM address display rule**: all EVM addresses (`ownerAddress`, `address` fields) must be displayed in **all-lowercase** (e.g. `0xabc...1234`, not `0xABC...1234`). The checksummed mixed-case format is a developer artifact; users see it on explorers in lowercase. Short form: `0x` + first 4 + `…` + last 4 hex chars (all lowercase).

**Chain / blockchain / NFT phrasing** (used inside user-visible "Please note" segments, post-success lines, error cards):
- `on-chain NFT` in cost/reversibility copy → render as `your record on the blockchain` — most non-engineer users don't think of identities as "NFTs", and the NFT framing is wire detail.
- `gas` / `network transaction fee` in cost copy → render as `transaction fees`. Drop the "phase 1 / OKX phase 1" framing — phase-numbering is a product-roadmap concern, not user-facing.

## Flow / internal-section terms (jargon)

These names exist purely inside the skill's own documentation and reasoning. ⛔ **Never surface them to the user.**

| Internal (skill docs / model reasoning) | How to handle in user output |
|---|---|
| `pre-check` / `Pre-Check` / `MANDATORY pre-check gate` | (just run it silently and report the result; never narrate "running pre-check") |
| `Phase 1` / `Phase 2` | If you must signpost a transition, say "**now let's set up your services**" — never "entering Phase 2" |
| `Q1:` / `Q2:` / `Q3:` / `S1:` / ... / `S6:` (numbered Q/S prompt prefixes) | Strip the prefix. Just ask the question in natural language. Example: "What's the name of this ASP?" — no `Q1:` prefix; use the canonical localized term (ASP), not raw `provider`. |
| `One-shot capture` / `pre-execute self-check` / `confirmation gate` / `post-execute gate` | (model-internal control-flow names; never appear in user text) |
| `passive onboarding` / `intent=need-requester` | (handoff metadata; never appear in user text) |
| `dual-scope rule` / `wrapper / accountName` | (rendering rule for the AI; user sees "Wallet wallet-N" headers in the agent list, not the words "wrapper" or "accountName") |
| `--service` JSON payload key names | Translate (see Field table above) |
| `MCP` (when rendered to first-time user) | Add gloss on first mention: `MCP (standard call protocol)`. Subsequent mentions in the same conversation may use bare `MCP`. |

## How to use this lexicon at runtime

The AI's user-visible draft → sweep these rules → emit:

1. Replace every `okx-*` skill literal with business language.
2. Replace every `onchainos agent <cmd>` literal with "I'll do it for you" + actually invoke the CLI.
3. Replace every role / status / field literal with its user-facing wording (see sections above). For **service-type** specifically, use **Pattern A long form** for Q&A teaching prompts / error messages / free-form chat; **Pattern B short form + footnote** for cards / tables.
4. Replace every flow-term / Q-prefix / S-prefix / Phase-N literal with natural-language phrasing (this file).
5. Check ≥5 agent counts have a reassurance footer (see `core/display-formats.md §1`).
6. Sweep for raw role enums (`requester` / `provider` / `evaluator`) outside of wire-level documentation — replace with the canonical user-facing wording (`User Agent` / `Agent Service Provider (ASP)` / `Evaluator Agent`).

If the draft survives all six sweeps without rewrite, it's safe to send.
