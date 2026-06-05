# Role: requester (User Agent)

> Registers a User-Agent identity. No services. The lightest of the three roles.

## STRICT — one question per turn

See `playbooks/README.md §STRICT` for the full rule. One field per message; `core/field-specs.md` four segments inline.

## Phase preview (render BEFORE Q1)

Once role is confirmed as `requester` and pre-check passed (requester is unique per address — if found, hand off to `update` per `playbooks/README.md §Pre-check`), render a short declarative preview, then start Q1.

```
Got it — starting a new User Agent registration. We'll collect:
  1. Name
  2. Profile photo (optional)
```

The preview is **declarative**, not imperative — it describes what's next but does NOT ask for all three at once. See `playbooks/README.md §STRICT — Preview ≠ multi-field ask`. Immediately follow the preview with a blank line and the first question — rendered in natural language with **no `Q1：` / `Q1:` prefix** (see SKILL.md Red line 3 and `core/ux-lexicon.md`).

## Standard Q&A chain

> ⛔ Fields from user's literal reply only — never pre-fill from userEmail, wallet name, or session metadata. Anti-pattern to avoid: "Jim's User Agent" / "Alice's User Agent". Full rules: SKILL.md Red line 6.

The `Q1 / Q2 / Q3` column labels below are **maintainer-internal indexes**. The prompt strings in the Prompt column are the **literal text** rendered to the user — they carry **no `Q1：` / `Q1:` prefix**. Each prompt inlines the four-segment field spec from `core/field-specs.md` in the user's language only. If §One-shot capture already captured a field, **silently skip that Q** and move to the next.

| Q | Prompt | Validation | On failure |
|---|---|---|---|
| Q1 | `What's the name of this User Agent?` + 4 segments | non-empty, ≤ 64 chars (Chinese input: ≤ 30 characters) | re-ask once with a shorter example |
| Q2 | `Profile photo? Use the default or upload one?` + Choice prompt (see `modules/avatar-upload.md`) | — | skip → backend default photo |

> **Description — do NOT prompt, do NOT show in confirmation card when absent.** For User Agent, skip the description question entirely. If the user volunteers a description in the same message as the name (one-shot capture), accept and record it AND include a `Description` row in the confirmation card for that run; if not volunteered, omit the `Description` row from the confirmation card entirely (do NOT render "(not set)") and send `ProfileDescription: ""` on-chain silently. Do NOT ask "description (optional)" in any turn.

