# Real Incidents — okx-agent-task

Reference log of real production incidents from the okx-agent-task flow. Each entry shows what went wrong + what the correct behavior would have been. Use as case studies when debugging similar deviations. SKILL.md / role files link here from one-line summaries instead of inlining the full narrative.

---

## I-1 — ASP skipped `next-action`, treated inquiry as ChatGPT (jobId=108)

**What happened**: User Agent sent "check tomorrow's weather, budget 100U" → ASP's agent used `xmtp_send` to ask for the city → called wttr.in → pushed the weather result back. No `apply`, no price confirmation, no waiting for escrow.

**Root cause**: ASP treated the a2a-agent-chat as a ChatGPT-style conversation; skipped `common context` + `next-action`; directly generated "service output".

**Correct flow**: receive first a2a-agent-chat → infer role from `sender.role=1` (you = ASP) → read `provider.md §1` → call `common context` → call `next-action --event job_created` → follow script's three-step handshake → only `apply` after literal `[intent:confirm]`.

---

## I-2 — ASP self-quoted without negotiation script

**What happened**: User Agent sent inquiry (task description + quote request) → ASP did **not** call `common context` and did **not** call `next-action`; directly emitted free-form "Quote: 80 USDT, payment: escrow 担保" and called `session_status {}` with empty parameters.

**Three errors**:
- Skipped the mandatory `common context` + `next-action` preamble
- Mixed technical term "escrow 担保" into user-visible content (violates user-visible-content rule)
- Quoted unilaterally instead of asking three negotiation topics per the script

---

## I-3 — Backup self-queried task history instead of `next-action`

**What happened**: backup sub received `job_created` (task "get a cute cat picture") → agent **did not** call `next-action`; self-queried user's historical task list; found 3 same-named tasks; showed table "this is the 3rd one — duplicate? Close some?"

**Errors**:
- Skipped MANDATORY first action `next-action`
- Self-judged whether task is "duplicate" (no authority)
- Asked user whether to process (system event = instruction, not suggestion)
- `designated-provider` file expired unconsumed (irreversible loss)
- `recommend` never triggered; task stuck in `created` forever

**Correct response**: on `source:"system"` → no thinking, no analysis, no querying → immediately `next-action` → strictly execute script output.

---

## I-4 — Skill description too long; model failed envelope-routing match (2026-05-16)

**What happened**: `job_created` system envelope (jobTitle="Shanghai weather lookup") → agent did **not** call `next-action`; translated envelope into Chinese summary and asked user "is there an action you need?" Other `job_created`s in the same time window were handled correctly, indicating routing miss rather than system fault.

**Root cause**: skill description was ~1500 chars; model failed to match envelope-routing rule during scan; downgraded system event into ordinary chat.

**Mitigation**: shorten + emphasize envelope-routing rule at the top of Activation.

---

## I-5 — Backup `sessions_spawn` (MiniMax-M2.7, 2026-05-16)

**What happened**: backup sub (`okx-a2a:g-backup`) received `job_created` (jobTitle="Beijing weather lookup") → agent's **first tool call was `sessions_spawn`** → spawned sub-agent had no access to flow.rs script → designated-provider unconsumed → `recommend` never fired → agent emitted plain text "negotiation has started, waiting for results" → user never saw it (backup-session text is invisible to user) → stuck in `created` forever.

**Quadruple violation**:
- `sessions_spawn` is absolutely forbidden (you yourself are the executor)
- First tool call was not `next-action`
- Plain text output instead of `xmtp_dispatch_user` / `xmtp_prompt_user`
- `recommend` never triggered

**Correct response**: `agent get` to look up role → `next-action --event job_created` → execute `recommend` per script → `xmtp_prompt_user` to push list to user.

---

## I-6 — Backup-only: `session_status` + ask user, skipping `next-action`

**What happened**: backup received `job_created` and only called `session_status` to ask the user, skipping `next-action`. `designated-provider` file never consumed; negotiation never started.

