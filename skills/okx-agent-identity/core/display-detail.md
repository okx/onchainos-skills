# Display Formats — Agent Detail & Confirmation Cards

> Supplement to `core/display-formats.md`. Contains §2 Agent detail card, §2.5 Multi-agent detail, and §3 Create/Update Diff confirmation card.
> Global rendering rules (service-type Pattern B, URL rule, `#<id>` placeholder, photo/description row rules) are defined in `core/display-formats.md`.

## Table of Contents

| Section | Content |
|---|---|
| **§2** | Agent detail card — rendered after create / update / activate / deactivate / get --agent-ids |
| **§2.5** | Multi-agent detail — when `agent get --agent-ids` returns multiple agents |
| **§3** | Create / Update Diff confirmation card — mandatory before every content-creating write |

---

## 2. Agent detail card — after `create` / `update` / `activate` / `deactivate` / `agent get --agent-ids <id>`

Chinese variant:

| 字段 | 值 |
|---|---|
| Agent ID | #99 |
| 名字 | DeFi Analyzer |
| 角色 | 服务提供商 |
| 状态 | 已上架 |
| 地址 | 0xabc…1234 |
| 描述 | 链上数据分析与收益模拟。 |
| 头像 | <url> |
| 服务 | [1] TVL Query — API 接口, 10 USDT, `<user-or-backend-provided-endpoint>` |
| 服务 | [2] Yield Check — agent 互调, 免费 |
| 服务 | [3] Whale Alert — agent 互调, 5 USDT |
| 评分 | ★ 4.6 (18 条评价) |
| txHash | 0xabcdef…0f12 |

> 服务类型：API 接口 = 按次调用、固定价格；agent 互调 = 议价 / 灵活协作。

English variant:

| Field | Value |
|---|---|
| Agent ID | #99 |
| Name | DeFi Analyzer |
| Role | Agent Service Provider (ASP) |
| Status | active |
| Address | 0xabc…1234 |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | <url> |
| Services | [1] TVL Query — API service, 10 USDT, `<user-or-backend-provided-endpoint>` |
| Services | [2] Yield Check — agent-to-agent, free |
| Services | [3] Whale Alert — agent-to-agent, 5 USDT |
| Rating | ★ 4.6 (18 reviews) |
| txHash | 0xabcdef…0f12 |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.

Rules:

- Two-column table. Never the Unicode box-drawing "字段 值" art.
- Pick ONE variant based on user language — do not render bilingual `Agent Service Provider (服务提供商)` or `active (已上架)`.
- Render `Role` using the user-language label: `用户 / 服务提供商 / 仲裁者` ↔ `User Agent / Agent Service Provider (ASP) / Evaluator Agent`. Never render the raw ERC-8004 enum (`requester / provider / evaluator`) or legacy CN nouns (`买家 / 卖家 / 服务方 / 验证者`).
- Render `Status` using the user-language label: `已上架 / 已下架` ↔ `active / inactive`.
- Short-form address: `0x`first 4`…`last 4 hex chars. Show the full address only when the user asks.
- **⛔ `服务` / `Services` rows are provider-only.** `requester` 和 `evaluator` 的角色定义里没有 service —— 渲染他们的详情卡时**必须把所有 `服务` / `Services` 行整行省略**（不要写 `服务 | 无` / `Services | none` / `服务 | —` 之类的占位，**直接删除整行不输出**）。即使后端 `services` 字段返回了 `[]` / `null` / 甚至意外塞了一条数据，**只对 `role == provider` 的 agent 渲染 Service 行**。这条规则同时适用于 `agent get --agent-ids <id>` 的详情卡、`create` / `update` 后的详情卡、以及 §3 Create variant / Update Diff variant —— 见 §3 顶部的对应规则。/ For `requester` and `evaluator` detail cards, **omit every `服务` / `Services` row entirely** — no `Services | none` / `Services | —` / `Services | (empty)` placeholders, just drop the rows. This holds even when the backend returns `services: []` or `services: null` (or, by anomaly, a non-empty array for a non-provider role): render Service rows **only when `role == provider`**. Same constraint applies to the §3 Create / Update Diff variants.
- Services — one row per service, numbered `[N]`, single-line format **(provider only — see the rule above; on requester / evaluator skip the rows entirely)**. The **name value** (what the user typed, e.g. `TVL Query`) stays verbatim; the following descriptor uses user-language words: Chinese `名称 — 类型, 价格, 接口地址`-style reading order, English `Name — Type, Fee, Endpoint`-style reading order. In practice the single-line format is `<ServiceName> — <Type>, <Fee or 免费/free>, <Endpoint>`. **A2A fee handling**: if the backend returned a non-empty `fee` for the A2A service, render it as `<N> USDT` exactly like A2MCP; if `fee` is absent / empty, render the short form `免费` / `free` (Type=A2A in the same row already gives readers the off-chain-pricing context, so no parenthetical is needed in this compact row). The Endpoint cell is always dropped for A2A regardless (CLI clears it).
- `txHash` row present only when the command produced a tx (absent on read-only commands).
- `Agent ID` row: follow the `#<id>` placeholder rule at the top of this file — omit the row entirely if the id is not available yet (e.g. fresh `create` response), don't render `#` alone.
- **Single source of data — no chain calls.** All rows above (including Services and Reputation aggregate) come from the **one** `agent get --agent-ids <id>` response. The envelope is double-layer (see `core/cli-reference.md §3`): outer `list[*]` is an accountName wrapper, the actual agent row sits at `list[0].agentList[0]` for a single-id detail lookup. Field set on the agent row: `{ agentId, name, role, status, description, picture, address, services: [...], reputation: { score, count } }`. Do **NOT** chain `agent service-list --agent-id <id>` to "populate" the Services rows — they're already in the response. Do **NOT** chain `agent feedback-list --agent-id <id>` to "populate" the Reputation row — the aggregate `{ score, count }` is already there; individual review entries belong to a separate, user-triggered request (see §Post-detail prompt below).

