# Role: provider (Agent Service Provider — ASP)

> Registers an ASP identity **with at least one service**. Longest Q&A — take it one step at a time.

## STRICT — one question per turn

No listing "Please provide 1. Name 2. Description 3. Service Name ..." in one message. Every field, including every service sub-field, is a separate turn in the user's language.

Field definitions live in `core/field-specs.md`. Inline the four segments (`Purpose / Visibility / Please note / Example`) in the user's language only, so they don't need to read a separate file to answer.

## Phase 1 — identity Q&A

### Phase 1 preview (render BEFORE Q1)

Once role is `provider` and pre-check resolved (either "no existing provider" or user chose "1. Register a new ASP" on the pre-check numbered prompt), render the Phase-1 preview, then start Q1.

```
Got it — starting a new Agent Service Provider (ASP) registration. First we'll collect identity info:
  1. Name
  2. Description
  3. Profile photo (optional)
(The service list is collected after identity is confirmed.)
```

The preview is declarative; Q1 follows after a blank line. See `playbooks/README.md §STRICT — Preview ≠ multi-field ask`.

### Q&A

> ⛔ Fields from user's literal reply only — never pre-fill from userEmail, wallet name, or session metadata. Anti-pattern to avoid: "Jim's ASP" / "Alice's ASP". Full rules: SKILL.md Red line 6.

The `Q1 / Q2 / Q3` labels in the column below are **maintainer-internal only** — they help this document index questions but **MUST NOT** appear in the prompt strings the AI sends to the user. The prompts in the Prompt column are the literal text rendered to the user; they carry no `Q1：` / `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` (no Q/S/Phase leakage) and `core/ux-lexicon.md` for the canonical rule. Each prompt inlines the four-segment field spec from `core/field-specs.md` in the user's language only. Skip any Q whose field was already captured via §One-shot capture.

| Q | Prompt | Validation |
|---|---|---|
| Q1 | `What's the name of this ASP?` + 4 segments | non-empty, ≤ 64 chars (Chinese input: ≤ 30 characters) |
| Q2 | `Describe this ASP in a sentence.` + 4 segments | non-empty, ≤ 500 chars |
| Q3 | `Profile photo? Use the default or upload one?` + Choice prompt (see `modules/avatar-upload.md`) | — |

**Strict phase boundary**: Phase 1 only captures `name` / `description` / `picture`. Even if the user mentions service info ("charging 10 USDT"), do NOT capture it here — see `core/choice-prompts.md §One-Shot Capture rule 4`.

After Q3 answered, render the Phase-1 confirmation card (identity only, no service rows yet — but note: that is **not** the final `create` — final confirmation happens after Phase 2). Or alternatively, hold identity in-memory and show one combined confirmation at the end of Phase 2. **This skill does the latter**: one final confirmation card after all services are collected. Phase-1 end transitions directly to Phase-2 preview.


> **Phase 2 — service Q&A** (per-service loop: name / description / type / fee / endpoint) has been moved to `playbooks/provider-services.md` to keep this file under 300 lines.

## Good / bad cases

| User input | Action |
|---|---|
| "I want to offer a data analysis service, charging 10 USDT" **(said in Phase 1)** | Do **NOT** capture `fee=10` at Phase 1 — phase boundary is strict (`core/choice-prompts.md §One-Shot Capture` rule 4). Continue Phase 1 Q&A; when Phase 2 starts fresh, ask Q3 (`servicetype`) first, then Q4 (`fee`) where the user can re-supply 10 if still relevant. |
| "I want to offer a data analysis service, charging 10 USDT" **(said during a service in Phase 2)** | Capture `fee=10` when Q4 asks it; still confirm `servicetype` at Q3 first. |
| "Write me some services" | Refuse to fabricate. Ask what they actually want to offer. |
| User pastes JSON blob | Thank them, but re-confirm **field by field** — typos in `servicetype` are the #1 cause of create failures. Do not pipe JSON straight to the CLI. |
| "endpoint is http://..." | Reject. Ask for HTTPS. |
| "API-interface service, fee is free" | Accept `0` but warn: "An API-interface service at 0 USDT is a free entry point — you won't be able to charge per-call later." |
| User answers multiple service fields in one sentence | Parse what you can, but next turn still asks the remaining fields individually. |
| "service type HTTP" | Reject politely and re-render the Q3 numbered prompt verbatim (see `core/choice-prompts.md`) — do not fabricate a new phrasing. |

## Confirmation

> ⛔ Mandatory before invoking the CLI — applies to both single-service and multi-service provider creates. See the mandatory confirmation gate in SKILL.md for the canonical rule + the rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") that do **NOT** bypass it.

