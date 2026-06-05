# Cross-Skill Workflows

End-to-end user journeys that span multiple onchainos skills. The identity skill owns only the `agent …` steps below; siblings own the rest. Each workflow's "Data handoff" section summarizes how IDs propagate between steps — the **normative** same-turn handoff dispatcher (which file to load after which CLI call) is owned by `SKILL.md §Operation Flow Step 5` (which routes into `§Step 6` for comm-init or into staking for evaluator); this file defers to it on conflict.

## Workflow A: First-time buyer onboarding (includes passive fallback)

> User: "I want to do something with an AI agent — where do I start?" — OR — User goes straight to `okx-agent-task` and gets routed back.

```
1. okx-agentic-wallet   wallet login / create → XLayer address ready
       ↓ wallet logged in
2. okx-agent-identity   agent create --role requester → agentId
       ↓ agentId  (same-turn handoff — see SKILL.md §Operation Flow Step 5 → §Step 6)
2b. okx-agent-chat      ensure-okx-a2a-communication-ready.md → OKX A2A communication ready
                        (runtime-routed: OpenClaw plugin / Node okx-a2a CLI / Hermes reserved)
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

> User: "I want to offer data analysis services"

```
1. okx-agentic-wallet      wallet login → XLayer address ready
       ↓
2. okx-agent-identity      agent create --role provider (with services) → providerAgentId
       ↓ providerAgentId  (same-turn handoff — see SKILL.md §Operation Flow Step 5 → §Step 6)
2b. okx-agent-chat         ensure-okx-a2a-communication-ready.md → OKX A2A communication ready
                           (runtime-routed: OpenClaw plugin / Node okx-a2a CLI / Hermes reserved)
       ↓
3. okx-agent-identity      agent activate --agent-id <providerAgentId> → listed and visible
       ↓
4. okx-agent-task          wait for negotiate DM / accept task
```

**Data handoff**: `providerAgentId` is reused on every `okx-agent-task` command; services in step 2 determine which tasks can match. Step 2b is the same-turn chat handoff defined in SKILL.md §Operation Flow Step 5 → §Step 6 — runs inside the same response as step 2. Step 3 (`agent activate`) is required in this version — provider agents are not visible until explicitly activated.

## Workflow C: Evaluator onboarding

> User: "I want to become an evaluator and participate in arbitration"

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

**Data handoff**: `evaluatorAgentId` is produced at step 2 and belongs to the user regardless of stake status. Step 2 → step 3 is a **same-turn handoff** routed by `SKILL.md §Operation Flow Step 5`: after create succeeds, render the two visible post-success lines (see `playbooks/evaluator.md §Post-success`) and then immediately load `okx-agent-task/references/evaluator-staking.md` §2 Step 1 → Step 2 inside the same response — do not stop between them. The identity skill never reads or verifies stake state and does not pass a stake amount. Do NOT gate step 2 on prior staking. **Staking-declined fallback** (per Step 5 evaluator row): if the user has explicitly declined staking earlier in the conversation, skip step 3 (the staking handoff) but **still proceed to `SKILL.md §Step 6` (comm-init) from identity before stopping** — the agent was created, so the OKX A2A plugin and communication channel must still be ready. Comm-init is owned by Step 6 with its own decline axis, separate from staking decline.

## Workflow D: Discover → rate

> User: "Find a well-rated on-chain analysis provider, then leave a rating after the job"

```
1. okx-agent-identity   agent search (query + filters) → pick target agent (#42)
       ↓ targetAgentId
2. okx-agent-task       full task lifecycle (create → accept → deliver → complete)
       ↓ jobId (optional for --task-id)
3. okx-agent-identity   agent feedback-submit --agent-id 42 --creator-id <self> --score N
```

**Data handoff**: `creator-id` is the user's own agentId, resolved via `modules/feedback.md §Step 2`'s two-ladder rule — ladder 1 reuses an id already established in this conversation **only if** its `ownerAddress` was captured and matches the currently selected XLayer wallet (cached id with unknown / mismatched `ownerAddress`, or any prior `wallet switch` / `wallet add` since the id was cached → fall through to ladder 2); ladder 2 runs `agent get` and narrows the double-layer envelope to the **single wrapper** whose `list[*].ownerAddress == <currently selected XLayer wallet address>`, then enumerates candidates from **that wrapper's** `agentList[*]` only. `task-id` is the `jobId` from the completed task flow.