### Post-detail prompt (after rendering §2)

After the detail card is rendered from a single-agent `agent get`, offer **one** numbered-options prompt asking whether to continue — do not auto-run anything. Follow the numbered-options patternuser language:

Chinese:
```
要继续看这个 agent（智能体）的评价详情吗？
  1. 要，拉评价列表
  2. 不用了
回复 1 或 2。
```

English:
```
Want to see this agent's review details?
  1. Yes, pull the review list
  2. No, I'm good
Reply 1 or 2.
```

- On `1`: run `agent feedback-list --agent-id <id>` once and render §5 (feedback list).
- On `2`: stop. No further calls.
- No other side-queries. `service-list` is **never** triggered from this prompt — services are already shown in the detail card.

---

## 2.5. Multi-agent detail — `agent get --agent-ids <id1>,<id2>,…` with multiple ids

When the response contains more than one agent — i.e. `sum(list[*].agentList.length) > 1` after walking all accountName wrappers — render **one §2 detail card per agent** in response order (iterate wrappers, then `agentList[*]` within each), separating consecutive cards with a `---` divider line. The same data-source / no-chain rule applies per card (servicesreputation already in the response — never chain `service-list` / `feedback-list` to "populate" rows that are already there).

> ⚠️ **Do NOT trigger on `list.length > 1` alone** — `list[*]` now counts accountName wrappers, not agents. `agent get --agent-ids 42,58` may land both ids inside the same wrapper's `agentList` (when both belong to one derived wallet), in which case `list.length == 1` but two agents are present. Trigger this multi-card path off the **flattened agent count**, not the wrapper count.

After all cards, render a **single multi-select Post-detail prompt** at the end (not per card):

Chinese:
```
要继续看哪几个 agent（智能体）的评价详情？
  0. 都不要
  1. #<id1>
  2. #<id2>
  …
回复对应数字（多选用逗号分隔，例如 "1,3"）。
```

English:
```
Which agents' review details do you want to see?
  0. None
  1. #<id1>
  2. #<id2>
  …
Reply with matching numbers (comma-separated, e.g. "1,3").
```

- On `0` → stop. No further calls.
- Otherwise → run `agent feedback-list --agent-id <id>` **once per selected agent**, render §5 for each, separated by `---`. Never run `service-list` from this prompt.
- If the user already named which subset of returned agents they want reviews for ("看 42 和 58 的评价"), skip the prompt entirely and go directly to those ids' `feedback-list`.

---

## 3. Create / Update Diff confirmation card

Used before executing any write that modifies fields (`create`, `update`). Three columns on `update`; two columns on `create` (nothing to diff against). Unchanged fields on `update` show `(不变)`.

> ⛔ **`服务[N]` / `Service [N]` rows are provider-only — applies to both Create variant and Update Diff variant.** When the role being created / updated is `requester` or `evaluator`, **do NOT** render any `服务[N] ...` / `Service [N] ...` row in the confirmation card (no `服务 | 无`, no `Service [1] | (none)`, no placeholder dash — **drop the rows entirely**). Only renders when `role == provider`. This mirrors the §2 detail-card rule above and is the canonical guard against the "buyer confirmation card shows a 服务 field" hallucination. Note: even on `update`, the role of the target agent (resolved from the mandatory `agent get --agent-ids <id>` pre-step of ) decides this — if you are editing a `requester` agent, the Update Diff card has no Service rows; if you are editing a `provider` agent, it does.

### Create variant (no current values to compare)

Render ONE language variant based on user language. Do NOT render bilingual labels like `Agent Service Provider (服务提供商)` or mix Chinese field labels with English service-field labels — see §Language Matching.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 服务提供商 |
| 名字 | DeFi Analyzer |
| 描述 | 链上数据分析与收益模拟。 |
| 头像 | 默认 |
| 服务[1] 名称 | TVL Query |
| 服务[1] 类型 | API 接口 |
| 服务[1] 价格 | 10 USDT |
| 服务[1] 接口地址 | `<user-provided-endpoint>` |

