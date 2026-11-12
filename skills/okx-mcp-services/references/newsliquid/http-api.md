# NewsLiquid HTTP API

## Supported Capabilities

| Section | Contents |
| --- | --- |
| [Account](#account) | Health, wallet login and current quota |
| [News](#news) | Sources and news search |
| [Twitter](#twitter) | Profiles, tweets and server monitors |
| [Finance](#finance) | Companies, reports, events and holdings |
| [Meme](#meme) | Token opinions and activity evidence |
| [Strategies](#strategies) | Saved strategies and their hits |
| [Billing](#billing) | Offers, orders, payments and renewal |

| Section | Access and cost | Parameter and result rules |
| --- | --- | --- |
| Finance | All operations require Max and quota. | Inspect nested status/message and partial results. Optional parameters accept omission/null unless a default is documented. Dates use `YYYY-MM-DD`. |
| Meme | Requires Plus/Pro/Max. Currently costs 10 points per request. | Multiple values accept an array or comma-separated values. No chain/time filter is supported. Pass returned `nextSeq` as `since` for later events. |
| Strategies | Requires Max. Successful calls currently cost one point. | Read only the current account's strategies. |
| News/Twitter | Ordinary queries require quota. | — |

All costs remain subject to actual usage.

Use the native CLI for service operations. User-managed clients reuse CLI login and retrieve credentials through the root-linked CLI reference. Each endpoint below names its CLI operation.

- For catalog operations, pass body or query fields in `--params` and path IDs in `--id`.
- For login, order creation, payment and cancellation, use the dedicated parameters in the operation reference.
- The CLI manages credentials and protocol headers internally. Use native operations rather than constructing HTTP requests for CLI-managed business flows.

Business decisions remain in the root-linked Flow references.

HTTP uses `https://x402-okx.newsliquid.com`. Wallet login and payment use X Layer (`chainId=196`); finance snapshot-chain filters do not change that network. Protected calls use Bearer authentication; JSON requests use `Content-Type: application/json`. IDs for Twitter are strings, booleans are JSON booleans, and usernames omit `@`. Check `success`, nested errors and `usage.cost`; quota values are decimal strings.

### Account

#### Service health

**GET /healthz**. No Token required. Parameters: none. CLI operation: `healthz`.

Returns ok and service. This only indicates whether the service is running.

#### Service login

Use the account Flow for CLI login. The CLI handles the wallet challenge and signature internally. User-managed clients read the stored Token through the `token` operation in the root-linked CLI reference.

#### Read one-time quota and the current plan

**GET /billing/quota**. Parameters: none. Reads only the current account. CLI operation: `billing-quota`.

| Return field | Meaning |
| --- | --- |
| oneTime | One-time quota, with quota, used and remaining |
| subscription | Current plan; null when no valid plan exists |
| subscription.planId / name / status | Plan tier, name and entitlement status |
| subscription.quota / used / remaining | Quota, used points and remaining points for this period |
| subscription.periodEnd / serviceEnd | Current quota-period end / purchased service expiry, in Unix seconds |
| autoRenew | Enabled renewal at expiry; null when disabled |
| autoRenew.subId / status | Renewal authorization ID for cancellation / active means effective; pending means awaiting on-chain confirmation |
| autoRenew.planId / name / price / currency | Renewal plan and monthly price/currency |
| autoRenew.nextChargeAt | Next charge time, in Unix seconds |
| dailyRefresh / legacy | Other available quota categories, each with quota, used and remaining |
| totalRemaining | Total available quota; one category must cover each call, and categories cannot be combined |

Quota values are decimal strings, not dollar amounts.

### News

#### News sources and types

**GET /news_type**. No request body. CLI operation: `news-type`.

Cost: 0. The account still needs available quota to pass the current gateway precheck.

Parameters: none.

Returns engine and category lists. Use their code values in searches.

#### Search news

**POST /news_search**。 CLI operation: `news-search`.

Cost: 1 point per 20 returned items, with a minimum of 1. No results still cost 1 point.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| q | string | No | "" | Full-text body search; supports phrases, OR and -excluded terms, such as "Bitcoin OR Ethereum" |
| engineTypes | object<string, string[]> | No | {} | Maps engine codes to category-code arrays from /news_type. An empty [] selects all categories for that engine. Multiple engines use OR |
| coins | string[] | No | [] | Token symbols, such as ["BTC","ETH"]; matches any listed token |
| hasCoin | boolean | No | false | true selects news tagged with a token; false applies no token-tag filter |
| score | integer / null | No | null | Minimum score, recommended range 0–100; filters by score >= this value. Omission or null disables filtering; 0 differs from omission |
| page | integer | No | 1 | Starts at 1; values <= 0 become 1, and values > 100 become 100 |
| limit | integer | No | 20 | Items per page; values <= 0 become 20, and values > 100 become 100 |

```json
{"q":"Bitcoin","coins":["BTC"],"page":1,"limit":20}
```

Returns a news list for the last month. Filters can be combined. total is not an exact match count. Stop pagination when fewer than a full page are returned.

### Twitter

#### Read a Twitter user by username

**POST /twitter_user_info**。 CLI operation: `twitter-user-info`.

Cost: 1 point per successful query.

| Parameter | Type | Required | Default | Description and examples |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Non-empty username without @, such as "OpenAI" |

```json
{"username":"OpenAI"}
```

Returns the user profile. Also check data.success and read data.error on failure.

#### Read a Twitter user by ID

**POST /twitter_user_by_id**。 CLI operation: `twitter-user-by-id`.

Cost: 1 point per successful query.

| Parameter | Type | Required | Default | Description and examples |
| --- | --- | --- | --- | --- |
| userId | string | Yes | None | Non-empty Twitter user ID, such as "<Twitter user ID>"; obtain it from a user profile response |

```json
{"userId":"<Twitter user ID>"}
```

Returns the user profile. Here userId identifies a Twitter user, not the logged-in wallet.

#### Read user tweets

**POST /twitter_user_tweets**。 CLI operation: `twitter-user-tweets`.

Cost: 1 point per 20 successfully returned tweets. When no tweets are found, the current endpoint returns an error without charging quota.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Target username, such as "OpenAI" |
| product | string | No | "Latest" | Latest or Top; results are not guaranteed to strictly follow that order |
| maxResults | integer | No | 20 | Maximum items per page; values <= 0 become 20, and values > 100 become 100. Excluding replies/retweets may reduce the returned count |
| includeReplies | boolean | No | false | Include replies by this user |
| includeRetweets | boolean | No | false | Include retweets by this user |
| cursor | string | No | "" | Omit initially; use the returned top-level nextCursor for the next page and keep other parameters unchanged |

```json
{"username":"OpenAI","product":"Latest","maxResults":20,"includeReplies":false,"includeRetweets":false}
```

Returns a tweet array in data and a top-level nextCursor when more results exist. An empty or missing cursor means no next page. Pass cursors unchanged and preserve username, product, includeReplies and includeRetweets. The final page may have empty data and `usage.cost: "0"`. This is equivalent to /twitter_search with `from:<username>` and the corresponding -filter:replies/-filter:retweets.

#### Search tweets

**POST /twitter_search**。 CLI operation: `twitter-search`.

Cost: 1 point per actual upstream query that returns results; 0 when none are found. One request may query several pages. maxResults: 20 does not guarantee a cost of 1, and cost is not simply the final item count divided by 20.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| query | string | Yes | None | Non-empty native Twitter query, such as "from:OpenAI since:2026-10-01 -filter:retweets"; include keyword, author and date filters here |
| product | string | No | "Top" | Top or Latest |
| cursor | string | No | "" | Omit initially; use the returned top-level nextCursor and preserve query and product. Do not construct a cursor |
| maxResults | integer | No | 20 | Maximum items; values <= 0 become 20, and values > 100 become 100 |

```json
{"query":"Bitcoin min_faves:10 -filter:retweets","product":"Latest","maxResults":20}
```

Returns a tweet array in data and a top-level nextCursor when more results exist. An empty or missing cursor means no next page.

#### Read follow/unfollow events

**POST /twitter_follower_events**。 CLI operation: `twitter-follower-events`.

Cost: 1 point per successful query.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Target username, such as "OpenAI" |
| isFollow | boolean | No | false | true reads follows; false reads unfollows. Pass explicitly when possible; omission selects unfollows |
| maxResults | integer | No | 20 | Maximum events; values <= 0 become 20, and values > 100 become 100 |

```json
{"username":"OpenAI","isFollow":true,"maxResults":20}
```

Returns recorded follow/unfollow events. This request does not create a monitor.

#### Read deleted tweets

**POST /twitter_deleted_tweets**。 CLI operation: `twitter-deleted-tweets`.

Cost: 1 point per 20 returned items; 0 when no results are returned.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Target username, such as "OpenAI" |
| maxResults | integer | No | 20 | Maximum items; values <= 0 become 20, and values > 100 become 100 |

```json
{"username":"OpenAI","maxResults":20}
```

Returns recorded deleted tweets. Complete historical coverage is not guaranteed.

#### Read KOL followers

**POST /twitter_kol_followers**。 CLI operation: `twitter-kol-followers`.

Cost: 1 point per successful query.

| Parameter | Type | Required | Default | Description and examples |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Target username, such as "OpenAI"; pagination and count parameters are not supported |

```json
{"username":"OpenAI"}
```

Returns data.totalCount and data.users.

#### Read a Twitter article

**POST /twitter_article_by_id**。 CLI operation: `twitter-article-by-id`.

Cost: 1 point per successful query.

| Parameter | Type | Required | Default | Description and examples |
| --- | --- | --- | --- | --- |
| id | string | Yes | None | Non-empty Twitter article ID, such as "<content ID>"; not an article URL or username |

```json
{"id":"<content ID>"}
```

Returns the article title, content, author and other available information.

#### Read one tweet

**POST /twitter_tweet_by_id**。 CLI operation: `twitter-tweet-by-id`.

Cost: 1 point per successful query.

| Parameter | Type | Required | Default | Description and examples |
| --- | --- | --- | --- | --- |
| id | string | Yes | None | Target tweet ID as a string, such as "<content ID>" |

```json
{"id":"<content ID>"}
```

Returns one tweet and available quote/reply context.

#### Read quote tweets

**POST /twitter_quote_tweets_by_id**。 CLI operation: `twitter-quote-tweets-by-id`.

Cost: 1 point per 20 returned items; 0 when no results are returned.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| id | string | Yes | None | Original quoted tweet ID, such as "<content ID>" |
| maxResults | integer | No | 20 | Maximum items per page; values <= 0 become 20, and values > 100 become 100 |
| cursor | string | No | "" | Omit initially; use the returned top-level nextCursor and preserve id |

```json
{"id":"<content ID>","maxResults":20}
```

Returns quote tweets. Each quoteId identifies the original quoted tweet. Cost is 1 point per 20 returned items. A top-level nextCursor indicates more results; an empty or missing cursor means no next page. Equivalent to /twitter_search with `quoted_tweet_id:<id>`.

#### Read retweet users

**POST /twitter_retweet_users_by_id**。 CLI operation: `twitter-retweet-users-by-id`.

Cost: 1 point per successful query. Each cursor page is a separate, separately charged request.

| Parameter | Type | Required | Default | Description and values |
| --- | --- | --- | --- | --- |
| id | string | Yes | None | Original retweeted tweet ID, such as "<content ID>" |
| cursor | string | No | "" | Omit initially; use the returned data.cursor and preserve id |

```json
{"id":"<content ID>"}
```

Returns data.users and the next data.cursor. Stop when the cursor is empty and preserve id across pages.

#### List Twitter monitors

**POST /twitter_watch**. Requires a valid **Max** plan and available quota. Cost: 0. Send {} as the body. CLI operation: `twitter-watch`.

```json
{}
```

Returns all monitors for the current account in data, with their count in total. Each item includes twAccount, twUserName, twGroupId, remark, event switches (newTweetBol, newRetweetBol and others, as defined below), isPaused (paused) and isSuspended (account no longer exists).

#### Add a Twitter monitor

**POST /twitter_watch_add**. Requires a valid **Max** plan and available quota. CLI operation: `twitter-watch-add`.

Cost: 10 points for a new account; 0 for an existing account or a group move.

| Body parameter | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Twitter username, preferably without @ |
| groupName | string | No | "\u5c0f\u9f99\u867e" | Monitor group name; an empty string also uses this default (JSON Unicode escapes shown) |
| remark | string | No | "" | Custom note |
| newTweetBol | boolean | No | true | New tweet events |
| newTweetReplyBol | boolean | No | true | Reply events |
| newTweetQuoteBol | boolean | No | true | Quote tweet events |
| newRetweetBol | boolean | No | true | Retweet events |
| newFlwBol | boolean | No | false | Follow events |
| newUnFlwBol | boolean | No | false | Unfollow events |
| updateNameBol | boolean | No | true | Display-name changes |
| updateDescBol | boolean | No | true | Profile-description changes |
| updateAvatarBol | boolean | No | true | Avatar changes |
| updateBannerBol | boolean | No | true | Banner changes |
| newCaBol | boolean | No | false | CA events |
| tweetToppingBol | boolean | No | false | Pinned-tweet events |

```json
{"username":"elonmusk","groupName":"Research","remark":"Elon","newTweetBol":true,"newFlwBol":false}
```

Returns data.success, username, groupId, cost and message. Moving an existing account preserves its switches and remark. Success automatically refreshes Twitter WSS monitoring.

#### Delete a Twitter monitor

**POST /twitter_watch_delete**. Requires a valid **Max** plan and available quota. CLI operation: `twitter-watch-delete`.

Cost: 0. Available quota is still required.

| Body parameter | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| username | string | Yes | None | Twitter username to stop monitoring, preferably without @ |

```json
{"username":"elonmusk"}
```

Returns the operation result and `usage.cost: "0"`, and automatically refreshes Twitter WSS monitoring. Only the current account's monitor record is deleted.

### Finance

#### Search companies

**POST /finance-enhance/company-search**. Search candidates by company name, ticker or company identifier. CLI operation: `finance-enhance-company-search`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| keyword / query / identifier | string | At least one | Search term or identifier; precedence is identifier, keyword, query |
| identifier_type | string | auto | auto, ticker, cik, krx_stock_code, dart_corp_code, company_name |
| market | string | GLOBAL | US, KR, GLOBAL |
| exchange | string | Optional | Exchange filter |
| result_scope | string | issuer | issuer groups by issuer; security groups by security |
| limit | integer | 20 | Number of results, 1–100 |
| auto_collect | boolean | false | Attempt collection when cached data is missing |

```json
{"keyword":"IBM","market":"US","limit":5,"auto_collect":false}
```

Returns data.companies, data.count and matching company identifiers.

#### Company profile and report directory

**POST /finance-enhance/company-info**. Read company information, available reports and financial field names. CLI operation: `finance-enhance-company-info`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| company / identifier | string | At least one | Company name, ticker or CIK; company takes precedence |
| identifier_type | string | auto | auto, ticker, cik, krx_stock_code, dart_corp_code, company_name |
| market | string | GLOBAL | US, KR, GLOBAL |
| exchange | string | Optional | Exchange filter |
| result_scope | string | issuer | issuer or security |
| auto_collect | boolean | false | Attempt collection when cached data is missing |
| filing_limit | integer | 50 | Number of filings, 1–1000 |
| fact_limit | integer | 1000 | Number of financial facts to scan, 1–20000 |
| research_limit | integer | 20 | Directly matched research reports, 0–100; 0 disables this query |
| related_research_limit | integer | 10 | Related reports when no direct match exists, 0–50; 0 disables this query |
| transcript_limit | integer | 20 | Earnings-call transcripts, 0–100; 0 disables this query |
| cursor | string | Optional | Research/transcript cursor; omit initially, then use data.pagination.next_cursor (such as page:1) and keep other parameters unchanged |
| offset | integer | 0 | Non-negative pagination offset when cursor is absent |
| include_research_companies | boolean | false | Include companies covered by research reports |
| include_all_filings | boolean | false | Include all report types; defaults to common types only |

```json
{"company":"IBM","filing_limit":5,"fact_limit":10,"research_limit":1,"related_research_limit":0,"transcript_limit":1,"auto_collect":false}
```

Returns data.company, data.available_reports, data.available_financial_item_names and data.pagination. Select a report from the directory, then read its text with the next endpoint.

#### Report, research and earnings-call text

**POST /finance-enhance/company-report-text**。 CLI operation: `finance-enhance-company-report-text`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| company / identifier | string | At least one | Company name, ticker or CIK; company takes precedence |
| report_name | string | Conditional | Report name, type (such as 10-K), research identifier or earnings-call quarter; provide at least one of report_name, report_id and accession |
| report_id | string | Optional | Exact identifier from the company report directory; takes precedence over report_name |
| report_type | string | Auto-detected | SEC, RESEARCH, TRANSCRIPT, DART |
| identifier_type | string | auto | auto, ticker, cik, krx_stock_code, dart_corp_code, company_name |
| market | string | GLOBAL | US, KR, GLOBAL |
| exchange | string | Optional | Exchange filter |
| accession | string | Optional | SEC accession or report identifier |
| form_type | string | Optional | Report type, such as 10-K or 10-Q; must match the target report |
| section_key | string | Optional | Target section; check selector_support for support |
| text_search | string | Optional | Text keyword; check selector_support for support |
| auto_collect | boolean | false | Attempt collection when cached data is missing |
| max_section_chars | integer | 50000 | Maximum characters per section; must be greater than 0 |
| filing_search_limit | integer | 100 | Number of candidate reports, 1–1000 |
| section_limit | integer | 100 | Number of returned sections, 1–1000 |

```json
{"company":"IBM","report_name":"10-K","max_section_chars":2000,"section_limit":2,"auto_collect":false}
```

Returns data.selected_report, data.sections, data.full_text and any available data.llm_summary. full_text_truncated indicates truncated text; a summary or truncated text is not a complete report. `status: "text_not_cached"` means the report exists but its text is unavailable, with no quota charged. With auto_collect enabled, the service downloads and converts uncached PDFs. The request may time out with query failed while background processing continues. Use the root-linked troubleshooting Flow for retry and summary choices.

#### Key market events

**POST /finance-enhance/key-market-events**. Read macro policy, economic-data and major-company earnings schedules. CLI operation: `finance-enhance-key-market-events`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| start_date / end_date | string | Optional | Start and end dates |
| categories | string[] | Optional | Categories, such as ["macro","earnings"] |
| countries | string[] | Optional | Countries/regions, such as ["US","JP"] |
| event_types | string[] | Optional | Event types, such as fed_rate_decision or cpi |
| tickers | string[] | Optional | Stock tickers, such as ["NVDA"] |
| sector | string | Optional | Industry sector |
| importance | string | Optional | Importance, such as high |
| include_estimated | boolean | true | Include estimated schedules |
| text_search | string | Optional | Text keyword |
| auto_collect | boolean | true | Attempt schedule updates when cached data is missing |
| force_collect | boolean | false | Force a refresh before querying |
| lookahead_days | integer | Optional | Days to look ahead when updating schedules |
| earnings_batch_size | integer | Optional | Batch size for company earnings-date updates |
| limit | integer | 100 | Number of results, maximum 500 |

```json
{"categories":["macro"],"countries":["US"],"limit":5,"auto_collect":false}
```

Returns data.events and data.count. source_status marks estimated schedules. Verify important dates against official releases.

#### Institution directory

**POST /finance-enhance/institution-managers**. Find institutions with SEC 13F filings. CLI operation: `finance-enhance-institution-managers`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| search | string | Optional | Institution name or manager CIK |
| as_of | string | Optional | Institutions with filings on or before this date |
| limit | integer | 100 | Number of results, 1–500 |
| offset | integer | 0 | Pagination offset |

```json
{"search":"Berkshire","limit":5}
```

Returns data.institutions, data.count and data.total_count. Use manager_cik from the result to query holdings.

#### Institutional stock holdings

**POST /finance-enhance/institution-stock-holdings**. Read quarterly SEC 13F filings. CLI operation: `finance-enhance-institution-stock-holdings`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| institution | string | Required | Institution name or manager_cik from the previous response |
| as_of | string | Optional | Select a reporting period on or before this date |
| changed_only | boolean | false | Return only changed holdings |
| include_options | boolean | false | Include call/put options |
| include_exited | boolean | false | Include exited positions |
| sort_by | string | value | value, shares, change, institution |
| auto_collect | boolean | true | Check and update filings at the refresh interval |
| force_collect | boolean | false | Compatibility parameter; does not bypass the filing refresh interval |
| limit | integer | 100 | Number of results, 1–500 |
| offset | integer | 0 | Pagination offset |

```json
{"institution":"0001067983","limit":5,"auto_collect":false}
```

Returns data.holdings, data.summary and data.pagination. These are quarter-end holdings with a filing delay.

#### On-chain holdings

**POST /finance-enhance/crypto-holdings**. Read holdings snapshots by wallet address or a listed institution. CLI operation: `finance-enhance-crypto-holdings`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| address | string | Optional | EVM wallet address; provide it to query a specific wallet |
| institution | string | Optional | Institution name; covers only listed addresses |
| chain | string | Optional | ethereum, base, optimism or gnosis; defaults to ethereum when address is supplied |
| token_address / token_symbol | string | Optional | Filter by token contract (case-insensitive) or symbol |
| as_of | string | Optional | Snapshot date, YYYY-MM-DD |
| auto_collect | boolean | false | Attempt address collection when snapshots are missing |
| force_collect | boolean | false | Force collection with auto_collect:true |
| collect_max_rows | integer | Optional | Maximum rows to collect, up to 10000 |
| limit | integer | 100 | Number of results, maximum 1000 |

```json
{"address":"<target wallet address>","chain":"ethereum","limit":5,"auto_collect":false}
```

Returns data.holdings and data.count. Query a specific wallet when the institution address is not listed.

#### On-chain holding changes

**POST /finance-enhance/crypto-holding-changes**. Read holdings differences between adjacent snapshots. CLI operation: `finance-enhance-crypto-holding-changes`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| address | string | Optional | EVM wallet address |
| institution | string | Optional | Listed institution name |
| chain | string | Optional | ethereum, base, optimism or gnosis; defaults to ethereum when address is supplied |
| token_address / token_symbol | string | Optional | Filter by token contract (case-insensitive) or symbol |
| change_types | string[] | Optional | new, increase, decrease, exit, unchanged |
| start_date / end_date | string | Optional | Start and end dates |
| include_unchanged | boolean | false | Include unchanged items |
| auto_collect | boolean | false | Attempt collection when snapshots are missing |
| force_collect | boolean | false | Force collection with auto_collect:true |
| collect_max_rows | integer | Optional | Maximum rows to collect, up to 10000 |
| limit | integer | 100 | Number of results, maximum 1000 |

```json
{"address":"<target wallet address>","chain":"ethereum","limit":5,"auto_collect":false}
```

Returns data.changes and data.count. At least two snapshots are needed. Snapshot differences are not individual transaction records.

### Meme

#### Token opinion evidence

**POST /meme/token-evidence**. Read original token opinions on fomo/pump. CLI operation: `meme-token-evidence`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| coin | string | Required | Full token contract address or Solana mint; lowercase is recommended for EVM addresses |
| kind | string or string[] | thesis,callout | Event types; supplied values replace the default |
| platform | string or string[] | All | fomo, pump |
| handle | string or string[] | Optional | Author username |
| user | string or string[] | Optional | Author user ID |
| since | integer | 0 | Return only events with seq greater than this value |
| limit | integer | 200 | Number of results, maximum 1000 |

```json
{"coin":"<token contract address or mint>","platform":["fomo","pump"],"limit":5}
```

Returns data.count, data.nextSeq and data.items. Each item includes platform, type, author, text and time.

#### Opinion changes and trade evidence

**POST /meme/activity**. Read later activity by token, author or event type. CLI operation: `meme-activity`.

| Parameter | Type | Required/default | Description |
| --- | --- | --- | --- |
| coin | string | Optional | Full token contract address or Solana mint |
| handle | string or string[] | Optional | Author username |
| user | string or string[] | Optional | Author user ID |
| wallet | string or string[] | Optional | pump wallet address |
| kind | string or string[] | All | thesis, callout, buy, sell, position_up, update, other; trade means buy and sell |
| platform | string or string[] | All | fomo, pump |
| since | integer | 0 | Event seq cursor |
| limit | integer | 200 | Number of results, maximum 1000 |

```json
{"kind":["buy","sell"],"since":0,"limit":5}
```

Returns data.count, data.nextSeq and data.items. items[].trade contains trade evidence; items[].text contains the original opinion.

### Strategies

#### List my strategies

**GET /strategy_list**. Pass parameters in the query. CLI operation: `strategy-list`.

| Parameter | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| page | integer | No | 1 | Page number, maximum 1000 |
| limit | integer | No | 20 | Items per page, maximum 100; page_size is also accepted |

Returns a data array, total, page and limit, sorted by creation time descending. Each item contains id (strategy ID), name, description, enabled and createdAt. With no strategies, data is null and total is absent.

#### Read hits by strategy ID

**GET /strategy_hits**. Pass parameters in the query. CLI operation: `strategy-hits`.

| Parameter | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| strategyId | integer | Yes | None | Strategy id from the preceding endpoint; only the current account's strategies can be read |
| page | integer | No | 1 | Page number, maximum 1000 |
| limit | integer | No | 20 | Items per page, maximum 100; page_size is also accepted |

Returns a data array, total, page and limit, sorted by hit time descending. Each item is the original matching content, usually a news item with id, text and aiRating, using the news response structure. Deduplicate by id. A strategy outside the current account returns 400 (strategy not found). A missing strategyId also returns 400. Neither error charges quota.

### Billing

#### List purchasable offers

**GET /billing/offers**. No Token required. Parameters: none. CLI operation: `billing-offers`.

Returns subscriptions and oneTimePackages. Main fields:

| Field | Meaning |
| --- | --- |
| kind / productId / name | Product kind, ID and name; IDs may be shared across different kinds |
| price / currency | Decimal-string price and currency |
| quota / dailyQuota | Plan/package quota and extra daily quota, both decimal strings |
| network / asset / payTo | Payment network, token contract and recipient |
| amount / decimals | Minimal-unit amount (integer string) and token decimals |
| maxPeriods | Plan authorization periods: 1 for the current single-month purchase; auto-renewal quotes use 12 |

#### Create a purchase order

**POST /billing/orders**. Creating an order does not charge. CLI operation: `create-order`. The CLI also requires `operationKey` in `--params`.

| Location | Parameter | Type | Required | Description |
| --- | --- | --- | --- | --- |
| Header | Idempotency-Key | string | Yes | 1–128 bytes; UUID recommended. Keep stable for the same operation |
| JSON | kind | string | Yes | subscription (single-month plan), one_time (quota package) or auto_renew (renew the current plan at expiry) |
| JSON | productId | integer | Yes | From the catalog for the selected kind; auto_renew requires the current planId |

```json
{"kind":"subscription","productId":2}
```

The example productId shows its type only. Use an actual ID from the selected kind's catalog. An auto_renew quote contains autoRenew: true, startAt, maxPeriods: 12 and autoRenewTerms. Only a valid plan with more than 20 minutes until expiry qualifies; otherwise the service returns 409. The body contains only kind and productId. Returns orderId, payUrl, quote, expiresAt and status.

#### Fetch a payment challenge and pay

**POST /billing/orders/:id/pay**。 CLI operations: `prepare-payment` and `pay`; pass the order ID via `--id`. Use their parameters from the operation reference.

| Location | Parameter | Type | Required | Description |
| --- | --- | --- | --- | --- |
| Path | id | string | Yes | orderId from the preceding step |
| Header | PAYMENT-SIGNATURE | string | When paying | Omit initially to obtain a challenge, then submit the SDK-encoded payment proof |

No request body. Without payment proof, the 402 response's PAYMENT-REQUIRED header contains the encoded challenge. Submit proof through PAYMENT-SIGNATURE to the same order.

Protocol constraints:

- Challenge amount, network, asset and payTo match the order quote.
- Single-month plans use period with maxPeriods=1; one-time quota packages use exact.
- Auto-renewal uses period with maxPeriods=12, extra.startAt matching the quote and zero initial charge.
- A period proof contains two EIP-712 signatures: Permit2 and subscription terms. Permit2 validity covers the complete service window.
- Login signatures and payment proofs serve different purposes and cannot be exchanged. The existing CLI/payment implementation handles encoding. See the [official subscription documentation](https://web3.okx.com/zh-hans/onchainos/dev-docs/payments/subscription) for the protocol.

202 or processing means pending. An auto-renewal order's paymentStatus: authorized describes order authorization. The account's autoRenew.status: active means renewal is effective.

#### Read order and fulfillment status

**GET /billing/orders/:id**. Path id is a required string containing the actual orderId. No other parameters. CLI operation: `billing-orders-id`; pass the original order ID via `--id`.

Returns orderId, quote, paymentStatus, fulfillmentStatus, expiresAt and any available periodStart, periodEnd, subId and transaction. paymentStatus: paid with fulfillmentStatus: completed confirms payment and fulfillment. For auto-renewal, paymentStatus: authorized confirms authorization, not a charge or new quota. Read actual rights and renewal status from /billing/quota.

#### Reconcile paid but unfulfilled orders

**POST /billing/orders/:id/reconcile**. Path id is a required string containing the actual orderId. No request body. CLI operation: `billing-orders-id-reconcile`; pass the original order ID via `--id`.

Use for an original paid order that remains incomplete after approximately 5 minutes or has no quota visible in /billing/quota. The gateway resends fulfillment notifications using official OKX payment records. The same payment is credited only once; **no new charge or signature is required**.

Returns orderId, result, paymentStatus, fulfillmentStatus and republished (payment IDs notified again). paymentStatus and fulfillmentStatus reflect the reconciled order. Crediting is asynchronous, so republished usually still means processing. result values:

| result | Protocol state |
| --- | --- |
| completed | Credited |
| republished | Fulfillment notification resent; completes asynchronously, usually in about 1 minute |
| processing | Payment is not yet confirmed |
| unpaid | Order is unpaid |
| failed | Payment failed; no charge |
| manual_review | Payment outcome needs manual verification |

A repeat call within 60 seconds after successful reconciliation returns 429 and Retry-After. An order with payment processing in progress returns 409.

#### List my subscription authorizations

**GET /billing/subscriptions**. Parameters: none. CLI operation: `billing-subscriptions`.

Returns an array with subId, status, quote, periodStart, periodEnd and subscription. subId and order ID are not interchangeable. Read current plan quota and expiry from /billing/quota.

#### Cancel automatic renewal

**POST /billing/subscriptions/:id/cancel**. Stops renewal at expiry, including earlier multi-period authorizations. Use autoRenew.subId from /billing/quota as id. Single-month purchases do not renew and need no cancellation. CLI operation: `cancel-renewal`; pass the subscription subId via `--id`. The CLI builds and signs the cancellation.

| Location | Parameter | Type | Required | Description |
| --- | --- | --- | --- | --- |
| Path | id | string | Yes | Current wallet's subId as a 32-byte hex string |
| JSON | cancelAuth | object | Yes | Cancellation authorization and signature below |
| JSON | cancelAuth.subId | string | Yes | Must exactly match path id |
| JSON | cancelAuth.action | integer | Yes | Fixed at 0 |
| JSON | cancelAuth.initiator | integer | Yes | Fixed at 0, buyer-initiated |
| JSON | cancelAuth.nonce | string | Yes | Fresh random bytes32 with a 0x prefix |
| JSON | cancelAuth.deadline | integer | Yes | Unix seconds; later than now and no more than now plus 600 seconds |
| JSON | cancelAuth.signature | string | Yes | Payer wallet's EIP-712 signature of CancelAuth |

The body contains cancelAuth. Its signature follows the subscription contract's CancelAuth EIP-712 domain. The existing CLI builds, signs and submits it. Users do not create cancellation request files.

Returns subId and state. 200 means cancelled; 202 means processing. Query subscriptions to verify. Cancellation stops future charges and preserves paid rights until expiry.
