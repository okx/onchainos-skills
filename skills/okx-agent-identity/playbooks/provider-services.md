# Provider — Service Collection

> Part of `playbooks/provider.md`. The collection overview (identity + service fields, with the batch invitation) is already rendered there — **do not re-render a separate service preview here.**
> Contains the per-service field set — five fields each. Ask only the fields the user did **not** already batch.

## Service collection (one service by default)

> ⛔ **No fabricated services. Ever.** Every `service.*` subfield (`name` / `servicetype` / `fee` / `endpoint`) MUST come from the user's literal in-conversation reply (batched or asked). When the user says "write some services for me" / "whatever" / "just give me examples" / "you figure it out" / "you fill it in" / "make some up" — **refuse and re-prompt** asking what they actually want to offer (see `playbooks/provider.md` §Good / bad cases row 3 for the canonical decline). Do not infer `servicetype` from the service name ("sounds like MCP" — wrong, the user must choose the type explicitly). Do not pick a default `fee`. Do not invent an `endpoint`. Do not pipe a user-pasted JSON blob straight to the CLI (re-confirm field-by-field). **The one carve-out is `servicedescription`** — the AI may format the user's stated meaning into the 3-part structure and draft example prompts illustrating it, for approval (see `§Description: AI drafts it`); it still must not invent a service the user never described. Full forbidden-action list + anti-patterns in `SKILL.md §Red line 6`.

**Default to a single service.** Collect exactly one service (required — providers need ≥1), then go to the confirmation card. **Do not ask "add another service?" as a routine turn.** The user can add more services after registration (post-success line tells them how), or batch multiple services in their overview reply — capture all of them when they do.

### Field complexity tiers — batch the simple, split the complex

Batch-first does **not** mean "swallow everything one-shot regardless of quality". Two service fields are structured / high-risk; force-fitting them into a batch line just produces a value that fails validation and bounces at the confirmation card — worse UX than a focused step. So split by tier:

