# Register flow — create (all 3 roles) · consent · QA · avatar · update

Loaded when: the user registers / creates an agent (any role), or arrives via passive need-user. Pairs with SKILL.md. (For update / fix-rejected-listing → load `identity-update.md` instead.)

The CLI does the work — `validate-listing` returns the QA `findings[]`, `create` always returns `newAgentId` — a string id when the WS push succeeded, `null` when it timed out. You collect fields → render the identity-invariants.md §Card skeleton card → confirm → invoke once → render the post-success template. Never re-implement a rule table or reconstruct an id.

---

## 1. Role ask (do FIRST — `--role` is required by pre-check)

`agent pre-check` **requires** `--role`. If the role is clear, use it; otherwise ask once (accept a number or role name: 1 User / 2 ASP / 3 Evaluator; never default or guess). Then run §2.

> **CLI value is strict.** Always pass the canonical token `--role user` / `--role asp` / `--role evaluator`. The CLI rejects any other value (no `buyer` / `provider` / `requester` / numeric aliases). Map whatever the user typed — a number (1/2/3), a synonym (buyer/卖家/provider/服务提供商/client…), or a label — to one of these three **before** calling.

## 2. Pre-check (Gate — `agent pre-check --role <role> [--consent-key <uuid>]`: consent + uniqueness in ONE command)

Run `agent pre-check --role <role>` (internal — never shown). It fetches the wallet's agents; **if the wallet has agents it's already consented** (→ straight to the uniqueness verdict); **if it has none it runs the consent gate first**. It always returns `{ canCreate, role, reason?, consent?, existingSameRole, aspCount }` — **never call `agent get-my-agents` / `agent consent` yourself for registration**. Branch on the result:

- **`consent` present** (always `canCreate:false`) → first-time wallet. Show `consent.terms` complete and translated (never summarized; never show `consentKey`). Present `1. Agree & continue` / `2. Decline & cancel`. `1` → re-run `agent pre-check --role <role> --consent-key <uuid>`; `2` → stop. Ambiguous → re-display once.
- **`canCreate:false`** (no `consent` field — a single-role identity already exists; `reason` explains) → do NOT create, do NOT offer "create new". Redirect to update with the mandatory per-wallet line, filling `<roleLabel>` / `<N>` / `<name>` from `existingSameRole[0]`:
  > "Under this wallet you already have a `<roleLabel>` identity #`<N>` (`<name>`). Each address can register only one `<roleLabel>` — say "update #`<N>`" to edit it, or keep using it. To register a separate one under a different address, switch / add a wallet first."
- **`canCreate:true`** → may register. ASP role with existing ASPs (K ≥ 1): K=1 → offer *1. New ASP / 2. Update #`<N>` (`<name>`)*; K ≥ 2 → list from `existingSameRole` by number (never auto-pick). If the user mentions fixing a rejected listing → steer to option 2 + §11 rule (only create if user explicitly insists). K=0 / user/evaluator → §3.
- Proceed to the §3 field Q&A and eventually `create` — the CLI always returns `newAgentId` (string id on WS success, `null` on timeout).

**Passive need-user** (handed in from a task flow): skip the pre-check loop / photo entirely. See §8.

## 3. Field checklists (one line per field — limits are enforced by `validate-listing`, not by you)

**user / evaluator:**
- **Name** — required, from the user's literal reply this turn only (never from email / wallet name — §Fields-from-user).
- **Profile photo** — optional; default if skipped (see §5).
- **Description** — do NOT prompt. If the user volunteers one, add a Description row to the card; otherwise omit the row and send `ProfileDescription:""` silently.

**ASP — two steps** (user may batch):
- **Step 1 · Identity** — Present all three as a **single numbered list in one message** (do NOT split into separate turns):
  1. **Name** — brand name (CN 2–12 chars / EN 3–25 chars; no test markers / celebrity names)
  2. **Description** — one-sentence summary of what the Agent does (required, ≤500 chars)
  3. **Avatar — required**: send an image file (§5).
