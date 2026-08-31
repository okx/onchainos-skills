# Build a Service Search Query

Convert the current service-search request into the JSON below. For a refined
search, update the previous query only where the user explicitly changes or
removes a condition.

Return JSON only. Include every field.

```json
{
  "keywords": [],
  "aspAgentId": null,
  "aspName": null,
  "serviceName": null,
  "sid": null,
  "minPaymentTokenAmount": null,
  "maxPaymentTokenAmount": null,
  "unsupportedConstraints": [],
  "requiresUserInput": false
}
```

Rules:

- Extract IDs and names only when explicitly identified. Preserve their values.
- Extract only explicit numeric price bounds; never quantify “cheap” or similar
  wording.
- `keywords`: 1–5 minimal capability phrases in the user's language, maximum
  10. Remove request wrappers; do not translate, infer synonyms, or duplicate
  dedicated fields.
- Keep capability modifiers with what they qualify, such as chain, asset,
  real-time behavior, language, and output format.
- Exclude rejected requirements. For “not X, but Y”, keep Y.
- Put unsupported filters such as availability, rating, sales, ranking, and
  sorting in `unsupportedConstraints`; never silently discard them.
- Set `requiresUserInput` when a necessary condition is ambiguous; never guess.

Example:

`找一个实时监控 Solana 聪明钱钱包并推送信号的服务，价格不超过 10`

```json
{"keywords":["实时监控 Solana 聪明钱钱包","推送信号"],"aspAgentId":null,"aspName":null,"serviceName":null,"sid":null,"minPaymentTokenAmount":null,"maxPaymentTokenAmount":10,"unsupportedConstraints":[],"requiresUserInput":false}
```
