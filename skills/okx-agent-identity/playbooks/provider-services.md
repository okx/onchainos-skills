# Provider — Phase 2: Service Q&A

> Part of `playbooks/provider.md`. Called after Phase 1 (identity Q&A) is complete.
> Contains the per-service Q&A loop — one service at a time, five fields each.

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

Preview is declarative, not imperative — see `playbooks/README.md §STRICT`.

### Per-service Q&A

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee is required for A2MCP and optional for A2A (when an A2A user skips, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` without `skip_serializing_if`); endpoint is only needed for A2MCP.

The `Q1 / Q2 / ... / Q5` column labels in the per-service tables below are **maintainer-internal indexes only** — they reset per service iteration but **MUST NOT** appear as prefixes in the prompt strings the AI sends to the user. The prompts in the Chinese/English columns are the literal text rendered to the user; they carry no `Q1：` / `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` and `core/ux-lexicon.md`. The preamble for service `[N]` ("接下来是服务[N]：" / "Now service [N]:") contextualizes which service is being collected. The loop gate is a numbered-options pattern, not a Q-labelled question.

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

#### Suggestion-as-prompt carve-out (Q1 + Q3, opt-in)

This is the **single carve-out** to `SKILL.md §Red line 6` "field values come from the user, not from elsewhere": when the user, in an earlier turn of THIS conversation, mentioned a candidate value for the service `name` or `servicetype` (e.g. Phase 1 ask "建个 provider 卖天气查询的服务" — they named "天气查询" as a likely service-name candidate, or "API 接口式服务" as a likely type), the Q1 / Q3 prompt **MAY** quote that mention inline as a default for the user to confirm-or-override. This is **suggestion text in the prompt**, NOT auto-fill — the user's reply this turn is still the authoritative value; if they ignore the suggestion and type something else, use what they typed.

Canonical examples (render exactly — **no `Q1：` / `Q3：` prefix** per `SKILL.md §UX Output Red Lines Red line 3`):

- **Q1 name**: `这个服务叫什么名字？（你刚提到「天气查北京」，确认就是它吗？或想改？）` / `What's the name of this service? (You mentioned "weather lookup for Beijing" earlier — confirm or change?)`
- **Q3 servicetype** when user said `A2A` / `agent 互调` / `agent-to-agent` / `agent 通信` in Phase 1: `服务类型？（你刚说想要 agent（智能体）通信式服务（议价 / 灵活协作），确认 2 即可；想改回 1 也行。）`
- **Q3 servicetype** when user said `A2MCP` / `MCP 服务` / `API 接口` in Phase 1: `服务类型？（你刚说想要 API 接口式服务（按次调用、固定价格），确认 1 即可；想改回 2 也行。）`

⛔ For Q3 specifically: when quoting the user's earlier type mention, **map their term to the long-form-with-gloss** per `core/ux-lexicon.md §Service-type` Pattern A — Q3 is a Pattern-A teaching context, so the short form alone is not enough on first encounter. **Never** echo the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK; output is not). Full source-of-truth rule: `SKILL.md §Sub-flows §Core Flow §Phase 2 Q1 UX guidance Option A`.

⛔ The carve-out **only** applies when the candidate value appeared as the user's own typed text in an earlier turn of this conversation. It does **NOT** legitimize pulling from `userEmail`, USER.md, CLAUDE.md, XMTP sender, the wallet account name, or any other session-metadata source — those remain forbidden per Red line 6.

Chinese per-service Q&A (render `接下来是服务[N]：` as a one-line preamble before Q1):

| Step | 问用户 (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `这个服务叫什么名字？` + 4 segments (see `core/field-specs.md`) | non-empty, CN ≤ 30 文字 | `name` |
| Q2 | `详细介绍一下这项服务。` + 4 segments | non-empty, CN ≤ 400 文字，需符合 3 段结构（摘要 / 核心能力 / 示例 Prompt）；不符合则提示用户按结构重填 | `servicedescription` |
| Q3 | `这项服务是哪种类型？` + numbered-options (`core/choice-prompts.md`):<br>&nbsp;&nbsp;`1. API 接口式服务（按次调用、固定价格，标准 MCP（标准调用接口）接口）`<br>&nbsp;&nbsp;`2. agent（智能体）通信式服务（双方协商定价 / 灵活协作；价格默认私下谈，可选填上链（写入区块链）参考价）`<br>`回复 1 或 2。`<br>**Pattern A (long form inline) per `core/ux-lexicon.md §Service-type`** — Q3 is a teaching prompt (user is being asked to choose, so they need the gloss to make the choice); the option text above uses the long form with gloss inside the parenthetical. This satisfies the first-occurrence-gloss requirement on its own; **no separate footnote needed below this prompt**. Subsequent renderings in the same conversation (e.g. the §3 confirmation card cell) MAY use the short form `API 接口` / `agent 互调`.<br>**Maintainer-internal mapping (NOT shown to user):** receive `1` / `2` and map to wire enum `1→A2MCP` / `2→A2A`; CLI has no numeric alias, sending raw `1` would `bail invalid servicetype`. ⛔ Never render the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK — if the user types `A2A` we accept it and map internally; output never carries the raw enum). | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if `A2MCP` → `每次调用收多少？格式：数字 + 空格 + 币种，支持 USDT / USDG，例如 10 USDT / 50 USDG / 0.5 USDT / 0 USDT。` + 4 segments ; if `A2A` → `这项服务的参考价是多少？（选填，不填表示价格由双方自行协商。回复 "跳过" 可跳过。）` + 4 segments | A2MCP: 格式 `数字 USDT\|USDG`，数字 ≥ 0，最多六位小数，非空必填。A2A: 空 或 满足同一格式。**Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})? (USDT\|USDG)$`（case-insensitive）；skill 解析数字部分写入 wire `fee` 字段，币种仅用于展示。 | `fee` (A2A 跳过时仍会以 `"fee":""` 进入 wire payload——`models.rs:21` 的 `fee: String` 没有 `skip_serializing_if`。skill 渲染时按 `空 → 免费/free`；后端是否区分"空串 vs 缺失键"由产品 spec 决定，本地代码不可证实) |
| Q5 | if `A2MCP` → `MCP（标准调用接口）服务地址是什么？必须 https:// 开头，且公网可达（其他 agent 会通过公网来调用你的服务）。` + 4 segments ; if `A2A` → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (A2A 即使用户给了 CLI 也会清掉，见 `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt (no `Q` label, it's a flow switch):<br>`还要再加一项服务吗？`<br>&nbsp;&nbsp;`1. 再加一项`<br>&nbsp;&nbsp;`2. 不加了，到此为止`<br>`回复 1 或 2。` | reply 1 or 2 | — |

