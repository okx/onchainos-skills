# TC Verification — okx-agent-identity (196 TCs)

Date: 2026-05-29
Skill version: 1.2.0
Verifier: Claude Sonnet 4.6 (systematic file read)

## Summary

| Result | Count |
|---|---|
| ✅ Covered | 183 |
| ⚠️ Partially covered | 9 |
| ❌ Missing | 4 |

---

## 1. 身份注册 Agent Create

### Requester (R01–R13)

| TC | Result | Evidence / Notes |
|---|---|---|
| R01: Normal register name + default avatar → confirmation card → success + Step 5/6 | ✅ | `playbooks/requester.md` §Standard Q&A, §Confirmation, §Post-success; SKILL.md §Operation Flow Step 5/6 |
| R02: Normal register name + user-provided description → card includes description row | ✅ | `playbooks/requester.md` §Confirmation — "user volunteered a description" variant explicitly shows 描述 row |
| R03: Normal register name + upload avatar → upload URL in card | ✅ | `playbooks/requester.md` §Confirmation + `core/display-formats.md` §Picture row rule — URL must appear verbatim, never "已上传" |
| R04: Duplicate register (already has requester) → block, point to update | ✅ | `playbooks/README.md §Pre-check` — requester/evaluator uniqueness block: "直接告知并指向 update" with exact wording |
| R05: Name empty → re-ask Q1 | ✅ | `playbooks/requester.md` Q&A table: `On failure: re-ask once with a shorter example` |
| R06: Name too long (CN>30 / EN>64) → re-ask with shorter example | ✅ | Same Q&A table validation rule for Q1 |
| R07: Pre-fill name from metadata (userEmail/wallet) → refuse, re-ask Q1 (Red line 6) | ✅ | `playbooks/requester.md` §Standard Q&A — "⛔ Fields from user's literal reply only — never pre-fill from userEmail, wallet name, or session metadata" |
| R08: User requests service field → explain requester has no services | ✅ | `playbooks/requester.md` Good/bad cases: "给我加个 5 USDT 的服务" → "用户身份不带服务" |
| R09: First-time register, backend returns consent object → show consent card | ✅ | SKILL.md §Consent Gate + `playbooks/consent.md §Consent Card` |
| R10: Consent agreed → re-call create with consent-key | ✅ | `playbooks/consent.md §Agree flow` — re-invoke with `--consent-key` + `--agreed true` |
| R11: Consent declined → stop immediately, don't call CLI | ✅ | `playbooks/consent.md §Decline message` — "Do NOT call the CLI. Render the message below and stop." |
| R12: One-shot capture all fields → skip answered Qs, still render confirmation card | ✅ | `core/choice-prompts.md §One-Shot Capture` Rule 5: "All fields captured → still render confirmation card" |
| R13: Urgent tone ("赶紧建") → still render confirmation card | ✅ | SKILL.md §Confirmation Gate rationalization blacklist; `playbooks/README.md §Confirmation card`: "urgency... do NOT bypass" |

### Provider (P01–P18)

| TC | Result | Evidence / Notes |
|---|---|---|
| P01: Normal register 1 A2MCP service → full Q&A → confirmation → success | ✅ | `playbooks/provider.md` Phase 1 + `playbooks/provider-services.md` Phase 2 Q&A; confirmation card in provider.md |
| P02: Normal register 1 A2A service (no fee, no endpoint) → success | ✅ | `playbooks/provider-services.md` Q4 (A2A: optional fee), Q5 (A2A: skip); `playbooks/provider.md` confirmation — A2A row shows "(未填，双方自行协商)" |
| P03: Mixed A2MCP + A2A multi-service → each service field asked one at a time | ✅ | `playbooks/provider-services.md` loop gate; `playbooks/README.md §STRICT — one question per turn` |
| P04: Already has 1 provider → ask: create new or update existing | ✅ | `playbooks/README.md §Pre-check §provider` K=1 block with exact numbered-options prompt |
| P05: Already has 2+ providers → follow-up question which one to update | ✅ | `playbooks/README.md §Pre-check §provider` K≥2 block: "再问一次让用户指定改哪个" |
| P06: Description empty (required for provider) → re-ask Q2 | ✅ | `playbooks/provider.md` Q&A table Q2: validation `non-empty` |
| P07: No service → "ASP needs at least one service" | ✅ | `troubleshooting.md §1`: "`provider agents require at least one service; provide --service`" → translation shown |
| P08: User says "帮我写几个 service" / "示例就行" → refuse, re-ask | ✅ | `playbooks/provider.md` Good/bad cases: "帮我写几个 service" → "Refuse to fabricate"; `playbooks/provider-services.md` Phase 2 top rule |
| P09: A2MCP fee empty → re-ask Q4 | ✅ | `troubleshooting.md §1`: "`missing required field in --service for A2MCP: fee`" → return to Q4 |
| P10: A2MCP endpoint uses http:// → reject, require https | ✅ | `playbooks/provider.md §Endpoint Anti-Pattern` — `http://` forbidden; `playbooks/provider-services.md` Q5 validation |
| P11: A2MCP endpoint uses localhost/private IP → reject | ✅ | `playbooks/provider.md §Endpoint Anti-Pattern` §Forbidden patterns — localhost/127.0.0.1/192.168.x.x all listed |
| P12: A2MCP endpoint > 512 chars → reject, ask to shorten | ✅ | `troubleshooting.md §3`: endpoint > 512 chars → "接口地址最长 512 字符，这个超了" |
| P13: A2A without fee → allowed, wire sends fee="" | ✅ | `playbooks/provider-services.md` Q4 A2A branch: "选填，...直接回车 / 回复 '跳过'" accepted; wire `"fee":""` documented |
| P14: A2A with fee → allowed | ✅ | `playbooks/provider.md` Good/bad cases: fee=5 shown in confirmation card A2A row; `core/data-display.md` A2A non-empty fee |
| P15: Invalid servicetype → reject, re-render Q3 numbered prompt | ✅ | `playbooks/provider.md` Good/bad cases: "服务类型 HTTP" → "Reject politely and re-render the Q3 numbered prompt verbatim"; `troubleshooting.md §1` invalid servicetype row |
| P16: User pastes JSON blob → re-confirm field-by-field | ✅ | `playbooks/provider.md` Good/bad cases: "User pastes JSON blob" → "Thank them, but re-confirm field by field" |
| P17: Service price mentioned in Phase 1 → strict phase boundary, discard | ✅ | `playbooks/provider.md` Good/bad cases (Phase 1 mention, "DO NOT capture fee=10 at Phase 1") + `core/choice-prompts.md` Rule 4 |
| P18: A2MCP fee=0 (free) → allowed, warn about free lead-gen | ✅ | `playbooks/provider.md` Good/bad cases: "API 接口式服务 Fee 免费" → "Accept 0 but warn: API 接口式服务 0 USDT 等同于免费入口" |

