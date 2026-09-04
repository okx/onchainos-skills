# Agent, Service, and Review output templates

Shared Agent/Service/Review templates and display rules.

## Agent table

```markdown
| Agent ID | Name | Role | Status | Approval status | Rating |
|---|---|---|---|---|---|
| <agentId> | <name> | <role> | <status> | <approvalStatus> | <rating> |
```

### Rules

- Localize all table labels and mapped display values.
- User/Evaluator: `Status` and `Approval status` MUST be `—`.

## Agent detail

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

### Rules

- Localize labels and mapped display values.
- User/Evaluator: omit `Status`, `Approval status`, and `Rating`.

## Service value display

- Show missing values as `—`.
- Show `serviceDescription` verbatim and `serviceType` unchanged (`A2MCP` or `A2A`).
- For `fee` / `subscription`: `"0"` → localized Free; positive `"N"` → `N USDT` / `N USDT / month`, respectively. Map `freeTrial: "72"` to `3 days`.
- In create confirmations and update diffs, show non-blank `serviceGuide` verbatim; otherwise omit it.

## Service table

```markdown
| # | Name | Type | Fee | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|
| 1 | <serviceName> | <serviceType> | <fee> | <freeTrial> | <endpoint> | <serviceDescription> |
```

### Rules

- Apply [Service value display](#service-value-display).
- Number services sequentially across Agent tables.
- Merge `Fee` and `Subscription` as `Fee`.
- Omit a column only when all of its values are `—`; otherwise display it.
- Never display `serviceGuide` or internal Service UUIDs.

## Agent Service group

```markdown
### <asp.aspName> (Agent ID: <asp.aspAgentId>) | Rating <asp.rating> | Sold Count <asp.soldCount>
```

### Rules

- Follow the heading with the `Service table` and all its rules.

## Reputation list

```markdown
| Date | Reviewer | Task | Rating | Comment |
|---|---|---|---|---|
| <date> | <reviewer> | <task> | <rating> | <comment> |
```

### Rules

- Localize all table labels and mapped display values.
