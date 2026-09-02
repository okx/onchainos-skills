# Agent and service discovery

Search before any task or subscription handoff.

## Search

Use `intent-keyword-extraction.md` on the user's original utterance.

```bash
onchainos agent service-match \
  [--keywords <kw>...] [--asp-agent-id <id>] [--asp-name <name>] \
  [--service-name <name>] [--sid <sid>] \
  [--min-payment-token-amount <n>] [--max-payment-token-amount <n>] \
  --limit <1..10>
```

Use the requested limit; otherwise **MUST** pass `--limit 3`.

Read `services[]`, `searchAfter`, `hasMore`, and `tip`.

## Display

**MUST** group `services[]` by `asp.aspAgentId` in returned order and render each Agent using this layout, localizing labels and units:

```markdown
### <asp.aspName> (Agent ID: <asp.aspAgentId>) | Rating <asp.rating> | Sold Count <asp.soldCount>

| # | Name | Type | Fee | Subscription | Free trial | Endpoint | Description |
|---|---|---|---|---|---|---|---|
| 1 | <serviceName> | <serviceType> | <fee> | <subscription> | <freeTrial> | <endpoint> | <serviceDescription> |
```

Render fields per [`identity-service-contract.md` §Display]. **MUST** render all Agent tables as above,
then the localized CLI `tip` once.

```text
<CLI-returned tip>
```

## Pagination

When `hasMore == true`, `searchAfter` is non-empty, and the user asks for more, run:

```bash
onchainos agent service-match --search-after <cursor> --limit <1..10>
```

Apply the same rules to every page.

## Selection

Use the selected Service's numeric `sid`.

**MUST** stop. Only after explicit confirmation or selection in a subsequent user message, run:

```bash
onchainos agent task-create-prepare --sid <selected-sid>
```
