# Role: provider (服务提供商 / Agent Service Provider — ASP)

> Registers an ASP identity **with at least one service**. Longest Q&A — take it one step at a time.

## STRICT — one question per turn

No listing "请提供 1. 名字 2. 描述 3. 服务名称 ..." / "Please provide 1. Name 2. Description 3. Service Name ...". Every field, including every service sub-field, is a separate turn in the user's language.

Field definitions live in `field-specs.md`. Inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese; `Purpose / Visibility / Please note / Example` for English) in the user's language only, so they don't need to read a separate file to answer.

## Phase 1 — identity Q&A

### Phase 1 preview (render BEFORE Q1)

Once role is `provider` and pre-check resolved (either "no existing provider" or user chose "1. 再开一个新的服务提供商" on the pre-check numbered prompt), render the Phase-1 preview, then start Q1.

Chinese:
```
好，开始注册新服务提供商身份。先收集身份基本信息：
  1. 名称
  2. 描述
  3. 头像（可选）
（服务列表会在身份信息确认后再继续收集。）
```

English:
```
Got it — starting a new Agent Service Provider (ASP) registration. First we'll collect identity info:
  1. Name
  2. Description
  3. Profile photo (optional)
(The service list is collected after identity is confirmed.)
```

The preview is declarative; Q1 follows after a blank line. See `role-playbook.md §STRICT — Preview ≠ multi-field ask`.

### Q&A

> ⛔ **Field values come from the user, not from elsewhere.** Each of `name` / `description` / `picture` MUST come from the user's literal reply to the matching Q below (or from their literal text in a §One-shot capture multi-field message). **Never** pre-fill `name` from `userEmail`, USER.md, CLAUDE.md, the wallet account name, the XMTP sender, a Telegram handle, or any session metadata. Do **not** generate "Jim 的卖家" / "Alice 的 provider" / "<email-prefix> 的 ASP" style templates. See `SKILL.md §Red line 6` for the complete forbidden-sources list and table of anti-patterns.

The `Q1 / Q2 / Q3` labels in the column below are **maintainer-internal only** — they help this document index questions but **MUST NOT** appear in the prompt strings the AI sends to the user. The prompts in the Chinese/English columns are the literal text rendered to the user; they carry no `Q1：` / `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` (no Q/S/Phase leakage) and `references/ux-lexicon.md` for the canonical rule. Each prompt inlines the four-segment field spec from `field-specs.md` in the user's language only. Skip any Q whose field was already captured via §One-shot capture.

| Q | Chinese prompt | English prompt | Validation |
|---|---|---|---|
| Q1 | `这个服务提供商身份叫什么名字？` + 4 segments | `What's the name of this ASP?` + 4 segments | non-empty, CN ≤ 30 文字 / EN ≤ 64 chars |
| Q2 | `用一句话描述这个服务提供商身份。` + 4 segments | `Describe this ASP in a sentence.` + 4 segments | non-empty, CN ≤ 500 文字 / EN ≤ 500 chars |
| Q3 | `头像呢？用默认还是上传一张？` + Choice prompt (see `avatar-upload.md`) | `Profile photo? Use the default or upload one?` + Choice prompt | — |

**Strict phase boundary**: Phase 1 only captures `name` / `description` / `picture`. Even if the user mentions service info ("收 10 USDT"), do NOT capture it here — see `SKILL.md §One-shot capture rule 4`.

After Q3 answered, render the Phase-1 confirmation card (identity only, no service rows yet — but note: that is **not** the final `create` — final confirmation happens after Phase 2). Or alternatively, hold identity in-memory and show one combined confirmation at the end of Phase 2. **This skill does the latter**: one final confirmation card after all services are collected. Phase-1 end transitions directly to Phase-2 preview.

## Phase 2 — service Q&A (loop once per service)

> ⛔ **No fabricated services. Ever.** Every `service.*` subfield (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) MUST come from the user's literal in-conversation reply to the matching per-service Q. When the user says "帮我写几个 service" / "随便几个" / "示例就行" / "你帮我想" / "you fill it in" / "make some up" — **refuse and re-prompt** asking what they actually want to offer (see §Good/bad cases row 3 for the canonical decline). Do not infer `servicetype` from the service name ("听起来像 MCP" — wrong, the user must choose Q3 explicitly). Do not pick a default `fee`. Do not invent an `endpoint`. Do not pipe a user-pasted JSON blob straight to the CLI (re-confirm field-by-field). Full forbidden-action list + anti-patterns in `SKILL.md §Red line 6`.

