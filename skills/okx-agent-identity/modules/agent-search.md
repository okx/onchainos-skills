# Search — Query Passthrough + 4-Dimension Split

`onchainos agent search` has **one mandatory** param `--query` plus **four optional filter** params. The skill's job is to split the user's one-liner so that semantic intent survives on the backend.

> After the CLI returns, read `core/display-lists.md §6` before rendering the results — it owns the field mapping, forbidden columns, and display-completeness rules.

---

## 🚨 Verbatim Passthrough — red line

`--query` **must carry the user's original sentence word-for-word**. This is the single most common source of bad search results and the hardest to catch after the fact, because the CLI will happily accept whatever you send.

Absolutely forbidden:

1. **No translation.** If the user types Chinese, keep it Chinese. English stays English. Mixed stays mixed. The backend matches on the original language.
2. **No paraphrasing or "cleaning up".** Keep the user's punctuation, interjections ("help me find"), and colloquial phrasing. `find me a provider doing data analysis` is not the same query as `data analysis provider`.
3. **No splitting one utterance into two searches.** The user said it once; you call `agent search` once. Do not follow up with a "translated" or "expanded" second call. One user intent = one `agent search`.
4. **No summarization.** Never boil "find a well-rated on-chain data analysis provider" down to `well-rated data analysis provider`. The adjacency and function words carry meaning.
5. **Filters are additive, not substitutive.** The 4-dimension split produces `--feedback` / `--agent-info` / `--status` / `--service`. These are **supplementary** — they live alongside the full `--query`, they never replace it.
6. **No automatic truncation.** The CLI passes `--query` through verbatim (see `queries.rs:105-108`) and neither the CLI nor any known backend contract imposes a length cap from the skill's perspective. Do NOT silently cut the query to a "safe" length — that contradicts the verbatim rule above. If the backend ever rejects an over-long query, surface that error; do not pre-guess.

**Single operational carve-out:** numeric agent id tokens (`#42`, `#N`, "look up 12, 33") are stripped from `--query`. They are not natural-language descriptors and pollute semantic matching. See Rule 9 below for the operational rule.

---

## Rules (do not skip)

1. **Full sentence into `--query`.** Always pass the user's original utterance verbatim. Never paraphrase, summarize, or "clean up" — the backend search relies on the full phrase. No length cap at the CLI level. (One operational carve-out: numeric agent id tokens — see Rule 9.)
2. **Skill splits into four filter dimensions — do not ask the user to split.** The user speaks naturally; the skill parses.
3. **Drop keywords that don't fit — but "fit" is broader than the example tables.** Any token identifiable as a role / domain / specialty / status / service-type belongs in a filter; do **not** drop it just because it isn't listed in §The four dimensions. Discard a keyword only when it truly maps to no dimension (e.g., generic vibe words like `trending`, `lately`, `whatever`). Do NOT invent a filter value, but equally do NOT under-extract — if the user named *what kind* of agent they want, that descriptor is a filter.
4. **Filters are `Vec<String>`.** Comma-separated on the CLI; multi-value is fine.
5. **Never default filters.** Only set a filter when the user explicitly mentioned the dimension. If they didn't name it, leave the filter off — especially `--status`.
6. **Filter values are verbatim user tokens — do NOT canonicalize.** If the user says `active`, send `--status "active"`, not a normalized enum. If they say `MCP service`, send `--service "MCP service"`, not `--service "A2MCP"`. The same applies across languages — Chinese filter values are passed verbatim as Chinese, never translated to English equivalents. The skill's job is split-only; synonym normalization belongs to the backend. This applies to all four filters: `--feedback`, `--agent-info`, `--status`, `--service`.
7. **No `--sort-by`.** That parameter does not exist on `agent search` — using it will cause a CLI error.
8. **One intent = one call.** See `_shared/no-polling.md`. Do not re-search "in English too" or "without filters to see more". If the user wants to refine, they will say so.
9. **Strip numeric agent id tokens from `--query`.** If the user's utterance contains agent id references (`#42`, `#N`, "look up 12, 33, 47"), remove these tokens **before** assigning to `--query`. Numeric ids are not natural-language descriptors — they don't help semantic matching and can pollute backend scoring. If the ids are the user's primary intent (no descriptor), route to `agent get --agent-ids` per `SKILL.md` §Intent → Sub-flow, not search. This is the **only** carve-out to Rule 1 / §Verbatim Passthrough.

---

## The four dimensions

> The keyword lists below show **what kinds of words trigger each dimension**. For `--feedback` / `--agent-info` / `--status`, the lists are **illustrative, not closed** — domain, role, specialty, reputation, and activity-state words are open-ended; do **not** gate extraction on the example list. ⚠️ **`--service` is the exception**: it follows a *closed* list of strict interface / service-type tokens (see the `--service` row note + 🎯 priority block below). Words like `plugin` / `endpoint` / `SDK` / `webhook` etc. that aren't on the explicit list go to `--agent-info`, not `--service`.
>
> ⚠️ **Filter values you send to the CLI are verbatim substrings of the user's utterance.** Do **not** canonicalize: don't normalize `MCP service` → `A2MCP`, don't rephrase `service provider` → `provider`. The example column shows typical *triggering* vocabulary; the value sent is whatever the user actually said.
>
> 🎯 **`--service` vs `--agent-info` — domain wins.** When a token could fit either dimension, default to `--agent-info`. `--service` is the narrow case: it only catches strict interface / service-type tokens from the explicit list above. Domain / specialty words (`on-chain data analysis`, `market monitoring`, `contract audit`, `blockchain gaming`, etc.) **never** belong in `--service`. Example: `on-chain data analysis API provider` → `--agent-info="on-chain data analysis,provider"`, `--service="API"` (not `--service="on-chain data analysis,API"`).

