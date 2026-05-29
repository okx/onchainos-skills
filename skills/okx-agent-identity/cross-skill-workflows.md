# Cross-Skill Workflows

End-to-end user journeys that span multiple onchainos skills. The identity skill owns only the `agent …` steps below; siblings own the rest. Each workflow's "Data handoff" section summarizes how IDs propagate between steps — the **normative** same-turn handoff dispatcher (which file to load after which CLI call) is owned by `SKILL.md §Operation Flow Step 5` (which routes into `§Step 6` for comm-init or into staking for evaluator); this file defers to it on conflict.

## Workflow A: First-time buyer onboarding (includes passive fallback)

> User: "我想用 AI agent 做点事，从哪开始？" — OR — User goes straight to `okx-agent-task` and gets routed back.

```
1. okx-agentic-wallet   wallet login / create → XLayer address ready
       ↓ wallet logged in
2. okx-agent-identity   agent create --role requester → agentId
       ↓ agentId  (same-turn handoff — see SKILL.md §Operation Flow Step 5 → §Step 6)
2b. okx-agent-chat      after-agent-list-changed.md → OpenClaw agent list synced
                        (silent no-op if not in OpenClaw runtime)
       ↓
3. okx-agent-task       create-task → start publishing work

Passive fallback (user skipped step 2):
  okx-agent-task detects no requester → hands back with intent=need-requester
       ↓
  okx-agent-identity (passive onboarding: 3 turns — name → description → confirm) → agentId
       ↓ back to okx-agent-task — identity skill does NOT chain chat here (passive contract)
  okx-agent-task resumes create-task (and triggers chat setup itself when needed)
```

**Data handoff**: step 1 makes a wallet with a selected XLayer address; step 2's `agent create` automatically signs with that selected address (the CLI has no `--address` flag — it always uses the current wallet's XLayer address). `agentId` from step 2 is the requester identity used across `okx-agent-task`. Step 2b is the same-turn chat handoff defined in SKILL.md §Operation Flow Step 5 → §Step 6 — runs inside the same response as step 2, no user reply between. Passive fallback owns the `intent=need-requester` contract in `playbooks/requester.md §Passive Onboarding` and explicitly **skips** step 2b ("No other chatter" rule).

## Workflow B: Service provider onboarding

> User: "我想提供数据分析服务"

```
1. okx-agentic-wallet      wallet login → XLayer address ready
       ↓
2. okx-agent-identity      agent create --role provider (with services) → providerAgentId，默认直接 active
       ↓ providerAgentId  (same-turn handoff — see SKILL.md §Operation Flow Step 5 → §Step 6)
2b. okx-agent-chat         after-agent-list-changed.md → OpenClaw agent list synced
                           (silent no-op if not in OpenClaw runtime)
       ↓
3. okx-agent-task          wait for negotiate DM / accept task
```

> `agent activate` 只用于用户之前主动 `agent deactivate` 过、现在想重新上架的场景。新建的 provider 不需要显式 activate。

**Data handoff**: `providerAgentId` is reused on every `okx-agent-task` command; services in step 2 determine which tasks can match. Step 2b is the same-turn chat handoff defined in SKILL.md §Operation Flow Step 5 → §Step 6 — runs inside the same response as step 2.

## Workflow C: Evaluator onboarding

> User: "我想成为 evaluator 参与仲裁"

```
1. okx-agentic-wallet             wallet login → XLayer address ready
       ↓
2. okx-agent-identity             collect name + description → confirm → execute
                                  create --role evaluator → evaluatorAgentId
       ↓ (same turn — no user reply between 2 and 3)
3. okx-agent-task                 load references/evaluator-staking.md §2
                                  Step 1 → Step 2 in the same response
                                  → render stake confirmation inline
       ↓
4. okx-agent-task                 user confirms stake next turn → eligible for assignment
```

**Data handoff**: `evaluatorAgentId` is produced at step 2 and belongs to the user regardless of stake status. Step 2 → step 3 is a **same-turn handoff** routed by `SKILL.md §Operation Flow Step 5`: after create succeeds, render the two visible post-success lines (see `playbooks/evaluator.md §Post-success`) and then immediately load `okx-agent-task/references/evaluator-staking.md` §2 Step 1 → Step 2 inside the same response — do not stop between them. The identity skill never reads or verifies stake state and does not pass a stake amount. Do NOT gate step 2 on prior staking. **Staking-declined fallback** (per Step 5 evaluator row): if the user has explicitly declined staking earlier in the conversation, skip step 3 (the staking handoff) but **still proceed to `SKILL.md §Step 6` (comm-init) from identity before stopping** — the local agent list changed when `create` succeeded, so the OpenClaw cache still needs sync. Comm-init is owned by Step 6 with its own decline axis, separate from staking decline.

## Workflow D: Discover → rate

> User: "找个口碑好的做链上分析的 provider，用完给打个分"

```
1. okx-agent-identity   agent search (query + filters) → pick target agent (#42)
       ↓ targetAgentId
2. okx-agent-task       full task lifecycle (create → accept → deliver → complete)
       ↓ jobId (optional for --task-id)
3. okx-agent-identity   agent feedback-submit --agent-id 42 --creator-id <self> --score N
```

**Data handoff**: `creator-id` is the user's own agentId, resolved via `modules/feedback.md §Step 2`'s two-ladder rule — ladder 1 reuses an id already established in this conversation **only if** its `ownerAddress` was captured and matches the currently selected XLayer wallet (cached id with unknown / mismatched `ownerAddress`, or any prior `wallet switch` / `wallet add` since the id was cached → fall through to ladder 2); ladder 2 runs `agent get` and narrows the double-layer envelope to the **single wrapper** whose `list[*].ownerAddress == <currently selected XLayer wallet address>`, then enumerates candidates from **that wrapper's** `agentList[*]` only. `task-id` is the `jobId` from the completed task flow.
