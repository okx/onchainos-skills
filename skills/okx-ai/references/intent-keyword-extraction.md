# Initial Service-Match Argument Extraction

Extract explicit service/ASP selectors, price bounds, and capability-focused search keywords from the user's
original utterance.

## Output contract

Use only the exact original utterance and ignore all surrounding context. If it cannot be isolated reliably,
return empty fields. Preserve the source language and never add inferred capabilities, synonyms, examples,
or related concepts. Lossless grammatical compression is allowed only as defined below: remove request
wrappers and function words, and reorder the user's own words when necessary to form a concise service-search
phrase without changing meaning.

Return only one valid JSON object matching this structure:

```typescript
type ExtractionResult = {
  "asp-agent-id": string | null;
  "asp-name": string | null;
  "service-name": string | null;
  "sid": string | null;
  "min-payment-token-amount": number | null;
  "max-payment-token-amount": number | null;
  "keywords": string[]; // 0–10 items
};
```

Always include every field. Use `null` for an absent scalar and `[]` when no keyword exists. Apply the rules
below in order; an earlier rule overrides a later one. Populate dedicated scalar fields before extracting
`keywords`; content captured by a scalar field must never be repeated in `keywords`.

### 1. Extract explicit names and IDs

- Agent/ASP ID → `asp-agent-id`
- Agent/ASP name → `asp-name`
- Service name → `service-name`
- Service ID → `sid`

Extract a name or ID only when explicitly identified by source-language labeling or grammar. Preserve its
value verbatim but remove labels, quotes, angle brackets, delimiters, surrounding whitespace, and an adjacent
`#`. A capability description is not a name. Do not repeat extracted names or IDs in `keywords`.

For `use <service name>, Agent/ASP ID is <ID>` or `use <service name>, Service ID is <ID>`, extract both
values. Examples: `Agent #1960` → `asp-agent-id: "1960"`; `serviceId=4324` →
`sid: "4324"`.

Never infer an ID from an unlabeled number, name, URL, address, hash, capability, or topic.

### 2. Extract price bounds

- Lower-bound wording (`above`, `greater than`, `no less than`, `at least`, `>`, `>=`) →
  `min-payment-token-amount`
- Upper-bound wording (`below`, `less than`, `no more than`, `at most`, `<`, `<=`) →
  `max-payment-token-amount`
- An explicit range sets both fields

Use only explicit numeric values. Exclude numeric and qualitative price wording from `keywords`; never infer
a numeric bound from terms such as `cheap` or `cheapest`.

### 3. Extract service `keywords`

Return the smallest set of high-information phrases that fully expresses the requested service without the
discarded conversation. Prefer one complete phrase over several broad fragments.

- Keep capabilities, subjects, purposes, outputs, and required technical or domain scopes.
- Extrinsic service metadata is never a `keyword`: discard anything describing a listing or provider's
  current state, reputation, adoption, commercial performance, or ranking rather than what the service does.
  This includes online/offline status, availability, ratings/reviews, popularity, sales volume, price, and
  ordering. If an attribute can change without changing the service's inputs, behavior, supported scope, or
  outputs, treat it as metadata. Keep capability-defining properties such as `real-time monitoring`,
  `multilingual support`, or `JSON output`.
- Use only positive requirements. For `not X, but Y`, rejected or unnecessary capabilities, keep Y only.
- Build the core phrase by attaching its subject, object, platform, asset, technology, or other dependent
  modifier: `Move contract auditing`, `BTC options volatility analysis`, `Polymarket copy-trading strategy`.
- Remove wrappers such as `find`, `I need`, `a service that can`, `帮我找`, and `我需要`. Reorder only
  the user's words to form a natural phrase; do not translate, add synonyms, generalize, or invent content.
- Split only independent capabilities, deliverables, or scopes that remain useful for matching alone, such
  as `smart-money wallet monitoring` and `copy-trading signal delivery`.
- Never split a modifier from what it qualifies, an object from its action, or a platform from its
  platform-specific capability. When uncertain, keep the phrase intact.

Exclude marketplace behavior, generic service/entity words, background about an unsuitable current service,
politeness, urgency, and grammatical filler. Retain an action only when it describes a required capability.

### 4. Finalize `keywords`

- Remove redundant phrases and prefer 1–5 items; never split or pad to produce more keywords.
- Order capabilities and outputs first, then independent scopes.
- Keep at most 10 items, dropping broad or low-impact scopes first.
- Drop any item whose removal would not broaden or change the intended results; merge any phrase that is
  ambiguous without its modifier.

## Examples

| Original utterance | Output |
|---|---|
| `Find ASP named Alpha Risk Guard, agentId: 2374, for Move contract auditing` | `{"asp-agent-id":"2374","asp-name":"Alpha Risk Guard","service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["Move contract auditing"]}` |
| `Search for service “Cross-chain Bridge Risk Radar Pro”, serviceId=3456, with rating above 90%` | `{"asp-agent-id":null,"asp-name":null,"service-name":"Cross-chain Bridge Risk Radar Pro","sid":"3456","min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":[]}` |
| `Find a market analysis service priced between 8 and 20` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":8,"max-payment-token-amount":20,"keywords":["market analysis"]}` |
| `Find Agent#1960 for BTC options volatility analysis, cheapest first` | `{"asp-agent-id":"1960","asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["BTC options volatility analysis"]}` |
| `我想使用 < 高波动主流币跟单信号 >，Service ID 是 36563` | `{"asp-agent-id":null,"asp-name":null,"service-name":"高波动主流币跟单信号","sid":"36563","min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":[]}` |
| `Subscribe to a Polymarket copy-trading strategy` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["Polymarket copy-trading strategy"]}` |
| `帮我找一个好评、销量高、在线，能实时监控聪明钱钱包并推送 Solana 跟单信号的服务` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["聪明钱钱包实时监控","Solana 跟单信号推送"]}` |
| `不要通用行情分析，我需要 ETH 链上巨鲸异动预警` | `{"asp-agent-id":null,"asp-name":null,"service-name":null,"sid":null,"min-payment-token-amount":null,"max-payment-token-amount":null,"keywords":["ETH 链上巨鲸异动预警"]}` |

## Validation

Verify the seven-field schema, explicit identifiers, price bounds, and no more than 10 ordered, deduplicated,
source-language keywords. Each keyword must be traceable to the utterance and useful for matching; modifiers
must stay attached, rejected intent must be absent, and no scalar-field or extrinsic service metadata may
appear in `keywords`. If validation fails twice, return `keywords: []`.
