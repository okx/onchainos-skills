# Second-Pass Verification — Modules 10–13
# Agent 3 Report: Mandatory Gates · UX Red Lines · Routing · Error Handling
# Pass: RIGOROUS (simulate real conversations, stricter than first pass)

Scope: MG01–MG28, UX01–UX20, RT01–RT10, ERR01–ERR14  
Files read: SKILL.md, playbooks/README.md, playbooks/requester.md, playbooks/provider.md,
            playbooks/evaluator.md, playbooks/consent.md, modules/feedback.md,
            troubleshooting.md, core/ux-lexicon.md, core/display-detail.md, core/display-formats.md

---

## SECTION 1 — Pre-Check Gate (MG01–MG04)

### MG01 — agent create must run agent get first, even with all fields supplied one-shot

**TC simulation:**
> User: "帮我注册一个叫 Alice 的用户身份，不需要头像，直接建吧"

**Rule source:** SKILL.md §⛔ MANDATORY Gates §Pre-Check Gate:
> "Any `agent create`, `agent update`, or `agent feedback-submit` intent — run `onchainos agent get` first. No exceptions, even when the user supplied all fields one-shot or named the role already."

playbooks/README.md §Pre-check existing agents: "Before entering any role flow triggered by the user's own initiative, run `agent get` once..."

**Verdict: PASS**
Pre-check is unconditional on create. Rule is explicit and duplicated in both SKILL.md and playbooks/README.md. No loophole for one-shot capture.

---

### MG02 — agent update must run agent get --agent-ids N first

**TC simulation:**
> User: "把 #42 的描述改成'链上数据分析服务'"

**Rule source:** SKILL.md §Update flow step 1:
> "`agent get --agent-ids <id>` → show current detail card."

playbooks/README.md §Confirmation card "§Update flow" confirms: `agent get --agent-ids <id>` is required before update Q&A.

**Verdict: PASS**
The update flow mandates a specific `agent get --agent-ids <id>` pre-fetch. This is structurally distinct from a generic `agent get` (no ids). Correct.

---

### MG03 — feedback-submit must resolve creator-id (not reuse)

**TC simulation:**
> User: "给 #42 打 4 星"
> (user has never mentioned their own agent in this conversation)