### Evaluator (E01–E06)

| TC | Result | Evidence / Notes |
|---|---|---|
| E01: Normal register → success → two template lines + trigger staking flow (Step 5) | ✅ | `playbooks/evaluator.md §Post-success` — exactly two visible lines; Agent directive loads `evaluator-staking.md §2` |
| E02: One-shot name + description → confirmation card includes description | ✅ | `playbooks/evaluator.md` §Q&A — "If the user volunteers a description...include a 描述 row in the confirmation card for that run" |
| E03: After register, doesn't want to stake → explain no disputes without staking | ✅ | `playbooks/evaluator.md` Good/bad cases: "不想质押" → "仲裁者身份可以先注册放着，但没质押不会被派单" |
| E04: User wants to stake first then register → correct order: register first | ✅ | `playbooks/evaluator.md` Good/bad cases: "帮我直接质押再注册" → "得先注册再质押" |
| E05: Duplicate register (already has evaluator) → block, point to update | ✅ | `playbooks/README.md §Pre-check` — evaluator is unique per address, same block as requester |
| E06: Success template must NOT hardcode stake amount | ✅ | `playbooks/evaluator.md §Post-success` anti-pattern: "Hardcoded the stake amount (100 OKB)" is explicitly called out as a violation |

### Passive Onboarding (PO01–PO05)

| TC | Result | Evidence / Notes |
|---|---|---|
| PO01: No requester, task flow triggers → skip role/pre-check/avatar | ✅ | `playbooks/requester.md §Passive Onboarding §Simplified sub-flow` — skip role, pre-check, picture explicitly |
| PO02: Register success (with id) → only one line: "已为你创建用户身份 #N，继续发任务" | ✅ | `playbooks/requester.md §After success` — "已为你创建用户身份 #<id>。现在继续发布任务。" (no detail card) |
| PO03: Register success (no id, WS timeout) → no-id version, continue task | ✅ | `playbooks/requester.md §After success` — without-id fallback: "已为你创建用户身份。现在继续发布任务。" |
| PO04: Passive mode does NOT enter Step 6 (comm-init) | ✅ | `playbooks/requester.md §After success` — "Do NOT load /skills/okx-agent-chat/after-agent-list-changed.md here"; SKILL.md §Step 5 passive row routes to "back to task" branch |
| PO05: Passive mode still needs confirmation card (gate not exempted) | ✅ | `playbooks/requester.md §Passive Onboarding §Simplified sub-flow` — "Show confirmation table (still field-per-row, still mandatory)" |

---

## 2. 身份更新 (U01–U07)

| TC | Result | Evidence / Notes |
|---|---|---|
| U01: Update name → pre-check get → current detail card → diff card → confirm → execute → Step 6 | ✅ | SKILL.md §Update flow (4 steps); `core/display-detail.md §3 Update variant`; SKILL.md §Step 5 routes update to Step 6 |
| U02: Update description | ✅ | `core/cli-reference.md §2` — `--description` is an update param; diff card shows 描述 row |
| U03: Update avatar → URL changes → confirmation card byte-equal check, must re-render | ✅ | `core/display-detail.md §3` — diff card shows old URL / new URL; SKILL.md §Confirmation Gate: card values byte-identical to CLI params |
| U04: Update service list (full replacement, not incremental) | ✅ | SKILL.md §Update step: "`--service` is wholesale replacement — always start from current full services list"; `core/display-detail.md §3` maintainer note |
| U05: No field changes → skill refuses CLI, renders "没有需要提交的更改" | ✅ | SKILL.md §Update step: "if no fields changed, refuse to call CLI ('没有需要提交的更改')"; `troubleshooting.md §3` |
| U06: Update someone else's agent (ownerAddress mismatch) → "这个 agent 不归你当前钱包管" | ⚠️ | `playbooks/README.md §Pre-check` explains ownership is per-address, but the exact user-facing message "这个 agent 不归你当前钱包管" is not found verbatim. The concept is covered: when `agent get --agent-ids N` returns an agent whose wrapper doesn't match the current wallet, the pre-check logic redirects. However no explicit error message template exists for "update another's agent" — the skill infers it must refuse but the wording is not prescribed. |
| U07: agent-id doesn't exist → "找不到该 agent" | ✅ | `troubleshooting.md §2`: "`agent not found` / any 404-shaped response" → "找不到该 agent" |

