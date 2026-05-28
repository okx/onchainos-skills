# UX Lexicon — 内部术语 → 用户视角翻译表

⛔ **This file is referenced by `SKILL.md §UX Output Red Lines` Red line 4.** Every AI user-visible message MUST follow the **per-section rendering rule** below. For Role / Status / Field sections that means using the canonical user-facing wording in the appropriate column; for the multi-form Service-type section that means using the form prescribed by the section's own pattern selector (long form for Pattern A teaching contexts, short form + footnote for Pattern B cell contexts). Never leak the left-column `内部` literal (wire-level enum / CLI flag / JSON key) into chat output. Internal reasoning, tool arguments, CLI invocations, and maintainer-facing doc blocks may use those left-column literals freely — the constraint applies only to text the user sees.

## Role 角色术语

The user-facing role terms are now **fully localized in BOTH languages** — Chinese users see the localized noun; English users see a friendly product term (NOT the raw ERC-8004 enum). The raw `requester` / `provider` / `evaluator` enum is wire-only.

| 内部 (CLI key / API field) | 对中文用户说 | 对英文用户说 |
|---|---|---|
| `requester` (CLI `--role` value, alias `1` / `buyer` / `requestor`) | **用户**（统一使用） | **User Agent** |
| `provider` (CLI `--role` value, alias `2`) | **服务提供商**（统一使用） | **Agent Service Provider (ASP)** — the abbreviation `ASP` is acceptable after first mention in the same conversation |
| `evaluator` (CLI `--role` value, alias `3`) | **仲裁者**（统一使用） | **Evaluator Agent** |

⛔ **Raw `requester` / `provider` / `evaluator` enum NEVER appears in user-visible text** — neither in Chinese nor in English. They're wire-only on the CLI `--role` flag. Same for the legacy CN words `买家` / `卖家` / `服务方` / `验证者` — those are deprecated user-facing terms; do not render them to the user from any new code path.

**Carve-out:** if the user themselves typed `provider` / `requester` / `evaluator` (or the legacy CN words) in their message, the AI MAY echo their wording in the immediate reply — but the next system-initiated mention should drift back to the canonical localized term so subsequent prompts stay consistent.

## Service-type 服务类型术语

| 内部 (`servicetype`) | 长形式（用于"教学型"上下文，gloss 内嵌） | 短形式（用于卡片单元格 / 标签场景） | 单独的 gloss 内容（用作 footnote） |
|---|---|---|---|
| `A2MCP` | 「**API 接口式服务**（按次调用、固定价格）」 / "**API-interface service** (pay-per-call, fixed price)" | "API 接口" / "API service" | 按次调用、固定价格 / pay-per-call, fixed price |
| `A2A` | 「**agent（智能体）通信式服务**（议价 / 灵活协作）」 / "**agent-to-agent service** (negotiated / off-chain pricing)" | "agent 互调" / "agent-to-agent" | 议价 / 灵活协作 / negotiated / off-chain pricing |

⛔ **Raw `A2MCP` / `A2A` enum NEVER appears in user-visible text — period.** The raw form is the wire-level CLI `--service` payload value only; user output uses one of the two localized forms above.

### Two acceptable rendering patterns (both deliver the gloss on first occurrence)

Both patterns satisfy the "user must see the gloss on first encounter" requirement; the choice is **context-driven**, not preferential. The skill MUST use exactly one of these patterns whenever serviceType reaches user-visible text:

- **Pattern A — Inline parenthetical (long form)**: render the **long form** verbatim — the gloss sits in the parenthetical attached to the name. Used in: Q&A prompts that teach the user the choice (e.g., `role-provider.md` Phase 2 type-choice numbered options), error messages explaining the constraint, free-form explanations in chat. Example:
  > 这项服务是哪种类型？
  >   1. API 接口式服务（按次调用、固定价格，标准 MCP（标准调用接口）接口）
  >   2. agent（智能体）通信式服务（双方协商定价 / 灵活协作；价格默认私下谈，可选填上链（写入区块链）参考价）

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
| `1` | 已上架（可接单） | active |
| `2` | 审核中（一般 24h 内出结果） | under review (typically resolved within 24h) |
| `3` | 审核未通过 | review failed |

⛔ Never render `status=0` / `status: 1` / `status=2` / raw integer status fields to the user. Always translate.

## ApprovalDisplayStatus

Translate per `SKILL.md §Language Matching` — the table below defines canonical English values; the AI renders them in the user's language.

| `approvalDisplayStatus` | 对中文用户说 | 对英文用户说 |
|---|---|---|
| `1` | 未发起审核 | Not submitted for review |
| `2` | 审核中，请耐心等待 | Under review, please wait |
| `4` | 审核通过，可被推荐自动接单 | Approved — eligible for task recommendations |
| `5` | 审核失败 | Review failed |
| `7` | 该 Agent 当前不可用 | This agent is currently unavailable |

Row label follows language matching: `审核状态` for Chinese users, `Approval status` for English users.

⛔ Never render the raw integer. Always translate. When `approvalRemark` is non-empty and `approvalDisplayStatus` is `5`, append it as a parenthetical: "审核失败（原因：xxx）" / "Review failed (reason: xxx)".

## Field 字段术语

