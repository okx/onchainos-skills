# Task and Deliverable Queries

This leaf is read-only. It never starts watch, creates a decision, or mutates a
task.

## One task

Require an explicit `jobId`. When none is identified, run `active-tasks`, show
numbered choices with short Job ID, title, role, status, and counterparty, then
wait. Never select from conversation recency.

```bash
onchainos agent status <jobId> --agent-id <currentAgentId>
```

For a one-time task, render this exact card from fresh returned facts:

scene: One-time task details

display template:

```markdown
### One-time Job Details

| Job Name | Job ID | Service Provider | Fee | Status | Job Description |
|---|---|---|---|---|---|
| {title} | {jobId} | Agent ID {providerAgentId} | {Fee} | {status} | {description} |
```

display rules:

1. Display the complete `jobId`; never shorten it.
2. Render Fee as `{tokenAmount} {tokenSymbol}`. Render `Free` when the exact
   amount is zero.
3. Use only the status name rendered by the CLI.
4. Preserve the returned Job Description without rewriting it.

A task status is not a substitute for Refund V2 settlement provenance.

## User tasks

Use the CLI command matching the explicit one-time/subscription and
active/ended filters. Preserve returned pagination and sections. For an
unfiltered “my tasks” request, render the CLI's unified response; do not merge
independently fetched pages or silently omit empty requested sections.

For an explicit one-time list, run:

```bash
onchainos agent my-tasks --task-type one-time --status-type <0|1|2> \
  --page <page> --page-size <pageSize>
```

scene: One-time task list

display template:

```markdown
### One-time Jobs

| # | Job Name | Job ID | Service Provider | Fee | Status |
|---|---|---|---|---|---|
| {n} | {title} | {jobId} | Agent ID {providerAgentId} | {Fee} | {statusName} |
```

display rules:

1. Render only the current `oneTimeTasks.list` page in CLI order and number it
   from 1.
2. Display every `jobId` in full.
3. Render Fee as `{tokenAmount} {tokenSymbol}`. Render `Free` when the exact
   amount is zero.
4. Use only the CLI-normalized `statusName`.
5. Preserve the returned pagination; do not merge pages.

## ASP tasks

Resolve an explicit ASP identity or the bound current one. If multiple local
ASP identities remain, present them and wait.

```bash
onchainos agent tasks --agent-id <aspAgentId> --page 1 --limit 20
```

Render `jobId`, title, exact amount/token, and status in returned order.
Rejected-task candidates and filed arbitration cases are different datasets;
route those requests to `arbitration-query.md`.

## Saved deliverables

```bash
onchainos agent task-deliverable-list --job-id <jobId> --role <user|asp>
onchainos agent task-deliverable-list --role <user|asp> [--search <keyword>]
```

For a single job show original name, type, human-readable size, absolute path,
and saved time. For multiple jobs group by title and Job ID. Empty means no
saved deliverables. Never shorten an absolute path.

## Forward task-scoped free text

When no pending decision owns the reply and the User wants to supplement,
nudge, or discuss an existing task, enter `peer.md`. Querying alone must not
send a peer message.
