# Role Playbook — Router + Shared Rules

> Entry point for `agent create`. This file is intentionally thin: it routes to the three role-specific files and spells out the rules that apply to all of them. Read the matching role file for the full Q&A chain.

## Route to the right role file

| User intent | Read |
|---|---|
| register user / user identity / buyer / requester / User Agent / system requires identity for task | `playbooks/requester.md` (includes Passive Onboarding sub-flow) |
| register service provider / ASP / Agent Service Provider / seller / provider / list a service | `playbooks/provider.md` |
| register evaluator / arbitrator / Evaluator Agent / want to arbitrate | `playbooks/evaluator.md` (create-first; staking happens separately via `okx-agent-task`) |
| Incoming context `intent=need-requester` | `playbooks/requester.md §Passive Onboarding` → `playbooks/requester.md` |

If the user said "register an agent" without specifying a role, ask the three-option question first using the **numbered-options pattern** (per the numbered-options pattern + `§Core Flow` gate 1) — never the prose `A / B / C` form, which is banned by the choice-prompt rule.

```
Which identity do you want to register?
  1. User Agent — publishes tasks, pays for services
  2. Agent Service Provider (ASP) — offers services, delivers work
  3. Evaluator Agent — arbitrates task disputes
Reply with a number: 1/2/3.
```

Also accept a written role name as a fallback — be permissive on input (users may type any of the legacy or new terms): `User Agent` / `requester` / `buyer` (→ requester); `ASP` / `Agent Service Provider` / `provider` / `seller` (→ provider); `Evaluator Agent` / `evaluator` / `arbitrator` (→ evaluator). The primary ask is numeric. CLI accepts `1`/`2`/`3` as `--role` aliases (`utils.rs:162-165`), so the numeric reply can pass straight through. The skill's own output uses the canonical localized term per `core/ux-lexicon.md §Role`.

Do NOT default. Do NOT guess from the name / description fields.

## Field prompts