### Phase 2 preview (render BEFORE the first service's Q1)

Once Phase 1 is complete, render the Phase-2 preview **once** (not repeated for subsequent services in the loop). Then start service[1]'s Q1.

Chinese:
```
身份信息收到。接下来给这个服务提供商身份配服务，每条服务会问：
  1. 名称
  2. 描述
  3. 类型（API 接口式服务 / agent（智能体）通信式服务 — 后面会展开问）
  4. 价格（API 接口式服务必填，agent 通信式服务选填，单位 USDT）
  5. 接口地址（仅 API 接口式服务需要）
加完一条后会问是否继续加下一条。可以加一条或多条。
```

English:
```
Identity info captured. Next we'll add services for this ASP. For each service we'll ask:
  1. Name
  2. Description
  3. Type (API-interface service / agent-to-agent service — explained again when we ask)
  4. Fee in USDT (required for API-interface, optional for agent-to-agent)
  5. Endpoint (API-interface service only)
After each service we'll ask whether to add another. One or more services, your choice.
```

Preview is declarative, not imperative — see `role-playbook.md §STRICT`.

### Per-service Q&A

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee is required for A2MCP and optional for A2A (when an A2A user skips, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` without `skip_serializing_if`); endpoint is only needed for A2MCP.

The `Q1 / Q2 / ... / Q5` column labels in the per-service tables below are **maintainer-internal indexes only** — they reset per service iteration but **MUST NOT** appear as prefixes in the prompt strings the AI sends to the user. The prompts in the Chinese/English columns are the literal text rendered to the user; they carry no `Q1：` / `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` and `references/ux-lexicon.md`. The preamble for service `[N]` ("接下来是服务[N]：" / "Now service [N]:") contextualizes which service is being collected. The loop gate is a numbered-options pattern, not a Q-labelled question.

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

#### Suggestion-as-prompt carve-out (Q1 + Q3, opt-in)

This is the **single carve-out** to `SKILL.md §Red line 6` "field values come from the user, not from elsewhere": when the user, in an earlier turn of THIS conversation, mentioned a candidate value for the service `name` or `servicetype` (e.g. Phase 1 ask "建个 provider 卖天气查询的服务" — they named "天气查询" as a likely service-name candidate, or "API 接口式服务" as a likely type), the Q1 / Q3 prompt **MAY** quote that mention inline as a default for the user to confirm-or-override. This is **suggestion text in the prompt**, NOT auto-fill — the user's reply this turn is still the authoritative value; if they ignore the suggestion and type something else, use what they typed.

Canonical examples (render exactly — **no `Q1：` / `Q3：` prefix** per `SKILL.md §UX Output Red Lines Red line 3`):

- **Q1 name**: `这个服务叫什么名字？（你刚提到「天气查北京」，确认就是它吗？或想改？）` / `What's the name of this service? (You mentioned "weather lookup for Beijing" earlier — confirm or change?)`
- **Q3 servicetype** when user said `A2A` / `agent 互调` / `agent-to-agent` / `agent 通信` in Phase 1: `服务类型？（你刚说想要 agent（智能体）通信式服务（议价 / 灵活协作），确认 2 即可；想改回 1 也行。）`
- **Q3 servicetype** when user said `A2MCP` / `MCP 服务` / `API 接口` in Phase 1: `服务类型？（你刚说想要 API 接口式服务（按次调用、固定价格），确认 1 即可；想改回 2 也行。）`

⛔ For Q3 specifically: when quoting the user's earlier type mention, **map their term to the long-form-with-gloss** per `references/ux-lexicon.md §Service-type` Pattern A — Q3 is a Pattern-A teaching context, so the short form alone is not enough on first encounter. **Never** echo the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK; output is not). Full source-of-truth rule: `SKILL.md §Sub-flows §Core Flow §Phase 2 Q1 UX guidance Option A`.

⛔ The carve-out **only** applies when the candidate value appeared as the user's own typed text in an earlier turn of this conversation. It does **NOT** legitimize pulling from `userEmail`, USER.md, CLAUDE.md, XMTP sender, the wallet account name, or any other session-metadata source — those remain forbidden per Red line 6.

Chinese per-service Q&A (render `接下来是服务[N]：` as a one-line preamble before Q1):

| Step | 问用户 (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `这个服务叫什么名字？` + 4 segments (see `field-specs.md`) | non-empty, CN ≤ 30 文字 | `name` |
| Q2 | `详细介绍一下这项服务。` + 4 segments | non-empty, CN ≤ 500 文字 | `servicedescription` |
| Q3 | `这项服务是哪种类型？` + numbered-options (`SKILL.md §Choice prompts`):<br>&nbsp;&nbsp;`1. API 接口式服务（按次调用、固定价格，标准 MCP（标准调用接口）接口）`<br>&nbsp;&nbsp;`2. agent（智能体）通信式服务（双方协商定价 / 灵活协作；价格默认私下谈，可选填上链（写入区块链）参考价）`<br>`回复 1 或 2。`<br>**Pattern A (long form inline) per `references/ux-lexicon.md §Service-type`** — Q3 is a teaching prompt (user is being asked to choose, so they need the gloss to make the choice); the option text above uses the long form with gloss inside the parenthetical. This satisfies the first-occurrence-gloss requirement on its own; **no separate footnote needed below this prompt**. Subsequent renderings in the same conversation (e.g. the §3 confirmation card cell) MAY use the short form `API 接口` / `agent 互调`.<br>**Maintainer-internal mapping (NOT shown to user):** receive `1` / `2` and map to wire enum `1→A2MCP` / `2→A2A`; CLI has no numeric alias, sending raw `1` would `bail invalid servicetype`. ⛔ Never render the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK — if the user types `A2A` we accept it and map internally; output never carries the raw enum). | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if `A2MCP` → `每次调用收多少 USDT？（最多六位小数，例如 1.234567 / 10 / 0.5 / 0）` + 4 segments ; if `A2A` → `这项服务的参考价是多少 USDT？（选填，最多六位小数；不填表示价格由双方自行协商。直接回车 / 回复 "跳过" / "skip" 即可跳过）` + 4 segments | A2MCP: number ≥ 0，最多六位小数。**Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})?$`，非空必填。A2A: 空 或 满足同一 pattern | `fee` (A2A 跳过时仍会以 `"fee":""` 进入 wire payload——`models.rs:21` 的 `fee: String` 没有 `skip_serializing_if`。skill 渲染时按 `空 → 免费/free`；后端是否区分"空串 vs 缺失键"由产品 spec 决定，本地代码不可证实) |
| Q5 | if `A2MCP` → `MCP（标准调用接口）服务地址是什么？必须 https:// 开头，且公网可达（其他 agent 会通过公网来调用你的服务）。` + 4 segments ; if `A2A` → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (A2A 即使用户给了 CLI 也会清掉，见 `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt (no `Q` label, it's a flow switch):<br>`还要再加一项服务吗？`<br>&nbsp;&nbsp;`1. 再加一项`<br>&nbsp;&nbsp;`2. 不加了，到此为止`<br>`回复 1 或 2。` | reply 1 or 2 | — |