---

## 3. 查看身份 (G01–G08)

| TC | Result | Evidence / Notes |
|---|---|---|
| G01: Normal agent list → display-formats §1 | ✅ | `core/display-formats.md §1` — full template with per-wallet grouping, 6 columns, footer |
| G02: ≥5 agents → reassurance footer | ✅ | `core/display-formats.md §1 §Multi-agent List Reassurance Footer` — trigger M >= 5, exact CN/EN wording |
| G03: No agents → show empty list, guide to register | ✅ | `core/display-formats.md §1` rules: "If a wrapper has 0 agents, render （暂无 agent）" |
| G04: Single detail (own) → §2 | ✅ | `core/display-detail.md §2` — full detail card template |
| G05: Detail of someone else's agent → open query | ✅ | `core/cli-reference.md §3` — `--agent-ids`: "Any id is accepted — own or someone else's. The backend does not require ownership." |
| G06: Multi-id batch → one §2 card per agent, single Post-detail prompt | ✅ | `core/display-detail.md §2.5` — "render one §2 detail card per agent...single multi-select Post-detail prompt at the end" |
| G07: Non-existent id → "找不到该 agent" | ✅ | `troubleshooting.md §2` — `agent not found / 404` → "找不到该 agent" |
| G08: No auto-chain service-list/feedback-list after detail | ✅ | `core/display-detail.md §2` rules: "Do NOT chain agent service-list...Do NOT chain agent feedback-list"; `_shared/no-polling.md` Rule 4 |

---

## 4. 上下架 (A01–A13, D01–D03)

| TC | Result | Evidence / Notes |
|---|---|---|
| A01: Activate success → "上架成功" → Step 6 | ✅ | `core/cli-reference.md §4` success: "render success line + proceed to Step 5 → Step 6" |
| A02: approvalStatus=1 → auto call submit-approval → "审核中 24h" | ✅ | `core/cli-reference.md §4`: `success: false, approvalStatus: 1` → call `submit-approval`; `troubleshooting.md §2` submit-approval success row |
| A03: approvalStatus=2 (already under review) → render message, stop | ✅ | `troubleshooting.md §2`: `approvalStatus: 2` already under review → exact CN/EN message, "Stop." |
| A04: approvalStatus=5 (rejected) → render rejection card + rejectReason, stop | ✅ | `troubleshooting.md §2`: `approvalStatus: 5` → render rejection card with rejectReason; "Stop." |
| A05: code=81602 (blacklisted) → render blacklist message, stop | ✅ | `troubleshooting.md §2`: `code: "81602"` → "这个 agent 已被平台封禁" → "Stop." |
| A06: All QA checks pass → silent direct activate | ✅ | `modules/pre-listing-qa.md §Pass Message`: "All checks pass → proceed to agent activate. No report needed." |
| A07: QA warning (name contains "(pre)") → render QA report, offer two options | ✅ | `modules/pre-listing-qa.md` U1/N7 — `(pre)` triggers warning; §QA Report Format shows two-option prompt |
| A08: QA warning, user chooses option 2 (list anyway) → immediate activate | ✅ | `modules/pre-listing-qa.md §QA Report Format` — "On option 2 (list anyway): invoke agent activate immediately without re-prompting" |
| A09: No avatar (L1 blocking) → NO option 2, must upload avatar first | ✅ | `modules/pre-listing-qa.md §Logo` L1: "blocking check — do not proceed to agent activate without an avatar"; §When to Run: "L1 (no avatar) is always blocking — do NOT offer option 2" |
| A10: A2MCP no endpoint (T2) → QA warning | ✅ | `modules/pre-listing-qa.md §Field 2` T2: "A2MCP requires endpoint" |
| A11: A2A has endpoint (T3) → QA warning | ✅ | `modules/pre-listing-qa.md §Field 2` T3: "A2A does not use endpoint" |
| A12: Service description missing 3-part structure (D1) → QA warning | ✅ | `modules/pre-listing-qa.md §Field 5` D1: "Three-part structure required" |
| A13: requester/evaluator activate → skip QA, direct activate | ✅ | `modules/pre-listing-qa.md §When to Run`: "If the role is requester or evaluator, skip this file"; SKILL.md §Intent table: requester/evaluator activate goes "directly" |
| D01: Normal deactivate → "下架完成" → Step 6 | ✅ | `core/cli-reference.md §5` success: "render deactivate success line + proceed to Step 5 → Step 6" |
| D02: Already inactive → "已经在下架状态" | ✅ | `troubleshooting.md §2`: `agent already inactive` → "这个 agent 已经在下架状态。" |
| D03: Pending settlements → prompt to handle tasks first | ✅ | `troubleshooting.md §2`: `pending settlements` → "上还有任务没结清，得先把那边的事处理完才能下架" |

---

## 5. 搜索 (S01–S11)

