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
- **Step 1 · Identity** — Name (CN 2–12 / EN 3–25; brand name; ❌ test tags / celebrity) · Description (required ≤500) · Photo (optional §5).
- **Step 2 · Service** — Service name (5–30 noun phrase; ❌ same as agent name / price in name) · Description (3 parts: summary / capabilities / 1–3 prompts) · Type (API / A2A) · Fee (API: `N USDT/USDG` ≤6 dec; A2A: optional) · Endpoint (API only — §6).

## 4. QA via `validate-listing` (provider only — requester/evaluator skip)

The CLI is the QA engine; you render its `findings[]` and add ONE check it can't make. Numbered steps:

1. **Run at the Step-2 service card only** (not at Step-1). Pass the full set: `--role provider --name … --description … --service '[…]'`. Returns `{ pass, findings[{field, code, severity:"block", issue, fix}] }`. `field` uses dot-notation (e.g. `service[0].fee`).
2. **Render each finding inline on its field row** as ` ⚠️ <issue> → <fix>`, mapping by the dotted `finding.field` to its card row (`service[0].fee` → the Fee row, `name` → the Name row). Surface a `(test)` marker on the identity name row if the name carries one.
3. **Findings are warnings, not blocks. Do NOT hand-apply rule tables. Do NOT silently auto-correct.** When `findings[]` is non-empty (regardless of `pass`), after rendering the card present exactly TWO numbered choices (localized):
   > 1. Fix — re-collect only the flagged field(s), then re-run `validate-listing` once.
   > 2. Skip — advance to the confirmation card immediately; do NOT re-run `validate-listing` (saves one API call).
   On choice **1**: accept the corrected value(s), re-run once, then show the card again (findings or not). On choice **2**: proceed without re-running. Never loop automatically; never force a fix.
4. **After rendering the CLI findings, add the semantic checks the CLI cannot do.** Ask yourself: Is the service name a descriptive noun-phrase — not just a letter like "Q"? Is the agent name a brand, not a personal label (Alice, Account2) or a celebrity name (Trump / Musk / CZ / 马斯克 / 马云)? Does the description avoid leaking tech-stack / infra names or legal disclaimers? Flag anything that fails; don't auto-fix.

## 5. Avatar (inline — image links are rejected)

- **Image links are not accepted.** If the user supplies a URL, reject it — do NOT pass it to `--picture`, do NOT download-and-reupload, do NOT claim it was set:
  > "Avatar links aren't supported — send an image file directly, or keep the default."
- **Actively offer at the provider identity card's close** (a CTA, not a passive row):
  > 📷 Profile photo is the default — **send an image to set one** (a plain square, no rounded corners or borders, renders best). Reply **1** when ready.
- **On opt-in:** Claude Code → save the inbound image attachment to a temp path → run the `upload` subcommand (`agent upload --file <temp>`) → use the returned URL as `--picture` (this temp write is the one allowed by SKILL §Gates One-call rule); >1 MB → stop and ask for a smaller one; render the URL verbatim in the Profile photo row. No image supplied → keep the default. 1:1 square is the tip.
- **Upload as-is — never resize/crop/convert.** >1 MB → ask for a smaller file; non-1:1 → accept and upload (square is advisory); non-PNG/JPEG/WebP → ask to convert and resend.

## 6. Endpoint anti-pattern (provider API service)

Require `https://`, publicly reachable, and really deployed. **Reject** `http://`, `localhost`, `127.0.0.1`, RFC-1918 private IPs (`192.168.*` / `10.*` / `172.16–31.*`), `*.local` / `*.internal`, mock URLs, and placeholders. Never suggest any of those as acceptable. Explain a publicly-reachable `https://` URL is required and is permanent on-chain (changing it later needs another update). If the user has no deployed endpoint yet: deploy first, or switch to agent-to-agent.

## 7. Confirmation card (§Invariants card skeleton; never redraw the markup)

requester / evaluator render ONE card. **Providers render TWO** cards in order:

1. **Identity card** (closes Step 1) — Role / Name / [Description] / Profile photo rows, with the avatar CTA at its close. This card closes with **`> Reply **1** to continue.`** (NOT the confirm-run footer). Confirming it (**1**) **advances to Step 2 and does NOT call the CLI** — no `agent create` runs at Step 1.
2. **Service card** (closes Step 2) — `Service [1] Name / Description / Type / Fee / Endpoint` rows; gloss service types once (wording per SKILL §Invariants Lexicon). This is the FINAL card → it carries the confirm-run footer; **1** runs the single `agent create` (carrying both identity and service).

The FINAL card ends with `> Reply **1** to confirm and run.` (localized) + the gate echo: `I won't run anything until you reply **1**.` NL field questions only; no `Q1:` labels, no bash shown.

## 8. Passive need-requester

Skip role-ask / pre-check / photo. Ask name → (description) → render the card → on confirm, execute. Post-success is ONE line, **no detail card, no Step 6**:
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
