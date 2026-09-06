# A2A User Job

Owns existing one-time delegation progress, terms, attachments, and deliverables.
Creation from a confirmed A2A Service is owned by `create.md`. This file
excludes subscription receipt and billing operations.

## Intent routing

| Existing one-time task request | Action |
|---|---|
| Chain-state status | [Query status](#status). |
| Re-submit, re-upload, supplement, nudge, change terms, or ask about prior negotiation | [Forward to the task session](#forward-to-the-task-session). |
| Add an attachment or view deliverables | Read the selected section of [`actions.md`](actions.md). |

## Status

Require one explicit `jobId`. If the request does not identify one task, run
`onchainos agent active-tasks`, present numbered choices, and wait. Never select
a task from conversation recency.

Run `onchainos agent status <jobId> --agent-id <myAgentId>` with the Buyer's
identity and render the fresh result.

## Forward to the task session

Use this flow only when no matching pending decision card owns the user's
reply. Forward task-scoped free text to the sub-session that has the task's
current chain state and conversation history; do not adjudicate whether the
requested update is still allowed.

1. Run `onchainos agent active-tasks`. Match an explicit `jobId`; otherwise
   present its `shortJobId`, status, role, counterparty, and title as numbered
   choices through `user-notify`, then wait. Never select by conversation
   recency.
2. Read `myAgentId`, `counterpartyAgentId`, and `jobId` from the selected row.
   If `counterpartyAgentId` is absent, ask for it and stop.
3. Confirm the session exists:

   ```bash
   okx-a2a session query \
     --job-id <jobId> --my-agent-id <myAgentId> \
     --to-agent-id <counterpartyAgentId>
   ```

   An empty result means there is no active conversation; report that through
   `onchainos agent user-notify` and stop.
4. Send once and end the turn:

   ```bash
   okx-a2a session send --no-wait \
     --job-id <jobId> --to-agent-id <counterpartyAgentId> \
     --content "<user verbatim>

   ---
   Reply to the user via `onchainos agent user-notify --content \"<localized natural-language reply>\"`. If a user decision is needed, use `pending-decisions-v2 request` instead; follow `session.md` §Communication Boundary."
   ```

Preserve the user's instruction verbatim. Let the daemon resolve the session
from `--job-id` and `--to-agent-id`; never compose or pass `--session-key`.
Do not call `active-tasks` for general conversation, and do not send to the same
task session more than once in a turn. The `active-tasks` schema is documented
in [`../../shared/task-cli-reference.md`](../../shared/task-cli-reference.md#active-tasks).

## Returned action

Handle `nextAction.id=send_task_params_response` with the
[`accept.md` `NEED_PARAMS` flow](../provider/accept.md#need_params).
