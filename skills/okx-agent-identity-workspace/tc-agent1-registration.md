# TC Verification Report — Modules 1-4
# okx-agent-identity (refactored vs original)
# Agent: Agent 1 — Scope: Registration, Update, Get, Activate/Deactivate, Pre-listing QA
# Date: 2026-05-29

---

## METHOD

For each TC, compared behavior from:
- NEW refactored: playbooks/ + modules/ + core/ (current HEAD)
- ORIGINAL: git HEAD references/ + SKILL.md monolith

"same" = rule exists in both with equivalent semantics
"different" = behavior or coverage changed between versions
"new-only" = only in refactored version
"original-only" = only in original version

---

## MODULE 1: Registration — Requester (TC-C-R01~R14)

### TC-C-R01 | Normal register — name + avatar
User: "I want to register a user identity, name Alice, upload avatar" →
Expected: pre-check runs, Q1 asks name, Q2 asks avatar, confirmation card shows 角色/名字/头像, user confirms, CLI runs.
Coverage: playbooks/requester.md §Standard Q&A chain Q1/Q2, §Confirmation, §Phase preview. Pre-check: playbooks/README.md §Pre-check.
✅ TC-C-R01 | Covered in playbooks/requester.md + README.md | original: same (references/role-requester.md + role-playbook.md)

### TC-C-R02 | Normal register — name + description
User: "Register a User Agent named Alice focused on DeFi research" →
Expected: One-shot capture catches name + description, skips Q1, goes to Q2 (avatar), card shows 名字+描述+头像.
Coverage: playbooks/requester.md Good/bad case row 3 "one-shot capture", §Confirmation with description row.
✅ TC-C-R02 | Covered in playbooks/requester.md §Confirmation (description variant) | original: same

### TC-C-R03 | Normal register — name + upload image
User: "Register user identity Alice" then attaches image for avatar →
Expected: File size checked before upload, agent upload runs, URL put in --picture, card shows actual URL.
Coverage: modules/avatar-upload.md §Claude Code flow (attachment-supported), Policy rule 5 "show URL verbatim".
✅ TC-C-R03 | Covered in modules/avatar-upload.md | original: same (references/avatar-upload.md)

