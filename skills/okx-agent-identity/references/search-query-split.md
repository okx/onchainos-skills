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

---

## Rules (do not skip)

1. **Full sentence into `--query`.** Always pass the user's original utterance verbatim. Never paraphrase, summarize, or "clean up" — the backend search relies on the full phrase. No length cap at the CLI level.
2. **Skill splits into four filter dimensions — do not ask the user to split.** The user speaks naturally; the skill parses.
3. **Drop keywords that don't fit.** If a keyword doesn't map into one of the four filters, discard it silently. Do NOT invent a filter value.
4. **Filters are `Vec<String>`.** Comma-separated on the CLI; multi-value is fine.
5. **Never default filters.** Only set a filter when the user explicitly or strongly implies it. Especially `--status`: only set `active` when the user says "只看活跃" / "active only" / similar.
6. **No `--sort-by`.** That parameter does not exist on `agent search` — using it will cause a CLI error.
7. **One intent = one call.** See `_shared/no-polling.md`. Do not re-search "in English too" or "without filters to see more". If the user wants to refine, they will say so.

---

## The four dimensions

| Filter | Collects | Typical keywords |
|---|---|---|
| `--feedback` | Reputation descriptors | `高分`, `好评`, `口碑好`, `差评`, `low rating`, `well-rated` |
| `--agent-info` | Role + domain descriptors | `provider`, `buyer`, `evaluator`, `做 xxx 的`, `link X domain`, `DeFi`, `数据分析` |
| `--status` | Activity state | `active`, `activated`, `活跃`, `上架中`, `inactive`, `下架` |
| `--service` | Service type tokens | `A2MCP`, `A2A`, `MCP 服务`, `agent-to-agent`, concrete service domain words |

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

### Example 2 — status + service

User: `活跃的 MCP 服务商`

```
--query="活跃的 MCP 服务商"
--status="active"
--service="A2MCP"
```

### Example 3 — nothing fits

User: `最近很火的 agent`

```
--query="最近很火的 agent"
```

"很火" doesn't map to any of the four dimensions — drop it. The backend semantic match on `--query` still works.

### Example 4 — multi-filter, precise

User: `只看活跃的高分 provider`

```
--query="只看活跃的高分 provider"
--feedback="高分"
--agent-info="provider"
--status="active"
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
- **Do not map `--status inactive` automatically** even if the user says "下架的"; ask back to confirm they really want to see inactive agents — that's usually a debugging request, not a discovery one.

---

## Skill implementation sketch (for maintainers)

The splitting is done by the LLM itself — there is no external parser. Keep the four dimensions memorized and apply them in order:

1. Take the raw utterance → assign to `--query`.
2. For each dimension, scan for matching keywords; emit matches as a comma-separated string.
3. Drop everything else.
4. Render the command, confirm with the user, then execute.

If the user explicitly wants a filter you cannot extract cleanly ("我想按最近的评价量排序"), tell them that dimension isn't supported on `agent search` and suggest `feedback-list <agentId>` with `--sort-by time_desc` (按时间倒序) or `score_desc` (按分数倒序) after picking the target. Full natural-language mapping → `cli-reference.md` §10.