English per-service Q&A (render `Now service [N]:` as a one-line preamble before Q1):

| Step | Ask the user (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `What's the name of this service?` + 4 segments | non-empty, EN ≤ 64 chars | `name` |
| Q2 | `Describe this service.` + 4 segments | non-empty, EN ≤ 400 chars, must follow 3-part structure (summary / capabilities / example prompts); if not, prompt user to rewrite | `servicedescription` |
| Q3 | `Which type is this service?` + numbered-options:<br>&nbsp;&nbsp;`1. API-interface service (pay-per-call, fixed price; standard MCP (standard call protocol) interface)`<br>&nbsp;&nbsp;`2. agent-to-agent service (negotiated pricing / flexible collaboration; pricing is off-chain by default, optional on-chain reference price)`<br>`Reply 1 or 2.`<br>**Pattern A (long form inline) per `core/ux-lexicon.md §Service-type`** — Q3 is a teaching prompt (user is choosing, so they need the gloss to decide); the option text above uses the long form with gloss inside the parenthetical. This satisfies the first-occurrence-gloss requirement on its own; **no separate footnote needed below this prompt**. Subsequent renderings in the same conversation (e.g. the §3 confirmation card cell) MAY use the short form `API service` / `agent-to-agent`.<br>**Maintainer-internal mapping (NOT shown to user):** map reply `1→A2MCP` / `2→A2A` before invoking the CLI — the CLI has no numeric alias and sending raw `1` bails with `invalid servicetype`. ⛔ Never render the raw enum `A2MCP` / `A2A` back to the user (input acceptance is OK — if the user types `A2A` we accept it and map internally; output never carries the raw enum). | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if A2MCP → `Price per call? Format: number + space + currency, supports USDT / USDG, e.g. 10 USDT / 50 USDG / 0.5 USDT / 0 USDT.` + 4 segments ; if A2A → `Reference price for this service? (optional; leave empty to allow direct negotiation. Reply "skip" to skip.)` + 4 segments | A2MCP: format `number USDT\|USDG`, number ≥ 0, ≤ 6 decimal places, must be non-empty. A2A: empty OR matches the same format. **Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})? (USDT\|USDG)$` (case-insensitive); skill extracts numeric part for wire `fee` field, currency used for display only. | `fee` (when A2A is left empty, the wire payload still carries `"fee": ""` — `models.rs:21` `fee: String` has no `skip_serializing_if`. The skill renders empty fee as `免费` / `free`; whether the backend distinguishes empty-string from absent-key is governed by the product spec and cannot be verified from this repo) |
| Q5 | if A2MCP → `What's the MCP (standard call protocol) endpoint URL? Must start with https:// and be reachable from the public internet (other agents will connect to your service over the public internet).` + 4 segments ; if A2A → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt:<br>`Want to add another service?`<br>&nbsp;&nbsp;`1. Add another`<br>&nbsp;&nbsp;`2. No more, finish here`<br>`Reply 1 or 2.` | reply 1 or 2 | — |

After each service is collected, echo back a one-line summary in the user's language before the loop gate:
- 中文：`已记录 服务[1]：TVL Query（API 接口，10 USDT，https://…）。`
- English: `Recorded Service [1]: TVL Query (API service, 10 USDT, https://…).`

