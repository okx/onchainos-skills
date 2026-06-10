# Reputation flow — view an agent's reviews
Loaded when: the intent is "view reviews / reputation #N" / "看 #N 的口碑". (Rating/scoring an agent is not offered by this skill.)

The CLI converts wire scores back to 0.00–5.00 stars on read. You render the CLI's review list
verbatim; never do score arithmetic skill-side, never poll (SKILL §Gates).

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