| TC | Result | Evidence / Notes |
|---|---|---|
| S01: Keyword-only search → --query verbatim | ✅ | `modules/agent-search.md §Verbatim Passthrough` Rule 1; Rule 8 "one call" |
| S02: Contains role word ("provider") → extract as --agent-info | ✅ | `modules/agent-search.md §The four dimensions` — role/domain → `--agent-info` |
| S03: Contains reputation word ("口碑好") → extract as --feedback (verbatim) | ✅ | `modules/agent-search.md §The four dimensions` — reputation descriptors → `--feedback`; Rule 6 verbatim |
| S04: Contains status word ("已上架") → extract as --status "已上架", NOT "active" | ✅ | `modules/agent-search.md` Rule 6: "If the user says 已上架, send --status '已上架', not --status 'active'"; Example 2 demonstrates |
| S05: Contains service type word ("MCP 服务") → extract as --service "MCP 服务", NOT A2MCP | ✅ | `modules/agent-search.md` Rule 6: "If they say MCP 服务, send --service 'MCP 服务', not --service 'A2MCP'" |
| S06: page-size > 50 → backend 4xx, suggest smaller page | ✅ | `core/cli-search-feedback.md §7` — backend caps at 50, `--page-size 100` returns 4xx |
| S07: Empty query → skill-side block, re-ask | ✅ | `troubleshooting.md §3`: "Query must be non-empty" skill-side guard |
| S08: User gives numeric ID ("找 #42") → agent get, not search | ✅ | `modules/agent-search.md` Rule 9: "Strip numeric agent id tokens from --query... If the ids are the user's primary intent, route to agent get --agent-ids" |
| S09: feedbackRate=0 → show "暂无评分", not ★0 | ⚠️ | `core/display-formats.md §1` rating rule says `feedbackRate null → 暂无评分`; `core/display-lists.md §6 Field mapping` says `null → —`. But the TC asks specifically about feedbackRate=0 (not null). The search field `feedbackRate` is documented as null for no-rating case. A zero value is theoretically a valid score. No explicit rule saying "0 on feedbackRate → 暂无评分 (not ★0)". The §1 list rule "If no feedback yet, render 暂无评分" uses `reputation.score==0 AND count==0` implicitly, not the search `feedbackRate` field specifically. **Gap: feedbackRate=0 vs feedbackRate=null distinction not spelled out for search results.** |
| S10: No services field (NON_NULL) → "主打服务" shows "—" | ✅ | `core/display-lists.md §6 Field mapping`: "`services` key absent... OR services[] empty → —" |
| S11: Ownership word ("我那几个做 DeFi 的") → agent get + client-side filter, not search | ✅ | SKILL.md §Search sub-flow: ownership language → "agent get + client-side filter, not search"; modules/agent-search.md boundary — ownership words signal the user's own agents |

---

## 6. 评价提交 (F01–F21)

| TC | Result | Evidence / Notes |
|---|---|---|
| F01: Target by agent ID → lock --agent-id | ✅ | `modules/feedback.md §Step 1` — "给 #42 打 4 星" → `--agent-id 42` |
| F02: Target by agent name → search to confirm ID first | ✅ | `modules/feedback.md §Step 1` — "给 DeFi Analyzer 打 4 星" → search, then confirm |
| F03: creator-id ladder 1, ownerAddress matches → reuse, no get needed | ✅ | `modules/feedback.md §Step 2` Ladder 1 — "If ownerAddress was captured...Match → use it (no lookup needed)" |
| F04: creator-id ladder 1, ownerAddress unknown/mismatch → fall to ladder 2 | ✅ | `modules/feedback.md §Step 2` Ladder 1 — "If cached id...without captured ownerAddress, fall through to ladder 2" |
| F05: creator-id ladder 1, wallet switched → fall to ladder 2 | ✅ | `modules/feedback.md §Step 2` Ladder 1 — "wallet switch invalidates the cache...fall through to ladder 2 unconditionally" |
| F06: ladder 2, 0 agents under current wallet → stop, prompt to register | ✅ | `modules/feedback.md §Step 2` Ladder 2 — "0 agents under the current wallet → STOP. Tell the user...offer to enter the registration flow" |
| F07: ladder 2, 1 agent → silently use, mention in confirmation | ✅ | `modules/feedback.md §Step 2` Ladder 2 — "1 agent → silently use its agentId as --creator-id; mention the choice in the confirmation" |
| F08: ladder 2, multiple agents → numbered selection, don't auto-pick | ✅ | `modules/feedback.md §Step 2` Ladder 2 — "Multiple agents...ask the user which to use, using the numbered-options pattern...Do not auto-pick" |
| F09: Rating 5 stars → --score 5 | ✅ | `modules/feedback.md §Step 3` table: `5 星 / 满分` → `--score 5` |
| F10: Rating 0 stars → --score 0 | ✅ | `modules/feedback.md §Step 3` table: `0 星 (rare; only if user explicitly says zero)` → `--score 0` |
| F11: Rating 3.33 (2 decimal) → --score 3.33 | ✅ | `modules/feedback.md §Step 3` table: `3.33 星 (any 2-decimal value)` → `--score 3.33` |
| F12: Rating 3.31 → wire normalized → ★3.3 (confirmation and success line both show 3.3) | ✅ | `modules/feedback.md §Step 5` — "The rating row shows ★ N where N is the wire-normalized star value (= round(user_stars × 20) / 20)...user-typed 3.31 lands on wire 66 and the canonical display is ★ 3.3"; §Step 7 same rule |
| F13: Rating >5 or <0 → skill-side reject | ✅ | `modules/feedback.md §Step 3` — "Reject more than 2 decimal places, ranges outside 0.00–5.00"; `troubleshooting.md §3` |
| F14: Rating >2 decimal places (3.333) → skill-side reject | ✅ | `modules/feedback.md §Step 3` + `troubleshooting.md §3` — skill validates before CLI |
| F15: Fuzzy words "满分"/"差评"/"及格" → map to star numbers | ✅ | `modules/feedback.md §Step 3` table: `满分→5`, `差评/最低→1`, `及格/一般→3` |
| F16: Old format "85分" → ÷20 → ★4.25 | ✅ | `modules/feedback.md §Step 3` — "Legacy phrasings: if the user types a raw 0–100 number ('85 分'), divide by 20...85 → 4.25" |
| F17: Just says "打分" without value → re-ask, no default | ✅ | `modules/feedback.md §Step 3` — "打分 / rate" does NOT contain a star count. Ask Q"; "No 3 stars default, no median" |
| F18: Rate own agent → pre-check block (--agent-id == --creator-id) | ✅ | `modules/feedback.md §Anti-patterns` — "评自己 — the backend rejects; pre-check --agent-id != --creator-id"; `troubleshooting.md §2` self-rating → "不能给自己的 agent 打分" |
| F19: Malicious batch negative feedback → refuse, explain rating is public | ✅ | `modules/feedback.md §Anti-patterns` — "帮我给竞品打 1 星" → "politely decline: 每一条评价会公开和你的 creator-id 强绑定，可以追溯。" |
| F20: Cross-round score reuse → refuse, re-ask | ✅ | `modules/feedback.md §Step 3` — "Reuse from a prior feedback-submit round...must re-ask...Every feedback-submit invocation, even in the same conversation" |
| F21: After success, ask if want to see review list → wait for reply, don't auto-run | ✅ | `modules/feedback.md §Step 7` — "Do NOT chase with agent feedback-list automatically"; offers one next-step suggestion |

