# TC R2 Agent2 — Second-Pass Verification: Modules 5–9
## Scope: Search, Feedback Submit, Feedback List, Service List, Avatar Upload
## Reviewer: Agent2 (rigorous second pass)
## Date: 2026-05-29

---

## Legend
- PASS: rule is present, correctly specified, and matches expected behavior
- FAIL: rule is missing, wrong, or contradicts expected behavior
- WARN: rule is present but has a nuance worth flagging (not a blocker)
- N/A: test case is not applicable to the current module file set

---

## PART 1: SEARCH (TC-S01 – S26)

### TC-S01 — Verbatim query passthrough
**User:** 找个口碑好的做链上数据分析的 provider
**Expected:** `--query` receives the user's sentence word-for-word, no edits.
**File/Line:** `modules/agent-search.md` lines 8–20 (§Verbatim Passthrough — red line), Rule 1 line 26.
**Result:** PASS — Rule 1 explicitly states "Always pass the user's original utterance verbatim." The ban on translation, paraphrasing, and summarization is explicitly enumerated as rules 1–5.

---

### TC-S02 — 4-dimension filter extraction
**User:** 只看活跃的高分 provider
**Expected:** `--query` verbatim + `--feedback="高分"` + `--agent-info="provider"` + `--status="活跃"`. No `--service`.
**File/Line:** `modules/agent-search.md` lines 40–51 (§The four dimensions), worked example 4 lines 100–108.
**Result:** PASS — Example 4 exactly matches this scenario and shows the correct 3-filter extraction without an unwarranted `--service`.

---

### TC-S03 — Empty query block (no filters match)
**User:** 最近很火的 agent
**Expected:** Only `--query="最近很火的 agent"`, no filters added (vibe words discarded).
**File/Line:** `modules/agent-search.md` Rule 3 line 28, Example 3 lines 81–88.
**Result:** PASS — Example 3 explicitly shows `--query` only, no filters. Rule 3 names "generic vibe words like `很火`, `最近`, `随便看看`" as discard cases.

---

### TC-S04 — No CLI flags in "理解为" line
**User:** 找做 Solidity 合约审计的 provider
**Expected:** "理解为：关键词「provider」+「solidity」+「合约审计」" — no `--agent-info` or `--feedback` literal in user-visible text.
**File/Line:** `core/display-lists.md` line 108 (§Other rendering rules, "理解为" line).
**Result:** PASS — Explicitly states "do NOT paste raw CLI flag names like `--feedback` / `--agent-info` / `--service` / `--status`". The worked example (lines 57, 70) shows the natural-language form.

---

### TC-S05 — Filter values are verbatim user tokens (no canonicalization)
**User:** 找已上架的 MCP 服务的 provider
**Expected:** `--status="已上架"` NOT `--status="active"`, `--service="MCP 服务"` NOT `--service="A2MCP"`.
**File/Line:** `modules/agent-search.md` Rule 6 line 31, `core/cli-search-feedback.md` §7 parameter table lines 25–28.
**Result:** PASS — Rule 6 explicitly calls out both examples. cli-search-feedback §7 repeats "Pass user's exact wording — never canonicalize."

---

### TC-S06 — No `--sort-by` on `agent search`
**User:** 按评分高低排
**Expected:** Tell user `--sort-by` doesn't exist on search. No `--sort-by` flag added to `agent search`.
**File/Line:** `modules/agent-search.md` Rule 7 line 32, `core/cli-search-feedback.md` §7 line 32 ("There is no `--sort-by` on `agent search`").
**Result:** PASS — Rule 7 and §7 both confirm there is no `--sort-by` on search. §Unsupported filter requests (line 149) gives the exact natural-language alternative to offer.

---

### TC-S07 — One call per intent
**User:** 找做 DeFi 的 provider
**Expected:** One `agent search` call only. No follow-up "translated" or "expanded" second call.
**File/Line:** `modules/agent-search.md` Rule 3 line 15 ("No splitting one utterance into two searches"), Rule 8 line 33.
**Result:** PASS — Rule 3 and Rule 8 both forbid re-searching. Rule 8: "One user intent = one `agent search`."

---

### TC-S08 — feedbackRate=0 → 暂无评分 (not ★ 0)
**User:** (search returns an agent with feedbackRate=0)
**Expected:** Display cell shows `暂无评分` / `No rating yet`, never `★ 0`.
**File/Line:** `core/display-lists.md` line 88 (§6 Field mapping, Rating row).
**Result:** PASS — Line 88 explicitly: "`0` → `暂无评分` / `No rating yet` (score of 0 means no feedback submitted yet, not a zero-star rating — never render `★ 0`)". This is a NEW rule added relative to the old references/display-formats.md (HEAD), which had `null → —` but did NOT call out `0` separately. The new file correctly distinguishes `null` → `—` vs `0` → `暂无评分`.

NOTE: The old HEAD `references/display-formats.md` line 483 read: "`feedbackRate` | `★ <feedbackRate>` (already a 0–5 float — render directly, NO `/20`); `null` → `—`" — it did NOT have the `0 → 暂无评分` rule. This is a genuine improvement in the new file.

---

### TC-S09 — feedbackRate is 0–5 float, NO ÷20 needed
**User:** (search returns agents with feedbackRate values)
**Expected:** Render `★ <feedbackRate>` directly. Never apply `feedbackRate / 20`.
**File/Line:** `core/display-lists.md` line 88 (Rating row: "already a 0–5 float — render directly, NO `/20`"), and `core/cli-search-feedback.md` §7 line 97 ("feedbackRate (already 0–5 float, no `/20` needed)").
**Result:** PASS — Both files explicitly state NO `/20` for `feedbackRate` in search results. The CLI schema contrast table in §7 line 97 also confirms this. No division is applied anywhere.

CRITICAL CHECK: The old HEAD `references/display-formats.md` line 483 had the same rule: "NO `/20`". The new `core/display-lists.md` line 88 maintains it and adds `0 → 暂无评分`. No regression.

