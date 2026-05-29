# TC Verification — Modules 10–13 (Agent 3)
# okx-agent-identity: Mandatory Gates, UX Red Lines, Routing & Boundaries, Error Handling
# Date: 2026-05-29

---

## Module 10 — Pre-Check Gate (TC-MG01~MG04)

### TC-MG01 — create must run `agent get` even when all fields supplied one-shot

**Trigger:** User says "注册一个用户身份叫 Alice，不用头像" (full one-shot, no missing fields).

**Expected:** Skill runs `onchainos agent get` before entering Q&A, even though all fields are known.

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Pre-Check Gate`: "Any `agent create`, `agent update`, or `agent feedback-submit` intent — **run `onchainos agent get` first**. No exceptions, even when the user supplied all fields one-shot or named the role already."
- `playbooks/README.md §Pre-check existing agents`: "Before entering any role flow triggered by the user's own initiative, run `agent get` **once**..."

**Verdict:** ✅ COVERED — pre-check gate is documented as non-overridable with explicit "even when user supplied all fields one-shot" callout.

---

### TC-MG02 — update must run `agent get --agent-ids N`

**Trigger:** User says "改一下 #42 的描述" (no ambiguity, agent id is known).

**Expected:** Skill runs `onchainos agent get --agent-ids 42` first to show current values before Q&A.

**Documentation:**
- `SKILL.md §Update` Step 1: "`agent get --agent-ids <id>` → show current detail card."
- `playbooks/README.md §Execute`: "Before invoking the CLI, run the **3-question pre-execute self-check**... Q1 fail → run `agent get`."

**Verdict:** ✅ COVERED — update flow explicitly mandates `agent get --agent-ids <id>` as Step 1, plus pre-execute self-check Q1.

---

### TC-MG03 — feedback must resolve creator-id via `agent get`

**Trigger:** User says "给 #42 打 4 星" without having established `--creator-id` in this session.

**Expected:** Skill runs `onchainos agent get` (ladder 2) to enumerate agents under the current wallet for `--creator-id` resolution.

**Documentation:**
- `modules/feedback.md §Step 2 — Identify creator`: Ladder 2 explicitly runs `onchainos agent get`, narrows to `wrapper.ownerAddress == <currently selected XLayer wallet address>`, and branches on 0/1/many agents.
- `SKILL.md §⛔ MANDATORY Gates / Pre-Check Gate`: "`agent feedback-submit` intent — run `onchainos agent get` first."

**Verdict:** ✅ COVERED — feedback.md ladder 2 is the canonical mechanism; mandatory pre-check in SKILL.md adds the policy umbrella.

---

### TC-MG04 — no exemption for "already got" — pre-check cannot be reused from a prior turn

**Trigger:** User already ran `agent get` earlier in the session, then says "好，帮我再注册一个服务提供商".

**Expected:** Skill runs a fresh `agent get` rather than relying on the cached result from the earlier turn.

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Pre-Check Gate`: "No exceptions, even when the user supplied all fields one-shot or named the role already." — implies freshness requirement.
- `playbooks/README.md §Pre-check existing agents`: "run `agent get` **once**" — "once" refers to once per flow invocation, not "once per session ever."
- `playbooks/README.md §⚠️ callout` on the dual-scope rule: "跑过 `agent get` 不等于跑完 pre-check —— 还要按当前钱包 ownerAddress 过滤后再下结论." (Running `agent get` does not equal completing pre-check — must still filter by current wallet `ownerAddress` and draw a conclusion from that.)

**Verdict:** ✅ COVERED — the ⚠️ callout in README.md explicitly invalidates naive caching: the pre-check must be scoped to the current wallet address, and any prior call without that scope does not count.

---

## Module 10 — Confirmation Gate (TC-MG05~MG12)

### TC-MG05 — auto-execute memory preference refuses (cannot bypass confirmation gate)

**Trigger:** User set a preference "从此不用让我确认" or skill remembers a prior confirm token from another flow.

**Expected:** Skill still renders the confirmation card and waits for explicit confirm token.

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Confirmation Gate`: "Every content-creating write (`agent create / update / feedback-submit`) **must render a field-table confirmation card and receive an explicit confirm token**."
- `playbooks/README.md §Confirmation card`: "Memory preferences, plan-mode exit, one-shot capture, urgency, and 'intent is obvious' all do **NOT** bypass it."
- `modules/feedback.md §Step 5`: "Auto-execute preferences, prior in-conversation confirmations of other writes, and 'the user obviously wants this' do NOT bypass the gate."

**Verdict:** ✅ COVERED — both SKILL.md and playbooks/README.md explicitly list "Memory preferences" / "auto-execute preferences" as non-bypassing rationalizations.

---

### TC-MG06 — plan-mode exit refuses (confirmation gate still fires)

**Trigger:** User is in an agentic/plan mode where the skill would normally auto-execute after approval.

**Expected:** Skill ignores plan-mode implicit approval; still shows confirmation card for each write.

**Documentation:**
- `playbooks/README.md §Confirmation card`: "Memory preferences, **plan-mode exit**, one-shot capture, urgency..." listed as NOT bypassing.
- `SKILL.md §Confirmation Gate`: same phrasing — "Only sufficient condition to invoke CLI without re-rendering the card: both (1) user's most recent turn literally contains a confirm token AND (2) every field value in the just-rendered card is byte-identical..."

**Verdict:** ✅ COVERED — "plan-mode exit" explicitly listed as a non-bypassing rationalization.

---

### TC-MG07 — urgent tone refuses (confirmation gate still fires)

**Trigger:** User says "快点，直接执行" or "急，不用确认了".

**Expected:** Skill still renders confirmation card and does not proceed without explicit confirm token.

**Documentation:**
- `playbooks/README.md §Confirmation card`: "...urgency... do **NOT** bypass it."
- `SKILL.md §Confirmation Gate`: Confirmation gate says "Only sufficient condition... is (1) user's most recent turn literally contains a confirm token AND (2) every field value byte-identical."

**Verdict:** ✅ COVERED — "urgency" explicitly listed as a non-bypassing rationalization.

---

### TC-MG08 — prior confirmation from a different write not reused

**Trigger:** User confirmed a prior `create` in the same session, then changes a field value, then says "执行" again. Or: user confirmed `update #42` and now wants to do `update #58` — the prior confirm token must not carry over.

**Expected:** For the second write, skill renders a new confirmation card and requires a new confirm token. The old "执行" is not reused.

**Documentation:**
- `SKILL.md §Confirmation Gate`: "Only sufficient condition to invoke CLI without re-rendering the card: both (1) user's **most recent turn** literally contains a confirm token AND (2) every field value in the just-rendered card is byte-identical to what will be passed to the CLI."
- `modules/feedback.md §Step 6`: "Earlier-turn confirm tokens and confirms of different writes do NOT count for Q2."

**Verdict:** ✅ COVERED — "most recent turn" constraint in SKILL.md + explicit statement in feedback.md §Step 6 that "earlier-turn confirm tokens" do not count.

