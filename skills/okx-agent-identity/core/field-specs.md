# Field Specs — 8 Fields, Four-Segment Descriptions

> Shared by all three role playbooks. When asking the user for any of these fields, deliver the four segments in order: **用途 / 可见范围 / 请注意 / 示例** (Chinese) or **Purpose / Visibility / Please note / Example** (English). Do not abbreviate — users need all four to answer well.

> **Language matching.** All four segment labels AND the examples must match the user's language. If the user types Chinese, render `用途 / 可见范围 / 请注意 / 示例` with Chinese examples; if English, render `Purpose / Visibility / Please note / Example` with English examples; mixed follows the user's dominant language. The bilingual examples below are for your reference only — pick or rewrite the one that fits the user's language in the moment. Never mix languages within a single prompt to the user.

## Agent-level fields

### Name

- **用途** / Purpose: 显示给其他 agent和交易对手，影响辨识度。 / Display name shown to counterparties; affects recognizability.
- **可见范围** / Visibility: 上链（写入区块链）公开，搜索结果、详情卡、评价卡都会露出。 / On-chain public; appears in search results, detail cards, and reviews.
- **请注意** / Please note: 非空；最多 30 个文字；支持中英文数字。 / Non-empty; up to 64 characters; Chinese/English/digits all OK.
- **示例** / Example: `DeFiResearcher` / `天气小明` / `TVL Sniper`.

### Description

- **用途** / Purpose: 出现在搜索结果和详情页，影响被发现的概率与匹配质量。 / Shown in search results and detail pages; affects discoverability and match quality.
- **可见范围** / Visibility: 上链（写入区块链）公开。 / On-chain public.
- **请注意** / Please note: **服务提供商必填；用户 / 仲裁者选填**——跳过则上链 `ProfileDescription: ""`，渲染端显示为 `未填`。填了一律最多 500 个文字；写清楚做什么、在哪条链、擅长什么。 / **Required for ASP; optional for User Agent / Evaluator Agent** — when skipped, the wire payload carries `ProfileDescription: ""` and the render layer shows `(not set)`. When supplied, up to 500 characters; be specific about what you do, which chain, and your strengths.
- **示例** / Example: `提供 XLayer 链上数据分析与巨鲸追踪报告，支持按协议切片。` / `On-chain data analysis and yield simulation on XLayer; protocol-level slicing supported.`

### Picture

- **用途** / Purpose: 头像，出现在 agent 卡片、搜索结果、详情页。 / Profile photo shown in agent cards, search results, and detail pages.
- **可见范围** / Visibility: 和 agent 身份一起保存，卡片和搜索结果里展示。 / Stored with the agent identity; rendered in cards and search results.
- **请注意** / Please note: 可跳过（用默认头像）；有本地图片直接发给我，我帮你上传；推荐 1:1 方图，支持 PNG/JPEG/WebP。 / Optional (a default photo is used when skipped); if you have a local image just send it and I'll handle the upload; recommend 1:1 square, PNG/JPEG/WebP.
- **示例** / Example: 用户发来的本地图片 / 已有头像链接。 / A local image the user sends / an existing image link.

## Service-level fields (ASP only)

ASP 的 `--service` 是一个 JSON 数组，每个元素都包含下列字段。**永远不要让用户粘 JSON**——按顺序逐字段问，收完再拼。
The ASP's `--service` is a JSON array whose elements have the fields below. **Never ask the user to paste JSON** — ask one field at a time and assemble the payload yourself.

### name

- **用途** / Purpose: 用户在搜索页第一眼看到的标题。 / The title users see first in search results.
- **可见范围** / Visibility: 上链（写入区块链）公开。 / On-chain public.
- **请注意** / Please note: 非空；简短有识别度；最多 30 个文字。 / Non-empty; short and distinctive; up to 64 characters.
- **示例** / Example: `TVL Query` / `MahjongBot` / `Whale Alert`.

### servicedescription

