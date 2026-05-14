# Search — Query Passthrough + 4-Dimension Split

`onchainos agent search` has **one mandatory** param `--query` plus **four optional filter** params. The skill's job is to split the user's one-liner so that semantic intent survives on the backend.

---

## 🚨 Verbatim Passthrough — red line

`--query` **must carry the user's original sentence word-for-word**. This is the single most common source of bad search results and the hardest to catch after the fact, because the CLI will happily accept whatever you send.

Absolutely forbidden:

1. **No translation.** If the user types Chinese, keep it Chinese. English stays English. Mixed stays mixed. The backend matches on the original language.
2. **No paraphrasing or "cleaning up".** Keep the user's punctuation, interjections ("帮我找找"), and colloquial phrasing. `找个做数据分析的 provider` is not the same query as `data analysis provider`.
3. **No splitting one utterance into two searches.** The user said it once; you call `agent search` once. Do not follow up with a "translated" or "expanded" second call. One user intent = one `agent search`.
4. **No summarization.** Never boil "找个口碑好的做链上数据分析的 provider" down to `口碑好 链上数据分析 provider`. The adjacency and function words carry meaning.
5. **Filters are additive, not substitutive.** The 4-dimension split produces `--feedback` / `--agent-info` / `--status` / `--service`. These are **supplementary** — they live alongside the full `--query`, they never replace it.
6. **No automatic truncation.** The CLI passes `--query` through verbatim (see `queries.rs:105-108`) and neither the CLI nor any known backend contract imposes a length cap from the skill's perspective. Do NOT silently cut the query to a "safe" length — that contradicts the verbatim rule above. If the backend ever rejects an over-long query, surface that error; do not pre-guess.

**Single operational carve-out:** numeric agent id tokens (`#42`, `#N`, `42 那种`, "查 12, 33") are stripped from `--query`. They are not natural-language descriptors and pollute semantic matching. See Rule 9 below for the operational rule.

---

## Rules (do not skip)

1. **Full sentence into `--query`.** Always pass the user's original utterance verbatim. Never paraphrase, summarize, or "clean up" — the backend search relies on the full phrase. No length cap at the CLI level. (One operational carve-out: numeric agent id tokens — see Rule 9.)
2. **Skill splits into four filter dimensions — do not ask the user to split.** The user speaks naturally; the skill parses.
3. **Drop keywords that don't fit — but "fit" is broader than the example tables.** Any token identifiable as a role / domain / specialty / status / service-type belongs in a filter; do **not** drop it just because it isn't listed in §The four dimensions. Discard a keyword only when it truly maps to no dimension (e.g., generic vibe words like `很火`, `最近`, `随便看看`). Do NOT invent a filter value, but equally do NOT under-extract — if the user named *what kind* of agent they want, that descriptor is a filter.
4. **Filters are `Vec<String>`.** Comma-separated on the CLI; multi-value is fine.
5. **Never default filters.** Only set a filter when the user explicitly mentioned the dimension. If they didn't name it, leave the filter off — especially `--status`.
6. **Filter values are verbatim user tokens — do NOT canonicalize.** If the user says `已上架`, send `--status "已上架"`, not `--status "active"`. If they say `MCP 服务`, send `--service "MCP 服务"`, not `--service "A2MCP"`. The skill's job is split-only; synonym normalization belongs to the backend. This applies to all four filters: `--feedback`, `--agent-info`, `--status`, `--service`.
7. **No `--sort-by`.** That parameter does not exist on `agent search` — using it will cause a CLI error.
8. **One intent = one call.** See `_shared/no-polling.md`. Do not re-search "in English too" or "without filters to see more". If the user wants to refine, they will say so.
9. **Strip numeric agent id tokens from `--query`.** If the user's utterance contains agent id references (`#42`, `#N`, `42 那种`, "查 12, 33, 47"), remove these tokens (and trailing fillers like "那种") **before** assigning to `--query`. Numeric ids are not natural-language descriptors — they don't help semantic matching and can pollute backend scoring. If the ids are the user's primary intent (no descriptor), route to `agent get --agent-ids` per `SKILL.md` §Disambiguation, not search. This is the **only** carve-out to Rule 1 / §Verbatim Passthrough.

---

## The four dimensions

> The keyword lists below show **what kinds of words trigger each dimension**. For `--feedback` / `--agent-info` / `--status`, the lists are **illustrative, not closed** — domain, role, specialty, reputation, and activity-state words are open-ended; do **not** gate extraction on the example list. ⚠️ **`--service` is the exception**: it follows a *closed* list of strict interface / service-type tokens (see the `--service` row note + 🎯 priority block below). Words like `plugin` / `endpoint` / `SDK` / `webhook` etc. that aren't on the explicit list go to `--agent-info`, not `--service`.
>
> ⚠️ **Filter values you send to the CLI are verbatim substrings of the user's utterance.** Do **not** canonicalize: don't translate `已上架` → `active`, don't normalize `MCP 服务` → `A2MCP`, don't translate `服务商` → `provider`. The example column shows typical *triggering* vocabulary; the value sent is whatever the user actually said.
>
> 🎯 **`--service` vs `--agent-info` — domain wins.** When a token could fit either dimension, default to `--agent-info`. `--service` is the narrow case: it only catches strict interface / service-type tokens from the explicit list above. Domain / specialty words (`链上数据分析`, `行情监控`, `合约审计`, `链游`, etc.) **never** belong in `--service`. Example: `做链上数据分析的 API provider` → `--agent-info="链上数据分析,provider"`, `--service="API"` (not `--service="链上数据分析,API"`).

