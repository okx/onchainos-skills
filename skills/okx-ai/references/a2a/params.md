# Task Parameter Clarification

This leaf owns the designated-provider `NEED_PARAMS` exchange for single tasks
only. It is a collaboration subflow inside Created state, not a new task status.
Subscriptions must never enter this leaf or call `service-param-update`; the ASP
must decline a subscription with a concrete reason when required input is absent.
If an older CLI playbook routes a subscription here, treat that route as stale:
do not send `task_params_request`, do not update the backend, and return to
[`provider/assignment.md`](provider/assignment.md) for `ACCEPT` or `REJECT`.

## ASP request

Send one natural-language question plus the exact structured block:

```text
[intent:task_params_request]
{"version":1,"jobId":"<jobId>","taskType":"single","requestId":"<unique-id>","round":<1..3>,"missing":["<field>"]}
```

Use `okx-a2a session send` once. Keep `requestId`, `round`, `jobId`, and the
missing-field names unchanged.

## User update

Construct one complete replacement `serviceParams` JSON value and run:

```bash
onchainos agent service-param-update <jobId> \
  --agent-id <buyerAgentId> \
  --task-type single \
  --request-id <same-request-id> \
  --round <same-round> \
  --service-params '<complete JSON>'
```

Only exit 0 with `backendUpdated=true`, or the explicit duplicate-confirmed
result, authorizes the returned `send_task_params_response` action. Send its
returned params unchanged through the existing task session. Unknown or failed
updates never produce a success response.

## ASP continuation

Fetch fresh task detail again. Continue only while status is Created and
evaluate the complete newly stored `serviceParams`. At most three successful
backend updates are allowed; replaying an identical request does not consume a
round. After the third successful update, evaluate once more and decline with a
concrete reason if required inputs remain missing.
