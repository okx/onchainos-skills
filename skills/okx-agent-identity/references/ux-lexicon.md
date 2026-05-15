# UX Lexicon — 内部术语 → 用户视角翻译表

⛔ **This file is referenced by `SKILL.md §UX Output Red Lines` Red line 4.** Every AI user-visible message MUST follow the **per-section rendering rule** below — for two-form sections (e.g. Role, Status, Field) that means using the user-language column wording; for multi-form sections (e.g. Service-type, which has both a long form for Pattern A teaching contexts and a short form + footnote for Pattern B cell contexts) that means **using the form prescribed by the section's own pattern selector**, not a single "right-column" default. Never leak the left-column `内部` literal (wire-level enum / CLI flag / JSON key) into chat output. Internal reasoning, tool arguments, CLI invocations, and maintainer-facing doc blocks may use those left-column literals freely — the constraint applies only to text the user sees.

## Role 角色术语

| 内部 (CLI key / API field) | 对中文用户说 | 对英文用户说 |
|---|---|---|
| `requester` (CLI `--role` value, alias `1` / `buyer` / `requestor`) | 买家（一律本地化，不要直接说 `requester`） | requester (ERC-8004 native term — keep as-is; do NOT translate to "buyer" — English crypto users recognize the on-chain role word, mixing "buyer" in mid-conversation creates inconsistency with the `Role` column in `display-formats.md §1/§2/§6`) |
| `provider` (CLI `--role` value, alias `2`) | 卖家（默认）/ 服务方（正式语境） | provider (ERC-8004 native; do NOT translate to "seller") |
| `evaluator` (CLI `--role` value, alias `3`) | 验证者（默认）/ 仲裁者（争议语境） | evaluator (ERC-8004 native; do NOT translate to "arbitrator" / "judge") |

