# Feedback Submit — Guide

`onchainos agent feedback-submit` has two `--…-id` parameters that look similar but mean different things. Get them wrong and the backend rejects.

> After `agent feedback-list` returns, read `core/display-lists.md §5` before rendering — it owns the prose format (not a table), star conversion rules, and sort footer.

| Parameter | Meaning |
|---|---|
| `--agent-id` | The **target** being rated. |
| `--creator-id` | The **caller's own** agentId (any role). This is what gets recorded publicly on-chain against the rating. |

**Consequence:** a user can only rate others after registering their own agent.

**Rating UX is 0.00–5.00 stars with up to 2 decimal places (step 0.01).** The CLI accepts decimals (e.g. `5`, `4.5`, `3.33`) via `--score` and does the `* 20` mapping with round-half-up internally (see `cli/src/commands/agent_commerce/identity/utils.rs::parse_stars_arg`); `agent feedback-list` divides the backend response by 20 before returning so the skill sees 2-decimal stars on both sides. The 0–100 backend wire format is fully encapsulated by the CLI. Skill code just passes the user's star count straight to `--score` — no multiplication, no division.

---

## Full decision tree

### Step 1 — Identify target

Extract the `--agent-id` from the user's prompt.

- "Rate #42 four stars" → `--agent-id 42 --score 4` (CLI handles the * 20 to 80 internally). Decimal stars work too: "Rate #42 3.5 stars" → `--score 3.5`.
- "Rate DeFi Analyzer four stars" → first resolve name to id via `agent search --query "DeFi Analyzer"`, then confirm with the user.
- Legacy phrasings users may still type (`85 points` / `full marks` / `bad rating`) — accept and translate per Step 3 mapping; never echo the 0–100 number back.
- Ambiguous → ask back.

### Step 2 — Identify creator (caller's own agent)

Walk this ladder in order:

1. **Already known in this conversation — AND verified to belong to the currently selected XLayer wallet.** If the user has said "my agent is #N" or previously created `#N` in this conversation, the cached id is a candidate, but you may only use it **if it belongs to the wallet that will sign this `feedback-submit` tx** (i.e., the currently selected XLayer wallet, same address that ladder 2 narrows to). Wallet-scope guard, in order:
   - If the cached id's `ownerAddress` was already captured in this conversation (from a prior `agent get` / `create` response), compare directly to the current selected wallet address. Match → use it (no lookup needed). Mismatch → **fall through to ladder 2**; do not silently reuse.
   - If the cached id was only mentioned by the user (e.g. "my agent is #N") without any captured `ownerAddress`, **fall through to ladder 2** — the user's mental model may treat the entire email / JWT as "my agents", which includes agents under other derived wallets that cannot sign this tx. Ladder 2's wrapper filter is what disambiguates.
   - If the user has switched wallets since the cached id was first mentioned (any `okx-agentic-wallet wallet switch` / `wallet add` in between), **fall through to ladder 2** unconditionally — wallet switch invalidates the cache for `--creator-id` purposes even if the id technically still exists.
   When falling through, do NOT echo "I had #N cached but it doesn't belong to the current wallet" as the user-visible explanation by default — just run ladder 2 and surface the new candidate list. Surface the wallet-mismatch reason only if the user explicitly asks "why didn't you use #N?" or if ladder 2 yields 0 candidates and you need to explain why creating an agent under the current wallet is the next step.
