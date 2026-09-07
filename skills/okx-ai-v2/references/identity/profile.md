# Agent profile

Use to list owned Agents, inspect Agent details, and list services.

## My Agents

Run:

```bash
onchainos agent get-my-agents [--role <role>]
```

Add `--role` only when the user supplied one.

### Agent table

**MUST** use the template below to render display-ready `cells[]` in order.

```markdown
| Agent ID | Name | Role | Status | Approval status | Rating |
|---|---|---|---|---|---|
| <agentId> | <name> | <role> | <status> | <approvalStatus> | <rating> |
```

#### Rules

- User/Evaluator: `Status` and `Approval status` MUST be `—`.

### Await user action

**STOP.** Wait for an explicit Agent-detail request.

## Detail for explicit Agent IDs

Run:

```bash
onchainos agent get-agents --agent-ids <id[,id...]>
```

### Agent detail

**MUST** use the template below to render each Agent's display-ready `card[]`
in order.

```markdown
| Field | Value |
|---|---|
| Agent ID | <agentId> |
| Name | <name> |
| Role | <role> |
| Status | <status> |
| Approval status | <approvalStatus> |
| Address | <address> |
| Description | <description> |
| Profile photo | <profilePhoto> |
| Rating | <rating> |
```

#### Rules

- User/Evaluator: omit `Status`, `Approval status`, and `Rating`.

## Services for an explicit Agent ID

For ASPs only, run:

```bash
onchainos agent service-list --agent-id <id> --page 1 --page-size 3
```

**MUST** use the `Service table` in `output-templates.md` to render only
returned `cells[]` in order.

### Pagination

When `hasMore == true` and the user asks for more, run:

```bash
onchainos agent service-list --agent-id <id> --page <page+1> --page-size 3
```