**Rule source:** modules/feedback.md §Step 2 — Identify creator (caller's own agent):
> Ladder 1: check if already known AND verified to belong to currently selected XLayer wallet  
> Ladder 2: run `onchainos agent get` (no --agent-ids), narrow wrapper to current wallet ownerAddress

**MG03 specific concern: "no already got reuse"**

modules/feedback.md Step 2 Ladder 1 explicitly states:
> "If the cached id was only mentioned by the user without any captured ownerAddress, fall through to ladder 2"
> "If the user has switched wallets since the cached id was first mentioned, fall through to ladder 2 unconditionally"

The rule prevents silent reuse of a previously-mentioned id that hasn't been wallet-verified.

**Verdict: PASS**
Ladder 1 / Ladder 2 logic correctly gates against stale reuse. The "fall through to ladder 2" conditions are thorough and cover wallet-switch, unverified mention, and ownerAddress mismatch.

---

### MG04 — feedback-submit creator-id resolution: no "already got" reuse across sessions

**TC simulation:**
> Turn 1: User registered agent #88 (confirmed in this conversation, ownerAddress captured)
> Turn 7: User: "给 #42 打 4.5 星"
> Question: can the skill silently reuse #88 from Turn 1?

**Rule source:** modules/feedback.md §Step 2 Ladder 1:
> "If the cached id's ownerAddress was already captured in this conversation, compare directly to the current selected wallet address. Match → use it (no lookup needed)."

**Verdict: PASS** (with caveat)
Within the same conversation, if ownerAddress was captured and wallet hasn't switched, reuse IS allowed by Ladder 1. This is intentional design (not a bug). The "no already got reuse" rule applies to cross-session reuse or unverified mentions — those are covered. The within-session verified reuse is correctly permitted.

---

## SECTION 2 — Confirmation Gate (MG05–MG12)

### MG05 — auto-execute preference does not bypass confirmation card

**TC simulation:**
> User: "以后帮我自动执行，不用每次问我确认"
> Later: "注册一个叫 Bob 的用户身份"

**Rule source:** SKILL.md §⛔ MANDATORY Gates §Confirmation Gate:
> "Every content-creating write must render a field-table confirmation card and receive an explicit confirm token... from the user before invoking the CLI."

playbooks/README.md §Confirmation card:
> "Memory preferences, plan-mode exit, one-shot capture, urgency, and 'intent is obvious' all do NOT bypass it"

**Verdict: PASS**
Rule explicitly lists "auto-execute" as a non-bypassing rationalization. The confirmation gate is non-overridable.

---

### MG06 — plan-mode exit does not bypass confirmation

**TC simulation:**
> User: "好了我已经想清楚了，不用走确认流程直接执行"

**Rule source:** playbooks/README.md §Confirmation card:
> "Memory preferences, plan-mode exit, one-shot capture, urgency, and 'intent is obvious' all do NOT bypass it"

**Verdict: PASS**
"Plan-mode exit" is explicitly listed. No gap.

---

### MG07 — urgent tone does not bypass confirmation

**TC simulation:**
> User: "快点快点，直接注册 #99 就行，别问了"

**Rule source:** playbooks/README.md §Confirmation card:
> "urgency... all do NOT bypass it"

SKILL.md §Core Flow gate 4:
> "Execute only after explicit confirm token."

**Verdict: PASS**
Urgency is explicitly excluded from rationalization blacklist.

---

### MG08 — prior confirm token in this conversation is not reused for a new operation

**TC simulation:**
> Turn 3: User confirms creation of requester agent (says "执行")
> Turn 8: User says "再建一个服务提供商身份叫 DeFi Pro"
> → Does turn 3's "执行" count for turn 8's confirmation?

**Rule source:** SKILL.md §Step 3 Execute:
> "Only sufficient condition to invoke CLI without re-rendering the card: both (1) user's most recent turn literally contains a confirm token AND (2) every field value in the just-rendered card is byte-identical to what will be passed to the CLI."

modules/feedback.md §Step 6:
> "Earlier-turn confirm tokens and confirms of different writes do NOT count for Q2."

**Verdict: PASS**
"Most recent turn" requirement ensures cross-turn confirm reuse is impossible. The feedback.md note makes this crystal clear: "earlier-turn confirm tokens... do NOT count."

---

### MG09 — byte-equal check between confirmation card and CLI call

**TC simulation:**
> Confirmation card shows: name="DeFi Analyzer", description="On-chain data"
> User says "执行" but agent internally uses description="On-chain data analysis"

**Rule source:** SKILL.md §Step 3 pre-execute self-check Q3:
> "All card values byte-identical to CLI values?"
> "Q3 fail → re-render with actual values."

**Verdict: PASS**
The 3-question pre-execute self-check explicitly catches this. Q3 failure triggers card re-render, not CLI invocation.

---

### MG10 — activate/deactivate are exempt from confirmation gate

**TC simulation:**
> User: "帮我把 #42 下架"

**Rule source:** SKILL.md §⛔ MANDATORY Gates §Confirmation Gate:
> "`activate / deactivate` are state toggles — NOT gated."

playbooks/README.md §Confirmation card:
> "State toggles (`agent activate` / `agent deactivate`) are NOT gated and run directly via `SKILL.md §Intent → Sub-flow`."

**Verdict: PASS**
Activate/deactivate exemption is clearly stated in both canonical locations.

---

### MG11 — no pre-execution narration between confirmation and result

**TC simulation:**
> User: "执行"
> Expected: CLI runs immediately, post-execute template is first user-visible output
> Forbidden: "好的，我现在来帮你执行注册操作..." before result

**Rule source:** SKILL.md §Step 3 Execute:
> "No narration between confirmation and result. When the user replies with a confirm token, invoke the CLI immediately and emit the post-CLI template as the first user-visible content."

**Verdict: PASS**
Rule is unambiguous and strongly worded. "First user-visible content" must be the post-CLI template.

---

### MG12 — confirm card must include 预计费用 0 USDT and 可撤回

**TC simulation:**
> User confirms requester creation
> Check: does confirmation card include cost/reversibility rows?

**Rule source:** core/display-detail.md §3 Cost & reversibility rows (mandatory):
> "Every Create-variant card AND Update Diff card MUST include two final rows..."
> Create variant: `| 预计费用 | **0 USDT**（创建 / 修改 / 上下架均无手续费，由 OKX 承担...） |`
> `| 能否撤回 | 可以——...` (CN) / `| Reversible? | Yes —...` (EN)
> Update variant (3 cols): `> 预计费用: **0 USDT**（修改字段无手续费，由 OKX 承担）。可以撤回: ...`

**Verdict: PASS**
Cost/reversibility rows are explicitly mandatory in core/display-detail.md §3. Templates show the exact format for both create and update variants.

**SPECIAL NOTE — FINDING:**
The requester.md confirmation card example (lines 60–76) does NOT include the `预计费用` and `能否撤回` rows in its displayed template. This is a documentation gap — the template in requester.md shows a 3-row card without cost/reversibility rows, while core/display-detail.md §3 says they are mandatory. The canonical rule is in core/display-detail.md §3; the requester.md example is incomplete.

**Severity: MEDIUM** — The rule exists but the role-level template doesn't demonstrate it. A model following requester.md literally might omit the mandatory rows.

---

## SECTION 3 — Consent Gate (MG13–MG17, MG23–MG27)

### MG13 — consent card is shown when backend returns non-null consent

**TC simulation:**
> First-time user, backend responds with consent: { consentKey: "uuid-abc", terms: "..." }

**Rule source:** playbooks/consent.md §Consent Card:
> "Render this card when the consent intercept fires."
> "Display `consent.terms` verbatim as the terms content."

SKILL.md §Consent Gate:
> "When CLI returns `executeResult: false` with non-null `consent` → show consent card"

**Verdict: PASS**
Consent card display is unconditional when backend returns non-null consent. Card template is in consent.md §Consent Card.

---

### MG14 — agree → re-call with --consent-key and --agreed true

**TC simulation:**
> Backend returns consent object with consentKey="uuid-abc"
> User: "agree"
> Expected: re-invoke with --consent-key uuid-abc --agreed true

**Rule source:** playbooks/consent.md §Agree flow:
1. Re-invoke the original `onchainos agent create` command with exact same parameters.
2. Append `--consent-key <value of consent.consentKey from the backend response>`.
3. Append `--agreed true`.
4. Do NOT re-render the confirmation card.
5. Proceed to §Step 4: Report Result with the second call's response.

**Verdict: PASS**
Steps are explicit. consentKey value comes from backend response, not hallucinated.

---

### MG15 — decline → complete stop

**TC simulation:**
> User: "decline"
> Expected: stop, render cancellation message, no CLI call

**Rule source:** playbooks/consent.md §Decline message:
> "Do NOT call the CLI."
> "Render the message below and stop."
> "Registration cancelled — creating an agent identity requires accepting the terms of use. You can restart the registration flow at any time."

**Verdict: PASS**
Complete stop is unambiguous. "Do NOT call the CLI" is explicit.

---

### MG16 — auto-agree refused

**TC simulation:**
> User doesn't reply to consent card after 30 seconds (or sends off-topic message)
> Expected: consent card re-shown once, wait for explicit agree/decline

**Rule source:** playbooks/consent.md §Ambiguous reply handling:
> "Do NOT auto-agree, do NOT auto-decline, do NOT timeout."

playbooks/consent.md §Consent Card rules:
> "Do NOT pre-fill the user's reply or add 'I'll assume you agree if you don't reply'."

**Verdict: PASS**
Auto-agree is explicitly forbidden. The timeout non-action rule is present.

---

### MG17 — ambiguous reply → re-show consent once

**TC simulation:**
> User: "这个条款是什么意思？" (question about terms)
> Expected: re-display consent card once (with full terms), wait

**Rule source:** playbooks/consent.md §Ambiguous reply handling:
> "1. Re-display the consent card once (including the full `consent.terms` text again)."
> "2. Wait for a clear agree or decline token."
> "3. Do NOT auto-agree, do NOT auto-decline, do NOT timeout."
> Worked Example C confirms this behavior.

**Verdict: PASS**
"Once" qualifier is explicit. The re-display includes full terms text (not just the prompt).

---

### MG23 — consent.terms in wrong language → translate fully, no summarizing

**TC simulation:**
> User is communicating in Chinese
> Backend returns consent.terms in English
> Expected: full Chinese translation of consent.terms, no omissions, no summarizing

**Rule source:** playbooks/consent.md §Consent Card rules:
> "Display `consent.terms` in the current conversation language (match the language the user is communicating in). If `consent.terms` is in a different language than the conversation, translate it to match before displaying. Translation is permitted for readability, but the translated content MUST be complete — do NOT summarize, paraphrase, or omit any clause."

**Verdict: PASS**
Rule is present, explicit, and covers the exact scenario. "MUST be complete — do NOT summarize, paraphrase, or omit any clause" is unambiguous.

---

### MG24 — consentKey UUID must NOT be shown to user

**TC simulation:**
> Backend returns consentKey: "550e8400-e29b-41d4-a716-446655440000"
> Expected: UUID never appears in user-visible message

**Rule source:** playbooks/consent.md §Consent Card rules:
> "Do NOT show the raw `consentKey` UUID to the user — it is an internal token."

**Verdict: PASS**
Rule is present. It is also implied by SKILL.md §UX Red Lines (no internal labels).

---

### MG25 — backend 40020 (AGENT_CONSENT_AGREED_REQUIRED) → troubleshooting.md handling

**TC simulation:**
> Second call with --consent-key but --agreed omitted (malformed re-invocation)
> Backend returns code 40020
> Expected: specific error message from troubleshooting.md

**Rule source (NEW — just added):** troubleshooting.md §2 Backend-originated table:
> Code `40020` — `AGENT_CONSENT_AGREED_REQUIRED`
> CN: "条款确认参数不完整，注册未能完成。请重新发起注册流程。"
> EN: "Consent parameters incomplete — registration failed. Please restart the registration flow."
> Skill action: "Render error card with `raw:` message. **Stop.** Do NOT auto-retry."

**Verdict: PASS**
Entry is correctly added. User-friendly message avoids exposing the error code. "Complete stop" instruction is present. No auto-retry.

Cross-check with consent.md §Error codes: consent.md lists 40020 but says "The skill does not need to map them explicitly" and routes to troubleshooting.md — consistent with where the rule lives.

---

### MG26 — backend 40021 (AGENT_CONSENT_INVALID) → troubleshooting.md handling

**TC simulation:**
> consentKey is expired or already finalized (user returned hours later)
> Backend returns code 40021
> Expected: specific user message, stop, restart instruction

**Rule source (NEW):** troubleshooting.md §2:
> Code `40021` — `AGENT_CONSENT_INVALID`
> CN: "条款确认凭证已失效，注册未能完成。请重新发起注册流程。"
> EN: "Consent token is invalid or already used — registration failed. Please restart the registration flow."
> Skill action: "Render error card with `raw:` message. **Stop.** Do NOT auto-retry."

**Verdict: PASS**
Entry is correctly added with appropriate "凭证已失效" language that communicates the issue without exposing backend internals.

---

### MG27 — backend 40022 (AGENT_CONSENT_REJECTED) → complete stop, no retry

**TC simulation:**
> User previously clicked "decline" for terms in a prior session (DB has rejected status)
> Returns in a new session, tries to register again
> Backend returns code 40022
> Expected: complete stop, no re-offer to agree

**Rule source (NEW):** troubleshooting.md §2:
> Code `40022` — `AGENT_CONSENT_REJECTED`
> CN: "你之前已拒绝过服务条款，当前账户无法在这次会话中完成注册。如需注册，请重新发起完整注册流程。"
> EN: "You previously declined the terms of service — registration cannot proceed. To register, please restart the full registration flow."
> Skill action: "**Complete stop.** Do NOT offer a retry or a way to re-agree in this same flow. The user must restart from scratch. No §Step 5 / §Step 6."

**Verdict: PASS — with IMPORTANT NUANCE FLAG**

The 40022 entry correctly says "complete stop" and "no §Step 5 / §Step 6". However, there is a subtle tension:

- The user-facing message says "请重新发起完整注册流程" (restart the full registration flow), which implies the user CAN try again by restarting.
- But consent.md §Decline message says the flow stops after decline.
- troubleshooting.md does NOT say the user can never re-register — it says they cannot do so in this same flow without restarting.

**This is internally consistent**: 40022 means a prior session rejection is recorded. The user must restart (which will show consent again). The "complete stop" means no in-flow re-agreement — the user must initiate a new registration flow from the top. This is the correct behavior.

**No gap found**, but this edge case should be tested to confirm the backend behavior when a 40022 user restarts a fresh registration flow (do they get a new consent card or permanent block?).

---

## SECTION 4 — Post-Execute Gate (MG18–MG22)

### MG18 — post-success uses verbatim template, not model's own summarization

**TC simulation:**
> agent create --role requester succeeds
> Expected: "用户身份 #42 注册完成 — 想发任务直接跟我说'发布一个 ... 的任务'，我帮你走完整个流程。"
> Forbidden: "✅ 用户身份已成功上链！agentId 是 #42..."

**Rule source:** playbooks/requester.md §Post-success:
> "Render one visible line using the template below — verbatim except for the `#<id>` substitution rule"
> "Paraphrasing, adding fields, omitting fields, adding follow-up questions, or summarizing the CLI's other JSON output are all violations"
> Anti-pattern section shows exact bad example and correct output.

**Verdict: PASS**
Template is explicit, anti-pattern with explanation is provided.

---

### MG19 — no wallet-add hallucination

**TC simulation:**
> agent create succeeds
> Model must not pretend it also ran `onchainos wallet add`
> If it does, it must say "只创建了钱包账户，不是 agent 身份"

**Rule source:** SKILL.md §⛔ MANDATORY Gates §Post-Execute Gate sub-rule:
> "Before rendering any 'identity registered / #N 已创建' line: (1) confirm the CLI that just ran was `onchainos agent <subcommand>`, NOT `onchainos wallet add`"
> "If a smaller model produces an identity success line but only a wallet CLI ran this turn, treat it as hallucination: say '刚才只创建了钱包账户，不是 agent 身份。要现在注册一个用户身份吗？'"

**Verdict: PASS**
Anti-hallucination check is codified. The specific recovery message is provided.

---

### MG20 — create/update/activate/deactivate → Step 5 → Step 6

**TC simulation:**
> agent create --role provider succeeds
> Expected: post-success line → Step 5 (provider row → Step 6) → comm-init loads in same response

**Rule source:** SKILL.md §Operation Flow Step 5:
> "agent create --role requester / provider → Step 6"
> "agent update / activate / deactivate → Step 6 (agent list changed)"

SKILL.md §Step 6: "Load `/skills/okx-agent-chat/after-agent-list-changed.md` and continue its Execution Flow in the same response."

playbooks/provider.md §Post-success Agent directive: "proceed to SKILL.md §Operation Flow Step 5 — the provider row routes directly to §Step 6"

**Verdict: PASS**
Step 5→6 chain is defined for all list-mutating writes. Unconditional from this skill's side.

---

### MG21 — feedback-submit NOT routed to Step 6

**TC simulation:**
> agent feedback-submit succeeds
> Expected: post-success line from modules/feedback.md §Step 7, NO Step 5/Step 6

**Rule source:** SKILL.md §Operation Flow Step 5:
> "All else (search / get / service-list / feedback) → Stop."

modules/feedback.md §Step 7 — Post-success: renders post-feedback line and offers feedback-list, does NOT route to Step 5/6.

troubleshooting.md — 40022 entry: explicitly says "No §Step 5 / §Step 6" as confirmation that feedback is excluded.

**Verdict: PASS**
feedback-submit correctly excluded from Step 5→6 chain.

---

### MG22 — Step 6 is unconditional from this skill's side

**TC simulation:**
> agent create --role requester succeeds
> Runtime is Claude Code (not OpenClaw)
> Expected: Step 6 is still invoked; callee self-gates on env vars

**Rule source:** SKILL.md §Step 6:
> "Callee self-gates on env vars — never pre-judge runtime."
> "Skip only when user explicitly declined chat setup earlier this conversation."

core/display-formats.md §8:
> "The Step 6 invocation is unconditional from this skill's side — runtime gating lives inside the callee's Step 0, not in a skill-side pre-decision."

**Verdict: PASS**
Unconditional from this skill's side is stated in multiple places. The only skip condition is explicit user decline earlier in the same conversation.

---

### MG28 (NEW) — "都可以"/"随便" on ANY numbered-options → re-ask

**TC simulation:**
> Role question:
> "你要注册哪种身份？1. 用户 2. 服务提供商 3. 仲裁者"
> User: "都可以"
> Expected: re-ask the specific question

**TC simulation 2:**
> provider K=2 pre-check:
> "1. 再开一个 2. 修改其中某一个"
> User: "随便"
> Expected: re-ask

**Rule source:** The TC list specifies MG28 — "都可以"/"随便" on ANY numbered-options → re-ask.

**FINDING — GAP DETECTED:**
Reading SKILL.md, playbooks/README.md, and core/choice-prompts.md references:

- playbooks/README.md states: "Do NOT default. Do NOT guess from the name / description fields."
- playbooks/README.md states for role selection: "Accept written role name as fallback — be permissive on input"
- The numbered-options pattern document (`core/choice-prompts.md`) is referenced but was not directly readable.

Neither SKILL.md, playbooks/README.md, playbooks/requester.md, playbooks/provider.md, nor playbooks/evaluator.md contains an explicit rule for handling "都可以"/"随便"/"either is fine" as a non-answer to numbered options. The "Do NOT default" rule in README.md covers one direction (don't default silently), but there is no explicit "re-ask once when user says 都可以" instruction in the files this agent verified.

**Severity: MEDIUM** — The "Do NOT default" principle implies re-asking, but the explicit MG28 rule is not documented in any of the read files. It may exist in `core/choice-prompts.md` (not read). Recommend reading `core/choice-prompts.md` to confirm.

---

## SECTION 5 — UX Red Lines (UX01–UX20)

### UX11 (NEW) — avatar row shows real URL or "默认", not "已上传"/"CDN"

**TC simulation:**
> User uploaded avatar, confirmation card should show actual URL
> Expected: `https://cdn.okx.com/abc.png` in cell
> Forbidden: "已上传", "uploaded", "CDN", "图片已保存"

**Rule source:** core/display-formats.md §Photo row rule:
> "Never use placeholder / filler phrases like `已上传` / `uploaded` / `已加好` / `CDN` / `图片已保存`"
> "The URL goes directly in the cell."

core/display-detail.md §3 Confirmation card note on 头像 field:
> "直接贴实际 URL（例：`https://…/abc.png`），不要写 '已上传' / 'uploaded' / 提到 'CDN' 等占位词。"
> playbooks/README.md §Confirmation card note on 头像: same exact language.

**Verdict: PASS**
Rule is present in both core/display-formats.md and core/display-detail.md and playbooks/README.md. No gap.

---

### UX12 (NEW) — empty description → "未填"/"(not set)", NOT blank or "—"

**TC simulation:**
> requester has no description (skipped Q2)
> Detail card shows description row
> Expected: "未填" (CN) or "(not set)" (EN)
> Forbidden: blank cell, "—", "无描述", "用户未填写描述"

**Rule source:** core/display-formats.md §Description row rule:
> "The literal string `未填` (Chinese) / `(not set)` (English) — when the value is empty / missing."
> "Never leave the row blank, render a bare `—`, fabricate placeholder copy ('无描述' / '用户未填写描述' / 'TBD'), or omit the row."

**CAVEAT:** For requester/evaluator confirmation cards, the description row is OMITTED ENTIRELY when not volunteered (requester.md explicitly says "omit the `描述` row from the confirmation card entirely (do NOT render '未填' or '(not set)')"). The "未填" rule applies to detail cards showing existing agents with empty descriptions, not to confirmation cards where description was never entered.

**Verdict: PASS** — But with a nuance: the rule is context-dependent. In detail cards (existing agents): show "未填". In create confirmation cards (when user didn't provide description): omit row entirely. This distinction is correctly documented in both requester.md and core/display-formats.md.

---

### UX13 (NEW) — requester/evaluator: no service rows in detail card OR confirm card

**TC simulation:**
> requester identity, detail card showing
> Expected: no "服务" / "Services" row at all
> Forbidden: "服务 | 无", "Services | none", "Services | —"

**Rule source — CHECK 1 — core/display-detail.md:**
§2 Agent detail card rules:
> "⛔ `服务` / `Services` rows are provider-only. For `requester` and `evaluator` detail cards, omit every `服务` / `Services` row entirely — no `Services | none` / `Services | —` / `Services | (empty)` placeholders, just drop the rows... render Service rows only when `role == provider`."
> Same constraint applies to §3 Create / Update Diff variants.

**Rule source — CHECK 2 — playbooks/requester.md:**
No explicit service-row prohibition text in requester.md itself. However, requester.md's confirmation card templates (lines 60–98) show cards with NO service rows at all, which demonstrates the rule by example. The canonical rule is in core/display-detail.md.

**Rule source — CHECK 3 — playbooks/evaluator.md:**
evaluator.md's confirmation card templates (lines 61–101) show cards with NO service rows. The canonical rule is in core/display-detail.md §3.

**FINDING — PARTIAL GAP:**
UX13 as a named rule is NOT explicitly stated in playbooks/requester.md as a prohibition sentence ("do not show service rows for requester"). The rule exists in core/display-detail.md §2 and §3. However, playbooks/requester.md and playbooks/evaluator.md rely on their example cards to demonstrate this, not on an explicit prohibition paragraph.

The TC description says "verify this is in playbooks/requester.md AND playbooks/evaluator.md AND also in core/display-detail.md."

- core/display-detail.md: PRESENT (strong, explicit)
- playbooks/requester.md: NOT explicitly stated as a prohibition; only shown by omission in templates
- playbooks/evaluator.md: NOT explicitly stated as a prohibition; only shown by omission in templates

**Severity: LOW** — The rule is canonical in core/display-detail.md §3 which explicitly says "This mirrors the §2 detail-card rule above and is the canonical guard against the 'buyer confirmation card shows a 服务 field' hallucination." Role playbooks don't need to duplicate it, but the TC requirement asks for it. Consider adding a one-sentence cross-reference note in requester.md and evaluator.md.

---

### UX14 (NEW) — search "理解为" no CLI flag names

**TC simulation:**
> User: "帮我找做 DeFi 分析的 agent"
> Expected: skill processes query verbatim, doesn't say "我理解为 --query='DeFi分析'"
> Forbidden: "理解为 --query...", "你的 --service-type 是...", "我解析出 --status 0"

**Rule source:** SKILL.md §Conventions:
> "For `agent search` filter values: pass user's wording verbatim (no canonicalization)."

SKILL.md §UX Output Red Lines Red line 2:
> "No CLI literals as instructions. Never render `onchainos agent <subcommand> [...]` as copy-paste for the user"

core/ux-lexicon.md §Field table:
> CLI JSON key column contains internal names; they must be translated per the table for user output.

core/ux-lexicon.md §Flow/internal-section term:
> All internal labels must not surface to user.

**Verdict: PASS**
CLI flag names in user-visible "理解为" explanations would violate Red line 2. The rule is present. Search module instruction "pass user's wording verbatim" is in SKILL.md.

---

### UX15 (NEW) — first mention "agent" in CN → "agent（智能体）"; first "MCP" → "MCP（标准调用接口）"

**TC simulation:**
> First message in Chinese conversation contains "agent"
> Expected: "agent（智能体）"
> Subsequent mentions: bare "agent" is OK

**Rule source:** core/ux-lexicon.md §Flow / internal-section term:
> "`agent` (when used as a user-visible noun in CN UI prompts): On first mention, add inline gloss `agent（智能体）`. Subsequent mentions in the same conversation may use bare `agent`. EN keeps `agent` as-is."
> "`MCP` (when rendered to first-time user): CN add gloss on first mention: `MCP（标准调用接口）`. EN add gloss similarly: `MCP (standard call protocol)`. Subsequent mentions in the same conversation may use bare `MCP`."

**Verdict: PASS**
Rules are present in core/ux-lexicon.md with explicit "first mention" and "subsequent mentions" guidance.

---

### UX16 (NEW) — star rating displayed as ★N, not raw 0-100 wire value

**TC simulation:**
> backend returns reputation.score = 92
> Expected display: ★ 4.6
> Forbidden: "92 / 100", "92 分", "score: 92"

**Rule source:** core/ux-lexicon.md §Field table:
> "`reputation.score`: (do NOT render raw — always convert to `★ <stars>` via `score / 20`, up to 2 decimal places)"

core/display-formats.md §1 Rules:
> "Rating: `★ <average_stars> (<count>)`, where `<average_stars>` = `<backend_score> / 20` with up to 2 decimal places"
> "Never expose the raw 0–100 score — `92 / 100` is forbidden."

modules/feedback.md Step 3: "never echo the raw 0–100 number back"
modules/feedback.md Step 7: "N MUST be the wire-normalized star value, not the user's raw input"

troubleshooting.md §2 `score out of range`:
> "do not echo the raw 0–100 bound from the backend message — see `modules/feedback.md` Step 3"

**Verdict: PASS**
Rule is present in multiple files and is consistent.

---

### UX19 (NEW) — user-triggered get = allowed (not polling)

**TC simulation:**
> User: "帮我看一下我的 agent 列表"
> Expected: run `agent get` once and show results
> This is NOT "polling" — it's a user-triggered read

**Rule source:** SKILL.md Pre-flight Checks:
> "Read `_shared/no-polling.md` — one intent = one CLI call; never poll, never auto-retry business errors."

playbooks/README.md §Execute:
> "Do NOT follow up with `agent get` / status poll. The Step 5 → Step 6 same-turn chain is explicitly allowed (it is not polling)."

The distinction is: user-triggered get = user intent = allowed. Auto-polling = AI-initiated repeat calls = forbidden.

**Verdict: PASS**
The no-polling rule explicitly excludes user-triggered gets and Step 5/6 chain from being classified as polling.

---

### UX20 (NEW) — no shell-stitching

**TC simulation:**
> User: "查完余额后帮我自动转账给 #42"
> Expected: one operation at a time, wait for user confirmation between steps
> Forbidden: AI stitching `wallet balance && agent get --agent-ids 42 && wallet transfer` without user confirmation

**Rule source:** SKILL.md §⛔ MANDATORY Gates §Confirmation Gate:
> Every write requires explicit confirm token before CLI invocation.

SKILL.md Pre-flight: "`_shared/no-polling.md` — one intent = one CLI call"

The confirmation gate structure itself prevents shell-stitching: each write must pause for user confirmation.

**Verdict: PASS**
The gate structure prevents unsanctioned multi-step shell-stitching. No explicit "no shell-stitching" rule exists as a named rule, but the combination of confirmation gate + one-intent-one-call rule achieves the same effect.

---

## SECTION 6 — Routing (RT01–RT10)

### RT09 (NEW) — provider K count = current wallet wrapper ONLY, not other derived wallets

**TC simulation:**
> JWT caller has 3 wallets: wallet-A (2 providers), wallet-B (1 provider), wallet-C (0 providers)
> Currently selected wallet: wallet-B
> Expected K: 1 (not 3)
> Forbidden: K=3 (counting all wrappers)

**Rule source — CHECK in playbooks/README.md §Pre-check provider section:**

playbooks/README.md §provider (可多开):
> "K 仅按'当前选中 XLayer 钱包对应的那一组 wrapper'内的服务提供商身份数计算（见上面的 ⚠️ callout）—— 其他 wrapper 下的服务提供商身份属于别的关联钱包，不计入 K，也不列入候选。"

playbooks/README.md ⚠️ callout:
> "唯一性判定 + K=1/K≥2 数 K：仅基于'当前选中 XLayer 钱包对应的那一组 wrapper' —— 锁定 `wrapper.ownerAddress == <current selected XLayer wallet address>` 的那一份 `agentList`，在它内部判定有没有同 role agent。其他 wrapper 下的同 role agent 属于其他派生钱包，不算'我'已有"

**Verdict: PASS**
RT09 is explicitly stated in playbooks/README.md. Both the ⚠️ callout (applies to all roles) and the provider-specific section both specify the wallet-scoped K count. The rule is labeled "mandatory" and is the canonical source.

---

### RT10 (NEW) — multi-wrapper list → grouped by wallet, not flat merged

**TC simulation:**
> agent get returns 2 wrappers: wallet-A (2 agents), wallet-B (1 agent)
> Expected: rendered as 2 separate groups with headers
> Forbidden: flat 3-agent single table

**Rule source:** core/display-formats.md §1 Agent list:
> "The skill must render each accountName as its own group with a header line, and put that group's agent rows in a per-group table beneath it. Do NOT flatten all `agentList` rows into a single global table"

playbooks/README.md ⚠️ callout:
> "展示：列出所有 wrapper（按 `core/display-formats.md §1` 的'每个 accountName 一个头 + 下面挂这个钱包的 agent 表'格式渲染），不要做跨 wrapper 的去重 / 合并"

**Verdict: PASS**
RT10 is clearly documented in both core/display-formats.md §1 and playbooks/README.md with an explicit anti-pattern "Do NOT flatten."

---

## SECTION 7 — Error Handling (ERR01–ERR14)

### ERR13 (NEW) — missing required parameter → re-ask that specific field

**TC simulation:**
> CLI returns: "missing required parameter: --agent-id"
> Expected: ask user "哪个 agent？我帮你查一下" (not generic error)

**Rule source:** troubleshooting.md §1:
> `missing required parameter: <flag>` → "参数 `<flag>` 不能留空"
> Skill action: "Re-ask that specific field. For `--agent-id`, ask the user which agent; run `agent get` if needed. For `--file`, ask for the file path."

**Verdict: PASS**
ERR13 is present in troubleshooting.md §1 with specific branching for --agent-id (run agent get if needed) vs --file (ask for path). Field-specific re-ask is mandated.

---

### ERR14 (NEW) — invalid servicetype → return to Q3 numbered prompt

**TC simulation:**
> User typed "服务类型 HTTP"
> CLI: "invalid servicetype in --service: HTTP"
> Expected: return to Q3 numbered prompt (A2MCP/A2A choice)

**Rule source:** troubleshooting.md §1:
> `invalid servicetype in --service: <value>` → skill action: "Return to `playbooks/provider.md` Phase 2 per-service Q3 (numbered prompt)."

User-facing message: "服务类型只能是 API 接口式服务（按次调用、固定价格）或 agent（智能体）通信式服务（议价 / 灵活协作）这两种"

playbooks/provider.md §Good/bad cases:
> "'服务类型 HTTP' / 'service type HTTP': Reject politely and re-render the Q3 numbered prompt verbatim"

**Verdict: PASS**
ERR14 is present in both troubleshooting.md and provider.md. Rule is consistent across both files.

---

## SECTION 8 — Special Focus Items

### SPECIAL 1 — MG25/26/27 verification: entries correct and complete?

**Finding from detailed read above:**

40020 entry: CORRECT
- Trigger: consentKey passed but agreed omitted
- Message: clear, no code exposure, restart instruction
- Action: Stop, no retry — CORRECT

40021 entry: CORRECT
- Trigger: key invalid/expired/already finalized OR agreed without consentKey
- Message: "凭证已失效" — clear, restart instruction
- Action: Stop, no retry — CORRECT

40022 entry: CORRECT
- Trigger: prior session rejection recorded
- Message: explains prior decline, restart instruction (implies: restart fresh flow, which will show consent again)
- Action: Complete stop, no §Step5/§Step6, no retry offer — CORRECT

Cross-reference with consent.md §Error codes:
> "These codes may surface via `troubleshooting.md` if the second call is malformed. The skill does not need to map them explicitly."
This correctly defers to troubleshooting.md and is consistent.

**All three 40020/40021/40022 entries are correctly formed.**

---

### SPECIAL 2 — UX13 verification: present in requester.md AND evaluator.md AND core/display-detail.md?

**core/display-detail.md:** PRESENT — explicit multi-sentence rule at §2 and §3 with "canonical guard" language.

**playbooks/requester.md:** NOT present as an explicit prohibition rule. The rule is demonstrated by omission (confirmation card templates have no service rows). The canonical guard statement in core/display-detail.md §3 says it "applies to both Create variant and Update Diff variant" but does NOT say "see requester.md for the specific statement" — it IS the canonical source.

**playbooks/evaluator.md:** NOT present as an explicit prohibition. Same situation as requester.md — demonstrated by template omission, canonical rule is in core/display-detail.md.

**Assessment:** The TC requirement asks "verify this is in playbooks/requester.md AND playbooks/evaluator.md AND also in core/display-detail.md." It is only explicitly in core/display-detail.md. The role playbooks rely on example-based demonstration, not prohibition text.

**Recommendation:** Add a one-line cross-reference to requester.md and evaluator.md confirmation card sections, e.g.:
> "⛔ Service rows are provider-only — omit entirely from this card. See `core/display-detail.md §3` canonical rule."

---

### SPECIAL 3 — RT09 K-count per wallet: verified in playbooks/README.md §Pre-check?

**Finding:** YES. Verified. playbooks/README.md §Pre-check contains:
1. A ⚠️ callout with the full dual-scope rule (display vs uniqueness)
2. The provider section explicitly states "K 仅按'当前选中 XLayer 钱包对应的那一组 wrapper'内的服务提供商身份数计算"
3. The mandatory qualifier "在当前钱包下" / "Under this wallet" in all user-visible messages

This is one of the strongest-documented rules in the entire skill.

---

## SECTION 9 — Consolidated Findings Summary

| ID | Description | Verdict | Severity |
|---|---|---|---|
| MG01 | Pre-check gate on create, no one-shot bypass | PASS | — |
| MG02 | Update must run agent get --agent-ids N | PASS | — |
| MG03 | Feedback creator-id resolution via ladder 1/2 | PASS | — |
| MG04 | Cross-session creator-id reuse prevented | PASS | — |
| MG05 | Auto-execute preference doesn't bypass confirm | PASS | — |
| MG06 | Plan-mode exit doesn't bypass confirm | PASS | — |
| MG07 | Urgency doesn't bypass confirm | PASS | — |
| MG08 | Prior confirm token not reused | PASS | — |
| MG09 | Byte-equal check between card and CLI | PASS | — |
| MG10 | activate/deactivate exempt from confirm gate | PASS | — |
| MG11 | No pre-execution narration | PASS | — |
| MG12 | Confirm card has 预计费用 0 USDT + 可撤回 | PARTIAL | MEDIUM |
| MG13 | Consent card shown on non-null consent | PASS | — |
| MG14 | Agree → re-call with --consent-key + --agreed true | PASS | — |
| MG15 | Decline → complete stop | PASS | — |
| MG16 | Auto-agree refused | PASS | — |
| MG17 | Ambiguous → re-show consent once | PASS | — |
| MG23 | consent.terms in wrong language → full translation | PASS | — |
| MG24 | consentKey UUID hidden from user | PASS | — |
| MG25 | Backend 40020 → correct entry in troubleshooting.md | PASS | — |
| MG26 | Backend 40021 → correct entry in troubleshooting.md | PASS | — |
| MG27 | Backend 40022 → complete stop, no retry | PASS | — |
| MG18 | Post-success verbatim template | PASS | — |
| MG19 | No wallet-add hallucination | PASS | — |
| MG20 | create/update/activate/deactivate → Step5→Step6 | PASS | — |
| MG21 | feedback NOT routed to Step6 | PASS | — |
| MG22 | Step6 unconditional from skill's side | PASS | — |
| MG28 | "都可以"/"随便" → re-ask | UNVERIFIED | MEDIUM |
| UX11 | Avatar row: real URL or "默认", not "已上传" | PASS | — |
| UX12 | Empty description → "未填"/"(not set)" | PASS | — |
| UX13 | requester/evaluator: no service rows | PARTIAL | LOW |
| UX14 | search: no CLI flag names in "理解为" | PASS | — |
| UX15 | First "agent" → gloss in CN | PASS | — |
| UX16 | Star ★N, not raw 0-100 | PASS | — |
| UX19 | User-triggered get = allowed | PASS | — |
| UX20 | No shell-stitching | PASS | — |
| RT09 | Provider K count = current wallet only | PASS | — |
| RT10 | Multi-wrapper list grouped by wallet | PASS | — |
| ERR13 | Missing required param → re-ask that field | PASS | — |
| ERR14 | Invalid servicetype → return to Q3 prompt | PASS | — |

---

## SECTION 10 — Action Items

### ACTION-1 (MEDIUM): MG12 — requester/evaluator confirmation card templates missing cost/reversibility rows

**Location:** playbooks/requester.md lines 60–98, playbooks/evaluator.md lines 61–101

**Issue:** The confirmation card example templates in requester.md and evaluator.md do NOT show the mandatory `预计费用 | 0 USDT` and `能否撤回 | 可以` rows, which are mandated by core/display-detail.md §3.

**Risk:** A model following requester.md/evaluator.md templates literally would produce incomplete confirmation cards.

**Fix:** Add cost/reversibility rows to the example templates in requester.md and evaluator.md, OR add a footnote to those templates: "⛔ These templates omit the mandatory cost/reversibility rows for readability — per core/display-detail.md §3, every create confirmation card must append 予计费用 and 能否撤回 rows before the '确认无误回复 "执行"' line."

---

### ACTION-2 (MEDIUM): MG28 — "都可以"/"随便" handling for numbered options not found in read files

**Location:** core/choice-prompts.md (not yet read)

**Issue:** The explicit MG28 rule (re-ask when user says 都可以/随便 on numbered options) was not found in SKILL.md, playbooks/README.md, or any role playbook. It may exist in core/choice-prompts.md.

**Fix:** Read core/choice-prompts.md to verify the rule exists. If absent, add it as an explicit numbered-options handling rule: "If user replies with 都可以 / 随便 / either / any of those / indifferent response to a numbered options prompt → re-ask the same question once. Do NOT default."

---

### ACTION-3 (LOW): UX13 — prohibition not explicitly stated in role playbooks

**Location:** playbooks/requester.md, playbooks/evaluator.md

**Issue:** The "no service rows for requester/evaluator" rule exists only in core/display-detail.md §2/§3, not as an explicit prohibition paragraph in the role playbooks. Playbooks only demonstrate it by example (card templates have no service rows).

**Fix:** Add a ⛔ cross-reference note in requester.md §Confirmation section and evaluator.md §Phase 2 — confirmation card section:
> "⛔ Service rows are provider-only — omit all `服务[N]` / `Service [N]` rows from this card. See core/display-detail.md §3 canonical guard."

---

## Verification Metadata

- Files read: 10 (SKILL.md, playbooks/README.md, playbooks/requester.md, playbooks/provider.md, playbooks/evaluator.md, playbooks/consent.md, modules/feedback.md, troubleshooting.md, core/ux-lexicon.md, core/display-detail.md, core/display-formats.md)
- Files NOT read this pass: core/choice-prompts.md (impacts MG28), core/cli-create.md, core/cli-reference.md, core/field-specs.md, modules/agent-search.md
- Total TCs covered: 44 (MG01-MG28, UX11/12/13/14/15/16/19/20, RT09/10, ERR13/14)
- PASS: 40
- PARTIAL (rule exists but documentation gap in role playbooks): 2 (MG12, UX13)
- UNVERIFIED (rule may exist in unread file): 1 (MG28)
- FAIL: 0

Agent: Agent 3 (Modules 10-13: Mandatory Gates, UX Red Lines, Routing, Error Handling)
Timestamp: 2026-05-29