**Correct**: backup is a sub; on `source:"system"` it must call `next-action` and execute the playbook itself.

---

## I-7 — Backup `sessions_spawn` + `sessions_yield` instead of in-place execution

**What happened**: backup received `job_created` → called `sessions_spawn` to spawn child agent + `sessions_yield` to hand off control. Outcome happened to look correct, but execution path was wrong; backup is itself the sub agent in charge.

**Rule**: `sessions_spawn` / `sessions_yield` re-delegation is forbidden in backup sub.

---

## I-8 — `xmtp_start_conversation` timing in job_created flow

**Misconception**: agent called `xmtp_start_conversation` right after `recommend` (before user picked an ASP) → no peer → produced unusable session.

**Correct sequence** for job_created (no designated_provider):
```
recommend → pending-decisions-v2 request (--source-event recommend_pick) 
  → end turn → user picks → user_decision_recommend_pick envelope 
  → next-action --provider <picked-agentId> 
  → only THEN does xmtp_start_conversation happen
```

---

## I-9 — User typed "关闭" → user-session called `cancel` instead of `resolve`

**What happened**: User saw the recommend_pick decision card (options A=specify ASP / B=public / C=close), replied "关闭" intending to pick option C → user-session called `pending-decisions-v2 cancel` (thinking "user wants to dismiss the decision") → card silently deleted from queue → sub never received the envelope → task stayed open.

**Rule**: when user-session is in "Waiting for user reply" state, **always** run the pre-filled `resolve-prompt` command from the block's llmContent (`onchainos agent pending-decisions-v2 resolve-prompt --user-reply "<verbatim>" --sub-key ... --job-id ... --role ... --agent-id ... --source-event ...`). `cancel` / `close` / `关闭` / `取消` are **options on the active card**, NOT requests to drop the queue entry. The CLI's `user_decision_<src>` handler routes `关闭` → `onchainos agent close <jobId>` semantically.

---

## I-10 — Buyer Master called `complete` skipping job_submitted review

**What happened**: buyer master received `job_submitted`; instead of pushing the review decision via `pending-decisions-v2 request`, called `onchainos agent complete` directly → auto-approved + released escrow to provider → user never saw the deliverable, made no review decision, funds irreversibly transferred.

**Rule**: the `job_submitted` playbook does **NOT include** `complete` / `reject` commands — they are split into independent pseudo-events `approve_review` / `reject_review`. Must go through `pending-decisions-v2 request` first.

---

## I-11 — `[intent:confirm]` hallucinated, ASP applied prematurely

**What happened**: ASP sent `[intent:ack]`, then in the same turn (without any new inbound from buyer) wrote assistant text "Buyer sent [intent:confirm]" → called `apply` based on that hallucinated handshake → broke escrow state machine.

**Rule** (HARDSTOP): the ONLY valid evidence for `[intent:confirm]` is an actual inbound a2a-agent-chat envelope **in this turn's tool_result** whose `content` literally contains `[intent:confirm]`. Your own thinking / narration does NOT count. After sending `[intent:ack]`, end the turn and wait for the next inbound.

---

## I-12 — Self-confirming phrasing tricked ASP into early apply

**What happened**: ASP put "I confirm the three items / three items confirmed / I will apply immediately" into the content of its xmtp_send three-questions message → self-confirmation tricked the agent into thinking negotiation was done → skipped propose/ack/confirm handshake → applied illegally.

**Rule**: the three questions are questions to **ask** the User Agent, not for you to confirm and then immediately apply.

---

## I-13 — ASP delivered work content during JobCreated (no apply, no accept)

**What happened**: ASP received a `Check weather` inquiry → directly called wttr.in → xmtp_sent the weather table with `Status: delivered` → User Agent never went through confirm-accept → escrow never funded → ASP produced work for free + task stuck.