English per-service Q&A (render `Now service [N]:` as a one-line preamble before Q1):

| Step | Ask the user (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `What's the name of this service?` + 4 segments | non-empty, EN ≤ 64 chars | `name` |
| Q2 | `Describe this service.` + 4 segments | non-empty, EN ≤ 500 chars | `servicedescription` |
| Q3 | `Which type is this service?` + numbered-options:<br>&nbsp;&nbsp;`1. API-interface service (pay-per-call, fixed price; standard MCP (standard call protocol) interface)`<br>&nbsp;&nbsp;`2. agent-to-agent service (negotiated pricing / flexible collaboration; pricing is off-chain by default, optional on-chain reference price)`<br>`Reply 1 or 2.`<br>**Pattern A (long form inline) per `references/ux-lexicon.md §Service-type`** — Q3 is a teaching prompt (user is choosing, so they need the gloss to decide); the option text above uses the long form with gloss inside the parenthetical. This satisfies the first-occurrence-gloss requirement on its own; **no separate footnote needed below this prompt**. Subsequent renderings in the same conversation (e.g. the §3 confirmation card cell) MAY use the short form `API service` / `agent-to-agent`.<br>**Maintainer-internal mapping (NOT shown to user):** map reply `1→A2MCP` / `2→A2A` before invoking the CLI — the CLI has no numeric alias and sending raw `1` bails with `invalid servicetype`. ⛔ Never render the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK — if the user types `A2A` we accept it and map internally; output never carries the raw enum). | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if A2MCP → `Price per call in USDT? (up to 6 decimal places, e.g., 1.234567 / 10 / 0.5 / 0)` + 4 segments ; if A2A → `Reference price in USDT for this service? (optional, up to 6 decimal places; leave empty to allow direct negotiation between parties. Press enter or reply "skip" to skip)` + 4 segments | A2MCP: number ≥ 0, ≤ 6 decimal places. **Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})?$`, must be non-empty. A2A: empty OR matches the same pattern | `fee` (when A2A is left empty, the wire payload still carries `"fee": ""` — `models.rs:21` `fee: String` has no `skip_serializing_if`. The skill renders empty fee as `免费` / `free`; whether the backend distinguishes empty-string from absent-key is governed by the product spec and cannot be verified from this repo) |
| Q5 | if A2MCP → `What's the MCP (standard call protocol) endpoint URL? Must start with https:// and be reachable from the public internet (other agents will connect to your service over the public internet).` + 4 segments ; if A2A → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt:<br>`Want to add another service?`<br>&nbsp;&nbsp;`1. Add another`<br>&nbsp;&nbsp;`2. No more, finish here`<br>`Reply 1 or 2.` | reply 1 or 2 | — |

