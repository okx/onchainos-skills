# Register flow — create (all 3 roles) · consent · QA · avatar

Preserve the canonical `User` / `ASP` / `Evaluator` role when provided.

The CLI does the work; use `identity-cli-reference.md` for invocation and response keys. You collect
fields → render the prescribed card → confirm → invoke once → render the post-success template.
Never re-implement a rule table or reconstruct an id.

---

## 1. Role ask (do FIRST — `--role` is required by pre-check)

`agent pre-check` **requires** `--role`. If the role is clear, use it; otherwise ask once (accept a number or role name: 1 User / 2 ASP / 3 Evaluator; never default or guess). Then run §2.

> **CLI value is strict.** Always pass the canonical token `--role user` / `--role asp` / `--role evaluator`. The CLI rejects any other value (no `buyer` / `provider` / `requester` / numeric aliases). Map whatever the user typed — a number (1/2/3), a synonym in any language (buyer/seller/provider/merchant/client/卖家/服务提供商…), or a label — to one of these three **before** calling.

Display `user` / `asp` / `evaluator` as localized User / ASP / Evaluator; Chinese = 用户 / 服务提供商
/ 评审员. Never expose raw role enums, legacy role words, or bilingual labels.

### Evaluator legacy aliases

When the user's input names Evaluator with 仲裁者 / 仲裁员 / 评估者 / arbiter / arbitrator / assessor
or an equivalent legacy term, recognize `evaluator` and emit this once per session before continuing:

- Chinese: 你说的角色现在叫「评审员」，已按此为你处理。
- English: That role is now called Evaluator — proceeding.

Afterward, use only the localized Evaluator label. Always pass `--role evaluator`.

## 2. Pre-check (Gate — consent + uniqueness in one command)

Invoke the initial form from `identity-cli-reference.md` (internal — never shown). It fetches the wallet's agents; **if the wallet has agents it's already consented** (→ straight to the uniqueness verdict); **if it has none it runs the consent gate first**. **Never call `agent get-my-agents` or a standalone consent command for registration.** Branch on the stable response keys:

- **`consent` present** (always `canCreate:false`) → first-time wallet. Show `consent.terms` complete and translated (never summarized; never show `consentKey`). Present `1. Agree & continue` / `2. Decline & cancel`. `1` → re-run `agent pre-check --role <role> --consent-key <uuid>`; `2` → stop. Ambiguous → re-display once.
- **`canCreate:false`** (no `consent` field — a single-role identity already exists; `reason` explains) → do NOT create, do NOT offer "create new". Redirect to update with the mandatory per-wallet line, filling `<roleLabel>` / `<N>` / `<name>` from `existingSameRole[0]`:
  > "Under this wallet you already have a `<roleLabel>` identity #`<N>` (`<name>`). Each address can register only one `<roleLabel>` — say "update #`<N>`" to edit it, or keep using it. To register a separate one under a different address, switch / add a wallet first."
- **`canCreate:true`** → may register. ASP role with existing ASPs (K ≥ 1): K=1 → offer *1. New ASP / 2. Update #`<N>` (`<name>`)*; K ≥ 2 → list from `existingSameRole` by number (never auto-pick). If the user mentions fixing a rejected listing → steer to option 2 and hand off to `identity-update.md` (only create if user explicitly insists). K=0 / user/evaluator → §3.
- Proceed to the §3 field Q&A and eventually `create` — the CLI always returns `newAgentId` (string id on WS success, `null` on timeout).

**Passive need-user** (handed in from a task flow): skip the pre-check loop / photo entirely. See §8.

## 3. Field checklists (one line per field — limits are enforced by `validate-listing`, not by you)

Take `name` and `description` from the user's replies and `picture` from their uploaded image—never
from email, wallet, or session metadata. Restructure only their words; never invent capabilities,
metrics, or optional content.

**user / evaluator:**
- **Name** — required, from the user's literal reply this turn only (never from email / wallet name).
- **Profile photo** — optional; default if skipped (see §5).
- **Description** — do NOT prompt. If the user volunteers one, add a Description row to the card; otherwise omit the row and send `ProfileDescription:""` silently.

**ASP — two steps** (user may batch):
- **Step 1 · Identity** — Present all three as a **single numbered list in one message** (do NOT split into separate turns):
  1. **Name** — brand name (CN 2–12 chars / EN 3–25 chars; no test markers / celebrity names)
  2. **Description** — one-sentence summary of what the Agent does (required, ≤500 chars)
  3. **Avatar — required**: send an image file (§5).
- Complete the avatar flow in §5. Once all three Identity fields are ready, render the ASP
  **Identity card immediately**: Role / Name / Description / Profile photo, with the uploaded CDN
  URL (never `default`). Close it with localized `> Reply **1** to continue.` and wait. Reply **1**
  advances to Step 2 only; it never runs `agent create`.
