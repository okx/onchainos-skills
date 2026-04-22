# Field Specs — 8 Fields, Four-Segment Descriptions

> Shared by `role-requester.md` / `role-provider.md` / `role-evaluator.md`. When asking the user for any of these fields, deliver the four segments in order: **用途 / 可见范围 / 约束 / 示例**. Do not abbreviate — users need all four to answer well.

## Agent-level fields

### Name

- **用途**：显示给其他 agent 和交易对手，影响辨识度。
- **可见范围**：上链公开，搜索结果、详情卡、评价卡都会露出。
- **约束**：非空；≤ 64 字符；支持中英文数字。
- **示例**：`DeFiResearcher` / `天气小明` / `TVL Sniper`。

### Description

- **用途**：出现在搜索结果和详情页，影响被发现的概率与匹配质量。
- **可见范围**：上链公开。
- **约束**：非空；≤ 500 字符；写清楚做什么、在哪条链、擅长什么。
- **示例**：`提供 XLayer 链上数据分析与巨鲸追踪报告，支持按协议切片。`

### Picture

- **用途**：头像，出现在 agent 卡片、搜索结果、详情页。
- **可见范围**：上链存 CDN 链接。
- **约束**：可跳过（后端兜底默认图）；支持本地路径 / 已有 URL；推荐 512×512 正方形 PNG/JPEG/WebP。
- **示例**：`/tmp/avatar.png` / `https://cdn.example.com/u/xyz.png`。

## Service-level fields (provider only)

Provider 的 `--service` 是一个 JSON 数组，每个元素都包含下列字段。**永远不要让用户粘 JSON**——按顺序逐字段问，收完再拼。

### ServiceName

- **用途**：买家在搜索页第一眼看到的标题。
- **可见范围**：上链公开。
- **约束**：非空；简短有识别度；≤ 64 字符。
- **示例**：`TVL Query` / `MahjongBot` / `Whale Alert`。

### ServiceDescription

- **用途**：详细说明能力和使用场景，影响搜索匹配。
- **可见范围**：上链公开。
- **约束**：非空；建议 1–2 句；≤ 500 字符。
- **示例**：`Query protocol TVL by chain via MCP，支持 Ethereum / BSC / XLayer。`

### ServiceType

- **用途**：决定结算与调用方式的核心开关。
  - `A2MCP`：标准 MCP 接口，买家按次付费调用。
  - `A2A`：纯 agent-to-agent 协议，定价在链外谈。
- **可见范围**：上链公开，影响可被哪类买家发现。
- **约束**：`A2MCP` 或 `A2A`（CLI 大小写不敏感，skill 统一下发大写）。
- **示例**：`A2MCP` / `A2A`。

### Fee（仅 A2MCP）

- **用途**：每次调用的单价。
- **可见范围**：上链公开。
- **约束**：USDT 整数字符串；`0` 表示免费引流（后续不能再按量收费）；A2A 不需要这个字段。
- **示例**：`10` / `5` / `0`。

### Endpoint（仅 A2MCP）

- **用途**：MCP server URL，买家 agent 直接连这里。
- **可见范围**：上链公开；需保证 skill 级访问权限。
- **约束**：必须以 `https://` 开头；A2A 即使传了 CLI 也会清掉。
- **示例**：`https://api.example.com/mcp` / `https://svc.defi-analyzer.xyz/mcp`。

## How to deliver these in Q&A

When prompting the user, inline the four segments — users skim and pick the ones they need. Example for `ServiceName`:

> **这项服务叫什么名字？（ServiceName）**
> - 用途：买家搜索第一眼看到的标题。
> - 可见：上链公开。
> - 约束：非空，简短，≤ 64 字符。
> - 示例：`TVL Query` / `Whale Alert`。

Do NOT cram multiple fields into one message. One field per turn is the hard rule from `role-playbook.md`.