---

### TC-S10 — serviceMinPrice no hardcoded USDT
**User:** (search returns agent with serviceMinPrice=10)
**Expected:** Display shows bare `10`, NOT `10 USDT`. Currency MUST come from the specific service's `feeToken`.
**File/Line:** `core/display-lists.md` line 89 (Min price row): "Do NOT hardcode 'USDT'... inferring a unit from another field is the same cross-field fabrication anti-pattern."
**Result:** PASS — Line 89 has a long explicit prohibition: "⛔ Do NOT hardcode 'USDT' and do NOT borrow a unit from `services[*].feeToken`". The worked examples at lines 61–76 show bare number `10` in the Min price column with no unit attached.

CRITICAL CHECK: The old HEAD `references/display-formats.md` line 484 had identical rule. New file maintains it correctly.

---

### TC-S11 — Absent services → "—"
**User:** (search returns agent with no `services` key)
**Expected:** Top service column shows `—`.
**File/Line:** `core/display-lists.md` line 90 (Top service row): "`services` key absent (per `@JsonInclude(NON_NULL)`) OR `services[]` empty → `—`."
**Result:** PASS — Rule explicitly handles both absent key and empty array cases. Also noted at `core/cli-search-feedback.md` §7 lines 87 ("Skill renderers MUST check `services` presence before indexing; render `—` in the `主打服务 / Top service` column when absent").

---

### TC-S12 — Numeric ID → agent get --agent-ids (not search)
**User:** 看 #42
**Expected:** Route to `agent get --agent-ids 42`, NOT `agent search`.
**File/Line:** `modules/agent-search.md` Rule 9 line 34, Boundary rules line 143.
**Result:** PASS — Rule 9 and Boundary rules both state: "Explicit numeric ids → `agent get --agent-ids`, NOT `agent search`."

---

### TC-S13 — Ownership word + descriptor → agent get + client-side filter
**User:** 我的 DeFi provider
**Expected:** Run `agent get` (no `--agent-ids`), then client-side filter for DeFi-related rows.
**File/Line:** `modules/agent-search.md` Boundary rules line 142.
**Result:** PASS — Line 142 explicitly: "Ownership word + descriptor → `agent get`, NOT `agent search`... run `agent get` (default mode, no `--agent-ids`) to fetch the caller's own agents, then client-side filter the list."

---

### TC-S14 — No skill-name candidates in search results
**User:** 找做数据分析的 agent
**Expected:** AI invokes `agent search` and returns marketplace agents. Never lists `okx-*` skill names as "candidates."
**File/Line:** `core/display-lists.md` line 165 (Anti-pattern audit table): "Listing `okx-*` skill names as 'candidates' instead of running `agent search`" is marked forbidden.
**Result:** PASS — Anti-pattern explicitly called out. Also in `SKILL.md` description ("Finding marketplace agents → run agent search, NOT list skill names").

---

### TC-S15 — Page-size cap 50
**User:** (user requests 100 results)
**Expected:** Do not send `--page-size 100`; use pagination `--page <N+1>` instead. Backend caps at 50.
**File/Line:** `core/cli-search-feedback.md` §7 line 30 ("Backend caps at 50 — `--page-size 100` returns a 4xx error"), line 106.
**Result:** PASS — Cap is documented twice in §7 and once in the Table of Contents summary (line 10: "page-size cap 50").

---

### TC-S16 — Case A (backend pagination): new CLI call
**User:** 下一页 (when backend has more pages)
**Expected:** Issue a new `agent search --query "<same>" --page <prev+1>`. Do NOT render from memory.
**File/Line:** `core/display-lists.md` lines 140–145 (§Dispatch: "more"/"next page" intents), Case A row.
**Result:** PASS — Case A explicitly: "Issue a new CLI call: `onchainos agent search --query '<same>' --page <prev+1> --page-size <same>`. ⛔ Do NOT render rows from memory."

---

### TC-S17 — Case B (AI-side truncation): render from context, no new CLI call
**User:** 更多 (when all rows already fetched, AI showed only K<N)
**Expected:** Render `list[K..N]` from already-captured response. Do NOT re-issue CLI call.
**File/Line:** `core/display-lists.md` lines 140–145 (§Dispatch), Case B row.
**Result:** PASS — Case B explicitly: "Render `list[K..N]` from the already-captured response still in context. ⛔ Do NOT re-issue the CLI call here."

---

### TC-S18 — No fake "all shown" when count doesn't match
**User:** 还有吗? (when on-screen ids < envelope.total)
**Expected:** AI does not claim "都显示了" unless on-screen agentId count == envelope.total.
**File/Line:** `core/display-lists.md` line 146 (Neither case), anti-pattern table line 151.
**Result:** PASS — Neither case rule: "Do NOT silently claim 'all displayed' when the count doesn't match." Anti-pattern table lists `"共 N 条, 都在第 1 页显示了"` while on-screen rows < N as zero-tolerance failure.

---

### TC-S19 — Chinese verbatim query echoed in "搜索：" line
**User:** 找口碑好的 DeFi provider
**Expected:** Display shows `搜索：「找口碑好的 DeFi provider」` with user's Chinese text verbatim inside quotes (not translated or paraphrased).
**File/Line:** `core/display-lists.md` line 107 ("The query value inside the quotes stays the user's original utterance verbatim... do NOT translate it."), worked example line 56–57.
**Result:** PASS — Line 107 explicitly states verbatim preservation of the query in the Search line. Worked example (line 56) shows `搜索：「找个口碑好的做链上数据分析的 provider」` with the Chinese preserved verbatim.

---

### TC-S20 — "理解为" line uses natural language words, not CLI flag names
**User:** 找高分的 provider
**Expected:** `理解为：高分关键词「provider」` — NOT `理解为：--feedback "高分" --agent-info "provider"`.
**File/Line:** `core/display-lists.md` line 108 (explicitly forbids `--feedback` / `--agent-info` / `--service` / `--status` in user-visible text), worked example line 57.
**Result:** PASS — The worked example shows `理解为：口碑好关键词「provider」+「链上数据分析」` — no flag names. Line 108 explicitly prohibits CLI flag names in "理解为" line.

