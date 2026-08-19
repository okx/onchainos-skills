# Register flow — create (all 3 roles) · consent · QA · avatar · update

Loaded when: the user registers / creates an agent (any role), or arrives via passive need-user. Pairs with SKILL.md. (For update / fix-rejected-listing → load `identity-update.md` instead.)

The CLI does the work — `validate-listing` returns the QA `findings[]`, `create` always returns `newAgentId` — a string id when the WS push succeeded, `null` when it timed out. You collect fields → render the identity-invariants.md §Card skeleton card → confirm → invoke once → render the post-success template. Never re-implement a rule table or reconstruct an id.

---

## 1. Role ask (do FIRST — `--role` is required by pre-check)

`agent pre-check` **requires** `--role`. If the role is clear, use it; otherwise ask once (accept a number or role name: 1 User / 2 ASP / 3 Evaluator; never default or guess). Then run §2.

> **CLI value is strict.** Always pass the canonical token `--role user` / `--role asp` / `--role evaluator`. The CLI rejects any other value (no `buyer` / `provider` / `requester` / numeric aliases). Map whatever the user typed — a number (1/2/3), a synonym in any language (buyer/seller/provider/merchant/client/卖家/服务提供商…), or a label — to one of these three **before** calling.
>
> **Evaluator rename (评审员).** The Evaluator role was previously surfaced with legacy words; `评审员` is the canonical Chinese label for `evaluator`. Still recognize legacy aliases and map them to `--role evaluator`, but when the user types one, gently correct in the reply before continuing **without echoing the old word** — emit the verbatim rename prompt from identity-invariants.md §Legacy role words — rename prompt, matched to the conversation language. Never surface a legacy alias in any prompt, card, confirmation, or correction message — always render the canonical **Evaluator** label (localized per identity-invariants.md §Lexicon).

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
- **Step 2 · Service — three sub-steps** (collect name + type first; the service type decides the description shape — A2MCP always uses the four-part request description, and A2A always uses the same three-part service description regardless of pricing — so **description comes LAST** in Step 2c; a user who sends everything at once is fine — just proceed). Present each sub-step warmly and scannably: a short numbered list, no `Q1:` jargon. Any example text is illustrative only — use the user's own reply (§Fields-from-user).
  - **Step 2a · name + type (ONE message — 2 fields):**
    1. **Service name** — 5–30 noun phrase; not the same as the agent name; no price in the name.
    2. **Type** — API service (pass `A2MCP`) or agent-to-agent (pass `A2A`).
  - **Step 2b · pricing (+ endpoint), tailored to the 2a type (ONE message — short lines, never a run-on):**
    - **`A2MCP`** — two fields: a per-call **Price** (one number) + a public `https://…` **Endpoint** (§6).
    - **`A2A`** — no endpoint; one numbered pick + price: **1** per-call · **2** monthly subscription · **3** monthly subscription + free 3-day trial (monthly only). e.g. reply `2 10` = monthly 10.
  - **Fee format (both types):** a **plain number sent as a string** (e.g. `"10"` — quoted in the JSON, never a bare number); currency is always USDT — tell the user (localized) the amount is **digits only, no unit/symbol** (no `USDT`/`USDG`/currency word in any language/symbol); ≤6 decimals; `0` is allowed (a free service); reject `10 USDT` / `approx 10` / a fee carrying a localized currency word → re-ask. Displayed back as `N USDT`. Full rule → `identity-invariants.md` §Input contract (`fee`); applies to every subscription-tier fee too.
  - **A2MCP pricing (unchanged):** a single required `fee`. No subscription. Pass `fee` = the number string.
  - **A2A pricing mechanics (per-call fee XOR monthly subscription — EXACTLY ONE; trial folded into the pick, never a standalone question):** the Step-2b `1/2/3` pick maps to `--service` as below. Monthly only — state it plainly (only `interval:"month"` is supported today; no weekly/yearly/other period). Never offer a "both" option.
    - **1 · per-call** → send `fee:"<n>"`, `subscription:[]`. (No trial — trials are subscription-only.)
    - **2 · monthly** → send `fee:""` (empty string — the "no single price" marker), `subscription:[{"interval":"month","fee":"<n>"}]`, and **omit `freeTrial` entirely** (never `""` / `"0"`).
    - **3 · monthly + trial** → same as **2**, plus `freeTrial:"72"` (72h = a **fixed 3 days**). `freeTrial` is valid ONLY here — never on per-call A2A or on A2MCP.
    - **Trial length is fixed at 3 days.** If the user asks for any other length (e.g. "5-day trial") → do NOT honor it: say the trial is fixed at 3 days, so it's pick **3** (with trial) or **2** (without) — re-ask.
    - **Follow up only to fill a gap — never re-ask what's already given.** If the reply already gave a valid pick + price, proceed straight to Step 2c. Ask a targeted follow-up ONLY for a missing/ambiguous piece: no clear **1/2/3** → re-show the three-way pick; a monthly reply that doesn't say whether they want the trial → clarify "**2** (no trial) or **3** (3-day trial)?"; a reply naming *both* per-call and monthly, or *neither* → explain it's exactly one of the three and re-ask. Do not advance until exactly one is settled.
  - **Step 2c · description (ONE message — branch on the Step-2a serviceType ONLY: A2MCP uses the four-part request description regardless of pricing ("per-call" does NOT make it an ordinary A2A service); every A2A uses the SAME three-part prompt regardless of the Step-2b pricing model — there is no per-call / subscription variant. Show ONLY the matching set. Put each part on its own line. **A2MCP keeps its `1.`/`2.`/`3.`/`4.` numbering + bracketed labels (they carry meaning for the request description); A2A needs NO numbering prefix** — one line per part is enough, and numbering is optional when several parts are present):**
    - **A2MCP = API service → request description (all four parts, BLOCKING at §4).** Show the A2MCP request-description prompt (NOT the A2A prompt). Each part on its own line, prefixed `1.`/`2.`/`3.`/`4.`:
      1. **service description** — what the service does (its function / purpose).
      2. **parameter specification** — ALL key parameters on ONE line, separated by `;`, each in the strict format `<name>(<type>, required/optional): <meaning>` (optional param: append `, <default>` to the meaning; punctuation and marker words localized per identity-invariants.md §A2MCP `serviceDescription` structure); key params suffice (overflow rule in §A2MCP `serviceDescription` structure). If the user's input is present but not in this format, proactively rewrite it into the strict one-line `;`-separated format and confirm (or let them correct it) with the user before storing — see identity-invariants.md §A2MCP `serviceDescription` structure.
      3. **request method** — how to call it: **only** an HTTP-protocol verb (`POST` / `GET` / `PUT` / `PATCH` / `DELETE` / …) **or** a bare MCP tool name. **No URL, no path, no other text** — the endpoint already lives in the separate `endpoint` field. If the user pastes a URL or path here (e.g. `POST https://…/okx/scan` or `/okx/scan`), keep only the verb (or tool name) and drop the rest → store `POST` (identity-invariants.md §A2MCP `serviceDescription` structure — *Request-method — verb / tool-name only* bullet).
      4. **request example** — a working `curl` command that a buyer can run to call the service. Must use the real endpoint URL and include a realistic request body / query params. If the user gives a non-`curl` example (Python snippet, pseudo-code, etc.), reformat into `curl` and confirm before storing (identity-invariants.md §A2MCP `serviceDescription` structure — *Request-example* bullet).
      Prefix each stored line with its bracketed label — `[Service Description]` / `[Parameter Spec]` / `[Request Method]` / `[Request Example]` (localized to the conversation language per identity-invariants.md §A2MCP `serviceDescription` structure — *Label prefix* bullet): keep the user's label if they typed one, supplement it if they didn't. **Do NOT show any copyable fill template** — the inline four-part prompt above is the only guidance; reformat whatever the user gives into `1./2./3./4.` for storage (identity-invariants.md §A2MCP `serviceDescription` structure holds the §4 block copy).
    - **A2A = agent to agent → three parts, the SAME prompt for per-call and subscription pricing alike** (do NOT branch on the Step-2b pick). Part `1.` is required; parts `2.` and `3.` are optional — if the user leaves either out, record what they gave and advance:
      1. **core-capability summary (required)** — capability points + who it's for. Write it as: capability points + who it's for (+ for a signal service, what kind of trading signals it pushes).
      2. **what the user must provide (optional)** — list the materials the user has to supply, e.g. `1. wallet address 2. amount 3. chain`.
      3. **delivery note (optional)** — the delivery format; for a trading-signal service you may add how copy-trading works.
    - **All cases (A2MCP + A2A):** each part on its own line; total ≤1000 CJK by **East-Asian display width** (CJK = 2, ASCII = 1) — no per-part length limit. **No links in an A2A description** — every URL there is hard-blocked at §4 (A2MCP is exempt, see below); a wallet or contract address in the text is fine and blocks nothing.
      - **A2MCP is exempt from the URL ban:** the request example on line `4.` MUST carry the real endpoint URL, so an A2MCP description necessarily contains one and `validate-listing` allows it. Collect it as asked.
    - **At COLLECTION time, record the service description verbatim and advance — never a hard gate here** (content only; everything else still gates — §4 step 1). If the user omits, contradicts, or asks to skip / keep any of it as-is (in any language), **record it verbatim and advance** — never refuse to proceed at collection, never present it as mandatory here. The gating happens later at §4:
      - **A test marker and an empty description BLOCK at §4** via `validate-listing` (`pass:false`) for every service type, and **a URL blocks for A2A** (A2MCP is exempt — its request example must carry the endpoint URL); they are corrected, not declined.
      - **Over-length (total > 1000 CJK) is advisory only at §4** — `validate-listing` reports it as `severity:"suggest"` (does NOT fail `pass`); surface as a suggestion, never decline or present as mandatory. (The length cap is A2A-only.)
      - **Paragraph count is NEVER validated** — the three-part layout above is collection guidance only, and parts `2.`/`3.` are optional. `validate-listing` has no paragraph-count rule, so a non-empty description of any shape passes, and subscription and per-call services are validated identically. Never tell the user a paragraph count is required, and never chase a missing part `2.` or `3.`.
      - **A2MCP request-description completeness is a hard block at §4** — §4 blocks until all four items are present. This applies to A2MCP ONLY; A2A has no completeness gate.
      - Only the A2A `serviceDescription` semantic quality described in §4 is handled by optimize-and-confirm, where the user may accept the drafted rewrite or keep their own.
