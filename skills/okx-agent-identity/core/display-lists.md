# Display Formats — Lists & Search

> Supplement to `core/display-formats.md`. Contains §5 Feedback list and §6 Search results.
> The global rendering rules (table convention, service-type Pattern B, URL rule, `#<id>` placeholder) are defined in `core/display-formats.md` and apply here too.

## 5. Feedback list — `agent feedback-list --agent-id <id>`

Header line and one entry per review. Prose-style, not a table — the description can be multi-line. Role labels follow `core/ux-lexicon.md §Role`: `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. The **review description** is the reviewer's own free text — render verbatim regardless of viewing-user language.

> Agent #42 — DeFi Analyzer (Agent Service Provider (ASP)) · ★ 4.6 (18 reviews)

**#1 · 2026-04-20 · reviewer #88 (User Agent MyBuyer) · ★ 5**
- task: `0xabc…03e8`
- "Timely delivery, accurate data"

**#2 · 2026-04-18 · reviewer #14 (User Agent CryptoPM) · ★ 5**
- "Good analysis, but response time could improve."

**#3 · 2026-04-15 · reviewer #77 (ASP DataCo) · ★ 4**
- (no comment)

> Page 1/2 — say "next page" to continue. Sorted by date (newest first).

Rules:

- Header mirrors the detail card's rating summary line — `★ <average> (<count> reviews)`, where `<average>` is the **already-converted 2-decimal star float** returned by `agent feedback-list` (CLI's `utils::convert_feedback_list_scores` maps backend 0–100 → 2-decimal stars before responding; the skill renders directly without dividing again).
- Each review's user-visible template: `#<index> · <date> · <reviewer-label> #<id> (<role> <name>) · ★ <stars>`, where `<stars>` is the **already-converted 0.00–5.00 float (up to 2 decimal places)** returned in each item's `score` field. Skill renders the value directly — no `score / 20` arithmetic here, no integer-bucket rounding. The conversion lives in `utils::convert_feedback_list_scores`. Trailing zeros are trimmed in display (`4.5` not `4.50`). Never render the raw 0–100 number. ⛔ The `<reviewer-label>` slot is `reviewer`, NOT the literal English word `creator`. The `<role>` slot follows `core/ux-lexicon.md §Role`: `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render the raw ERC-8004 enum (`requester / provider / evaluator`). See the worked example above — that is the canonical rendering; the template here is just a schematic.
- Optional `task:` row shows the jobId in backticks; omit if absent.
- Description in quotes when present. When the field is empty / missing, render the placeholder `(no comment)`.
- Footer: page indicator and **natural-language sort summary**. ⛔ **Never paste the raw `--sort-by` flag or its `time_desc` / `score_desc` literal into the footer** (CLI flags must never appear in user-visible text). Render instead: `Sorted by date (newest first)` / `Sorted by rating (highest first)` / `Sorted by backend default`. The mapping between user-supplied sort intent ↔ `--sort-by` flag value is the AI's internal concern (see `core/cli-reference.md` §10) and never appears in the chat.

---

## 6. Search results

> Search: `"find a highly-rated provider doing on-chain data analysis"`
> Read as: highly-rated and keywords "provider" / "on-chain data analysis"

| Agent ID | Name | Rating | Min price | Top service |
|---|---|---|---|---|
| #42 | DeFi Analyzer | ★ 4.6 | 10 | TVL Query (API service, 10 USDT) |
| #77 | On-chain Insights | ★ 4.5 | — | Chain Analytics (agent-to-agent, free) |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.
> N results total. Say "detail #42" for details; "what services does #42 offer" for services; "rate #42 N stars" to rate.

### Field mapping (P0 — every cell MUST come from the named backend field)

`agent search` response shape per `core/cli-search-feedback.md §7` (NOT the same as `agent get` §3). Each row in the user-facing table corresponds to one element of the backend `list[*]`. Bind columns **strictly** to the named fields below — do NOT invent columns, do NOT cross-row-copy a value, do NOT fabricate a number when the field is `null` or missing.

