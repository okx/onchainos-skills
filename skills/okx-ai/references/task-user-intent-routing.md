# User Intent Routing

User-session needs to forward free-form user instructions targeting a specific task (e.g. "re-upload the dispute evidence for the cat-picture job", "remind ASP 963 that the deliverable is overdue", "switch to a different ASP") to the **specific sub session that owns that task**, when there's no matching active pending decision.

**Trigger phrases** — when the user says any of the following AND no matching entry exists in `pending-decisions-v2`, **MUST** enter this flow:

| Intent | Trigger phrases |
|---|---|
| Re-submit / supplement | "re-submit / re-upload / resubmit / add more / append / supplement evidence / change my X" |
| Nudge / request a sub-session update | "remind / nudge / chase up / tell the ASP X / tell the buyer X" |
| Change terms | "use a different provider / switch provider" |

🛑🛑🛑 **CRITICAL — do NOT make domain assumptions on behalf of the user**: when the queue is empty and the user issues a task-scoped instruction, your job is to **route**, not to **adjudicate**. **Do NOT** reply "the evidence phase is over" / "this state doesn't allow that". Only the sub session can query the chain and know for sure. Forward the user's verbatim wording and let the sub respond authoritatively. (🔴 I-15: the user requested "re-submit evidence," but the user session refused because it assumed the evidence phase had ended; the correct path was to route to the sub.)

**Decision tree** (apply in order, stop at first hit):

1. `onchainos agent active-tasks` → flat array of non-terminal tasks (with `myRole` / `counterpartyAgentId`).
2. `onchainos agent user-notify` a numbered list (`shortJobId` + status + role + counterparty + title) → end turn, wait for user's pick.
3. **Later turn after pick**: read `myAgentId` / `counterpartyAgentId` / `jobId` from the chosen row. If `counterpartyAgentId == null` → ask the user for it, else proceed.
4. `okx-a2a session query --job-id <jobId> --my-agent-id <myAgentId> --to-agent-id <counterpartyAgentId>` → confirms an active session exists. Empty → notify "no active conversation" via `onchainos agent user-notify` and end turn.
5. Dispatch the user's instruction to the sub via `okx-a2a session send` — the daemon resolves the session from `--job-id` + `--to-agent-id`:

   ```bash
   okx-a2a session send --no-wait \
     --job-id <jobId> --to-agent-id <counterpartyAgentId> \
     --content "<user verbatim>

   ---
   Reply to the user via `onchainos agent user-notify --content \"<localized natural-language reply>\"`. If a user decision is needed (A/B/C / approve / reject / etc.), use `pending-decisions-v2 request` instead (see `task-user-sub-playbook.md` §Communication Contract)."
   ```

   Forward verbatim then append reply-path instruction. End turn.

**Hard rules**:
- ❌ Do NOT pass `--session-key` you composed by string concatenation — always let the daemon resolve from `--job-id` + `--to-agent-id`.
- ❌ Do NOT call `active-tasks` proactively for general chitchat — only when task-scoped.
- ❌ Do NOT paraphrase / translate / reformat the user's instruction — pass verbatim.
- ❌ Do NOT call `okx-a2a session send` multiple times in one turn.

**Output schema of `active-tasks`**: see [`task-cli-reference.md → active-tasks`](task-cli-reference.md#active-tasks).

---

## Multi-task disambiguation

When the user has multiple active tasks, every routing decision **must** anchor to a specific `jobId`:

- **Always confirm `jobId` before acting**. If ambiguous → ask which task or render an `active-tasks` numbered list. Never assume the most-recent task is the one they mean.
- **Track each task's state independently**. Don't apply task A's context to task B.
- **Echo the `jobId` in every reply that touches a task** — `<title> (Job <shortId>)` is the standard prefix.

See [`entry-points.md`](./entry-points.md#multi-task-context-management) for the full deep-dive.

---

## Task list / "what am I working on"

When the user asks for **their tasks list without a specific jobId**, the user session answers directly (do NOT 6-step forward). Triggers: `my tasks` / `tasks I accepted` / `what am I working on` / `list my tasks` / `active tasks` / `show all my tasks` / `task list`.

**Action — pick the right CLI by intent**:
- **All non-terminal tasks across accounts**: `onchainos agent active-tasks` — use for "what am I working on" / "active tasks".
- **Tasks tied to a specific agent**: `onchainos agent tasks --agent-id <agentId> [--status <s>] [--page <n>] [--limit <m>]` — historical + active for that agent's role.

Render as numbered list. ❌ Do NOT 6-step forward. ❌ Do NOT mix with "decision list".

⚠️ **"all my tasks" / "show all tasks"** map to the caller's own tasks (→ this section). There is no public marketplace pool to browse.

---

## Close a task (irreversible)

Triggers (only when there's no active card the user might be answering): `close this task` / `cancel the task` / `drop this job` / `withdraw the task`.

**Preconditions**: clear jobId in context; status must be `created` (no provider accepted yet).

**Action**: `onchainos agent close <jobId> --agent-id <agentId>` after explicit user confirmation.

🛑 **CRITICAL ambiguity — `close` vs `resolve C`**:
- `close` is overloaded:
  1. **In "Waiting for user reply" state** on a `recommend_pick` card → run the block's pre-filled `resolve-prompt` command with the user's verbatim reply (CLI maps it to `close`).
  2. **Outside Waiting state** → `onchainos agent close <jobId>` directly.
- 🔴 I-9: case (1) mistakenly mis-routed. **Default when in doubt**: prefer `resolve-prompt`.

## Funding completed (after balanceWarning)

Trigger: `I topped up`, only with saved `balanceWarning`.
No saved warning → ask which payment/task; do not Watch.

Action:
- Saved pending create command → rerun it. If still insufficient, render `funding-notice` again and END TURN.
- Saved `jobId` only → Claude Code/Codex read `watch-core.md` and run scoped watch; Hermes/OpenClaw rely on native push and END TURN.

---

## Entry intents (start something new)

| Intent                                                                        | Action | Detail |
|-------------------------------------------------------------------------------|---|---|
| Publish task — `publish a task` / `create a task` / `use the service of Agent X` | Preserve the original utterance. Resolve `<X>` using the User Agent ID rules in [`task-user-actions-publish.md`](task-user-actions-publish.md) §1, then run `onchainos agent next-action --role user --agentId <X> --message '{"event":"create_task","jobId":"_"}'` and follow the script. When an ASP is specified, `task-service-select` receives the extracted `asp-agent-id`. | user publish flow |
| Take specific task (ASP) — `take {jobId}` / `contact the User Agent of {jobId}` | No proactive-accept path — ASPs are passive; designated tasks arrive via system events. Reply with passive-readiness guidance and STOP. | task-asp-accept.md §1 |
| Stake (Evaluator) — `I want to stake`                                         | `staking-config` + `my-stake` → confirm → `stake` (do NOT hardcode 100 OKB) | [`task-evaluator-staking.md §2`](task-evaluator-staking.md) |
| Direct help — "help me check…" **without** hiring intent                      | Route to appropriate skill; do NOT suggest task creation | — |

🛑 **ASP constraint**: `find tasks` / `start accepting jobs` / `take task {jobId}` → ASPs cannot discover or proactively accept tasks, with or without a `jobId`. Designated tasks arrive only through `JobAspSelected` system events. Reply with passive-readiness guidance and do not call `apply`.

---

## Status / progress query (specific task)

| Trigger | Action |
|---|---|
| **Chain-state snapshot** — `what's the status of {jobId}` / `query task {jobId}` | `onchainos agent status <jobId> --agent-id <myAgentId>` (pass the user's own `agentId` from envelope context). User session answers directly. |
| **Negotiation / chat-context detail** — `what did the seller say last time` / `what price did we agree on` | 6-step forward to sub (sub has chat history). |
| `view deliverables` | `task-deliverable-list [--job-id <jobId>] --role <user|asp>` |
| `upload evidence` / `supplement evidence` | **Friendly-reject** — evidence auto-submitted by CLI on `job_disputed`. |