- **Step 2 · Service — three sub-steps** (collect name + type first; the pricing model picked in Step 2b decides **how many description parts** to collect, so **description comes LAST** in Step 2c; a user who sends everything at once is fine — just proceed). Present each sub-step warmly and scannably: a short numbered list, no `Q1:` jargon. Any example text is illustrative only — use the user's own reply (§Fields-from-user).
  - **Step 2a · name + type (ONE message — 2 fields):**
    1. **Service name** — 5–30 noun phrase; not the same as the agent name; no price in the name.
    2. **Type** — API service (pass `A2MCP`) or agent-to-agent (pass `A2A`).
  - **Step 2b · pricing (+ endpoint), tailored to the 2a type (ONE message — short lines, never a run-on):**
    - **`A2MCP`** — two fields: a per-call **Price** (one number) + a public `https://…` **Endpoint** (§6).
    - **`A2A`** — no endpoint; one numbered pick + price: **1** per-call · **2** monthly subscription · **3** monthly subscription + free 3-day trial (monthly only). e.g. reply `2 10` = monthly 10.
  - **Fee format (both types):** a **plain number sent as a string** (e.g. `"10"` — quoted in the JSON, never a bare number); currency is always USDT — tell the user (localized) the amount is **digits only, no unit/symbol** (no `USDT`/`USDG`/`元`/symbol); ≤6 decimals; `0` is allowed (a free service); reject `10 USDT` / `approx 10` / `5元` → re-ask. Displayed back as `N USDT`. Full rule → `identity-invariants.md` §Input contract (`fee`); applies to every subscription-tier fee too.
  - **A2MCP pricing (unchanged):** a single required `fee`. No subscription. Pass `fee` = the number string.
  - **A2A pricing mechanics (per-call fee XOR monthly subscription — EXACTLY ONE; trial folded into the pick, never a standalone question):** the Step-2b `1/2/3` pick maps to `--service` as below. Monthly only — state it plainly (only `interval:"month"` is supported today; no weekly/yearly/other period). Never offer a "both" option.
    - **1 · per-call** → send `fee:"<n>"`, `subscription:[]`. (No trial — trials are subscription-only.)
    - **2 · monthly** → send `fee:""` (empty string — the "no single price" marker), `subscription:[{"interval":"month","fee":"<n>"}]`, and **omit `freeTrial` entirely** (never `""` / `"0"`).
    - **3 · monthly + trial** → same as **2**, plus `freeTrial:"72"` (72h = a **fixed 3 days**). `freeTrial` is valid ONLY here — never on per-call A2A or on A2MCP.
    - **Trial length is fixed at 3 days.** If the user asks for any other length (e.g. "5-day trial") → do NOT honor it: say the trial is fixed at 3 days, so it's pick **3** (with trial) or **2** (without) — re-ask.
    - **Follow up only to fill a gap — never re-ask what's already given.** If the reply already gave a valid pick + price, proceed straight to Step 2c. Ask a targeted follow-up ONLY for a missing/ambiguous piece: no clear **1/2/3** → re-show the three-way pick; a monthly reply that doesn't say whether they want the trial → clarify "**2** (no trial) or **3** (3-day trial)?"; a reply naming *both* per-call and monthly, or *neither* → explain it's exactly one of the three and re-ask. Do not advance until exactly one is settled.
  - **Step 2c · description (ONE message — the Step-2b pricing model decides BOTH the part count AND which prompt set to show. Show ONLY the matching set. Put each part on its own line, prefixed `1.` / `2.` / `3.`):**
    - **Non-subscription = ordinary service** (A2MCP, or A2A per-call — pick **1**) → collect **all three parts**, using the **ordinary-service** prompt ONLY (do NOT show trading-signal hints — the market-declaration / signal-example requirements do NOT apply here):
      1. **core-capability summary** — what it does + who it's for.
      2. **what the user must provide** — e.g. wallet address / amount / chain.
      3. **delivery note** — what the user gets + whether copy-trading is supported (e.g. delivered as a file, no copy-trading).
    - **Subscription-priced = trading-signal service** (A2A monthly — pick **2** or **3**) → collect **two parts** — the core-capability summary and the delivery note (omit "what the user must provide": a subscription auto-delivers, so there is nothing to submit per request), prefixed `1.` / `2.`:
      1. **core-capability summary** — what it does + who it's for; name each covered market explicitly, using only **DEX / Polymarket / Hyperliquid** (list only the ones actually supported).
      2. **delivery note** — delivered as structured signals + whether copy-trading is supported + one concrete signal example that starts with the full market name (never an abbreviation like "HL") and references only a market declared in part 1. **Always show the user this illustrative signal-example format in the prompt so they know the expected shape** (display hint only — do NOT store it verbatim; the user supplies their own): `DEX Signal: X Layer | $TOKEN (0x12…ab) | BUY | 0.042-0.045 | Slippage ≤1% | Position 5% | Valid within 24h`.
    - **Both cases:** each part on its own line; each part ≤200 CJK / total ≤600 CJK by **East-Asian display width** (CJK = 2, ASCII = 1; enforced by `validate-listing`). **No example prompts, no GitHub/wallet links, no tech-stack/infra details, no disclaimers, and no profit/return guarantees (e.g. "guaranteed profit" / "double your money").**
