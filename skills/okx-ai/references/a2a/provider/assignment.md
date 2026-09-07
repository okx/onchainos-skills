# ASP — Designated Provider Decision (v2)

This reference implements Lark flow §1.3 for one-time tasks. The buyer has
already created and funded the task. The ASP no longer applies or counter-applies.

## Trigger and authoritative status

- Single task trigger: `job_asp_selected`.
- Subscription trigger: `sub_open`, delivered to both Buyer and ASP after the
  Buyer's create-subscribe transaction is confirmed. The later acceptance event
  is role-specific: `sub_created` goes to the Buyer and `sub_asp_selected` goes
  to the ASP.
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

# subscription — backend type/broadcast bizType 205
onchainos agent accept-subscription <jobId> --agent-id <aspAgentId>
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

# subscription — type/bizType 206
onchainos agent decline-subscription <jobId> \
  --agent-id <aspAgentId> --reason "<reason>"
```

The backend mutation body receives `sessionCert`; the reason is included in the
broadcast `bizContext`.

## NEED_PARAMS

Enter [`../params.md`](../params.md). It is the sole owner of request IDs, rounds,
complete replacement parameters, backend-update confirmation, and the
three-successful-update limit.
