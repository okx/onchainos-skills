# Pre-Listing Quality Assurance

This file defines the quality gate the AI runs for `provider`-role agents. It operationalises the five display-field standards from the OKX marketplace listing specification. It fires in **three** situations: **before registering** a new provider (full agent, advisory — Trigger C), at listing time (full agent — Trigger A), and when editing an already-existing provider (changed fields only — Trigger B).

> **Why a pre-create trigger.** The hard gate is at listing (`activate`), not at `create`. Without Trigger C a user can register a provider whose name/description/services are doomed to be rejected at listing, then has to fix and resubmit. Trigger C pulls those findings forward to **before registration**, so the listing-blocking issues are caught while the user is still composing — no register→reject→fix→resubmit loop. It is **advisory**: the user may always register anyway.

## When to Run

Automatically trigger this checklist when **any** trigger matches:

**Trigger C — Pre-create (advisory, folded into the two confirmation cards).** Run inside the `agent create` provider flow, **as part of rendering** each confirmation card, when the role being created is `provider`. The provider create is a **two-step flow with two cards** (`playbooks/provider.md §Confirmation cards — two steps`); Trigger C runs in two scopes, one per card:
- **Identity card (Step 1):** agent `name` (N1–N8) and `description` (U1–U5 only — there is no agent-level D-rule). Avatar → L2/L3 advisory only.
- **Service card (Step 2):** every collected service (T1–T3 / S1–S6 / P1–P5 / D1–D11 + U rules).

This trigger is **advisory** — it never blocks `create`. Unlike Trigger A/B, it does **not** render a separate §QA Report: findings are surfaced **inline on the offending field's row** of the card that owns it (a ⚠️ + fix suggestion), and each card's edit/confirm affordance is the "fix now / register anyway" choice. Confirming the service card with warnings still present = register anyway. Purpose: surface listing blockers before they are minted on-chain, not to gate registration. See `playbooks/provider.md §Confirmation cards — two steps`.

**Trigger A — Pre-listing (full scope).** Run after `agent activate` returns `success: false, approvalStatus: 1` **or** `approvalStatus: 5`, and **before** calling `agent submit-approval`, when:
- `agent activate` returns `success: false` with `approvalStatus` of `1` **or** `5`, **AND**
- The target agent's `role` is `provider`

> `approvalStatus: 5` is handled exactly like `1` (resubmit). Do NOT surface the rejection or `rejectReason` — see `troubleshooting.md §2` and `core/cli-reference.md §4`.

Scope = **all** display fields of the agent (every check in the tables below, against every service).

