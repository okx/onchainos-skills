# Agent and service discovery

## Search

### Extract arguments

#### Query context

Extract explicit service/ASP selectors, price bounds, and capability-focused search keywords from
the current query. Use the previous query only to resolve follow-ups:

1. For a standalone or unrelated request, use only the current query.
2. For a follow-up, use the previous query only to fill omitted context; the current query
   overrides conflicting, replaced, or rejected conditions.
3. Use only explicit or contextually resolved content; never invent conditions.

#### Output contract

Build every field in this internal argument object; use `null` for absent scalars and `[]` for no
keywords:

```typescript
type SearchArguments = {
  "asp-agent-id": string | null;
  "asp-name": string | null;
  "service-name": string | null;
  "sid": string | null;
  "min-payment-token-amount": number | null;
  "max-payment-token-amount": number | null;
  "keywords": string[];
};
```

#### Extraction rules

Apply these extraction rules:

1. **Names and IDs:** Map explicitly labeled Agent/ASP ID, Agent/ASP name, Service name, and Service
   ID to `asp-agent-id`, `asp-name`, `service-name`, and `sid`. Preserve values verbatim after
   removing labels, quotes, brackets, delimiters, whitespace, and an adjacent `#`.
2. **Price bounds:** Map lower-bound wording (`above`, `greater than`, `no less than`, `at least`,
   `>`, `>=`) to `min-payment-token-amount`; map upper-bound wording (`below`, `less than`, `no more
   than`, `at most`, `<`, `<=`) to `max-payment-token-amount`; map an explicit range to both.
3. **Keywords:** MUST keep only requested capabilities and outputs with required subjects, modifiers,
   and scopes. MUST use only keywords explicitly present in the current query or carried-over context;
   MUST NOT invent, infer, paraphrase, translate, or expand them. Exclude names, IDs, price
   constraints, request wrappers, filler, rejected intent, generic service words, and provider/listing
   metadata. When a follow-up adds a scope, attach it to the previous capability as one phrase. Return
   1–5 concise, deduplicated phrases; if no capability or output is requested, MUST return
   `"keywords": []`.

#### Examples

| Previous query | Current query | Arguments |
|---|---|---|
| — | `Find a market analysis service priced between 8 and 20` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":8,"max-payment-token-amount":20,"keywords":["market analysis"]}` |
| `找一个 BTC 行情分析服务` | `换成 ETH，价格低于 10` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":10,"keywords":["ETH 行情分析"]}` |

### Run the search

Pass the non-null/non-empty arguments to:

```bash
onchainos agent service-match \
  [--keywords <kw>...] [--asp-agent-id <id>] [--asp-name <name>] \
  [--service-name <name>] [--sid <sid>] \
  [--min-payment-token-amount <n>] [--max-payment-token-amount <n>] \
  --limit <1..10>
```

Use the requested limit; otherwise **MUST** pass `--limit 3`, **NEVER** use another value.

### Read the result

Read `services[]`, `searchAfter`, `hasMore`, and `tip`.
Only now read `output-templates.md` for the selected Agent/Service display, and **MUST** render the
result according to its applicable template.
Do not load any A2A creation or action reference before the user explicitly
selects a Service and `task-create-prepare` returns its next action.

## Display results

**MUST** group `services[]` by `asp.aspAgentId` in returned order and render
each group with the `Agent Service group` template in `output-templates.md`.

**MUST** render all Agent groups, then the localized CLI `tip` once.

```text
<CLI-returned tip>
```

## Pagination

When `hasMore == true`, `searchAfter` is non-empty, and the user asks for more, run:

```bash
onchainos agent service-match --search-after <cursor> --limit <1..10>
```

Apply the same rules to every page.

## Select a service

Use the selected Service's numeric `sid` internally. **NEVER** show `sid`.

**MUST** stop. Only after explicit confirmation or selection in a subsequent
User message, hand the exact selected numeric `sid` to
[`../a2a/user/create-prepare.md`](../a2a/user/create-prepare.md). That leaf owns the one
`task-create-prepare` call and every resulting branch.