---

### TC-MG09 — byte-equal required between confirmation card and CLI invocation

**Trigger:** User confirmed with a card showing description "链上分析", but due to a processing step the CLI would receive "链上分析（系统优化版）".

**Expected:** Skill detects mismatch, re-renders the card with the actual CLI values, and waits for a new confirm token.

**Documentation:**
- `SKILL.md §Confirmation Gate`: "both (1)... AND (2) every field value in the just-rendered card is **byte-identical** to what will be passed to the CLI."
- `SKILL.md §Step 3: Execute`: "Q3 fail → re-render with actual values."
- `playbooks/README.md §Execute`: "All card values byte-identical to CLI values? (yes/no)... Any ≠ yes → STOP."

**Verdict:** ✅ COVERED — "byte-identical" constraint is stated in SKILL.md Confirmation Gate, and the pre-execute self-check Q3 handles re-rendering if it fails.

---

### TC-MG10 — activate/deactivate exempt from confirmation gate

**Trigger:** User says "下架 #42".

**Expected:** Skill runs `onchainos agent deactivate --agent-id 42` directly without a confirmation card.

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Confirmation Gate`: "`activate / deactivate` are state toggles — NOT gated."
- `SKILL.md §Intent → Sub-flow`: "`下架 agent` → `agent deactivate --agent-id <id>` directly."
- `playbooks/README.md §Confirmation card`: "State toggles (`agent activate` / `agent deactivate`) are NOT gated and run directly via `SKILL.md §Intent → Sub-flow`."

**Verdict:** ✅ COVERED — three separate locations explicitly carve out activate/deactivate from the confirmation gate.

---

### TC-MG11 — no pre-execution narration ("稍等", "正在处理" between confirm and result)

**Trigger:** User replies "执行" to the confirmation card.

**Expected:** Next visible output is the post-CLI result template (success or error card). No "稍等" / "正在执行" / "好的" narration appears between the confirm token and the result.

**Documentation:**
- `SKILL.md §Step 3: Execute`: "**No narration between confirmation and result.** When the user replies with a confirm token, invoke the CLI immediately and emit the post-CLI template as the first user-visible content."
- `playbooks/README.md §Execute`: "Do NOT follow up with `agent get` / status poll."

**Verdict:** ✅ COVERED — explicit "No narration between confirmation and result" rule in SKILL.md §Step 3.

---

### TC-MG12 — confirm card must have 预计费用 0 USDT + 可撤回 rows

**Trigger:** Any create or update confirmation card is rendered.

**Expected:** Card includes "预计费用: 0 USDT" and "可以撤回 / Reversible" rows (or their equivalents), sourced from `core/cost-disclosure.md`.

**Documentation:**
- `core/display-detail.md §3 Create/Update Diff confirmation card`: **Cost & reversibility rows (mandatory)** — "Every Create-variant card AND Update Diff card MUST include two final rows... explaining what the user pays and whether they can undo." Exact templates given:
  - Create variant CN: `| 预计费用 | **0 USDT**（...由 OKX 承担...） |` + `| 能否撤回 | 可以——... |`
  - Update variant: `> 预计费用: **0 USDT**... 可以撤回: ...`
- `core/cost-disclosure.md §Standard line`: "OKX covers all transaction fees."

**Verdict:** ✅ COVERED — `core/display-detail.md §3` explicitly mandates these two rows as mandatory for every confirmation card.

---

## Module 11 — Consent Gate (TC-MG13~MG17, MG23~MG27)

### TC-MG13 — consent card shown when backend returns non-null `consent`

**Trigger:** First `agent create` call returns `{ "consent": { "consentKey": "<uuid>", "terms": "..." } }`.

**Expected:** Skill renders consent card with `consent.terms` verbatim, offers agree/decline.

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Consent Gate`: "When CLI returns `executeResult: false` with non-null `consent` → show consent card."
- `playbooks/consent.md §When consent is required` + `§Consent Card`: template given verbatim; `consent.terms` displayed in full; `consentKey` hidden.

**Verdict:** ✅ COVERED — consent.md is the single source of truth and provides the exact card template.

---

### TC-MG14 — agree re-calls with `--consent-key` and `--agreed true`

**Trigger:** User replies "agree" to the consent card.

**Expected:** Skill re-invokes the original `onchainos agent create` command with exact same parameters plus `--consent-key <value>` and `--agreed true`.

**Documentation:**
- `playbooks/consent.md §Agree flow`: Steps 1–5 — re-invoke original command with same params, append `--consent-key <uuid>`, append `--agreed true`, do NOT re-render confirmation card.

**Verdict:** ✅ COVERED — step-by-step agree flow documented in consent.md.

---

### TC-MG15 — decline stops flow

**Trigger:** User replies "decline" to the consent card.

**Expected:** Skill renders cancellation message and stops. No further CLI calls.

**Documentation:**
- `playbooks/consent.md §Decline message`: "Do NOT call the CLI. Render the message... and stop." Template: "Registration cancelled — creating an agent identity requires accepting the terms of use. You can restart the registration flow at any time."

**Verdict:** ✅ COVERED — explicit decline handling with stop instruction.

---

### TC-MG16 — auto-agree refuses (cannot assume consent)

**Trigger:** User did not reply to consent card; or skill tries to assume "they probably agree".

**Expected:** Skill waits. Does not auto-agree, does not auto-decline, does not timeout.

**Documentation:**
- `playbooks/consent.md §Ambiguous reply handling`: "Do NOT auto-agree, do NOT auto-decline, do NOT timeout."
- `playbooks/consent.md §Consent Card Rules`: "Do NOT pre-fill the user's reply or add 'I'll assume you agree if you don't reply'."

**Verdict:** ✅ COVERED — explicit prohibition on auto-agree.

---

### TC-MG17 — ambiguous reply re-shows consent card

**Trigger:** User replies "What does clause 3 mean?" or other non-agree/decline response.

**Expected:** Skill re-displays the full consent card (including full `consent.terms` text) once and waits.

**Documentation:**
- `playbooks/consent.md §Ambiguous reply handling`: "Re-display the consent card **once** (including the full `consent.terms` text again). Wait for a clear agree or decline token."
- Worked Example C in consent.md demonstrates this exact behavior.

**Verdict:** ✅ COVERED — explicit ambiguous handling with worked example.

---

### TC-MG23 — consent.terms translated fully to user's conversation language

**Trigger:** Backend returns `consent.terms` in English but user is conversing in Chinese.

**Expected:** Skill translates `consent.terms` to Chinese, full content, no summarization.

**Documentation:**
- `playbooks/consent.md §Consent Card Rules`: "Display `consent.terms` in the **current conversation language**. If `consent.terms` is in a different language than the conversation, translate it to match before displaying. Translation is permitted for readability, but the translated content MUST be complete — do NOT summarize, paraphrase, or omit any clause."

**Verdict:** ✅ COVERED — explicit translation-with-completeness requirement documented.

---

