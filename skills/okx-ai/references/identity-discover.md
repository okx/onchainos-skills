# Discover — service-match · list my agents · detail · service-list

## Routing nuances (decide before calling)
- "my <descriptor> agents" / any ownership word → **list** = `agent get-my-agents` + client-side group/filter,
  NOT `service-match`. Explicit `#ids` ("detail #42", "#42 #58") → **detail** = `agent get-agents --agent-ids`, NOT service-match.
- Free-text "find agents/services doing X" → **service-match**.

For both service-rendering paths, apply visibility from
[identity-service-contract.md §Display](identity-service-contract.md#display) and raw service-ID
handling from
[identity-cli-reference.md §Read and discovery](identity-cli-reference.md#read-and-discovery).

---

## service-match — `agent service-match`

For the initial search, pass the user's original utterance verbatim to
`intent-keyword-extraction.md`, then use its output unchanged in
the initial `service-match` form from `identity-cli-reference.md` with `--limit 5`. Do not
preprocess or enrich the input or output.

### Initial-search argument example

The extraction object is internal; convert non-null fields to the flags in
`identity-cli-reference.md`, emit `--keywords` once, and preserve keyword order. Example extraction:

```text
# Extraction:
{"asp-agent-id":null,"asp-name":null,"service-name":null,"service-id":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["analyze this wallet","generate a report"]}

# CLI:
onchainos agent service-match --keywords "analyze this wallet" "generate a report" --limit 5
```

### Rendering (blocking)

Group `services[]` by `asp.aspAgentId`, preserving the returned Agent and Service order. Render a
full Markdown table for the **first returned Agent only**. After that table, render every remaining
Agent as one compact bullet under an "Other results" heading. The first Agent is determined solely
by returned order — never score, recommend, filter, or reorder Agents model-side.

Render these blocks in order. The first Agent's §service-list table goes between the two blocks:

```markdown
### <asp.aspName> (Agent ID: <asp.aspAgentId>) | Rating <asp.rating> | Sold Count <asp.soldCount>
```

Immediately render the §service-list table without its lead-in line, then render:

```markdown
### Other results

- **<asp.aspName> (Agent ID: <asp.aspAgentId>):** Rating <asp.rating>, Sold Count <asp.soldCount>; <service 1 name> — <service 1 description>, <service 1 price>[, Free trial <service 1 freeTrial>]; <service 2 name> — <service 2 description>, <service 2 price>[, Free trial <service 2 freeTrial>]
```

- **First Agent table:** keep the existing §service-list 8-column contents and presentation rules.
  Populate them from each Service's `serviceName`, `serviceType`, `feeAmount`, `feeTokenSymbol`,
  `subscription[]`, `freeTrial`, `endpoint`, and `serviceDescription`; number rows from 1. Apply the
  §service-list all-`—` column omission rule only to the other optional columns.
- **Other results:** emit one bullet per remaining Agent, in returned order. Within each bullet,
  append every returned Service in order as `name — description, price`, separated by semicolons.
  `price` is the populated Fee or Subscription value rendered per §service-list; append the localized
  Free-trial label and value only when present. Do not show Type, Endpoint, or other `—` placeholders
  in these compact bullets.
- The table's `#` column is only a display row number starting from 1.

Render the CLI-normalized `rating` directly.

### Pagination

After each page with `hasMore == true`, append a "more" prompt. On "more", invoke the continuation
form in `identity-cli-reference.md` with the returned `searchAfter` and `--limit 5`; do not repeat
initial-search filters. Apply this rule to every continuation page.

---

## list — `agent get-my-agents`

Rows arrive at `list[*]`; each row carries `accountName`, `ownerAddress`, and a ready `cells[]` (with
`roleLabel`/`statusLabel`/`ratingStars` already resolved). **Group by `accountName`** — one header + table
per group; render `cells` **verbatim** (no hand-mapped role/status integers or raw 0–100 score).

```
> Wallet <accountName> (<0x…short>)

| Agent ID | Name | Role | Status | Approval status | Rating |
|---|---|---|---|---|---|
| #<id> | <name> | <roleLabel> | <statusLabel> | <approval> | <ratingStars> |

> Total N wallets, M agents in all. Say "detail #42" to drill in.
```

- Rating renders the CLI's stars directly; no feedback → `No rating yet` (never `—`, never `92/100`).
- Footer counts: N = wrappers/accountNames, M = total agents. A wrapper with 0 agents → render `(no agents)`, not an empty table.
- **M ≥ 5 → append the reassurance footer**: the agents are theirs, spread across the
  user's own wallet accounts; if unremembered they're from past test runs / batch scripts; **the wallet is
  not compromised**; offer to deactivate any. Non-alarmist. Single-account variant (one wallet, M ≥ 5) drops
  the "across multiple wallets" clause. M < 5 → no footer.

---

## detail — `agent get-agents --agent-ids N`

Invoke `get-agents` per `identity-cli-reference.md`. The response is a flat array of agents (one per id), each carrying a ready `card[]` of `{label,value}` with `roleLabel`/`statusLabel`/`approvalLabel`
resolved — **identity rows only**. Render the `card` rows **verbatim**.
The agent-list card does **not** inline services or rating. **ASP → chain exactly ONE
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
  On `1` → hand to `identity-reviews.md` (feedback-list, one per selected agent, `---`-separated). On `2` → stop.
  If the user already named a subset ("reviews for 42 and 58"), skip the prompt → straight to those ids.

---

## service-list — `agent service-list --agent-id N`

Invoke `service-list` per `identity-cli-reference.md`. Render its single 8-column table with values
verbatim. Do not add a service-type gloss: display `A2MCP` / `A2A` exactly.

```
> Agent #<id> — <name> (<role label>) services:

| # | Name | Type | Fee | Subscription | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|---|
| 1 | <name> | <A2MCP or A2A> | <fee> | <subscription> | <free trial> | <endpoint> | <description> |

Do not append a service-type explanation or alias.
```

- `#` is a display-only row number starting from 1. Type per Lexicon: render only the exact raw
  value `A2MCP` or `A2A`; never translate or rewrite it.
- Omit any column whose values are all `—`; otherwise keep it and render missing values as `—`.
- **Fee / Subscription / Free trial:** render per `identity-service-contract.md` §Display. Its zero-price normalization is the sole price-value exception to verbatim rendering; otherwise render `cells` verbatim and never recompute prices.
  **Endpoint:** A2A always `—` (CLI clears it); wrap URLs in backticks so the table doesn't break.
- Values verbatim except the zero-price normalization above — don't normalize other odd shapes; truncate long descriptions with `…`, keep first sentence.
  If a value's shape diverges from the local schema (e.g. `serviceType: query`, fee in ETH), render it as-is
  and add a one-line footnote: looks like backend demo data — verify before integrating.
