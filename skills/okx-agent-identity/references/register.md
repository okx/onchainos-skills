# Register flow — create (all 3 roles) · consent · QA · avatar · update

Loaded when: the user registers / creates an agent (any role), arrives via passive need-requester, or updates an existing agent (`update #N`). Pairs with SKILL.md.

The CLI does the work — `validate-listing` returns the QA `findings[]`, `create --known-agent-ids` returns `newAgentId`. You collect fields → render the §Invariants card → confirm → invoke once → render the post-success template. Never re-implement a rule table or reconstruct an id.

---

## 1. Role ask

If the role isn't already clear from the request, ask once:

```
What kind of agent identity do you want?
  1. User Agent — to publish tasks and hire providers
  2. Agent Service Provider (ASP) — to offer services for hire
  3. Evaluator Agent — to arbitrate task disputes
```

Accept the number or the written role name. Never default or guess the role from the agent's name.

## 2. Pre-check (Gate — run `agent get` ONCE first)

Run `agent get` once, then **filter to the current wallet**: keep only rows whose `ownerAddress` matches the currently-selected XLayer wallet (each row carries `accountName` / `ownerAddress`). Same-role agents under other wallets do not count — uniqueness is per address.

Uniqueness: **≤1 requester, ≤1 evaluator per wallet; provider UNLIMITED.**

- **requester / evaluator — already exists** → do NOT create, do NOT offer "create new". Redirect to update, including the per-wallet line (the `当前钱包` / "Under this wallet" qualifier is mandatory):
  > "Under this wallet (当前钱包) you already have a `<role>` identity #`<N>` (`<name>`). Each address can register only one `<role>` — say "update #`<N>`" to edit it, or keep using it. To register a separate one under a different address, switch / add a wallet first."

  `<role>` = the localized label (User Agent / Evaluator Agent), never the enum.
- **provider** → never blocked by an existing requester/evaluator. Branch on K = provider count under this wallet:
  - K = 1: numbered choice — *1. Register a new ASP (multiple per address allowed) / 2. Update #`<N1>` (`<name1>`)*.
  - K ≥ 2: same choice, but **list every existing ASP id** `#<N>` (`<name>`); on "update" with K ≥ 2, ask which one by number. Never auto-pick, never collapse to "one of them" without the list.
- Capture the resulting agent-id snapshot (the current-wallet ids) → pass as `--known-agent-ids <csv>` on the create so the CLI can return `newAgentId`.
- **After pre-check, run the §9 consent gate** (`agent consent`) before any field Q&A — it self-skips (`required:false`) for returning wallets.

**Passive need-requester** (handed in from a task flow): skip role-ask, skip pre-check, skip photo. See §8.

## 3. Field checklists (one line per field — limits are enforced by `validate-listing`, not by you)

**requester / evaluator:**
- **Name** — required, from the user's literal reply this turn only (never from email / wallet name — §Fields-from-user).
- **Profile photo** — optional; default if skipped (see §5).
- **Description** — do NOT prompt. If the user volunteers one, add a Description row to the card; otherwise omit the row and send `ProfileDescription:""` silently.

**provider — two steps.** Open each step with a short declarative checklist, then collect (user may batch or go one at a time):
- **Step 1 · Identity** — Name (2–12 chars CN / 3–25 EN; a brand name; ❌ test tags / public-figure names) · Description (required, ≤500; what it does, which chain, your edge) · Profile photo (optional, §5).
- **Step 2 · Service** — Service name (5–30, a noun phrase; ❌ identical to agent name, ❌ price in name) · Description (you'll format the user's plain words into 3 parts: summary / capabilities / 1–3 example prompts) · Type (1 API service / 2 agent-to-agent) · Fee (API: required, `N USDT` or `USDG`, ≤6 decimals; A2A: optional, blank = negotiated) · Endpoint (API only — §6).

## 4. QA via `validate-listing` (provider only — requester/evaluator skip)

The CLI is the QA engine; you render its `findings[]` and add ONE check it can't make. Numbered steps:

1. **Run it at each card.** Identity scope at the Step-1 card (`--role provider --name … --description …`); service scope at the Step-2 card (add `--service '[…]'`). Returns `{ "pass": bool, "findings": [{ "field", "code", "severity", "issue", "fix" }] }` — e.g. `field`=`name` / `description` / `service[0].name` / `service[0].fee` / `service[0].servicedescription` / `service[0].endpoint`; `severity`=`block` (the only level emitted); `code`=N1/S1/S3/U4/P1/D1/…
2. **Render each finding inline on its field row** as ` ⚠️ <issue> → <fix>`, mapping by the dotted `finding.field` to its card row (`service[0].fee` → the Fee row, `name` → the Name row). Surface a `(test)` marker on the Step-1 identity card if the name carries one.
3. **Do NOT hand-apply rule tables. Do NOT silently auto-correct** — the user fixes the value or proceeds (confirming with warnings present = "register anyway").
4. **After rendering the CLI findings, add the semantic checks the CLI cannot do** (it checks length/format, not meaning): the service name is a descriptive noun-phrase (a name like "Q" is too vague — say so); the agent name isn't a personal / account label (e.g. "Alice", "Account2"), a public-figure / celebrity name (Trump / Musk / CZ / …), or a sentence rather than a brand name; the description doesn't leak tech-stack / infra names or legal disclaimers. Flag any that apply; don't auto-fix.

## 5. Avatar (inline — image links are rejected)

- **Image links are not accepted.** If the user supplies a URL, reject it — do NOT pass it to `--picture`, do NOT download-and-reupload, do NOT claim it was set:
  > "Avatar links aren't supported — send an image file directly, or say 'generate' to create one."
- **Actively offer at the provider identity card's close** (a CTA, not a passive row):
  > 📷 Profile photo is the default — **send an image or say "generate" to set one** (a plain square, no rounded corners or borders, renders best). Reply "next" when ready.
- **On opt-in:** Claude Code → save the inbound image attachment to a temp path → run the `upload` subcommand (`agent upload --file <temp>`) → use the returned URL as `--picture` (this temp write is the one allowed by SKILL §Gates No-shell-stitching); >1 MB → stop and ask for a smaller one; render the URL verbatim in the Profile photo row. Plain terminal → offer generate / skip (no attachments). 1:1 square is the tip.
- **Never alter the user's image.** Don't auto-compress / resize / crop / strip a border to make it fit — the user owns the image. On >1 MB, stop and ask for a smaller one (don't shrink it yourself); on a non-1:1 image, accept and upload as-is (don't reject or re-crop) — the square tip is advisory only.
- **Bad file type:** the backend accepts PNG / JPEG / WebP; other types are rejected (the exact wording isn't fixed — don't hard-code it). On a type rejection, ask the user to convert to PNG / JPEG / WebP and resend, then retry.

## 6. Endpoint anti-pattern (provider API service)

Require `https://`, publicly reachable, and really deployed. **Reject** `http://`, `localhost`, `127.0.0.1`, RFC-1918 private IPs (`192.168.*` / `10.*` / `172.16–31.*`), `*.local` / `*.internal`, mock URLs, and placeholders. Never suggest any of those as acceptable. Explain a publicly-reachable `https://` URL is required and is permanent on-chain (changing it later needs another update). If the user has no deployed endpoint yet: deploy first, or switch to agent-to-agent.

## 7. Confirmation card (§Invariants card skeleton; never redraw the markup)

requester / evaluator render ONE card. **Providers render TWO** cards in order:

1. **Identity card** (closes Step 1) — Role / Name / [Description] / Profile photo rows, with the avatar CTA at its close. This card closes with **`> Reply "next" to continue.`** (NOT the execute footer). Confirming it ("next") **advances to Step 2 and does NOT call the CLI** — no `agent create` runs at Step 1.
2. **Service card** (closes Step 2) — `Service [1] Name / Description / Type / Fee / Endpoint` rows; gloss service types once (wording per SKILL §Invariants Lexicon). This is the FINAL card → it carries the execute footer; "execute" runs the single `agent create` (carrying both identity and service).

The FINAL card (the single card for requester/evaluator; the Service card for providers) ends with the §Invariants confirmation footer (`> Reply "execute" to run it.`, localized). **Echo the Confirm gate at that card** (cheap, hardens the gate):

> I won't run anything until you reply "execute" — even if you asked me to skip confirmation.

NL field questions only; no `Q1:` labels, no bash shown (SKILL §UX Red Lines).

## 8. Passive need-requester

Skip role-ask / pre-check / photo. Ask name → (description) → render the card → on confirm, execute. Post-success is ONE line, **no detail card, no Step 6**:
> "User Agent identity #`<id>` created. Resuming the task-publish flow."