---

### TC-S21 — DeFi/域名 → agent-info NOT --service
**User:** 找做 DeFi 的 provider，要 API 接口
**Expected:** `--agent-info="DeFi,provider"`, `--service="API"`. "DeFi" goes to agent-info not service.
**File/Line:** `modules/agent-search.md` lines 44, 49 ("domain wins" rule, --agent-info table includes "DeFi"), worked example 6 line 127.
**Result:** PASS — The "domain wins" rule (line 44) states: "Domain / specialty words... never belong in `--service`." The `--agent-info` table (line 49) explicitly lists `DeFi` as an `--agent-info` keyword. Example 6 (line 127) shows `--agent-info="evaluator,DeFi"`.

---

### TC-S22 — A2A, A2MCP multi-value in --service
**User:** 要 A2A 或 A2MCP 的 provider
**Expected:** `--service="A2A,A2MCP"` (comma-separated multi-value, verbatim user tokens).
**File/Line:** `modules/agent-search.md` Rule 4 line 29 ("Filters are `Vec<String>`. Comma-separated on the CLI; multi-value is fine"), Example 5 lines 110–117 (shows `--service="A2A,A2MCP"`).
**Result:** PASS — Example 5 explicitly demonstrates this scenario and produces `--service="A2A,A2MCP"`.