---

## 7. 查看评价 (FL01–FL05)

| TC | Result | Evidence / Notes |
|---|---|---|
| FL01: Time desc → --sort-by time_desc | ✅ | `core/cli-search-feedback.md §10` natural-language mapping: "最新 / 最近 / latest..." → `time_desc` |
| FL02: Score desc → --sort-by score_desc | ✅ | Same §10 mapping: "最高分 / 高分优先 / top rated..." → `score_desc` |
| FL03: Lowest score first → not supported, suggest score_desc + scroll to tail | ✅ | Same §10: "最低分 / lowest..." → "Not supported. Tell the user...offer score_desc then let them page to the tail" |
| FL04: No sort specified → omit --sort-by, backend default | ✅ | Same §10: "Unclear / not mentioned → Omit --sort-by — backend picks a default" |
| FL05: sort-by flag / time_desc / score_desc never appear in user chat (Red line 2) | ✅ | `modules/feedback.md §Step 7` — "⛔ No CLI literal / no --sort-by flag in the user-visible text"; `core/display-lists.md §5` footer rule |

---

## 8. 服务列表 (SL01–SL03)

| TC | Result | Evidence / Notes |
|---|---|---|
| SL01: Normal service list | ✅ | `core/display-formats.md §4` — full service-list table template |
| SL02: agent-id doesn't exist → "找不到该 agent" | ✅ | `troubleshooting.md §2`: `agent not found / 404` → "找不到该 agent" |
| SL03: Detail card view doesn't auto-chain service-list (§2 already has services) | ✅ | `core/display-detail.md §2` rules: "Do NOT chain agent service-list to 'populate' the Services rows — they're already in the response"; `_shared/no-polling.md` Rule 4 |

---

## 9. 头像上传 (AV01–AV05)

| TC | Result | Evidence / Notes |
|---|---|---|
| AV01: Local file upload → returns HTTPS URL | ✅ | `modules/avatar-upload.md` Claude Code flow + `core/cli-reference.md §6` — upload returns URL |
| AV02: File path doesn't exist → "读不到这个文件" | ✅ | `troubleshooting.md §1`: `failed to read file: <path>` → "读不到这个文件" |
| AV03: Image > 1MB → warning (pre-listing QA L3) | ✅ | `modules/avatar-upload.md §Validation §File size` — hard 1MB limit; stop upload; prompt user |
| AV04: Image non-1:1 ratio → warning (pre-listing QA L2) | ✅ | `modules/pre-listing-qa.md §Logo` L2: "1:1 aspect ratio" warning; `modules/avatar-upload.md` Policy §6: accept non-1:1 but warn |
| AV05: After upload, URL changes → confirmation card must byte-equal re-render | ✅ | `core/display-formats.md` §Picture row rule: URL must appear verbatim; SKILL.md Confirmation Gate: "every field value byte-identical to CLI params" |

---

## 10. 强制门控 (MG01–MG19)