(If a requester already exists: "You already have a User Agent identity #`<N>` (`<name>`) — using it to continue.") Hand back to the task flow with that single line; don't ask "want to publish a task?".

## 9. Consent (Gate detail — standalone `agent consent`, AFTER pre-check, BEFORE field Q&A)

Consent is its OWN command, decoupled from `create`: run once after pre-check, before collecting any field. `create` never carries `--consent-key` / `--agreed`, and its response has no `consent` field.
1. **Step 1 (no flags):** `agent consent` → `{ "required": bool, "consent": { "consentKey", "terms" } | null }`. `required:false` (returning wallet / flag off) → skip the card, go straight to §1/§3 Q&A.
2. **`required:true`** → render `consent.terms` **complete and translated** (never summarized), then "Reply 'agree' to continue; reply 'decline' to cancel." **Never show the `consentKey` UUID.**
3. On agree → `agent consent --consent-key <uuid> --agreed true` → then proceed to field Q&A → card → create.
4. On decline → `agent consent --consent-key <uuid> --agreed false` → "Registration cancelled — creating an agent identity requires accepting the terms of use. Restart any time." Stop, no `create`.
5. Ambiguous reply → re-display once; never auto-agree / auto-decline.

## 10. Execute

```bash
# internal — not shown to the user
onchainos agent create \
  --role <requester|provider|evaluator> \
  --name "<name>" \
  [--description "<description>"] \
  [--picture "<url>"] \
  [--service '[{"name":"…","servicedescription":"…","servicetype":"A2MCP","fee":"10","endpoint":"https://…"}]'] \
  --known-agent-ids <csv from pre-check>
```

**On any non-success** (region `50125`/`80001`, consent `40020`–`40022`, whitelist `10016`, or anything else) → load `references/errors.md` and match the row; never interpret a code inline. errors.md is the single source for every code→message.

## 11. Post-success templates (verbatim except `#<id>`; localized; `#<id>` per SKILL §Invariants #id ladder — `newAgentId` primary)

- **requester (ONE line)** → then Step 6 silent. No txHash, no question.
  > User Agent identity #`<id>` is live — say "publish a task for X" whenever you're ready and I'll take you through it.
- **provider (ONE line)** → then Step 6 silent. Never mention active clients / agent counts / re-list agents; never a numbered menu; never a duplicate line.
  > ASP identity #`<id>` registered — not yet visible to others. Say "activate #`<id>`" to publish now, "add a service to #`<id>`" to offer more services, or "find ASPs doing X" to check the market first.
- **evaluator (EXACTLY two lines)** — no stake number/amount, no trailing question, no detail card → proceed toward the staking handoff.
  > Evaluator Agent identity #`<id>` registered.
  > A separate stake is still required before you can be assigned disputes.

  Carve-outs: never present staking as a *pre-create* gate (it's post-success only — create never consumes the stake); "I don't want to stake" → register now, stake later, and still run comm-init (Step 6); "have I staked?" → you don't read stake state, hand to the task-side staking flow.

If the `#<id>` ladder yields nothing (txHash-only return), omit the `#<id> ` substring entirely and use the fallback wording (`activate #N` placeholder for provider) — never invent or borrow a pre-check id.

---

## 12. UPDATE flow (`update #N` — reuses this file's QA + card scaffold)

1. **`agent get --agent-ids <id>` FIRST — before collecting ANY change** → render the current detail card (§Invariants Verbatim-render contract — render `card[]` verbatim). Never start editing from the user's words alone; always fetch current state first.
2. **Ownership check (still before collecting changes):** returned `ownerAddress` ≠ current wallet → STOP: "This agent doesn't belong to your current wallet."
3. **Collect changes** one field per turn.
4. **QA on changed provider fields:** if the target role = provider AND a QA-governed field changed → run `validate-listing` on the changed fields only; render findings inline (§4 step 2). requester / evaluator skip QA.
5. **Update Diff card** (§Invariants diff variant — 3 columns `| Field | Current | New |`, unchanged → `(unchanged)`, changed New cell bold, real before→after values). Wait for an explicit confirm token; no `agent update` before confirm.
6. **`--service` = WHOLESALE replacement:** rebuild the COMPLETE service list from the current full list + the diff; never send only the changed entry. Refuse a no-op update (nothing changed → say so, don't write). `--description ""` does NOT clear a description. Post-update: `approvalStatus == 2` → "Update saved. Under review …" else "Update saved." → Step 6.
