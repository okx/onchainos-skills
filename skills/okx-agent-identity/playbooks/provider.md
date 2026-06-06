# Role: provider (Agent Service Provider — ASP)

> Registers an ASP identity **with at least one service**, in **two explicit steps**: Step 1 Identity → Step 2 Service. Each step opens with a **numbered checklist of its fields, each annotated with its requirement**, so the user sees exactly what to provide and the rules up front. Within a step the user may batch all items in one message or go one at a time. (The on-chain write is still a single `agent create` carrying both — `ensure_provider_has_service`.)

Field definitions live in `core/field-specs.md`; listing requirements (lengths, bans) come from `modules/pre-listing-qa.md`. The numbered checklist below merges both so the user can comply first-try.

## Step 1 · Identity

Once role is `provider` and pre-check resolved (either "no existing provider" or user chose "1. Register a new ASP"), render the Step-1 checklist, then collect.

```
Step 1 of 2 · Identity — please provide these 3 items (send all at once, or one at a time):
  1. Name (required) — 2–12 chars CN / 3–25 EN; a short brand name; ❌ no test tags (-test/(beta)/_dev), ❌ no public-figure names
  2. Description (required) — ≤ 500 chars; one line: what it does, which chain, your edge
  3. Profile photo (optional) — 1:1 square PNG/JPEG/WebP, < 1 MB; for best display avoid rounded corners and borders (a plain square renders best); skip to use the default
```

- The checklist is a **declarative requirements preview** (allowed — `playbooks/README.md §STRICT`); it lists fields + rules, then the user fills them (batched or one-at-a-time). Localize to the user's language.
- ⛔ Field values from the user's literal reply only (Red line 6) — never pre-fill from userEmail / wallet name / session metadata. Anti-pattern: "Jim's ASP".
- Capture whatever the user batches (`core/choice-prompts.md §One-Shot Capture`); ask only the **still-missing** item(s), one at a time — no `Q1:` prefix (Red line 3).
- **Profile photo (item 3)** can't be batched (image links are rejected; it needs the upload/generate interaction). Surface it as part of the **identity confirmation card's closing CTA** (📷 send an image / "generate" / skip) — not a separate collection turn.
- When identity is collected, render the **identity confirmation card** (`§Confirmation cards — two steps`); confirming it advances to Step 2 and does NOT call the CLI.

## Step 2 · Service

After the identity card is confirmed, render the Step-2 checklist, then collect the service fields (full per-field spec + validation in `playbooks/provider-services.md`).

```
Step 2 of 2 · Service (at least one) — please provide these 5 items (send all at once, or one at a time):
  1. Service name (required) — 5–30 chars; a noun phrase; ❌ not identical to the agent name, ❌ no price in the name
  2. Service description (required) — 3-part: ① summary ≤50 ② capabilities ≤150 ③ 1–3 example prompts ≤80 each (just say it in plain words — I'll format it for you)
  3. Type (required) — 1. API-interface (pay-per-call, needs endpoint) / 2. agent-to-agent (negotiated, no endpoint)
  4. Fee — API: required, "number + currency" (USDT/USDG), ≤6 decimals, e.g. 10 USDT; agent-to-agent: optional (leave blank to negotiate)
  5. Endpoint — API only, required: starts with https://, publicly reachable, really deployed (❌ localhost / private IP / mock)
```

- Same rules: declarative checklist → batch or one-at-a-time → capture, ask only what's missing.
- ⚠️ Item 5 is the dead-end risk — if the user picks API but has no deployed endpoint, say so now (deploy first, or switch to agent-to-agent), don't collect the rest first.
- When the service is collected, render the **service confirmation card**; "execute" runs the single `agent create` (identity from Step 1 + this service).

## Good / bad cases