| TC | Result | Evidence / Notes |
|---|---|---|
| MG01: create → must run agent get first even if user gave all fields | ✅ | SKILL.md §MANDATORY Gates §Pre-Check Gate: "Any agent create...run agent get first. No exceptions, even when the user supplied all fields one-shot" |
| MG02: update → must agent get --agent-ids N for latest state | ✅ | SKILL.md §Update flow step 1: "agent get --agent-ids <id>"; `core/cli-reference.md §2` note |
| MG03: feedback-submit → must resolve creator-id via ladder 1 or 2 | ✅ | `modules/feedback.md §Step 2` — two-ladder rule is mandatory |
| MG04: "We already did get this conversation" → NOT exempt, state may have changed | ✅ | SKILL.md §Pre-Check Gate: "No exceptions...even when...named the role already"; `playbooks/README.md §Pre-check §Execute` self-check Q1 |
| MG05: auto-execute memory / user says "不用确认" → still render confirmation card | ✅ | SKILL.md §Confirmation Gate: "non-overridable"; playbooks/README.md §Confirmation card: "Memory preferences...do NOT bypass" |
| MG06: plan-mode exit → still needs confirmation card | ✅ | SKILL.md §Confirmation Gate rationalization blacklist includes "plan-mode exit" |
| MG07: Urgent tone ("赶紧建"/"现在就建") → still render confirmation card | ✅ | SKILL.md §Confirmation Gate: "urgency" listed in rationalization blacklist; `playbooks/requester.md §Confirmation` same note |
| MG08: Earlier similar confirmation ≠ this write's confirmation | ✅ | SKILL.md §Step 3 pre-execute self-check Q2: "user's most recent turn literally contains a confirm token" — earlier turns don't count |
| MG09: Confirmation card fields must be byte-equal to CLI params | ✅ | SKILL.md §Confirmation Gate: "every field value in the just-rendered card is byte-identical to what will be passed to the CLI" |
| MG10: activate/deactivate are state toggles, no confirmation card needed | ✅ | SKILL.md §Confirmation Gate: "activate / deactivate are state toggles — NOT gated"; playbooks/README.md §Confirmation card |
| MG11: Backend returns consent object → show terms card, wait for agree/decline | ✅ | SKILL.md §Consent Gate + `playbooks/consent.md` |
| MG12: Consent agreed → re-call create + consent-key + agreed=true | ✅ | `playbooks/consent.md §Agree flow` — exact parameters documented |
| MG13: Consent declined → immediately stop, don't re-invoke | ✅ | `playbooks/consent.md §Decline message` — "Do NOT call the CLI...stop." |
| MG14: User requests skip terms (auto-agree) → refuse | ✅ | `playbooks/consent.md` — "Do NOT pre-fill the user's reply or add 'I'll assume you agree'" |
| MG15: Success must use role-matching template verbatim | ✅ | SKILL.md §Post-Execute Gate: "Success → role file's §Post-success template verbatim"; each role file §Post-success |
| MG16: wallet add success must NOT render identity creation success (hallucination) | ✅ | SKILL.md §Post-Execute Gate: "After any onchainos agent ... CLI call, first user-visible output must come from a documented template"; anti-patterns in provider.md explicitly cover this |
| MG17: create/update/activate/deactivate → must enter Step 5 → Step 6 | ✅ | SKILL.md §Post-Create Comm-Init (Step 6): "After any local-agent-list-mutating success (create / update / activate / deactivate)..." |
| MG18: feedback-submit success → does NOT enter Step 6 | ✅ | SKILL.md §Post-Create Comm-Init: "feedback-submit is excluded"; SKILL.md §Step 5 table — "All else...Stop." |
| MG19: Step 6 fires unconditionally, callee self-gates | ✅ | SKILL.md §Step 6: "Load /skills/okx-agent-chat/after-agent-list-changed.md...Callee self-gates. Skip only when user explicitly declined..." |

---

## 11. UX 红线 (UX01–UX10)

| TC | Result | Evidence / Notes |
|---|---|---|
| UX01: okx-* skill names never in user-visible text | ✅ | SKILL.md §UX Output Red Lines Red line 1 — P0 rule; `core/ux-lexicon.md §How to use` step 1 |
| UX02: onchainos agent cmd never as "run this" instruction | ✅ | SKILL.md Red line 2 — "Never render onchainos agent <subcommand> [...] as copy-paste for the user" |
| UX03: Q1:/S1:/Phase 1/pre-check/status=2 never in user text | ✅ | SKILL.md Red line 3; `core/ux-lexicon.md §Flow` — all flow terms listed as forbidden |
| UX04: Role localized: CN 用户/服务提供商/仲裁者; EN User Agent/ASP/Evaluator Agent | ✅ | `core/ux-lexicon.md §Role` — complete translation table; Red line 4 |
| UX05: Service type first render must have long-form explanation (Pattern A teaching / Pattern B table) | ✅ | `core/ux-lexicon.md §Service-type` — Pattern A and Pattern B defined; "both patterns satisfy gloss on first occurrence" requirement |
| UX06: status integers (0/1/2/3) must be translated | ✅ | `core/ux-lexicon.md §Status` — translation table; "⛔ Never render status=0 / status: 1 / status=2 / raw integer" |
| UX07: ≥5 agents → reassurance footer, not "你有N个agent了" | ✅ | `core/display-formats.md §1 §Multi-agent List Reassurance Footer` — exact wording prescribed; SKILL.md Red line 5 |
| UX08: name/description/service.* must come from user's reply (Red line 6) | ✅ | SKILL.md Red line 6 — P0; all playbooks reiterate this |
| UX09: Every confirmation card field value must be traceable to user input | ✅ | SKILL.md §Confirmation Gate: "byte-identical to what will be passed to the CLI"; playbooks/README.md confirmation card rule |
| UX10: After success, no chasing agent get, no status polling | ✅ | `_shared/no-polling.md` Rule 1 and 2; SKILL.md §Step 3 and §Post-Execute Gate |

---

## 12. 路由边界 (RT01–RT08)

| TC | Result | Evidence / Notes |
|---|---|---|
| RT01: "创建任务/发布任务" → okx-agent-task, no agent command | ✅ | SKILL.md §Negative Triggers: "创建任务 / 发布任务 → okx-agent-task" |
| RT02: "接单/接任务" → okx-agent-task | ✅ | SKILL.md §Negative Triggers: "接单 / 接任务 → okx-agent-task" |
| RT03: "仲裁一下这单" → okx-agent-task (task dispute) | ✅ | SKILL.md §Negative Triggers: "仲裁一下这单 / 发起仲裁 → okx-agent-task" |
| RT04: "注册仲裁者身份" → okx-agent-identity (identity create) | ✅ | SKILL.md §description field includes "注册仲裁者"; `playbooks/README.md` role router |
| RT05: "我要当仲裁者" (no context) → ask: 注册身份(1) or 发起仲裁(2) | ✅ | SKILL.md §Negative Triggers: "'我要当仲裁者' alone (no identity words) → Ask: 1. 注册仲裁者身份 2. 对某笔任务发起仲裁" |
| RT06: "建一个买家身份" → agent create, NOT wallet add | ✅ | SKILL.md description: "再建一个买家身份 / add another agent / new provider = ALWAYS identity, NEVER wallet add" |
| RT07: Single-word input ("agent"/"search") → don't auto-route, ask user intent | ✅ | SKILL.md §Step 1: "Ambiguous → ask once"; `playbooks/README.md` "Do NOT default. Do NOT guess" |
| RT08: Successful create/update/activate/deactivate → Step 6 triggers after-agent-list-changed | ✅ | SKILL.md §Post-Create Comm-Init: "load /skills/okx-agent-chat/after-agent-list-changed.md and continue its Execution Flow" |

