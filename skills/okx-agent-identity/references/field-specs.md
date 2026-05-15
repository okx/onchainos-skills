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
- **请注意** / Please note: **provider 必填；requester / evaluator 选填**——跳过则上链 `ProfileDescription: ""`，渲染端显示为 `未填`。填了一律 ≤ 500 字符；写清楚做什么、在哪条链、擅长什么。 / **Required for provider; optional for requester / evaluator** — when skipped, the wire payload carries `ProfileDescription: ""` and the render layer shows `(not set)`. When supplied, ≤ 500 chars; be specific about what you do, which chain, and your strengths.
- **示例** / Example: `提供 XLayer 链上数据分析与巨鲸追踪报告，支持按协议切片。` / `On-chain data analysis and yield simulation on XLayer; protocol-level slicing supported.`

### Picture

- **用途** / Purpose: 头像，出现在 agent 卡片、搜索结果、详情页。 / Avatar shown in agent cards, search results, and detail pages.
- **可见范围** / Visibility: 和 agent 身份一起保存，卡片和搜索结果里展示。 / Stored with the agent identity; rendered in cards and search results.
- **请注意** / Please note: 可跳过（用默认头像）；有本地图片直接发给我，我帮你上传；推荐 1:1 方图，支持 PNG/JPEG/WebP。 / Optional (a default avatar is used when skipped); if you have a local image just send it and I'll handle the upload; recommend 1:1 square, PNG/JPEG/WebP.
- **示例** / Example: 用户发来的本地图片 / 已有头像链接。 / A local image the user sends / an existing image link.

## Service-level fields (provider only)

Provider 的 `--service` 是一个 JSON 数组，每个元素都包含下列字段。**永远不要让用户粘 JSON**——按顺序逐字段问，收完再拼。
The provider's `--service` is a JSON array whose elements have the fields below. **Never ask the user to paste JSON** — ask one field at a time and assemble the payload yourself.

### name

- **用途** / Purpose: 买家在搜索页第一眼看到的标题。 / The title buyers see first in search results.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: 非空；简短有识别度；≤ 64 字符。 / Non-empty; short and distinctive; ≤ 64 chars.
- **示例** / Example: `TVL Query` / `MahjongBot` / `Whale Alert`.

### servicedescription

- **用途** / Purpose: 详细说明能力和使用场景，影响搜索匹配。 / Describe capability and use case; affects search matching.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: 非空；建议 1–2 句；≤ 500 字符。 / Non-empty; 1–2 sentences recommended; ≤ 500 chars.
- **示例** / Example: `Query protocol TVL by chain via MCP，支持 Ethereum / BSC / XLayer。` / `Query protocol TVL by chain via MCP, covering Ethereum / BSC / XLayer.`

### servicetype

- **用途** / Purpose: 决定结算与调用方式的核心开关。 / Switch that determines settlement and call protocol.
  - **API 接口式服务**（按次调用、固定价格）：标准 MCP 接口，买家按次付费调用。 / **API-interface service** (pay-per-call, fixed price): standard MCP interface; buyers pay per call.
  - **agent 通信式服务**（议价 / 灵活协作）：纯 agent-to-agent 协议，定价默认在链外谈；可选填一个 USDT 参考价上链供搜索 / 匹配参考。 / **agent-to-agent service** (negotiated / off-chain pricing): pure agent-to-agent protocol; pricing is off-chain by default, with an optional USDT reference price stored on-chain to aid search / matching.
- **可见范围** / Visibility: 上链公开，影响可被哪类买家发现。 / On-chain public; affects which buyers discover you.
- **请注意** / Please note: 用户回复 `1` / `2` 选择，或者直接说 `API 接口` / `agent 互调` (中文) / `API service` / `agent-to-agent` (English)；skill 会把选择映射成 CLI 接受的值再下发。 / The user replies `1` / `2` to choose, or names the kind directly as `API service` / `agent-to-agent` (English) or `API 接口` / `agent 互调` (Chinese); the skill maps the choice to the CLI's accepted value before issuing.
- **示例** / Example: `1` / `2` / `API 接口` / `agent 互调` / `API service` / `agent-to-agent`.

**Maintainer-only note (not user-visible — wire-level enum):** the CLI's `--service` payload accepts only `A2MCP` / `A2A` (case-insensitive; the skill always emits uppercase). The raw enum NEVER appears in user-visible text per `references/ux-lexicon.md §Service-type` + `references/display-formats.md` top-level "Service-type rendering" rule.

### fee