After each service is collected, echo back a one-line summary in the user's language before the loop gate:
- 中文：`已记录 服务[1]：TVL Query（API 接口，10 USDT，https://…）。`
- English: `Recorded Service [1]: TVL Query (API service, 10 USDT, https://…).`

## Good / bad cases

| User input | Action |
|---|---|
| "我要做数据分析服务，收 10 USDT"（**在 Phase 1 说的**） | Do **NOT** capture `fee=10` at Phase 1 — phase boundary is strict (`SKILL.md §One-shot capture` rule 4). Continue Phase 1 Q&A; when Phase 2 starts fresh, ask Q3 (`servicetype`) first, then Q4 (`fee`) where the user can re-supply 10 if still relevant. |
| "我要做数据分析服务，收 10 USDT"（**在 Phase 2 的某条服务中说的**） | Capture `fee=10` when Q4 asks it; still confirm `servicetype` at Q3 first. |
| "帮我写几个 service" | Refuse to fabricate. Ask what they actually want to offer. |
| User pastes JSON blob | Thank them, but re-confirm **field by field** — typos in `servicetype` are the #1 cause of create failures. Do not pipe JSON straight to the CLI. |
| "endpoint 是 http://..." | Reject. Ask for HTTPS. |
| "API 接口式服务 Fee 免费" | Accept `0` but warn: "API 接口式服务 0 USDT 等同于免费入口，后续不能再按量收费。" |
| User answers multiple service fields in one sentence | Parse what you can, but next turn still asks the remaining fields individually. |
| "服务类型 HTTP" / "service type HTTP" | Reject politely and re-render the Q3 numbered prompt verbatim (see `SKILL.md §Choice prompts`) — do not fabricate a new phrasing. |

## Confirmation

> ⛔ Mandatory before invoking the CLI — applies to both single-service and multi-service provider creates. See `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)` for the canonical rule + the rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") that do **NOT** bypass it.

Two-column table (`display-formats.md` §Create/Update Diff), services numbered inline. Render in the user's language — pick ONE variant.

> ⛔ The `<user-provided-endpoint>` token in the example below is a **doc-only placeholder** — at runtime substitute it with the **literal URL the user gave you in Phase 2 Q5** (or, on `update`, the new value the user just typed). **Never** copy any `https://api.example.com/...` / `https://cdn.example.com/...` / any other sample URL from these docs into the user's confirmation card. See `display-formats.md` top "URL literals are doc-only" rule.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 服务提供商 |
| 名字 | DeFi Analyzer |
| 描述 | 链上数据分析与收益模拟。 |
| 头像 | 默认 |
| 服务[1] 名称 | TVL Query |
| 服务[1] 描述 | 通过 MCP 按链查询协议 TVL。 |
| 服务[1] 类型 | API 接口 |
| 服务[1] 价格 | 10 USDT |
| 服务[1] 接口地址 | `<user-provided-endpoint>` |
| 服务[2] 名称 | Yield Check |
| 服务[2] 类型 | agent 互调 |
| 服务[2] 价格 | （未填，双方自行协商） |

> 服务类型：API 接口 = 按次调用、固定价格；agent 互调 = 议价 / 灵活协作。
> 确认无误回复 "执行" 即可。

**Maintainer note (not rendered):** for `agent 互调` (servicetype=A2A) the price row renders the user's value verbatim (e.g., `5 USDT`) when supplied, otherwise `（未填，双方自行协商）`. Do NOT render `A2A` to the user in this card — the canonical type cell shows `agent 互调` per `display-formats.md` top-level "Service-type rendering" rule.