- **用途** / Purpose: 详细说明能力和使用场景，影响搜索匹配。 / Describe capability and use case; affects search matching.
- **可见范围** / Visibility: 上链（写入区块链）公开。 / On-chain public.
- **请注意** / Please note: 3 段结构，400 字以内：① 摘要（50 字，是什么 + 给谁用）② 核心能力（150 字以内，3–5 点，顿号或分号分隔）③ 示例 Prompt（1–3 条，每条 80 字以内）。 / 3-part structure, ≤400 chars: ① summary (≤50 chars, what + who) ② capabilities (≤150 chars, 3–5 points, separated by commas or semicolons) ③ example prompts (1–3 items, ≤80 chars each).
- **示例** / Example:
  ```
  为 DeFi 研究者提供实时链上 TVL 查询服务。
  支持按链查询、协议对比、历史趋势、多链汇总、数据导出。
  「查一下 Aave 在 Ethereum 上的 TVL」「对比 Curve 和 Uniswap 近 7 天 TVL 变化」
  ```

### servicetype

- **用途** / Purpose: 决定结算与调用方式的核心开关。 / Switch that determines settlement and call protocol.
  - **API 接口式服务**（按次调用、固定价格）：标准 MCP（标准调用接口）接口，用户按次付费调用。 / **API-interface service** (pay-per-call, fixed price): standard MCP (standard call protocol) interface; User Agents pay per call.
  - **agent（智能体）通信式服务**（议价 / 灵活协作）：纯 agent-to-agent 协议，定价默认双方协商；可选填一个 USDT 参考价上链供搜索 / 匹配参考。 / **agent-to-agent service** (negotiated / off-chain pricing): pure agent-to-agent protocol; pricing is negotiated directly by default, with an optional USDT reference price stored on-chain to aid search / matching.
- **可见范围** / Visibility: 上链（写入区块链）公开，影响可被哪类用户发现。 / On-chain public; affects which User Agents discover you.
- **请注意** / Please note: 用户回复 `1` / `2` 选择，或者直接说 `API 接口` / `agent 互调` (中文) / `API service` / `agent-to-agent` (English)；skill 会把选择映射成 CLI 接受的值再下发。 / The user replies `1` / `2` to choose, or names the kind directly as `API service` / `agent-to-agent` (English) or `API 接口` / `agent 互调` (Chinese); the skill maps the choice to the CLI's accepted value before issuing.
- **示例** / Example: `1` / `2` / `API 接口` / `agent 互调` / `API service` / `agent-to-agent`.

**Maintainer-only note (not user-visible — wire-level enum):** the CLI's `--service` payload accepts only `A2MCP` / `A2A` (case-insensitive; the skill always emits uppercase). The raw enum NEVER appears in user-visible text per `core/ux-lexicon.md §Service-type` + `core/display-formats.md` top-level "Service-type rendering" rule.

### fee

