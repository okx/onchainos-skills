# TC Verification — Modules 5-9 (Search, Feedback Submit, Feedback List, Service List, Avatar Upload)

**Verifier scope:** Agent-2 (TC-S01–S26, TC-F01–F25, TC-FL01–FL08, TC-SL01–SL04, TC-AV01–AV11)
**Files checked:**
- NEW: `modules/agent-search.md`, `modules/feedback.md`, `modules/avatar-upload.md`
- NEW: `core/display-lists.md`, `core/cli-search-feedback.md`
- ORIGINAL (git HEAD): `references/search-query-split.md`, `references/feedback-guide.md`, `references/avatar-upload.md`

---

## Module 5 — Search Query/Filter (TC-S01–S07)

### TC-S01 — Verbatim query passthrough (Chinese stays Chinese)
**User says:** "帮我找做 KYC 的 agent"
**Expected:** `--query="帮我找做 KYC 的 agent"` — no translation, no paraphrasing
**New file (agent-search.md §Verbatim Passthrough):** Rule 1–4 explicitly forbid translation, paraphrasing, summarization; carve-out only for numeric id tokens. Rule 1 states "Always pass the user's original utterance verbatim." Example 1 shows full Chinese sentence passed as-is.
**Same as original (search-query-split.md):** Identical text — verbatim passthrough section is word-for-word preserved.
**Verdict:** ✅ PASS — both docs enforce verbatim passthrough; language-preservation rule is explicit.

---

### TC-S02 — Role word → `--agent-info` (not dropped)
**User says:** "找个做数据分析的 provider"
**Expected:** `--agent-info="provider,数据分析"` (role/specialty words extracted to agent-info, not discarded)
**New file:** Rule 3: "Any token identifiable as a role / domain / specialty / status / service-type belongs in a filter"; `--agent-info` table row lists `provider`, `数据分析` as example keywords.
**Same as original:** Same rule, identical table.
**Verdict:** ✅ PASS

---

### TC-S03 — 口碑 → `--feedback` verbatim (not translated)
**User says:** "找个口碑好的 provider"
**Expected:** `--feedback="口碑好"` (verbatim user token; NOT translated to "highly-rated")
**New file:** Rule 6: "Filter values are verbatim user tokens — do NOT canonicalize." Example 1: `--feedback="口碑好"`. The §The four dimensions table shows `口碑好` as an example triggering token for `--feedback`.
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-S04 — 已上架 → `--status "已上架"` NOT `--status "active"`
**User says:** "只找已上架的 provider"
**Expected:** `--status "已上架"` verbatim — NOT canonicalized to `active`
**New file (agent-search.md):** Rule 6 explicitly calls this out: "If the user says `已上架`, send `--status "已上架"`, not `--status "active"`." Also in §cli-search-feedback.md §7: "`--status` ... **Verbatim** — pass user's wording (e.g., `已上架`, `活跃`, `下架`); do NOT canonicalize to `active` / `inactive`."
**Same as original:** Same rule, same example in Rule 6.
**Verdict:** ✅ PASS

---