**Trigger B — Provider edit (changed-fields scope).** Run inside the `§Update` flow (SKILL.md), after the user's changes are collected and **before** the Update Diff card is confirmed, when:
- The target agent's `role` is `provider`, **AND**
- The user is changing at least one QA-governed field: agent `name`, `description`, `picture`, or any service field (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`), incl. adding/removing a service. (For `picture`, only the format advisories L2/L3 apply — presence is never AI-checked; see §Logo.)

Scope = **only the fields the user is actually modifying**. Do NOT flag pre-existing issues in fields the user left untouched — those were already on-chain and re-surfacing them mid-edit is noise. Concretely:
- Changed agent `name` → run N1–N8 (incl. N4b public-figure list, N8 special-symbol) + U1–U3, U5 on the new name only.
- Changed `description` → run D1–D11 on the new description only.
- Changed/added service → run T1–T3 / S1–S6 / P1–P5 / D-rules + U1–U5 on **that** service only; leave sibling services alone. (Changing a service's `servicetype` also re-triggers U5 on that service's name + description, since the token-vs-type match may now break.)
- Changed `picture` (new upload) → L2/L3 advisory only (ratio / size); never L1 (presence) — see §Logo.
- A field the user did NOT touch → skip every check for it, even if it looks non-compliant.

If the role is `requester` or `evaluator`, skip this file under all three triggers — it does not apply (those roles have no service fields).

## How to Run

1. Source the values to check:
   - **Trigger A / B:** use the `agent get --agent-ids <N>` data already in context (do **NOT** make an extra CLI call just for QA).
   - **Trigger C:** the agent does **not** exist yet — use the **field values buffered during collection** (no `agent get`, no CLI call).
2. Determine scope by trigger:
   - **Trigger C (pre-create):** two scopes, one per card — **Identity card**: collected `name` (N1–N8) + `description` (U1–U5); **Service card**: all collected service entries (T/S/P/D + U). Avatar → L2/L3 advisory only.
   - **Trigger A (pre-listing):** extract top-level `name`, `description`, and all `services[]` entries. Check everything. For the avatar, run **only L2/L3 (format, advisory)** — never L1 (presence); see §Logo.
   - **Trigger B (provider edit):** extract **only the user's new/changed values** (the deltas you collected in the `§Update` flow). Check only those — ignore untouched fields entirely.
3. Run the relevant checks in the tables below against the in-scope values (per-service for any in-scope service).
4. **All checks pass** → proceed to the trigger's next step with no report: Trigger C → render that step's card (Step 1 identity card / Step 2 service card); Trigger A → `agent submit-approval`; Trigger B → render the Update Diff card as usual.
5. **Any check fails:**
   - **Trigger A / B** → render the §QA Report (two explicit options) and stop; wait for the user to choose.
   - **Trigger C** → do **not** render a separate report. Fold each finding inline onto the offending field's row of the card that owns it (Step 1 identity card / Step 2 service card) (⚠️ + fix) and let each card's edit/confirm affordance be the choice — see `playbooks/provider.md §Confirmation cards — two steps`. Confirming the service card unchanged = register anyway.
   - **Avatar presence is never a QA finding** (Trigger A, B, or C) — the AI does not check whether an avatar was uploaded; see §Logo. There is no L1 (presence) block. Avatar *format* (L2 ratio / L3 size) may appear as ⚠️ advisory only and never blocks; every trigger always lets the user proceed anyway.
   - **Trigger B** option 2 wording is "Submit the change anyway" (proceeds to the Update Diff card → `agent update`), not "list anyway".

---

## Universal Prohibitions (apply to all fields)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| U1 | No test / environment markers | Field contains any of the following patterns (case-insensitive): **parentheses / bracket forms** `(pre)` `(test)` `(dev)` `(beta)` `(alpha)` `(staging)` `(uat)` `(sandbox)` `[pre]` `[test]` `[dev]` `[beta]` `{pre}` `{test}`; **delimiter-suffix forms** `-pre` `-test` `-dev` `-beta` `-staging` `_pre` `_test` `_dev` `_beta` `_staging` `.pre` `.test`; **space-suffix forms** ` pre` ` test` ` dev` ` beta` ` staging` appearing at the **end** of the field value (trailing space marker). Matching is **case-insensitive** (`(PRE)`, `_Test`, `-DEV` all fail). | Remove the marker entirely — do not replace with another tag |
| U2 | No internal addresses | Any `0x…` wallet / owner / tx hash in name, description, or service fields | Remove the address |
| U3 | No negative capability statements | Contains `currently not supported` / `does not support` (or equivalent in any language) | Rewrite positively or remove |
| U4 | Free service must be explicit | A2MCP `fee` is empty/blank when the service is free | Set to `0 USDT` |
| U5 | Service-type token must match the actual type | Field text contains the literal token `A2A` or `A2MCP` that contradicts the service's `servicetype` — e.g. a service name / description says "A2A …" but the type is set to `A2MCP`, or the agent name advertises "A2MCP" while the relevant service is `A2A`. (Substring match is **token-aware**: only flag a standalone `A2A`/`A2MCP` token, not an unrelated word.) | Align the wording with the actual service type, or drop the token from the text |

---

## Field 1 — Agent Name (`name`)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| N1 | Length in range | CN: < 2 or > 12 characters; EN: < 3 or > 25 characters | Shorten or expand |
| N2 | No agent ID embedded | Contains `#123`, `_1083`, or any numeric agent ID | Remove the ID |
| N3 | No ordinal suffixes | Ends with bare digit, `_2`, `_v2`, `(2)`, or a language-native ordinal suffix (e.g. `No.3`, `#3`) | Remove the ordinal |
| N4 | No personal names or account labels | Contains personal name, email prefix, or wallet account label (e.g. `Account2`, `Jim`, `bob123`) | Remove the personal reference |
| N4b | No public-figure / celebrity names | Name contains any well-known public figure — the listing spec calls these out explicitly: **Trump / Donald Trump / Elon Musk / Steve Jobs / Justin Sun / CZ / Obama / Putin / Biden / Jeff Bezos / Mark Zuckerberg / Sam Altman / SBF / Michael Saylor / Warren Buffett**. The list is illustrative, not exhaustive — flag any other obvious public figure too. Matching is case-insensitive and applies to both the standalone name and substrings (e.g. `Elon's Trade Bot`, `CZ Signal`). | Remove the public-figure reference; pick a neutral product brand name |
| N5 | Brand name — not a sentence | Reads as a full verb + object sentence rather than a product brand | Rewrite as a short brand name |
| N6 | Bilingual separator | Bilingual name must use `NativeName · EnglishName` format (middle dot `·` + spaces) | Fix the separator |
| N7 | No test / environment markers in name | Name contains any U1 marker — e.g. `WeatherBot-test` / `MyAgent_dev` / `SentryX(beta)` / `AgentX staging`. This is the **#1 reported rejection reason for names** and must be checked explicitly even though U1 also covers it globally. Caution: `Predict` is NOT a violation (`pre` is embedded in a genuine word); only flag when the marker is delimited (parentheses / bracket / hyphen / underscore / trailing space). | Remove the marker; pick a clean brand name |
| N8 | Letters / CJK only — no decorative special symbols | Name should be Chinese characters or English letters only; flag punctuation / special symbols such as `!` `?` `@` `#` `$` `%` `*` `~` `/` `\` `|` `+` `=`, or a stray leading / trailing / standalone `-`. **Allowed exceptions (do NOT flag):** the bilingual middle dot `·` (the N6 separator) and a single internal brand hyphen joining two word parts (e.g. `Predict-Raven`). Spaces inside an English brand are fine. | Keep only letters / CJK characters; drop the decorative symbols |

**Good:** `Predict-Raven` / `灵镜 · ChainMirror` / `SentryX` / `WakeMeUp` / `PMAlpha`

**Bad:** `FitnessBot(pre)` / `WeatherHelper_test` / `MyAgent-dev` / `SentryX(beta)` / `Account2buyer` / `Elon Musk Bot` / `CZ Alpha` / `Trump Predictor` / `WakeMeUp!` (N8 — stray `!`) / `Price?Bot` (N8 — stray `?`)

---

## Field 2 — Service Type (`servicetype`)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| T1 | Enum values only | Value is not exactly `A2A` or `A2MCP` (case-sensitive) | Correct to `A2A` or `A2MCP` |
| T2 | A2MCP requires endpoint | `servicetype=A2MCP` but `endpoint` is empty or absent | Provide a valid public HTTPS endpoint |
| T3 | A2A does not use endpoint | `servicetype=A2A` but `endpoint` is non-empty | Remove the endpoint value |

**Reminder:** A2A = agent-to-agent natural-language interaction, no endpoint needed. A2MCP = API-interface service, endpoint is mandatory.

---

## Field 3 — Service Name (`name` inside each service object)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| S1 | Length 5–30 characters | < 5 or > 30 characters | Adjust |
| S2 | Noun phrase — not a sentence | Contains a full sentence with subject + verb | Rewrite as a short noun phrase |
| S3 | Not a duplicate of agent name | Service `name` is identical to the agent-level `name` | Write a distinct service name |
| S4 | No price in service name | Contains price info (`0.1 USDT`, `free`, or equivalent in any language) | Move pricing to the fee field |
| S5 | No technical implementation details | Mentions internal framework, API key, infra provider | Remove or abstract |
| S6 | No test / environment markers in service name | Service name contains any U1 marker — e.g. `WeatherQuery(pre)` / `AnalysisAPI_test` / `RecommendService-beta`. Apply the same delimiter-awareness as N7: `protest` is NOT a violation; only flag when the marker is clearly delimited. | Remove the marker; rewrite as a clean noun phrase |

**Good:** `Polymarket Daily Signal` / `On-chain Signature Analysis` / `Crypto Price Alert`

**Bad:** Same as agent name (duplication) / too vague (e.g. `General Query`) / too long + tech exposure / `Market Push(test)`

> **Also enforce U5 here:** if the service name contains the token `A2A` / `A2MCP`, it must match this service's actual type (e.g. `A2A Quote Bot` on a service whose type is `A2MCP` fails U5).

---

## Field 4 — Default Price (`fee`) — A2MCP required; A2A optional

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| P1 | Format: `{number} {currency}` — both segments required | Missing either segment | Complete both |
| P2 | Currency must be `USDT` or `USDG` | Any other currency symbol | Change to `USDT` or `USDG` |
| P3 | No negotiation language | Contains `TBD` / `negotiable` / `flexible` (or equivalent in any language) | Set a concrete price |
| P4 | No parenthetical notes | Contains any parenthetical after the price (e.g. `0.05 USDT (USDG accepted)`) | Remove the parenthetical |
| P5 | A2A fee format | A2A fee is optional; if provided, must still follow all format rules above | Leave empty or apply format |

**Good:** `0.1 USDT` / `0.5 USDG` / `0 USDT`

**Bad:** `0.2 USDT (complexity-based negotiation)` / `USDG` (missing number) / `0.05 USDT (USDG accepted)`

---

## Field 5 — Service Description (`servicedescription`)

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| D1 | Three-part structure required | Missing any of: ① summary / ② capabilities / ③ example prompts | Add the missing part |
| D2 | Total ≤ 400 characters | Total character count > 400 | Trim |
| D3 | Part 1 summary ≤ 50 characters | First paragraph > 50 characters | Shorten: "what it is + who it's for" in one sentence |
| D4 | Part 2 capabilities ≤ 150 characters | Second paragraph > 150 characters | Reduce to 3–5 key capability points separated by `;` |
| D5 | Part 3: 1–3 example prompts, each ≤ 80 characters | No prompts / > 3 prompts / any prompt > 80 characters | Adjust count and length |
| D6 | No external links or GitHub URLs | Contains `github.com` or any URL | Remove |
| D7 | No wallet/contract addresses | Contains `0x…` | Remove |
| D8 | No tech-stack exposure | Mentions internal framework names, model names, infra details | Abstract or remove |
| D9 | No negative statements | Contains `currently not supported` or equivalent in any language | Remove or rephrase |
| D10 | No legal disclaimers | Contains liability statements or legal caveats | Remove |
| D11 | Service-type token must match (U5) | Description text contains a standalone `A2A` / `A2MCP` token that contradicts the service's actual `servicetype` | Align the wording with the actual type, or drop the token |

**Good structure:**
```
① [≤50 chars] Prediction market research agent for Polymarket, delivering daily actionable betting signals.
② [≤150 chars] Scans active markets; combines market data, settlement rules, order book liquidity, and web search; outputs direction, AI probability, evidence chain, position sizing, and key risks.
③ [1–3 prompts, each ≤80 chars] 1. Recommend 3 Polymarket opportunities worth betting on now  2. Scan active markets for top 3 mispriced opportunities
```

---

## Logo — do NOT check whether an avatar was uploaded; format stays advisory

- ⛔ **No presence check (no L1).** Do NOT check whether an avatar was uploaded, do NOT block `agent activate` / `submit-approval` on a missing avatar, and do NOT raise "avatar not uploaded" as a QA finding.

The AI still gives **advisory** (⚠️, never blocking) guidance on avatar *format* when a picture is present or being uploaded:

| # | Rule | Failing pattern | Fix |
|---|------|----------------|-----|
| L2 | 1:1 aspect ratio | Non-square image | Suggest re-uploading a square image |
| L3 | < 1 MB | File too large | Compress and re-upload via `modules/avatar-upload.md` |

L2 / L3 are ⚠️ warnings only — best surfaced at upload time (they cannot always be verified post-upload); they never block listing. The upload-time 1 MB guard in `modules/avatar-upload.md` is the enforcement point for L3.

---

## §QA Report Format

When any check fails, render the report below in the user's language, then ask whether to fix first or proceed anyway. The header line and option 2 wording depend on the trigger.

**Trigger C (pre-create) — no separate report.** Findings are surfaced **inline on the card that owns the field** (Step 1 identity card / Step 2 service card): append ` ⚠️ <issue> → <suggestion>` to the offending field's value row (localized, user-facing labels only — never raw JSON keys). Each card's edit/confirm affordance carries the choice (edit a field → re-run the check → re-render; confirm the service card unchanged → register anyway). Full behavior: `playbooks/provider.md §Confirmation cards — two steps`.

**Trigger A (pre-listing):**
```
QA check found some issues before listing:

**Agent #<id> — <name>**

Service [N] "<service name>":
  ⚠️ <Field> — <issue> → <suggestion>

(repeat for each service with failures)

---
How would you like to proceed?
  1. Fix and list (Recommended)
  2. List anyway (⚠️ non-compliant information may cause listing failure)
```

**Trigger B (provider edit — only the changed fields appear):**
```
QA check found some issues with your changes:

**Agent #<id> — <name>**

  ⚠️ <Field> — <issue> → <suggestion>
(only fields the user is changing; repeat per changed field / service)

---
How would you like to proceed?
  1. Fix and submit (Recommended)
  2. Submit the change anyway (⚠️ non-compliant information may be rejected on review)
```

**Rules for the report:**

- Use ⚠️ (warning), not ❌ (error) — this is advisory, not a hard block.
- Group failures by service (service index + name); list all failing checks. For Trigger B, only list fields/services the user is actually changing.
- Use fix instructions from the tables above, translated to the user's language.
- ⛔ Do NOT show raw JSON, field key names (`servicedescription`, `servicetype`), or CLI flag names — use the user-facing labels from `core/ux-lexicon.md`.
- ⛔ Do NOT auto-correct values — the user must supply corrected content (Red line 6 in `SKILL.md`).
- **Trigger C — option 1 (fix now)**: re-collect the corrected value(s) one field per turn (focused step, with the relevant field guidance inline), re-run this checklist on the new value(s), then proceed to the create confirmation card. **Option 2 (register anyway)**: proceed directly to the create confirmation card → confirm → `agent create`, no re-prompting. Trigger C never calls a CLI itself — it only gates whether the confirmation card is rendered now or after a fix.
- **Trigger A — option 1 (fix first)**: route through `§Update` flow (`agent update` → re-run QA → `agent submit-approval`). **Option 2 (list anyway)**: invoke `agent submit-approval` immediately without re-prompting.
- **Trigger B — option 1 (fix first)**: re-collect the corrected value (one field per turn), re-run this checklist on the new value, then continue to the Update Diff card. **Option 2 (submit anyway)**: proceed directly to the Update Diff card → confirm → `agent update`, no re-prompting.

---

## Pass Message (all checks green)

No separate message needed — silently proceed to `agent submit-approval`. The post-submit line from `troubleshooting.md §2` is the only user-visible output.

If you want to surface the clean result (optional, e.g. when the user explicitly asked for a QA check without intending to submit right away):

- "QA passed — all fields meet listing requirements. Say the word and I'll submit for review."