Two-column table (`core/display-formats.md` §Create/Update Diff), services numbered inline. Render in the user's language — pick ONE variant.

> ⛔ The `<user-provided-endpoint>` token in the example below is a **doc-only placeholder** — at runtime substitute it with the **literal URL the user gave you in Phase 2 Q5** (or, on `update`, the new value the user just typed). **Never** copy any `https://api.example.com/...` / `https://cdn.example.com/...` / any other sample URL from these docs into the user's confirmation card. See `core/display-formats.md` top "URL literals are doc-only" rule.

| Field | Value |
|---|---|
| Role | Agent Service Provider (ASP) |
| Name | DeFi Analyzer |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | default |
| Service [1] Name | TVL Query |
| Service [1] Description | Query protocol TVL by chain via MCP. |
| Service [1] Type | API service |
| Service [1] Fee | 10 USDT |
| Service [1] Endpoint | `<user-provided-endpoint>` |
| Service [2] Name | Yield Check |
| Service [2] Type | agent-to-agent |
| Service [2] Fee | (skipped — negotiated directly) |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.
> Reply "execute" to run it.

**Maintainer note (not rendered):** for `agent-to-agent` (servicetype=A2A) the Fee row renders the user's value verbatim (e.g., `5 USDT`) when supplied, otherwise `(skipped — negotiated directly)`. Do NOT render `A2A` to the user in this card.

Service-field **labels in the card** are localized (see mapping table in `core/display-detail.md §Create/Update Diff`: `Name / Description / Type / Fee / Endpoint`). The **JSON keys actually sent to the CLI** (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) are lowercase schema per `models.rs::AgentService` — they only show up in the raw bash command, which we render only if the user explicitly asks.

**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks ("show me the CLI").

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role provider \
  --name "<name>" \
  --description "<description>" \
  --service '[{"name":"…","servicedescription":"…","servicetype":"A2MCP","fee":"10","endpoint":"https://…"}, {"name":"…","servicedescription":"…","servicetype":"A2A","fee":""}, {"name":"…","servicedescription":"…","servicetype":"A2A","fee":"5"}]' \
  [--picture "<url>"]
```

## ⛔ Post-success — MANDATORY template (do NOT paraphrase)

> ⛔ **After the visible line, this turn is NOT over.** → proceed to SKILL.md §Operation Flow Step 5 (which routes to `§Step 6` for the unconditional comm-init handoff). Full rules (anti-skip clauses, runtime self-gating, decline carve-out) live in Step 6 — not duplicated here.

Render **one visible line** using the template below — **verbatim except for the `#<id>` substitution rule**. Then follow the §Agent directive block (internal — not rendered to the user). Paraphrasing, adding fields (txHash, agentList, activeClients, refresh-agents output), omitting fields, adding follow-up questions, or summarizing the CLI's other JSON output are all violations of `SKILL.md §⛔ MANDATORY post-execute gate`.

### Visible line (template)

Render **one line, declarative, no question mark, no pre-announcement of the chat handoff** (the chat flow is a silent no-op outside an OpenClaw runtime; pre-announcing would mislead users in Claude Code / Claude Desktop):

`ASP identity #<id> registered — not yet visible to others. Say "activate #<id>" to publish now, or "find ASPs doing X" to check the market first.`

**`#<id>` substitution rule** (per `core/display-formats.md` top, `#<id>` placeholder rule, **with provider-specific carve-out**):

- The legitimate sources of `#<id>` for this post-success line are, in priority order:
  1. **CLI response (direct):** the `create` call's response directly contains the new agent id — substitute it verbatim.
  2. **Post-create envelope diff:** follow the two-step algorithm in `core/cli-create.md §1` "Finding the newly-minted agentId". For provider: works regardless of K=0 or K≥1 existing providers — the diff isolates the freshly-minted id. ❌ Do NOT write the filter as `agentList[*].ownerAddress == ...` — agent rows have no `ownerAddress` field.
  3. (Future) a follow-up `agent get` in a later turn — irrelevant for this immediate response.