| Column | Source field (within agent_row) | Render rule |
|---|---|---|
| `Agent ID` | `agentId` | `#<id>` (verbatim) |
| `Name` | `name` | Truncate to 20 chars with `…` if longer |
| `Rating` | `feedbackRate` | `★ <feedbackRate>` (already a 0–5 float — render directly, NO `/20`); `null` → `—`; **`0` → `No rating yet`** (score of 0 means no feedback submitted yet, not a zero-star rating — never render `★ 0`) |
| `Min price` | `serviceMinPrice` | Bare number — `<serviceMinPrice>`; `null` or missing → `—`. ⛔ **Do NOT hardcode "USDT"** and **do NOT borrow a unit from `services[*].feeToken`** — `serviceMinPrice` is a Double with no associated token symbol at agent level, and an agent's services may use different `feeToken` values per row (the "lowest" service is by min(feeAmount across mixed tokens), not necessarily `services[0]`, and there is no backend-guaranteed common unit). Inferring a unit from another field is the same cross-field fabrication anti-pattern banned for `profileDescription` cross-row copy. If the user needs the unit, invite them to drill into `§2` detail (which renders each service's `feeAmount` and `feeToken` verbatim). |
| `Top service` | `services[0]` → `serviceName` + **localized** `serviceType` + `feeAmount` + `feeToken` | Cell format: `<serviceName> (<localized serviceType>, <feeAmount> <feeToken>)`. ⛔ **`serviceType` MUST be rendered via `core/ux-lexicon.md §Service-type` short-form mapping** — `A2MCP` → "API service"; `A2A` → "agent-to-agent". **The raw enum `A2MCP` / `A2A` NEVER appears in user-visible text**, period — see top-of-file "Service-type rendering" rule. (There is no "after gloss has been shown" carve-out; the gloss footnote is rendered ON FIRST appearance of the localized short form, after which the localized short form continues to be the canonical output — never the raw enum.) Example (feeToken=USDT): `TVL Query (API service, 10 USDT)`; example (feeToken=ETH): `TVL Query (API service, 0.005 ETH)`. **The unit comes from `services[0].feeToken` verbatim** — do NOT substitute "USDT" when the backend returned something else (same "render verbatim from backend" rule as §4). `services` key absent (per `@JsonInclude(NON_NULL)` — see `core/cli-search-feedback.md §7`) OR `services[]` empty → `—`. Truncate the full cell to ≤ 40 chars with `…`. |

⛔ **Columns explicitly forbidden in the default search-result table** (the backend does NOT return these on `agent search`):
- `Role` — search response has no `role` field. `categoryCode` is a domain tag (e.g. `["FINANCE"]`), NOT the role enum.
- `Status` — search response has no `status` field. `onlineStatus` is a different signal (presence/heartbeat) and is not the on-chain activate/deactivate state.
- `Description` — keep it for the §2 detail card; on the §6 search-result table it forces over-long rows and was the surface that AI fabricated identical values across rows (see "Search-result anti-pattern audit" below).
- `Endpoint` — service detail, not search summary.

If you find yourself wanting one of these, the user is asking for **detail** — render §2 instead by running `agent get --agent-ids <N>`.

⛔ **Fabrication anti-patterns (P0, zero-tolerance):**
- Repeating the same `profileDescription` across multiple rows (copy-from-first-row failure mode).
- Inventing a number for `feedbackRate` / `serviceMinPrice` / `feeAmount` when the field is `null`. Render `—` instead.
- Inferring a `role` / `status` value when the field doesn't exist in the response. Drop the column entirely.

### Other rendering rules

- Echo the `Search:` line so the user sees what query produced the result. The **query value inside the quotes stays the user's original utterance verbatim** (verbatim passthrough rule: do not translate or canonicalize).
- Render the follow-up "Read as" line in **natural language** — list the buckets (reputation / volume / price / status) and the surviving keyword tokens; **⛔ do NOT paste raw CLI flag names like `--feedback` / `--agent-info` / `--service` / `--status`** (CLI flags must never appear in user-visible text). If no filter survived filter-extraction rules, omit the second line entirely; just show `Search:`.
- `Top service` = first service returned by backend; keep it short (≤ 40 chars; truncate with `…`).
- Inactive-agent filtering is decided by the backend based on `--status` filter; the skill does not post-filter rows. Surface whatever rows the backend returned.

### Display Completeness — backend pagination vs AI-side truncation

There are **two distinct truncation cases**; they have separate rules. Confusing them is the root cause of the "AI says total 14, all displayed, but only 3 rows actually rendered" failure.

**Case A — Backend pagination** (`envelope.total > page_size`):
The backend itself returned only a page. The skill renders that page's rows and appends the pagination footer (`Page <page>/<total_pages> — say "next page" to continue.`). This case is already documented above in §1 footer rules.

**Case B — AI-side truncation** (`envelope.total ≤ page_size` AND backend returned all rows in this single response, but the AI chooses to render only a subset for brevity):

The full list is in the skill's context (CLI returned all `N` rows in one response). AI rendering K rows where K < N is a **voluntary skill-side compression** — must be signalled explicitly.