- **After EACH service (BLOCKING — incl. the first; the "batched fields ≠ Done" rule is SKILL §Gates Service-collection)** — ask once (localized) **1. Add another service / 2. Done**; on **1** repeat Step 2 and append to the service array, then ask again; on **2** (or other) → §4 with the complete array. You MUST wait for the explicit Done choice — never auto-advance because one service's fields look complete; all services ship in one `agent create`.
- **Do NOT run `validate-listing` inside this loop.** QA is a single batch pass that happens in §4 *after* the array is complete — never validate per service, never validate while still collecting.

## 4. QA via `validate-listing` (ASP only — user/evaluator skip) — runs EXACTLY ONCE

Validate is a **single batch gate**, NOT a per-service step. Collect the **complete** identity (Step 1) **and the full service array** (every service, via the §3 Step-2 add-another loop) BEFORE you call it. One registration = one `validate-listing` call. Numbered steps:

1. **Call once, on the full set.** **Hard precondition (SKILL §Gates Service-collection): unless the user has explicitly chosen Done in the §3 Step-2 add-another prompt (1. Add another / 2. Done), you MUST NOT call `validate-listing` — no matter how complete the fields look.** A single batched message carrying every field for one service does NOT satisfy this; ask the add-another prompt and wait for the Done choice first. Only after the user picks *Done* in §3 Step 2, run `validate-listing --role asp --name … --description … --service '[… all collected services …]'` a single time. Returns `{ pass, findings[{field, code, severity, message}] }`. `severity` is `"suggest"` (advisory — never fails `pass`) for exactly one `serviceDescription` finding: the A2A total-length `D2`. It is `"block"` for **everything else**, including the URL `D6` (A2A only — an A2MCP description may carry the endpoint URL of its `curl` example), the test-marker `U1`, the empty-description `D1`, and every non-serviceDescription field. There is **no paragraph-count finding at all** — the CLI never counts paragraphs, for either billing model. `code` is a fine-grained diagnostic (`U1`/`N1`/`D6`…) for grouping only (never shown to the user); `message` is the rule's single unified user-facing message text and the only rule-message field exposed. `field` uses dot-notation (e.g. `service[0].fee`, `service[1].name`).
2. **Render the findings card.** Always run the semantic checks in step 4 first and merge with the CLI findings. **A2MCP request-description block:** if step 4's A2MCP four-item check fails for any A2MCP service, the flow is **blocked** — show that service's rejection reason + user suggestion (identity-invariants.md §A2MCP `serviceDescription` structure — no copyable template) and re-collect its description; do NOT advance to §7 until every A2MCP service passes, regardless of the apply/revise choice below (which governs only how the collected corrections are gathered). Only when `pass:true` AND no semantic issues AND every A2MCP service's request description is complete → say it passed and go straight to §7. Otherwise render the findings inline on their field rows, mapping by the dotted `finding.field` to its card row (`service[0].fee` → Service [1]'s Fee row, `service[1].*` → Service [2]'s rows, `name` → the identity Name row). **Render each finding's translated `message` — de-duplicated by (`field`, `message`) so one unified sentence shows once per field** even when several `code`s produced the same `message` (never print the `code` token itself). Surface a `(test)` marker on the name row if present. **`severity:"block"` findings must be corrected before advancing to §7 — do not treat them as optional tips; `severity:"suggest"` findings (the A2A total-length `D2`) and the A2A core-capability semantic suggestion are advisory and never block `pass`.** **The field values are unchanged at display time — do NOT silently apply any change yet.**
3. **Confirmation is mandatory — never apply a change before the user chooses.** After showing the card (each flagged field carrying its unified `message` + any skill-semantic optimize-and-confirm draft), ask once how to proceed, and do NOT re-run `validate-listing`:
   - **If there is ANY blocking item** (`severity:"block"` CLI finding or A2MCP request-description block), ask exactly TWO numbered choices (localized):
     > 1. Use the corrected version — I'll fill each blocking field with the fix drafted above, then redraw the card for you to review.
     > 2. I'll revise it myself — tell me the new value(s).
     On **1**: this choice **is** the user's confirmation for the blocking fixes. Apply the drafted correction for blocking fields FROM their `message` (e.g. trim to ≤ 500 chars, remove the URL/test-marker, fill in a missing description), then redraw the card with the corrected values. Apply **once** — do not iterate. Every field you drafted stays flagged ` ✏️ drafted from your words — please review`. Advisory suggestions on the same card remain optional and must not be forced.
   - **If the card contains ONLY advisory suggestions** (`severity:"suggest"` and/or the A2A core-capability semantic suggestion), ask exactly THREE numbered choices, with skip first (localized). This applies to every non-blocking rewrite or style suggestion shown during listing QA; never show a separate two-choice prompt for advisory-only content:
     > 1. Skip suggestions — submit the original text as is.
     > 2. Use the suggested version — I'll fill the drafted text, then redraw the card for you to review.
     > 3. I'll revise it myself — tell me the new value(s).
     On **1**, keep the original field values unchanged and continue to §7. On **2**, apply the ✏️ draft once and redraw the card. On **3**, collect the user's replacement value(s) and redraw the card.
   The selected values still flow into the §7 confirmation card — **nothing is written on-chain until the user confirms there (Reply 1)**. **`validate-listing` has already run its single pass — never call it again** (`activate` does NOT re-run QA; listing QA happens only here at register and at update). Never apply a `fix` before the user picks; never silently auto-correct; never force a fix.