> 服务类型：API 接口 = 按次调用、固定价格；agent 互调 = 议价 / 灵活协作。

English variant:

| Field | Value |
|---|---|
| Role | Agent Service Provider (ASP) |
| Name | DeFi Analyzer |
| Description | On-chain data analysis and yield simulation. |
| Profile photo | default |
| Service [1] Name | TVL Query |
| Service [1] Type | API service |
| Service [1] Fee | 10 USDT |
| Service [1] Endpoint | `<user-provided-endpoint>` |

> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.

Service-field label mapping (user-facing labels ↔ CLI JSON keys the skill sends to `--service`):

| CLI JSON key | 中文标签 | English label |
|---|---|---|
| `name` | 名称 | Name |
| `servicedescription` | 描述 | Description |
| `servicetype` | 类型 | Type |
| `fee` | 价格 | Fee |
| `endpoint` | 接口地址 | Endpoint |

Left column is the exact JSON key sent on the wire inside the `--service` payload (new lowercase schema). The middle / right columns are the user-facing labels rendered in cards and Q&A prompts — keep those localized and never leak the raw JSON key into user-visible text.

### Update variant (diff)

Chinese variant:

| 字段 | 当前值 | 新值 |
|---|---|---|
| 名字 | DeFi Analyzer | (不变) |
| 描述 | 链上数据分析。 | **链上数据分析与收益模拟。** |
| 头像 | <旧 URL> | **<新 URL>** |
| 服务[1] 价格 | 10 USDT | (不变) |

> 本次会改 描述 和 头像；其它字段保持不变。
> 预计费用: **0 USDT**（修改字段无手续费，由 OKX 承担）。可以撤回: 想退回原值再更新一次即可；操作随时可逆。
> 确认后回复 "执行" 即可。

English variant:

| Field | Current | New |
|---|---|---|
| Name | DeFi Analyzer | (unchanged) |
| Description | On-chain data analysis. | **On-chain data analysis with yield simulation.** |
| Profile photo | <old URL> | **<new URL>** |
| Service [1] Fee | 10 USDT | (unchanged) |

> This update changes Description and Profile photo; everything else stays as-is.
> Estimated cost: **0 USDT** (editing fields costs no transaction fees — OKX covers them). Reversible: re-run update to revert to the old value at any time.
> Reply "execute" to run.

Rules:

- **Three columns for update**: label them `字段 / 当前值 / 新值` or `Field / Current / New` to match user language. Unchanged rows show `(不变)` / `(unchanged)` in the new-value column — never empty, never repeated value.
- Changed rows: bold the new-value cell so the diff reads at a glance.
- For each service entry, always list all sub-fields — easy to spot accidental drops. Localize the service-field labels per the mapping table above.
- **Do NOT show the bash command in this card.** If the user asks "把命令给我看", render it as a separate code block afterward; otherwise omit.
- **Maintainer note (wholesale `--service` replacement, internal — do NOT surface to user):** the `--service` flag wire-level **replaces the full services list**, not a per-field patch. When only one sub-field of one service changes (e.g. only `Service [1] Fee`), the skill MUST construct the new `--service` JSON by **starting from the current full services list** (from the mandatory `agent get` pre-step) and applying the diff in memory — then send the **complete** list. Sending only the changed entry would silently delete every other service. This is a wire-level concern; do not mention `--service` in the user-visible card footer (Red line 2).
- End every diff card with exactly one line: `确认后回复 "执行" 即可。` (English variant: `Reply "execute" to run.`). Do NOT use any verb like "下发" / "dispatch" / "send" in this footer — the SKILL.md "no narration between confirmation and result" rule for why.
- **Cost row (mandatory).** Every Create-variant card AND Update Diff card MUST include a final row (rendered immediately above the `确认后回复 "执行" 即可。` line) explaining what the user pays. Phrasings (substitute the role / action wording per context — these are templates, not literal):
  - Create variant (2 cols):
    - 中文: `| 预计费用 | **0 USDT**（创建 / 修改 / 上下架均无手续费，由 OKX 承担；服务费用由用户在调用时支付，100% 归你） |`
    - 英文: `| Estimated cost | **0 USDT** (creating / editing / activating / deactivating costs no transaction fees — OKX covers them; service fees are paid by User Agents per call and go 100% to you) |`
  - Update variant (3 cols — this row uses only 1 cell that spans across, so render as plain text below the table instead of as a table row):
    - 中文: `> 预计费用: **0 USDT**（修改字段无手续费，由 OKX 承担）。`
    - 英文: `> Estimated cost: **0 USDT** (editing fields costs no transaction fees — OKX covers them).`
- Source of truth for these costs: `core/cost-disclosure.md`. ⛔ **Never fabricate other cost items** (no "平台服务费", no "Agent 调度费", no "审核费").

---