---

## Replying to pending decisions (when `[USER_DECISION_REQUEST]` is in context)

If your context contains an active `[USER_DECISION_REQUEST]` block (you're in "Waiting for user reply" state from a recent push), the user's reply routes via the matching block's pre-filled `resolve-prompt` command:

- **Single active card** (latest block below the stale-notice line): run its `resolve-prompt` with `--user-reply "<user's verbatim text>"`.
- **Multiple blocks visible, user disambiguates with a jobId/label** (e.g. `Job 0x4652 select 1500`): scan context for the block whose `[job: <jobId>]` matches, then run THAT block's `resolve-prompt` with the user's verbatim text as `--user-reply`.
- **Truly ambiguous** (no jobId, no label hint, multiple cards): ask the user "which task?" via plain text reply.

---

## Decision list

Triggers (only when there's no active `[USER_DECISION_REQUEST]` block the user might be answering): `decision list` / `show decision list` / `list decisions` / `pending decisions` / `what's pending`.

**Action**: `onchainos agent pending-decisions-v2 list --format markdown` → **follow the CLI's returned playbook verbatim**. The playbook includes both the user-facing rendering instructions AND the routing rules for the user's subsequent reply. Do NOT improvise — only do what the playbook prints.

---

## Task watch / history / outstanding decisions

When the user wants to monitor task progress, drain unread/missed events, or list un-replied decision cards → route to **§Task Watch** (`watch-core.md`). Do NOT inline `okx-a2a user watch` / `okx-a2a user outdated-list` from here; load that file and follow it.

Triggers:
- **Live monitor**: `task watch` / `user watch` / `monitor task progress` / `watch tasks` / `keep me posted on tasks`
- **Subscription signal receipt**: `receive signals` / `start receiving signals` / `are you receiving signals` / `resume watching subscribed services` / `continue receiving signals` / `resume subscription` / `restore subscription`, plus semantically equivalent wording in any language and the prompted `listen to <subscription title>` form when the title resolves from the just-created or just-rendered buyer-subscription context. With an ACTIVE buyer subscription in current focus, a bare request to resume or restore that subscription means restore its receipt + scoped watch even when it omits “signals” or “watch”; it is not a backlog-first generic watch.
- **History / backlog drain**: `history` / `past messages` / `unread messages` / `show past messages` / `catch me up on tasks`
- **Outstanding (un-replied) decisions**: `outstanding decisions` / `unhandled decisions` / `unanswered decisions` / `what am I missing`

**Platform gate**: only Claude Code (`CLAUDECODE=1`) and Codex (`CODEX_THREAD_ID`); other platforms push natively and the skill stops with an unsupported-platform message.

🛑 Watch is itself a long-poll — the long-poll IS the wait. Do NOT wrap it in `/loop` / Cron / `sleep` / any scheduler.

⚠️ **Disambig — "decision list" vs "outstanding decisions"**: `decision list` → §Decision list above (`pending-decisions-v2 list`, the full queue). `outstanding decisions` → this section (`outdated-list`, only un-`check`ed `decision_request` items).

## Subscriptions (my subscriptions / detail)

| Trigger | Action |
|---|---|
| `my subscriptions` / `subscription list` / `what am I subscribed to` | `onchainos agent my-subscriptions --role buyer` → render per [`task-user-playbook.md` §My Subscriptions](task-user-playbook.md). User session answers directly (do NOT 6-step forward). |
| `subscription detail` / `show this subscription` | `onchainos agent subscribe-detail <jobId>` (id = the row's `jobId`) → render per [`task-user-playbook.md` §Subscription Detail](task-user-playbook.md). |
| `device list` / `list my logged-in devices` / `which devices are online` | `onchainos agent device-list` → render per [`task-user-playbook.md` §Device List](task-user-playbook.md). |
| `start receiving X on this device` | [`task-user-playbook.md` §Subscription management](task-user-playbook.md) — fresh-read; `deviceList:null` already means default-all, so report already receiving without a write; otherwise union → overwrite → re-read. |
| `start receiving Y on device X` / `also send Y to devices X and Z` | [`task-user-playbook.md` §Subscription management](task-user-playbook.md) — resolve device names→ids via `device-list` (never fabricate an unresolvable name); `deviceList:null` already includes every logged-in device (no write), otherwise UNION with the fresh-read list → overwrite → re-read → confirm the complete receiving set. |
| `stop pushing Y to device X` / `stop this device from receiving subscription Y` | [`task-user-playbook.md` §Subscription management](task-user-playbook.md) — fresh-read; for `null`, fetch the complete `device-list` and materialize all-minus-target before overwrite; for an explicit array, subtract normally; read back remaining. |
| `replay missed deliverables` / `discard offline deliverables` / `change how offline deliverables are handled` | [`task-user-playbook.md` §Subscription management → Change Offline-Deliverables Handling](task-user-playbook.md) — fresh-read `subscribe-detail` current `offlineReceiveFlag` → if already the target, say no change needed (do NOT re-write) → else `subscribe-offline-update --job-id <id> --flag <0/1>` → re-read to confirm. |
| `receive signals` / `start receiving signals` / `are you receiving signals` / `resume watching subscribed services` / `continue receiving signals` / `resume subscription` / `restore subscription`, plus semantically equivalent wording in any language and the prompted `listen to <subscription title>` form from a just-created/rendered buyer-subscription context | [`task-user-playbook.md` §Signal-receipt watch entry](task-user-playbook.md) — when an ACTIVE buyer subscription is in current focus, treat even a bare restore/resume-subscription request as receipt + watch; resolve one subscription, ensure this device can receive, run authorization precheck before reading backlog, then enter sticky scoped watch. Multiple ACTIVE subscriptions require a choice; none stops without global fallback. |
| `listen for task messages` / `watch task` with no specific task | [`task-user-playbook.md` §Listen entry](task-user-playbook.md) — confirm exactly one task ("Only one task can be watched at a time") → turn on this-device receipt → enter watch flow through the existing-subscription scoped-watch authorization gate. |

⚠️ Disambig — `my subscriptions` vs `my tasks`: subscriptions → `my-subscriptions`; tasks → `active-tasks` / `tasks` (§Task list). Do NOT mix. **Device routing is a subscription concept** — it governs A2A subscription-service message delivery only (never one-shot tasks), buyer side only. Do NOT route these to `task-asp.md` or any ASP/provider rendering.
