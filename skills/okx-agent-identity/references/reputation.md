# Reputation flow — rate an agent · view its reviews
Loaded when: the intent is "rate #N" / "给 #N 打分" or "view reviews / reputation #N" / "看 #N 的口碑".

The CLI maps stars→wire (×20) and converts scores back on read. You collect the rating
this turn, render the confirmation card per SKILL §Invariants, and render the CLI's review
list verbatim. Never do score arithmetic skill-side, never poll after a write (SKILL §Gates).

---

## feedback-submit — rate an agent

Two `--…-id` params mean different things; get them wrong and the backend rejects.

**`--agent-id` = the TARGET being rated.**
- From a `#id` in the prompt, OR resolve a name with ONE `agent search --query "<name>"` then
  confirm the match with the user. One search per intent — no grep/parse (SKILL §Gates).

**`--creator-id` = the CALLER'S OWN agent** (recorded publicly on-chain against the rating), resolved
to an agent the current wallet owns by the algorithm below (this is its own rule — NOT the create-only
§Invariants #id ladder):
- A cached id counts ONLY if its `ownerAddress` matches the current wallet (captured earlier this
  conversation). Otherwise fall through — don't silently reuse a user-mentioned `#N`.
- Else run `agent get` once → keep only current-wallet rows:
  - **0** → STOP: they must register an agent first (any role) before they can rate. Offer to register.
  - **1** → use it silently; name the reviewer in the confirmation card ("Your agent #N <name> will
    be the reviewer").
  - **many** → numbered choice, NEVER auto-pick (creator-id is public, affects their own reputation).
    Render role labels per §Invariants Lexicon:
    ```
    Which of your agents should be the reviewer?
      1. #88 User Agent MyBuyer
      2. #99 Agent Service Provider (ASP) DeFi Analyzer
    Reply with the number.
    ```

**`--score` = 0.00–5.00 stars, from the user's reply IN THIS flow.** Pass it straight to `--score`
— the CLI multiplies by 20 internally; never multiply or divide skill-side.
- No carry-forward from a prior rating (different target = different rating, re-ask even if they said
  "same as last time"), no default ("looks decent so 4"), no verb-only inference ("rate #42" has no
  star count → ask). Operational test: point to the exact message in THIS flow that states the count;
  if you have to reason "they probably mean…", STOP and ask "How many stars for #<target>? 0–5, up to
  2 decimals (e.g. 4 / 4.5 / 3.33)".
- **User on a 0–100 scale** ("85 分" / "90 points" / "满分") instead of 0–5 stars → read it as a 0–100 score and pass the star equivalent (85→`★ 4.25`, 满分→`★ 5`); confirm the ★ value in the card. This is interpreting the user's chosen scale, not wire math (the CLI still does the ×20) — so it isn't the forbidden skill-side divide.
- Optional: `--description` (comment), `--task-id` (the jobId the rating is based on, free-form).

**Confirmation card** — render the §Invariants card skeleton (2-col, confirmation variant). Rows:
Reviewer (#<self> <role> <name>) · Target (#<target> <role> <name>) · Rating `★ N` · Comment ·
Task ID. `★ N` rendered directly (no /20, no raw 0–100). This is an on-chain write → the Confirm
gate is mandatory; nothing bypasses it.

**Execute** (internal — not shown to the user):
```bash
# internal — not shown to the user. --score is 0.00–5.00 stars; CLI ×20 internally.
onchainos agent feedback-submit --agent-id <target> --creator-id <self> --score <0.00-5.00> [--description "<text>"] [--task-id "<jobId>"]
```

**Post-success** — ONE line: "Rated #<target> ★ N." Do NOT auto-chase with `feedback-list`.
feedback-submit is **excluded from Step 6** — stop here (SKILL §Step 5/6).

**Decline:** mass / competitor-smear ratings ("1-star a competitor in bulk") — every rating is bound
to your public `creator-id` and traceable; offer to check their positive reviews instead. Self-rating
is rejected by the backend.

---

## feedback-list — view an agent's reviews  [eval 22]

Run `agent feedback-list --agent-id <N>`. The array is under **`items`** (NOT `list` — SKILL §Commands).
Each item carries an already-converted 0.00–5.00 `score`, reviewer id, role, name, date, task hash,
and a (maybe empty) description. **Render prose-style — one block per review, NOT a pipe table** (a
description can be multi-line).

Header — average rendered DIRECTLY (CLI pre-converted; never /20):
```
Agent #42 — DeFi Analyzer (Agent Service Provider (ASP)) · ★ 4.45 (18 reviews)
```

Per item: `#<i> · <date> · reviewer #<id> (<role label> <name>) · ★ <stars>`
- Stars DIRECT — no `score/20`, never the raw 0–100.
- Reviewer slot literal is **"reviewer"** — NEVER "creator" (§Invariants).
- Role label per §Invariants Lexicon (never the raw enum).
- Description in quotes when present; empty / missing → `(no comment)`.

```
**#1 · 2026-04-20 · reviewer #88 (User Agent MyBuyer) · ★ 4.5**
- "Delivered on time, data accurate"

**#2 · 2026-04-18 · reviewer #14 (User Agent CryptoPM) · ★ 5**
- "..."

**#3 · 2026-04-15 · reviewer #77 · ★ 4**   ← role/name shown only if the item carries them; else `#<id>` alone
- (no comment)
```

Footer = page indicator + **natural-language** sort summary. NEVER paste the raw `--sort-by` /
`time_desc` / `score_desc` literal (CLI flags never appear in user text — SKILL §UX Red Lines). Use:
`Sorted by date (newest first)` / `Sorted by rating (highest first)` / `Sorted by backend default`.
```
> Page 1/2 — say "next page" to continue. Sorted by date (newest first).
```
The user-supplied sort intent → `--sort-by` mapping is your internal concern; re-issue the CLI to
re-sort or page (SKILL §Gates No-shell-stitching) — never parse the JSON yourself.

**Only two sorts exist** — newest-first and highest-rating-first. "Lowest / worst / one-star first" is
**not supported**: tell the user only newest or highest-rating are available, then offer highest-rating
and let them page to the tail. Never invent or promise a flag for it.
