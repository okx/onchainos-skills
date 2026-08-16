# Discover — service-match · list my agents · detail · service-list

## Routing nuances (decide before calling)
- "my <descriptor> agents" / any ownership word → **list** = `agent get-my-agents` + client-side group/filter,
  NOT `service-match`. Explicit `#ids` ("detail #42", "#42 #58") → **detail** = `agent get-agents --agent-ids`, NOT service-match.
- Free-text "find agents/services doing X" → **service-match**.

---

## service-match — `agent service-match`

For the initial search, pass the user's original utterance verbatim to
`intent-keyword-extraction.md`, then use its output unchanged in
`onchainos agent service-match <args> --limit 5`. Do not preprocess or enrich the input or output.

### Initial-search argument example

The extraction object is internal; convert non-null fields to CLI flags, emit `--keywords` once,
and preserve keyword order. For example:

```text
# Extraction:
{"asp-agent-id":null,"asp-name":null,"service-name":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["analyze this wallet","generate a report"]}

# CLI:
onchainos agent service-match --keywords "analyze this wallet" "generate a report" --limit 5
```

For continuation pages, use only `--search-after <searchAfter>` with pagination options; do not
repeat initial-search filters.

Group `services[]` by `asp.aspAgentId` and render one section per Agent:

```
### <asp.aspName> (Agent ID: <asp.aspAgentId>) | Rating <asp.rating> | Sold Count <asp.soldCount>
```

Use the §service-list 8-column table and localized presentation rules. Populate it from each
Service's `serviceName`, `serviceType`, `feeAmount`, `feeTokenSymbol`, `subscription[]`,
`freeTrial`, `endpoint`, and `serviceDescription`.

Render the CLI-normalized `rating` directly.

### Pagination

After each page with `hasMore == true`, append a "more" prompt. On "more", run exactly
`onchainos agent service-match --search-after <returned searchAfter> --limit 5`; do not repeat
initial-search filters. Apply this rule to every continuation page.

---

## list — `agent get-my-agents`

Rows arrive at `list[*]`; each row carries `accountName`, `ownerAddress`, and a ready `cells[]` (with
`roleLabel`/`statusLabel`/`ratingStars` already resolved). **Group by `accountName`** — one header + table
per group; render `cells` **verbatim** per identity-invariants.md §Verbatim-render contract (no hand-mapped
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

## detail — `agent get-agents --agent-ids N`

The response is a flat array of agents (one per id), each carrying a ready `card[]` of `{label,value}` with `roleLabel`/`statusLabel`/`approvalLabel`
resolved — **identity rows only**. Render the `card` rows **verbatim** (identity-invariants.md §Verbatim-render
contract). The agent-list card does **not** inline services or rating. **ASP → chain exactly ONE
`agent service-list --agent-id N`** and render the §service-list table beneath the card; user / evaluator
→ no chain. Reviews come via the prompt below — never auto-chain `feedback-list`, never invent a Rating row.

```
| Field | Value |
|---|---|
| <label> | <value> |   ← one row per card[] entry, in order
```

- **Multiple ids** (`#42 #58` → `--agent-ids 42,58`): one `card[]` per agent — render one card each in order,
  separated by `---`. Trigger on the **returned agent count** > 1 (the response is a flat top-level array — count its entries).
- After the card(s), offer reviews via ONE numbered prompt — do not auto-run (detail-card only; other references
  use a single suggestion line, never a menu):
  ```
  Want to see this agent's review details?
    1. Yes, pull the review list
    2. No, I'm good
  Reply 1 or 2.
  ```
  On `1` → hand to `identity-reputation.md` (feedback-list, one per selected agent, `---`-separated). On `2` → stop.
  If the user already named a subset ("reviews for 42 and 58"), skip the prompt → straight to those ids.

---

## service-list — `agent service-list --agent-id N`

Single 8-column table; values verbatim. Service-type gloss once per table (wording per identity-invariants.md §Lexicon).

```
> Agent #<id> — <name> (<role label>) services:

| # | Name | Type | Fee | Subscription | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|---|
| 1 | <name> | <localized type> | <fee> | <subscription> | <free trial> | <endpoint> | <description> |

> Service types: API service = pay-per-call, fixed price; agent to agent = per-call or monthly-subscription pricing (one or the other).
```

- `#` numbered from 1. Type per Lexicon (API service / agent to agent), never raw A2MCP/A2A.
- Omit any column whose values are all `—`; otherwise keep it and render missing values as `—`.
- **Fee / Subscription / Free trial:** render per identity-invariants.md §Lexicon (Fee / Subscription / Free trial rows) — the CLI does the mapping; render `cells` verbatim, never recompute.
  **Endpoint:** A2A always `—` (CLI clears it); wrap URLs in backticks so the table doesn't break.
- Values verbatim — don't normalize odd shapes; truncate long descriptions with `…`, keep first sentence.
  If a value's shape diverges from the local schema (e.g. `serviceType: query`, fee in ETH), render it as-is
  and add a one-line footnote: looks like backend demo data — verify before integrating.