- **After EACH service (BLOCKING — incl. the first; the "batched fields ≠ Done" rule is SKILL §Gates Service-collection)** — ask once (localized) **1. Add another service / 2. Done**; on **1** repeat Step 2 and append to the service array, then ask again; on **2** (or other) → §4 with the complete array. You MUST wait for the explicit Done choice — never auto-advance because one service's fields look complete; all services ship in one `agent create`.
- **Do NOT run `validate-listing` inside this loop.** QA is a single batch pass that happens in §4 *after* the array is complete — never validate per service, never validate while still collecting.

## 4. QA via `validate-listing` (ASP only — user/evaluator skip) — runs EXACTLY ONCE

Validate is a **single batch gate**, NOT a per-service step. Collect the **complete** identity (Step 1) **and the full service array** (every service, via the §3 Step-2 add-another loop) BEFORE you call it. One registration = one `validate-listing` call. Numbered steps:

1. **Call once, on the full set.** **Hard precondition (SKILL §Gates Service-collection): unless the user has explicitly chosen Done in the §3 Step-2 add-another prompt (1. Add another / 2. Done), you MUST NOT call `validate-listing` — no matter how complete the fields look.** A single batched message carrying every field for one service does NOT satisfy this; ask the add-another prompt and wait for the Done choice first. Only after the user picks *Done* in §3 Step 2, run `validate-listing --role asp --name … --description … --service '[… all collected services …]'` a single time. Returns `{ pass, findings[{field, code, severity:"block", issue, fix}] }`. `field` uses dot-notation (e.g. `service[0].fee`, `service[1].name`).
2. **Render the findings card — as suggestions only.** Always run the semantic checks in step 4 first and merge with the CLI findings. Only when both `pass:true` AND no semantic issues are found → say it passed and go straight to §7. Otherwise render each finding (CLI + semantic) inline on its field row as ` ⚠️ <issue> → <fix>`, mapping by the dotted `finding.field` to its card row (`service[0].fee` → Service [1]'s Fee row, `service[1].*` → Service [2]'s rows, `name` → the identity Name row). Surface a `(test)` marker on the name row if present. **At this point the `<fix>` text is only a recommendation on display — the field values are unchanged; do NOT apply any `fix` yet.**
3. **Confirmation is mandatory — never apply a suggestion before the user chooses.** After showing the card, ask once how to proceed — exactly TWO numbered choices (localized). Do NOT re-run `validate-listing`:
   > 1. Apply the suggested fixes — I'll update the flagged field(s) with the fixes shown above, then redraw the card for you to review.
   > 2. I'll revise it myself — tell me the new value(s).
   - On **1**: this choice **is** the user's confirmation for the whole batch of suggestions. Only now apply each shown `finding.fix` to its mapped field (plus your own semantic fixes), then redraw the card with the corrected values. Apply **once** — do not iterate.
   - On **2**: collect the user's replacement value(s) for the flagged field(s) and redraw the card.
   Either way, the corrected values still flow into the §7 confirmation card — **nothing is written on-chain until the user confirms there (Reply 1)**. **`validate-listing` has already run its single pass — never call it again** (`activate` does NOT re-run QA; listing QA happens only here at register and at update). Never apply a `fix` before the user picks; never silently auto-correct; never force a fix.
