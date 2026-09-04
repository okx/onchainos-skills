# Agent and Service output templates

Shared Agent/Service templates and display rules.

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

Apply these mappings to service create confirmations and update diffs:

- `serviceDescription`: render verbatim with all line breaks; never summarize, rewrite, or omit.
- `serviceType`: display raw `A2MCP` or `A2A` unchanged.
- `fee`: `"0"` → localized Free without suffix; positive `"N"` → `N USDT`; empty/inapplicable →
  `—`.
- `subscription` fee: `"0"` → localized Free without suffix; positive `"N"` → `N USDT / month`;
  absent/inapplicable → `—`.
- `freeTrial`: `"72"` → `3 days`; absent/inapplicable → `—`.
- `serviceGuide`: show non-blank text verbatim; omit when absent/blank.

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
