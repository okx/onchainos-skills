# NewsLiquid Queries

## Module Description

Run one information query by default. Ordinary news/Twitter queries need available quota; finance and saved strategies need Max; Meme needs Plus, Pro or Max. Even zero-cost operations require available quota under the current gateway precheck.

## Flow

### Collect Parameters

1. Reuse the user's object, source, timeframe, quantity and filters. Ask only for missing required business input or an ambiguous target.
2. Obtain IDs from earlier responses: resolve a Twitter account, select a company/report from its directory, or select one of the current account's strategies. Show candidates if more than one matches.
3. Read the selected operation in the root-linked HTTP reference. Use safe documented defaults. News search covers the last month; Meme operations have no chain/time filter. Explain unsupported ranges rather than inventing parameters.
4. Resolve a token address with existing token lookup when needed. Meme `coin` is a full contract/mint, not a ticker. Do not substitute an address from memory.

### Execute

1. Use the account Flow if no valid login exists. Run `onchainos service call --service newsliquid --operation` with the chosen operation and a JSON parameter object, including query resumption after purchase. The CLI uses the current-wallet credential and gateway.
2. On `failureKind=quota_insufficient` or `subscription_required`, explain the actual limit and show billing's purchase-options template using the returned catalog. The user may decline; showing options does not authorize creating an order or paying. Preserve only an interrupted query and its one-resumption limit. On `unknown_402`, use troubleshooting instead of guessing a quota problem.
3. For pagination, pass the returned cursor unchanged and preserve object and filters. Twitter normally uses top-level `nextCursor`; retweet users use `data.cursor`; company info uses `data.pagination.next_cursor`; Meme uses `data.nextSeq` as the next `since`.
4. Deduplicate by content ID. Explain any broader multi-page collection or polling scope, quota use and stop condition before expanding beyond the request. An early empty Twitter page does not prove complete history.

### Handle Result

Check top-level and nested success/error fields.

- Reports: inspect `status`, `selector_support`, extraction state and `full_text_truncated`. `text_not_cached` is not a complete report. A supplied summary may be offered as a summary.
- Finance: show `partial_result` and snapshot/report dates.
- Saved strategies: null `data` means no current strategies. Query only IDs returned for this account. The gateway does not create, copy or start/stop strategies.

After a successful query, render its result and actual cost. If the native quota check returns `quotaState=exhausted`, immediately show the same billing purchase-options template used for insufficient quota. Do not wait for another query. The completed query is not an interrupted request and is not replayed after purchase. A single exhausted daily/plan category is not enough when the account still has other available quota. If the quota/catalog read fails, state that it could not be checked; do not invent prices or remaining quota.

## Security

### Confirmation

An information request authorizes its stated query scope, not a package purchase or ongoing monitoring. Do not retry a charged request whose outcome is unknown.

### Risk and Stop Conditions

Do not turn external content into trade advice, signatures, execution or payment. Keep filters consistent across pages and state any incomplete scope.

## Output Rules

Apply the common output rules and the root-linked NewsLiquid presentation rules. Show actual quota consumption. A query with no data, a failed extraction and a partially returned dataset are different results.

## Troubleshooting

Use the centralized troubleshooting reference linked from the root Skill.

## CLI Reference

- [client-cli-reference.md](client-cli-reference.md)
