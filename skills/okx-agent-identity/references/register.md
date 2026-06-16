# Register flow — create (all 3 roles) · consent · QA · avatar · update

Loaded when: the user registers / creates an agent (any role), or arrives via passive need-requester. Pairs with SKILL.md. (For update / fix-rejected-listing → load `references/update.md` instead.)

The CLI does the work — `validate-listing` returns the QA `findings[]`, `create` always returns `newAgentId` — a string id when the WS push succeeded, `null` when it timed out. You collect fields → render the §Invariants card → confirm → invoke once → render the post-success template. Never re-implement a rule table or reconstruct an id.

---

## 1. Role ask (do FIRST — `--role` is required by pre-check)

`agent pre-check` **requires** `--role`. If the role is clear, use it; otherwise ask once (accept a number or role name: 1 User Agent / 2 Agent Service Provider / 3 Evaluator Agent; never default or guess). Then run §2.

## 2. Pre-check (Gate — `agent pre-check --role <role> [--consent-key <uuid>]`: consent + uniqueness in ONE command)

Run `agent pre-check --role <role>` (internal — never shown). It fetches the wallet's agents; **if the wallet has agents it's already consented** (→ straight to the uniqueness verdict); **if it has none it runs the consent gate first**. It always returns `{ canCreate, role, reason?, consent?, existingSameRole, providerCount }` — **never call `agent get` / `agent consent` yourself for registration**. Branch on the result:

- **`consent` present** (always `canCreate:false`) → first-time wallet. Show `consent.terms` complete and translated (never summarized; never show `consentKey`). Present `1. Agree & continue` / `2. Decline & cancel`. `1` → re-run `agent pre-check --role <role> --consent-key <uuid>`; `2` → stop. Ambiguous → re-display once.
- **`canCreate:false`** (no `consent` field — a single-role identity already exists; `reason` explains) → do NOT create, do NOT offer "create new". Redirect to update with the mandatory per-wallet line, filling `<roleLabel>` / `<N>` / `<name>` from `existingSameRole[0]`:
  > "Under this wallet you already have a `<roleLabel>` identity #`<N>` (`<name>`). Each address can register only one `<roleLabel>` — say "update #`<N>`" to edit it, or keep using it. To register a separate one under a different address, switch / add a wallet first."
- **`canCreate:true`** → may register. Provider with existing ASPs (K ≥ 1): K=1 → offer *1. New ASP / 2. Update #`<N>` (`<name>`)*; K ≥ 2 → list from `existingSameRole` by number (never auto-pick). If the user mentions fixing a rejected listing → steer to option 2 + §11 rule (only create if user explicitly insists). K=0 / requester/evaluator → §3.
- Proceed to the §3 field Q&A and eventually `create` — the CLI always returns `newAgentId` (string id on WS success, `null` on timeout).

**Passive need-requester** (handed in from a task flow): skip the pre-check loop / photo entirely. See §8.

## 3. Field checklists (one line per field — limits are enforced by `validate-listing`, not by you)

**requester / evaluator:**
- **Name** — required, from the user's literal reply this turn only (never from email / wallet name — §Fields-from-user).
- **Profile photo** — optional; default if skipped (see §5).
- **Description** — do NOT prompt. If the user volunteers one, add a Description row to the card; otherwise omit the row and send `ProfileDescription:""` silently.

**provider — two steps** (user may batch):
- **Step 1 · Identity** — Present all three as a **single numbered list in one message** (do NOT split into separate turns):
  1. **Name** — brand name (CN 2–12 chars / EN 3–25 chars; ❌ test markers / celebrity names)
  2. **Description** — one-sentence summary of what the Agent does (required, ≤500 chars)
  3. **Avatar** — inline sub-choices inside item 3 (see §5 for wording); skip → keep default