- **用途** / Purpose: 每次调用的单价。 / Price per call.
- **可见范围** / Visibility: 上链公开。 / On-chain public.
- **请注意** / Please note: USDT 数字字符串，最多六位小数（如 `1.234567` / `10` / `0.5`）；`0` 表示免费引流（**API 接口** 上填 `0` 等于承诺后续不再按量收费）。**API 接口必填，agent 互调选填** —— agent 互调跳过时，skill 端会按 `免费` / `free` 渲染。 / USDT numeric string with up to 6 decimal places (e.g., `1.234567` / `10` / `0.5`); `0` means free lead-gen (on **API service**, `0` means you've committed to no per-call charges going forward). **API service requires it; agent-to-agent is optional** — when the user skips on agent-to-agent, the skill renders the price as `免费` / `free`. <br><br>**Maintainer-only note (not user-visible):** the CLI wire-level enums are `A2MCP` / `A2A` (case-insensitive). When `A2A` skips fee, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` with no `skip_serializing_if`; whether the backend distinguishes empty-string from absent-key is governed by the product spec, not anything in this repo. Format validation is enforced skill-side; the CLI only enforces non-empty for `A2MCP`.
- **示例** / Example: `1.22` / `10` / `0.5` / `0` / （agent 互调选填留空）/ (empty for agent-to-agent optional skip).

### endpoint (API 接口 / API service only)

- **用途** / Purpose: MCP server URL，买家 agent 直接连这里。 / MCP server URL the buyer's agent connects to.
- **可见范围** / Visibility: 上链公开；需保证 skill 级访问权限。 / On-chain public; ensure skill-level access.
- **请注意** / Please note: 必须以 `https://` 开头；如果服务类型是 agent 互调，这个字段填了也不会上链（CLI 自动清掉）。 / Must start with `https://`. If the service type is agent-to-agent, this field is dropped at CLI level even if supplied (it never goes on-chain).
- **示例** / Example: 你部署的 MCP server 公网地址（必须以 `https://` 开头，例如域名 + 路径形式）。 / Your deployed MCP server's public URL (must start with `https://`, typically a domain + path).
- **⛔ 渲染禁令 / Render constraint**: 写到这条 spec 时**绝对不要**在 `示例 / Example` 段里贴具体的 `https://...` 字面值（包括 `https://api.example.com/...` / `https://svc.example.com/...` / 任何形如 `https://xxx.yyy/zzz` 的占位串）。原因：这些字面值会被 Lark / 飞书 / Slack / 微信等 IM 渲染器自动识别为可点击的超链接，部分用户会真的点过去，而该域名要么不存在要么是错误目标。**只用文字描述**告诉用户「填什么样的链接」，不给 URL 范本。/ When rendering this spec, do **NOT** put a literal `https://...` value inside the `Example` segment (no `https://api.example.com/...`, no `https://svc.example.com/...`, no `https://anything/anything`). IM renderers auto-linkify these and users may accidentally click — the example domains are not real targets. Describe **what kind of URL** in words; never give a URL template.
- **Internal validation, do NOT inline into user-facing prompt** / **内部校验，不要进入对外提示**: A2MCP endpoint length ≤ 512 chars (skill-side check; CLI does not enforce length). On rejection, surface the 512-char limit verbatim in the error copy (see `troubleshooting.md` §3).

## How to deliver these in Q&A

When prompting the user, inline the four segments **in the user's language only** — users skim and pick the ones they need. Do NOT expose the CLI JSON key (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) in the prompt — that's internal schema, it only belongs in the raw bash command (which the user sees only if they ask).

Example for the service-name field when the user is typing Chinese:

> **这项服务叫什么名字？**
> - 用途：买家搜索第一眼看到的标题。
> - 可见：上链公开。
> - 请注意：非空，简短，≤ 64 字符。
> - 示例：`TVL Query` / `Whale Alert`。

Same field when the user is typing English:

> **What's the name of this service?**
> - Purpose: the title buyers see first in search results.
> - Visibility: on-chain public.
> - Please note: non-empty, short, ≤ 64 chars.
> - Example: `TVL Query` / `Whale Alert`.

Do NOT cram multiple fields into one message. Do NOT mix languages in the same message. Do NOT leak the CLI JSON key (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint` / …) into the user-visible prompt — localize the label (`名称 / 描述 / 类型 / 价格 / 接口地址` or `Name / Description / Type / Fee / Endpoint`) instead. One field per turn is the hard rule from `role-playbook.md`.