- **Option ①** (recommended default): render all `N` rows. The user came here to discover and the cost of more rows is a few hundred tokens.
- **Option ②** (only when N is large, e.g. > 8): render **the first K rows in the backend response order**. ⛔ The skill MUST NOT skill-side re-sort the list. The backend already ranks search results by its own relevance signal; AI re-sorting (a) creates ties / inversions the user can't see the rationale for, and (b) is per-row-key-picking when fields are partially null, which is not a comparable total order. ⛔ There is **no sort knob** on `agent search` — `core/cli-search-feedback.md §7` shows no `--sort-by`, and the four filter flags (`--feedback / --agent-info / --status / --service`) are **keyword filters** (verbatim user tokens passed to backend's relevance ranker), **not sort directives**. If a user says "sort by rating", do NOT promise a "different CLI call with a sort flag" — that flag does not exist. Instead, narrow the result set with a more specific `--query` (e.g. add the user's quality cue as part of the natural-language query so the backend ranker weights it) and let the user page through, or invite them to look at specific rows via `agent get --agent-ids`. After picking the first K, MUST append:

  ```
  > Showing first K (in backend's returned order), N total. Say "more" / "show all" /
  > "expand" for the remaining N-K, or "detail #<id>" to drill into a specific one.
  ```

### Dispatch: "more" / "next page" intents (P0)

User-intent keywords — `next page / more / show all / expand / continue` — **do NOT individually disambiguate case**. The disambiguator is **the state of the most-recent `agent search` tool-call response in context**. Branch on that state first:

| State (from most-recent `agent search` response) | Case | Path |
|---|---|---|
| `envelope.total > envelope.pageSize` — more pages exist server-side | **A — Backend pagination** | Issue a **new** CLI call: `onchainos agent search --query "<same>" --page <prev+1> --page-size <same>`. Render the new response's `list[*]` via the §6 Field-mapping table. ⛔ Do NOT render rows from memory of an earlier turn — memory of a JSON response degrades silently across turns; the new CLI call is the only authoritative source for page `N+1`. |
| `envelope.total ≤ envelope.pageSize` AND prior turn used Option ② (rendered top `K` < `N` for brevity) | **B — Cross-turn truncation** | Render `list[K..N]` from the **already-captured response still in context** — those rows ARE in the response, you chose not to print them before. ⛔ Do NOT re-issue the CLI call here — the data is already in your context; re-issuing wastes a round-trip. |
| `envelope.total ≤ envelope.pageSize` AND prior turn already rendered every row (`K == N`) | **Neither — nothing more exists** | Reply "those are all N results above" — but only when on-screen `agentId` count actually equals `envelope.total`. Do NOT silently claim "all displayed" when the count doesn't match. |

This dispatcher is the **single source of truth** for "more"-class intents on `agent search` output.

⛔ **Universal forbidden patterns (apply in both cases):**
- Saying "all displayed" while on-screen `agentId` count `< envelope.total` — self-contradictory; the user can count.
- Emitting "I'll summarize: total N agents" with **zero new `agentId`s** rendered — no-progress turn; almost always means fabrication is the next move.
- Cross-page stitching: concatenating `page N` and `page N+1` (from memory or from two CLI calls) into one combined table before showing the user. Boundary errors (duplicate / missing ids at the page split) are nearly guaranteed. Let the user keep paging.
- Reading own session log / writing `/tmp/parse.sh` / `grep -A N "agentId"`-style bash parsers (shell-stitching is forbidden).

**Self-test before emitting any "more"-intent response:** for each rendered row, can I quote a **specific** `agentId` AND name **which tool-call response it came from**? For Case A specifically, does that response's `page` field equal the page the user just asked for? If any answer is no, the response is not grounded — re-evaluate which case applies and follow that path.

### Search-result anti-pattern audit (zero-tolerance failures)

| Anti-pattern | Why forbidden |
|---|---|
| `"found N agents"` and `"all shown on page 1"` while on-screen rows < N | Self-contradictory; user can count |
| `"other candidates: #X / #Y"` where #X #Y were already rendered in the same response | "Other" must mean other |
| `tool_calls: []` and claims about marketplace agents the model couldn't have just looked up | Hallucination — must invoke `agent search` first |
| Listing `okx-*` skill names as "candidates" instead of running `agent search` | `agent != skill` confusion — skill names are not marketplace agents |
| Reading `~/.claude/projects/.../tool-results/<tid>.txt` or writing `/tmp/parse.sh` / `/tmp/extract_*.py` to bash-parse a captured CLI JSON | Shell-stitching is forbidden — use CLI `--page` instead |
| Cross-row copy of `profileDescription` / `feeAmount` / `serviceMinPrice` | Per-row data must be verbatim from the named backend field; identical values across N rows are almost certainly a parser bug, see `§Field mapping` |
| Stitching `page 1` and `page 2` locally before rendering | Boundary errors at the page split (duplicate / missing ids) — let the user page through |
| Fabricating a `serviceMinPrice` / `feeAmount` number when the backend returned `null` | Render `—`. Search response can legitimately have null prices |

---