- **Step 2 · Service** — Service name (5–30 noun phrase; ❌ same as agent name / price in name) · Description (3 parts: summary / capabilities / 1–3 prompts) · Type (API service → pass `A2MCP` / agent-to-agent → pass `A2A`) · Fee — a **plain number sent as a string** (e.g. `"10"` — quoted in the JSON, never a bare number); **USDT is the default and only currency, so do NOT include any currency** (no `USDT`/`USDG`/`元`/symbol); API service required, A2A optional (may be left empty); ≤6 decimals; reject `10 USDT` / `approx 10` / `5元` → re-ask. Displayed back to the user as `N USDT`. · Endpoint (API service only — §6).
- **After EACH service (MANDATORY — incl. the first)** — ask once (localized) **1. Add another service / 2. Done**; on **1** repeat Step 2 and append to the service array, then ask again; on **2** (or other) → §4 with the complete array. Never auto-advance on the assumption one is enough; all services ship in one `agent create` (post-create "add a service" via update is a fallback, not a reason to skip this).
- **Do NOT run `validate-listing` inside this loop.** QA is a single batch pass that happens in §4 *after* the array is complete — never validate per service, never validate while still collecting.

## 4. QA via `validate-listing` (provider only — requester/evaluator skip) — runs EXACTLY ONCE

Validate is a **single batch gate**, NOT a per-service step. Collect the **complete** identity (Step 1) **and the full service array** (every service, via the §3 Step-2 add-another loop) BEFORE you call it. One registration = one `validate-listing` call. Numbered steps:

1. **Call once, on the full set.** Only after the user picks *Done* in §3 Step 2, run `validate-listing --role provider --name … --description … --service '[… all collected services …]'` a single time. Returns `{ pass, findings[{field, code, severity:"block", issue, fix}] }`. `field` uses dot-notation (e.g. `service[0].fee`, `service[1].name`).
2. **Render the findings card — as suggestions only.** `pass:true` / empty `findings[]` → say it passed and go straight to §7. Otherwise render each finding inline on its field row as ` ⚠️ <issue> → <fix>`, mapping by the dotted `finding.field` to its card row (`service[0].fee` → Service [1]'s Fee row, `service[1].*` → Service [2]'s rows, `name` → the identity Name row). Surface a `(test)` marker on the name row if present. Also fold in the semantic checks from step 4 (same list). **At this point the `<fix>` text is only a recommendation on display — the field values are unchanged; do NOT apply any `fix` yet.**
3. **Confirmation is mandatory — never apply a suggestion before the user chooses.** After showing the card, ask once how to proceed — exactly TWO numbered choices (localized). Do NOT re-run `validate-listing`:
   > 1. Apply the suggested fixes — I'll update the flagged field(s) with the fixes shown above, then redraw the card for you to review.
   > 2. I'll revise it myself — tell me the new value(s).
   - On **1**: this choice **is** the user's confirmation for the whole batch of suggestions. Only now apply each shown `finding.fix` to its mapped field (plus your own semantic fixes), then redraw the card with the corrected values. Apply **once** — do not iterate.
   - On **2**: collect the user's replacement value(s) for the flagged field(s) and redraw the card.
   Either way, the corrected values still flow into the §7 confirmation card — **nothing is written on-chain until the user confirms there (Reply 1)**. **`validate-listing` has already run its single pass — never call it again** (the CLI re-runs QA internally at `activate`). Never apply a `fix` before the user picks; never silently auto-correct; never force a fix.