English variant:

| Field | Value |
|---|---|
| Role | Agent Service Provider (ASP) |
| Name | DeFi Analyzer |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | default |
| Service [1] Name | TVL Query |
| Service [1] Description | Query protocol TVL by chain via MCP. |
| Service [1] Type | API service |
| Service [1] Fee | 10 USDT |
| Service [1] Endpoint | `<user-provided-endpoint>` |
| Service [2] Name | Yield Check |
| Service [2] Type | agent-to-agent |
| Service [2] Fee | (skipped — negotiated directly) |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.
> Reply "execute" to run it.

**Maintainer note (not rendered):** for `agent-to-agent` (servicetype=A2A) the Fee row renders the user's value verbatim (e.g., `5 USDT`) when supplied, otherwise `(skipped — negotiated directly)`. Do NOT render `A2A` to the user in this card — the canonical type cell shows `agent-to-agent` per `display-formats.md` top-level "Service-type rendering" rule.

Service-field **labels in the card** are localized (see mapping table in `display-formats.md §Create/Update Diff`: `名称 / 描述 / 类型 / 价格 / 接口地址` ↔ `Name / Description / Type / Fee / Endpoint`). The **JSON keys actually sent to the CLI** (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) are lowercase schema per `models.rs::AgentService` — they only show up in the raw bash command, which we render only if the user explicitly asks.

**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks ("把命令给我看" / "show me the CLI").

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role provider \
  --name "<name>" \
  --description "<description>" \
  --service '[{"name":"…","servicedescription":"…","servicetype":"A2MCP","fee":"10","endpoint":"https://…"}, {"name":"…","servicedescription":"…","servicetype":"A2A","fee":""}, {"name":"…","servicedescription":"…","servicetype":"A2A","fee":"5"}]' \
  [--picture "<url>"]
```

## ⛔ Post-success — MANDATORY template (do NOT paraphrase)

> ⛔ **After the visible line, this turn is NOT over.** → proceed to `SKILL.md §Operation Flow Step 5` (which routes to `§Step 6` for the unconditional comm-init handoff). Full rules (anti-skip clauses, runtime self-gating, decline carve-out) live in Step 6 — not duplicated here.

Render **one visible line** using the template below — **verbatim except for the `#<id>` substitution rule**. Then follow the §Agent directive block (internal — not rendered to the user). Paraphrasing, adding fields (txHash, agentList, activeClients, refresh-agents output), omitting fields, adding follow-up questions, or summarizing the CLI's other JSON output are all violations of `SKILL.md §⛔ MANDATORY post-execute gate`.

### Visible line (template)

Pick the variant matching the user's language. Render **one line, declarative, no question mark, no pre-announcement of the chat handoff** (the chat flow is a silent no-op outside an OpenClaw runtime; pre-announcing would mislead users in Claude Code / Claude Desktop):

- Chinese: `服务提供商身份 #<id> 注册完成，默认已上架可以接单。想看看市场上同类服务提供商长什么样、或确认你自己的曝光，跟我说"找做 ... 的服务提供商"我帮你搜；否则就等用户来找你。`
- English: `ASP identity #<id> is live and active by default. Say "find ASPs doing X" if you want me to scan the marketplace for similar agents or confirm your exposure; otherwise just wait for matching tasks.`

**`#<id>` substitution rule** (per `display-formats.md` top, `#<id>` placeholder rule, **with provider-specific carve-out**):

- The legitimate sources of `#<id>` for this post-success line are, in priority order:
  1. **CLI response (direct):** the `create` call's response directly contains the new agent id — substitute it verbatim.
  2. **Post-create envelope diff:** the response envelope is double-layer (see `cli-reference.md §3`), so the filter is **wrapper-level**, not agent-row-level — **two steps, in order**: (a) locate the single wrapper in `envelope.agentList.list[*]` whose `list[*].ownerAddress == <currently selected XLayer wallet address>` (the address that signed this `create`), then (b) inside **that wrapper's** `agentList[*]` only, **diff against the pre-check `agent get` snapshot** captured by §⛔ MANDATORY pre-check gate — pick the agentId that's **newly present** (in the post-create envelope but not in the pre-check snapshot). This works regardless of whether pre-check returned K=0 or K≥1 existing providers; the diff isolates the freshly-minted id either way. ❌ Do NOT write the filter as `agentList[*].ownerAddress == ...` — agent rows have no `ownerAddress` field; that phrasing always misses. See `cli-reference.md §1` "Finding the newly-minted `agentId`" for the canonical algorithm.
  3. (Future) a follow-up `agent get` in a later turn — irrelevant for this immediate response.
