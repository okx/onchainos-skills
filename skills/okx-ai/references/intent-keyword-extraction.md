# Initial Service-Match Argument Extraction

Extract explicit service/ASP selectors, price bounds, and capability-focused search keywords from the current query, using the previous query only to resolve follow-ups.

## Query Context

1. **Standalone or unrelated request:** extract from the current query only; ignore the previous query.
2. **Follow-up request:** use the previous query only to fill omitted context; the current query overrides conflicting, replaced, or rejected conditions.
3. Extract only explicitly stated or contextually resolved content; do not invent conditions.

## Output contract

1. **Return JSON only** with every field in this schema:
   ```typescript
   type ExtractionResult = {
     "asp-agent-id": string | null;
     "asp-name": string | null;
     "service-name": string | null;
     "sid": string | null;
     "min-payment-token-amount": number | null;
     "max-payment-token-amount": number | null;
     "keywords": string[];
   };
   ```
2. Use `null` for absent scalars and `[]` for no keywords.

## Keyword Extraction Rules

1. **Extract explicit names and IDs**
   - Agent/ASP ID → `asp-agent-id`
   - Agent/ASP name → `asp-name`
   - Service name → `service-name`
   - Service ID → `sid`
   - Extract only explicitly labeled values. Preserve them verbatim after removing labels, quotes, brackets, delimiters, whitespace, and an adjacent `#`.
2. **Extract price bounds**
   - Lower-bound wording (`above`, `greater than`, `no less than`, `at least`, `>`, `>=`) → `min-payment-token-amount`
   - Upper-bound wording (`below`, `less than`, `no more than`, `at most`, `<`, `<=`) → `max-payment-token-amount`
   - An explicit range sets both fields
3. **Extract and finalize service `keywords`**
   - Keep only requested capabilities and outputs with their required subjects, modifiers, and scopes; split only independent items useful alone.
   - Exclude names, IDs, price constraints, request wrappers, filler, rejected intent, generic service words, and provider/listing metadata; never quantify qualitative prices.
   - Build keywords only from source content in the previous or current query; when a follow-up adds a scope, attach it to the previous capability as one phrase, without expanding names into categories, synonyms, or related concepts.
   - Return 1–5 concise, deduplicated phrases; never exceed 10 or pad the list.

## examples

| Previous query | Current query | Output |
|---|---|---|
| — | `Find a market analysis service priced between 8 and 20` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":8,"max-payment-token-amount":20,"keywords":["market analysis"]}` |
| `找一个 BTC 行情分析服务` | `换成 ETH，价格低于 10` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":10,"keywords":["ETH 行情分析"]}` |