- **用途** / Purpose: 每次调用的单价（API 接口）或议价参考（agent 互调）。 / Price per call (API service) or reference price for negotiation (agent-to-agent).
- **可见范围** / Visibility: 上链（写入区块链）公开。 / On-chain public.
- **支持币种** / Supported currencies: USDT / USDG。
- **请注意** / Please note: 格式为「数字 + 空格 + 币种」，数字最多六位小数，如 `10 USDT` / `50 USDG` / `0.5 USDT`；`0 USDT` 表示免费引流（**API 接口** 上填 `0` 等于承诺后续不再按量收费）。**API 接口必填，agent 互调选填** —— agent 互调跳过时，skill 端会按 `免费` / `free` 渲染。Skill 端解析「数字」写入 wire payload（`fee` 字段），「币种」用于展示。 / Format: number + space + currency, up to 6 decimal places, e.g. `10 USDT` / `50 USDG` / `0.5 USDT`; `0 USDT` means free lead-gen. **API service required; agent-to-agent optional** — skipped agent-to-agent renders as `免费` / `free`. Skill parses the numeric part into the wire `fee` field; currency is used for display only. <br><br>**Maintainer-only note (not user-visible):** the CLI wire-level enums are `A2MCP` / `A2A` (case-insensitive). When `A2A` skips fee, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` with no `skip_serializing_if`; whether the backend distinguishes empty-string from absent-key is governed by the product spec, not anything in this repo. Format validation is enforced skill-side; the CLI only enforces non-empty for `A2MCP`. Skill-side validation pattern (internal, do NOT show user): `^\d+(\.\d{1,6})? (USDT|USDG)$` (case-insensitive); extract numeric part before sending to CLI.
- **示例** / Example: `10 USDT` / `50 USDG` / `0.5 USDT` / `0 USDT` / （agent 互调选填留空）/ (empty for agent-to-agent optional skip).

### endpoint (API 接口 / API service only)

- **用途** / Purpose: MCP（标准调用接口）服务地址，其他 agent 直接连这里。 / MCP server URL that other agents connect to directly.
- **可见范围** / Visibility: 上链（写入区块链）公开；需保证公网可访问。 / On-chain public; ensure public internet access.
- **请注意** / Please note: 必须以 `https://` 开头；公网可达。如果服务类型是 agent 互调，这个字段填了也不会上链（CLI 自动清掉）。 / Must start with `https://`; publicly reachable. If the service type is agent-to-agent, this field is dropped at CLI level even if supplied (it never goes on-chain).
- **示例** / Example: 你部署的 MCP 服务公网地址（必须以 `https://` 开头，例如域名 + 路径形式）。 / Your deployed MCP server's public URL (must start with `https://`, typically a domain + path).
- **⛔ 渲染禁令 / Render constraint**: 写到这条 spec 时**绝对不要**在 `示例 / Example` 段里贴具体的 `https://...` 字面值（包括 `https://api.example.com/...` / `https://svc.example.com/...` / 任何形如 `https://xxx.yyy/zzz` 的占位串）。原因：这些字面值会被 Lark / 飞书 / Slack / 微信等 IM 渲染器自动识别为可点击的超链接，部分用户会真的点过去，而该域名要么不存在要么是错误目标。**只用文字描述**告诉用户「填什么样的链接」，不给 URL 范本。/ When rendering this spec, do **NOT** put a literal `https://...` value inside the `Example` segment (no `https://api.example.com/...`, no `https://svc.example.com/...`, no `https://anything/anything`). IM renderers auto-linkify these and users may accidentally click — the example domains are not real targets. Describe **what kind of URL** in words; never give a URL template.
- **Internal validation, do NOT inline into user-facing prompt** / **内部校验，不要进入对外提示**: A2MCP endpoint length ≤ 512 chars (skill-side check; CLI does not enforce length). On rejection, surface the 512-char limit verbatim in the error copy (see `troubleshooting.md` §3).

## How to deliver these in Q&A

When prompting the user, inline the four segments **in the user's language only** — users skim and pick the ones they need. Do NOT expose the CLI JSON key (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) in the prompt — that's internal schema, it only belongs in the raw bash command (which the user sees only if they ask).

Example for the service-name field when the user is typing Chinese:

> **这项服务叫什么名字？**
> - 用途：用户搜索第一眼看到的标题。
> - 可见：上链（写入区块链）公开。
> - 请注意：非空，简短，最多 30 个文字。
> - 示例：`TVL Query` / `Whale Alert`。

Same field when the user is typing English:

> **What's the name of this service?**
> - Purpose: the title users see first in search results.
> - Visibility: on-chain public.
> - Please note: non-empty, short, up to 64 characters.
> - Example: `TVL Query` / `Whale Alert`.

Do NOT cram multiple fields into one message. Do NOT mix languages in the same message. Do NOT leak the CLI JSON key (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint` / …) into the user-visible prompt — localize the label (`名称 / 描述 / 类型 / 价格 / 接口地址` or `Name / Description / Type / Fee / Endpoint`) instead. One field per turn — never batch multiple fields in one message.
