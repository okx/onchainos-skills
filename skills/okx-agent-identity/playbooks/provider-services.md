# Provider — Phase 2: Service Q&A

> Part of `playbooks/provider.md`. Called after Phase 1 (identity Q&A) is complete.
> Contains the per-service Q&A loop — one service at a time, five fields each.

## Phase 2 — service Q&A (loop once per service)

> ⛔ **No fabricated services. Ever.** Every `service.*` subfield (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) MUST come from the user's literal in-conversation reply to the matching per-service Q. When the user says "write some services for me" / "whatever" / "just give me examples" / "you figure it out" / "you fill it in" / "make some up" — **refuse and re-prompt** asking what they actually want to offer (see §Good/bad cases row 3 for the canonical decline). Do not infer `servicetype` from the service name ("sounds like MCP" — wrong, the user must choose Q3 explicitly). Do not pick a default `fee`. Do not invent an `endpoint`. Do not pipe a user-pasted JSON blob straight to the CLI (re-confirm field-by-field). Full forbidden-action list + anti-patterns in `SKILL.md §Red line 6`.

### Phase 2 preview (render BEFORE the first service's Q1)

Once Phase 1 is complete, render the Phase-2 preview **once** (not repeated for subsequent services in the loop). Then start service[1]'s Q1.

```
Identity info captured. Next we'll add services for this ASP. For each service we'll ask:
  1. Name
  2. Description
  3. Type (API-interface service / agent-to-agent service — explained again when we ask)
  4. Fee in USDT (required for API-interface, optional for agent-to-agent)
  5. Endpoint (API-interface service only)
After each service we'll ask whether to add another. One or more services, your choice.
```

Preview is declarative, not imperative — see `playbooks/README.md §STRICT`.

### Per-service Q&A

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee is required for A2MCP and optional for A2A (when an A2A user skips, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` without `skip_serializing_if`); endpoint is only needed for A2MCP.

The `Q1 / Q2 / ... / Q5` column labels in the per-service table below are **maintainer-internal indexes only** — they reset per service iteration but **MUST NOT** appear as prefixes in the prompt strings the AI sends to the user. The prompts carry no `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` and `core/ux-lexicon.md`. The preamble for service `[N]` ("Now service [N]:") contextualizes which service is being collected. The loop gate is a numbered-options pattern, not a Q-labelled question.

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

#### Suggestion-as-prompt carve-out (Q1 + Q3, opt-in)

This is the **single carve-out** to `SKILL.md §Red line 6` "field values come from the user, not from elsewhere": when the user, in an earlier turn of THIS conversation, mentioned a candidate value for the service `name` or `servicetype` (e.g. Phase 1 ask "build a provider that sells weather lookup service" — they named "weather lookup" as a likely service-name candidate, or "API-interface service" as a likely type), the Q1 / Q3 prompt **MAY** quote that mention inline as a default for the user to confirm-or-override. This is **suggestion text in the prompt**, NOT auto-fill — the user's reply this turn is still the authoritative value; if they ignore the suggestion and type something else, use what they typed.

Canonical examples (render exactly — **no `Q1：` / `Q3：` prefix** per `SKILL.md §UX Output Red Lines Red line 3`):

- **Q1 name**: `What's the name of this service? (You mentioned "weather lookup for Beijing" earlier — confirm or change?)`
- **Q3 servicetype** when user said `A2A` / `agent-to-agent` in Phase 1: `Service type? (You mentioned agent-to-agent service earlier — confirm option 2, or switch to option 1.)`
- **Q3 servicetype** when user said `A2MCP` / `API` in Phase 1: `Service type? (You mentioned API-interface service earlier — confirm option 1, or switch to option 2.)`

⛔ For Q3 specifically: when quoting the user's earlier type mention, **map their term to the long-form-with-gloss** per `core/ux-lexicon.md §Service-type` Pattern A — Q3 is a Pattern-A teaching context, so the short form alone is not enough on first encounter. **Never** echo the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK; output is not). Full source-of-truth rule: `SKILL.md §Sub-flows §Core Flow §Phase 2 Q1 UX guidance Option A`.

⛔ The carve-out **only** applies when the candidate value appeared as the user's own typed text in an earlier turn of this conversation. It does **NOT** legitimize pulling from `userEmail`, USER.md, CLAUDE.md, XMTP sender, the wallet account name, or any other session-metadata source — those remain forbidden per Red line 6.

Per-service Q&A (render `Now service [N]:` as a one-line preamble before Q1):

| Step | Ask the user (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `What's the name of this service?` + 4 segments | non-empty, ≤ 64 chars (Chinese input: ≤ 30 characters) | `name` |
| Q2 | `Describe this service.` + 4 segments | non-empty, ≤ 400 chars, must follow 3-part structure (summary / capabilities / example prompts); if not, prompt user to rewrite | `servicedescription` |
| Q3 | `Which type is this service?` + numbered-options:<br>&nbsp;&nbsp;`1. API-interface service (pay-per-call, fixed price; standard MCP (standard call protocol) interface)`<br>&nbsp;&nbsp;`2. agent-to-agent service (negotiated pricing / flexible collaboration; pricing is off-chain by default, optional on-chain reference price)`<br>`Reply 1 or 2.`<br>**Pattern A (long form inline) per `core/ux-lexicon.md §Service-type`** — Q3 is a teaching prompt (user is choosing, so they need the gloss to decide); the option text above uses the long form with gloss inside the parenthetical. This satisfies the first-occurrence-gloss requirement on its own; **no separate footnote needed below this prompt**. Subsequent renderings in the same conversation (e.g. the §3 confirmation card cell) MAY use the short form `API service` / `agent-to-agent`.<br>**Maintainer-internal mapping (NOT shown to user):** map reply `1→A2MCP` / `2→A2A` before invoking the CLI — the CLI has no numeric alias and sending raw `1` bails with `invalid servicetype`. ⛔ Never render the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK — if the user types `A2A` we accept it and map internally; output never carries the raw enum). | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if A2MCP → `Price per call? Format: number + space + currency, supports USDT / USDG, e.g. 10 USDT / 50 USDG / 0.5 USDT / 0 USDT.` + 4 segments ; if A2A → `Reference price for this service? (optional; leave empty to allow direct negotiation. Reply "skip" to skip.)` + 4 segments | A2MCP: format `number USDT\|USDG`, number ≥ 0, ≤ 6 decimal places, must be non-empty. A2A: empty OR matches the same format. **Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})? (USDT\|USDG)$` (case-insensitive); skill extracts numeric part for wire `fee` field, currency used for display only. | `fee` (when A2A is left empty, the wire payload still carries `"fee": ""` — `models.rs:21` `fee: String` has no `skip_serializing_if`. The skill renders empty fee as `free`; whether the backend distinguishes empty-string from absent-key is governed by the product spec and cannot be verified from this repo) |
| Q5 | if A2MCP → `What's the MCP (standard call protocol) endpoint URL? Must start with https:// and be reachable from the public internet (other agents will connect to your service over the public internet).` + 4 segments ; if A2A → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt:<br>`Want to add another service?`<br>&nbsp;&nbsp;`1. Add another`<br>&nbsp;&nbsp;`2. No more, finish here`<br>`Reply 1 or 2.` | reply 1 or 2 | — |

After each service is collected, echo back a one-line summary before the loop gate:
- `Recorded Service [1]: TVL Query (API service, 10 USDT, https://…).`

