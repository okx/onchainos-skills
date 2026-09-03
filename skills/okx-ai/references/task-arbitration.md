# Arbitration Queries

Use this flow when the user wants to view arbitration tasks or inspect one arbitration's current
status. It is read-only and supports User and ASP identities.

## 1. Select Identity

If the user explicitly supplied a User or ASP Agent ID, retain it. Otherwise run:

```text
onchainos agent my-agents
```

Keep only User (`role=1`) and ASP (`role=2`) identities. Display them and ask the user to choose which
identity will query arbitration data. Always wait for the choice, even when only one identity remains.

## 2. List Arbitrations

```text
onchainos agent arbitration-list --agent-id <selectedAgentId> [--page <n>] [--page-size <n>]
```

Render `data.list[]` as numbered cards containing `jobId`, `title`, `statusName`, and `createTime`.
If the list is empty, state that this identity has no arbitration tasks. Otherwise ask the user to
choose one task; do not infer a selection.

## 3. Show Arbitration Detail

Use the same selected identity:

```text
onchainos agent arbitration-detail <jobId> --agent-id <selectedAgentId>
```

Render the returned task and round status, current round, deadlines, and normalized `phase`:

- `evidence_preparation`: evidence is being prepared; show `prepareEndTime` when present.
- `arbitrating`: arbitration is in progress; show `roundEndTime` when present.
- `resolved`, `rejected`, or `invalidated`: show the returned status and any settlement fields that
  are actually present.
- `unknown`: show the raw status fields without guessing.

This flow never votes, uploads evidence, requests a refund, or starts another task action. Never
invent a verdict, deadline, fund direction, refund amount, or transaction hash when absent.