4. **Semantic checks the CLI cannot do** (fold into step 2's list, flag alongside the CLI findings): Is the service name a descriptive noun-phrase — not just a letter like "Q"? Is the agent name a brand, not a personal label (Alice, Account2) or a celebrity name (Trump / Musk / CZ / 马斯克 / 马云)? Does the description avoid leaking tech-stack / infra names or legal disclaimers?

## 5. Avatar (inline — image links are rejected)

- **Image links are not accepted.** If the user supplies a URL, reject it — do NOT pass it to `--picture`, do NOT download-and-reupload, do NOT claim it was set:
  > "Avatar links aren't supported — send an image file directly, or keep the default."
- **Avatar appears as item 3 in the Step 1 numbered list** — render as a single optional line (no sub-choices):
  > 3. Avatar — 📷 Optional. Send an image file to set a custom avatar; skip to keep the default.

  User sends an image file → upload it; no image / skips → keep default. Never ask the user to pick 1/2.
- **On opt-in:** Claude Code → save the inbound image attachment to a temp path → run the `upload` subcommand (`agent upload --file <temp>`) → use the returned URL as `--picture` (this temp write is the one allowed by SKILL §Gates One-call rule); >1 MB → stop and ask for a smaller one; render the URL verbatim in the Profile photo row. No image supplied → keep the default. 1:1 square is the tip.
- **Upload as-is — never resize/crop/convert.** >1 MB → ask for a smaller file; non-1:1 → accept and upload (square is advisory); non-PNG/JPEG/WebP → ask to convert and resend.

## 6. Endpoint anti-pattern (provider API service)

Require `https://`, publicly reachable, and really deployed. **Reject** `http://`, `localhost`, `127.0.0.1`, RFC-1918 private IPs (`192.168.*` / `10.*` / `172.16–31.*`), `*.local` / `*.internal`, mock URLs, and placeholders. Never suggest any of those as acceptable. Explain a publicly-reachable `https://` URL is required and is permanent on-chain (changing it later needs another update). If the user has no deployed endpoint yet: deploy first, or switch to agent to agent.

**Length guard** — endpoint URL must be ≤512 chars; if longer → "The endpoint URL must be at most 512 chars; this one is longer. Use a shorter URL." Re-ask.

## 7. Confirmation card (§Invariants card skeleton; never redraw the markup)

requester / evaluator render ONE card. **Providers render TWO** cards in order:

1. **Identity card** (closes Step 1) — Role / Name / [Description] / Profile photo rows, with the avatar CTA at its close. This card closes with **`> Reply **1** to continue.`** (NOT the confirm-run footer). Confirming it (**1**) **advances to Step 2 and does NOT call the CLI** — no `agent create` runs at Step 1.
2. **Service card** (closes Step 2) — render ONE block of `Service [N] Name / Description / Type / Fee / Endpoint` rows **per collected service** (`Service [1]`, `Service [2]`, … — never assume a single service); gloss service types once (wording per SKILL §Invariants Lexicon). This is the FINAL card → it carries the confirm-run footer; **1** runs the single `agent create` (carrying the identity plus ALL collected services).

The FINAL card ends with `> Reply **1** to confirm and run.` (localized) + the gate echo: `I won't run anything until you reply **1**.` NL field questions only; no `Q1:` labels, no bash shown.

## 8. Passive need-requester

Run `agent pre-check --role requester` (consent + uniqueness gate, same as §2). On consent required → run full consent flow per §2. On `canCreate:false` (requester already exists) → use the existing one, skip create entirely. On `canCreate:true` → ask name only (skip photo). Then render the card → on confirm, execute. Post-success is ONE line, **no detail card, no Step 6**:
> "User Agent identity #`<id>` created. Resuming the task-publish flow."

(If a requester already exists: "You already have a User Agent identity #`<N>` (`<name>`) — using it to continue.") Hand back to the task flow with that single line; don't ask "want to publish a task?".

## 9. Execute

Run `agent create` with the collected fields (role/name/description/picture/service — all from §3). **On any non-success** → load `references/errors.md`; never interpret a code inline.

## 10. Post-success templates (verbatim except `#<id>`; localized; `#<id>` per SKILL §Invariants #id ladder — `newAgentId` primary)

- **requester (ONE line)** → then Step 6 silent. No txHash, no question.
  > User Agent identity #`<id>` is live — say "publish a task for X" whenever you're ready and I'll take you through it.
- **provider (ONE line)** → then Step 6 silent. Never mention active clients / agent counts / re-list agents; never a numbered menu; never a duplicate line.
  > ASP identity #`<id>` registered — not yet visible to others. Say "activate #`<id>`" to publish now, "add a service to #`<id>`" to offer more services, or "find ASPs doing X" to check the market first.
- **evaluator (EXACTLY two lines)** — no stake number/amount, no trailing question, no detail card → proceed toward the staking handoff.
  > Evaluator Agent identity #`<id>` registered.
  > A separate stake is still required before you can be assigned disputes.

  (Staking is post-create, never a pre-create gate; "don't want to stake" → register now, stake later; "have I staked?" → hand to staking flow.)

If `#<id>` ladder yields nothing: requester/evaluator → omit `#<id>` entirely; provider → `Say "list my agents" to find your new identity, then "activate #<id>" to publish.`

---

## 11. UPDATE flow

See [`references/update.md`](update.md) — ownership check, QA, diff card, wholesale service replacement, post-update messages, and rejected-listing remediation rule.