- ⚠️ **Provider-specific danger zone — DO NOT pick any id directly from the pre-check list as `#<id>`.** Pre-check reflects state *before* this `create`, so its rows are all older providers, never the newly minted one. Source 2 above is **diff-based** (post-create envelope MINUS pre-check snapshot), not "borrow from pre-check"; it picks the id that's in the post-create envelope but **not** in the pre-check snapshot. Conflating the two is a real failure mode — the agent that does "I see provider #88 in pre-check, must be the new one" instead of running the diff will surface an older provider's id as if it were freshly created, which is misleading.
- If **both** source 1 (CLI direct id) and source 2 (envelope diff) miss — i.e. CLI returned `txHash` only **AND** the post-create `agentList` segment is also absent (WS + HTTP both failed, per `cli-reference.md §1`) **OR** the diff yielded no new candidate under the current wallet — **omit the `#<id> ` substring entirely**: do NOT render `#`, `#<id>`, `# ?`, do NOT invent a number, do NOT borrow from the pre-check list. Fallback lines:
  - Chinese: `服务提供商身份注册完成，默认已上架可以接单。想看看市场上同类服务提供商长什么样跟我说"找做 ... 的服务提供商"我帮你搜；否则就等用户来找你。`
  - English: `ASP identity is live and active by default. Say "find ASPs doing X" if you want me to scan the marketplace; otherwise wait for matching tasks.`

**Create returns active by default** / **Create 默认返回 active** — no need to follow up with `agent activate`. `activate` is only for users who previously ran `deactivate` and now want to re-publish.

Do NOT mention the `okx-agent-chat/after-agent-list-changed.md` path to the user in the visible line — the same-turn handoff below loads that skill's own prompt, which decides on its own whether to surface anything (silent in non-OpenClaw runtimes).

### ❌ Anti-pattern (real incident, jobId=961) → ✅ Correct

❌ Agent paraphrased:
> "✅ 第二个 provider 已上链 / agentId 961 / 4 个活跃客户端 / 你现在有 4 个 agent"

Why this is a violation of `SKILL.md §⛔ MANDATORY post-execute gate`:

- Not the documented template wording — "已上链" / "第二个 provider" / "4 个活跃客户端" / "你现在有 4 个 agent" are all paraphrases.
- Mentions `活跃客户端` — that's internal `xmtp_refresh_agents` output, not user-relevant. Internal CLI fields (`agentList`, `activeClients`, refresh-agents counts, the full tx receipt) are NEVER user-facing; the template defines exactly what to expose.
- Re-renders / counts the agent list (`你现在有 4 个 agent`) — violates the §Agent directive's "do NOT run `agent get`" rule. Even if the count is technically derivable from a prior response, surfacing it on the post-success line is template drift.
- The natural-language "想看市场上同类服务提供商就跟我说…" half is missing — the suggested next action got dropped in favor of the inflated-success preamble.
- Uses the raw English `provider` and the `agent search` CLI literal in Chinese user-visible text — violates `SKILL.md §UX Output Red Lines Red lines 1/2/4` and `references/ux-lexicon.md` (Chinese must localize role term to `服务提供商`, never paste CLI command for user to run).

✅ Correct (with id):
> 服务提供商身份 #961 注册完成，默认已上架可以接单。想看看市场上同类服务提供商长什么样、或确认你自己的曝光，跟我说"找做 ... 的服务提供商"我帮你搜；否则就等用户来找你。

✅ Correct (id unknown, txHash-only return):
> 服务提供商身份注册完成，默认已上架可以接单。想看看市场上同类服务提供商长什么样跟我说"找做 ... 的服务提供商"我帮你搜；否则就等用户来找你。

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. → proceed to `SKILL.md §Operation Flow Step 5` — the provider row routes directly to `§Step 6` (comm-init), which loads `/skills/okx-agent-chat/after-agent-list-changed.md` Execution Flow in the same response. A fresh ASP was added and is immediately discoverable, so the OpenClaw runtime side needs sync.

Skip / decline carve-outs and the runtime self-gating contract are owned by Step 6 — not duplicated here.

**Do NOT** run `agent get` or poll status after create (that is about querying chain state — different from the Step 5 → Step 6 chain above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to Phase 2 Q1 of service[1]. If `missing required field in --service: name` surfaces, return to the specific Q (see `troubleshooting.md`). Never silently retry with a filler value.