| Tier | Fields | Batch behavior |
|---|---|---|
| 🟢 **Simple** | `name`, `servicetype`, `fee` | Capture straight from the batch. `servicetype` still echoes the long-form gloss; `fee` re-asks only on format error. |
| 🔴 **Complex / high-risk** | `servicedescription` (3-part structure, ≤400), `endpoint` (https + public + permanent on-chain) | Accept from the batch **only if it passes validation**. If the batched value fails (description not 3-part, endpoint fails the anti-pattern blacklist or isn't https), **peel that one field into its own focused step** with the structure template / endpoint requirements inline — do **not** silently accept it and do **not** defer the failure to the confirmation card. If the field was not batched at all, ask it as its own guided step (it always is, per the table below). |

So a one-shot like "TVL query, API, 10 USDT, https://api.x/mcp, queries TVL" registers name/type/fee/endpoint from the batch, then **splits out** one focused step for the service description (because "queries TVL" is not the required 3-part structure). The simple fields never block; only the complex one gets the extra turn it actually needs.

#### Description: AI drafts it (the 3-part structure must never bounce the user repeatedly)

The 3-part description is the hardest field to write cold, and real runs showed the strict structure + sub-limits + "examples must come from the user" repeatedly bouncing a user who typed one marketing line. **Fix: the AI does the heavy lifting — the user supplies meaning in plain words once, the AI produces a compliant draft for approval.** The description step should almost never re-ask.

Offer, in the focused description step:

> "Just tell me in plain words what this service does and who it's for — I'll write the listing-ready description for you to approve. Or write it yourself if you prefer."

From whatever the user gives, the AI **produces a compliant 3-part draft in one shot**:
- ① summary, ② capabilities, ③ example prompts — **auto-trimmed** to the D-limits (①≤50 / ②≤150 / ③ each ≤80). The user is **never** re-prompted just because their wording was too long; the AI trims its own draft.
- ③ **example prompts may be AI-drafted** — write 1–3 sample user questions that **illustrate the capability the user already stated** (a usage example is a restatement of stated function, not a new capability). Show them for approval.
- Then present the full draft for **explicit approval** ("Use this?" / edit). The user's edits win.

⛔ **Guardrail — illustrate, don't invent:**
- ①/② use **only** what the user stated — add **no** new capability, chain, metric, or claim they did not say.
- ③ example prompts must be **plausible usages of the stated capability** — never imply a feature the user didn't describe.
- **Re-ask only when the core "what it does" is genuinely missing** (user said essentially nothing) — not for length, not for a missing example (the AI drafts the example), not for phrasing.
- This carve-out applies to `servicedescription` only — never to `name` / `servicetype` / `fee` / `endpoint`, which always come verbatim from the user. See `SKILL.md §Red line 6`.

### Per-service field set

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee is required for A2MCP and optional for A2A (when an A2A user skips, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` without `skip_serializing_if`); endpoint is only needed for A2MCP.

The `Q1 / Q2 / ... / Q5` column labels in the per-service table below are **maintainer-internal indexes only** — they reset per service iteration but **MUST NOT** appear as prefixes in the prompt strings the AI sends to the user. The prompts carry no `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` and `core/ux-lexicon.md`. The preamble for service `[N]` ("Now service [N]:") contextualizes which service is being collected, and is only used when collecting more than one service.

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

#### Suggestion-as-prompt carve-out (Q1 + Q3, opt-in)

This is the **single carve-out** to `SKILL.md §Red line 6` "field values come from the user, not from elsewhere": when the user, in an earlier turn of THIS conversation (e.g. the overview reply, or the initial intent), mentioned a candidate value for the service `name` or `servicetype` (e.g. "build a provider that sells weather lookup service" — they named "weather lookup" as a likely service-name candidate, or "API-interface service" as a likely type), the name / type prompt **MAY** quote that mention inline as a default for the user to confirm-or-override. This is **suggestion text in the prompt**, NOT auto-fill — the user's reply this turn is still the authoritative value; if they ignore the suggestion and type something else, use what they typed.

Canonical examples (render exactly — **no `Q1：` / `Q3：` prefix** per `SKILL.md §UX Output Red Lines Red line 3`):

- **name**: `What's the name of this service? (You mentioned "weather lookup for Beijing" earlier — confirm or change?)`
- **servicetype** when user said `A2A` / `agent-to-agent` earlier: `Service type? (You mentioned agent-to-agent service earlier — confirm option 2, or switch to option 1.)`
- **servicetype** when user said `A2MCP` / `API` earlier: `Service type? (You mentioned API-interface service earlier — confirm option 1, or switch to option 2.)`

⛔ For the type field specifically: when quoting the user's earlier type mention, **map their term to the long-form-with-gloss** per `core/ux-lexicon.md §Service-type` Pattern A — the type prompt is a Pattern-A teaching context, so the short form alone is not enough on first encounter. **Never** echo the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK; output is not).

⛔ The carve-out **only** applies when the candidate value appeared as the user's own typed text in an earlier turn of this conversation. It does **NOT** legitimize pulling from `userEmail`, USER.md, CLAUDE.md, XMTP sender, the wallet account name, or any other session-metadata source — those remain forbidden per Red line 6.

Ask only the fields the user did not batch. When collecting more than one service (user batched or explicitly asked to add another), render `Now service [N]:` as a one-line preamble before the first field of service [N]:

| Step | Ask the user (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `What's the name of this service?` + 4 segments | non-empty, 5–30 chars (Chinese input: ≤ 30 characters) | `name` |
| Q2 | `Describe this service.` + 4 segments | non-empty, ≤ 400 chars, must follow 3-part structure (summary / capabilities / example prompts); if not, prompt user to rewrite | `servicedescription` |
| Q3 | `Which type is this service?` + numbered-options:<br>&nbsp;&nbsp;`1. API-interface service (pay-per-call, fixed price; standard MCP (standard call protocol) interface)`<br>&nbsp;&nbsp;`2. agent-to-agent service (negotiated pricing / flexible collaboration; pricing is off-chain by default, optional on-chain reference price)`<br>`Reply 1 or 2.`<br>**Pattern A (long form inline) per `core/ux-lexicon.md §Service-type`** — Q3 is a teaching prompt (user is choosing, so they need the gloss to decide); the option text above uses the long form with gloss inside the parenthetical. This satisfies the first-occurrence-gloss requirement on its own; **no separate footnote needed below this prompt**. Subsequent renderings in the same conversation (e.g. the §3 confirmation card cell) MAY use the short form `API service` / `agent-to-agent`.<br>**Maintainer-internal mapping (NOT shown to user):** map reply `1→A2MCP` / `2→A2A` before invoking the CLI — the CLI has no numeric alias and sending raw `1` bails with `invalid servicetype`. ⛔ Never render the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK — if the user types `A2A` we accept it and map internally; output never carries the raw enum). | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if A2MCP → `Price per call? Format: number + space + currency, supports USDT / USDG, e.g. 10 USDT / 50 USDG / 0.5 USDT / 0 USDT.` + 4 segments ; if A2A → `Reference price for this service? (optional; leave empty to allow direct negotiation. Reply "skip" to skip.)` + 4 segments | A2MCP: format `number USDT\|USDG`, number ≥ 0, ≤ 6 decimal places, must be non-empty. A2A: empty OR matches the same format. **Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})? (USDT\|USDG)$` (case-insensitive); skill extracts numeric part for wire `fee` field, currency used for display only. | `fee` (when A2A is left empty, the wire payload still carries `"fee": ""` — `models.rs:21` `fee: String` has no `skip_serializing_if`. The skill renders empty fee as `free`; whether the backend distinguishes empty-string from absent-key is governed by the product spec and cannot be verified from this repo) |
| Q5 | if A2MCP → `What's the MCP (standard call protocol) endpoint URL? Must start with https:// and be reachable from the public internet (other agents will connect to your service over the public internet).` + 4 segments ; if A2A → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |

This is **Step 2** of the two-step flow (`playbooks/provider.md §Step 2 · Service`); the identity card already closed Step 1. Once the single service is complete, run the silent service-scope QA pass and render the **service confirmation card** (`playbooks/provider.md §Confirmation cards — two steps` → `modules/pre-listing-qa.md` Trigger C service scope, QA inline); "execute" runs the single `agent create`. **Do not ask whether to add another.** (Only loop back for another service if the user explicitly asks, or batched multiple services in one message.) After each service is captured, echo back a one-line summary:
- `Recorded Service [1]: TVL Query (API service, 10 USDT, https://…).`