- **Step 2 · Service:** load `identity-service-contract.md` and follow §Collect and route end to end.
  Follow its field order, batched-answer handling, matching type section, Add another / Done gate,
  and validation timing. Continue to §4 only after explicit Done.

## 4. QA via `validate-listing` (ASP only)

User and Evaluator skip. For ASP, after explicit Done load `identity-validate-listing.md` and follow Create
mode end to end. Continue to §7 only after it allows progression.

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
- **On opt-in:** Save the inbound image attachment to a temp path → invoke `upload` per `identity-cli-reference.md` → use the returned URL as `--picture`; >1 MB → stop and ask for a smaller one; render the URL verbatim in the Profile photo row. No image → keep default (user/evaluator only). 1:1 square is the tip.
- **Upload as-is — never resize/crop/convert.** >1 MB → ask for a smaller file; non-1:1 → accept and upload (square is advisory); non-PNG/JPEG/WebP → ask to convert and resend.

## 6. Endpoint anti-pattern (ASP A2MCP service)

Follow
[identity-service-contract.md §A2MCP endpoint](identity-service-contract.md#a2mcp-endpoint);
it is the sole endpoint-validation rule source.

## 7. Final confirmation card

Use `| Field | Value |`. Run `create` only after a new explicit confirmation; confirmation cannot be
skipped or reused from an earlier action.

user / evaluator render ONE card. For ASP, the Identity card was already rendered and confirmed in
§3 Step 1; do not render it again here. Render only the final Service card:

**Service card** (closes Step 2) — render ONE block of `Service [N] Name / Description / Type / Fee / Subscription / Free trial / Endpoint` rows **per collected service** (`Service [1]`, `Service [2]`, … — never assume a single service). Keep Type exactly `A2MCP` or `A2A`; render pricing/trial per `identity-service-contract.md` §Display and the matching type section. This is the FINAL card → its **1** runs the single `agent create` carrying the identity and every collected service.

Render Service guide per
[identity-service-contract.md §Display](identity-service-contract.md#display).

The FINAL card ends with `> Reply **1** to confirm and run.` (localized) + the gate echo: `I won't run anything until you reply **1**.` NL field questions only; no `Q1:` labels, no bash shown.

## 8. Passive need-user

Run `agent pre-check --role user` (consent + uniqueness gate, same as §2). On consent required → run full consent flow per §2. On `canCreate:false` (user already exists) → use the existing one, skip create entirely. On `canCreate:true` → ask name only (skip photo). Then render the card → on confirm, execute. Post-success is ONE line, **no detail card**:
> "User identity #`<id>` created. Resuming the task-publish flow."

(If a user already exists: "You already have a User identity #`<N>` (`<name>`) — using it to continue.") Hand back to the task flow with that single line; don't ask "want to publish a task?".

## 9. Execute

Invoke `create` once with the collected fields per `identity-cli-reference.md` (all values come from
§3). **On any non-success** → load `identity-errors.md`; never interpret a code inline.

## 10. Post-success templates

Resolve `#<id>` from the first available value: non-empty top-level `newAgentId`, then
`agent.agentId`. If neither exists, omit the id and use the fallback below. Never borrow a pre-check
id or emit a bare `#`.

- **user (ONE line)** — No txHash, no question. After emitting it, run the communication-init flow in [`chat-comm-init.md`](chat-comm-init.md) so the new agent can communicate (create has no CLI-level readiness gate).
  > User identity #`<id>` is live — say "publish a task for X" whenever you're ready and I'll take you through it.
- **ASP (ONE line)** — Never mention active clients / agent counts / re-list agents; never a numbered menu; never a duplicate line. After emitting it, run the communication-init flow in [`chat-comm-init.md`](chat-comm-init.md) so the new agent can communicate (create has no CLI-level readiness gate).
  > ASP identity #`<id>` registered — not yet visible to others. Say "activate #`<id>`" to publish now, "add a service to #`<id>`" to offer more services, or "find ASPs doing X" to check the market first.
- **evaluator (EXACTLY two lines)** — no stake number/amount, no trailing question, no detail card.
  > Evaluator identity #`<id>` registered.
  > A separate stake is still required before you can be assigned disputes.

  With a resolved `#<id>`, emit the two lines, run only [`task-core.md`](task-core.md) §Pre-flight,
  then follow the matching scenario in [`task-evaluator-staking.md`](task-evaluator-staking.md). Do
  not end on a question or detail card. Staking is post-create, never a pre-create gate; "don't want
  to stake" → register now, stake later; "have I staked?" → hand to staking flow.

If neither id field is available: user → omit `#<id>`; ASP → `Say "list my agents" to find your new identity, then "activate #<id>" to publish.` Evaluator → keep exactly two lines, do not enter staking, and use:
> Evaluator identity registered.
> A separate stake is still required. Say "list my agents" to resolve its ID, then "stake Evaluator #<id>" to continue.