**Asymmetric rule rationale.** Chinese gets localized because `requester / provider / evaluator` are unfamiliar English words to Chinese-speaking non-engineer users (the r7 test confirmed 林姐 / 小贾 personas didn't recognize them). English-speaking crypto users, by contrast, learn `requester / provider / evaluator` as part of ERC-8004 vocabulary; translating them to `buyer / seller / arbitrator` creates conversational mismatch with on-chain explorers, the OKX marketplace UI, and the rest of the ecosystem. The asymmetry is intentional — do not "fix" it by translating the English side.

**Carve-out (Chinese):** if the user explicitly typed the English role word ("我要注册一个 provider"), the AI MAY echo `provider` in that immediate reply (it's the user's vocabulary). But the **next system-initiated mention** in the same conversation should drift back to the localized term, so subsequent prompts stay consistent.

## Service-type 服务类型术语

| 内部 (`servicetype`) | 长形式（用于"教学型"上下文，gloss 内嵌） | 短形式（用于卡片单元格 / 标签场景） | 单独的 gloss 内容（用作 footnote） |
|---|---|---|---|
| `A2MCP` | 「**API 接口式服务**（按次调用、固定价格）」 / "**API-interface service** (pay-per-call, fixed price)" | "API 接口" / "API service" | 按次调用、固定价格 / pay-per-call, fixed price |
| `A2A` | 「**agent 通信式服务**（议价 / 灵活协作）」 / "**agent-to-agent service** (negotiated / off-chain pricing)" | "agent 互调" / "agent-to-agent" | 议价 / 灵活协作 / negotiated / off-chain pricing |

⛔ **Raw `A2MCP` / `A2A` enum NEVER appears in user-visible text — period.** The raw form is the wire-level CLI `--service` payload value only; user output uses one of the two localized forms above.

### Two acceptable rendering patterns (both deliver the gloss on first occurrence)

Both patterns satisfy the "user must see the gloss on first encounter" requirement; the choice is **context-driven**, not preferential. The skill MUST use exactly one of these patterns whenever serviceType reaches user-visible text:

- **Pattern A — Inline parenthetical (long form)**: render the **long form** verbatim — the gloss sits in the parenthetical attached to the name. Used in: Q&A prompts that teach the user the choice (e.g., `role-provider.md` Phase 2 Q3 numbered options), error messages explaining the constraint, free-form explanations in chat. Example:
  > 这项服务是哪种类型？
  >   1. API 接口式服务（按次调用、固定价格，标准 MCP 接口）
  >   2. agent 通信式服务（议价 / 灵活协作；价格默认链外谈，可选填上链参考价）

- **Pattern B — Short form + footnote below table** (preferred in cells / tables where space is tight): the **short form** sits in the cell; **on first occurrence in the conversation**, append a one-line gloss footnote below the table. Used in: `display-formats.md` §2 detail card, §3 confirmation card, §4 service-list, §6 search results, anywhere `serviceType` appears as a cell value. Example:
  > | TVL Query | API 接口 | 10 USDT | ... |
  > | Yield Check | agent 互调 | 免费 | ... |
  >
  > 服务类型：API 接口 = 按次调用、固定价格；agent 互调 = 议价 / 灵活协作。

### Subsequent reuse in the same conversation

After the user has seen the gloss (either via Pattern A or Pattern B), subsequent renderings in the same conversation MAY use the **short form alone** — no further gloss / footnote needed. The skill MUST still NEVER render the raw enum.

This framework is the single source of truth referenced from `SKILL.md §UX Output Red Lines Red line 4` and `display-formats.md` top-level "Service-type rendering" rule; both files must stay aligned with this section.

## Status 状态术语

| 内部 (`status` int) | 对中文用户说 | 对英文用户说 |
|---|---|---|
| `0` | 已下架 | inactive |
| `1` | 已上架（在售卖） | active |
| `2` | 审核中（一般 24h 内出结果） | under review (typically resolved within 24h) |
| `3` | 审核未通过 | review failed |

⛔ Never render `status=0` / `status: 1` / `status=2` / raw integer status fields to the user. Always translate.

## Field 字段术语

| 内部 (CLI JSON key) | 对中文用户说 | 对英文用户说 |
|---|---|---|
| `agentId` | "ID #N" or "#N"（保留 `#` 前缀） | "#N" or "Agent ID #N" |
| `ownerAddress` | 拥有者地址 / 持有钱包 | owner wallet |
| `address` (agent record `address` field) | 链上地址 | on-chain address |
| `chainIndex` | (不说 — XLayer 是默认且唯一 chain) | (don't mention — XLayer is default) |
| `name` (agent or service) | 名字 / 名称 | name |
| `description` (agent) | 描述 / 简介 | description |
| `picture` | 头像 | avatar / picture |
| `servicedescription` | 服务描述 | service description |
| `servicetype` | 服务类型 | service type |
| `fee` | 价格 / 费用 | price / fee |
| `endpoint` | 接口地址 | endpoint |
| `reputation.score` | (do NOT render — always convert to `★ <stars>` per `SKILL.md §Amount Display Rules`) | (same — render as `★ <stars>`) |
| `reputation.count` | 评价数 | review count |
| `txHash` | 交易哈希 | tx hash |
| `creator-id` | (do NOT expose the literal `creator-id`; just say "你的 agent #N 会作为这次评价的发起人") | (same — phrase as "your agent #N will be the reviewer") |
| `--agent-id` flag value | (don't expose the flag; AI fills it itself) | (same) |
| `--score` flag value | (don't expose the flag; "X 星" / "X stars") | (same) |

⛔ The carve-out: `Agent ID` as a column header in cards / `#<N>` as a row value is allowed (it's a stable identifier the user will see again on explorer). Everywhere else, translate.

## Flow / internal-section term 黑话术语

These names exist purely inside the skill's own documentation and reasoning. ⛔ **Never surface them to the user.**

| 内部 (skill docs / model reasoning) | 对用户怎么处理 |
|---|---|
| `pre-check` / `Pre-Check` / `MANDATORY pre-check gate` / `前置检查` | (just run it silently and report the result; never narrate "正在执行 pre-check") |
| `Phase 1` / `Phase 2` / `阶段 1` / `阶段 2` | If you must signpost a transition, say "**接下来配置你的服务**" / "**now let's set up your services**" — never "进入 Phase 2" |
| `Q1：` / `Q1:` / `Q2：` / `Q3：` / `S1：` / ... / `S6：` (numbered Q/S prompt prefixes) | Strip the prefix. Just ask the question in natural language. Chinese example: "这个卖家身份叫什么名字?" — **not** "Q1: 这个卖家身份叫什么名字?" and **not** "这个 provider 叫什么名字?" (the raw `provider` word also violates the role-term localization rule above). English example: "What's the name of this provider?" — no `Q1:` prefix; `provider` is fine as-is per English keep-native rule. |
| `One-shot capture` / `pre-execute self-check` / `confirmation gate` / `post-execute gate` | (model-internal control-flow names; never appear in user text) |
| `passive onboarding` / `intent=need-requester` | (handoff metadata; never appear in user text) |
| `dual-scope rule` / `wrapper / accountName` | (rendering rule for the AI; user sees "钱包 wallet-N" headers per `display-formats.md §1`, not the words "wrapper" or "accountName") |
| `--service` JSON payload key names | Translate (see Field table above) |

## How to use this lexicon at runtime

The AI's user-visible draft → sweep these rules → emit:

1. Replace every `okx-*` literal with business language (see `SKILL.md §UX Red Lines Red line 1`).
2. Replace every `onchainos agent <cmd>` literal with an "I'll do it for you" + actually invoke the CLI (Red line 2).
3. Replace every role / status / field literal with its user-language wording (Role section / Status section / Field section in this file). For **service-type** specifically, do NOT pick a single column blindly — pick the form prescribed by the section's "Two acceptable rendering patterns" selector: **Pattern A long form** for Q&A teaching prompts / error messages / free-form chat; **Pattern B short form + footnote** for cards / tables (§2 / §3 / §4 / §6 in `display-formats.md`).
4. Replace every flow-term / Q-prefix / S-prefix / Phase-N literal with natural-language phrasing (this file).
5. Check ≥5 agent counts have a reassurance footer (`display-formats.md §1`, Red line 5).

If the draft survives all five sweeps without rewrite, it's safe to send.