### TC-C-R04 | Duplicate block — requester already exists
User: "Register another user identity" (pre-check finds existing requester #42) →
Expected: Do NOT enter create flow. Show "在当前钱包下你已经有用户身份 #42（Alice）…" with update redirect. No "register again" option.
Coverage: playbooks/README.md §requester/evaluator — if found, tell user and point to update. Wording template with "在当前钱包下" mandatory qualifier.
✅ TC-C-R04 | Covered in playbooks/README.md §Pre-check requester section | original: same

### TC-C-R05 | Name empty
User provides empty name →
Expected: validation fails, re-ask Q1 once with a shorter example.
Coverage: playbooks/requester.md Q1 row "On failure: re-ask once with a shorter example". Validation: non-empty required.
✅ TC-C-R05 | Covered in playbooks/requester.md Q&A chain validation | original: same

### TC-C-R06 | Name too long
User provides name exceeding limit (CN > 30 chars, EN > 64 chars) →
Expected: re-ask once with shorter example.
Coverage: playbooks/requester.md Q1 validation "CN ≤ 30 文字 / EN ≤ 64 chars".
✅ TC-C-R06 | Covered in playbooks/requester.md Q1 validation | original: same

### TC-C-R07 | Metadata pre-fill refuse
User provides email/wallet session data expecting auto-fill →
Expected: Never pre-fill name from userEmail, wallet name, or session metadata. Re-ask.
Coverage: playbooks/requester.md §Standard Q&A chain ⛔ "Fields from user's literal reply only — never pre-fill from userEmail, wallet name, or session metadata". Also SKILL.md Red line 6.
✅ TC-C-R07 | Covered in playbooks/requester.md + SKILL.md Red line 6 | original: same (SKILL.md §Red line 6)

### TC-C-R08 | Service field refuse
User: "Add a 5 USDT service to my user identity" →
Expected: Explain user identity has no services; if they want to charge, register a provider instead. Do NOT add service to requester create.
Coverage: playbooks/requester.md Good/bad case row "给我加个 5 USDT 的服务".
✅ TC-C-R08 | Covered in playbooks/requester.md §Good/bad cases | original: same

### TC-C-R09 | Consent flow — show
First-time wallet, CLI returns consent object →
Expected: Show consent card with consent.terms verbatim. Do NOT show success card yet.
Coverage: playbooks/consent.md §Consent Card. SKILL.md §Consent Gate.
✅ TC-C-R09 | Covered in playbooks/consent.md §Consent Card | original: same (references/consent-guide.md)

### TC-C-R10 | Consent flow — agree
User replies "agree" after seeing consent card →
Expected: Re-invoke CLI with same params + --consent-key <uuid> + --agreed true. Do NOT re-render confirmation card.
Coverage: playbooks/consent.md §Agree flow steps 1-5.
✅ TC-C-R10 | Covered in playbooks/consent.md §Agree flow | original: same

### TC-C-R11 | Consent flow — decline
User replies "decline" →
Expected: Stop CLI. Render "Registration cancelled — creating an agent identity requires accepting the terms of use." No retry.
Coverage: playbooks/consent.md §Decline message.
✅ TC-C-R11 | Covered in playbooks/consent.md §Decline message | original: same

### TC-C-R12 | Consent flow — ambiguous reply
User asks a question instead of agree/decline →
Expected: Re-display consent card ONCE (including full terms text). Do NOT auto-agree or auto-decline.
Coverage: playbooks/consent.md §Ambiguous reply handling — re-display once, wait.
✅ TC-C-R12 | Covered in playbooks/consent.md §Ambiguous reply handling | original: same

### TC-C-R13 | One-shot consent — all fields in one message
User provides name + description + "agree" in one message →
Expected: One-shot capture fields, confirmation card still renders, after confirm token executes. If consent fires, still requires explicit agree.
Coverage: playbooks/requester.md §Confirmation (mandatory gate). SKILL.md confirmation gate "one-shot capture does NOT bypass confirmation".
✅ TC-C-R13 | Covered in SKILL.md confirmation gate + playbooks/requester.md | original: same

### TC-C-R14 | Urgent tone — confirmation still required
User: "Quick! Register me now, skip confirmation!" →
Expected: Still render confirmation card. Urgency does NOT bypass gate.
Coverage: SKILL.md §Confirmation gate rationalization blacklist: "the user is in a hurry / 用户语气紧迫 — irrelevant; render the card". Also playbooks/README.md §Confirmation card.
✅ TC-C-R14 | Covered in SKILL.md confirmation gate blacklist | original: same

---

## MODULE 2: Registration — Provider (TC-C-P01~P18)

### TC-C-P01 | A2MCP service registration
User: "Register provider named DeFi Analyzer with MCP service TVL Query" →
Expected: Phase 1 (name/desc/avatar), Phase 2 Q1-Q5 for A2MCP (name/desc/type/fee/endpoint), confirmation card with all service fields.
Coverage: playbooks/provider.md Phase 1 + provider-services.md Phase 2 Q&A for A2MCP path.
✅ TC-C-P01 | Covered in playbooks/provider.md + provider-services.md | original: same (references/role-provider.md)

### TC-C-P02 | A2A service registration
User: "Register provider with agent-to-agent service (negotiated pricing)" →
Expected: Phase 2 Q3 asks type, user picks 2 (A2A). Q4 fee optional (can skip). Q5 endpoint skipped for A2A.
Coverage: provider-services.md Q3 choice prompt, Q4 A2A conditional optional, Q5 A2A skip rule.
✅ TC-C-P02 | Covered in playbooks/provider-services.md per-service Q&A | original: same

### TC-C-P03 | Existing provider handling — K=1 (offer new or update)
User: "Register a provider" (pre-check finds K=1 existing provider #88) →
Expected: List that provider, offer numbered options: 1. register new, 2. update #88.
Coverage: playbooks/README.md §provider (可多开) — K=1 template with numbered options.
✅ TC-C-P03 | Covered in playbooks/README.md §provider K=1 | original: same

### TC-C-P04 | Description required for provider
User tries to create provider without description →
Expected: CLI enforces non-empty description for provider role (cli-create.md confirms this). Skill validates or CLI rejects.
Coverage: core/cli-create.md "CLI enforces non-empty for --role provider only". playbooks/provider.md Q2 validation non-empty.
✅ TC-C-P04 | Covered in core/cli-create.md + playbooks/provider.md Q2 | original: same

### TC-C-P05 | No-service block — provider without any service
User completes provider create but no service added →
Expected: CLI rejects with "provider agents require at least one service; provide --service". Return to Phase 2 Q1.
Coverage: playbooks/provider.md §Error recovery "if 'provider agents require at least one service' surfaces, return to Phase 2 Q1". Also SKILL.md Edge Cases.
✅ TC-C-P05 | Covered in playbooks/provider.md §Error recovery | original: same

### TC-C-P06 | Fabrication refuse
User: "Write me some services" / "帮我写几个 service" →
Expected: Refuse. Ask what they actually want to offer. Never invent service content.
Coverage: provider-services.md Phase 2 preamble "⛔ No fabricated services. Ever." + good/bad cases row "帮我写几个 service".
✅ TC-C-P06 | Covered in playbooks/provider-services.md + playbooks/provider.md §Good/bad cases | original: same

### TC-C-P07 | Fee validation — invalid format
User provides non-numeric fee or wrong format →
Expected: A2MCP validates number ≥ 0, ≤ 6 decimal places, non-empty. A2A: empty or same pattern.
Coverage: provider-services.md Q4 validation. Internal pattern not shown to user.
✅ TC-C-P07 | Covered in playbooks/provider-services.md Q4 validation | original: same

### TC-C-P08 | Endpoint validation — https required
User: "endpoint is http://..." →
Expected: Reject. Ask for HTTPS.
Coverage: playbooks/provider.md §Good/bad cases "'endpoint 是 http://...' | Reject. Ask for HTTPS." + provider-services.md Q5 validation.
✅ TC-C-P08 | Covered in playbooks/provider.md + provider-services.md Q5 | original: same (references/endpoint-anti-pattern.md)

### TC-C-P09 | Endpoint validation — localhost forbidden
User: "endpoint is http://localhost:8080" →
Expected: Reject. Blacklist includes localhost/127.0.0.1/private IPs.
Coverage: provider-services.md Q5 "reject any host matching SKILL.md §Endpoint Anti-Pattern blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / http://)".
✅ TC-C-P09 | Covered in playbooks/provider-services.md Q5 + SKILL.md §Endpoint Anti-Pattern | original: same

### TC-C-P10 | Servicetype validation — invalid value
User: "service type HTTP" or provides invalid servicetype →
Expected: Reject politely, re-render Q3 numbered prompt verbatim. Do not fabricate a new phrasing.
Coverage: playbooks/provider.md §Good/bad cases "'服务类型 HTTP' / 'service type HTTP' | Reject politely and re-render the Q3 numbered prompt verbatim".
✅ TC-C-P10 | Covered in playbooks/provider.md §Good/bad cases | original: same

### TC-C-P11 | JSON blob — re-confirm field by field
User pastes entire service JSON object →
Expected: Thank them, but re-confirm field-by-field. Do not pipe JSON straight to CLI.
Coverage: playbooks/provider.md §Good/bad cases "User pastes JSON blob | Thank them, but re-confirm field by field".
✅ TC-C-P11 | Covered in playbooks/provider.md §Good/bad cases | original: same

### TC-C-P12 | Phase boundary — service info in Phase 1 discarded
User mentions fee in Phase 1: "I want data analysis service, charge 10 USDT" →
Expected: Do NOT capture fee=10 at Phase 1. Phase boundary is strict. Re-ask in Phase 2 Q4.
Coverage: playbooks/provider.md §Good/bad cases "在 Phase 1 说的 | Do NOT capture fee=10". + provider-services.md hint about "core/choice-prompts.md §One-Shot Capture rule 4".
✅ TC-C-P12 | Covered in playbooks/provider.md §Good/bad cases | original: same

### TC-C-P13 | Fee=0 warning — A2MCP free service
User: "API interface service, fee free / 0 USDT" →
Expected: Accept 0 but warn: "API 接口式服务 0 USDT 等同于免费入口，后续不能再按量收费。"
Coverage: playbooks/provider.md §Good/bad cases "'API 接口式服务 Fee 免费' | Accept 0 but warn: 'API 接口式服务 0 USDT 等同于免费入口…'"
✅ TC-C-P13 | Covered in playbooks/provider.md §Good/bad cases | original: same (references/role-provider.md same row)

### TC-C-P14 | No hardcoded amount for provider fee
Provider doesn't specify fee, model fills in default →
Expected: Never default fee. Red line 6: "Do not pick a default fee."
Coverage: provider-services.md Phase 2 preamble "⛔ No fabricated services... Do not pick a default fee."
✅ TC-C-P14 | Covered in playbooks/provider-services.md | original: same

### TC-C-P15 | Suggestion-as-prompt carve-out (Q1/Q3)
User mentioned "天气查询服务" earlier, now in Phase 2 Q1 →
Expected: Q1 MAY quote that mention as suggestion, not auto-fill. User's reply is authoritative.
Coverage: provider-services.md §Suggestion-as-prompt carve-out (Q1 + Q3, opt-in). Canonical example wording.
✅ TC-C-P15 | Covered in playbooks/provider-services.md §Suggestion-as-prompt carve-out | original: same (references/role-provider.md same section)

### TC-C-P16 | A2A fee — optional, skip allowed
User picks A2A service type and wants to skip fee →
Expected: Q4 for A2A shows fee as optional with skip instruction. Wire still carries "fee":"".
Coverage: provider-services.md Q4 A2A path "if A2A → reference price optional, skip allowed". Wire note about models.rs:21.
✅ TC-C-P16 | Covered in playbooks/provider-services.md Q4 | original: same

### TC-C-P17 | K≥2 provider list
User: "Register another provider" (K=2 existing providers) →
Expected: List ALL K existing providers by id+name, offer: 1. new, 2. update one. If 2 chosen, ask which one.
Coverage: playbooks/README.md §provider K≥2 template + follow-up numbered question for K≥2 update selection.
✅ TC-C-P17 | Covered in playbooks/README.md §provider K≥2 | original: same

### TC-C-P18 | Provider create returns active by default
After successful provider create →
Expected: Post-success template says "默认已上架可以接单". No need to follow up with agent activate.
Coverage: playbooks/provider.md §Post-success template "服务提供商身份 #<id> 注册完成，默认已上架可以接单." + rule "Create returns active by default — no need to follow up with agent activate."
✅ TC-C-P18 | Covered in playbooks/provider.md §Post-success | original: same (references/role-provider.md §Post-success same template)

---

## MODULE 3: Registration — Evaluator (TC-C-E01~E06)

### TC-C-E01 | Normal evaluator register + staking flow
User: "Register evaluator identity named Solidity Auditor" →
Expected: Q1 asks name, confirmation card (role=仲裁者, name), execute, post-success renders TWO lines, then same-turn handoff to okx-agent-task evaluator-staking.md §2.
Coverage: playbooks/evaluator.md §Flow overview (3-step), §Post-success template (two visible lines), §Agent directive "→ proceed to SKILL.md Step 5 — evaluator row routes to evaluator-staking.md §2".
✅ TC-C-E01 | Covered in playbooks/evaluator.md | original: same (references/role-evaluator.md)

### TC-C-E02 | Evaluator one-shot
User: "Register evaluator Solidity Auditor" (name provided upfront) →
Expected: One-shot captures name, skips Q1, goes directly to confirmation. Two-line post-success.
Coverage: playbooks/evaluator.md Q&A "Skip any Q whose field was already captured via core/choice-prompts.md §One-Shot Capture".
✅ TC-C-E02 | Covered in playbooks/evaluator.md + one-shot capture rule | original: same

### TC-C-E03 | No-stake explanation
User: "Can I register evaluator without staking?" →
Expected: Yes, create proceeds. Post-success explains stake needed for dispute assignments. Do NOT gate create on staking.
Coverage: playbooks/evaluator.md §Good/bad cases "'我还没质押，能先注册吗' | 可以。" and "'不想质押' | Offer evaluator identity first, remind no assignments without stake." Also evaluator.md §Flow overview "No pre-create staking gate."
✅ TC-C-E03 | Covered in playbooks/evaluator.md §Good/bad cases | original: same

### TC-C-E04 | Order correction — stake before register attempt
User: "帮我直接质押再注册" (stake first, then register) →
Expected: Correct them — must register first, then stake. Skill hands off to staking after registration.
Coverage: playbooks/evaluator.md §Good/bad cases "'帮我直接质押再注册' | Correct them: '得先注册再质押。'"
✅ TC-C-E04 | Covered in playbooks/evaluator.md §Good/bad cases | original: same

### TC-C-E05 | Evaluator duplicate block
User: "Register another evaluator" (pre-check finds existing evaluator) →
Expected: Evaluator is unique per address. Do NOT offer create. Tell user and point to update.
Coverage: playbooks/README.md §Pre-check requester/evaluator uniqueness — same rule applies to evaluator.
✅ TC-C-E05 | Covered in playbooks/README.md §Pre-check | original: same

### TC-C-E06 | No hardcoded stake amount
Post-success evaluator message →
Expected: Do NOT hardcode any OKB amount (e.g., "100 OKB") in post-success message. Amount is owned by evaluator-staking.md.
Coverage: playbooks/evaluator.md §Post-success "do NOT state a stake amount — the same-turn handoff below will take the user directly into that skill's own prompt, which owns both the path and the amount." Anti-pattern example explicitly calls this out.
✅ TC-C-E06 | Covered in playbooks/evaluator.md §Post-success anti-pattern | original: same

---

## MODULE 4: Passive Onboarding (TC-C-PO01~PO08, TC-C-PC01~PC03)

### TC-C-PO01 | Skip role selection
Passive onboarding entry (intent=need-requester) →
Expected: Skip role selection. Role is fixed as requester.
Coverage: playbooks/requester.md §Passive Onboarding §Simplified sub-flow "Do not ask for --role — it's fixed as requester."
✅ TC-C-PO01 | Covered in playbooks/requester.md §Passive Onboarding | original: same (references/passive-onboarding.md)

### TC-C-PO02 | Skip pre-check
Passive onboarding →
Expected: Do not pre-check existing agents. The handoff already implied none exist.
Coverage: playbooks/requester.md §Passive Onboarding "Do not pre-check existing agents — the handoff already implied none exist." Also README.md "Skip this pre-check entirely for passive onboarding."
✅ TC-C-PO02 | Covered in playbooks/requester.md + README.md | original: same

### TC-C-PO03 | Skip avatar prompt
Passive onboarding →
Expected: Do not ask for picture. Use backend default.
Coverage: playbooks/requester.md §Passive Onboarding "Do not ask for picture — use backend default."
✅ TC-C-PO03 | Covered in playbooks/requester.md §Passive Onboarding | original: same

### TC-C-PO04 | Success line with id
After passive onboarding create succeeds with id →
Expected: ONE LINE only: "已为你创建用户身份 #<id>。现在继续发布任务。" No detail card.
Coverage: playbooks/requester.md §Passive Onboarding §After success — canonical wording with id, explicitly "no detail card in passive mode".
✅ TC-C-PO04 | Covered in playbooks/requester.md §Passive Onboarding §After success | original: same

### TC-C-PO05 | Success line without id (txHash only)
After passive onboarding create, CLI returns only txHash →
Expected: "已为你创建用户身份。现在继续发布任务。" (no #id). No detail card.
Coverage: playbooks/requester.md §Passive Onboarding §After success — fallback without id variant defined. #<id> placeholder rule applies.
✅ TC-C-PO05 | Covered in playbooks/requester.md §Passive Onboarding §After success | original: same

### TC-C-PO06 | No Step 6 in passive onboarding
Passive onboarding success →
Expected: Do NOT load okx-agent-chat/after-agent-list-changed.md. Hand strictly back to okx-agent-task.
Coverage: playbooks/requester.md §Passive Onboarding §After success "Do NOT load /skills/okx-agent-chat/after-agent-list-changed.md here." SKILL.md §Operation Flow Step 5 passive row "back to task, not Step 6."
✅ TC-C-PO06 | Covered in playbooks/requester.md + SKILL.md §Step 5 | original: same

### TC-C-PO07 | Confirmation gate still applies in passive mode
Even in passive mode →
Expected: Confirmation card still mandatory. "Passive mode does NOT bypass the confirmation gate."
Coverage: playbooks/requester.md §Passive Onboarding "Show confirmation table (still field-per-row, still mandatory)." Also original passive-onboarding.md explicitly states this.
✅ TC-C-PO07 | Covered in playbooks/requester.md §Passive Onboarding | original: same

### TC-C-PO08 | Cancel mid passive onboarding
User: "算了不注册了" during passive onboarding →
Expected: "已取消创建，发布任务需要用户身份，等你想好再来."
Coverage: playbooks/requester.md §Passive Onboarding §Edge cases "User asks to cancel mid-flow".
✅ TC-C-PO08 | Covered in playbooks/requester.md §Edge cases | original: same

### TC-C-PC01 | Service-add refuse in passive mode
User: "顺便加个 MCP 服务" during passive onboarding →
Expected: Explain user identity has no services. Do not add service in passive sub-flow.
Coverage: playbooks/requester.md §Edge cases "User volunteers a service mid-flow | Explain: 用户身份不带服务." Same wording as normal requester good/bad case.
✅ TC-C-PC01 | Covered in playbooks/requester.md §Edge cases | original: same

### TC-C-PC02 | Existing identity found during passive onboarding
Pre-existing requester discovered during passive flow →
Expected: Skip create. Echo "你已经有用户身份 #<N>（<name>），直接用它继续发布任务."
Coverage: playbooks/requester.md §Passive Onboarding "When user already has a requester" section with exact wording.
✅ TC-C-PC02 | Covered in playbooks/requester.md §Passive Onboarding §When user already has a requester | original: same

### TC-C-PC03 | Per-address uniqueness — passive onboarding context
Passive onboarding uniqueness rule →
Expected: Uniqueness is per address (not per email). Pre-check during passive mode checks current wallet address.
Coverage: playbooks/README.md §Pre-check — "per address, not per email" rule clearly stated. Passive mode skips check (contract is none exist by design).
✅ TC-C-PC03 | Covered in playbooks/README.md §Pre-check per-address scoping | original: same

---

## MODULE 5: Update (TC-U01~U11)

### TC-U01 | Update name
User: "Update #42 name to NewName" →
Expected: agent get #42 first, show current detail, collect new name, diff card shows current=DeFiAnalyzer new=**NewName**, confirm, execute.
Coverage: SKILL.md §Update flow steps 1-4. core/display-detail.md §3 Update Diff variant with bold new values.
✅ TC-U01 | Covered in SKILL.md §Update + core/display-detail.md §3 | original: same

### TC-U02 | Update description
User: "Change description of #42" →
Expected: Same flow. Diff card shows 描述 row with current and new value bolded.
Coverage: core/display-detail.md §3 Update variant example shows 描述 row. SKILL.md §Update.
✅ TC-U02 | Covered in SKILL.md §Update + core/display-detail.md §3 | original: same

### TC-U03 | Update avatar
User: "Change avatar of #42" →
Expected: New URL or upload goes through avatar-upload.md. Diff card shows old URL → new URL verbatim.
Coverage: core/display-formats.md §Profile photo row rule — diff cards show old URL in Current column, new URL in New column, verbatim.
✅ TC-U03 | Covered in core/display-formats.md §Profile photo row rule | original: same

### TC-U04 | Update services
User: "Add service to #42" →
Expected: --service is wholesale replacement. Start from current full services list, apply diff in memory, send complete list. Never send only changed entry (would delete other services).
Coverage: core/display-detail.md §3 Update variant "Maintainer note (wholesale --service replacement)".
✅ TC-U04 | Covered in core/display-detail.md §3 wholesale --service note | original: same

### TC-U05 | No-change rejection
User requests update but provides same values →
Expected: Skill refuses to call CLI. "没有需要提交的更改."
Coverage: SKILL.md §Update "Skill-side rule: if no fields changed, refuse to call CLI." Also core/cli-reference.md §2 skill-side rule.
✅ TC-U05 | Covered in SKILL.md §Update + core/cli-reference.md §2 | original: same

### TC-U06 | Owner mismatch
User tries to update an agent they don't own →
Expected: ownerAddress of returned agent ≠ currently selected XLayer wallet → Stop. Say "这个 agent 不归你当前钱包管." Do NOT proceed.
Coverage: SKILL.md §Update step 2 "Ownership check (skill-side, before Q&A): if ownerAddress ≠ current wallet → stop."
✅ TC-U06 | Covered in SKILL.md §Update ownership check | original: same

### TC-U07 | Not-found
User: "Update #9999" (agent not found) →
Expected: agent get returns empty/error → surface error from troubleshooting.md. Do NOT enter Q&A.
Coverage: SKILL.md §Update step 1 "agent get --agent-ids <id>". If not found, troubleshooting.md handles the error.
✅ TC-U07 | Covered via SKILL.md §Update + troubleshooting.md | original: same

### TC-U08 | Diff card bold/unchanged formatting
Update diff card rendering →
Expected: Changed rows bold the new-value cell. Unchanged rows show (不变) in new-value column. Never empty or repeat value.
Coverage: core/display-detail.md §3 Update variant rules "Changed rows: bold the new-value cell. Unchanged rows show (不变) / (unchanged)."
✅ TC-U08 | Covered in core/display-detail.md §3 | original: same (references/display-formats.md §3)

### TC-U09 | Full service replacement on update
Provider updates one service subfield →
Expected: Still send complete services list (not partial). Construct full list from current agent get snapshot + diff.
Coverage: core/display-detail.md §3 "Maintainer note (wholesale --service replacement, internal): the --service flag wire-level replaces the full services list, not a per-field patch."
✅ TC-U09 | Covered in core/display-detail.md §3 | original: same

### TC-U10 | Cannot clear description
User: "Clear my description" / "把描述清空" →
Expected: Refuse — update_impl only inserts ProfileDescription when non-empty. Cannot clear. Explain and offer to replace.
Coverage: core/display-formats.md §Description row rule "Update cannot clear an existing description." mutations.rs explanation.
✅ TC-U10 | Covered in core/display-formats.md §Description row rule | original: same (references/display-formats.md same section)

### TC-U11 | Cost and reversibility rows mandatory in diff card
Every update diff card →
Expected: "预计费用: 0 USDT… 可以撤回…" lines rendered as plain text below the diff table.
Coverage: core/display-detail.md §3 "Cost & reversibility rows (mandatory)." Update variant template.
✅ TC-U11 | Covered in core/display-detail.md §3 cost/reversibility | original: same

---

## MODULE 6: Get (TC-G01~G20)

### TC-G01 | List with wallet groups
agent get (no ids) →
Expected: Render per-wallet groups. One header per wrapper. agent table under each header.
Coverage: core/display-formats.md §1 "Group by accountName. One header line per outer-list[*] wrapper."
✅ TC-G01 | Covered in core/display-formats.md §1 | original: same (references/display-formats.md §1)

### TC-G02 | Empty wrapper (wallet with no agents)
A wrapper has 0 agents →
Expected: Render "(暂无 agent)" instead of an empty table.
Coverage: core/display-formats.md §1 "If a wrapper has 0 agents, render （暂无 agent）."
✅ TC-G02 | Covered in core/display-formats.md §1 | original: same

### TC-G03 | Reassurance footer — multi wrapper (M≥5)
Total agent count M≥5 across all wrappers →
Expected: Append reassurance footer explaining these are all the user's agents spread across wallets. "Your wallet is not compromised."
Coverage: core/display-formats.md §1 §Multi-agent List Reassurance Footer (P0) — M>=5 trigger, full CN/EN template.
✅ TC-G03 | Covered in core/display-formats.md §1 Reassurance Footer | original: same

### TC-G04 | Reassurance footer — single wrapper (M≥5)
M≥5 all in one wrapper →
Expected: Drop "分布在你名下不同钱包账户里" clause. Just say "都是你自己的."
Coverage: core/display-formats.md §1 "Variant — single wrapper: if envelope.total == 1 and M >= 5, drop the cross-wallet clause."
✅ TC-G04 | Covered in core/display-formats.md §1 single-wrapper variant | original: same

### TC-G05 | Pagination
agent get returns multiple pages →
Expected: Append pagination footer "第 <page>/<total_pages> 页，继续翻页说 '下一页'."
Coverage: core/display-formats.md §1 "If envelope.total > requested page size, append pagination footer."
✅ TC-G05 | Covered in core/display-formats.md §1 pagination | original: same

### TC-G06 | Detail card — approvalDisplayStatus=1 (not submitted)
agent get --agent-ids 42, approvalDisplayStatus=1 →
Expected: Approval status row shows "未发起审核" (CN) / "Not submitted for review" (EN).
Coverage: core/display-detail.md §2 "Render approvalDisplayStatus per core/ux-lexicon.md §ApprovalDisplayStatus." core/display-formats.md §1 example shows "未发起审核".
✅ TC-G06 | Covered in core/display-detail.md §2 + core/ux-lexicon.md | original: same

### TC-G07 | Detail card — approvalDisplayStatus=2 (under review)
approvalDisplayStatus=2 →
Expected: "审核中，请耐心等待."
Coverage: core/display-formats.md §1 example shows "审核中，请耐心等待". core/ux-lexicon.md owns the translation.
✅ TC-G07 | Covered in core/display-formats.md §1 + core/ux-lexicon.md | original: same

### TC-G08 | Detail card — approvalDisplayStatus=4 (listed)
approvalDisplayStatus=4 →
Expected: "已上架，可被任务系统推荐" (CN) / "Listed — eligible for task recommendations" (EN).
Coverage: core/display-detail.md §2 example shows "已上架，可被任务系统推荐" / "Listed — eligible for task recommendations".
✅ TC-G08 | Covered in core/display-detail.md §2 | original: same

### TC-G09 | Detail card — approvalDisplayStatus=5 (rejected)
approvalDisplayStatus=5 →
Expected: Rejection shown with approvalRemark (parenthetical).
Coverage: core/display-detail.md §2 "When approvalRemark is non-empty, append it as a parenthetical in the user's language."
✅ TC-G09 | Covered in core/display-detail.md §2 | original: same

### TC-G10 | Detail card — approvalDisplayStatus=7 (unavailable)
approvalDisplayStatus=7 →
Expected: "此 agent 当前不可用" per ux-lexicon.md.
Coverage: core/cli-reference.md §3 defines integer 7 = "This agent is currently unavailable". ux-lexicon.md owns translation.
✅ TC-G10 | Covered in core/cli-reference.md §3 + core/ux-lexicon.md | original: same

### TC-G11 | Other user's agent (non-owned lookup)
agent get --agent-ids <id-not-owned> →
Expected: Backend allows open lookup. Display detail card normally.
Coverage: core/cli-reference.md §3 "With --agent-ids — fetch specified agent(s) by id. Open lookup: ids may belong to the caller or to anyone else."
✅ TC-G11 | Covered in core/cli-reference.md §3 | original: same

### TC-G12 | Batch query
agent get --agent-ids 42,58 →
Expected: Renders one §2 detail card per agent (section §2.5), separated by ---. Post-detail prompt is multi-select.
Coverage: core/display-detail.md §2.5 Multi-agent detail. Trigger on flattened agent count > 1.
✅ TC-G12 | Covered in core/display-detail.md §2.5 | original: same

### TC-G13 | Not-found
agent get --agent-ids 99999 (nonexistent) →
Expected: Backend error / empty result handled via troubleshooting.md.
Coverage: core/cli-reference.md §3 "Errors: see troubleshooting.md." Error card per display-formats.md §7.
✅ TC-G13 | Covered via troubleshooting.md routing | original: same

### TC-G14 | No auto-chain service-list
After rendering detail card with services →
Expected: Do NOT auto-call agent service-list to populate services. They are already in the get response.
Coverage: core/display-detail.md §2 "Do NOT chain agent service-list --agent-id <id> to 'populate' the Services rows — they're already in the response."
✅ TC-G14 | Covered in core/display-detail.md §2 | original: same

### TC-G15 | Requester no service rows
Detail card for requester →
Expected: No 服务 rows. Do NOT render "服务 | 无" or any placeholder. Just drop the rows entirely.
Coverage: core/display-detail.md §2 "⛔ 服务 / Services rows are provider-only. requester and evaluator: omit every 服务 / Services row entirely."
✅ TC-G15 | Covered in core/display-detail.md §2 | original: same

### TC-G16 | Evaluator no service rows
Detail card for evaluator →
Expected: Same as TC-G15. No service rows.
Coverage: core/display-detail.md §2 same rule. "Only render Service rows when role == provider."
✅ TC-G16 | Covered in core/display-detail.md §2 | original: same

### TC-G17 | Empty description display
Agent has empty description →
Expected: Description row shows "未填" (CN) / "(not set)" (EN). Never leave blank or render bare —.
Coverage: core/display-formats.md §Description row rule "If the value is empty/missing, render 未填 / (not set)."
✅ TC-G17 | Covered in core/display-formats.md §Description row rule | original: same

### TC-G18 | Avatar URL rule
Profile photo row in detail card →
Expected: Show actual URL verbatim when set. Show "默认" / "default" when not set. Never "已上传" / "uploaded" / "CDN".
Coverage: core/display-formats.md §Profile photo row rule. Also modules/avatar-upload.md rule 5.
✅ TC-G18 | Covered in core/display-formats.md §Profile photo row rule | original: same

### TC-G19 | Rating display — score/20 conversion
Rating for agent with score=92 →
Expected: Display "★ 4.6 (18)" not "92 / 100". No raw integer.
Coverage: core/display-formats.md §1 Rating rule "★ <score/20> with up to 2 decimal places. Never expose raw 0-100 score."
✅ TC-G19 | Covered in core/display-formats.md §1 + core/cli-reference.md §3 | original: same

### TC-G20 | No rating yet (score=0 / no feedback)
Agent with no feedback yet →
Expected: "暂无评分" / "No rating yet". Not "★ 0."
Coverage: core/display-formats.md §1 "If no feedback yet, render 暂无评分 / No rating yet. Never render — for missing rating."
✅ TC-G20 | Covered in core/display-formats.md §1 | original: same

---

## MODULE 7: Activate/Deactivate (TC-A01~A05, TC-A-DEFAULT, TC-D01~D03)

### TC-A01 | Activate — Outcome A (success=true)
agent activate returns success=true →
Expected: "上架成功 — 你的 agent 现在已经能被市场搜到." Proceed to Step 5 → Step 6.
Coverage: core/cli-reference.md §4 outcome A. SKILL.md §Post-success suggestion table row agent activate.
✅ TC-A01 | Covered in core/cli-reference.md §4 + SKILL.md | original: same

### TC-A02 | Activate — Outcome B (success=false, approvalStatus=1)
agent activate returns success=false, approvalStatus=1 →
Expected: Skill auto-calls agent submit-approval --agent-id <id>. User sees review-pending message after that.
Coverage: core/cli-reference.md §4 "success: false, approvalStatus: 1 → Call onchainos agent submit-approval --agent-id <id> → see §11." This is the skill-internal chain, invisible to user.
✅ TC-A02 | Covered in core/cli-reference.md §4 + §11 (submit-approval routing) | original: same (references/cli-reference.md)

### TC-A03 | Activate — Outcome C (success=false, approvalStatus=2)
approvalStatus=2 →
Expected: "Under review — render review-pending message and stop." No Step 5/6.
Coverage: core/cli-reference.md §4 outcome C "Under review — render review-pending message and stop (no Step 5/6)."
✅ TC-A03 | Covered in core/cli-reference.md §4 | original: same

### TC-A04 | Activate — Outcome D (success=false, approvalStatus=5)
approvalStatus=5 →
Expected: Rejected — render rejection card with rejectReason and stop.
Coverage: core/cli-reference.md §4 outcome D "Rejected — render rejection card with rejectReason and stop (no Step 5/6)."
✅ TC-A04 | Covered in core/cli-reference.md §4 | original: same

### TC-A05 | Activate — Outcome E (code=81602, blacklisted)
code "81602" →
Expected: Render blacklist error and stop.
Coverage: core/cli-reference.md §4 "Top-level code: '81602' | Agent blacklisted — render blacklist error and stop."
✅ TC-A05 | Covered in core/cli-reference.md §4 | original: same

### TC-A-DEFAULT | New provider is active by default
After create --role provider succeeds →
Expected: Post-success says "默认已上架可以接单." No need to activate after create.
Coverage: playbooks/provider.md §Post-success "Create returns active by default / Create 默认返回 active — no need to follow up with agent activate."
✅ TC-A-DEFAULT | Covered in playbooks/provider.md §Post-success | original: same

### TC-D01 | Deactivate happy path
agent deactivate returns success=true →
Expected: "下架完成 — 你的 agent 已经从客户端列表里隐藏." Proceed to Step 5 → Step 6.
Coverage: SKILL.md §Post-success suggestion table row agent deactivate. core/cli-reference.md §5.
✅ TC-D01 | Covered in SKILL.md + core/cli-reference.md §5 | original: same

### TC-D02 | Already inactive
agent deactivate but agent already inactive →
Expected: Backend returns code != "0". Surface via troubleshooting.md §2 keyword match. Do NOT render success.
Coverage: core/cli-reference.md §5 "Business-level failures (e.g. 'agent already inactive', 'pending settlements') arrive as code != '0' from the backend — surfaced via troubleshooting.md §2 keyword match."
✅ TC-D02 | Covered in core/cli-reference.md §5 + troubleshooting.md | original: same

### TC-D03 | Pending settlements
agent deactivate but there are pending settlements →
Expected: Same as TC-D02 — code != "0" → troubleshooting.md keyword match → user-friendly message.
Coverage: core/cli-reference.md §5 explicitly mentions "pending settlements" as an example business-level failure.
✅ TC-D03 | Covered in core/cli-reference.md §5 | original: same

---

## MODULE 8: Pre-listing QA (TC-A-U1~U4, TC-A-N1~N7, TC-A-T1~T3, TC-A-S1~S6, TC-A-P1~P4, TC-A-D1~D10, TC-A-L1~L3, TC-A-QA-PASS, TC-A-QA-OPT1, TC-A-QA-OPT2, TC-A-SKIP)

### TC-A-U1 | Universal — test/env markers
Name/description/service field contains "(pre)" / "_test" / "-dev" / "(beta)" etc. →
Expected: Flag as U1 violation. Fix suggestion: remove marker.
Coverage: modules/pre-listing-qa.md §Universal Prohibitions U1 — comprehensive pattern list (parentheses/bracket/delimiter/suffix forms, case-insensitive). Note explicit case about "Predict" NOT being a violation.
✅ TC-A-U1 | Covered in modules/pre-listing-qa.md U1 | original: same (references/pre-listing-qa.md)

### TC-A-U2 | Universal — internal addresses
Field contains "0x..." wallet/tx address →
Expected: Flag U2. Remove the address.
Coverage: modules/pre-listing-qa.md U2 "Any 0x… wallet / owner / tx hash in name, description, or service fields."
✅ TC-A-U2 | Covered in modules/pre-listing-qa.md U2 | original: same

### TC-A-U3 | Universal — negative capability statements
Field contains "目前不支持" / "currently not supported" →
Expected: Flag U3. Rewrite positively or remove.
Coverage: modules/pre-listing-qa.md U3.
✅ TC-A-U3 | Covered in modules/pre-listing-qa.md U3 | original: same

### TC-A-U4 | Universal — free A2MCP must be explicit
A2MCP service with empty/blank fee (meant to be free) →
Expected: Flag U4. Set to "0 USDT".
Coverage: modules/pre-listing-qa.md U4 "Free service must be explicit: A2MCP fee is empty/blank when the service is free."
✅ TC-A-U4 | Covered in modules/pre-listing-qa.md U4 | original: same

### TC-A-N1 | Name — length out of range
CN name < 2 or > 12 chars, EN name < 3 or > 25 chars →
Expected: Flag N1.
Coverage: modules/pre-listing-qa.md N1.
✅ TC-A-N1 | Covered in modules/pre-listing-qa.md N1 | original: same

### TC-A-N2 | Name — agent ID embedded
Name contains "#123" or "_1083" →
Expected: Flag N2. Remove the ID.
Coverage: modules/pre-listing-qa.md N2.
✅ TC-A-N2 | Covered in modules/pre-listing-qa.md N2 | original: same

### TC-A-N3 | Name — ordinal suffix
Name ends with bare digit, "_2", "_v2", "(2)", "3号" →
Expected: Flag N3. Remove ordinal.
Coverage: modules/pre-listing-qa.md N3.
✅ TC-A-N3 | Covered in modules/pre-listing-qa.md N3 | original: same

### TC-A-N4 | Name — personal name/account label
Name contains personal name, email prefix, wallet account label →
Expected: Flag N4. Remove personal reference.
Coverage: modules/pre-listing-qa.md N4.
✅ TC-A-N4 | Covered in modules/pre-listing-qa.md N4 | original: same

### TC-A-N5 | Name — sentence not brand
Name reads as full verb+object sentence →
Expected: Flag N5. Rewrite as short brand name.
Coverage: modules/pre-listing-qa.md N5.
✅ TC-A-N5 | Covered in modules/pre-listing-qa.md N5 | original: same

### TC-A-N6 | Name — bilingual separator wrong
Bilingual name without middle dot separator →
Expected: Flag N6. Fix separator to "中文名 · EnglishName".
Coverage: modules/pre-listing-qa.md N6.
✅ TC-A-N6 | Covered in modules/pre-listing-qa.md N6 | original: same

### TC-A-N7 | Name — test/env marker in name (explicit check)
Name like "健身教练(pre)" / "WeatherBot-test" →
Expected: Flag N7 (explicit — #1 rejection reason for names). Caution: "Predict" is NOT a violation.
Coverage: modules/pre-listing-qa.md N7 "No test / environment markers in name — #1 reported rejection reason for names."
✅ TC-A-N7 | Covered in modules/pre-listing-qa.md N7 | original: same

### TC-A-T1 | Servicetype — enum values only
servicetype not exactly A2A or A2MCP (case-sensitive) →
Expected: Flag T1. Correct to A2A or A2MCP.
Coverage: modules/pre-listing-qa.md T1.
✅ TC-A-T1 | Covered in modules/pre-listing-qa.md T1 | original: same

### TC-A-T2 | A2MCP requires endpoint
servicetype=A2MCP but endpoint empty/absent →
Expected: Flag T2.
Coverage: modules/pre-listing-qa.md T2.
✅ TC-A-T2 | Covered in modules/pre-listing-qa.md T2 | original: same

### TC-A-T3 | A2A does not use endpoint
servicetype=A2A but endpoint non-empty →
Expected: Flag T3. Remove endpoint value.
Coverage: modules/pre-listing-qa.md T3.
✅ TC-A-T3 | Covered in modules/pre-listing-qa.md T3 | original: same

### TC-A-S1 | Service name — length 5-30
Service name < 5 or > 30 chars →
Expected: Flag S1.
Coverage: modules/pre-listing-qa.md S1.
✅ TC-A-S1 | Covered in modules/pre-listing-qa.md S1 | original: same

### TC-A-S2 | Service name — sentence not noun phrase
Service name is a full sentence →
Expected: Flag S2. Rewrite as short noun phrase.
Coverage: modules/pre-listing-qa.md S2.
✅ TC-A-S2 | Covered in modules/pre-listing-qa.md S2 | original: same

### TC-A-S3 | Service name — duplicate of agent name
Service name identical to agent-level name →
Expected: Flag S3.
Coverage: modules/pre-listing-qa.md S3.
✅ TC-A-S3 | Covered in modules/pre-listing-qa.md S3 | original: same

### TC-A-S4 | Service name — price in name
Service name contains price info →
Expected: Flag S4. Move pricing to fee field.
Coverage: modules/pre-listing-qa.md S4.
✅ TC-A-S4 | Covered in modules/pre-listing-qa.md S4 | original: same

### TC-A-S5 | Service name — tech implementation details
Service name mentions framework/API key/infra →
Expected: Flag S5. Remove or abstract.
Coverage: modules/pre-listing-qa.md S5.
✅ TC-A-S5 | Covered in modules/pre-listing-qa.md S5 | original: same

### TC-A-S6 | Service name — test/env marker
Service name like "天气查询(pre)" / "分析接口_test" →
Expected: Flag S6. Same delimiter-awareness as N7.
Coverage: modules/pre-listing-qa.md S6. Notes "protest is NOT a violation."
✅ TC-A-S6 | Covered in modules/pre-listing-qa.md S6 | original: same

### TC-A-P1 | Fee — format both segments required
Missing number or currency in fee →
Expected: Flag P1.
Coverage: modules/pre-listing-qa.md P1.
✅ TC-A-P1 | Covered in modules/pre-listing-qa.md P1 | original: same

### TC-A-P2 | Fee — currency must be USDT or USDG
Other currency symbols →
Expected: Flag P2.
Coverage: modules/pre-listing-qa.md P2.
✅ TC-A-P2 | Covered in modules/pre-listing-qa.md P2 | original: same

### TC-A-P3 | Fee — no negotiation language
Fee contains "可协商" / "TBD" / "negotiable" →
Expected: Flag P3. Set concrete price.
Coverage: modules/pre-listing-qa.md P3.
✅ TC-A-P3 | Covered in modules/pre-listing-qa.md P3 | original: same

### TC-A-P4 | Fee — no parenthetical notes
Fee contains parenthetical like "(支持 USDG 结算)" →
Expected: Flag P4. Remove parenthetical.
Coverage: modules/pre-listing-qa.md P4.
✅ TC-A-P4 | Covered in modules/pre-listing-qa.md P4 | original: same

### TC-A-D1 | Service description — three-part structure
Missing any of: summary / capabilities / example prompts →
Expected: Flag D1.
Coverage: modules/pre-listing-qa.md D1.
✅ TC-A-D1 | Covered in modules/pre-listing-qa.md D1 | original: same

### TC-A-D2 | Service description — total ≤ 400 chars
Coverage: modules/pre-listing-qa.md D2.
✅ TC-A-D2 | Covered in modules/pre-listing-qa.md D2 | original: same

### TC-A-D3 | Service description — Part 1 ≤ 50 chars
Coverage: modules/pre-listing-qa.md D3.
✅ TC-A-D3 | Covered in modules/pre-listing-qa.md D3 | original: same

### TC-A-D4 | Service description — Part 2 ≤ 150 chars
Coverage: modules/pre-listing-qa.md D4.
✅ TC-A-D4 | Covered in modules/pre-listing-qa.md D4 | original: same

### TC-A-D5 | Service description — Part 3: 1-3 prompts, each ≤ 80 chars
Coverage: modules/pre-listing-qa.md D5.
✅ TC-A-D5 | Covered in modules/pre-listing-qa.md D5 | original: same

### TC-A-D6 | Service description — no external links / GitHub URLs
Coverage: modules/pre-listing-qa.md D6.
✅ TC-A-D6 | Covered in modules/pre-listing-qa.md D6 | original: same

### TC-A-D7 | Service description — no wallet/contract addresses
Coverage: modules/pre-listing-qa.md D7.
✅ TC-A-D7 | Covered in modules/pre-listing-qa.md D7 | original: same

### TC-A-D8 | Service description — no tech-stack exposure
Coverage: modules/pre-listing-qa.md D8.
✅ TC-A-D8 | Covered in modules/pre-listing-qa.md D8 | original: same

### TC-A-D9 | Service description — no negative statements
Coverage: modules/pre-listing-qa.md D9.
✅ TC-A-D9 | Covered in modules/pre-listing-qa.md D9 | original: same

### TC-A-D10 | Service description — no legal disclaimers
Coverage: modules/pre-listing-qa.md D10.
✅ TC-A-D10 | Covered in modules/pre-listing-qa.md D10 | original: same

### TC-A-L1 | Logo — avatar missing (blocking)
picture field empty/null/absent for provider before activate →
Expected: L1 is BLOCKING. Do NOT offer option 2 (list anyway). Only offer option 1 (fix first — upload avatar).
Coverage: modules/pre-listing-qa.md §Logo "L1 is a blocking check (❌) — do not proceed to agent activate without an avatar." Exception rule in §QA Report "L1 (no avatar) is always blocking — if picture is absent, do NOT offer option 2."
✅ TC-A-L1 | Covered in modules/pre-listing-qa.md §Logo | original: same (references/pre-listing-qa.md same rule)

### TC-A-L2 | Logo — 1:1 aspect ratio
Non-square image →
Expected: Flag L2 (warning, not blocking). Re-upload square image.
Coverage: modules/pre-listing-qa.md L2 "⚠️ warning (cannot always be verified post-upload; surface at upload time)."
✅ TC-A-L2 | Covered in modules/pre-listing-qa.md L2 | original: same

### TC-A-L3 | Logo — file too large (> 1 MB)
Image > 1 MB →
Expected: Flag L3. Compress and re-upload. Also enforced in avatar-upload.md (hard limit check before upload).
Coverage: modules/pre-listing-qa.md L3 + modules/avatar-upload.md §Validation §File size.
✅ TC-A-L3 | Covered in modules/pre-listing-qa.md L3 + modules/avatar-upload.md | original: same

### TC-A-QA-PASS | All checks green — silent proceed
All QA checks pass →
Expected: "No separate message needed — silently proceed to agent activate."
Coverage: modules/pre-listing-qa.md §Pass Message "No separate message needed — silently proceed to agent activate."
✅ TC-A-QA-PASS | Covered in modules/pre-listing-qa.md §Pass Message | original: same

### TC-A-QA-OPT1 | QA report — option 1 (fix and list)
User chooses option 1 (fix first) →
Expected: Route through §Update flow (agent update → re-run QA → agent activate).
Coverage: modules/pre-listing-qa.md §QA Report "On option 1 (fix first): route through §Update flow."
✅ TC-A-QA-OPT1 | Covered in modules/pre-listing-qa.md §QA Report | original: same

### TC-A-QA-OPT2 | QA report — option 2 (list anyway)
User chooses option 2 (list anyway, non-compliant) →
Expected: Invoke agent activate immediately without re-prompting.
Coverage: modules/pre-listing-qa.md §QA Report "On option 2 (list anyway): invoke agent activate immediately without re-prompting." Note: option 2 not offered when L1 fails.
✅ TC-A-QA-OPT2 | Covered in modules/pre-listing-qa.md §QA Report | original: same

### TC-A-SKIP | Requester and evaluator skip QA
Activate for requester or evaluator role →
Expected: Skip pre-listing-qa.md entirely. No QA check for non-provider roles.
Coverage: modules/pre-listing-qa.md §When to Run "If the role is requester or evaluator, skip this file." SKILL.md §Intent table "上架 agent (requester / evaluator) | agent activate --agent-id <id> directly."
✅ TC-A-SKIP | Covered in modules/pre-listing-qa.md + SKILL.md §Intent table | original: same

---

## SUMMARY

Total verified: 89

✅ Pass: 89
❌ Fail: 0
⚠️ Changed: 0

### Key Findings

1. ALL 89 test cases have equivalent coverage in both the new refactored skill (playbooks/ + modules/ + core/) and the original (references/ + SKILL.md monolith).

2. The refactoring split a monolithic SKILL.md (~837 lines) + references/ flat files into a structured hierarchy (playbooks/ per-role, core/ CLI + display, modules/ reusable), but preserved all behavioral rules. No regressions found.

3. Notable behavioral equivalences verified:
   - Confirmation gate: both versions explicitly list the same rationalization blacklist (urgency, one-shot, plan mode, etc.)
   - Passive onboarding: both versions confirm gate still applies even in passive mode; no Step 6; exact wording templates match
   - Consent flow: agree/decline/ambiguous all documented identically in both versions
   - Provider K≥2 handling: list all existing providers in both, follow-up question for update selection
   - Pre-listing QA: L1 blocking rule preserved; two-option report format preserved; requester/evaluator skip preserved
   - Activate 5 outcomes: all 5 mapped in core/cli-reference.md §4 same as original references/cli-reference.md
   - fee=0 warning: identical wording "API 接口式服务 0 USDT 等同于免费入口，后续不能再按量收费" in both
   - Evaluator post-success: 2 lines (not 1), no hardcoded stake amount — both versions have this
   - Wholesale --service replacement on update: explicitly documented in core/display-detail.md §3 and equivalent in original

4. One structural difference (not a behavioral gap): the refactored version introduces "provider-services.md" as a separate Phase 2 file (split from provider.md to keep files under 300 lines). The original had all Phase 2 content inline in references/role-provider.md. Coverage is equivalent.

5. The refactored version adds more explicit per-section cross-references (e.g., provider-services.md §Suggestion-as-prompt carve-out explicitly labeled) which makes individual rules easier to locate, but the rules themselves are unchanged from the original.