- ⚠️ **Provider-specific danger zone — DO NOT pick any id directly from the pre-check list as `#<id>`.** Pre-check reflects state *before* this `create`, so its rows are all older providers, never the newly minted one. Source 2 above is **diff-based** (post-create envelope MINUS pre-check snapshot), not "borrow from pre-check"; it picks the id that's in the post-create envelope but **not** in the pre-check snapshot. Conflating the two is a real failure mode — the agent that does "I see provider #88 in pre-check, must be the new one" instead of running the diff will surface an older provider's id as if it were freshly created, which is misleading.
- If **both** source 1 (CLI direct id) and source 2 (envelope diff) miss — i.e. CLI returned `txHash` only **AND** the post-create `agentList` segment is also absent (WS + HTTP both failed, per `core/cli-create.md §1`) **OR** the diff yielded no new candidate under the current wallet — **omit the `#<id> ` substring entirely**: do NOT render `#`, `#<id>`, `# ?`, do NOT invent a number, do NOT borrow from the pre-check list. Fallback line:
  - `ASP identity registered — not yet visible to others. Say "activate #N" to publish now, or "find ASPs doing X" to check the market first.`

**Create does NOT auto-list** — user must explicitly run `agent activate` to publish the agent. Only after a successful activate can the agent accept tasks.

Do NOT mention the `okx-agent-chat/after-agent-list-changed.md` path to the user in the visible line — the same-turn handoff below loads that skill's own prompt, which decides on its own whether to surface anything (silent in non-OpenClaw runtimes).

### ❌ Anti-pattern (real incident, jobId=961) → ✅ Correct

❌ Agent paraphrased:
> "✅ Second provider is on-chain / agentId 961 / 4 active clients / you now have 4 agents"

Why this is a violation of `SKILL.md §⛔ MANDATORY post-execute gate`:

- Not the documented template wording — paraphrases throughout.
- Mentions `active clients` — that's internal `xmtp_refresh_agents` output, not user-relevant. Internal CLI fields (`agentList`, `activeClients`, refresh-agents counts, the full tx receipt) are NEVER user-facing; the template defines exactly what to expose.
- Re-renders / counts the agent list — violates the §Agent directive's "do NOT run `agent get`" rule.
- The natural-language suggested next action got dropped in favor of the inflated-success preamble.
- Uses raw wire-level identifiers in user-visible text — violates `SKILL.md §UX Output Red Lines` and `core/ux-lexicon.md`.

✅ Correct (with id):
> ASP identity #961 registered — not yet visible to others. Say "activate #961" to publish now, or "find ASPs doing X" to check the market first.

✅ Correct (id unknown, txHash-only return):
> ASP identity registered — not yet visible to others. Say "activate #N" to publish now, or "find ASPs doing X" to check the market first.

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. → proceed to SKILL.md §Operation Flow Step 5 — the provider row routes directly to `§Step 6` (comm-init), which loads `/skills/okx-agent-chat/after-agent-list-changed.md` Execution Flow in the same response. A fresh ASP was added and is immediately discoverable, so the OpenClaw runtime side needs sync.

Skip / decline carve-outs and the runtime self-gating contract are owned by Step 6 — not duplicated here.

**Do NOT** run `agent get` or poll status after create (that is about querying chain state — different from the Step 5 → Step 6 chain above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to Phase 2 Q1 of service[1]. If `missing required field in --service: name` surfaces, return to the specific Q (see `troubleshooting.md`). Never silently retry with a filler value.

---

## Endpoint Anti-Pattern (surfaces from Q5 and from description-level Endpoint Inquiry)

A2MCP `endpoint` MUST be:
1. `https://` scheme (not `http://`).
2. **Publicly reachable** — reachable from the open internet by the buyer's agent.
3. A real deployed service — not a placeholder, Mock URL, or doc-only example.

The CLI does NOT validate (2) or (3). Bad endpoints will be accepted and minted permanently on-chain.

### Forbidden patterns

| Pattern | Why forbidden |
|---|---|
| `http://...` (no `s`) | Insecure; many buyer agents will refuse non-TLS endpoints |
| `http://localhost` / `https://localhost` | `localhost` = buyer's own machine; buyer gets connection-refused |
| `http://127.0.0.1` / `https://127.0.0.1` | Same reason as `localhost` |
| `http://192.168.x.x` / `10.*` / `172.16-31.*` | Private RFC-1918 IPs, not publicly reachable |
| `*.local` / `*.internal` | mDNS / corporate-internal hostnames, no public DNS |
| Mock service URLs (Swagger UI / Postman Mock / mockable.io) | Time-limited; will expire into a dead endpoint |
| Placeholder strings (`https://TODO.example.com`) | Each change requires another on-chain `agent update` write |

### "No endpoint yet" response

User: "I don't have a deployed API yet" / "I haven't deployed my service yet".

> "The endpoint must be a publicly reachable `https://` URL — other agents will call it from the open internet after your service is on-chain. Deploy first, create afterwards (changing the endpoint later requires another on-chain `agent update`). Deploy your MCP server to any PaaS that gives you a public https URL, then come back to create the agent."

⛔ Never suggest `localhost` / private IP / mock services / placeholder strings.
