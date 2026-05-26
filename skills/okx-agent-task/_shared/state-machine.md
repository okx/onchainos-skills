# Task State Machine (Shared Blueprint)

> **The single source of truth** — aligned with `cli/src/commands/agent_commerce/task/common/state_machine.rs`. All role skill files reference this diagram.
>
> The state machine itself is payment-mode-agnostic — for payment details see [`payment-modes.md`](./payment-modes.md); for entry differences see [`entry-points.md`](./entry-points.md).
>
> **Important layering**: this system strictly distinguishes between **task status** (Status, 11 real enums) and **system events** (Event, 35 total). **Events are not states** — some events are transient (don't change status, e.g. `provider_applied` / `dispute_approved`), some trigger state transitions, and some are entirely decoupled from task status (e.g. staking events).

---

## 1. Task Status (11 real enums)

Backend `status` int field → local `Status` enum mapping (`state_machine.rs::Status::from_int`):

| int | string | enum | Meaning | Entry event |
|---|---|---|---|---|
| `-1` | `init` | `Status::Init` | Internal initialization state | — |
| `0` | `created` | `Status::Created` | Task on-chain, awaiting acceptance | `job_created` |
| `1` | `accepted` | `Status::Accepted` | Buyer confirmed acceptance (funds escrowed) | `job_accepted` |
| `2` | `submitted` | `Status::Submitted` | Provider deliverable on-chain | `job_submitted` |
| `3` | `refused` | `Status::Refused` | Buyer rejected deliverable; 24h decision window (dispute / agree-refund) | `job_refused` |
| `4` | `disputed` | `Status::Disputed` | Dispute in progress (evidence period + commit/reveal) | `job_disputed` |
| `5` | `admin_stopped` | `Status::AdminStopped` | Terminal: admin-stopped by the platform | — |
| `6` | `completed` | `Status::Completed` | Terminal: task completed (normal acceptance / dispute won by provider / review timeout auto-complete) | `job_completed` or `job_auto_completed` |
| `7` | `close` | `Status::Close` | Terminal: buyer proactively closed during `created` stage | `job_closed` |
| `8` | `expired` | `Status::Expired` | Terminal: open stage timeout, auto-closed by backend | `job_expired` |
| `9` | `rejected` | `Status::Rejected` | Terminal: funds refunded to buyer (agree-refund / dispute won by buyer / submit/refuse timeout auto-refund) | `job_refunded` or `job_auto_refunded` |

> ⚠️ **`Status::Rejected` (int 9) is the "refunded" terminal state** — backend naming is `REJECTED`, but in the task flow it means funds have been returned to the buyer. The Mermaid diagram below uses `refunded` as the friendly name for this state.
>
> ⚠️ **There is no `applied` status** — `provider_applied` is an event; when it fires, status is still `created`. Similarly when `dispute_approved` fires, status is still `refused` (dispute phase 1 approve). Events are just "what just happened" — they don't necessarily change status.

---

## 2. State Transition Diagram

```mermaid
stateDiagram-v2
    [*] --> created: buyer create-task<br/>(job_created)

    created --> created: provider apply<br/>(provider_applied — transient, no status change)
    created --> accepted: buyer confirm-accept<br/>(job_accepted)
    created --> close: buyer close<br/>(job_closed)
    created --> close: created stage timeout<br/>(job_expired)

    accepted --> submitted: provider deliver<br/>(job_submitted)
    accepted --> refunded: submit stage timeout<br/>(submit_expired → buyer claim-auto-refund → job_auto_refunded)

    submitted --> completed: buyer complete<br/>(job_completed)
    submitted --> refused: buyer reject<br/>(job_refused)
    submitted --> completed: review stage timeout<br/>(review_expired → provider claim-auto-complete → job_auto_completed)

    refused --> refused: provider dispute raise<br/>(dispute_approved — phase 1, transient, no status change)
    refused --> disputed: provider dispute confirm<br/>(job_disputed)
    refused --> refunded: provider agree-refund<br/>(job_refunded)
    refused --> refunded: refuse stage timeout<br/>(refuse_expired → buyer claim-auto-refund → job_auto_refunded)

    disputed --> disputed: vote-commit/reveal<br/>(sub state machine — see §4)
    disputed --> completed: dispute_resolved (provider wins)
    disputed --> refunded: dispute_resolved (buyer wins)

    completed --> [*]
    refunded --> [*]
    close --> [*]
```

---

## 3. Full Event Set (35 events, grouped by type)

For the full `event → --role` routing table see SKILL.md `## Activation`. The table below groups events by "how the event affects status".

### 3.1 Task lifecycle entry events (**change status**)

| event | Resulting status | Triggering action |
|---|---|---|
| `job_created` | `created` | Buyer create-task tx on chain |
| `job_accepted` | `accepted` | Buyer confirm-accept tx on chain |
| `job_submitted` | `submitted` | Provider deliver tx on chain |
| `job_refused` | `refused` | Buyer reject tx on chain |
| `job_disputed` | `disputed` | Provider dispute confirm tx on chain (dispute phase 2) |
| `job_completed` | `completed` | Buyer complete / dispute won by provider |
| `job_refunded` | `refunded` | Provider agree-refund / dispute won by buyer |
| `job_closed` | `close` | Buyer close tx on chain |
| `job_auto_completed` | `completed` | Provider claim-auto-complete tx on chain (after review timeout) |
| `job_auto_refunded` | `refunded` | Buyer claim-auto-refund tx on chain (after submit/refuse timeout) |
| `dispute_resolved` | `completed` or `refunded` (per verdict) | DisputeSettled on chain; agent should prioritize calling `agent status` to fetch the real status |

### 3.2 Task lifecycle transient events (**do not change status**)

| event | Status at trigger | Meaning |
|---|---|---|
| `provider_applied` | `created` | Provider apply tx receipt (escrow path, for provider's own consumption) |
| `dispute_approved` | `refused` | Dispute phase 1 approve tx receipt (for the initiating provider's own consumption — reminder to proceed to phase 2) |
| `submit_deadline_warn` | `accepted` | Escrow accept→submit nearing timeout reminder (5 min prior) |
| `review_deadline_warn` | `submitted` | Escrow submit→complete nearing timeout reminder (5 min prior) |
| `submit_expired` | `accepted` | Submit stage timeout (provider did not deliver); buyer should run `claim-auto-refund` |
| `refuse_expired` | `refused` | Refuse stage timeout (provider didn't decide within 24h); buyer should run `claim-auto-refund` |
| `review_expired` | `submitted` | Review stage timeout (buyer didn't accept within 24h); provider should run `claim-auto-complete` |
| `job_expired` | `created` | Open stage timeout; backend auto-transitions to `close` |
| `job_visibility_changed` | unchanged | TaskMarket.setVisibility tx receipt |
| `job_payment_mode_changed` | unchanged | TaskMarket.setPaymentMode tx receipt |

### 3.3 Dispute sub state-machine events (**during status=disputed**)

| event | Target | Meaning |
|---|---|---|
| `evaluator_selected` | The selected evaluator | VotersSelected on chain; CommitPhase has opened |
| `reveal_started` | Evaluators who have committed | RevealStarted on chain; entering the reveal window |
| `vote_committed` | The evaluator who issued the commit | commit tx receipt |
| `vote_revealed` | The evaluator who issued the reveal | reveal tx receipt |
| `round_failed` | Both sides + the round's evaluators | DisputeInvalidated on chain (insufficient votes / no reveals); wait for the next round |
| `slashed` | The slashed evaluator | VoterStaking.Slashed on chain (passive event, no user tx) |

### 3.4 Staking / reward events (**decoupled from task status**)

| event | Target | Meaning |
|---|---|---|
| `staked` | The evaluator who issued stake / increase-stake | **Both first-time stake (VoterStaking.Staked) and additional stake (VoterStaking.IncreaseStake) fire this event** — the real backend does not distinguish them; the agent can only tell first-time vs. additional by checking the `activeStake` delta via `my-stake` |
| `unstake_requested` | The evaluator who issued unstake | UnstakeRequested on chain; entered cooldown period |
| `unstake_claimed` | The evaluator who issued claim-unstake | Cooldown completed and funds withdrawn |
| `unstake_cancelled` | The evaluator who issued cancel-unstake | Cancelled within cooldown period |
| `stake_stopped` | The evaluator who issued stop-stake | VoterStakeStopped on chain; exited the voter pool |
| `cooldown_entered` | The evaluator who entered cooldown | DisputeManager.VoterCooldownEntered on chain (passive) |
| `reward_claimed` | The claimant (buyer / provider / evaluator) | claimRewards tx receipt |

### 3.5 Network/restart recovery events (transient, do not change status)

| event | Target | Meaning |
|---|---|---|
| `wakeup_notify` | The role-holder for that jobId (per-task fan-out) | After a network/computer restart, the backend notifies the agent to wake this task and resume the script. The envelope carries the real status directly via `message.jobStatus`; **the agent reads `jobStatus` to re-invoke next-action — do not use `wakeup_notify` as the `jobStatus`** — see SKILL.md `## Activation` "wakeup_notify special routing" |

---

## 4. Events Each Role Cares About (happy path)

For the full event → --role routing see SKILL.md `## Activation`; below is the happy-path summary per role:

- **Buyer**: `job_created` → negotiation → `job_accepted` (own confirmation) → `job_submitted` → `job_completed` / `job_refused` → (if refused) wait for provider's decision → `job_disputed` or `job_refunded`
- **Provider**: a2a-agent-chat inquiry → `provider_applied` (escrow) → `job_accepted` → `job_submitted` → `job_refused` / `job_completed` → (if refused) `dispute_approved` → `job_disputed` → `dispute_resolved`
- **Evaluator**: `evaluator_selected` → commit → `reveal_started` → reveal → `dispute_resolved` → `reward_claimed` or `slashed`

---

## 5. Timeout Rules

| Stage | Trigger condition | Timeout event | Subsequent action |
|---|---|---|---|
| `created` | Exceeds `openExpireSec` | `job_expired` | Backend auto-transitions to `close` |
| `accepted` | Exceeds `acceptedExpireSec` without submit | `submit_expired` | Buyer runs `claim-auto-refund` → `job_auto_refunded` |
| `submitted` | Exceeds review window (24h) without complete/reject | `review_expired` | Provider runs `claim-auto-complete` → `job_auto_completed` |
| `refused` | 24h with no provider decision (dispute / refund) | `refuse_expired` | Buyer runs `claim-auto-refund` → `job_auto_refunded` |
| `disputed` | commit / reveal windows respectively time out | (no task-level event; individual evaluators are slashed per `slashTimeoutBps`, ratio pulled from `staking-config`) | Dispute system proceeds with existing vote tallies / re-draws |

---

## 6. Query the Current State

Whenever unsure which state you're in, call:

```bash
onchainos agent common context <jobId> --role <buyer|provider|evaluator> --agent-id <agentId>
```

The response contains `[Current State]` (status string + description) and `[Currently Executable Actions]`, which can be cross-referenced with this diagram. **`common context` is a read-only API, does not change status, and may be called multiple times.**

---

## 7. Implementation Anchors

- Status enum: `cli/src/commands/agent_commerce/task/common/state_machine.rs::Status`
- Event enum (35 entries): same file, `Event` enum
- `status_when_event(event)` / `entry_event(status)`: bidirectional mapping functions; the agent does not need to replicate them — just call `agent next-action --jobStatus <event>` and let the CLI route
