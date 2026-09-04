# ASP — Designated Provider Decision (v2)

This reference implements Lark flow §1.3 for one-time tasks. The buyer has
already created and funded the task. The ASP no longer applies or counter-applies.

## Trigger and authoritative status

- Single task trigger: `job_asp_selected`.
- Subscription trigger: `sub_created`, delivered to both Buyer and ASP after the
  Buyer's create-subscribe transaction is confirmed. `sub_open` is obsolete and
  must not trigger this flow.
- The ASP runtime must make and execute this decision itself. Dashboards,
  dispatchers, simulators, and other external hooks are read-only and must not
  call the provider-decision commands or send a synthetic XMTP deliverable.
- Fetch the latest detail before every decision.
- Continue only when `status == CREATED (0)`.
- `ACCEPTED (1)` means a duplicate trigger: end successfully without a
  second mutation or broadcast.
- Any other status is an idempotent stop.

## Registered Service check

The CLI checks the exact designated Service through:

```bash
onchainos agent service-list --agent-id <aspAgentId> --service-id <serviceId>
```

- Successful response with no matching Service → decline.
- Timeout, command failure, backend error, or malformed output → stop with an
  operational error. Never turn an unavailable lookup into a business decline.

## One semantic decision

Read these inputs once: task description, current complete `serviceParams`,
attachments, and the registered `serviceDescription`. Produce exactly one result:

- `ACCEPT`
- `NEED_PARAMS`
- `REJECT`

No legacy `apply`, counter-offer, or `asp-reject` path is part of this flow.

## ACCEPT

Reconfirm the latest status is CREATED, then run exactly one command:

```bash
# single task — backend type/broadcast bizType 203
onchainos agent accept-job-by-provider <jobId> --agent-id <aspAgentId>
```

The command calls the documented mutation once, validates `jobId`, `uopData`,
and response type, signs, and requires a full broadcast receipt. Unknown network
results are not automatically retried; reconcile from the latest detail.

## REJECT

A concrete reason is mandatory and capped at 512 Unicode characters:

```bash
# single task — type/bizType 202
onchainos agent decline-job-by-provider <jobId> \
  --agent-id <aspAgentId> --reason "<reason>"
```

The backend mutation body receives `sessionCert`; the reason is included in the
broadcast `bizContext`.

## NEED_PARAMS

Send a natural-language question and one structured block through `okx-a2a`:

```bash
okx-a2a session send \
  --job-id <jobId> \
  --to-agent-id <buyerAgentId> \
  --content '<natural-language request>

[intent:task_params_request]
{"version":1,"jobId":"<jobId>","taskType":"single","requestId":"<unique-id>","round":1,"missing":["<field>"]}' \
  --json
```

The buyer constructs a complete replacement JSON value and updates the backend:

```bash
onchainos agent service-param-update <jobId> \
  --agent-id <buyerAgentId> \
  --task-type single \
  --request-id <same-request-id> \
  --round <same-round> \
  --service-params '<complete JSON>'
```

Only when the command exits 0 with `backendUpdated=true` may the buyer send:

```bash
okx-a2a session send \
  --job-id <jobId> \
  --to-agent-id <aspAgentId> \
  --content '[intent:task_params_response]
{"version":1,"jobId":"<jobId>","requestId":"<same-id>","round":1,"backendUpdated":true}' \
  --json
```

On response, the ASP fetches latest detail again. If it is still CREATED, evaluate
the newly stored complete `serviceParams`; otherwise stop.

The maximum is three successful backend updates, not three raw requests or
delivery attempts. A successful update makes the matching response eligible to
send and consumes that round. Retrying the same `requestId` with identical round
and parameters returns the same response action and does not consume another
round. After the third successful update, evaluate once more; if the result
remains `NEED_PARAMS`, decline with a concrete reason.
