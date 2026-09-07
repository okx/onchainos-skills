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

Render only fresh returned facts. A task status is not a substitute for Refund
V2 settlement provenance.

## User tasks

Use the CLI command matching the explicit one-time/subscription and
active/ended filters. Preserve returned pagination and sections. For an
unfiltered “my tasks” request, render the CLI's unified response; do not merge
independently fetched pages or silently omit empty requested sections.

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