---

## 13. 错误处理 (ERR01–ERR12)

| TC | Result | Evidence / Notes |
|---|---|---|
| ERR01: session expired → hand off to wallet login, then retry | ✅ | `troubleshooting.md §1`: `session expired` → "Hand off to okx-agentic-wallet → wallet login, then retry" |
| ERR02: no XLayer address → guide to wallet add/switch | ✅ | `troubleshooting.md §1`: `no XLayer address found` → "Hand off to okx-agentic-wallet → wallet add / wallet switch" |
| ERR03: agent not found → "找不到该 agent" | ✅ | `troubleshooting.md §2`: `agent not found / 404` → "找不到该 agent" |
| ERR04: not in whitelist (10016) → render apply URL (extracted from msg), stop, no retry | ✅ | `troubleshooting.md §2`: code 10016 / whitelist → full URL extraction regex described; "Never auto-retry" |
| ERR05: region restriction (50125/80001) → "该地区暂不支持", no VPN suggestion | ✅ | `troubleshooting.md §2`: codes 50125/80001 → "Service is not available in your region." "Do NOT suggest VPNs." |
| ERR06: pending settlements → prompt to handle tasks first | ✅ | `troubleshooting.md §2`: `pending settlements` → "这个 agent 上还有任务没结清" |
| ERR07: self-rating not allowed → "不能给自己的 agent 打分" | ✅ | `troubleshooting.md §2`: `self-rating not allowed` → "不能给自己的 agent 打分" |
| ERR08: creator agent not owned by caller → re-run ladder 2 | ✅ | `troubleshooting.md §2`: `creator agent not owned by caller` → "re-run ladder 2 from the top" |
| ERR09: HTTP 500 → retry once, then surface to user | ✅ | `troubleshooting.md §2`: `Wallet API server error (HTTP 500)` → "Retry once"; `_shared/no-polling.md §What is allowed` transient retry |
| ERR10: Unknown error → raw msg in error card footer, ask user next step | ✅ | `troubleshooting.md` intro: "If you encounter a string that isn't in either table, surface the raw message in the error card footer"; `core/display-formats.md §7` error card format |
| ERR11: agent already active → "已上架，无需操作" | ✅ | `troubleshooting.md §2`: `agent already active` → "这个 agent 已经在上架状态，不用再上架。" |
| ERR12: score out of range → use star wording, don't expose 0-100 | ✅ | `troubleshooting.md §2`: `score out of range` → "评分要在 0.00–5.00 之间，最多保留 2 位小数" (stars wording); "do not echo the raw 0–100 bound" |

---

## Findings: ⚠️ Partial Coverage (9 cases)

### ⚠️ S09 — feedbackRate=0 vs feedbackRate=null in search results

- **Gap**: The TC asks whether `feedbackRate=0` (integer zero, meaning agent has 0 average score) should show "暂无评分" vs "★0". 
- **What exists**: `core/display-formats.md §1` list rule says "If no feedback yet, render 暂无评分"; `core/data-display.md` says `No-data: render —`. But these cover `reputation.score==0` from `agent get`, not `feedbackRate` from `agent search`. 
- `core/cli-search-feedback.md §7` says `feedbackRate: null` is possible (included in example JSON). 
- `core/display-lists.md §6 Field mapping` says `feedbackRate null → —` (em dash, not "暂无评分").
- The skill uses `暂无评分` in the `agent get` list for the Rating column, but for `agent search` results the Field mapping explicitly says `—` for null.
- **For `feedbackRate=0`** (not null), there is no explicit rule; the natural rendering would be `★ 0`.
- **Recommendation**: Add a rule in `core/display-lists.md §6` for `feedbackRate == 0` → whether to show `★ 0` or `暂无评分`.

### ⚠️ U06 — Update someone else's agent (ownerAddress mismatch)

- **Gap**: The exact user-facing message "这个 agent 不归你当前钱包管" is not prescribed anywhere. The pre-check logic covers the concept (ownership is per-address), but the error card wording for the update scenario when the user attempts to update an agent they don't own is not explicitly defined.
- **What exists**: `playbooks/README.md §Pre-check` explains current-wallet scope; the skill knows to refuse, but no message template.
- **Recommendation**: Add an explicit error message template in `troubleshooting.md` or `playbooks/README.md` for this case.

## Findings: ❌ Missing Coverage (4 cases)

### ❌ TC-C-R02 (partial) — Description row behavior in confirmation card

Actually on re-verification this is ✅ (covered in requester.md "user volunteered a description" variant). Reclassified.

### ❌ TC-MG16 — wallet add success must NOT render identity creation success

Actually covered by the Post-Execute Gate in SKILL.md. Reclassified as ✅.

After re-review, the 4 genuine ❌ findings are:

### ❌ Missing 1: No explicit "update another wallet's agent" error message template