4. **Semantic checks the CLI cannot do — always run, regardless of `pass:true`** (merge into step 2's findings list). Check, by meaning:
   - **Service name** — a descriptive noun-phrase, not just a letter like "Q".
   - **Agent name** — a brand, not a personal label (Alice, Account2), and NOT containing a celebrity / public-figure name as a substring (block even if prefixed/suffixed — Trump, Musk, CZ, 马斯克, 马云). Per `identity-invariants.md` §Fields-from-user.
   - **Description structure** — matches the pricing-based part rule in §3 Step 2c (non-subscription = 3 parts / subscription = 2 parts, each part on its own line) and leaks no tech-stack / infra names or legal disclaimers.
   - **Subscription (= trading-signal) services ONLY** (non-subscription/ordinary services SKIP this entirely): the core-capability part must explicitly declare every covered market using ONLY DEX / Polymarket / Hyperliquid, and the delivery note must carry a concrete signal example that starts with the full market name (never an abbreviation like "HL") and references only a market declared in the core-capability part; no undeclared market may appear in the example.
   - **Profit / return guarantee — any language.** The CLI's **D9** is only a deterministic backstop for common guarantee phrases in Chinese and English (hardcoded in the CLI) — surface any D9 finding as-is. Its list cannot cover every language, so you MUST **additionally** block, by meaning, any profit / return / no-loss guarantee in **ANY** language even when D9 did not flag it — including a "guaranteed profit" / "guaranteed income" / "no-loss" / "double your money" claim phrased in any language outside the D9 list (e.g. "blow up your gains") — describe the capability, not a promised outcome.

## 5. Avatar (inline — image links are rejected)

- **Image links are not accepted.** If the user supplies a URL, reject it — do NOT pass it to `--picture`, do NOT download-and-reupload, do NOT claim it was set:
  > "Avatar links aren't supported — send an image file directly (ASPs must; user/evaluator may keep the default)."
- **ASP — required** (item 3 of the Step 1 list; no sub-choices):
  > 3. Avatar — 📷 Required. Send an image file to set your avatar (1:1 square recommended).

  Must send an image → upload it. No image → no default fallback: re-ask and do NOT advance to Step 2 / render the identity card until one is uploaded. (The CLI is the authoritative gate — `create` rejects an ASP with no `--picture` — but the upload must happen here so the user never hits that error.)
- **user / evaluator — optional** (no sub-choices):
  > Profile photo — 📷 Optional. Send an image file to set a custom avatar; skip to keep the default.

  Image → upload; skip → keep default.
- Never ask the user to pick 1/2.
- **On opt-in:** Claude Code → save the inbound image attachment to a temp path → run the `upload` subcommand (`agent upload --file <temp>`) → use the returned URL as `--picture` (this temp write is the one allowed by SKILL §Gates One-call rule); >1 MB → stop and ask for a smaller one; render the URL verbatim in the Profile photo row. No image → keep default (user/evaluator only). 1:1 square is the tip.
- **Upload as-is — never resize/crop/convert.** >1 MB → ask for a smaller file; non-1:1 → accept and upload (square is advisory); non-PNG/JPEG/WebP → ask to convert and resend.

## 6. Endpoint anti-pattern (ASP API service)

Require `https://`, publicly reachable, and really deployed. **Reject** `http://`, `localhost`, `127.0.0.1`, RFC-1918 private IPs (`192.168.*` / `10.*` / `172.16–31.*`), `*.local` / `*.internal`, mock URLs, and placeholders. Never suggest any of those as acceptable. Explain a publicly-reachable `https://` URL is required and is permanent on-chain (changing it later needs another update). If the user has no deployed endpoint yet: deploy first, or switch to agent to agent.

**Length guard** — endpoint URL must be ≤512 chars; if longer → "The endpoint URL must be at most 512 chars; this one is longer. Use a shorter URL." Re-ask.

## 7. Confirmation card (identity-invariants.md §Card skeleton; never redraw the markup)

user / evaluator render ONE card. **ASPs render TWO** cards in order:

1. **Identity card** (closes Step 1) — Role / Name / [Description] / Profile photo rows, with the avatar CTA at its close. **ASP avatar is mandatory (§5): the Profile photo row is an uploaded CDN URL, never `default` — if none yet, re-ask before rendering this card.** This card closes with **`> Reply **1** to continue.`** (NOT the confirm-run footer). Confirming it (**1**) **advances to Step 2 and does NOT call the CLI** — no `agent create` runs at Step 1.
2. **Service card** (closes Step 2) — render ONE block of `Service [N] Name / Description / Type / Fee / Subscription / Free trial / Endpoint` rows **per collected service** (`Service [1]`, `Service [2]`, … — never assume a single service); gloss service types once (wording per identity-invariants.md §Lexicon). **Pricing rows:** show the single `Fee` as `N USDT` (or `—` when subscription-priced, i.e. `fee:""`); show `Subscription` as `N USDT / month` per monthly tier (or `—` when there is none). **Free trial row:** `3 days` when `freeTrial:"72"` is set, otherwise `—` (single-fee A2A and A2MCP always show `—`; duration-display rule per identity-invariants.md §Lexicon Free trial). A2MCP always shows a single Fee and `Subscription: —`. This is the FINAL card → it carries the confirm-run footer; **1** runs the single `agent create` (carrying the identity plus ALL collected services).

The FINAL card ends with `> Reply **1** to confirm and run.` (localized) + the gate echo: `I won't run anything until you reply **1**.` NL field questions only; no `Q1:` labels, no bash shown.

## 8. Passive need-user

Run `agent pre-check --role user` (consent + uniqueness gate, same as §2). On consent required → run full consent flow per §2. On `canCreate:false` (user already exists) → use the existing one, skip create entirely. On `canCreate:true` → ask name only (skip photo). Then render the card → on confirm, execute. Post-success is ONE line, **no detail card**:
> "User identity #`<id>` created. Resuming the task-publish flow."

(If a user already exists: "You already have a User identity #`<N>` (`<name>`) — using it to continue.") Hand back to the task flow with that single line; don't ask "want to publish a task?".

## 9. Execute

Run `agent create` with the collected fields (role/name/description/picture/service — all from §3). **On any non-success** → load `identity-errors.md`; never interpret a code inline.

## 10. Post-success templates (verbatim except `#<id>`; localized; `#<id>` per identity-invariants.md §#id ladder — `newAgentId` primary)

- **user (ONE line)** — No txHash, no question. After emitting it, run the communication-init flow in [`chat-comm-init.md`](chat-comm-init.md) so the new agent can communicate (create has no CLI-level readiness gate).
  > User identity #`<id>` is live — say "publish a task for X" whenever you're ready and I'll take you through it.
- **ASP (ONE line)** — Never mention active clients / agent counts / re-list agents; never a numbered menu; never a duplicate line. After emitting it, run the communication-init flow in [`chat-comm-init.md`](chat-comm-init.md) so the new agent can communicate (create has no CLI-level readiness gate).
  > ASP identity #`<id>` registered — not yet visible to others. Say "activate #`<id>`" to publish now, "add a service to #`<id>`" to offer more services, or "find ASPs doing X" to check the market first.
- **evaluator (EXACTLY two lines)** — no stake number/amount, no trailing question, no detail card → proceed toward the staking handoff.
  > Evaluator identity #`<id>` registered.
  > A separate stake is still required before you can be assigned disputes.

  (Staking is post-create, never a pre-create gate; "don't want to stake" → register now, stake later; "have I staked?" → hand to staking flow.)

If `#<id>` ladder yields nothing: user/evaluator → omit `#<id>` entirely; ASP → `Say "list my agents" to find your new identity, then "activate #<id>" to publish.`

---

## 11. UPDATE flow

See [`identity-update.md`](identity-update.md) — ownership check, QA, diff card, wholesale service replacement, post-update messages, and rejected-listing remediation rule.
