# Identity service output template

Shared service table and display rules.

## Service table

```markdown
| # | Name | Type | Fee | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|
| 1 | <serviceName> | <serviceType> | <fee> | <freeTrial> | <endpoint> | <serviceDescription> |
```

## Rules

- Render localized fields according to [`identity-service-contract.md` §Display], after mapping command-specific source fields.
- Number services sequentially across Agent tables.
- Merge `Fee` and `Subscription` as `Fee`; subscription price takes precedence.
- Omit columns containing only `—`.
- Never display `serviceGuide` or internal Service UUIDs.
