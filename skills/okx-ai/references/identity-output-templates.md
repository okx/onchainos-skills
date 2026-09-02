# Agent and Service output templates

Shared Agent/Service templates and display rules.

## Agent table

```markdown
| Agent ID | Name | Role | Status | Approval status | Rating |
|---|---|---|---|---|---|
| <agentId> | <name> | <role> | <status> | <approvalStatus> | <rating> |
```

- Localize all table labels and mapped display values.
- User/Evaluator: `Status` and `Approval status` MUST be `—`.

## Service table

```markdown
| # | Name | Type | Fee | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|
| 1 | <serviceName> | <serviceType> | <fee> | <freeTrial> | <endpoint> | <serviceDescription> |
```

- Render localized fields according to [`identity-service-contract.md` §Display Rules], after mapping command-specific source fields.
- Number services sequentially across Agent tables.
- Merge `Fee` and `Subscription` as `Fee`; subscription price takes precedence.
- Omit columns containing only `—`.
- Never display `serviceGuide` or internal Service UUIDs.