| 内部 (CLI JSON key) | 对中文用户说 | 对英文用户说 |
|---|---|---|
| `agentId` | "ID #N" or "#N"（保留 `#` 前缀） | "#N" or "Agent ID #N" |
| `ownerAddress` | 拥有者地址 / 持有钱包 | owner wallet |
| `address` (agent record `address` field) | 链上地址（区块链上的地址） | on-chain address |
| `chainIndex` | (不说 — XLayer 是默认且唯一 chain) | (don't mention — XLayer is default) |
| `name` (agent or service) | 名字 / 名称 | name |
| `description` (agent) | 描述 / 简介 | description |
| `picture` | 头像 | profile photo |
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

**A2A 服务未填价格的渲染**: when a service of type `A2A` carries an empty / missing `fee`, render the user-facing value as `免费 / （未填，双方自行协商）` (Chinese) or `free / (skipped — negotiated directly)` (English) — do NOT echo the wire-level empty string, and do NOT use the older "链外议价 / off-chain negotiation" wording (that phrasing was changed to emphasize that pricing happens **between the two parties directly**, not on some "external chain").

**链 / 区块链 / NFT 的口语化** (used inside user-visible "请注意" segments, post-success lines, error cards):
- `链上` / `on-chain` → CN add gloss on first user-facing mention: `上链（写入区块链）`. EN may keep `on-chain` (English-speaking users recognize the term).
- `链上 NFT` / `on-chain NFT` in cost/reversibility copy → render as `区块链上的记录` (CN) / `your record on the blockchain` (EN) — most non-engineer users don't think of identities as "NFTs", and the NFT framing is wire detail.
- `gas` / `网络手续费` in cost copy → render as `手续费`（中文）/ `transaction fees` (English). Drop the "phase 1 / OKX 一期" framing; just say "由 OKX 承担" / "OKX covers them" — phase-numbering is a product-roadmap concern, not user-facing.

**钱包术语**:
- ⛔ Never write `钉包` — that's a typo of `钱包`. Sweep the entire codebase before shipping.
- `派生钱包` (in display-formats reassurance footer) → `关联钱包`. "派生" is implementation-leak (it implies HD-wallet derivation under the hood); "关联" is what the user sees.

## Flow / internal-section term 黑话术语

These names exist purely inside the skill's own documentation and reasoning. ⛔ **Never surface them to the user.**

| 内部 (skill docs / model reasoning) | 对用户怎么处理 |
|---|---|
| `pre-check` / `Pre-Check` / `MANDATORY pre-check gate` / `前置检查` | (just run it silently and report the result; never narrate "正在执行 pre-check") |
| `Phase 1` / `Phase 2` / `阶段 1` / `阶段 2` | If you must signpost a transition, say "**接下来配置你的服务**" / "**now let's set up your services**" — never "进入 Phase 2" |
| `Q1：` / `Q1:` / `Q2：` / `Q3：` / `S1：` / ... / `S6：` (numbered Q/S prompt prefixes) | Strip the prefix. Just ask the question in natural language. Chinese example: "这个服务提供商身份叫什么名字？" — **not** "Q1: 这个服务提供商身份叫什么名字？" and **not** "这个 provider 叫什么名字？" (the raw `provider` word also violates the Role-term localization rule above). English example: "What's the name of this ASP?" — no `Q1:` prefix; use the canonical localized term (ASP), not raw `provider`. |
| `One-shot capture` / `pre-execute self-check` / `confirmation gate` / `post-execute gate` | (model-internal control-flow names; never appear in user text) |
| `passive onboarding` / `intent=need-requester` | (handoff metadata; never appear in user text) |
| `dual-scope rule` / `wrapper / accountName` | (rendering rule for the AI; user sees "钱包 wallet-N" headers per `display-formats.md §1`, not the words "wrapper" or "accountName") |
| `--service` JSON payload key names | Translate (see Field table above) |
| `MCP` (when rendered to first-time user) | CN add gloss on first mention: `MCP（标准调用接口）`. EN add gloss similarly: `MCP (standard call protocol)`. Subsequent mentions in the same conversation may use bare `MCP`. |
| `agent` (when used as a user-visible noun in CN UI prompts) | On first mention, add inline gloss `agent（智能体）`. Subsequent mentions may use bare `agent`. EN keeps `agent` as-is. |

## How to use this lexicon at runtime

The AI's user-visible draft → sweep these rules → emit:

1. Replace every `okx-*` literal with business language (see `SKILL.md §UX Red Lines Red line 1`).
2. Replace every `onchainos agent <cmd>` literal with an "I'll do it for you" + actually invoke the CLI (Red line 2).
3. Replace every role / status / field literal with its user-language wording (Role section / Status section / Field section in this file). For **service-type** specifically, do NOT pick a single column blindly — pick the form prescribed by the section's "Two acceptable rendering patterns" selector: **Pattern A long form** for Q&A teaching prompts / error messages / free-form chat; **Pattern B short form + footnote** for cards / tables (§2 / §3 / §4 / §6 in `display-formats.md`).
4. Replace every flow-term / Q-prefix / S-prefix / Phase-N literal with natural-language phrasing (this file).
5. Check ≥5 agent counts have a reassurance footer (`display-formats.md §1`, Red line 5).
6. Sweep for legacy CN role nouns (`买家` / `卖家` / `服务方` / `验证者`) and the typo `钉包` — replace with the new canonical (`用户` / `服务提供商` / `仲裁者` / `钱包`). Same sweep applies to raw EN role enums (`requester` / `provider` / `evaluator`) outside of wire-level documentation.

If the draft survives all six sweeps without rewrite, it's safe to send.