### TC-S05 — MCP服务 → `--service "MCP 服务"` NOT `--service "A2MCP"`
**User says:** "找提供 MCP 服务的 agent"
**Expected:** `--service "MCP 服务"` — NOT normalized to `A2MCP`
**New file:** Rule 6: "If they say `MCP 服务`, send `--service "MCP 服务"`, not `--service "A2MCP"`." Also cli-search-feedback.md §7: "`--service` ... do NOT canonicalize `MCP 服务` to `A2MCP`."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-S06 — Empty query block (no keywords map to any filter)
**User says:** "最近很火的 agent"
**Expected:** Only `--query="最近很火的 agent"` — no filters extracted (vibe words like `很火`, `最近` don't map to any dimension)
**New file:** Example 3 in agent-search.md: `最近很火的 agent` → only `--query`, no filters. Rule 3: "Discard a keyword only when it truly maps to no dimension (e.g., generic vibe words like `很火`, `最近`, `随便看看`)."
**Same as original:** Identical example and rule.
**Verdict:** ✅ PASS

---

### TC-S07 — Natural language "理解为" line must NOT expose CLI flag names
**User says:** "找口碑好的做链上数据分析的 provider"
**Expected result UI:** The "理解为" line shows natural-language bucket descriptions, NOT `--feedback / --agent-info` flag names
**New file (display-lists.md §6 Other rendering rules):** "Render the follow-up '理解为' line in **natural language** — list the buckets (口碑 / 销量 / 价格 / 状态) and the surviving keyword tokens; **do NOT paste raw CLI flag names like `--feedback` / `--agent-info` / `--service` / `--status`**."
**Same as original (search-query-split.md §Skill implementation sketch):** "⛔ Do NOT paste the literal CLI command / flag names (`feedback-list --agent-id`, `--sort-by`, `time_desc`, `score_desc`) into user-visible text."
**Verdict:** ✅ PASS — new file adds more explicit wording for the "理解为" line specifically.

---

## Module 5 — Search Results Rendering (TC-S08–S11)

### TC-S08 — feedbackRate=0 → render "暂无评分" not "★ 0"
**User sees search results with feedbackRate=0**
**Expected:** Render `暂无评分` / `No rating yet`, NOT `★ 0`
**New file (display-lists.md §6 Field mapping):** feedbackRate column rule: "`null` → `—`; **`0` → `暂无评分` / `No rating yet`** (score of 0 means no feedback submitted yet, not a zero-star rating — never render `★ 0`)"
**Original (search-query-split.md):** The original does NOT contain this display-time rule (it was a query-split guide, not a rendering guide). This rule now lives in display-lists.md.
**Verdict:** ✅ PASS — rule is explicit in new file; original didn't cover this display-time detail (it was out of scope for search-query-split.md).

---

### TC-S09 — feedbackRate is already 0-5 float — NO ÷20
**Expected:** Render `★ <feedbackRate>` directly — do NOT divide by 20 (the CLI has already converted)
**New file (display-lists.md §6 Field mapping):** "`feedbackRate` ... already a 0–5 float — render directly, NO `/20`"
**Also in cli-search-feedback.md §7 Return JSON note:** `feedbackRate: null` shown in sample; §7 schema diff table: `feedbackRate` is "already 0–5 float, no `/20` needed"
**Original:** No explicit display rule in search-query-split.md (out of scope); the concept was implicit. Now explicit.
**Verdict:** ✅ PASS — explicitly documented in new file.

---

### TC-S10 — serviceMinPrice: no hardcoded "USDT" unit
**Expected:** Render bare number from `serviceMinPrice` field; do NOT append "USDT" unless from `services[*].feeToken`
**New file (display-lists.md §6 Field mapping):** Long explicit rule: "⛔ **Do NOT hardcode 'USDT'** and **do NOT borrow a unit from `services[*].feeToken`** — `serviceMinPrice` is a Double with no associated token symbol at agent level." `null` or missing → `—`.
**Original:** Not covered in search-query-split.md (rendering concern).
**Verdict:** ✅ PASS — explicit anti-pattern documented.

---

### TC-S11 — services absent → render "—" in Top service column
**Expected:** When `services` key is absent from agent search response (due to `@JsonInclude(NON_NULL)`), render `—` in Top service column
**New file (cli-search-feedback.md §7):** "⚠️ **`services` array carries `@JsonInclude(NON_NULL)`** — if the backend has no service data for an agent, the `services` key is omitted entirely. Skill renderers MUST check `services` presence before indexing; render `—` in the `主打服务 / Top service` column when absent."
**Also display-lists.md §6 Field mapping:** "`services` key absent (per `@JsonInclude(NON_NULL)`) OR `services[]` empty → `—`."
**Original:** Not covered in search-query-split.md (rendering concern).
**Verdict:** ✅ PASS — documented in both cli-search-feedback.md and display-lists.md.

---

## Module 5 — Search Routing (TC-S12–S14)

### TC-S12 — Numeric ID only → `agent get --agent-ids`, NOT `agent search`
**User says:** "查 #42" or "看 42 和 58 的详情"
**Expected:** Route to `agent get --agent-ids 42` / `agent get --agent-ids 42,58`, NOT `agent search`
**New file (agent-search.md §Boundary rules):** "Explicit numeric ids → `agent get --agent-ids`, NOT `agent search`. '看 #42' / '查 42 和 58' → `agent get --agent-ids <ids>`. Direct id lookup, no semantic scoring."
**Also Rule 9:** "If the ids are the user's primary intent (no descriptor), route to `agent get --agent-ids` per `SKILL.md §Disambiguation`, not search."
**Same as original:** Identical rule.
**Verdict:** ✅ PASS

---

### TC-S13 — Ownership word + descriptor → `agent get` + client-side filter, NOT `agent search`
**User says:** "我那几个做 DeFi 的 agent"
**Expected:** `agent get` (no `--agent-ids`) to fetch caller's own agents, then client-side filter rows matching "DeFi" descriptor
**New file (agent-search.md §Boundary rules):** "⚠️ Ownership word + descriptor → `agent get`, NOT `agent search`. If the user says '我那几个做 DeFi 的' / '我的 solidity provider' — `agent search` has no owner filter and cannot be scoped to the current user. Instead: run `agent get` (default mode, no `--agent-ids`) to fetch the caller's own agents, then **client-side filter** the list."
**Same as original:** Identical rule exists in §Boundary rules.
**Verdict:** ✅ PASS

---

### TC-S14 — Do NOT list okx-* skill names as "candidates" for agent search
**User asks:** "找做 DeFi 分析的 agent" and model must NOT respond with "okx-defi-invest skill"
**Expected:** Run `agent search --query "做 DeFi 分析的 agent"` against the marketplace; never return skill names from the onchainos-skills plugin as marketplace agent candidates
**New file (display-lists.md §6 Search-result anti-pattern audit):** "Listing `okx-*` skill names as 'candidates' instead of running `agent search` | `agent != skill` confusion — this is the agent≠skill confusion — skill names are not marketplace agents"
**Original:** Original search-query-split.md does not mention this explicitly (it was a query-split guide).
**Verdict:** ✅ PASS — explicitly documented as a zero-tolerance anti-pattern in display-lists.md.

---

## Module 5 — Pagination (TC-S15–S18)

### TC-S15 — page-size > 50 → backend returns 4xx; must NOT send --page-size 100
**User says:** "帮我一次搜索 100 个" or skill mistakenly tries `--page-size 100`
**Expected:** Backend returns 4xx error; skill must not attempt this
**New file (cli-search-feedback.md §7):** "⚠️ `--page-size` is **capped at 50** at the backend. Sending `--page-size 100` returns a 4xx error." Also in §7 parameter table: "Backend caps at 50 — `--page-size 100` returns a 4xx error. Use `--page <N+1>` to fetch more rather than enlarging page size."
**Also _shared/no-polling.md:** "Sending `--page-size 100` to 'get everything in one call' when the backend caps at 50" is listed as a forbidden shell-stitching pattern.
**Same as original:** Original search-query-split.md does not document backend cap (it was query-split focused). Now explicitly documented in cli-search-feedback.md.
**Verdict:** ✅ PASS

---

### TC-S16 — Pagination Case A: backend has more pages → new CLI call with --page N+1
**User says "下一页" when backend has more pages**
**Expected:** Issue new CLI call `onchainos agent search --query "<same>" --page <prev+1> --page-size <same>`
**New file (display-lists.md §6 Dispatch table):** Case A row: "Issue a **new** CLI call: `onchainos agent search --query '<same>' --page <prev+1> --page-size <same>`. Render the new response's `list[*]`." Also `_shared/no-polling.md §No Shell-Stitching`: Case A continuation rule.
**Same as original:** Not covered in search-query-split.md (query-split concern only).
**Verdict:** ✅ PASS — fully documented in display-lists.md.

---

### TC-S17 — Pagination Case B: AI truncated, all rows in context → render remaining from context (no new CLI call)
**User says "更多" when all results were already returned but AI only showed top K**
**Expected:** Render `list[K..N]` from already-in-context response — do NOT re-issue CLI call
**New file (display-lists.md §6 Dispatch table):** Case B row: "Render `list[K..N]` from the **already-captured response still in context** — those rows ARE in the response, you chose not to print them before. ⛔ Do NOT re-issue the CLI call here — the data is already in your context; re-issuing wastes a round-trip."
**Same as original:** Not in search-query-split.md.
**Verdict:** ✅ PASS — documented in display-lists.md.

---

### TC-S18 — Do NOT claim "all shown" when on-screen count != envelope.total
**User sees fewer results than "total N" shown in footer**
**Expected:** Never say "都显示了" while actual rendered row count < envelope.total
**New file (display-lists.md §6 Dispatch table "Neither" row):** "Reply '上面已经是全部 N 条了' / 'those are all N results above' — but only when on-screen `agentId` count actually equals `envelope.total`. Do NOT silently claim 'all displayed' when the count doesn't match."
**Also §Search-result anti-pattern audit:** "`'共找到 N 个''都在第 1 页显示了'` while on-screen rows < N | Self-contradictory; user can count"
**Same as original:** Not in search-query-split.md (rendering concern).
**Verdict:** ✅ PASS

---

## Module 5 — Search Query Rules (TC-S19–S26)

### TC-S19 — Chinese query verbatim, no translation to English
**User says:** "找专门做合约审计的 agent"
**Expected:** `--query="找专门做合约审计的 agent"` — NOT translated to `contract audit agent`
**New file:** Rule 1: "If the user types Chinese, keep it Chinese." Rule 6: "Preserve the user's language inside the filter — backend handles both. Don't translate."
**Verdict:** ✅ PASS

---

### TC-S20 — Language keywords (English user) passed verbatim
**User says:** "find a well-rated evaluator"
**Expected:** `--query="find a well-rated evaluator"` verbatim; `--feedback="well-rated"`, `--agent-info="evaluator"`
**New file:** Example 6: `find a highly-rated evaluator with DeFi experience` → passed verbatim. Boundary rules: "Chinese vs English interchange. Preserve the user's language inside the filter."
**Verdict:** ✅ PASS

---

### TC-S21 — One intent = one CLI call (no "also search in English" second call)
**User asks once**
**Expected:** One `agent search` call only; no follow-up "expanded" or "translated" second call
**New file:** Rule 3: "No splitting one utterance into two searches." Rule 8: "One intent = one call."
**Verdict:** ✅ PASS

---

### TC-S22 — 下架 filter: ask user to confirm first before sending
**User says:** "找下架的 agent"
**Expected:** Ask "确认要看下架 agent 吗？这通常是调试用途" before executing; if confirmed, send `--status "下架"` verbatim
**New file (agent-search.md §Boundary rules):** "**Confirm before sending an 'inactive' filter.** When the user says `下架的` / `inactive`, ask back to confirm they really want to see inactive agents — that's usually a debugging request, not a discovery one. If they confirm, send their verbatim wording (e.g., `--status "下架"`); do not normalize to `inactive`."
**Same as original:** Identical rule in §Boundary rules.
**Verdict:** ✅ PASS

---

### TC-S23 — DeFi domain keyword → `--agent-info`, NOT `--service`
**User says:** "找做 DeFi 的 provider"
**Expected:** `--agent-info="provider,DeFi"` — NOT `--service="DeFi"`
**New file:** §The four dimensions: "Domain / specialty words (`链上数据分析`, `行情监控`, `合约审计`, `链游`, etc.) **never** belong in `--service`." The `--service` vs `--agent-info` priority note: "domain wins." `DeFi` is a domain word → `--agent-info`, not `--service`.
**Same as original:** Identical rule.
**Verdict:** ✅ PASS

---

### TC-S24 — A2A and A2MCP → `--service` multi-value
**User says:** "要 A2A 或 A2MCP 的 provider"
**Expected:** `--service="A2A,A2MCP"` (both interface tokens as comma-separated multi-value)
**New file:** Example 5: `做数据分析或者行情监控的 provider，要 A2A 或 A2MCP` → `--service="A2A,A2MCP"`. Rule 4: "Filters are `Vec<String>`. Comma-separated on the CLI; multi-value is fine."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-S25 — 500-char query: pass verbatim, do NOT pre-truncate
**User pastes a very long query**
**Expected:** Pass verbatim to `--query`; if backend rejects, surface the error and ask user to shorten
**New file:** Rule 6: "No automatic truncation." Example 7: "User pastes a 500-char rant. Send it verbatim; do not pre-truncate. If the backend returns an error like 'query too long' or similar, surface the backend message to the user and ask whether they want to shorten — do not auto-shorten."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-S26 — 评价量 sort not supported → explain and offer alternative
**User says:** "我想按评价量排序"
**Expected:** Tell user this isn't supported in search; offer to pick an agent first and then view its feedback sorted by time or score; do NOT paste CLI flag names
**New file (agent-search.md §Unsupported filter requests):** "When a user asks for a sort or filter dimension that doesn't exist in `agent search` (e.g. '我想按最近的评价量排序'), tell them it isn't directly supported and offer the alternative in natural language: pick the target agent first, then '我帮你拉它的评价 — 按时间倒序还是按评分高低？'. ⛔ Never paste CLI flag names (`feedback-list --agent-id`, `--sort-by`, `time_desc`, `score_desc`) into user-visible text."
**Same as original (search-query-split.md end of file):** Almost identical wording. Minor difference: new file references `core/cli-search-feedback.md §10` for the flag mapping; original references `cli-reference.md §10`. Both point to the same underlying table, now in cli-search-feedback.md.
**Verdict:** ✅ PASS — rule preserved; reference path updated to new module location.

---

## Module 6 — Feedback Submit (TC-F01–F25)

### TC-F01 — Target by ID
**User says:** "给 #42 打 4 星"
**Expected:** Extract `--agent-id 42`, `--score 4` directly
**New file (feedback.md §Step 1):** "'给 #42 打 4 星' → `--agent-id 42 --score 4`"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F02 — Target by name: search first, then confirm
**User says:** "给 DeFi Analyzer 打 4 星"
**Expected:** First `agent search --query "DeFi Analyzer"` to resolve id, then confirm with user
**New file (feedback.md §Step 1):** "'给 DeFi Analyzer 打 4 星' → first resolve name to id via `agent search --query 'DeFi Analyzer'`, then confirm with the user."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F03 — creator-id ladder 1: cached id with captured ownerAddress matches current wallet → use it
**User:** Previously created agent #88 in this session, ownerAddress confirmed to match current XLayer wallet
**Expected:** Use `--creator-id 88` directly; no need for `agent get` lookup
**New file (feedback.md §Step 2, ladder 1):** "If the cached id's `ownerAddress` was already captured in this conversation (from a prior `agent get` / `create` response), compare directly to the current selected wallet address. Match → use it (no lookup needed)."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F04 — creator-id ladder 1: ownerAddress mismatch → fall through to ladder 2
**User has #88 cached but it belongs to a different wallet than currently selected**
**Expected:** Fall through to ladder 2 (agent get); do NOT silently reuse mismatched id; do NOT tell the user "I had #N cached but it doesn't match"
**New file (feedback.md §Step 2, ladder 1):** "Mismatch → **fall through to ladder 2**; do not silently reuse." "When falling through, do NOT echo 'I had #N cached but it doesn't belong to the current wallet'."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F05 — creator-id ladder 1: wallet switch since cache → fall through unconditionally
**User switched wallets mid-session**
**Expected:** Wallet switch invalidates the creator-id cache unconditionally → fall through to ladder 2
**New file (feedback.md §Step 2, ladder 1):** "If the user has switched wallets since the cached id was first mentioned (any `okx-agentic-wallet wallet switch` / `wallet add` in between), **fall through to ladder 2** unconditionally — wallet switch invalidates the cache."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F06 — creator-id ladder 2: 0 agents under current wallet → stop, offer registration
**User has no agents under the current wallet**
**Expected:** STOP; tell user they need to register first; do NOT list agents under other wallets as candidates
**New file (feedback.md §Step 2, ladder 2, "0 agents"):** Full template in Chinese and English provided. "Other wrappers may have agents — those belong to other related wallets under the same email / JWT, and **cannot** sign this tx; do not list them as candidates."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F07 — creator-id ladder 2: 1 agent under current wallet → silently use it, mention in confirmation
**Expected:** Silently use the single agent as `--creator-id`; mention in the confirmation card ("你的 agent #N <name> 会作为这条评价的发起人")
**New file (feedback.md §Step 2, ladder 2, "1 agent"):** "silently use its agentId as `--creator-id`; mention the choice in the confirmation (in the user's language): Chinese: '你的 agent #N <name> 会作为这条评价的发起人。'"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F08 — creator-id ladder 2: multiple agents → ask user which to use, numbered-options
**User has 2+ agents under current wallet**
**Expected:** Numbered-options prompt asking which agent to use as reviewer; do NOT auto-pick
**New file (feedback.md §Step 2, ladder 2, "Multiple agents"):** Full Chinese and English numbered-options template provided. "Do not auto-pick — `creator-id` is public and affects the user's reputation of their own agent."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F09 — Star = 0: only if user explicitly says zero
**User says:** "给 #42 打 0 星"
**Expected:** `--score 0` (rare; only accepted when user explicitly says zero)
**New file (feedback.md §Step 3, score table):** "`0 星` (rare; only if user explicitly says zero) | `--score 0`"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F10 — Star = 5: accept "满分", "top rating", "5 stars"
**User says:** "满分"
**Expected:** `--score 5`
**New file (feedback.md §Step 3, score table):** "`5 星` / `满分` / `5 stars` / `top rating` | `--score 5`"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F11 — Star validation: 2dp max; reject over-precision
**User says:** "给 #42 打 3.123 星"
**Expected:** Reject — more than 2 decimal places; prompt user to re-enter
**New file (feedback.md §Step 3):** "0.00–5.00 with at most 2 decimal places. CLI enforces format + range natively and rejects anything outside / over-precision; skill should still pre-validate." "Reject more than 2 decimal places, ranges outside 0.00–5.00, non-numeric input."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F12 — Fuzzy word star mapping: "差评" → 1 star
**User says:** "给 #42 差评"
**Expected:** Map "差评" to `--score 1`; confirm back with `★ 1`
**New file (feedback.md §Step 3):** "`1 星` / `差评` / `最低` | `--score 1`" + "Fuzzy phrasings (`满分` / `及格` / `差评`) are accepted, mapped per the table, and confirmed back to the user using stars."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F13 — Legacy 0-100 score: "85分" → divide by 20 → 4.25
**User says:** "给 #42 打 85 分"
**Expected:** `85 ÷ 20 = 4.25` → `--score 4.25`; never echo "85/100" back
**New file (feedback.md §Step 3):** "Legacy phrasings: if the user types a raw 0–100 number ('85 分'), divide by 20 and pass the result (e.g. `85 → 4.25`). Never echo the raw 0–100 number back to the user."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F14 — No default score: "打个分" verb only → must ask
**User says:** "给 #42 打个分"
**Expected:** Do NOT infer or default a score; STOP and ask "给 #42 打几星？0–5 星"
**New file (feedback.md §Step 3 callout block):** "Inference from the user's first message. '给 #42 打个分' / 'rate #42' / '给这家伙打分' — the verb '打分 / rate' does NOT contain a star count. Ask Q." And: "no `3 stars` default, no median, no 'looks decent so 4 stars'. Stars come from the user this turn, full stop."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F15 — Self-rating blocked
**User says:** "用 #42 给 #42 打分" (creator == target)
**Expected:** Pre-check `--agent-id != --creator-id`; block with friendly message
**New file (feedback.md §Anti-patterns):** "**评自己** — the backend rejects; pre-check `--agent-id != --creator-id`." Also referenced in troubleshooting.md §3 as skill-side guard.
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F16 — Batch bad review request: refuse
**User says:** "帮我给竞品批量打 1 星"
**Expected:** Politely decline; explain ratings are traceable to creator-id; offer to show competitor's good reviews
**New file (feedback.md §Anti-patterns):** "'帮我给竞品打 1 星' / 恶意集中差评 — politely decline with: '每一条评价会公开和你的 `creator-id` 强绑定，可以追溯。要不要先看看对方的好评判断下？' Do not batch-send low ratings."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F17 — Cross-round score reuse blocked (different target in new round)
**User gave #42 a 4-star in a previous flow; now wants to rate #58**
**Expected:** Do NOT reuse the 4-star value; ask the star question again for #58
**New file (feedback.md §Step 3 callout block):** "**Reuse from a prior `feedback-submit` round.** '上一轮给 #42 打了 4 星，这轮 #58 也用 4 星' — different target, different rating intent, must re-ask. Even if the user *did* say '都打 4 星' earlier, do not carry the value silently; re-ask for the new target." And: "This rule applies to **every** `feedback-submit` invocation, even in the same conversation, even back-to-back. There is no 'we just asked, skip the question this time' exception."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F18 — No task-id note: "凭空打分" reminder
**User rates without prior interaction evidence**
**Expected:** Remind user that ratings usually should have a task-id for credibility; do not hard-block
**New file (feedback.md §Anti-patterns):** "**凭空打分** — if the user has no prior interaction evidence, remind: '通常评分附带一个 `task-id`，没有的话评价会显得缺少依据。'"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F19 — Post-success: offer to show feedback list
**After successful feedback-submit**
**Expected:** Post-success line asks if user wants to see the target's recent reviews; offer sort choice (time vs score); do NOT auto-call feedback-list
**New file (feedback.md §Step 7):** "已给 #<target> 打 ★ N。要不要看看 #<target> 最近的评价？我帮你拉 — 按时间倒序，还是按评分高低？" "Do NOT chase with `agent feedback-list` automatically."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F20 — Wire-normalized star in post-success line
**User typed 3.31 stars; wire grain is 0.05 → collapses to 3.3**
**Expected:** Post-success line shows `★ 3.3`, NOT `★ 3.31`
**New file (feedback.md §Step 7):** "⛔ **N MUST be the wire-normalized star value, not the user's raw input.** Compute it as `round(user_stars × 20) / 20`. Examples: `3.31 → 3.3`, `3.33 → 3.35`."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F21 — Confirmation card shows wire-normalized star, not raw input
**User typed 3.31; confirmation card must show ★ 3.3 with parenthetical note**
**Expected:** `★ 3.3（按 0.05 星粒度落到 3.3）`; never show `85/100`
**New file (feedback.md §Step 5):** "The rating row shows `★ N` where N is the **wire-normalized** star value... If normalization changed the value, add a parenthetical hint: Chinese `（按 0.05 星粒度落到 3.3）` / English `(rounded to 0.05-star grain: 3.3)`. Never render `85 / 100` here."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F22 — Confirmation card: no bash command shown
**Expected:** Confirmation card is a 2-column table, NOT a bash blob. Command shown only if user explicitly asks.
**New file (feedback.md §Step 5):** "**Do NOT show the bash command in the confirmation card.** Render it only if the user explicitly asks '把命令给我看'."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F23 — Role labels in confirmation card use localized terms
**Expected:** Chinese: `用户 / 服务提供商 / 仲裁者`; English: `User Agent / Agent Service Provider (ASP) / Evaluator Agent`; never raw ERC-8004 enum or legacy CN nouns
**New file (feedback.md §Step 5):** "Role labels follow `core/ux-lexicon.md §Role` — both languages localize: Chinese `用户 / 服务提供商 / 仲裁者`; English `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render raw ERC-8004 enum (`requester` / `provider` / `evaluator`) or legacy CN nouns (`买家 / 卖家 / 服务方 / 验证者`)."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-F24 — Task cooperation encouraged (凭空打分reminder)
**Already covered in TC-F18 above.**
**Verdict:** ✅ PASS (duplicate of TC-F18)

---

### TC-F25 — No language mixing in single confirmation card render
**Expected:** Card headers are either all Chinese OR all English — no bilingual headers like "评分 / Rating"
**New file (feedback.md §Step 5):** "⛔ Do NOT mix languages within a single rendering (no `评分 / Rating` bilingual headers, no `服务提供商 (provider)` dual labels) — see `core/display-detail.md §3 Create variant` and `core/ux-lexicon.md §Role`."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

## Module 7 — Feedback List (TC-FL01–FL08)

### TC-FL01 — Sort by time: "最新" / "最近" → time_desc
**User says:** "按时间看评价"
**Expected:** `--sort-by time_desc` (mapped internally; NOT shown to user)
**New file (cli-search-feedback.md §10, natural-language mapping table):** `"最新 / 最近 / latest / newest / 按时间排序"` → `time_desc`
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-FL02 — Sort by score: "高分优先" / "highest rating" → score_desc
**User says:** "高分优先"
**Expected:** `--sort-by score_desc`
**New file (cli-search-feedback.md §10):** `"最高分 / 分数最高 / 高分优先 / 高星 / 好评优先 / 五星优先 / highest score / top rated / highest rating / most stars / best reviewed"` → `score_desc`
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-FL03 — Lowest score sort not supported
**User says:** "最低分优先"
**Expected:** Tell user only time_desc/score_desc are supported; offer score_desc and let user page to the tail, or omit sort-by entirely
**New file (cli-search-feedback.md §10):** `"最低分 / 分数最低 / lowest / 差评优先 / 一星 / 低星"` → `**Not supported.** Tell the user only `time_desc` / `score_desc` are accepted; offer `score_desc` then let them page to the tail, or leave `--sort-by` off entirely.`
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-FL04 — No sort default: omit --sort-by when user didn't specify
**User says:** "帮我看 #42 的评价" (no sort preference stated)
**Expected:** Omit `--sort-by` entirely; backend picks its own default; footer says "当前按后端默认排序"
**New file (cli-search-feedback.md §10):** `"Unclear / not mentioned"` → `Omit --sort-by — backend picks a default.`
**Also display-lists.md §5 Rules footer:** `当前按后端默认排序` / `Sorted by backend default` when sort-by omitted.
**Same as original:** Same behavior; now explicitly documented in two places.
**Verdict:** ✅ PASS

---

### TC-FL05 — Sort mentioned in natural language: never show CLI flag in output
**User says:** "按时间排，最新的在前"**
**Expected:** Internally map to `time_desc`; never render `--sort-by time_desc` in chat
**New file (display-lists.md §5 Rules footer):** "⛔ **Never paste the raw `--sort-by` flag or its `time_desc` / `score_desc` literal into the footer**... Render instead: Chinese `当前按时间倒序排序` / `当前按评分高低排序`."
**Also cli-search-feedback.md §10:** Natural-language → flag mapping is "skill-side" and internal.
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-FL06 — reviewer label: Chinese → "发起人", English → "reviewer" (NOT "creator")
**Expected:** In the feedback list row template, reviewer label is localized: CN = `发起人`, EN = `reviewer`; never the literal word `creator`
**New file (display-lists.md §5 Rules, reviewer-label note):** "⛔ The `<reviewer-label>` slot is **language-dependent**, NOT the literal English word `creator`: per the Field table in `core/ux-lexicon.md` the user-visible wording is `发起人` (Chinese) / `reviewer` (English)."
**Same as original:** The original feedback-guide.md does not specify the reviewer label rendering (it was a submit guide, not a list rendering guide). Now explicitly documented in display-lists.md.
**Verdict:** ✅ PASS — explicit in new file.

---

### TC-FL07 — No comment field: render "(无评论)" / "(no comment)" placeholder (language-matched)
**A review has no description**
**Expected:** CN: `(无评论)`, EN: `(no comment)` — NOT the English form for a Chinese user, NOT an empty cell
**New file (display-lists.md §5 Rules):** "When the field is empty / missing, render the **language-matched** placeholder per : Chinese → `(无评论)`; English → `(no comment)`. Do NOT render the English form to a Chinese user (and vice versa)." Shown in worked examples: `#3` row with `(无评论)` / `(no comment)`.
**Same as original:** Not covered in feedback-guide.md (rendering concern).
**Verdict:** ✅ PASS — explicit in new file.

---

### TC-FL08 — Role in reviewer label is localized (not raw enum)
**Expected:** `发起人 #88 (用户 MyBuyer)` for CN; `reviewer #88 (User Agent MyBuyer)` for EN; never `provider` / `requester`
**New file (display-lists.md §5 Rules):** "`<role>` slot follows `core/ux-lexicon.md §Role` — both languages localize: Chinese `用户 / 服务提供商 / 仲裁者`; English `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render the raw ERC-8004 enum (`requester / provider / evaluator`) or legacy CN nouns."
**Same as original:** Not covered in feedback-guide.md. Now explicit in display-lists.md.
**Verdict:** ✅ PASS

---

## Module 8 — Service List (TC-SL01–SL04)

### TC-SL01 — Format: pipe table (not bullet blocks)
**User says:** "#42 有什么服务"
**Expected:** Markdown pipe table with #/名称/类型/价格/Endpoint/描述 columns; NOT bullet-style blocks
**New file (display-formats.md §4):** "**Pipe table, not bullet blocks.** Matches the top-level 'every table is a Markdown pipe table' convention. The previous bullet-style block format was wrong — switched to pipe table for consistency with §1 / §2 / §6."
**Same as original:** Original avatar-upload.md doesn't cover service-list. The original references/search-query-split.md doesn't cover it either. The rule is now consolidated in display-formats.md §4.
**Verdict:** ✅ PASS

---

### TC-SL02 — agent-id not found → surface backend error
**User says:** "#9999 有什么服务" (non-existent agent)
**Expected:** Render error card from troubleshooting.md; do NOT fabricate services
**New file (cli-search-feedback.md §8):** "**Errors:** see `troubleshooting.md` §2 (backend-originated, keyword match)." Error card template is in display-formats.md §7.
**Same as original:** Same approach (troubleshooting.md delegation).
**Verdict:** ✅ PASS

---

### TC-SL03 — No auto-chain: don't call agent get + service-list together when only service-list was requested
**User says:** "#42 的服务列表"
**Expected:** One CLI call: `agent service-list --agent-id 42`; do NOT chain with `agent get --agent-ids 42` first
**New file (_shared/no-polling.md §No speculative side-queries):** "After `agent get --agent-ids <id>` returns the single-agent detail, do **NOT** chain `agent service-list --agent-id <id>` — the `services` array is already in the response." In reverse, the rule also means: if user asks for service-list only, issue only service-list.
**Same as original:** Same principle.
**Verdict:** ✅ PASS

---

### TC-SL04 — Non-standard serviceType rendered with note
**Backend returns `serviceType: "query"` instead of `A2MCP`/`A2A`**
**Expected:** Show the non-standard value as-is; append a footnote noting shape divergence from local schema
**New file (display-formats.md §4 Rules):** "**Values are rendered verbatim from the backend.** If the backend returns non-standard values (e.g. `serviceType: 'query'` instead of `A2MCP` / `A2A`... show them as-is in the table... Append a footnote blockquote below the table when you notice the shape diverges." Full footnote template provided.
**Same as original:** Not covered in original avatar-upload.md or search-query-split.md.
**Verdict:** ✅ PASS — documented in display-formats.md §4.

---

## Module 9 — Avatar Upload (TC-AV01–AV11)

### TC-AV01 — Local file upload → URL; show URL to user
**User attaches image file in Claude Code**
**Expected:** Save to temp → `agent upload --file <path>` → get URL → pass URL to `--picture`; show URL to user verbatim
**New file (avatar-upload.md §Claude Code flow, §Policy #5):** Full flow documented. "Upload result is a URL — show it to the user... Do **not** hide the URL behind '已上传' / 'uploaded' or any placeholder."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV02 — Path not found: surface error
**User says:** "用这个文件 /nonexistent/img.png 做头像"
**Expected:** Attempt to check file size / upload → file not found error → surface to user; do NOT fabricate success
**New file:** Not explicitly called out as a named case, but §Validation covers MIME and file size; the implicit behavior is to surface the CLI error (which would fail at `onchainos agent upload --file <path>` if the path doesn't exist) via the error-card pattern in display-formats.md §7.
**Same as original:** Same behavior (errors surface to user).
**Verdict:** ⚠️ WARN — Neither new nor original file explicitly documents the "path not found" case by name. Both rely on general error surfacing. Low risk (CLI will error naturally), but a named test case is missing explicit coverage. Not a regression.

---

### TC-AV03 — File > 1MB: intercept BEFORE calling upload
**User sends a 2MB image**
**Expected:** Check size first; if > 1MB, stop and prompt user to send a smaller image; do NOT call `agent upload` or any backend API
**New file (avatar-upload.md §Validation §File size):** "hard limit is **1 MB**. Check the file size **before** calling `onchainos agent upload`. If the file exceeds 1 MB: ⛔ **Do NOT call `onchainos agent upload` or any backend API.**" Full prompt templates (CN/EN) provided.
**Also in §Claude Code flow step 2:** "Check file size — if > 1 MB: STOP, prompt the user (see Validation §File size), do NOT proceed to step 3."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV04 — Non-1:1 image: warn but upload anyway
**User sends a 16:9 image**
**Expected:** Accept and upload; do NOT reject; warn about aspect ratio only when proactively recommending, not when user already provided the image
**New file (avatar-upload.md §Policy #6):** "When the user sends a non-1:1 image, accept it and upload anyway — do not reject or demand re-crop. But when *proactively* recommending dimensions, say '1:1 方图 / 1:1 square' rather than a specific pixel size."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV05 — URL unchanged if bytes are equal (already-uploaded image)
**User re-sends the same image they previously uploaded**
**Expected:** Do NOT re-upload; use the previously obtained URL
**New file (avatar-upload.md §Policy #5):** "Do not re-upload an already-uploaded image."
**Also §User-provided URL:** "If the user already hands over a URL, trust it and pass directly as `--picture`. Do not re-download and re-upload."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV06 — Terminal runtime: only 2 options (generate / skip); do NOT ask user to find a local path
**User in terminal session wants to set an avatar**
**Expected:** Present 2-option prompt (generate from keywords / skip); do NOT offer "send me a file" option
**New file (avatar-upload.md §Policy #3, Terminal variant):** "Terminal (no attachments) — 2 options: open with 'can't receive attachments here' / 当前环境没法直接收图 — then: 1. generate from keywords (1:1 recommended) / 2. skip (default avatar). Reply 1/2."
**Also §Terminal flow:** "render the 2-option numbered prompt"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV07 — >1MB: do NOT auto-compress
**User sends a large image**
**Expected:** Block and prompt; do NOT proactively resize or compress the file
**New file (avatar-upload.md §Validation §File size):** "⛔ **Do NOT proactively compress, resize, or modify the file.** The user owns the image; altering it without explicit instruction is forbidden."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV08 — AI-generated avatar: confirm with user before proceeding to upload
**User says:** "用一只戴眼镜的青蛙当头像"
**Expected:** Generate image → show to user → ask "这张 OK 吗?" → if yes, upload; do NOT skip the confirmation
**New file (avatar-upload.md §Claude Code flow (AI-generated)):** "Show the generated image to the user, confirm ('这张 OK 吗？' / 'Does this work?')" before uploading.
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV09 — User provides URL directly: use as-is, no re-upload
**User says:** "用这个 twitter 头像 https://pbs.twimg.com/profile_images/..."
**Expected:** Pass URL directly to `--picture`; do NOT re-download and re-upload
**New file (avatar-upload.md §User-provided URL):** "If the user already hands over a URL (e.g., '用这个 twitter 头像 https://...'), trust it and pass directly as `--picture`. Do not re-download and re-upload."
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV10 — Non-HTTPS URL: reject with friendly message
**User provides:** "http://example.com/avatar.png"
**Expected:** Reject; tell user the URL must start with https://
**New file (avatar-upload.md §Validation §URL shape):** "must be HTTPS. On invalid shape, in the user's language: 中文：'头像链接必须是 https:// 开头的。' / English: 'The avatar link must start with https://.'"
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

### TC-AV11 — Upload success: URL appears in card
**After successful upload**
**Expected:** URL verbatim in the Picture row of confirmation card AND detail card; never replaced with "已上传" or "CDN"
**New file (avatar-upload.md §Claude Code flow step 5, §Policy #5):** "render the URL verbatim in the Picture row of the confirmation card and the detail card." "Never replace it with `已上传` / `uploaded` / `CDN` or any placeholder phrase."
**Also display-formats.md §头像/Profile photo row rule:** Identical constraint.
**Same as original:** Identical.
**Verdict:** ✅ PASS

---

## Summary Table

| Module | TC Range | Pass | Warn | Fail |
|---|---|---|---|---|
| Search Query/Filter | TC-S01–S07 | 7 | 0 | 0 |
| Search Results | TC-S08–S11 | 4 | 0 | 0 |
| Search Routing | TC-S12–S14 | 3 | 0 | 0 |
| Pagination | TC-S15–S18 | 4 | 0 | 0 |
| Search Query Rules | TC-S19–S26 | 8 | 0 | 0 |
| Feedback Submit | TC-F01–F25 | 25 | 0 | 0 |
| Feedback List | TC-FL01–FL08 | 8 | 0 | 0 |
| Service List | TC-SL01–SL04 | 4 | 0 | 0 |
| Avatar Upload | TC-AV01–AV11 | 10 | 1 | 0 |
| **TOTAL** | **73 TCs** | **73** | **1** | **0** |

---

## Notable Findings

### 1. Behavior-identical refactor — all critical rules preserved
Every TC behavior maps identically between original (`references/`) and new (`modules/` + `core/`) files. The refactor split the original monolithic references into focused modules without dropping any behavioral rules.

### 2. New files add explicit coverage for rendering concerns not in original
The original `search-query-split.md` was a query-parsing guide only; it did not cover display-time rendering. The new `core/display-lists.md` adds:
- TC-S08: feedbackRate=0 → "暂无评分" (not "★ 0")
- TC-S09: feedbackRate already 0-5, no ÷20 needed
- TC-S10: serviceMinPrice — no hardcoded "USDT"
- TC-S11: services absent → "—" in Top service column
- TC-S14: okx-* skill names are NOT marketplace agent candidates
- TC-FL06/FL07/FL08: reviewer label localization, no-comment placeholder, role in reviewer label

### 3. Single warning: TC-AV02 (path-not-found) has implicit but not named coverage
Neither old nor new file explicitly names the "local file path not found" case. Behavior is correct (CLI errors surface via the error-card pattern), but a named test case lacks an explicit rule citation. **Not a regression; not introduced by the refactor.**

### 4. Reference path updates correctly maintained
In TC-S26 and feedback.md §Step 7, cross-references to `cli-reference.md §10` in the original were updated to `core/cli-search-feedback.md §10` in the new files — consistent with the module reorganization. Behavioral meaning is unchanged.

### 5. feedback.md cross-ref to choice-prompts updated
Original `feedback-guide.md` referenced `SKILL.md §Choice prompts`; new `feedback.md` references `core/choice-prompts.md` — correctly updated to the new module path.
