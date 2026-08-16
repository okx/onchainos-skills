# Initial Service-Match Argument Extraction

Extract the requested service or ASP's capabilities and matching attributes from the user's original utterance.

## Output contract

Use only the exact original utterance and ignore all surrounding context. If it cannot be isolated reliably,
return empty fields. Do not translate, paraphrase, summarize, or add information, except for reductions
explicitly defined below.

Return only one valid JSON object matching this structure:

```typescript
type ExtractionResult = {
  "asp-agent-id": string | null;
  "asp-name": string | null;
  "service-name": string | null;
  "min-payment-token-amount": number | null;
  "max-payment-token-amount": number | null;
  "keywords": string[]; // 0–10 items
};
```

Always include every field. Use `null` for an absent scalar and `[]` when no keyword exists.

1. `asp-agent-id`: explicit Agent/ASP ID
2. `asp-name`: explicit Agent/ASP name
3. `service-name`: explicit service name
4. `min-payment-token-amount`: explicit lower price bound
5. `max-payment-token-amount`: explicit upper price bound
6. `keywords`: up to 10 capability phrases

Use these exact field names in the JSON object.

Apply these rules in order; an earlier rule overrides a later one.

### 1. Extract explicit names and IDs

- Agent/ASP ID → `asp-agent-id`
- Agent/ASP name → `asp-name`
- Service name → `service-name`
- Service ID → preserve verbatim as one `keywords` item because no `serviceId` field exists

Extract a name or ID only when source-language labeling or grammar identifies it unambiguously. Preserve
names and service IDs verbatim, excluding surrounding labels, quotation marks, and delimiters. Do not
repeat `asp-agent-id`, `asp-name`, or `service-name` in `keywords`.

A capability or deliverable description is not a name unless explicit naming grammar identifies it as one.

For the structure `use <service name>, Agent/ASP ID is <ID>`, always extract both `service-name` and
`asp-agent-id` accurately, excluding the angle brackets and surrounding whitespace.

For `asp-agent-id`, return only the ID value: remove an adjacent `Agent`/`ASP` label and `#` separator, but keep
the complete value following a separate ID label. For example, `Agent #1960` yields `1960`, while
`agentId: ASP-009` yields `ASP-009`.

Never infer an ID from a platform or brand name, domain, URL, email-like value, wallet or contract address,
transaction hash, capability, topic, ordinary token, or unlabeled number.

### 2. Extract matching units into `keywords`

Never expand the user's wording with inferred subtopics, synonyms, examples, or related terms that do not appear in the original utterance.

Keep only units describing what the service or ASP must do, provide, or be:

- Capability or function
- Subject or topic
- Purpose or output
- Capability scope, such as a required technology, language, region, asset, data type, integration, or
  format
- Matching attribute, such as quality, rating, status, availability, popularity, sales volume, qualitative
  price, category, role, service type, or ordering

Each item must be one concise, independently matchable semantic unit. Split combined expressions when
their parts remain meaningful and useful for matching independently. Keep a unit intact when splitting
would make it generic, incomplete, or different from the user's intent.

Extract semantic phrases, not tokens: split only when every resulting phrase remains complete and
independently narrows the candidate service set.

### 3. Remove non-matching text

Exclude:

- User behavior or workflow intent, such as finding, browsing, comparing, viewing, recommending, buying,
  using, subscribing, publishing, creating, or switching
- Marketplace-location phrases
- Generic entity or workflow words with no matching meaning
- Relational or grammatical wrappers with no matching meaning

Retain an action when it describes a required service capability or purpose; remove it when it only
describes the user's marketplace behavior. These exclusions never override an explicit value from rule 1.

### 4. Extract price bounds

- Lower-bound wording (`above`, `greater than`, `no less than`, `at least`, `>`, `>=`) →
  `min-payment-token-amount`
- Upper-bound wording (`below`, `less than`, `no more than`, `at most`, `<`, `<=`) →
  `max-payment-token-amount`
- An explicit range sets both fields

Use only explicit numeric values and exclude their price phrases from `keywords`. Keep qualitative price
attributes in `keywords` without inferring a number; reduce ranking wording to its core attribute when
appropriate, such as `cheapest` → `cheap`.

### 5. Finalize `keywords`

- Remove exact or semantically redundant duplicates.
- If more than 10 items remain, prioritize service IDs, capabilities, subjects, purposes, outputs, scopes,
  then matching attributes.
- Return selected items in their first-appearance order.
- Only trim wrappers or apply reductions explicitly defined above.

## Examples

| Original utterance | Output |
|---|---|
| `Find ASP named Alpha Risk Guard, agentId: 2374, for Move contract auditing` | `{"asp-agent-id":"2374","asp-name":"Alpha Risk Guard","service-name":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["Move","contract auditing"]}` |
| `Search for service “Cross-chain Bridge Risk Radar Pro”, serviceId=svc_CN-7, with rating above 90%` | `{"asp-agent-id":null,"asp-name":null,"service-name":"Cross-chain Bridge Risk Radar Pro","min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["svc_CN-7","rating above 90%"]}` |
| `Find a market analysis service priced between 8 and 20` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"min-payment-token-amount":8,"max-payment-token-amount":20,"keywords":["market analysis"]}` |
| `Find Agent#1960 for BTC options volatility analysis, cheapest first` | `{"asp-agent-id":"1960","asp-name":null,"service-name":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["BTC options","volatility analysis","cheap"]}` |
| `我想使用< 高波动主流币跟单信号 >，Agent ID 是 8136` | `{"asp-agent-id":"8136","asp-name":null,"service-name":"高波动主流币跟单信号","min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":[]}` |
| `Subscribe to a Polymarket copy-trading strategy` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["Polymarket","copy-trading strategy"]}` |
| `Find a highly rated, online, cheapest Agent` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["highly rated","online","cheap"]}` |

## Validation

Before returning, verify the six-field schema, explicit identifiers, price-bound mapping, and an ordered,
deduplicated, source-language `keywords` array of no more than 10 items. Every keyword must be traceable to
the original utterance, independently useful for matching, and free of workflow or generic terms. If this
validation fails, re-extract once from the original utterance only; if it still fails, return `keywords: []`.