No service questions. No staking. (Signing address is never asked — the CLI always uses the current wallet's selected XLayer address; `--address` does not exist.)

## Good / bad cases

| User input | Action |
|---|---|
| "Named Alice" | Record `name=Alice`; next turn asks for profile photo. |
| "Write me a description" | Decline — User Agent description is not prompted for and is optional. Tell the user it will be left blank by default; if they want to add one, they can include it now in the same message. Do NOT offer example wording or guidance on what to write. |
| "I want a User Agent named Alice, focused on DeFi research" | Capture `name=Alice` + `description=DeFi research` in one turn. Still render the confirmation table with each field on its own row. |
| "Add a 5 USDT service" | Explain: User Agent identities don't include services. To charge for services, register an ASP instead. Do NOT add service to a requester create. |

## Confirmation

> ⛔ Mandatory before invoking the CLI. See the mandatory confirmation gate in SKILL.md — that section enumerates the rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") that do **NOT** bypass it.

Show the two-column table (`core/display-formats.md` §Create/Update Diff → Create variant) in the user's language. Render ONE variant — never bilingual.

Variant (user did NOT volunteer a description — omit Description row):

| Field | Value |
|---|---|
| Role | User Agent |
| Name | Alice |
| Profile photo | default |
| Estimated cost | **0 USDT** (creating/editing/activating/deactivating costs no transaction fees — OKX covers them) |

> Reply "execute" to run it.

Variant (user volunteered a description via one-shot capture — include Description row):

| Field | Value |
|---|---|
| Role | User Agent |
| Name | Alice |
| Description | Independent DeFi researcher, frequently purchases data services. |
| Profile photo | default |
| Estimated cost | **0 USDT** (creating/editing/activating/deactivating costs no transaction fees — OKX covers them) |

> Reply "execute" to run it.

**Do NOT show the bash command** unless the user explicitly asks ("show me the CLI"). Confirmation cards are field-only.

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role requester \
  --name "<name>" \
  [--description "<description>"] \
  [--picture "<url>"]
```

## ⛔ Post-success — MANDATORY template (do NOT paraphrase)

> ⛔ **After the visible line, this turn is NOT over.** → proceed to SKILL.md §Operation Flow Step 5 (which routes to `§Step 6` for the unconditional comm-init handoff). Full rules (anti-skip clauses, runtime self-gating, decline carve-out) live in Step 6 — not duplicated here.

Render **one visible line** using the template below — **verbatim except for the `#<id>` substitution rule**. Then follow the §Agent directive block (internal — not rendered to the user). Paraphrasing, adding fields, omitting fields, adding follow-up questions, or summarizing the CLI's other JSON output are all violations of the mandatory post-execute gate in SKILL.md.

### Visible line (template)

Render **one line, declarative, no question mark, no pre-announcement of the chat handoff** (the communication-init flow owns any runtime-specific messages or prompts):

`User Agent identity #<id> is live — say "publish a task for X" whenever you're ready and I'll take you through it.`

**`#<id>` substitution rule** (per `core/display-formats.md` top, `#<id>` placeholder rule, **requester-specific constraints**):

- Requester is **unique per address** (product invariant — see `playbooks/README.md §Pre-check`). If we got here successfully, the pre-check `agent get` lookup by definition returned **zero requesters** under this address — otherwise the pre-check gate would have stopped the flow and redirected to `update`. **Therefore the pre-check list contains NO same-role agent for this create**; any agent ids in that list belong to *other* roles (provider, evaluator) and MUST NOT be used as `#<id>` here.
- The legitimate sources of `#<id>` for this post-success line are, in priority order:
  1. **CLI response (direct):** the `create` call's response directly contains the new agent id.
  2. **Post-create envelope diff:** follow the two-step algorithm in `core/cli-create.md §1` "Finding the newly-minted agentId". For requester: pre-check returned 0 requesters by construction, so the lone newly-appeared requester-role row in the current wallet's wrapper IS the new id. ❌ Do NOT write the filter as `agentList[*].ownerAddress == ...` — agent rows have no `ownerAddress` field.
  3. (Future) a follow-up `agent get` in a later turn — irrelevant for this immediate response.
- If **both** source 1 and source 2 miss — i.e. CLI returned `txHash` only **AND** the post-create `agentList` segment is absent (WS + HTTP both failed, per `core/cli-create.md §1`) **OR** the diff yielded no new candidate under the current wallet — → **omit the `#<id> ` substring entirely** — do NOT render `#`, `#<id>`, `# ?`, do NOT invent a number, and do NOT borrow an id from the pre-check list. Fallback line:
  - `User Agent identity is live — say "publish a task for X" whenever you're ready and I'll take you through it.`

Do NOT mention the `okx-agent-chat/ensure-okx-a2a-communication-ready.md` path to the user in the visible line — the same-turn handoff below loads that skill's own prompt, which decides on its own whether to surface anything for the detected runtime.

### ❌ Anti-pattern → ✅ Correct

❌ Agent paraphrased:
> "✅ User Agent identity successfully on-chain! agentId is #42, block hash 0xabc...def. You can now go to okx-agent-task to find providers. Want me to show you some recommended providers?"

Why this is a violation of the mandatory post-execute gate in SKILL.md + `§UX Output Red Lines`:

- Adds `txHash` to the user-visible line — not in the template (txHash lives in the detail card if rendered, not the suggestion line).
- Adds a follow-up question — turns a declarative line into a question. The same-turn handoff to `ensure-okx-a2a-communication-ready.md` does not wait for a reply; a trailing question creates a stuck prompt.
- Adds reassurance phrasing not in the template — paraphrasing.
- Leaks the literal skill name `okx-agent-task` to the user (`§UX Red Line 1` — never expose `okx-*` skill identifiers).

✅ Correct (with id):
> User Agent identity #42 is live — say "publish a task for X" whenever you're ready and I'll take you through it.

✅ Correct (id unknown, txHash-only return):
> User Agent identity is live — say "publish a task for X" whenever you're ready and I'll take you through it.

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. → proceed to SKILL.md §Operation Flow Step 5 — the requester row routes directly to `§Step 6` (comm-init), which loads `/skills/okx-agent-chat/ensure-okx-a2a-communication-ready.md` Execution Flow in the same response.

Skip / decline carve-outs and the runtime self-gating contract are owned by Step 6. The **passive onboarding** path is filtered out at Step 5 (different row — see `§Passive Onboarding After success` below).

**Do NOT** chase with `agent get` / status poll (that is about querying chain state — different from the Step 5 → Step 6 chain above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Passive Onboarding — entry from `okx-agent-task`

When `okx-agent-task` hands control over with context `intent=need-requester` (the user tried to publish a task but has no User-Agent identity yet), enter the **simplified requester sub-flow**.

### Simplified sub-flow

Skip these normally-required steps:

- Do **not** ask for `--role` — it's fixed as `requester`.
- Do **not** pre-check existing agents — the handoff already implied none exist.
- Do **not** ask for `picture` — use backend default.
- Do **not** render the Phase preview — passive mode is deliberately lean, go straight to Q1 (see the passive onboarding section below).

Keep these:

- Ask `name` first (in natural language, no `Q1：` prefix — same Standard Q&A chain wording).
- Ask `description` second (same — no `Q2：` prefix).
- Show confirmation table (still field-per-row, still mandatory).
- Execute.

### After success

Return control to the caller. The response to the user is **only one line** in the user's language — **no detail card** in passive mode (the user just confirmed all fields a turn ago; rendering the full detail card would be redundant and would break the lean handoff to `okx-agent-task`). Follow the `#<id>` placeholder rule.

Canonical wording — this section is the single source of truth. Do NOT drift this wording — update here and propagate to SKILL.md §Passive Onboarding.

With id available:
- "User Agent identity #<id> created. Resuming the task-publish flow."

Without id:
- "User Agent identity created. Resuming the task-publish flow."

Do NOT ask "want to publish a task?" — the task skill already has the pending intent; it will resume.

Do NOT load `/skills/okx-agent-chat/ensure-okx-a2a-communication-ready.md` here — passive mode is contracted to hand strictly back to `okx-agent-task` with the single line above (see the passive onboarding section below "No other chatter"). The task skill triggers communication initialization itself when its flow needs it.

> **Why no detail card here?** Normal (non-passive) requester onboarding renders a detail card after success per `§Post-success`; passive mode deliberately omits it because (a) the user just saw all fields on the confirmation card one turn earlier, (b) the goal of passive mode is the leanest possible handoff back to `okx-agent-task`, and (c) detail-card-only fields the user might want later (full address, txHash) are always retrievable via `agent get --agent-ids <id>`. If you want to change this, change it in the passive onboarding messages below and `SKILL.md §Passive Onboarding` together.

### When user already has a requester

If a pre-existing requester agent happens to be found (e.g., the user returns mid-flow), **skip create** (requester is unique per address — see `playbooks/README.md §Pre-check`). Echo:
- "You already have a User Agent identity #<N> (<name>) — using it to continue publishing the task."

Hand back.

### Edge cases (passive mode)

| Situation | Action |
|---|---|
| User asks to cancel mid-flow | Confirm cancellation: "Registration cancelled. Publishing tasks requires a User Agent identity — come back when you're ready." |
| User volunteers a service mid-flow | Explain: User Agent identities don't include services. To charge for services, register an ASP identity later. Do NOT mix services into the passive sub-flow. |
| Backend rejects create | Render the error card (`core/display-formats.md §7`). Do NOT auto-retry. |