4. **Semantic checks the CLI cannot do — always run, regardless of `pass:true`** (merge into step 2's findings list). Check, by meaning:
   - **Service name** — a descriptive noun-phrase, not just a letter like "Q".
   - **Agent name** — a brand, not a personal label (Alice, Account2), and NOT containing a celebrity / public-figure name as a substring (block even if prefixed/suffixed — Trump, Musk, CZ — in any language or script). Per `identity-invariants.md` §Fields-from-user.
   - **Description quality (A2A advisory)** — check ONLY whether the A2A `serviceDescription` contains a core-capability introduction by meaning: a clear statement of what the service does / what capability it provides. The core-capability introduction may appear anywhere in the text; do not require a specific paragraph number, label, order, or count. **Only when no core-capability introduction is present or it is too unclear to identify, surface ONE suggestion — do NOT block, and NEVER present it as a mandatory requirement.** That suggestion must use the advisory-only confirmation branch in step 3, with "skip suggestions / submit original" first. If a core-capability introduction is present, do NOT suggest changes for missing target audience, missing signal type, missing user-provided materials, missing delivery note, wording style, optional sections, paragraph count, pricing model split, markets/venues, examples, tech stack, disclaimers, or profit wording. Parts `2.` and `3.` remain optional; never ask for a missing part `2.` or part `3.`.
   - **A2MCP request-description completeness (BLOCKING — `serviceType == "A2MCP"` only).** **Contract-address exemption: if the `serviceDescription` contains a contract address (`0x` followed by 40 hex chars), skip this check entirely and pass.** Otherwise, for each A2MCP service, verify by semantic judgment that its `serviceDescription` carries all four, in order: (1) **what the service does**, (2) the **parameter spec** — all key parameters on ONE line, separated by `;`, each in the strict format `<name>(<type>, required/optional): <meaning>` (identity-invariants.md §A2MCP `serviceDescription` structure), (3) the **request method** (`POST`/`GET` or tool name), (4) a **working CURL example** using the real endpoint. All four present by meaning → pass. Any missing → **block the flow**: do NOT advance to the §7 confirmation card; show the rejection reason + user suggestion from identity-invariants.md §A2MCP `serviceDescription` structure (no copyable template), then re-collect that description. This is a hard gate (the PRD mandates a block) — unlike the advisory A2A content checks above. Register and update (identity-update.md §4) use the same rule and copy. Never block solely because not every parameter is enumerated (overflow tie-break — §A2MCP `serviceDescription` structure). A present-but-loosely-worded per-parameter spec is **proactively normalized into the strict format and confirmed with the user** (§A2MCP `serviceDescription` structure), NOT blocked — the block fires only for an entirely-missing item.

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
