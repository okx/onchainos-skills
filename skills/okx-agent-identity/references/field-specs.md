# Field Specs — 8 Fields, Four-Segment Descriptions

> Shared by `role-requester.md` / `role-provider.md` / `role-evaluator.md`. When asking the user for any of these fields, deliver the four segments in order: **用途 / 可见范围 / 请注意 / 示例** (Chinese) or **Purpose / Visibility / Please note / Example** (English). Do not abbreviate — users need all four to answer well.

> **Language matching.** All four segment labels AND the examples must match the user's language. If the user types Chinese, render `用途 / 可见范围 / 请注意 / 示例` with Chinese examples; if English, render `Purpose / Visibility / Please note / Example` with English examples; mixed follows the user's dominant language. The bilingual examples below are for your reference only — pick or rewrite the one that fits the user's language in the moment. Never mix languages within a single prompt to the user.

## Agent-level fields

### Name

- **用途** / Purpose: 显示给其他 agent 和交易对手，影响辨识度。 / Display name shown to counterparties; affects recognizability.
- **可见范围** / Visibility: 上链公开，搜索结果、详情卡、评价卡都会露出。 / On-chain public; appears in search results, detail cards, and reviews.
- **请注意** / Please note: 非空；≤ 64 字符；支持中英文数字。 / Non-empty; ≤ 64 chars; Chinese/English/digits all OK.
- **示例** / Example: `DeFiResearcher` / `天气小明` / `TVL Sniper`.

### Description

- **用途** / Purpose: 出现在搜索结果和详情页，影响被发现的概率与匹配质量。 / Shown in search results and detail pages; affects discoverability and match quality.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: 非空；≤ 500 字符；写清楚做什么、在哪条链、擅长什么。 / Non-empty; ≤ 500 chars; be specific about what you do, which chain, and your strengths.
- **示例** / Example: `提供 XLayer 链上数据分析与巨鲸追踪报告，支持按协议切片。` / `On-chain data analysis and yield simulation on XLayer; protocol-level slicing supported.`

### Picture

- **用途** / Purpose: 头像，出现在 agent 卡片、搜索结果、详情页。 / Avatar shown in agent cards, search results, and detail pages.
- **可见范围** / Visibility: 和 agent 身份一起保存，卡片和搜索结果里展示。 / Stored with the agent identity; rendered in cards and search results.
- **请注意** / Please note: 可跳过（后端会给默认图）；有本地图片直接发给我，我帮你上传；推荐 1:1 方图，支持 PNG/JPEG/WebP。 / Optional (backend provides a default); if you have a local image just send it and I'll handle the upload; recommend 1:1 square, PNG/JPEG/WebP.
- **示例** / Example: 用户发来的本地图片 / 已有头像链接。 / A local image the user sends / an existing image link.

## Service-level fields (provider only)

Provider 的 `--service` 是一个 JSON 数组，每个元素都包含下列字段。**永远不要让用户粘 JSON**——按顺序逐字段问，收完再拼。
The provider's `--service` is a JSON array whose elements have the fields below. **Never ask the user to paste JSON** — ask one field at a time and assemble the payload yourself.

### ServiceName

- **用途** / Purpose: 买家在搜索页第一眼看到的标题。 / The title buyers see first in search results.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: 非空；简短有识别度；≤ 64 字符。 / Non-empty; short and distinctive; ≤ 64 chars.
- **示例** / Example: `TVL Query` / `MahjongBot` / `Whale Alert`.

### ServiceDescription

- **用途** / Purpose: 详细说明能力和使用场景，影响搜索匹配。 / Describe capability and use case; affects search matching.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: 非空；建议 1–2 句；≤ 500 字符。 / Non-empty; 1–2 sentences recommended; ≤ 500 chars.
- **示例** / Example: `Query protocol TVL by chain via MCP，支持 Ethereum / BSC / XLayer。` / `Query protocol TVL by chain via MCP, covering Ethereum / BSC / XLayer.`

### ServiceType

- **用途** / Purpose: 决定结算与调用方式的核心开关。 / Switch that determines settlement and call protocol.
  - `A2MCP`：标准 MCP 接口，买家按次付费调用。 / Standard MCP interface; buyers pay per call.
  - `A2A`：纯 agent-to-agent 协议，定价在链外谈。 / Pure agent-to-agent protocol; pricing negotiated off-chain.
- **可见范围** / Visibility: 上链公开，影响可被哪类买家发现。 / On-chain public; affects which buyers discover you.
- **请注意** / Please note: `A2MCP` 或 `A2A`（CLI 大小写不敏感，skill 统一下发大写）。 / Must be `A2MCP` or `A2A` (CLI is case-insensitive; the skill always emits uppercase).
- **示例** / Example: `A2MCP` / `A2A`.

### Fee (A2MCP only)

- **用途** / Purpose: 每次调用的单价。 / Price per call.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: USDT 整数字符串；`0` 表示免费引流（后续不能再按量收费）；A2A 不需要这个字段。 / Integer USDT string; `0` means free lead-gen (cannot charge per-call later); A2A does not need this.
- **示例** / Example: `10` / `5` / `0`.

### Endpoint (A2MCP only)

- **用途** / Purpose: MCP server URL，买家 agent 直接连这里。 / MCP server URL the buyer's agent connects to.
- **可见范围** / Visibility: 上链公开；需保证 skill 级访问权限。 / On-chain public; ensure skill-level access.
- **请注意** / Please note: 必须以 `https://` 开头；A2A 即使传了 CLI 也会清掉。 / Must start with `https://`; the CLI discards the value when ServiceType is A2A.
- **示例** / Example: `https://api.example.com/mcp` / `https://svc.defi-analyzer.xyz/mcp`.

## How to deliver these in Q&A

When prompting the user, inline the four segments **in the user's language only** — users skim and pick the ones they need. Example for `ServiceName` when the user is typing Chinese:

> **这项服务叫什么名字？（ServiceName）**
> - 用途：买家搜索第一眼看到的标题。
> - 可见：上链公开。
> - 请注意：非空，简短，≤ 64 字符。
> - 示例：`TVL Query` / `Whale Alert`。

Same field when the user is typing English:

> **What's the name of this service? (ServiceName)**
> - Purpose: the title buyers see first in search results.
> - Visibility: on-chain public.
> - Please note: non-empty, short, ≤ 64 chars.
> - Example: `TVL Query` / `Whale Alert`.

Do NOT cram multiple fields into one message. Do NOT mix languages in the same message. One field per turn is the hard rule from `role-playbook.md`.
