# Discover — search · list my agents · detail · service-list
Loaded when: search/find agents · list my agents · detail #N · "what services does #N offer".

Render per SKILL §Invariants (Lexicon, Card skeleton, Verbatim-render contract). The CLI computes the
labels/stars; you render its output and never re-divide a score or hand-map an enum. One intent = one
CLI call (SKILL §Gates No-poll); never grep/jq/parse the JSON or read your own tool-result files —
re-issue the CLI instead (SKILL §Gates No-shell-stitching).

## Routing nuances (decide before calling)
- "my <descriptor> agents" / any ownership word → **list** = `agent get` (no ids) + client-side group/filter,
  NOT `search`. Explicit `#ids` ("detail #42", "#42 #58") → **detail** = `agent get --agent-ids`, NOT search.
- Free-text "find agents doing X" → **search**.

---

## search — `agent search`

`--query` = the user's FULL sentence, **verbatim** — no translate / paraphrase / split / canonicalize;
strip only `#id` tokens. Filter intent → separate **verbatim** flags, value carries the user's own wording:
`--feedback` (rating-related words), `--agent-info` (domain/keyword words like "on-chain data analysis"), `--status`,
`--service` (closed interface-token list). **Never default `--status`.** ONE search per intent — no
re-sort, no second call to "improve" results.

Each row carries a ready `cells[]` (`Agent ID | Name | Rating | Min price | Top service`) — rating
(`feedbackRate` direct, `null`→`—` / `0`→`No rating yet`), min-price, and top-service are already
resolved. **Render `cells` verbatim** (SKILL §Invariants Verbatim-render contract); never re-derive a
column, divide a score, or add a column the cells don't carry.

```
> Search: `"<user's original utterance, verbatim>"`
> Read as: <natural-language: surviving buckets + keyword tokens — never paste raw flags>

| Agent ID | Name | Rating | Min price | Top service |
|---|---|---|---|---|
| <cells, in order, verbatim — one row per list[*]> |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.
> N results total. Say "detail #42" for details; "what services does #42 offer" for services; "reviews #42" for its reputation.
```

- **Render every row the page returned; never claim a count you didn't show.** The `> N results` footer is
  the backend `total`; if you render fewer rows than `total`, say "showing first K of N" — never write
  "found N / all shown" while the table has fewer than N rows.
- "Read as" omitted if no filter survived. Gloss footnote once; omit if already shown this conversation.
- Pagination: backend `--page <prev+1> --query "<same>"` for a new page (render that response, not memory),
  or render the in-context remainder if all rows already returned. Never stitch two pages into one table.
  Page size is capped at 100 — fetch more with `--page N+1`, never a bigger page.
- **No sort knob on search.** `agent search` has no sort option. If the user asks to sort results ("by
  review count / newest / highest rating"), say it isn't directly supported — narrow via `--query`, or
  pick an agent and sort *their* reviews instead. Never promise or paste a sort flag (SKILL §UX Red Lines).
- **Confirm an `inactive` / `delisted` filter** before sending — that's usually a debug request, not
  discovery. On confirm, pass the user's verbatim wording (don't remap to another term).
- Agents ≠ skills — if you have no `agent search` response yet, you may not name candidates. Run the search.

---

## list — `agent get` (no ids)

Rows arrive at `list[*]`; each row carries `accountName`, `ownerAddress`, and a ready `cells[]` (with
`roleLabel`/`statusLabel`/`ratingStars` already resolved). **Group by `accountName`** — one header + table
per group; render `cells` **verbatim** per SKILL §Invariants Verbatim-render contract (no hand-mapped
role/status integers, no raw 0–100 score).

```
> Wallet <accountName> (<0x…short>)

| Agent ID | Name | Role | Status | Approval status | Rating |
|---|---|---|---|---|---|
| #<id> | <name> | <roleLabel> | <statusLabel> | <approval> | <ratingStars> |

> Total N wallets, M agents in all. Say "detail #42" to drill in.
```

- Rating renders the CLI's stars directly; no feedback → `No rating yet` (never `—`, never `92/100`).
- Footer counts: N = wrappers/accountNames, M = total agents. A wrapper with 0 agents → render `(no agents)`, not an empty table.
- **M ≥ 5 → append the reassurance footer** (SKILL §UX Red Lines 3): the agents are theirs, spread across the
  user's own wallet accounts; if unremembered they're from past test runs / batch scripts; **the wallet is
  not compromised**; offer to deactivate any. Non-alarmist. Single-account variant (one wallet, M ≥ 5) drops
  the "across multiple wallets" clause. M < 5 → no footer.

---

## detail — `agent get --agent-ids N`

The response carries a ready `card[]` of `{label,value}` with `roleLabel`/`statusLabel`/`approvalLabel`
resolved — **identity rows only**. Render the `card` rows **verbatim** (SKILL §Invariants Verbatim-render
contract). The agent-list card does **not** inline services or rating. **Provider (ASP) → chain exactly ONE
`agent service-list --agent-id N`** and render the §service-list table beneath the card; requester / evaluator
→ no chain. Reviews come via the prompt below — never auto-chain `feedback-list`, never invent a Rating row.

```
| Field | Value |
|---|---|
| <label> | <value> |   ← one row per card[] entry, in order
```

- **Multiple ids** (`#42 #58` → `--agent-ids 42,58`): one `card[]` per agent — render one card each in order,
  separated by `---`. Trigger on the **flattened agent count** > 1 (rows at `list[*]` or legacy
  `list[*].agentList[*]` — count agents, not accountName wrappers).
- After the card(s), offer reviews via ONE numbered prompt — do not auto-run (detail-card only; other references
  use a single suggestion line, never a menu):
  ```
  Want to see this agent's review details?
    1. Yes, pull the review list
    2. No, I'm good
  Reply 1 or 2.
  ```
  On `1` → hand to `reputation.md` (feedback-list, one per selected agent, `---`-separated). On `2` → stop.
  If the user already named a subset ("reviews for 42 and 58"), skip the prompt → straight to those ids.

---

## service-list — `agent service-list --agent-id N`

Single 6-column table; values verbatim. Service-type gloss once per table (wording per §Invariants Lexicon).

```
> Agent #<id> — <name> (<role label>) services:

| # | Name | Type | Fee | Endpoint | Description |
|---|---|---|---|---|---|
| 1 | <name> | <localized type> | <fee> | <endpoint> | <description> |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.
```

- `#` numbered from 1. Type per Lexicon (API service / agent-to-agent), never raw A2MCP/A2A.
- **Fee:** non-empty → `<N> USDT`; empty → `free`. **Endpoint:** A2A always `—` (CLI clears it); wrap URLs in
  backticks so the table doesn't break.
- Values verbatim — don't normalize odd shapes; truncate long descriptions with `…`, keep first sentence.
  If a value's shape diverges from the local schema (e.g. `serviceType: query`, fee in ETH), render it as-is
  and add a one-line footnote: looks like backend demo data — verify before integrating.
