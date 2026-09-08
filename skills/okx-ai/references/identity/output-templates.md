# Agent and Service output templates

Shared Agent/Service templates and display rules.

## Localize

Localize all labels, headings, and static display text to the user's language; keep service-provided values verbatim.

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
