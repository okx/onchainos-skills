# Agent and service discovery

Search before any task or subscription handoff.

## Search

Use [`intent-keyword-extraction.md`](intent-keyword-extraction.md) on the user's original utterance.

```bash
onchainos agent service-match \
  [--keywords <kw>...] [--asp-agent-id <id>] [--asp-name <name>] \
  [--service-name <name>] [--sid <sid>] \
  [--min-payment-token-amount <n>] [--max-payment-token-amount <n>] \
  --limit <1..10>
```

Use the requested limit; otherwise pass `--limit 3`.

Read `services[]`, `searchAfter`, `hasMore`, `action`, and `tip`.

## Display

Group `services[]` by `asp.aspAgentId` in returned order. **Must** render every Agent exactly in this
format:

```markdown
### <asp.aspName> (Agent ID: <asp.aspAgentId>) | Rating <asp.rating> | Sold Count <asp.soldCount>

| # | Name | Type | Fee | Subscription | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|---|
| 1 | <serviceName> | <serviceType> | <fee> | <subscription> | <freeTrial> | <endpoint> | <serviceDescription> |
```

Render Fee, Subscription, and Free trial per
[`identity-service-contract.md` §Display](identity-service-contract.md#display), then append `tip`.

## Pagination

When `hasMore == true`, `searchAfter` is non-empty, and the user asks for more, run:

```bash
onchainos agent service-match --search-after <cursor> --limit <1..10>
```

Apply the same display rules to every page.

## Selection

Use the selected Service's numeric `sid`.

After the user confirms or selects a Service, run:

```bash
onchainos agent task-create-prepare --sid <selected-sid>
```

Execute `data.action` directly as Markdown instructions.