All eight fields (Name / Description / Profile photo / `name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) have standardized four-segment specs — **Purpose / Visibility / Please note / Example**. See `core/field-specs.md`. When you ask the user a field, inline all four segments with the question in the user's language only (never mix languages in one message).

## STRICT — one question per turn

Applies to every role flow. Applies to every service sub-field. No exceptions.

- Never list "Please provide 1. Name 2. Description 3. ..." in one message as an **imperative ask**.
- Never enumerate more than one field per turn in an **asking** message.
- If the user volunteered multiple values in one sentence ("named Alice, focused on DeFi"), you may capture them at parse time (see `core/choice-prompts.md §One-Shot Capture`) — but the confirmation card still renders one row per field, and any still-unanswered fields are still asked one at a time.
- The rationale is not just UX; users answer one question more accurately than a list. List format causes dropped fields and typos that force re-prompting, which is worse than the extra turns.

### Preview ≠ multi-field ask

Showing a **declarative preview** at the start of each phase ("Next we'll collect: Name, Description, Profile photo (optional).") is **allowed and encouraged** — it sets expectations and lets users decide whether to one-shot. Previews are statements, not asks; they are always followed by a single field question, **asked in natural language — no `Q1：` / `Q1:` prefix** in the user-visible prompt (see `SKILL.md §UX Output Red Lines Red line 3` and `core/ux-lexicon.md` flow-term table).

The distinction is verb mood:

- ❌ Banned (imperative, multi-field): "Please provide: 1. Name 2. Description 3. Profile photo"
- ❌ Banned (declarative preamble + Q-prefix leak): "Next we'll collect: …\n\n**Q1: What's the name of this ASP?**" — the `Q1:` prefix leaks an internal label into chat output (Red line 3).
- ✅ Allowed (declarative preamble + single natural-language question): "Next we'll collect: Name, Description, Profile photo (optional).\n\nWhat's the name of this ASP?" — use `ASP` per `core/ux-lexicon.md §Role` (the raw ERC-8004 enum `provider` is wire-only).

If in doubt: the preamble describes what will happen; the Q asks for exactly one thing.

## Pre-check existing agents (normal onboarding only)

Before entering any role flow triggered by the user's own initiative, run `agent get` **once** to see if they already have an agent of the requested role.

**Each address can have at most one User Agent (wire-level `requester`) and at most one Evaluator Agent (wire-level `evaluator`)** (product constraint, backend enforces). Agent Service Provider (wire-level `provider`) is unlimited — the same address can have multiple ASP identities. Pre-check branches by role:

> ⚠️ **Display scope vs. uniqueness scope differ (double-envelope artifact — read carefully).**
>
> `agent get` default list mode returns a **double-layer envelope** (`core/cli-reference.md §3`): `list[*]` is a wrapper grouped by accountName (one email / JWT caller may have multiple derived wallets, each with one wrapper), actual agent rows live at `list[*].agentList[*]`. The two scopes must be handled separately:
>
> - **Display**: list **all** wrappers (render per `core/display-formats.md §1` "one header per accountName + agent table for that wallet"). Do NOT deduplicate or merge across wrappers — the user should see the full wallet–agent topology.
> - **Uniqueness check + K count**: **scoped only to the wrapper matching the currently selected XLayer wallet** — lock to `wrapper.ownerAddress == <current selected XLayer wallet address>` and check for same-role agents within that agentList only. Same-role agents in other wrappers belong to different derived wallets and do NOT count as "already registered" — because the product's "requester / evaluator are unique" constraint is **per address**, not per email.
>
> This is also the alignment point for §Pre-check Q1 self-verification: running `agent get` is not the same as completing pre-check — the result must be filtered by the current wallet's ownerAddress before drawing any conclusion.

### requester / evaluator (unique per address)

If a same-role agent already exists, **do NOT** offer a "create new" option and do NOT enter the create flow. Inform the user directly and point to update:

> "**Under this wallet** you already have a <role> identity #<N> (<name>). Each address can only register one <role> — say "update #<N>" if you want to edit description / picture, or just keep using this one. (If you want a separate <role> under a different address, switch / add a wallet first — say "switch wallet" or "add wallet" to do that.)"

`<role>` renders as the user-facing label (`User Agent` / `Evaluator Agent`) — never the raw ERC-8004 enum (`requester` / `evaluator`). The "Under this wallet" qualifier is mandatory — uniqueness is per-address, not per-email; agents under other linked wallets are separate. ⛔ **Never expose the skill name `okx-agentic-wallet` in user-visible text** (Red line 1) — the AI routes internally when the user says "switch wallet" or "add wallet".

If the user insists on creating another, restate the product constraint — do not work around it (the backend will reject).

### provider (multiple per address allowed)

Both paths are allowed. Use numbered-options (see the numbered-options pattern). **K is counted only within the wrapper matching the currently selected XLayer wallet** (see ⚠️ callout above) — ASP identities under other wrappers belong to other linked wallets and are excluded from K and from the candidate list. **When K ≥ 1 in the current wallet wrapper, list all existing ASP identities** (the user needs to see them all to decide whether to create a new one or update an existing one). ⛔ **Never use raw wire-level identifiers (`provider`) or legacy nouns in user-visible text** (Red line 4 + `core/ux-lexicon.md §Role`):

(K = 1):
```
Under this wallet you already have 1 ASP identity: #<N1> (<name1>). What would you like to do?
  1. Register a new ASP (multiple ASPs per address are allowed)
  2. Update #<N1> (description / picture / services)
Reply 1 or 2.
```

(K ≥ 2, list them all):
```
Under this wallet you already have K ASP identities: #<N1> (<name1>), #<N2> (<name2>), …, #<NK> (<nameK>). What would you like to do?
  1. Register a new ASP (multiple ASPs per address are allowed)
  2. Update one of them
Reply 1 or 2.
```

If the user picks 2 and K ≥ 2, ask a follow-up numbered question: "Which one? Reply with a number: 1 (#<N1>) / 2 (#<N2>) / … / K (#<NK>)."

Do not auto-choose for provider. Don't silently default. **Do not collapse the K ≥ 2 case to "one of them" without listing the ids** — the user must see the full list to make an informed pick (and to notice if they have stale providers they forgot about).

**The "Under this wallet" qualifier is mandatory and must not be dropped.** Reason: the `agent get` list display shows **all** wrappers (cross-wallet, by accountName), while K is counted **only** on the wrapper matching the currently selected XLayer wallet address. If the user already saw N agents in the display but the prompt says K < N, the qualifier reconciles the apparent contradiction. Same logic applies to the requester / evaluator uniqueness messages above.

### Language

The prompt **must match the user's language**. Follow .

**Skip this pre-check entirely for passive onboarding** (`intent=need-requester`) — see `playbooks/requester.md §Passive Onboarding`.

## Confirmation card

> ⛔ The card is **mandatory before every content-creating on-chain write** — `agent create` / `update` / `feedback-submit`. This is enforced by the mandatory confirmation gate in SKILL.md; that section is the canonical source. Memory preferences, plan-mode exit, one-shot capture, urgency, and "intent is obvious" all do **NOT** bypass it — see the rationalization list in `SKILL.md §Core Flow` gate 4. State toggles (`agent activate` / `agent deactivate`) are NOT gated and run directly via `SKILL.md §Intent → Sub-flow`.

Always a table of fields — never a bash blob. Match the user's language. Render field labels and row values in one language only. ⛔ The `role` row MUST follow `core/ux-lexicon.md §Role`: `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. **Never render the raw ERC-8004 enum (`requester / provider / evaluator`) or legacy nouns; never render bilingual parentheticals** (`SKILL.md §UX Output Red Lines Red line 4`). See `core/display-formats.md` §Create/Update Diff for the full template.

| Field | Value |
|---|---|
| Role | Agent Service Provider (ASP) |
| Name | <...> |
| Description | <...> |
| Profile photo | default — if the user uploaded an image or supplied a link, render the **actual URL verbatim** here (e.g. `https://…/abc.png`). Never write "uploaded" / mention "CDN" as a placeholder. |
| Service [1] Name / Description / Type / Fee / Endpoint | (ASP only) |

End with: `Reply "execute" to run it.` Do not promise a verb the model could echo as pre-execution chatter — see `SKILL.md §Step 3 — No narration between confirmation and result`.

**The bash `onchainos agent create ...` command is NOT shown in the confirmation card.** Show it only if the user explicitly asks "show me the CLI".

## Execute

> Before invoking the CLI, run the **3-question pre-execute self-check** defined in `SKILL.md §Step 3: Execute` — externalize your answers (pre-check ran? confirm token in latest turn? card values byte-equal to CLI values?). **If any answer ≠ yes, render the confirmation card and wait — do NOT call the tool.** The canonical wording, command-specific reinterpretations, and full remediation table all live in `SKILL.md §Step 3`; do not maintain a parallel summary here.

After the user replies "execute" / "yes" / equivalent:

1. Run the CLI command once.
2. On success → read `core/display-detail.md §2` + the role-specific `§Post-success` in the matching playbook before rendering anything. Render the detail card + the role-specific next-step line (see each role file). **Exception — passive onboarding** (`intent=need-requester`): render **only one line** and **no detail card** per the passive onboarding section in `playbooks/requester.md` + `playbooks/requester.md §Passive Onboarding`. **For the list-mutating writes** (`requester` / `provider` / `evaluator` create, `update`, plus `activate` / `deactivate`), control then flows into `SKILL.md §Operation Flow Step 5` (dispatcher) → `§Step 6` (comm-init) in the same response. The Step 6 invocation is **unconditional from this skill's side** — runtime gating lives inside the callee's Step 0, not in this skill's pre-decision. Do not stop between visible line and Step 5. See each role file's §Post-success "Agent directive" block. **Passive onboarding lands in Step 5's "back to task" branch** — no Step 6.
3. On failure → render the error card (`core/display-formats.md` §Error card) + the recovery action (see `troubleshooting.md`). **Do NOT auto-retry.**

See  — do NOT follow up with `agent get` / status poll. The Step 5 → Step 6 same-turn chain is explicitly allowed (it is not polling).

## bash blocks in these files

Every `onchainos agent create ...` bash block inside `playbooks/requester.md` / `playbooks/provider.md` / `playbooks/evaluator.md` is labeled **maintainer reference — not shown to user**. They are there so developers can grep for the exact CLI shape and keep translations in sync. Your user-facing output is the confirmation card, not the bash.