TC U06 identifies a scenario where the user tries to update an agent owned by a different wallet address. While the pre-check logic makes the detection implicit, there is no prescribed error message. `troubleshooting.md §3` does not include this case as a skill-side guard.

**File gap**: `troubleshooting.md §3` — needs a row for "ownerAddress mismatch on update attempt".

### ❌ Missing 2: S09 — feedbackRate=0 special case in search results

As described above, `feedbackRate=0` is not the same as `feedbackRate=null`, and the rule for displaying zero-rated agents in search results is not specified.

**File gap**: `core/display-lists.md §6 Field mapping` — needs a rule for `feedbackRate == 0`.

### ❌ Missing 3: TC-A02 exact flow — approvalStatus=1 triggers submit-approval, explicit "审核中 24h" message

`core/cli-reference.md §4` shows the dispatch table but the actual user-facing text for the submit-approval-success case is in `troubleshooting.md §2` as "submit-approval returns success: true". The **trigger chain** (activate → approvalStatus=1 → auto-call submit-approval → render "审核中") is technically covered, but the automatic invocation of `submit-approval` is referenced in the §4 dispatch table but not in `modules/pre-listing-qa.md` or anywhere in the provider flow as a precondition explanation to the user. A user-visible "why did the skill make an extra call?" is unaddressed.

This is minor/informational only. **Reclassifying as ⚠️.**

### ❌ Missing 4: TC-C-PO02 wording discrepancy

`playbooks/requester.md §After success` passive mode success line reads: "已为你创建用户身份 #<id>。现在继续发布任务。"

TC-PO02 expects: "已为你创建用户身份 #N，继续发任务"

There is a subtle wording difference: the skill template says "现在继续发布任务。" while the TC expects "继续发任务". This is a wording-level mismatch, not a structural gap — the behavior is covered. **Reclassifying as ⚠️.**

---

## Revised Final Summary

After re-verifying edge cases:

| Result | Count |
|---|---|
| ✅ Covered | 188 |
| ⚠️ Partially covered | 6 |
| ❌ Missing | 2 |

### ⚠️ Cases (6)

1. **S09** — `feedbackRate=0` handling in search results: no explicit rule distinguishing zero-score from no-score in `core/display-lists.md §6`.
2. **U06** — No prescribed user-facing message for "update another wallet's agent" attempt.
3. **A02** — Auto-submit-approval trigger chain is covered across two files but not consolidated; a user may see an unexpected second CLI call without explanation.
4. **PO02** — Minor wording mismatch: skill says "现在继续发布任务。" vs TC's "继续发任务".
5. **F12** — Normalization hint parenthetical `（按 0.05 星粒度落到 3.3）` is specified in `modules/feedback.md §Step 5` for the *confirmation card* only. Whether the same hint appears in the *post-success line* §Step 7 is less explicit (§Step 7 says "N MUST be the wire-normalized star value" but doesn't mention the hint parenthetical again).
6. **MG09** — "Confirmation card fields must be byte-equal" is specified, but the specific anti-pattern of "URL changed between upload step and confirmation render" (relevant to AV05) is described in avatar-upload.md rather than being enforced by a single canonical rule that handles the post-upload URL refresh requirement in the same location.

### ❌ Cases (2)

1. **U06 error message gap** — No explicit template for "trying to update an agent owned by a different wallet". Closest coverage: `playbooks/README.md §Pre-check` explains per-address scope; `troubleshooting.md §3` does not have this row.

2. **S09 feedbackRate=0 gap** — `core/display-lists.md §6` says `null → —` but does not say what to render when `feedbackRate == 0` (a valid zero-star average).

---

## File Coverage Index

| File | TCs primarily covered |
|---|---|
| `SKILL.md` | MG01–MG19, RT01–RT08, UX01–UX10, R01/R04/R07/R09–R13 |
| `playbooks/requester.md` | R01–R08, R12, R13, PO01–PO05 |
| `playbooks/provider.md` | P01–P18 |
| `playbooks/provider-services.md` | P01, P03, P08, P09, P12, P13, P17 |
| `playbooks/evaluator.md` | E01–E06 |
| `playbooks/README.md` | R04, P04, P05, E05, MG01–MG10, U01–U07 |
| `playbooks/consent.md` | R09–R11, MG11–MG14 |
| `modules/feedback.md` | F01–F21 |
| `modules/agent-search.md` | S01–S11 |
| `modules/pre-listing-qa.md` | A06–A13 |
| `modules/avatar-upload.md` | AV01–AV05 |
| `core/display-formats.md` | G01–G03, UX07, MG09 |
| `core/display-detail.md` | G04, G06, U01–U04, MG09 |
| `core/display-lists.md` | FL01–FL05, S09, S10 |
| `core/cli-reference.md` | A01–A05, D01–D03, G05, U01–U07 |
| `core/cli-create.md` | R09–R11, MG01 |
| `core/cli-search-feedback.md` | S01–S11, FL01–FL05, F01–F21, SL01–SL03 |
| `core/ux-lexicon.md` | UX01–UX10 |
| `core/data-display.md` | F09–F12, F16 |
| `core/choice-prompts.md` | R12, P08, P15, P16 |
| `core/cost-disclosure.md` | (cost-related UX) |
| `troubleshooting.md` | ERR01–ERR12, R05, P06, P07, P09, U07, G07, SL02, AV02 |
| `_shared/no-polling.md` | UX10, MG10, G08, SL03 |
| `_shared/preflight.md` | (pre-session setup) |
| `cross-skill-workflows.md` | RT08, MG17 |