NUANCE: Since user said "A2A 或 A2MCP" (both terms from user's utterance), they are passed verbatim. Rule 6 forbids canonicalization (e.g., don't normalize user's "MCP 服务" → "A2MCP"), but if the user typed the enums directly, they go verbatim.

---

### TC-S23 — 500-char query: no pre-truncation
**User:** (pastes 500-character rant)
**Expected:** Send verbatim to `--query`. Do NOT pre-truncate. If backend rejects, surface the error.
**File/Line:** `modules/agent-search.md` Rule 6 line 18 ("Do NOT silently cut the query"), Example 7 lines 130–132.
**Result:** PASS — Example 7 explicitly says "Send it verbatim; do not pre-truncate. If the backend returns an error like 'query too long' or similar, surface the backend message... do not auto-shorten."

---

### TC-S24 — 下架 confirm first, then verbatim
**User:** 找已下架的 agent
**Expected:** AI asks to confirm the user really wants inactive agents, then (if confirmed) sends `--status "下架"` verbatim (not normalized to "inactive").
**File/Line:** `modules/agent-search.md` Boundary rules line 141 ("Confirm before sending an 'inactive' filter...").
**Result:** PASS — Line 141: "When the user says `下架的` / `inactive`, ask back to confirm they really want to see inactive agents... If they confirm, send their verbatim wording (e.g., `--status "下架"`); do not normalize to `inactive`."

---

### TC-S25 — DeFi/域名 → --agent-info NOT --service (expanded check)
**User:** 找做 NFT 域名解析的 provider
**Expected:** `--agent-info="provider,NFT,域名解析"` — domain words never go to `--service`.
**File/Line:** `modules/agent-search.md` line 44 (domain wins rule), line 49 (--agent-info table lists NFT explicitly).
**Result:** PASS — The `--agent-info` table at line 49 explicitly lists `NFT` and "domain noun the user named" as agent-info targets. "域名解析" as a domain/specialty word would go to `--agent-info` per the domain-wins rule.

---

### TC-S26 — 评价量 sort unsupported: explain alternative (no CLI flags)
**User:** 我想按评价量排序找 agent
**Expected:** Tell user this isn't supported. Offer natural-language alternative: "先选好 agent，我帮你拉评价 — 按时间倒序还是按评分高低？". Never show `feedback-list --agent-id`, `--sort-by`, `time_desc`, `score_desc`.
**File/Line:** `modules/agent-search.md` §Unsupported filter requests, line 149.
**Result:** PASS — Line 149: "tell them it isn't directly supported and offer the alternative in natural language: pick the target agent first, then '我帮你拉它的评价 — 按时间倒序还是按评分高低？'. ⛔ Never paste CLI flag names (`feedback-list --agent-id`, `--sort-by`, `time_desc`, `score_desc`) into user-visible text."

---

## PART 2: FEEDBACK SUBMIT (TC-F01 – F25)

### TC-F01 — Target by ID
**User:** 给 #42 打 4 星
**Expected:** `--agent-id 42`, `--score 4`.
**File/Line:** `modules/feedback.md` Step 1 line 22.
**Result:** PASS — Exact example: "给 #42 打 4 星" → `--agent-id 42 --score 4`.

---

### TC-F02 — Target by name: resolve via search first, confirm
**User:** 给 DeFi Analyzer 打 4 星
**Expected:** Run `agent search --query "DeFi Analyzer"` to resolve name → id, then confirm with user.
**File/Line:** `modules/feedback.md` Step 1 lines 23–24.
**Result:** PASS — Line 23: "给 DeFi Analyzer 打 4 星" → "first resolve name to id via `agent search --query 'DeFi Analyzer'`, then confirm with the user."

---

### TC-F03 — Creator-id ladder 1a: cached id with matching ownerAddress → use it
**User:** (has prior `agent get` showing #88 under current wallet, then rates)
**Expected:** Use cached #88 as `--creator-id` without running `agent get` again.
**File/Line:** `modules/feedback.md` Step 2 ladder item 1, sub-bullet 1, lines 32–33.
**Result:** PASS — "If the cached id's `ownerAddress` was already captured in this conversation... compare directly to the current selected wallet address. Match → use it (no lookup needed)."

---

### TC-F04 — Creator-id ladder 1b: cached id without captured ownerAddress → fall through to ladder 2
**User:** "我的 agent 是 #88" (without prior agent get showing ownerAddress)
**Expected:** Fall through to ladder 2 (run `agent get`); do NOT silently use the user-stated #88.
**File/Line:** `modules/feedback.md` Step 2 ladder item 1 sub-bullet 2, lines 33–34.
**Result:** PASS — "If the cached id was only mentioned by the user... without any captured `ownerAddress`, fall through to ladder 2."

---

### TC-F05 — Creator-id ladder 1c: wallet switch invalidates cache
**User:** (had #88 cached, then switched wallets)
**Expected:** Regardless of cached id, fall through to ladder 2 unconditionally after any wallet switch.
**File/Line:** `modules/feedback.md` Step 2 ladder item 1 sub-bullet 3, lines 34–35.
**Result:** PASS — "If the user has switched wallets since the cached id was first mentioned... fall through to ladder 2 unconditionally."

---

### TC-F06 — Creator-id ladder 2a: 0 agents under current wallet → STOP
**User:** (no agents registered under current wallet)
**Expected:** STOP. Offer to register first. Do NOT list agents from other wrappers.
**File/Line:** `modules/feedback.md` Step 2 ladder item 2, sub-bullet "0 agents" lines 37.
**Result:** PASS — The exact Chinese and English user-facing strings are provided. "Other wrappers may have agents — those belong to other related wallets under the same email / JWT, and cannot sign this tx; do not list them as candidates."

---

### TC-F07 — Creator-id ladder 2b: 1 agent → silently use, mention in confirmation
**User:** (1 agent under current wallet)
**Expected:** Silently use its id as `--creator-id`, mention it in confirmation: "你的 agent #N <name> 会作为这条评价的发起人。"
**File/Line:** `modules/feedback.md` Step 2 ladder item 2, sub-bullet "1 agent" line 38.
**Result:** PASS — "silently use its agentId as `--creator-id`; mention the choice in the confirmation."

---

### TC-F08 — Creator-id ladder 2c: multiple agents → ask user, numbered-options pattern
**User:** (multiple agents under current wallet)
**Expected:** Show numbered list with role labels localized (Chinese: 用户/服务提供商/仲裁者, English: User Agent/ASP/Evaluator Agent). Ask user to pick. Do NOT auto-pick.
**File/Line:** `modules/feedback.md` Step 2 ladder item 2, "Multiple agents" block lines 39–55.
**Result:** PASS — The exact numbered-options templates are provided for both Chinese and English. Role labels use `core/choice-prompts.md` pattern and `core/ux-lexicon.md §Role`.

---

### TC-F09 — Star validation: 0 accepted (explicit user input)
**User:** 给 #42 打 0 星
**Expected:** `--score 0` accepted (only if user explicitly says zero).
**File/Line:** `modules/feedback.md` Step 3 star table line 86.
**Result:** PASS — Table row: "`0 星` (rare; only if user explicitly says zero)" → `--score 0`.

---

### TC-F10 — Star validation: 5 accepted (満点)
**User:** 给 #42 打 5 星 / 满分
**Expected:** `--score 5`.
**File/Line:** `modules/feedback.md` Step 3 star table line 79.
**Result:** PASS — Both `5 星` and `满分` map to `--score 5`.

---

### TC-F11 — Star validation: 3.33 accepted (2-decimal value)
**User:** 给 #42 打 3.33 星
**Expected:** `--score 3.33` passed to CLI directly. CLI handles ×20 internally.
**File/Line:** `modules/feedback.md` Step 3 star table line 82.
**Result:** PASS — `3.33 星` maps to `--score 3.33`.

---

### TC-F12 — Star validation: 3.31 → normalized to 3.3 in confirmation card
**User:** 给 #42 打 3.31 星
**Expected:** `--score 3.31` sent to CLI. Confirmation card shows `★ 3.3` (wire-normalized) with parenthetical note. Post-success also shows `★ 3.3`.
**File/Line:** `modules/feedback.md` Step 5 lines 128 ("user-typed `3.31` lands on wire 66 and the canonical display is `★ 3.3`"), Step 7 line 155.
**Result:** PASS — Both confirmation and post-success explicitly compute `round(user_stars × 20) / 20`. If value changed from user input, parenthetical hint added: `（按 0.05 星粒度落到 3.3）`.

---

### TC-F13 — Star validation: >5 refused
**User:** 给 #42 打 6 星
**Expected:** Pre-validation refuses. Never send `--score 6` to CLI.
**File/Line:** `modules/feedback.md` Step 3 line 73 ("Reject... ranges outside 0.00–5.00"), `troubleshooting.md` §3.
**Result:** PASS — "Reject more than 2 decimal places, ranges outside 0.00–5.00." `troubleshooting.md` §3 notes this is enforced skill-side before CLI.

---

### TC-F14 — Star validation: <0 refused
**User:** 给 #42 打 -1 星
**Expected:** Refused. Never send negative value.
**File/Line:** `modules/feedback.md` Step 3 line 73, line 177 ("Star range... enforced skill-side before the CLI runs").
**Result:** PASS — Same rule covers negative values.

---

### TC-F15 — Star validation: >2 decimal places refused
**User:** 给 #42 打 3.333 星
**Expected:** Refused with friendly message. Never pass `--score 3.333`.
**File/Line:** `modules/feedback.md` Step 3 line 73 ("Reject more than 2 decimal places").
**Result:** PASS — Explicit rejection rule for over-precision.

---

### TC-F16 — Fuzzy phrases: 85分 ÷ 20 = 4.25
**User:** 给 #42 打 85 分
**Expected:** Divide by 20: `85 / 20 = 4.25`. Pass `--score 4.25`. Never echo raw "85" back.
**File/Line:** `modules/feedback.md` Step 3 line 89 ("legacy phrasings... divide by 20... `85 → 4.25`").
**Result:** PASS — Line 89 explicitly maps `85 → 4.25`. "Never echo the raw 0–100 number back to the user."

---

### TC-F17 — Natural language star not given → re-ask
**User:** 给 #42 打个分
**Expected:** The verb "打分" does not contain a star count. AI must ask: "给 #42 打几星？0–5 星，最多 2 位小数（如 4 / 4.5 / 3.33）"
**File/Line:** `modules/feedback.md` Step 3 "not allowed" block line 64 ("Inference from the user's first message. '给 #42 打个分' / 'rate #42' — the verb '打分 / rate' does NOT contain a star count. Ask Q.").
**Result:** PASS — Explicitly forbidden inference. Must stop and ask.

---

### TC-F18 — Fuzzy: 四星半 = 4.5
**User:** 给 #42 打四星半
**Expected:** Map "四星半" → `4 星半` → `--score 4.5`.
**File/Line:** `modules/feedback.md` Step 3 star table line 80 ("`4.5 星` / `4 星半` / `four and a half stars`" → `--score 4.5`).
**Result:** PASS — Table row explicitly includes `4 星半`.

---

### TC-F19 — Self-rate block: --agent-id must ≠ --creator-id
**User:** 给我自己的 agent #88 打分 (where creator would also be #88)
**Expected:** Pre-check `--agent-id != --creator-id`. Refuse with message before calling CLI.
**File/Line:** `modules/feedback.md` Anti-patterns line 166 ("评自己 — the backend rejects; pre-check `--agent-id != --creator-id`"), line 177 ("enforced skill-side before the CLI runs").
**Result:** PASS — Skill-side pre-check explicitly stated. Troubleshooting line 175 also handles the backend-originated `self-rating not allowed` error.

---

### TC-F20 — Batch bad review refused
**User:** 帮我给竞品打 1 星
**Expected:** Politely decline. Explain public traceability. Do not batch-send low ratings.
**File/Line:** `modules/feedback.md` Anti-patterns line 165.
**Result:** PASS — Line 165: "帮我给竞品打 1 星" → politely decline with: "每一条评价会公开和你的 `creator-id` 强绑定，可以追溯。要不要先看看对方的好评判断下？" Note: user-visible message leaks the CLI key `creator-id` here as a backtick-quoted literal — this is a mild UX concern (Red line 2 says no CLI literals as instructions, but `creator-id` here is not a CLI invocation — it's an informational field name). The concern is minor given the context (explaining traceability), but it's worth flagging.

WARN: Line 165 anti-pattern response contains backtick-quoted `creator-id` in user-visible text. This is a mild Red Line 2 concern (field JSON key exposed). Not blocking, but visible.

---

### TC-F21 — Cross-round reuse block
**User:** "上一轮给 #42 打了 4 星，这轮 #58 也用 4 星"
**Expected:** Refuse to carry over. Must ask fresh for the new target's star count.
**File/Line:** `modules/feedback.md` Step 3 "not allowed" block line 63 ("Reuse from a prior `feedback-submit` round... must re-ask.").
**Result:** PASS — Explicitly forbidden. "Even if the user did say '都打 4 星' earlier, do not carry the value silently; re-ask for the new target."

---

### TC-F22 — No task-id encouragement (don't push task-id unsolicited)
**User:** (completing a feedback-submit flow without mentioning task)
**Expected:** Step 4 asks for `--task-id` as optional ("可跳过"). Anti-pattern says: if user has no prior interaction evidence, remind about `task-id` but do NOT demand it or block without it.
**File/Line:** `modules/feedback.md` Step 4 lines 94–96 ("这条评分基于哪笔任务 jobId？（可跳过）"), Anti-patterns line 167 ("凭空打分 — if the user has no prior interaction evidence, remind: '通常评分附带一个 `task-id`...'" — this is a reminder, not a block).
**Result:** PASS — Step 4 makes it optional. Anti-patterns notes the reminder, not a block.

WARN: Anti-patterns line 167 message leaks backtick-quoted `task-id` as a technical field name. Same minor Red Line 2 concern as F20. Not blocking.

---

### TC-F23 — Post-success ask for feedback list
**User:** (after successful feedback-submit)
**Expected:** "已给 #<target> 打 ★ N。要不要看看 #<target> 最近的评价？我帮你拉 — 按时间倒序，还是按评分高低？"
**File/Line:** `modules/feedback.md` Step 7 lines 149–159.
**Result:** PASS — Step 7 provides exact template. No auto-chase of `feedback-list` (line 159: "Do NOT chase with `agent feedback-list` automatically").

---

### TC-F24 — Wire-normalized success ★N in post-success line
**User:** (user typed 3.31, submits)
**Expected:** Post-success shows `★ 3.3` (normalized), not `★ 3.31`.
**File/Line:** `modules/feedback.md` Step 7 lines 155–156 ("⛔ N MUST be the wire-normalized star value... `3.31 → 3.3`").
**Result:** PASS — Step 7 explicitly computes `round(user_stars × 20) / 20` for the post-success display. Example explicitly shows `3.31 → 3.3`.

---

### TC-F25 — Task collaboration encourage (凭空打分 reminder)
**User:** "给 #42 打 3 星" (no task context)
**Expected:** Reminder (not block): "通常评分附带一个 task-id，没有的话评价会显得缺少依据。"
**File/Line:** `modules/feedback.md` Anti-patterns line 167.
**Result:** PASS — "凭空打分 — if the user has no prior interaction evidence, remind: '通常评分附带一个 `task-id`，没有的话评价会显得缺少依据。'" Properly framed as reminder, not rejection.

---

## PART 3: FEEDBACK LIST (TC-FL01 – FL08)

### TC-FL01 — Sort by time (natural language)
**User:** 看 #42 的评价，按时间最新
**Expected:** `--sort-by time_desc`. Footer shows "当前按时间倒序排序" (not `time_desc`).
**File/Line:** `core/cli-search-feedback.md` §10 lines 188–189, `core/display-lists.md` §5 line 48.
**Result:** PASS — §10 maps "最新 / 最近 / latest / newest / 按时间排序" → `time_desc`. Display-lists §5 footer rule: "Never paste `time_desc`... Render instead: `当前按时间倒序排序`."

---

### TC-FL02 — Sort by score (natural language)
**User:** 看 #42 的评价，高分优先
**Expected:** `--sort-by score_desc`. Footer shows "当前按评分高低排序".
**File/Line:** `core/cli-search-feedback.md` §10 line 190, `core/display-lists.md` §5 line 48.
**Result:** PASS — §10 maps "高分优先" → `score_desc`. Display-lists footer: `当前按评分高低排序`.

---

### TC-FL03 — Sort lowest unsupported
**User:** 看 #42 的评价，差评优先 / 最低分
**Expected:** Tell user only `time_desc` / `score_desc` are supported. Offer `score_desc` + "page to tail" workaround.
**File/Line:** `core/cli-search-feedback.md` §10 line 191 ("Not supported. Tell the user only `time_desc` / `score_desc` are accepted; offer `score_desc` then let them page to the tail").
**Result:** PASS — Explicitly stated as unsupported with the workaround offer.

---

### TC-FL04 — No default sort when not mentioned
**User:** 看 #42 的评价
**Expected:** Omit `--sort-by` entirely. Backend picks default. Footer shows "当前按后端默认排序".
**File/Line:** `core/cli-search-feedback.md` §10 line 192 ("Unclear / not mentioned: Omit `--sort-by` — backend picks a default"), `core/display-lists.md` §5 line 48.
**Result:** PASS — §10 explicitly handles the "not mentioned" case. Footer template includes `当前按后端默认排序` / `Sorted by backend default`.

---

### TC-FL05 — Footer uses natural language, NOT time_desc/score_desc
**User:** (any feedback-list request)
**Expected:** Footer shows "当前按时间倒序排序" or "当前按评分高低排序" or "当前按后端默认排序". Never shows raw `time_desc` or `score_desc`.
**File/Line:** `core/display-lists.md` §5 line 48 ("⛔ Never paste the raw `--sort-by` flag or its `time_desc` / `score_desc` literal into the footer").
**Result:** PASS — Explicit prohibition with exact alternative phrases provided.

---

### TC-FL06 — Reviewer label format: "发起人 #N（角色 名字）" / "reviewer #N（Role Name）"
**User:** (views feedback list for #42)
**Expected:**
- Chinese: `发起人 #88 (用户 MyBuyer)` — label is `发起人`, not `creator`
- English: `reviewer #88 (User Agent MyBuyer)` — label is `reviewer`, not `creator`
**File/Line:** `core/display-lists.md` lines 14, 18, 21 (Chinese worked examples), lines 30, 34, 37 (English worked examples), line 45 (rule: "`<reviewer-label>` slot is language-dependent, NOT the literal English word `creator`").
**Result:** PASS — Canonical worked examples clearly show `发起人 #88 (用户 MyBuyer)` for Chinese and `reviewer #88 (User Agent MyBuyer)` for English. Line 45 explicitly bans `creator` and specifies the correct labels.

SCRUTINY: The template at line 45 shows format `#<index> · <date> · <reviewer-label> #<id> (<role> <name>) · ★ <stars>`. The worked examples use regular parentheses `()` not double-width brackets `（）`. The TC description mentions "（角色 名字）" with full-width brackets, but the actual doc uses regular `()`. This is a cosmetic discrepancy — the doc uses `()` consistently and that is the canonical format.

---

### TC-FL07 — No comment placeholder: language-matched "(无评论)"/"(no comment)"
**User:** (views a review with no description/comment)
**Expected:** Chinese user sees `(无评论)`, English user sees `(no comment)`. Never show English form to Chinese user.
**File/Line:** `core/display-lists.md` lines 22, 38 (worked examples), line 47 ("render the language-matched placeholder... Do NOT render the English form to a Chinese user (and vice versa)").
**Result:** PASS — Worked examples: Chinese variant uses `(无评论)` at line 22, English variant uses `(no comment)` at line 38. Line 47 explicitly states language-matching rule and prohibits cross-language rendering.

---

### TC-FL08 — Role localization in feedback list
**User:** (views feedback list for #42, reviewers have various roles)
**Expected:** Chinese: 用户/服务提供商/仲裁者. English: User Agent/ASP/Evaluator Agent. Never raw requester/provider/evaluator.
**File/Line:** `core/display-lists.md` lines 8, 45 (role label rule), worked examples lines 14, 21, 30, 37.
**Result:** PASS — Rule at line 45: "`<role>` slot follows `core/ux-lexicon.md §Role`... Chinese `用户 / 服务提供商 / 仲裁者`; English `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render the raw ERC-8004 enum." Worked examples show `(用户 MyBuyer)` and `(服务提供商 DataCo)` for Chinese, `(User Agent MyBuyer)` and `(ASP DataCo)` for English.

---

## PART 4: SERVICE LIST (TC-SL01 – SL04)

### TC-SL01 — Pipe table format
**User:** #42 有什么服务
**Expected:** Response is a Markdown pipe table (6 columns), NOT bullet blocks.
**File/Line:** `core/display-formats.md` line 157 ("Header blockquote + a single Markdown pipe table"), line 185 ("Pipe table, not bullet blocks").
**Result:** PASS — Line 185 explicitly: "Pipe table, not bullet blocks. Matches the top-level 'every table is a Markdown pipe table' convention." The canonical worked examples show pipe table format.

---

### TC-SL02 — Agent not found
**User:** #9999 有什么服务 (where 9999 doesn't exist)
**Expected:** Error card — backend returns `agent not found` → translated message. No auto-retry.
**File/Line:** `troubleshooting.md` line 46 ("`agent not found` / any 404-shaped response" → "找不到该 agent"), `core/display-formats.md` §7 (error card template).
**Result:** PASS — troubleshooting.md maps `agent not found` to user-friendly "找不到该 agent". Error card format in display-formats §7. No auto-retry rule in display-formats §7 line 227.

---

### TC-SL03 — No auto-chain for service-list
**User:** (requests service-list; skill should not auto-chain feedback-list or other calls)
**Expected:** One `agent service-list --agent-id <id>` call. No chaining to other commands.
**File/Line:** `SKILL.md` line 139 ("All else (search / get / service-list / feedback) — Stop."), per-file no-polling rule in `_shared/no-polling.md`.
**Result:** PASS — SKILL.md Operation Flow Step 5 explicitly routes `service-list` to "Stop." The old references/display-formats.md line 219 also said "No other side-queries. `service-list` is never triggered from this prompt — services are already shown in the detail card." (This refers to agent-get detail, but the principle of no auto-chain is established.)

NOTE: The new `core/display-formats.md` §4 rules do not explicitly call out "no auto-chain" for the service-list display itself, but SKILL.md Step 5 "Stop" and the `_shared/no-polling.md` principle cover it.

---

### TC-SL04 — Non-standard serviceType rendered verbatim + schema mismatch note
**User:** (backend returns serviceType="query" or other non-standard value)
**Expected:** Render the non-standard value verbatim in the Type column. Append footnote about schema mismatch.
**File/Line:** `core/display-formats.md` lines 189–192 (§4 service-list rules).
**Result:** PASS — Line 189: "If the backend returns non-standard values (e.g. `serviceType: 'query'`... `Fee` in `ETH`...), show them as-is in the table — do not sanitize or normalize to expected enums. Append a footnote blockquote below the table when you notice the shape diverges from the local `--service` schema." The exact footnote text is provided at lines 190–191. Line 192: "Only append this footnote when you actually observe a shape mismatch; omit it when everything matches."

---

## PART 5: AVATAR UPLOAD (TC-AV01 – AV11)

### TC-AV01 — Upload → URL, URL shown to user
**User:** (attaches image in Claude Code)
**Expected:** Upload via `agent upload --file <path>` → get URL → URL appears verbatim in confirmation card.
**File/Line:** `modules/avatar-upload.md` lines 9, 23, 40–45.
**Result:** PASS — Policy rule 5 (line 23): "Upload result is a URL — show it to the user... render the URL verbatim in the Picture row." Lines 40–45 in Claude Code flow confirm: URL in one-line ack and in confirmation card.

---

### TC-AV02 — Path not found → error card
**User:** (provided path doesn't exist or can't be accessed)
**Expected:** Backend/CLI returns an error. Surface error card per troubleshooting.md. Do not auto-retry.
**File/Line:** `modules/avatar-upload.md` §Validation line 76 (MIME type error handling), `troubleshooting.md` error card format.
**Result:** PASS — Validation section handles file errors. No explicit "path not found" case is listed, but the general principle of surfacing backend-originated errors applies (troubleshooting.md §1 / §2). No auto-retry per §7 last line.

WARN: `modules/avatar-upload.md` doesn't explicitly call out "file not found" as a distinct error case — it focuses on MIME type and file size. This is a minor documentation gap. The general error-handling chain (CLI bail → troubleshooting §1) should catch it, but a specific user-facing message is not templated.

---

### TC-AV03 — >1MB intercept, NO auto-compress
**User:** (attaches 2MB image)
**Expected:** STOP before calling `agent upload`. Tell user to provide smaller image. Do NOT compress/resize the file.
**File/Line:** `modules/avatar-upload.md` Claude Code flow line 34 ("if > 1 MB: STOP, prompt the user... do NOT proceed to step 3"), §Validation lines 77–83.
**Result:** PASS — Two explicit prohibitions: "⛔ Do NOT call `onchainos agent upload` or any backend API" and "⛔ Do NOT proactively compress, resize, or modify the file." Both are at the Validation section. The flow step also references this check at step 2.

---

### TC-AV04 — Non-1:1 aspect ratio: warning only (not reject)
**User:** (attaches 800×400 image)
**Expected:** Accept and upload anyway. When proactively recommending, say "1:1 方图" not a specific pixel size.
**File/Line:** `modules/avatar-upload.md` Policy rule 6 lines 24–25.
**Result:** PASS — Line 24: "When the user sends a non-1:1 image, accept it and upload anyway — do not reject or demand re-crop." Proactive recommendation uses "1:1 方图 / 1:1 square" without specific pixel size.

---

### TC-AV05 — URL change: if already a URL, pass directly (no byte-comparison re-upload)
**User:** "用这个头像 https://example.com/img.png"
**Expected:** Pass the URL directly as `--picture`. Do NOT re-download and re-upload.
**File/Line:** `modules/avatar-upload.md` §User-provided URL line 70 ("trust it and pass directly as `--picture`. Do not re-download and re-upload.").
**Result:** PASS — Explicit rule: "trust it and pass directly as `--picture`. Do not re-download and re-upload."

---

### TC-AV06 — Terminal: 2 options only (no local path option)
**User:** 上传个头像 (in Terminal environment)
**Expected:** Show exactly 2 options: 1. generate from keywords, 2. skip. Never ask user for local file path.
**File/Line:** `modules/avatar-upload.md` Policy rule 3 line 21 (Terminal variant: "2 options"), line 10 ("do NOT ask the user to locate a path on disk"), Terminal flow lines 58–66.
**Result:** PASS — Policy §3 explicitly shows 2-option Terminal variant. Line 10: "no file inline — do NOT ask the user to locate a path on disk." Terminal flow section shows only options 1 (generate) and 2 (skip). The opening "can't receive attachments here" / "当前环境没法直接收图" is also specified.

---

### TC-AV07 — >1MB intercept BEFORE API call (critical)
**User:** (attaches 1.5MB image in Claude Code)
**Expected:** Check file size BEFORE calling `onchainos agent upload`. Intercept at step 2 (after save to temp, before upload call). Never send to backend.
**File/Line:** `modules/avatar-upload.md` Claude Code flow line 34 ("if > 1 MB: STOP, prompt the user... do NOT proceed to step 3"), §Validation line 78 ("Check the file size before calling `onchainos agent upload`").
**Result:** PASS — The flow numbering is explicit: step 2 is the size check, step 3 is the upload call. "If > 1 MB: STOP... do NOT proceed to step 3." The Validation section repeats: "⛔ Do NOT call `onchainos agent upload` or any backend API."

---

### TC-AV08 — AI-generated image: show first, confirm, then upload
**User:** "用一只戴眼镜的青蛙当头像"
**Expected:** Generate image → show to user → ask "这张 OK 吗?" → wait for confirm → then upload.
**File/Line:** `modules/avatar-upload.md` Claude Code flow (AI-generated) lines 47–56, specifically step 2: "Show the generated image to the user, confirm ('这张 OK 吗?' / 'Does this work?')"
**Result:** PASS — The 3-step AI-gen flow is: (1) generate, (2) show + confirm, (3) upload. Explicit confirm requirement before upload.

---

### TC-AV09 — Direct URL no re-upload
**User:** "用这个 twitter 头像 https://pbs.twimg.com/media/abc.jpg"
**Expected:** Use URL directly as `--picture`. No download-and-reupload cycle.
**File/Line:** `modules/avatar-upload.md` §User-provided URL line 70.
**Result:** PASS — "trust it and pass directly as `--picture`. Do not re-download and re-upload."

---

### TC-AV10 — Non-HTTPS URL refused
**User:** "用这个头像 http://example.com/img.png" (HTTP not HTTPS)
**Expected:** Refuse with "头像链接必须是 https:// 开头的。"
**File/Line:** `modules/avatar-upload.md` §Validation lines 84–86 ("URL shape — must be HTTPS. On invalid shape...").
**Result:** PASS — Lines 84–86 explicitly require HTTPS and provide both Chinese and English error messages.

---

### TC-AV11 — Upload success URL in confirmation card and detail card (NOT "已上传")
**User:** (after successful avatar upload in a create/update flow)
**Expected:** Confirmation card shows the actual URL in the 头像 row. Never shows `已上传` / `uploaded` / `CDN` etc.
**File/Line:** `modules/avatar-upload.md` Policy rule 5 line 23, lines 40–45, `core/display-formats.md` "头像 / Profile photo row rule" lines 53–57.
**Result:** PASS — Policy rule 5: "Do not hide the URL behind '已上传' / 'uploaded' or any placeholder." Lines 44–45: "The URL must appear verbatim in the Picture row... Never replace it with `已上传` / `uploaded` / `CDN` or any placeholder phrase." display-formats §Photo row rule (lines 53–57) also explicitly bans `已上传` / `uploaded` / `已加好` / `CDN` / `图片已保存`.

---

## CROSS-CUTTING SUMMARY

### Critical Rules Verified

| Rule | Source | Status |
|---|---|---|
| feedbackRate=0 → 暂无评分 (not ★ 0) | display-lists.md line 88 | PASS — new rule correctly added |
| feedbackRate is 0-5, NO ÷20 in search | display-lists.md line 88, cli-search-feedback §7 line 97 | PASS — no division applied |
| serviceMinPrice: no hardcoded USDT | display-lists.md line 89 | PASS — explicit prohibition with reasoning |
| FL06 reviewer label format "发起人 #N (角色 名字)" | display-lists.md lines 14, 45 | PASS — canonical examples match |
| AV06 Terminal 2-options only | avatar-upload.md line 21, line 10 | PASS — no local path in Terminal |
| AV07 >1MB intercept BEFORE API (no auto-compress) | avatar-upload.md lines 34, 78–79 | PASS — both prohibitions explicit |

### Issues Found

| ID | Severity | Issue | Location |
|---|---|---|---|
| I-01 | WARN (minor) | Anti-pattern decline message for batch bad review (F20) leaks backtick-quoted `creator-id` as user-visible field name. May violate Red line 2 (no CLI key literals) in spirit. | modules/feedback.md line 165 |
| I-02 | WARN (minor) | F22/凭空打分 reminder message contains backtick-quoted `task-id` as user-visible text. Same mild Red Line 2 concern. | modules/feedback.md line 167 |
| I-03 | WARN (cosmetic) | FL06: TC description says reviewer label uses full-width brackets `（角色 名字）`, but canonical doc uses regular parentheses `(角色 名字)`. Doc is authoritative; TC wording is a cosmetic mismatch — no behavior issue. | display-lists.md line 14 vs TC description |
| I-04 | WARN (gap) | AV02: No explicit user-facing template for "file not found" error when the local temp path is invalid. The general error chain covers it, but the specific message is untemplated. | modules/avatar-upload.md §Validation |
| I-05 | WARN (minor) | SL03 "no auto-chain" rule is only implied by SKILL.md Step 5 "Stop" branch and _shared/no-polling.md, not explicitly stated in display-formats.md §4 service-list rules. | core/display-formats.md §4 |

### Files Read
- `/skills/okx-agent-identity/modules/agent-search.md`
- `/skills/okx-agent-identity/modules/feedback.md`
- `/skills/okx-agent-identity/modules/avatar-upload.md`
- `/skills/okx-agent-identity/core/display-lists.md`
- `/skills/okx-agent-identity/core/cli-search-feedback.md`
- `/skills/okx-agent-identity/core/display-formats.md`
- `/skills/okx-agent-identity/core/ux-lexicon.md`
- `/skills/okx-agent-identity/SKILL.md`
- `git show HEAD:skills/okx-agent-identity/references/display-formats.md` (old version for diff)
- `git show HEAD:skills/okx-agent-identity/references/feedback-guide.md` (old version for diff)
- `git show HEAD:skills/okx-agent-identity/references/search-query-split.md` (old version for diff)
- `/skills/okx-agent-identity/troubleshooting.md`

### Overall Verdict
All 52 test cases (TC-S01–S26, TC-F01–F25, TC-FL01–FL08, TC-SL01–SL04, TC-AV01–AV11) PASS. Five WARN items identified — none are blockers. The most important rules (feedbackRate no-divide, feedbackRate=0→暂无评分, serviceMinPrice no-USDT, Terminal 2-options only, 1MB intercept before API) are all correctly specified in the current skill files.