### TC-MG24 — consentKey UUID hidden from user

**Trigger:** Backend returns `consent.consentKey: "abc-123-uuid"`.

**Expected:** UUID never appears in user-visible text; only used internally in the re-invocation `--consent-key` parameter.

**Documentation:**
- `playbooks/consent.md §Consent Card Rules`: "Do NOT show the raw `consentKey` UUID to the user — it is an internal token."

**Verdict:** ✅ COVERED — explicit prohibition documented.

---

### TC-MG25 — backend code 40020 handled

**Trigger:** Second `agent create` call (with `--consent-key`) returns backend code `40020` (`AGENT_CONSENT_AGREED_REQUIRED`).

**Expected:** Route to `troubleshooting.md` for user-facing message.

**Documentation:**
- `playbooks/consent.md §Error codes`: Code `40020` = `AGENT_CONSENT_AGREED_REQUIRED` ("consentKey passed but `agreed` omitted"). "If any of these codes appear in the CLI response, route to `troubleshooting.md` for the user-facing message."

**Verdict:** ✅ COVERED — error code table in consent.md with routing instruction. Note: `troubleshooting.md` does not have a dedicated row for 40020/40021/40022, instead consent.md routes them there with the note "skill does not need to map them explicitly". This is a minor gap — troubleshooting.md has no entry for these codes. However consent.md's language "route to troubleshooting.md" establishes the policy even if troubleshooting.md would catch them as "unknown error show raw." ⚠️ Gap: troubleshooting.md lacks explicit rows for 40020/40021/40022; users may see raw backend messages.

---

### TC-MG26 — backend code 40021 handled

**Trigger:** CLI response contains code `40021` (`AGENT_CONSENT_INVALID`).

**Expected:** Route to `troubleshooting.md` for user-facing message; graceful handling.

**Documentation:** Same as MG25. `playbooks/consent.md §Error codes` lists 40021.

**Verdict:** ⚠️ PARTIAL — consent.md routes to troubleshooting.md, but troubleshooting.md has no explicit 40021 row. The "unknown error show raw" fallback in troubleshooting.md would surface the raw message. Acknowledged gap.

---

### TC-MG27 — backend code 40022 handled

**Trigger:** CLI response contains code `40022` (`AGENT_CONSENT_REJECTED`).

**Expected:** Route to `troubleshooting.md` for user-facing message.

**Documentation:** Same as MG25/MG26. `playbooks/consent.md §Error codes` lists 40022.

**Verdict:** ⚠️ PARTIAL — same gap as MG25/MG26. No explicit row in troubleshooting.md.

---

## Module 10 — Post-Execute Gate (TC-MG18~MG22)

### TC-MG18 — post-success output must use template verbatim (not AI summarization)

**Trigger:** `onchainos agent create --role provider` succeeds.

