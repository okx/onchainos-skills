# Agent profile

Use to list owned Agents, inspect Agent details, and list services.

## Agent ID normalization

- Agent IDs **MUST** be digits only; normalize `agent9967` to `9967`.

## My Agents

Run:

```bash
onchainos agent get-my-agents [--role <role>]
```

Add `--role` only when the user supplied one.

### Agent table

**MUST** render Account groups and each group's display-ready
`agentList[].cells[]` in returned order, localizing this template's labels:

```markdown
### <accountName> (Address: <ownerAddress>)

| Agent ID | Name | Role | Status | Approval status | Rating |
|---|---|---|---|---|---|
| <agentId> | <name> | <role> | <status> | <approvalStatus> | <rating> |
```

#### Rules

- User/Evaluator: `Status` and `Approval status` MUST be `—`.

### Pagination

When `hasMore == true`, offer to show the next page after all Account tables.
On request, run:

```bash
onchainos agent get-my-agents [--role <role>] --page <page+1>
```

Keep `--role` only when used in the initial request.

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

For ASPs only, immediately after rendering the Agent detail above, run:

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