| User input | Action |
|---|---|
| "I want to offer a data analysis service, charging 10 USDT" **(batched up front)** | Capture `fee=10` into the service buffer (`core/choice-prompts.md §One-Shot Capture` rule 4 — batch capture, no longer discarded). Still confirm `servicetype` explicitly (it's a choice field — never infer it from the service name); ask only the service fields the user did not batch. |
| "data analysis service, 10 USDT, API-interface, https://…" | Capture `name` / `fee` / `servicetype=A2MCP` / `endpoint` from the batch; ask only the missing `servicedescription`. |
| "Write me some services" | Refuse to fabricate. Ask what they actually want to offer. |
| User pastes JSON blob | Thank them, but re-confirm **field by field** — typos in `servicetype` are the #1 cause of create failures. Do not pipe JSON straight to the CLI. |
| "endpoint is http://..." | Reject. Ask for HTTPS. |
| "API-interface service, fee is free" | Accept `0` but warn: "An API-interface service at 0 USDT is a free entry point — you won't be able to charge per-call later." |
| User answers multiple service fields in one sentence | Parse what you can, but next turn still asks the remaining fields individually. |
| "service type HTTP" | Reject politely and re-render the Q3 numbered prompt verbatim (see `core/choice-prompts.md`) — do not fabricate a new phrasing. |

## Confirmation cards — two steps (identity card → service card)

The two-step flow has **two confirmation cards**: the Identity card closes Step 1, the Service card closes Step 2 and triggers the create. The on-chain write is still one `agent create` carrying both (the CLI requires a provider to have ≥1 service — `ensure_provider_has_service`).

> ⛔ Both cards are mandatory before the CLI runs (the confirmation gate in SKILL.md). The rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") do **NOT** bypass either. Even if the user batched everything in Step 1, render both cards in order.
>
> Token note: two cards cost ~1 extra turn vs a single merged card — this is the user-requested two-step structure; the trade is accepted for the clearer identity/service separation.

### Identity card (closes Step 1 — does NOT create anything)

**Run identity-scope QA silently first** (`modules/pre-listing-qa.md` Trigger C — name N1–N7, description U1–U4); inline ⚠️ on any offending row.

| Field | Value |
|---|---|
| Role | Agent Service Provider (ASP) |
| Name | DeFi Analyzer |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | default (not set) |

> 📷 Profile photo is the default — **send an image or say "generate" to set one** (for best display: a plain square, no rounded corners or borders — see `modules/avatar-upload.md §Policy 7`).
> Identity good? Reply "next" to set up your service (or change anything above).

- **Avatar is actively prompted here at the card's close — not a passive row hint** (real runs showed a faint row hint is ignored, leaving every agent on the default image). The user skips by replying "next". On opt-in (image / "generate") run `modules/avatar-upload.md` and show the URL in the row.
- **Editable in place** — apply, re-run identity QA, re-render.
- **Confirming the identity card ("next") advances to Step 2 — it does NOT call the CLI.**

### Service card (closes Step 2 — this is the create confirmation)

**Run service-scope QA silently first** (`modules/pre-listing-qa.md` Trigger C — T/S/P/D + U on the service); inline ⚠️ on offending rows.

> ⛔ The `<user-provided-endpoint>` token below is a **doc-only placeholder** — substitute the **literal endpoint URL the user gave**. **Never** copy any `https://api.example.com/...` / sample URL from these docs into the card. See `core/display-formats.md` top "URL literals are doc-only" rule.

| Field | Value |
|---|---|
| Service [1] Name | TVL Query |
| Service [1] Description | Query protocol TVL by chain via MCP. |
| Service [1] Type | API service |
| Service [1] Fee | 10 USDT |
| Service [1] Endpoint | `<user-provided-endpoint>` |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.
> Want to change anything? Just say so (e.g. "fee 5 USDT"). Otherwise reply "execute" to register (with the identity from Step 1).

- **Editable in place** — apply, re-run service QA, re-render. To change an identity field here, accept it and note it'll be included.
- **QA inline** — append ` ⚠️ <issue> → <fix>` to any offending row. Confirming with warnings present = "register anyway" (issues resurface at listing).

**Maintainer note (not rendered):** for `agent-to-agent` (servicetype=A2A) the Fee row renders the user's value verbatim (e.g., `5 USDT`) when supplied, otherwise `(skipped — negotiated directly)`. Do NOT render `A2A` to the user.

Service-field **labels in the cards** are localized (`core/display-detail.md §Create/Update Diff`: `Name / Description / Type / Fee / Endpoint`). The CLI JSON keys (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) are wire-only schema per `models.rs::AgentService` — they appear only in the raw bash command, rendered only if the user explicitly asks.

**Do NOT show bash** in either card. Only render the bash command if the user explicitly asks ("show me the CLI").

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

`ASP identity #<id> registered — not yet visible to others. Say "activate #<id>" to publish now, "add a service to #<id>" to offer more services, or "find ASPs doing X" to check the market first.`

**`#<id>` substitution rule** (per `core/display-formats.md` top, `#<id>` placeholder rule, **with provider-specific carve-out**):

- The legitimate sources of `#<id>` for this post-success line are, in priority order:
  1. **CLI response (direct):** the `create` call's response directly contains the new agent id — substitute it verbatim.
  2. **Post-create envelope diff:** follow the two-step algorithm in `core/cli-create.md §1` "Finding the newly-minted agentId". For provider: works regardless of K=0 or K≥1 existing providers — the diff isolates the freshly-minted id. ❌ Do NOT write the filter as `agentList[*].ownerAddress == ...` — agent rows have no `ownerAddress` field.
  3. (Future) a follow-up `agent get` in a later turn — irrelevant for this immediate response.
- ⚠️ **Provider-specific danger zone — DO NOT pick any id directly from the pre-check list as `#<id>`.** Pre-check reflects state *before* this `create`, so its rows are all older providers, never the newly minted one. Source 2 above is **diff-based** (post-create envelope MINUS pre-check snapshot), not "borrow from pre-check"; it picks the id that's in the post-create envelope but **not** in the pre-check snapshot. Conflating the two is a real failure mode — the agent that does "I see provider #88 in pre-check, must be the new one" instead of running the diff will surface an older provider's id as if it were freshly created, which is misleading.
- If **both** source 1 (CLI direct id) and source 2 (envelope diff) miss — i.e. CLI returned `txHash` only **AND** the post-create `agentList` segment is also absent (WS + HTTP both failed, per `core/cli-create.md §1`) **OR** the diff yielded no new candidate under the current wallet — **omit the `#<id> ` substring entirely**: do NOT render `#`, `#<id>`, `# ?`, do NOT invent a number, do NOT borrow from the pre-check list. Fallback line:
  - `ASP identity registered — not yet visible to others. Say "activate #N" to publish now, "add a service to #N" to offer more services, or "find ASPs doing X" to check the market first.`

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
> ASP identity #961 registered — not yet visible to others. Say "activate #961" to publish now, "add a service to #961" to offer more services, or "find ASPs doing X" to check the market first.

✅ Correct (id unknown, txHash-only return):
> ASP identity registered — not yet visible to others. Say "activate #N" to publish now, "add a service to #N" to offer more services, or "find ASPs doing X" to check the market first.

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. → proceed to SKILL.md §Operation Flow Step 5 — the provider row routes directly to `§Step 6` (comm-init), which loads `/skills/okx-agent-chat/after-agent-list-changed.md` Execution Flow in the same response. A fresh ASP was added and is immediately discoverable, so the OpenClaw runtime side needs sync.

Skip / decline carve-outs and the runtime self-gating contract are owned by Step 6 — not duplicated here.

**Do NOT** run `agent get` or poll status after create (that is about querying chain state — different from the Step 5 → Step 6 chain above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to service collection and collect service[1]'s name first. If `missing required field in --service: name` surfaces, return to the specific service field (see `troubleshooting.md`). Never silently retry with a filler value.

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