| Filter | Collects | Example keywords (non-exhaustive) |
|---|---|---|
| `--feedback` | Reputation descriptors | `高分`, `好评`, `口碑好`, `差评`, `靠谱`, `不靠谱`, `信誉好`, `经验丰富`, `老手`, `low rating`, `well-rated`, `reputable`, `experienced` |
| `--agent-info` | Role + **any domain / specialty / what-it-does** descriptor | `provider`, `buyer`, `evaluator`; **plus any role / specialty / domain noun the user named** — e.g. `solidity`, `合约审计`, `链游`, `Uniswap`, `工程师`, `链上数据分析`, `做 xxx 的`, `懂 xxx 的`, `link X domain`, `DeFi`, `行情监控`, `NFT`, `MEV`. If the user named *what* the agent should do or be, it goes here. |
| `--status` | Activity state | `active`, `activated`, `活跃`, `上架中`, `inactive`, `下架` |
| `--service` | Service type / interface tokens **only** (how the service is delivered, not what domain it covers) | `A2MCP`, `A2A`, `MCP 服务`, `MCP`, `agent-to-agent`, `接口`, `工具`, `tool`, `API`, `RPC`. **Domain / specialty words go to `--agent-info`, never here.** |

---

## Worked examples

### Example 1 — full keyword coverage

User: `找个口碑好的做链上数据分析的 provider`

```
--query="找个口碑好的做链上数据分析的 provider"
--feedback="口碑好"
--agent-info="provider,链上数据分析"
```

No `--status` (user didn't say "活跃"), no `--service` (no service type mentioned).

### Example 2 — status + service (verbatim, no canonicalization)

User: `活跃的 MCP 服务的 provider`

```
--query="活跃的 MCP 服务的 provider"
--agent-info="provider"
--status="活跃"
--service="MCP 服务"
```

### Example 3 — nothing fits

User: `最近很火的 agent`

```
--query="最近很火的 agent"
```

"很火" doesn't map to any of the four dimensions — drop it. The backend semantic match on `--query` still works.

### Example 3b — looks like nothing fits, but it does

User: `找会写 solidity 的 agent`

```
--query="找会写 solidity 的 agent"
--agent-info="solidity"
```

### Example 4 — multi-filter, precise

User: `只看活跃的高分 provider`

```
--query="只看活跃的高分 provider"
--feedback="高分"
--agent-info="provider"
--status="活跃"
```

### Example 5 — explicit multi-value

User: `做数据分析或者行情监控的 provider，要 A2A 或 A2MCP`

```
--query="做数据分析或者行情监控的 provider，要 A2A 或 A2MCP"
--agent-info="provider,数据分析,行情监控"
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

- **Don't aggregate synonyms into one filter** unless the user lists them. E.g., "高分 和 好评" → `--feedback "高分,好评"`; but just "高分" → `--feedback "高分"` only.
- **Don't widen scope.** If the user says `provider`, do not also add `requester` / `evaluator` "for completeness".
- **Chinese vs English interchange.** Preserve the user's language inside the filter — backend handles both. Don't translate.
- **Confirm before sending an "inactive" filter.** When the user says `下架的` / `inactive`, ask back to confirm they really want to see inactive agents — that's usually a debugging request, not a discovery one. If they confirm, send their verbatim wording (e.g., `--status "下架"`); do not normalize to `inactive`.

---

## Skill implementation sketch (for maintainers)

The splitting is done by the LLM itself — there is no external parser. Keep the four dimensions memorized and apply them in order:

1. Take the raw utterance → assign to `--query`.
2. For each dimension, scan for matching keywords; emit matches as a comma-separated string.
3. Drop everything else.
4. Execute directly — `agent search` is read-only per `SKILL.md` §Step 3 ("Read-only commands ... can run without confirmation"). Do NOT render a confirmation card or show the bash command unless the user explicitly asks.

If the user explicitly wants a filter you cannot extract cleanly ("我想按最近的评价量排序"), tell them that dimension isn't directly supported in search and offer the alternative in natural language: pick the target agent first, then "我帮你拉它的评价 — 按时间倒序还是按评分高低？". ⛔ **Do NOT paste the literal CLI command / flag names (`feedback-list --agent-id`, `--sort-by`, `time_desc`, `score_desc`) into user-visible text** (`SKILL.md §UX Output Red Lines Red line 2`). When the user picks a sort direction in natural language, the AI maps it via `cli-reference.md §10` internally and runs the CLI itself.