| Filter | Collects | Example keywords (non-exhaustive) |
|---|---|---|
| `--feedback` | Reputation descriptors | `high score`, `good reviews`, `well-regarded`, `low rating`, `reliable`, `unreliable`, `experienced`, `well-rated`, `reputable` |
| `--agent-info` | Role + **any domain / specialty / what-it-does** descriptor | `provider`, `buyer`, `evaluator`; **plus any role / specialty / domain noun the user named** — e.g. `solidity`, `contract audit`, `blockchain gaming`, `Uniswap`, `engineer`, `on-chain data analysis`, `DeFi`, `market monitoring`, `NFT`, `MEV`. If the user named *what* the agent should do or be, it goes here. |
| `--status` | Activity state | `active`, `activated`, `listed`, `inactive`, `delisted` |
| `--service` | Service type / interface tokens **only** (how the service is delivered, not what domain it covers) | `A2MCP`, `A2A`, `MCP service`, `MCP`, `agent-to-agent`, `interface`, `tool`, `API`, `RPC`. **Domain / specialty words go to `--agent-info`, never here.** |

---

## Worked examples

### Example 1 — full keyword coverage

User: `find a well-rated provider for on-chain data analysis`

```
--query="find a well-rated provider for on-chain data analysis"
--feedback="well-rated"
--agent-info="provider,on-chain data analysis"
```

No `--status` (user didn't say "active"), no `--service` (no service type mentioned).

### Example 2 — status + service (verbatim, no canonicalization)

User: `active MCP service provider`

```
--query="active MCP service provider"
--agent-info="provider"
--status="active"
--service="MCP service"
```

### Example 3 — nothing fits

User: `trending agent lately`

```
--query="trending agent lately"
```

"trending" doesn't map to any of the four dimensions — drop it. The backend semantic match on `--query` still works.

### Example 3b — looks like nothing fits, but it does

User: `find an agent that writes solidity`

```
--query="find an agent that writes solidity"
--agent-info="solidity"
```

### Example 4 — multi-filter, precise

User: `only show active highly-rated providers`

```
--query="only show active highly-rated providers"
--feedback="highly-rated"
--agent-info="provider"
--status="active"
```

### Example 5 — explicit multi-value

User: `provider doing data analysis or market monitoring, needs A2A or A2MCP`

```
--query="provider doing data analysis or market monitoring, needs A2A or A2MCP"
--agent-info="provider,data analysis,market monitoring"
--service="A2A,A2MCP"
```

### Example 6 — English query

User: `find a highly-rated evaluator with DeFi experience`

```
--query="find a highly-rated evaluator with DeFi experience"
--feedback="highly-rated"
--agent-info="evaluator,DeFi"
```

### Example 7 — very long query

User pastes a 500-char rant. Send it verbatim; do not pre-truncate. If the backend returns an error like "query too long" or similar, surface the backend message to the user and ask whether they want to shorten — do not auto-shorten.

---

## Boundary rules

- **Don't aggregate synonyms into one filter** unless the user lists them. E.g., "high score and good reviews" → `--feedback "high score,good reviews"`; but just "high score" → `--feedback "high score"` only.
- **Don't widen scope.** If the user says `provider`, do not also add `requester` / `evaluator` "for completeness".
- **Language passthrough.** Preserve the user's exact wording inside the filter — backend handles both languages. Don't translate.
- **Confirm before sending an "inactive" filter.** When the user says `inactive` / `delisted`, ask back to confirm they really want to see inactive agents — that's usually a debugging request, not a discovery one. If they confirm, send their verbatim wording; do not remap to a different term.
- **⚠️ Ownership word + descriptor → `agent get`, NOT `agent search`.** If the user says "my DeFi agents" / "my solidity provider" / "my agent doing X" — `agent search` has no owner filter and cannot be scoped to the current user. Instead: run `agent get` (default mode, no `--agent-ids`) to fetch the caller's own agents, then **client-side filter** the list to rows matching the descriptor. Never route ownership-word queries to `agent search`.
- **Explicit numeric ids → `agent get --agent-ids`, NOT `agent search`.** "look up #42" / "fetch 42 and 58" → `agent get --agent-ids <ids>`. Direct id lookup, no semantic scoring. (Per Rule 9, strip id tokens if the user's utterance is mainly a descriptor with an incidental id reference.)

---

## Unsupported filter requests

When a user asks for a sort or filter dimension that doesn't exist in `agent search` (e.g. "I'd like to sort by the most recent review count"), tell them it isn't directly supported and offer the alternative in natural language: pick the target agent first, then "I'll pull up their reviews — sorted by time or by rating?". ⛔ Never paste CLI flag names (`feedback-list --agent-id`, `--sort-by`, `time_desc`, `score_desc`) into user-visible text (SKILL.md Red line 2). Map the user's natural-language sort preference to the flag internally via `core/cli-search-feedback.md §10`.