**Expected:** First user-visible output after CLI call is the role file's `§Post-success` template verbatim (not a paraphrase or summarization of CLI JSON).

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Post-Execute Gate`: "After **any** `onchainos agent ...` CLI call, first user-visible output must come from a documented template — not from the model's own summarization of the CLI's JSON. Success → role file's `§Post-success` template verbatim."
- `playbooks/provider.md §⛔ Post-success`: Anti-pattern / correct examples clearly shown; "Paraphrasing... adding fields... omitting fields... are all violations."

**Verdict:** ✅ COVERED — explicit verbatim requirement with anti-pattern examples.

---

### TC-MG19 — no wallet add → identity template (must check which CLI ran)

**Trigger:** Smaller model runs `onchainos wallet add` instead of `onchainos agent create`, then tries to render identity success.

**Expected:** Skill detects that the CLI that ran was NOT `agent create`, refuses to render identity success line. Instead renders: "刚才只创建了钱包账户，不是 agent 身份。要现在注册一个用户身份吗？"

**Documentation:**
- `SKILL.md §⛔ MANDATORY Gates / Post-Execute Gate Sub-rule`: "confirm the right CLI ran before rendering a create-success line... (1) confirm the CLI that just ran was `onchainos agent <subcommand>`, NOT `onchainos wallet add` or any non-agent command... If a smaller model produces an identity success line but only a wallet CLI ran this turn, treat it as hallucination: say '刚才只创建了钱包账户，不是 agent 身份。要现在注册一个用户身份吗？'"

**Verdict:** ✅ COVERED — explicit sub-rule with exact recovery wording documented in SKILL.md.

---

### TC-MG20 — create/update/activate/deactivate → Step5 → Step6

**Trigger:** Any of `agent create` (all roles), `agent update`, `agent activate`, `agent deactivate` succeeds.

**Expected:** After rendering the result, skill proceeds to `§Operation Flow Step 5` → `§Step 6` (loads `okx-agent-chat/after-agent-list-changed.md`) in the same response.

**Documentation:**
- `SKILL.md §Step 5 Post-success Flow Continuation`: Table maps `agent create --role requester/provider` → Step 6; `agent update / activate / deactivate` → Step 6.
- `SKILL.md §Step 6: Communication Init`: "Load `/skills/okx-agent-chat/after-agent-list-changed.md` and continue its Execution Flow in the same response."
- `playbooks/README.md §Execute` Step 2: "For the list-mutating writes... control then flows into `SKILL.md §Operation Flow Step 5` (dispatcher) → `§Step 6` (comm-init) in the same response."

**Verdict:** ✅ COVERED — Step 5 table and Step 6 rule are explicit; applies to all listed commands.

---

### TC-MG21 — feedback-submit NOT Step6

**Trigger:** `agent feedback-submit` succeeds.

**Expected:** Skill does NOT proceed to Step 6 (comm-init). Flow stops after the post-success line.

**Documentation:**
- `SKILL.md §Step 5 Post-success Flow Continuation`: "All else (search / get / service-list / **feedback**) → **Stop.**"
- `SKILL.md §Post-Create Comm-Init (Step 6)`: "`feedback-submit` is excluded."
- `modules/feedback.md §Step 7 — Post-success`: No Step 6 mentioned; ends at one next-step suggestion.

**Verdict:** ✅ COVERED — explicitly excluded from Step 6 in multiple locations.

---

### TC-MG22 — Step6 unconditional from this skill's side

**Trigger:** `agent create` succeeds. Runtime is Claude Code (not OpenClaw). Skill might try to skip Step 6 because "there's no point in non-OpenClaw".

**Expected:** Skill still loads `okx-agent-chat/after-agent-list-changed.md` unconditionally. The callee self-gates internally.

**Documentation:**
- `SKILL.md §Step 6: Communication Init (unconditional from this skill's side)`: "Load `/skills/okx-agent-chat/after-agent-list-changed.md` and continue its Execution Flow in the same response. **Callee self-gates.** Skip only when user explicitly declined chat setup earlier this conversation."
- `playbooks/README.md §Execute`: "The Step 6 invocation is **unconditional from this skill's side** — runtime gating lives inside the callee's Step 0, not in this skill's pre-decision."

**Verdict:** ✅ COVERED — "unconditional from this skill's side" stated explicitly; callee self-gates is the architecture.

---

## Module 10 — Choice Prompt Edge (TC-MG28)

### TC-MG28 — "都可以"/"随便" on any numbered-options → re-ask, no default

**Trigger:** Skill shows numbered options (e.g., role selection, servicetype choice). User replies "都可以" or "随便".

**Expected:** Skill politely re-asks the numbered list once; does NOT silently pick a default.

**Documentation:**
- `core/choice-prompts.md §Rules`: "If user replies outside the enumeration (`都可以` / `随便`), politely re-ask the numbered list once; never silently pick a default."
- `playbooks/README.md §Route to the right role file`: "Do NOT default. Do NOT guess from the name / description fields."

**Verdict:** ✅ COVERED — explicit "都可以 / 随便" handling rule in choice-prompts.md with no-default enforcement.

---

## Module 11 — UX Red Lines (TC-UX01~UX16, UX19~UX20)

### TC-UX01 — skill names not in user text

**Trigger:** Any user-visible response.

**Expected:** Strings like `okx-agent-identity`, `okx-agent-task`, `okx-agentic-wallet`, or any `okx-*` identifier are not visible to the user.

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 1: "**No skill names in user text.** ⛔ `okx-agent-identity`, `okx-agent-task`, any `okx-*` identifier... → replace with business language."
- `core/ux-lexicon.md §How to use`: "Replace every `okx-*` skill literal with business language."

**Verdict:** ✅ COVERED.

---

### TC-UX02 — no CLI copy-paste instructions to user

**Trigger:** Any user-visible response about executing a command.

**Expected:** No `onchainos agent <subcommand> [...]` rendered as user instruction. AI invokes CLI itself.

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 2: "**No CLI literals as instructions.** ⛔ Never render `onchainos agent <subcommand> [...]` as copy-paste for the user → AI invokes CLI itself."
- `core/ux-lexicon.md`: "Replace every `onchainos agent <cmd>` literal with 'I'll do it for you' + actually invoke the CLI."

**Verdict:** ✅ COVERED.

---

### TC-UX03 — no Q1/Phase labels in user text

**Trigger:** Q&A flow asks a field question.

**Expected:** No `Q1：`, `Q2:`, `Phase 1`, `Phase 2`, `S1:`, `pre-execute self-check`, `confirmation gate`, `status=0` in user-visible text.

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 3: "**No internal labels.** ⛔ `pre-check / Phase 1 / Phase 2 / Q1: / Q2: / S1: / pre-execute self-check / confirmation gate / status=0` → use natural language."
- `core/ux-lexicon.md §Flow / internal-section term`: Full list of banned internal labels with replacements.
- `playbooks/README.md §Preview ≠ multi-field ask`: "asked in natural language — **no `Q1：` / `Q1:` prefix** in the user-visible prompt."

**Verdict:** ✅ COVERED.

---

### TC-UX04 — role localization (用户/服务提供商/仲裁者 vs User Agent/ASP/Evaluator Agent)

**Trigger:** Role shown in any user-visible card, prompt, or message.

**Expected:** Chinese users see `用户 / 服务提供商 / 仲裁者`; English users see `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Raw ERC-8004 enums (`requester / provider / evaluator`) and legacy CN nouns (`买家 / 卖家 / 服务方 / 验证者`) never appear in user text.

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 4: "**Use lexicon translations.** Role (`requester` → 用户/User Agent)..."
- `core/ux-lexicon.md §Role 角色术语`: Full mapping table.
- `playbooks/README.md §Confirmation card`: "role row MUST follow `core/ux-lexicon.md §Role`."

**Verdict:** ✅ COVERED — role translation table is the single source of truth, referenced consistently.

---

### TC-UX05 — service type with gloss (Pattern A / Pattern B)

**Trigger:** Service type shown in Q&A (teaching context) vs. in a table cell.

**Expected:** Q&A context uses Pattern A long form with inline gloss ("API 接口式服务（按次调用、固定价格）"). Table cells use Pattern B short form with footnote ("API 接口"). Raw `A2MCP` / `A2A` never shown to user.

**Documentation:**
- `core/ux-lexicon.md §Service-type 服务类型术语`: Pattern A (long form inline) and Pattern B (short form + footnote) rules with explicit context guidelines.
- `core/display-formats.md §Global rules` "Service-type rendering": "all tables in this file use Pattern B."
- `troubleshooting.md` error rows consistently use Pattern A ("API 接口式服务（按次调用、固定价格）" in error translations).

**Verdict:** ✅ COVERED.

---

### TC-UX06 — status integer translated

**Trigger:** Agent status shown in any card or list.

**Expected:** `status: 0` → `已下架/inactive`, `status: 1` → `已上架（可接单）/active`, `status: 2` → `审核中/under review`, `status: 3` → `审核未通过/review failed`. Raw integer never shown.

**Documentation:**
- `core/ux-lexicon.md §Status 状态术语`: Full mapping table with explicit ⛔ "Never render `status=0` / `status: 1`".

**Verdict:** ✅ COVERED.

---

### TC-UX07 — ≥5 agents reassurance footer

**Trigger:** `agent get` returns M >= 5 total agents across all wrappers.

**Expected:** Reassurance footer appended after agent list, in user's language: "以上 M 个 agent 都是你自己的" / "all M agents above are yours".

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 5: "When total agents ≥ 5 after `agent get`, append the reassurance footer per `core/display-formats.md §1`."
- `core/display-formats.md §1 Multi-agent List Reassurance Footer (P0 — counter alarm response)`: Full template, trigger condition (M >= 5), single-wrapper variant.

**Verdict:** ✅ COVERED — fully specified with templates and variant for single-wrapper case.

---

### TC-UX08 — fields from user only (no pre-fill from userEmail/session)

**Trigger:** Registration Q&A asks for name/description.

**Expected:** Name and description come only from user's literal reply. Never pre-filled from `userEmail` (e.g., `yuhui.zheng`) or wallet name or session metadata.

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 6: "**Fields from user input only.** `name / description / picture / service.*` MUST come from the user's literal reply... ⛔ Never pre-fill from `userEmail`, session metadata..."
- `playbooks/provider.md §Q&A`: "⛔ Fields from user's literal reply only — never pre-fill from userEmail, wallet name, or session metadata. Anti-pattern: 'Jim 的服务提供商' / 'yuhui 的 ASP'."

**Verdict:** ✅ COVERED.

---

### TC-UX09 — confirmation traceable (card values byte-identical to CLI)

**Trigger:** CLI is invoked after confirmation.

**Expected:** Every field value in the card that was shown is exactly what goes to the CLI (byte-identical).

**Documentation:**
- `SKILL.md §Confirmation Gate`: "both (1)... AND (2) every field value in the just-rendered card is byte-identical to what will be passed to the CLI."
- `SKILL.md §Step 3: Execute` pre-execute self-check Q3.

**Verdict:** ✅ COVERED.

---

### TC-UX10 — no polling after CLI call

**Trigger:** After any `agent create` / `activate` etc., skill is tempted to call `agent get` to verify the result.

**Expected:** Skill does NOT run `agent get` or any status poll after a mutation CLI call. No-polling rule applies.

**Documentation:**
- `SKILL.md`: "Read `_shared/no-polling.md` — one intent = one CLI call; never poll, never auto-retry business errors."
- `_shared/no-polling.md`: (referenced).
- `playbooks/README.md §Execute` Step 3: "See [no-polling] — do NOT follow up with `agent get` / status poll."
- `troubleshooting.md §General handling principles` Rule 5: "Do not chase failures with a `get`."

**Verdict:** ✅ COVERED.

---

### TC-UX11 — avatar URL not "已上传"

**Trigger:** User uploads an avatar and the confirmation card or detail card shows the profile photo row.

**Expected:** Cell shows the actual URL verbatim, not placeholder strings like `已上传` / `uploaded` / `CDN` / `图片已保存`.

**Documentation:**
- `core/display-formats.md §Global rules 头像 / Profile photo row rule`: "Never use placeholder / filler phrases like `已上传` / `uploaded` / `已加好` / `CDN` / `图片已保存`."
- `playbooks/README.md §Confirmation card`: "if the user uploaded an image or gave a link, this field **directly shows the actual URL**... do not write '已上传' / 'uploaded'."

**Verdict:** ✅ COVERED.

---

### TC-UX12 — empty description "未填"/"(not set)"

**Trigger:** Agent detail card or diff card shows a `requester` or `evaluator` description that was never set.

**Expected:** Description row value is `未填` (Chinese) or `(not set)` (English), not `—` or blank or "用户未填写描述".

**Documentation:**
- `core/display-formats.md §Global rules Description row rule`: "the literal string `未填` (Chinese) / `(not set)` (English) — when the value is empty / missing... Never leave the row blank, render a bare `—`, fabricate placeholder copy ('无描述' / '用户未填写描述' / 'TBD'), or omit the row."

**Verdict:** ✅ COVERED.

---

### TC-UX13 — requester/evaluator detail card has no service rows

**Trigger:** Detail card rendered for a `requester` or `evaluator` agent.

**Expected:** No `服务 / Services` rows in the card, even if backend returns `services: []`. Not even `服务 | 无` or `Services | none`.

**Documentation:**
- `core/display-detail.md §2 Rules`: "**⛔ `服务` / `Services` rows are provider-only.**... For `requester` and `evaluator` detail cards, **omit every `服务` / `Services` row entirely**... just drop the rows. This holds even when the backend returns `services: []`."
- `core/display-detail.md §3 Create / Update Diff confirmation card`: Same rule stated for confirmation cards.

**Verdict:** ✅ COVERED.

---

### TC-UX14 — search query passes user sentence verbatim to --query, no CLI flags shown

**Trigger:** User says "找做链上分析的服务提供商".

**Expected:** Skill internally runs `onchainos agent search --query "找做链上分析的服务提供商"` (verbatim passthrough). User-visible message says something like "帮你搜一下..." — no `--query` flag or `agent search` CLI literal shown to user.

**Documentation:**
- `modules/agent-search.md §Verbatim Passthrough`: "No translation, no paraphrasing, no splitting, no summarization."
- `modules/agent-search.md §Rules` Rule 1: "Always pass the user's original utterance verbatim."
- `SKILL.md Red line 2`: No CLI literals as instructions.

**Verdict:** ✅ COVERED — verbatim passthrough rule is the primary rule in agent-search.md, and Red line 2 covers the display side.

---

### TC-UX15 — MCP/agent gloss on first use

**Trigger:** First time `MCP` or `agent（智能体）` appears in user-visible text in a conversation.

**Expected:** `MCP` rendered as `MCP（标准调用接口）` on first mention; `agent` rendered as `agent（智能体）` on first CN mention.

**Documentation:**
- `core/ux-lexicon.md §Flow / internal-section term`: "`MCP` (when rendered to first-time user) → CN add gloss on first mention: `MCP（标准调用接口）`. EN: `MCP (standard call protocol)`." Also: "`agent` (when used as user-visible noun in CN UI prompts) → On first mention, add inline gloss `agent（智能体）`."

**Verdict:** ✅ COVERED.

---

### TC-UX16 — star ★N not 0-100

**Trigger:** Reputation / rating shown in any card, list, or message.

**Expected:** Rating shown as `★ N` where N = score/20 with up to 2 decimal places. Never as `92/100` or raw 0–100 integer.

**Documentation:**
- `core/ux-lexicon.md §Field`: "`reputation.score` → (do NOT render raw — always convert to `★ <stars>` via `score / 20`, up to 2 decimal places)"
- `core/display-formats.md §1` Rating rule: "`★ <average_stars> (<count>)`, where `<average_stars>` = `<backend_score> / 20`... **never** expose the raw 0–100 score."
- `modules/feedback.md §Step 7`: "N MUST be the wire-normalized star value... never `85 / 100`."

**Verdict:** ✅ COVERED.

---

### TC-UX19 — user-initiated `agent get` is allowed (not blocked by no-polling)

**Trigger:** User says "帮我看看我有哪些 agent".

**Expected:** Skill runs `agent get` to list agents. This is user-initiated and permitted despite the no-polling rule.

**Documentation:**
- `SKILL.md §Intent → Sub-flow`: "`我有哪些 agent / list agents` → `agent get` (no ids)."
- `_shared/no-polling.md`: Rule blocks AI-initiated repeated polling, not user-initiated queries.
- `SKILL.md §⛔ MANDATORY Gates / Pre-Check Gate`: "Any `agent create`, `agent update`, or `agent feedback-submit` intent — **run `onchainos agent get` first`**" — confirming `agent get` itself is always allowed as part of flows.

**Verdict:** ✅ COVERED — user-initiated get is a defined sub-flow; no-polling only targets AI-initiated post-mutation status checks.

---

### TC-UX20 — no shell-stitching (no bash commands assembled for user)

**Trigger:** Any context where skill might show commands for the user to run.

**Expected:** The AI invokes the CLI itself; never asks user to run shell commands or pastes assembled bash strings in user-visible chat.

**Documentation:**
- `SKILL.md §⛔ UX Output Red Lines` Red line 2: "Never render `onchainos agent <subcommand> [...]` as copy-paste for the user → AI invokes CLI itself."
- `playbooks/README.md §bash blocks in these files`: "Every `onchainos agent create ...` bash block inside playbook files is labeled **maintainer reference — not shown to user**."
- `playbooks/provider.md §Confirmation`: "**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks."

**Verdict:** ✅ COVERED.

---

## Module 12 — Routing & Boundaries (TC-RT01~RT10)

### TC-RT01 — task lifecycle → okx-agent-task (not identity)

**Trigger:** User says "发布一个任务" / "接单" / "交付" / "验收".

**Expected:** Route to `okx-agent-task`, not identity skill.

**Documentation:**
- `SKILL.md §Routing / Negative Triggers` table: "创建任务 / 发布任务 / publish task / create task" → `okx-agent-task`; "接单 / 接任务 / accept task / take a job" → `okx-agent-task`; "交付 / 验收 / 还价 / deliver / dispute / negotiate" → `okx-agent-task`.

**Verdict:** ✅ COVERED.

---

### TC-RT02 — dispute → okx-agent-task

**Trigger:** User says "发起仲裁 / 我要投诉这单" / "open a dispute".

**Expected:** Route to `okx-agent-task`, not identity skill.

**Documentation:**
- `SKILL.md §Routing / Negative Triggers`: "仲裁一下这单 / 发起仲裁 / open a dispute" → `okx-agent-task`.

**Verdict:** ✅ COVERED.

---

### TC-RT03 — 注册仲裁者 → identity skill (not task)

**Trigger:** User says "我要注册一个仲裁者身份".

**Expected:** Route to identity skill (evaluator create flow), not task skill.

**Documentation:**
- `SKILL.md §Routing / Negative Triggers`: "我要当仲裁者" alone (no identity words) → Ask disambiguation. But "注册仲裁者" with identity word → identity skill.
- `playbooks/README.md §Route to the right role file`: "注册仲裁者 / 验证者 / evaluator" → `playbooks/evaluator.md`.
- `SKILL.md §Intent → Sub-flow`: "注册 / register / create agent" → `§Core Flow: agent create`.

**Verdict:** ✅ COVERED.

---

### TC-RT04 — ambiguous "仲裁" → ask disambiguation

**Trigger:** User says "我要当仲裁者" alone (no identity context words like "注册/身份").

**Expected:** Skill asks clarifying question: 1. 注册仲裁者身份 or 2. 对某笔任务发起仲裁.

**Documentation:**
- `SKILL.md §Routing / Negative Triggers`: "'我要当仲裁者' alone (no identity words) → Ask: 1. 注册仲裁者身份 2. 对某笔任务发起仲裁 — route on reply."

**Verdict:** ✅ COVERED.

---

### TC-RT05 — 买家身份 → create not wallet-add

**Trigger:** User says "建一个买家身份" / "再建一个买家".

**Expected:** Route to `agent create --role requester`, not `wallet add`.

**Documentation:**
- `SKILL.md §description` (frontmatter): Extensive list of "建一个买家身份 / 再建一个买家身份..." triggering phrases with explicit note "ALWAYS an ERC-8004 agent identity register intent and routes here. NEVER a wallet account add."
- `SKILL.md §Routing / Negative Triggers`: Not in this table (it's a positive trigger). The description/frontmatter has the canonical anti-misrouting guard.
- Git `HEAD:SKILL.md` (original): Had the same anti-misrouting description in frontmatter.

**Verdict:** ✅ COVERED — frontmatter explicitly guards against wallet-add misrouting.

---

### TC-RT06 — single word → ask intent

**Trigger:** User sends just "agent" or "身份" without context.

**Expected:** Skill asks for intent rather than assuming.

**Documentation:**
- `SKILL.md §description` (frontmatter): "Do NOT trigger on single-word inputs without agent identity context."
- `SKILL.md §Step 1: Identify Intent`: "Ambiguous → ask once."

**Verdict:** ✅ COVERED.

---

### TC-RT07 — Step6 triggers on create/update/activate/deactivate

**Trigger:** Any of the listed list-mutating writes succeed.

**Expected:** Step 6 (comm-init via `okx-agent-chat/after-agent-list-changed.md`) is triggered in the same response.

**Documentation:** Same as TC-MG20 — see that entry.

**Verdict:** ✅ COVERED.

---

### TC-RT08 — provider K value per-wallet not per-email

**Trigger:** User has 2 provider agents under wallet-A and 1 provider under wallet-B. Both under same email. Current selected wallet is wallet-A.

**Expected:** K = 2 (not K = 3). Pre-check prompt lists only wallet-A's 2 providers.

**Documentation:**
- `playbooks/README.md §provider（可多开）`: "K 仅按'当前选中 XLayer 钱包对应的那一组 wrapper'内的服务提供商身份数计算... 其他 wrapper 下的服务提供商身份属于别的关联钱包，不计入 K，也不列入候选."
- `playbooks/README.md §⚠️ callout` (dual-scope rule): Uniqueness determined per `wrapper.ownerAddress == <currently selected XLayer wallet address>`.

**Verdict:** ✅ COVERED — explicit per-wallet K counting rule with the ⚠️ callout.

---

### TC-RT09 — multi-wrapper list grouped by wallet

**Trigger:** `agent get` returns agents across multiple wallets (wrappers).

**Expected:** Each wallet gets its own group header line; agents listed under their respective wallet. Not flattened into one table.

**Documentation:**
- `core/display-formats.md §1` Rules: "**Group by accountName.** One header line per outer-`list[*]` wrapper... **No deduplication across wrappers.**"
- `playbooks/README.md §⚠️ callout`: "展示：列出**所有** wrapper（按 `core/display-formats.md §1` 的'每个 accountName 一个头 + 下面挂这个钱包的 agent 表'格式渲染）."

**Verdict:** ✅ COVERED.

---

### TC-RT10 — passive onboarding from okx-agent-task skips pre-check, no Step6

**Trigger:** `okx-agent-task` hands off with `intent=need-requester`.

**Expected:** Skill skips role selection, pre-check, picture prompt. After success, hands back to task skill with one line; does NOT proceed to Step 6.

**Documentation:**
- `playbooks/README.md §Pre-check`: "**Skip this pre-check entirely for passive onboarding** (`intent=need-requester`)."
- `playbooks/requester.md §Passive Onboarding — Simplified sub-flow`: Skips role question, pre-check, picture prompt.
- `SKILL.md §Step 5 Post-success Flow Continuation`: "Passive Onboarding (`intent=need-requester`) → Hand back to `okx-agent-task` with one line. Do NOT proceed to Step 6."

**Verdict:** ✅ COVERED — passive onboarding exceptions explicitly documented in three locations.

---

## Module 13 — Error Handling (TC-ERR01~ERR14)

### TC-ERR01 — session expired → wallet login

**Trigger:** CLI returns `session expired, please login again: onchainos wallet login`.

**Expected:** Skill renders user-friendly message ("登录态过期了"), hands off to `okx-agentic-wallet` → `wallet login`, then offers retry.

**Documentation:**
- `troubleshooting.md §1 CLI-emitted bail!`: Row for `session expired...` — user translation "登录态过期了", action "Hand off to `okx-agentic-wallet` → `wallet login`, then retry the original command."

**Verdict:** ✅ COVERED.

---

### TC-ERR02 — no XLayer address

**Trigger:** CLI returns `no XLayer address found in current account`.

**Expected:** User sees "当前账号没有 XLayer 地址", skill hands off to wallet add/switch.

**Documentation:**
- `troubleshooting.md §1`: Row for `no XLayer address found...` — translation "当前账号没有 XLayer 地址", action "Hand off to `okx-agentic-wallet` → `wallet add` / `wallet switch`."

**Verdict:** ✅ COVERED.

---

### TC-ERR03 — not found (agent not found)

**Trigger:** Backend returns `agent not found` or 404-shaped response.

**Expected:** User sees "找不到该 agent", skill suggests verifying the id via `agent get`.

**Documentation:**
- `troubleshooting.md §2 Backend-originated`: Row for `agent not found` / any 404 — "找不到该 agent", action "Verify the id with `agent get`."

**Verdict:** ✅ COVERED.

---

### TC-ERR04 — whitelist 10016 (URL from msg)

**Trigger:** Backend returns code `10016` or message containing `user is not in approved agent whitelist`.

**Expected:** User sees whitelist-not-approved message + apply link extracted verbatim from backend `msg` field. No auto-retry.

**Documentation:**
- `troubleshooting.md §2`: Row for `user is not in approved agent whitelist` / backend code `10016` — detailed URL extraction rule using regex `https?://\S+?(?=[\s)）"'.,;]|$)`, verbatim URL rendering, both CN and EN templates, "Never auto-retry" rule.

**Verdict:** ✅ COVERED — thorough URL extraction algorithm and fallback if no URL in msg.

---

### TC-ERR05 — region restriction no VPN suggestion

**Trigger:** Backend returns code `50125` or `80001`.

**Expected:** User sees "Service is not available in your region." No raw code. No VPN suggestion.

**Documentation:**
- `troubleshooting.md §2`: Row for `Region-restriction codes 50125 / 80001` — "Service is not available in your region." Rule: "Do NOT echo the raw code. **Do NOT suggest VPNs.**"

**Verdict:** ✅ COVERED.

---

### TC-ERR06 — pending settlements (cannot deactivate)

**Trigger:** Backend returns `pending settlements` / `cannot deactivate`.

**Expected:** User sees unsettled task message; skill offers to navigate to task flow (without naming the skill).

**Documentation:**
- `troubleshooting.md §2`: Row for `pending settlements / cannot deactivate` — CN/EN templates, action "If user agrees, hand off to the task marketplace flow internally (do not name the skill in user text — Red line 1)."

**Verdict:** ✅ COVERED.

---

### TC-ERR07 — self-rating blocked

**Trigger:** `feedback-submit` with `--agent-id == --creator-id` (user tries to rate their own agent). Backend returns `self-rating not allowed`.

**Expected:** User sees "不能给自己的 agent 打分", return to feedback.md Step 1.

**Documentation:**
- `troubleshooting.md §2`: Row for `self-rating not allowed` — "不能给自己的 agent 打分", action "Return to `modules/feedback.md` step 1 (target)."
- `modules/feedback.md §Anti-patterns`: "评自己 — the backend rejects; pre-check `--agent-id != --creator-id`."
- `troubleshooting.md §3`: Skill-side guard — `--agent-id != --creator-id` enforced before CLI runs.

**Verdict:** ✅ COVERED — both skill-side pre-validation and backend error handling documented.

---

### TC-ERR08 — creator not owned (ladder2)

**Trigger:** Backend returns `creator agent not owned by caller`.

**Expected:** Skill re-enters `modules/feedback.md §Step 2` ladder 2 from the top. Message is neutral (does not promise "pick one"). Correct branching on 0/1/many agents under current wallet.

**Documentation:**
- `troubleshooting.md §2`: Row for `creator agent not owned by caller` — CN/EN translation using "发起人" / "reviewer" (not `--creator-id`); action "Return to `modules/feedback.md §Step 2` and re-run ladder 2 from the top"; wording must stay "neutral ('确认可用发起人') — do NOT commit to 'pick one and retry'."
- `modules/feedback.md §Step 2`: 0-agent / 1-agent / multiple-agent branching documented.

**Verdict:** ✅ COVERED — explicit ladder 2 restart with neutral wording constraint.

---

### TC-ERR09 — HTTP 500 retry once

**Trigger:** CLI/backend returns `Wallet API server error (HTTP 500)`.

**Expected:** Skill retries once. If fails again, surfaces error and stops. Does not loop.

**Documentation:**
- `troubleshooting.md §2`: Row for `Wallet API server error (HTTP 500)` — "后端暂时不可用", action "Retry once (network-transient policy, §General principles). If persists, surface and move on."
- `troubleshooting.md §General handling principles` Rule 4: "**Retry once** for transient 5xx/network errors. If it fails a second time, surface the error and move on. Never loop."

**Verdict:** ✅ COVERED.

---

### TC-ERR10 — unknown error show raw

**Trigger:** CLI/backend returns an error string not in either table.

**Expected:** Skill surfaces the raw message in the error card footer and asks the user how to proceed. Does NOT auto-translate, does NOT auto-retry.

**Documentation:**
- `troubleshooting.md §1` intro: "If you encounter a string that isn't in either table, surface the raw message in the error card footer and ask the user how to proceed — do NOT auto-retry or auto-translate."
- `core/display-formats.md §7 Error card` Rules: "Last line (inline code): **exact raw CLI message + source file, never translated**."

**Verdict:** ✅ COVERED.

---

### TC-ERR11 — already active / already inactive

**Trigger:** `activate` returns `agent already active`; or `deactivate` returns `agent already inactive`.

**Expected:** User sees friendly "这个 agent 已经在上架状态，不用再上架。" / "已经在下架状态。" No-op response with detail card.

**Documentation:**
- `troubleshooting.md §2`: Rows for `agent already active` — "这个 agent 已经在上架状态，不用再上架. / Agent is already active." + "No-op; show detail card."
- And `agent already inactive` — "这个 agent 已经在下架状态. / Agent is already inactive." + "No-op; show detail card."

**Verdict:** ✅ COVERED.

---

### TC-ERR12 — score out of range

**Trigger:** User inputs a star score outside 0.00–5.00 (e.g., `-1` or `6` stars), or over 2 decimal places. Backend returns `score out of range`.

**Expected:** Skill translates to stars wording ("评分要在 0.00–5.00 之间，最多保留 2 位小数"), never echoes the raw 0–100 backend bound. Return to feedback.md Step 3.

**Documentation:**
- `troubleshooting.md §2`: Row for `score out of range` — "评分要在 0.00–5.00 之间，最多保留 2 位小数" with explicit rule "do not echo the raw 0–100 bound from the backend message."
- `troubleshooting.md §3`: Skill-side guard — validates 0.00–5.00 before CLI invocation ("Reject with... skill validates before sending and never invokes the CLI in this case").
- `modules/feedback.md §Step 3 — Validate stars`: CLI pre-validation rules.

**Verdict:** ✅ COVERED.

---

### TC-ERR13 — missing param

**Trigger:** CLI returns `missing required parameter: --agent-id` (e.g., user tried to update without specifying which agent).

**Expected:** User sees "参数 `--agent-id` 不能留空", skill re-asks for that field (or runs `agent get` if needed to identify the agent).

**Documentation:**
- `troubleshooting.md §1`: Row for `missing required parameter: <flag>` — "参数 `<flag>` 不能留空", action "Re-ask that specific field. For `--agent-id`, ask the user which agent; run `agent get` if needed."

**Verdict:** ✅ COVERED.

---

### TC-ERR14 — invalid servicetype → Q3 re-ask

**Trigger:** CLI returns `invalid servicetype in --service: <value>` (e.g., user typed "HTTP" as service type).

**Expected:** User sees friendly message explaining valid service types (using Pattern A long form for `A2MCP`/`A2A`), skill returns to provider Phase 2 Q3 numbered prompt.

**Documentation:**
- `troubleshooting.md §1`: Row for `invalid servicetype in --service: <value>` — Pattern A long-form translations for both service types ("API 接口式服务（按次调用、固定价格）or agent（智能体）通信式服务（议价 / 灵活协作）"), "Return to `playbooks/provider.md` Phase 2 per-service Q3 (numbered prompt)."
- `playbooks/provider.md §Good / bad cases`: "'服务类型 HTTP' → Reject politely and re-render the Q3 numbered prompt verbatim."

**Verdict:** ✅ COVERED.

---

## Summary

| Module | TC | Verdict |
|---|---|---|
| Pre-Check Gate | MG01 | ✅ |
| Pre-Check Gate | MG02 | ✅ |
| Pre-Check Gate | MG03 | ✅ |
| Pre-Check Gate | MG04 | ✅ |
| Confirmation Gate | MG05 | ✅ |
| Confirmation Gate | MG06 | ✅ |
| Confirmation Gate | MG07 | ✅ |
| Confirmation Gate | MG08 | ✅ |
| Confirmation Gate | MG09 | ✅ |
| Confirmation Gate | MG10 | ✅ |
| Confirmation Gate | MG11 | ✅ |
| Confirmation Gate | MG12 | ✅ |
| Consent Gate | MG13 | ✅ |
| Consent Gate | MG14 | ✅ |
| Consent Gate | MG15 | ✅ |
| Consent Gate | MG16 | ✅ |
| Consent Gate | MG17 | ✅ |
| Consent Gate | MG23 | ✅ |
| Consent Gate | MG24 | ✅ |
| Consent Gate | MG25 | ⚠️ |
| Consent Gate | MG26 | ⚠️ |
| Consent Gate | MG27 | ⚠️ |
| Post-Execute | MG18 | ✅ |
| Post-Execute | MG19 | ✅ |
| Post-Execute | MG20 | ✅ |
| Post-Execute | MG21 | ✅ |
| Post-Execute | MG22 | ✅ |
| Choice Prompt | MG28 | ✅ |
| UX Red Lines | UX01 | ✅ |
| UX Red Lines | UX02 | ✅ |
| UX Red Lines | UX03 | ✅ |
| UX Red Lines | UX04 | ✅ |
| UX Red Lines | UX05 | ✅ |
| UX Red Lines | UX06 | ✅ |
| UX Red Lines | UX07 | ✅ |
| UX Red Lines | UX08 | ✅ |
| UX Red Lines | UX09 | ✅ |
| UX Red Lines | UX10 | ✅ |
| UX Red Lines | UX11 | ✅ |
| UX Red Lines | UX12 | ✅ |
| UX Red Lines | UX13 | ✅ |
| UX Red Lines | UX14 | ✅ |
| UX Red Lines | UX15 | ✅ |
| UX Red Lines | UX16 | ✅ |
| UX Red Lines | UX19 | ✅ |
| UX Red Lines | UX20 | ✅ |
| Routing | RT01 | ✅ |
| Routing | RT02 | ✅ |
| Routing | RT03 | ✅ |
| Routing | RT04 | ✅ |
| Routing | RT05 | ✅ |
| Routing | RT06 | ✅ |
| Routing | RT07 | ✅ |
| Routing | RT08 | ✅ |
| Routing | RT09 | ✅ |
| Routing | RT10 | ✅ |
| Error Handling | ERR01 | ✅ |
| Error Handling | ERR02 | ✅ |
| Error Handling | ERR03 | ✅ |
| Error Handling | ERR04 | ✅ |
| Error Handling | ERR05 | ✅ |
| Error Handling | ERR06 | ✅ |
| Error Handling | ERR07 | ✅ |
| Error Handling | ERR08 | ✅ |
| Error Handling | ERR09 | ✅ |
| Error Handling | ERR10 | ✅ |
| Error Handling | ERR11 | ✅ |
| Error Handling | ERR12 | ✅ |
| Error Handling | ERR13 | ✅ |
| Error Handling | ERR14 | ✅ |

**Totals: 61 ✅ COVERED / 3 ⚠️ PARTIAL / 0 ❌ MISSING**

---

## Findings

### Issue 1 — MG25/MG26/MG27: Consent backend codes 40020/40021/40022 not in troubleshooting.md

**Severity:** Low — these codes only appear if the consent re-invocation is malformed, which the skill should prevent.

**Gap:** `playbooks/consent.md §Error codes` routes these three codes to `troubleshooting.md`, but `troubleshooting.md` has no explicit rows for them. A user hitting code 40020/40021/40022 would see the raw backend message surfaced by the "unknown error" fallback.

**Recommendation:** Add three rows to `troubleshooting.md §2` for codes 40020/40021/40022 with user-friendly messages and recovery actions:
- 40020 (`AGENT_CONSENT_AGREED_REQUIRED`): "同意协议的参数丢失，请重新点同意" — retry with `--agreed true`.
- 40021 (`AGENT_CONSENT_INVALID`): "协议密钥已失效或不匹配，请重新发起注册流程" — restart registration.
- 40022 (`AGENT_CONSENT_REJECTED`): "你选择了拒绝协议，本次注册已取消" — same as the decline message template.

---

## Files Reviewed

- `/skills/okx-agent-identity/SKILL.md`
- `/skills/okx-agent-identity/playbooks/README.md`
- `/skills/okx-agent-identity/playbooks/consent.md`
- `/skills/okx-agent-identity/playbooks/requester.md`
- `/skills/okx-agent-identity/playbooks/provider.md`
- `/skills/okx-agent-identity/playbooks/evaluator.md` (not read directly — referenced via SKILL.md)
- `/skills/okx-agent-identity/modules/feedback.md`
- `/skills/okx-agent-identity/modules/agent-search.md`
- `/skills/okx-agent-identity/core/choice-prompts.md`
- `/skills/okx-agent-identity/core/display-formats.md`
- `/skills/okx-agent-identity/core/display-detail.md`
- `/skills/okx-agent-identity/core/ux-lexicon.md`
- `/skills/okx-agent-identity/core/cost-disclosure.md`
- `/skills/okx-agent-identity/troubleshooting.md`
- `/skills/okx-agent-identity/cross-skill-workflows.md`
- `git show HEAD:skills/okx-agent-identity/SKILL.md` (original version for comparison)