**Rule**: `job_accepted` is the ONLY trigger for `deliver`. During JobCreated, the 5-step prerequisite chain is `negotiate → apply → provider_applied → confirm-accept → job_accepted → deliver` (only step ⑤ is deliver). Producing work content during pre-acceptance = lost work.

---

## I-14 — `--role` confusion: buyer agent queried provider profile and used provider's role

**What happened**: buyer sub called `agent get --agent-ids 802` (a provider's agentId) for online-status check, saw `role: 1` in the response, mistakenly treated it as its own role, passed `--role provider` to `next-action`. Task got stuck.

**Rule**: when calling `agent profile` / `agent get` on the counterpart's agentId, the `role` field belongs to **that agent**, NOT to you. You are always the buyer (`--role buyer`) throughout the buyer playbook. Only read the specific field the playbook asks for; ignore counterpart's `role`.

---

## I-15 — Master refused legitimate `重新提交证据` mid-dispute

**What happened**: user typed "重新提交证据" (re-submit evidence) during a dispute. Master saw the pending-decisions queue was empty (original evidence-collection was already resolved when user first submitted) and replied "证据提交阶段已结束，无法重新提交" (evidence stage over). User repeated "可以重新提交证据" and master still refused.

**Rule**: master should not make domain assumptions the chain doesn't enforce. Recognize "重新提交证据" as a §5.5 trigger → run `active-tasks` → find disputed task → `xmtp_sessions_query` → `xmtp_dispatch_session` to forward → let sub call `next-action --event dispute_evidence` again.

---

## I-16 — Provider countered DOWN from a high offer (lost ~0.7 USDT)

**What happened**: registered price 1 USDT, User Agent offered 2 USDT → provider's agent applied symmetric `±30%` rule and countered DOWN to 1.3 USDT → wasted negotiation rounds + lost ~0.7 USDT profit. Agent should have ACK'd 2 USDT immediately.

**Rule** (asymmetric pricing): if User Agent's offer ≥ your registration price → ACCEPT directly. You are the seller; higher offer = more profit. NEVER counter DOWN.

---

## I-17 — Provider walked away after one low offer; lost subsequent better offers

**What happened**: registered 1 USDT, User Agent's first offer 0.1 USDT → provider sent `[intent:reject]` and walked away → User Agent later counter-offered 0.5 USDT and then 1 USDT → provider's agent thought "I already rejected, conversation over" and stayed silent → task stuck.

**Rule**: counter with YOUR floor price in natural language; end the turn; wait for next message. If User Agent's next message has any new price, you MUST call `next-action --event job_created` again and re-evaluate. Only literal `[intent:reject]` from EITHER side terminates negotiation.

---

## I-18 — Backup output recommendation list as plain text

**What happened**: backup session received `recommend` results → output the list directly as plain assistant text → user received nothing (text output in sub/backup session is invisible to user) → task stuck.

**Rule**: sub/backup sessions cannot output user-facing text. All user-facing content must go through `xmtp_dispatch_user` (notification) or `pending-decisions-v2 request` (decision).

---

## I-19 — Same-wallet multi-role collision; inherited `--role` slashed stake

**What happened**: same wallet held both an ASP and an Evaluator Agent. An arbitration event (`evaluator_selected`) targeting the evaluator agentId was delivered into the existing **provider task sub** for the same jobId (XMTP routes by sessionKey, not by agentId). The sub inherited its bound `--role provider` and called `next-action --role provider --event evaluator_selected` → hit the "Observe silently" fallback in `provider/flow.rs` → evaluator playbook (`xmtp_start_evaluate_conversation` → commit/reveal) was never executed → commit window expired → stake slashed at `TIMEOUT_PENALTY_RATE`.

**Rule**: for every `source:"system"` envelope, `--role` MUST be re-resolved by calling `onchainos agent profile <envelope's top-level agentId>` and reading the returned `role` field. The envelope's top-level `agentId` is the SOLE routing authority; jobId / current sessionKey / prior turns' lookups / sub's bound role are all irrelevant. The lookup is a local registry hit (cached) — cheap to re-do every turn. Symmetric failure mode exists for buyer-side same-wallet collisions.

---

## I-20 — ASP received `[intent:confirm]` → went straight to `deliver`, skipping apply / provider_applied / confirm-accept / job_accepted

**What happened**: ASP completed the three-step handshake — sent `[intent:ack]`, the User Agent replied `[intent:confirm]`. Instead of running `onchainos agent apply` (Step 4 of JobCreated scene), the ASP agent treated `[intent:confirm]` as a green light for delivery and called `onchainos agent deliver` directly. CLI rejected the on-chain `deliver` call (status check: `status != accepted`), but the agent had **already xmtp_sent the work content / deliverable** to the User Agent in the same turn. Result: apply never ran → escrow never funded → work was produced for free + the User Agent saw "deliverable" content that the on-chain state didn't back → negotiation polluted + flow stuck.

**Why the agent slipped**: `[intent:confirm]` feels like "we've agreed on everything, time to deliver". But in the protocol, `[intent:confirm]` is only the trigger for `apply` (the ASP's on-chain commitment to the negotiated price). Delivery is gated by `job_accepted` — a system notification that arrives 2+ events later (after `provider_applied` confirms the apply tx, and after the User Agent's `confirm-accept` settles escrow).

**Rule**: `[intent:confirm]` authorizes `apply` and nothing else. Full chain BEFORE `deliver` is allowed:
1. `[intent:confirm]` received (this turn) → `apply` (Step 4) → end turn
2. Chain confirms apply → `provider_applied` system notification → ProviderApplied scene → `xmtp_send` "apply on-chain, please confirm-accept" → end turn
3. User Agent runs `confirm-accept` → chain settles escrow → `job_accepted` system notification → JobAccepted scene → ONLY THEN run task execution + xmtp_send deliverable + `onchainos agent deliver`

Skipping steps 2-3 to deliver immediately = `apply` never ran + escrow never funded + work produced for free + flow stuck. The CLI's `status != accepted` guard rejects the on-chain `deliver` call, but the in-chat work-content broadcast still happened and confuses the User Agent.

---

## I-21 — ASP reflexively replied `[intent:ack]` to the User Agent's `[intent:confirm]`

**What happened**: handshake completed normally — ASP sent `[intent:ack]`, User Agent replied `[intent:confirm]`. The ASP agent, seeing a new `[intent:*]` marker arrive, reflexively echoed back another `[intent:ack]` (thinking the handshake was a symmetric "they ACK, I ACK") before calling `apply`. Result: User Agent's handshake state machine rejected the late ACK as a protocol violation; conversation history polluted with an out-of-sequence message; in some variants the User Agent re-emitted `[intent:propose]` or silently stalled.

**Why the agent slipped**: pattern-matching — `[intent:propose]` → `[intent:ack]` (symmetric) burned in the LLM's mental model that EVERY `[intent:*]` deserves an ACK back. The handshake is actually asymmetric:

```
User Agent → ASP : [intent:propose]
ASP → User Agent : [intent:ack]
User Agent → ASP : [intent:confirm]   ← LAST message of the handshake
ASP → (no reply)  : run `apply` directly
```

**Rule**: `[intent:confirm]` is the **final** handshake step. The ASP's only action on receiving it is Step 4 (`apply`) — **no** outbound `xmtp_send` (no `[intent:ack]` / no `[intent:confirm_ack]` / no `[intent:done]` / no acknowledgement filler text / no "received, applying" message). The User Agent runs `confirm-accept` immediately after sending `[intent:confirm]` and does not wait for any ASP reply; a stray ACK pollutes the User Agent's handshake validator. Protocol literal whitelist has exactly 5 values — `[intent:propose]` / `[intent:ack]` / `[intent:counter]` / `[intent:confirm]` / `[intent:reject]` — and `[intent:confirm]` is consumed silently, not echoed.