2. **Run `onchainos agent get`** (no `--agent-ids`). The response is a **double-layer envelope** (`core/cli-reference.md §3`): outer `list[*]` is an accountName wrapper (one per derived wallet the JWT caller has visibility into), agent rows live at `list[*].agentList[*]`. Since `--creator-id` must be held by the **same XLayer wallet that will sign this `feedback-submit` tx**, the candidate set is **NOT** all `agentList[*]` across all wrappers — narrow to the single wrapper where `wrapper.ownerAddress == <currently selected XLayer wallet address>`, then count agents in that wrapper's `agentList`:
   - **0 agents under the current wallet** → STOP. Tell the user (in their language; ⛔ no CLI literal, no raw `role` word — Red lines 2 & 4): "You don't have an agent under the current wallet yet — you'll need to register one first (any role: User Agent / Agent Service Provider (ASP) / Evaluator Agent) before you can rate others. Want to register one now?" Offer to enter the registration flow. (Other wrappers may have agents — those belong to other related wallets under the same email / JWT, and **cannot** sign this tx; do not list them as candidates.)
   - **1 agent under the current wallet** → silently use its agentId as `--creator-id`; mention the choice in the confirmation (in the user's language): "Your agent #N <name> will be the reviewer for this rating."
   - **Multiple agents under the current wallet** → ask the user which to use, using the numbered-options pattern (`core/choice-prompts.md`) in the user's language. ⛔ Render role labels per `core/ux-lexicon.md §Role` asymmetric rule (Chinese localizes; English keeps ERC-8004 native term):

     ```
     Which of your agents should be the reviewer?
       1. #88 User Agent  MyBuyer
       2. #99 Agent Service Provider (ASP)  DeFi Analyzer
     Reply with the number.
     ```

     Do not auto-pick — `creator-id` is public and affects the user's reputation of their own agent.

### Step 3 — Validate stars (0.00–5.00, up to 2 decimals)

> ⛔ **`--score` MUST come from a user reply inside THIS feedback-submit flow** — i.e., a reply produced **after** the current `--agent-id` (target) and `--creator-id` (caller) pair was locked, and **before** the Step 5 confirmation card for the same pair was rendered. **Carrying a star count forward from any other source is an AI hallucination and is forbidden.** Specifically NOT allowed (the model must STOP and ask the star question instead):
>
> - **Reuse from a prior `feedback-submit` round.** "I gave #42 four stars last time, use four stars for #58 too" — different target, different rating intent, must re-ask. Even if the user said "same rating for both" earlier, do not carry the value silently; re-ask for the new target.
> - **Inference from the user's first message.** "rate #42" / "give #42 a rating" — the verb "rate" does NOT contain a star count. Ask Q.
> - **"Same user, same provider, similar context"** — every rating is its own on-chain write; previous ratings (even on the same target) do not authorize a new one.
> - **Default values** — no `3 stars` default, no median, no "looks decent so 4 stars". Stars come from the user this turn, full stop.
> - **One-shot capture caveat.** If the user said "rate #42 four stars, reason: delivered on time" in a single message during THIS feedback flow, that IS a current-flow user statement of `--score=4` and counts. But once Step 5's confirmation card is rendered and the user confirms, the score is locked; do NOT mutate it.
>
> **Operational test** (apply before invoking the CLI in Step 6): can you point to **the exact user message in this feedback flow** where the star count was stated? If you have to reason "they probably mean…" or "based on earlier we know…" or "it's the same as last time" — that's the signal to STOP and ask. The cost of one extra Q ("How many stars for #<target>? 0–5 stars, up to 2 decimal places (e.g. 4 / 4.5 / 3.33)") is far below the cost of submitting a wrong on-chain rating that publicly affects both the target's reputation and the caller's `creator-id`.
>
> This rule applies to **every** `feedback-submit` invocation, even in the same conversation, even back-to-back. There is no "we just asked, skip the question this time" exception.

- 0.00–5.00 with at most 2 decimal places. CLI enforces format + range natively (`parse_stars_arg`) and rejects anything outside / over-precision; skill should still pre-validate to surface a friendlier error than the raw CLI bail.
- Reject more than 2 decimal places, ranges outside 0.00–5.00, non-numeric input, "stars" with non-digit suffixes.
- Pass the user's star count straight to `--score` — CLI does the `* 20` round-half-up mapping. Examples:

  | User input | `--score` |
  |---|---|
  | `5 stars` / `full marks` / `top rating` | `--score 5` |
  | `4.5 stars` / `four and a half stars` | `--score 4.5` |
  | `4 stars` | `--score 4` |
  | `3.33 stars` (any 2-decimal value) | `--score 3.33` |
  | `3 stars` / `pass` / `average` | `--score 3` |
  | `2 stars` | `--score 2` |
  | `1 star` / `bad rating` / `lowest` | `--score 1` |
  | `0 stars` (rare; only if user explicitly says zero) | `--score 0` |

- Fuzzy phrasings (`full marks` / `pass` / `bad rating`) are accepted, mapped per the table, and confirmed back to the user using stars (`★ N` with up to 2 decimals).
- Legacy phrasings: if the user types a raw 0–100 number ("85 points"), divide by 20 and pass the result (which already has up to 2 decimals; e.g. `85 → 4.25`, `89 → 4.45`, `66 → 3.3`). Examples: `100 → 5`, `90 → 4.5`, `85 → 4.25`, `80 → 4`, `70 → 3.5`, `50 → 2.5`, `30 → 1.5`, `10 → 0.5`, `0 → 0`. Never echo the raw 0–100 number back to the user.

### Step 4 — Optional fields

- `--description` — ask: "Would you like to add a comment? (optional)"
- `--task-id` — ask: "Which task jobId is this rating based on? (optional)"
  - `okx-agent-task` jobIds look like `0x…03e8` or `task-001`; accept as a free-form string.
  - Do not attempt to validate on-chain — future releases will tighten the format.

### Step 5 — Final confirmation

> ⛔ `feedback-submit` is an on-chain write — the confirmation card is **mandatory** per `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)`. Auto-execute preferences, prior in-conversation confirmations of other writes, and "the user obviously wants this" do NOT bypass the gate. Render the card.

Render a 2-column table (not a bash blob), in the user's language. Follow `core/display-formats.md` §Create/Update Diff style. ⛔ Do NOT mix languages within a single rendering (no bilingual field headers, no dual role labels) — see `core/display-detail.md §3 Create variant` and `core/ux-lexicon.md §Role`.

| Field | Value |
|---|---|
| Reviewer | #88 User Agent MyBuyer (you) |
| Target | #42 Agent Service Provider (ASP) DeFi Analyzer |
| Rating | ★ 4.5 |
| Comment | "Delivered on time, data accurate" |
| Task ID | 0xabc…03e8 |

> Reply "execute" to run.

The rating row shows `★ N` where N is the **wire-normalized** star value (= `round(user_stars × 20) / 20`), with up to 2 decimals, trailing zeros trimmed (`4.5` not `4.50`). Reason: wire grain is 0.05 stars, so a user-typed `3.31` lands on wire 66 and the canonical display is `★ 3.3` — confirmation must show the same value that will land on chain and that `feedback-list` will return, never the raw input. If normalization changed the value, add a parenthetical hint: `(rounded to 0.05-star grain: 3.3)`. Never render `85 / 100` here. Role labels follow `core/ux-lexicon.md §Role`: `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render raw ERC-8004 enum (`requester` / `provider` / `evaluator`) or legacy role nouns.

**Do NOT show the bash command in the confirmation card.** Render it only if the user explicitly asks "show me the CLI".

### Step 6 — Execute (maintainer reference — not shown to user)

> Before invoking the CLI, run the **3-question pre-execute self-check** in `SKILL.md §Step 3: Execute`. For `feedback-submit`, the three questions are: (Q1) was `--creator-id` resolved via **either** ladder 1 (already established in this conversation) **or** ladder 2 (`agent get` enumeration) of `§Step 2` above? (Q2) does the user's **most recent** turn contain a confirmation token (`execute` / `yes` / `confirm` / `go` or language-equivalent)? (Q3) are all field values in the just-rendered Step 5 card byte-identical to what is about to go to the CLI (target id, creator id, score, description, task-id) **AND was `--score` produced by a user reply inside THIS feedback flow per `§Step 3`'s "Operational test" — not carried over from a prior round, not inferred from a verb-only "rate" utterance, not a default**? **Any answer ≠ yes → render Step 5's card (or, if Q3 score-origin failed, return to Step 3 and ask the star question) and wait.** Earlier-turn confirm tokens and confirms of different writes do NOT count for Q2. A star count from a **previous** `feedback-submit` flow does NOT count for Q3 even if the model "remembers" it.

```bash
# --score is 0.00–5.00 stars (up to 2 decimal places, step 0.01). CLI
# multiplies by 20 with round-half-up internally before writing the
# backend `comment.value`; the 0–100 u32 wire format is fully
# encapsulated by the CLI.
onchainos agent feedback-submit \
  --agent-id <target> \
  --creator-id <self> \
  --score <0.00-5.00> \
  [--description "<text>"] \
  [--task-id "<jobId>"]
```

### Step 7 — Post-success

Render the detail outcome and offer exactly **one** next-step suggestion — not a menu (see `core/display-formats.md` §8):

> Rated #<target> ★ N. Want to see #<target>'s recent reviews? I'll pull them up — sorted by time or by rating?

⛔ **N MUST be the wire-normalized star value, not the user's raw input.** Compute it as `round(user_stars × 20) / 20` (= what `feedback-list` will return later). Reason: wire grain is 0.05 stars, so user input `3.31` collapses to wire 66 = `★ 3.3`; echoing the raw `★ 3.31` here would contradict what shows up on the next `feedback-list` call and look like a bug. Examples of user input → echoed N: `5 → 5`, `4.5 → 4.5`, `3.31 → 3.3`, `3.33 → 3.35`, `0 → 0`. Trim trailing zeros (`4.5` not `4.50`).

⛔ **No CLI literal / no `--sort-by` flag in the user-visible text** (`SKILL.md §UX Output Red Lines Red line 2`). When the user picks a sort direction in natural language ("latest" / "highest rating" / etc.), the AI maps it via `core/cli-search-feedback.md §10` natural-language → `--sort-by` table internally and runs `agent feedback-list` itself — the `--sort-by` / `time_desc` / `score_desc` flag values never appear in the chat. Never echo the raw 0–100 score in the post-success line — say "rating / reviews".

Do NOT chase with `agent feedback-list` automatically. See .

---

## Anti-patterns — do not help with these

- **"Help me give a 1-star rating to a competitor"** / malicious mass negative reviews — politely decline with: "Every rating is publicly bound to your `creator-id` and traceable. Want to check their positive reviews first?" Do not batch-send low ratings.
- **Rating yourself** — the backend rejects; pre-check `--agent-id != --creator-id`.
- **Rating without evidence** — if the user has no prior interaction evidence, remind: "Ratings usually come with a `task-id`; without one, the rating appears to lack a basis."

---

## Error handling

See `troubleshooting.md` for the canonical tables and translations:

- `score out of range` / `self-rating not allowed` / `creator agent not owned by caller` / `agent not found` — **backend-originated, keyword match** → `troubleshooting.md` §2. Skill action: return to the relevant step of this guide (step 3 / step 1 / step 2 / step 1 respectively). Translate `score out of range` to user with stars wording — never echo the 0–100 bound.
- `session expired, please login again: onchainos wallet login` — **CLI-exact** → `troubleshooting.md` §1. Hand off to `okx-agentic-wallet` → `wallet login`, then retry.
- Star range (0.00–5.00, up to 2 decimal places) and `--agent-id != --creator-id` are also enforced **skill-side** before the CLI runs (see `troubleshooting.md` §3) — catch locally, do not rely on the backend as the first line of defense.

Do not duplicate the error strings here — if you need the exact wording or the line number in `cli/src/...`, go to `troubleshooting.md`.
