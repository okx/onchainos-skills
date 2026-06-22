# 任务发布新流程 — 从意图识别到调用 create 接口

## 1. 背景

### 1.1 现有流程

用户说"发布任务" → 采集任务字段（description/budget/currency/deadlines） → 确认 → `create-task` 上链。ASP 选择和服务发现发生在**任务创建之后**（`job_created` 事件触发 `recommend`）。

### 1.2 新流程目标

ASP 选择和服务参数提取**前置到创建之前**，任务创建时即携带 ASP + 服务入参，一步到位。

### 1.3 核心接口变更

| 变更 | 说明 |
|------|------|
| 新增 `POST /priapi/v1/aieco/task/asp/match` | 替代旧 `recommend`，创建前查询匹配 ASP + 服务 |
| 改动 `POST /priapi/v1/aieco/task/create` | 新增 `serviceId`/`serviceParams`/`serviceTokenAddress`/`serviceTokenAmount` 字段 |
| 删除 `POST {jobId}/setProviderAndAgentId` | 不再需要，ASP 在创建时确定 |
| 删除 `POST {jobId}/setTokenAndBudget` | 不再需要 |
| 改动 `POST {jobId}/setVisibility` | 改为直接修改 DB，链下存储，端上无需签名广播 |
| 新增 `POST {jobId}/asp/reject` | ASP 拒单 |
| 新增 `POST {jobId}/user/reject` | User 拒单 |
| 新增 `POST {jobId}/reset/asp` | ASP 拒单后 User 清理 ASP/服务信息 |
| 新增 `POST {jobId}/set/asp` | User 重新设置 ASP + 服务 |

---

## 2. 整体流程

```
开始
  │
  ▼
Step 1: 采集基础任务字段 + 识别是否指定 ASP
  │
  ▼
Step 2: 区分 ──────────────────────────────┐
  │                                         │
  │ 指定 ASP                                 │ 未指定 ASP
  ▼                                         ▼
Step 3A: asp/match                       Step 3B: asp/match
  (带 providerAgentId)                     (不带 providerAgentId)
  → 取回该 ASP 最高分服务                    → 取回推荐 ASP 列表
  │                                         │
  │                                         ▼
  │                                      Step 4B: 引导用户选择 ASP
  │                                         │
  ▼─────────────────────────────────────────┘
  │
  ▼
Step 5: LLM 推理 serviceParams
  （基于 serviceDescription + taskDesc）
  │
  ▼
Step 6: 确认表单（基础字段 + ASP + 服务 + serviceParams）
  │
  ▼
Step 7: 调用 create-task
```

---

## 3. 各步骤详细需求

### Step 1: 采集基础任务字段

**触发**：用户意图匹配 "发布任务 / create a task / 帮我发个任务 / publish task"

**采集内容**：

| 字段 | CLI 参数 | 约束 | 采集方式 |
|------|---------|------|---------|
| description | --description | 20-2000 chars | 用户提供 |
| title | --title | ≤30 chars | Agent 生成 |
| summary | --description-summary | ≤200 chars | Agent 生成 |
| currency | --currency | USDT / USDG | 用户确认（不默认） |
| budget | --budget | >0, ≤5位小数, ≤10,000,000 | 用户提供（不猜测） |
| max-budget | --max-budget | **必传**; ≥ budget | 用户提供（不猜测） |
| deadline-open | --deadline-open | 10min - 6months | 用户提供（不猜测） |
| deadline-submit | --deadline-submit | 1min - 6months | 用户提供（不猜测） |
| provider (可选) | --provider | agentId | 用户提供；不主动询问 |

**变更点**：
- `max-budget` 改为**必传**（后端要求）
- 识别用户是否指定了 ASP，输出 `has_designated_provider: bool`

**指定 ASP 的信号**：用户提及 agentId（数字 ID）、Agent 名称、"用 XX 的服务"、"指定 XX 做"
**未指定 ASP 的信号**：用户未提及任何 provider 信息，或说"帮我找人做"、"谁能做"

### Step 2: 分支判断

纯条件判断：`providerAgentId` 非空 → Step 3A；空 → Step 3B。

### Step 3A: 指定 ASP — 查询其最高分服务

**调用接口**：

```json
POST /priapi/v1/aieco/task/asp/match
{
  "taskDesc": "<用户的任务描述>",
  "providerAgentId": "<指定的 agentId>"
}
```

> 此时任务尚未创建，无 jobId，用 `taskDesc`。

**处理返回**：
- `recommendations[0].services[0]` 即为最高分服务（后端已过滤，只返回一个匹配分最高的服务）
- 提取字段：

| 字段 | 用途 |
|------|------|
| `serviceId` | 创建任务时传入 |
| `serviceName` | 展示给用户 |
| `serviceDescription` | LLM 推理 serviceParams 的依据 |
| `serviceType` | A2MCP / A2A，展示给用户 |
| `feeAmount` | 服务价格，创建任务时传入 `serviceTokenAmount` |
| `feeToken` | 服务代币合约地址，创建任务时传入 `serviceTokenAddress` |
| `feeTokenSymbol` | 展示给用户 + 币种一致性校验 |
| `endpoint` | A2MCP 场景使用 |

**校验规则**：
1. 返回为空（该 ASP 无服务）→ 告知用户"该 ASP 暂无已注册服务，请选择其他 ASP 或去掉指定"
2. 任务 `currency` 必须与 `feeTokenSymbol` 一致 → 不一致则拦截："任务支付币种（{currency}）与该服务的计费币种（{feeTokenSymbol}）不一致，请修改任务币种或选择其他 ASP。"
3. `max-budget` ≥ `feeAmount` → 不满足则拦截："任务最高预算（{max-budget}）低于服务价格（{feeAmount} {feeTokenSymbol}），请提高最高预算。"

→ 校验通过后进入 Step 5。

### Step 3B: 未指定 ASP — 查询推荐列表

**调用接口**：

```json
POST /priapi/v1/aieco/task/asp/match
{
  "taskDesc": "<用户的任务描述>",
  "page": 1
}
```

**返回数据结构**（每个 ASP 附带唯一最高分服务）：

```json
{
  "recommendations": [
    {
      "providerAgentId": "agent_001",
      "securityRate": 0.95,
      "feedbackRate": 0.88,
      "soldCount": 12,
      "categoryCode": ["development", "audit"],
      "tagCodes": ["solidity", "defi"],
      "supportA2MCP": true,
      "services": [
        {
          "serviceId": "svc_001",
          "serviceName": "Smart Contract Audit",
          "serviceDescription": "Professional smart contract security audit",
          "serviceType": "A2MCP",
          "endpoint": "https://agent.example.com/a2mcp",
          "feeAmount": 100.0,
          "feeToken": "0xtoken_address",
          "feeTokenSymbol": "USDT"
        }
      ]
    }
  ],
  "nextPage": 2
}
```

**展示格式**：

```
为您匹配到以下服务商：

1. Agent 001 — 安全评分: 0.95 | 好评率: 0.88 | 已完成 12 单
   服务：Smart Contract Audit (A2MCP) — 100 USDT
   「Professional smart contract security audit」

2. Agent 002 — 安全评分: 0.90 | 好评率: 0.92 | 已完成 8 单
   服务：Code Review (A2A) — 50 USDT
   「Thorough code review service」

请选择序号，或输入"更多"查看下一页。
```

> 展示语言跟随用户会话语言。

### Step 4B: 引导用户选择 ASP

**交互规则**：
- 用户输入数字序号 → 选定对应 ASP + 其最高分服务
- 用户输入"更多 / 下一页 / next" → 调 `asp/match` page+1
- 列表为空 → "暂未匹配到合适的服务商，请调整任务描述后重试。"

**选定后执行 Step 3A 的校验**（币种一致性、预算 ≥ 服务价格）→ 通过后进入 Step 5。

### Step 5: LLM 推理 serviceParams

**输入**：
- `serviceDescription`：服务描述（来自 asp/match 返回）
- `serviceName`：服务名称
- 用户的 `description`：任务描述

**说明**：服务没有 formal inputSchema 字段。`serviceParams` 是一个自由格式的 JSON 字符串，直接存入 DB（`text` 类型）。LLM 需要根据服务描述 + 任务描述来推理出 `serviceParams` 应该填什么。

**LLM 推理逻辑**：

1. 从 `serviceDescription` 中识别该服务接受什么类型的输入
2. 从用户 `description` 中提取能匹配上的具体值
3. 生成 `serviceParams` JSON

**示例**：
- 服务：`"Smart Contract Audit"` — `"Professional smart contract security audit"`
- 任务描述：`"帮我审计 0x1234...abcd 这个合约，在 ETH 链上"`
- LLM 推理 serviceParams：`{"contractAddress": "0x1234...abcd", "chain": "ETH"}`

**如果 LLM 无法推理出任何参数**（服务描述太泛、任务描述不含可提取信息）→ `serviceParams` 为空字符串，不阻塞流程。

### Step 6: 确认表单

在现有确认表单基础上新增服务相关行。**一次确认覆盖全部字段**，不分多轮。

**表单模板**：

```
| 字段         | 值                                            |
|-------------|-----------------------------------------------|
| 标题         | <title, ≤30 chars>                             |
| 摘要         | <summary, ≤200 chars>                          |
| 描述         | <description 全文或 "见下方">                    |
| 支付代币      | <USDT 或 USDG>                                 |
| 预算         | <budget>                                       |
| 最高预算      | <max-budget>                                   |
| 任务过期时间   | <deadline-open>                                |
| 预期工作时长   | <deadline-submit>                              |
|-------------|-----------------------------------------------|
| 指定服务商    | Agent <providerAgentId>                        |
| 服务名称      | <serviceName> (<serviceType>)                  |
| 服务价格      | <feeAmount> <feeTokenSymbol>                   |
| 服务参数      | <serviceParams 可读展示，或 "无">                |

> 确认无误？确认后我立即创建任务。
```

**字段标签语言**：跟随用户会话语言（中文会话 → 中文标签，英文会话 → 英文标签）。

**确认表单的准确度保障**：
- 用户可看到 LLM 推理出的 `serviceParams`，发现错误可纠正
- 币种/预算校验已在 Step 3 完成，此处直观展示
- 服务信息（名称、价格、类型）透明可见

**用户响应路由**：
- 确认 → Step 7
- 修改基础字段 → 回到 Step 1 修改
- 修改 ASP → 回到 Step 3B 重新选择
- 修改服务参数 → 更新 serviceParams，重新展示表单
- 保存草稿 → 走草稿路径（见 §4.2）

### Step 7: 调用 create-task

```bash
onchainos agent create-task \
  --description "<description>" \
  --description-summary "<summary>" \
  --title "<title>" \
  --budget <budget> --max-budget <max_budget> \
  --currency <USDT|USDG> \
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \
  --provider <providerAgentId> \
  --service-id <serviceId> \
  --service-params '<serviceParams JSON>' \
  --service-token-address <feeToken> \
  --service-token-amount <feeAmount>
```

**CLI 参数 → 后端接口字段映射**：

| CLI 参数 | 接口字段 | 来源 |
|---------|---------|------|
| --provider | providerAgentId | 用户指定或从列表选择 |
| --service-id | serviceId | asp/match 返回 |
| --service-params | serviceParams | LLM 推理 + 用户确认 |
| --service-token-address | serviceTokenAddress | asp/match 返回的 feeToken |
| --service-token-amount | serviceTokenAmount | asp/match 返回的 feeAmount |
| (现有字段) | title, description, descriptionSummary, paymentTokenSymbol, paymentTokenAmount, paymentMostTokenAmount, deadlines | 用户提供 + Agent 生成 |

**创建后行为**：
- 通知用户 jobId（不说"发布成功"，尚未上链确认）
- 不再调用 `recommend`（ASP 已在创建前确定）
- 等待 `job_created` 或 `job_asp_selected` 事件驱动后续流程

---

## 4. 草稿流程适配

### 4.1 创建草稿

指定 ASP 时必须带上服务相关参数：

```bash
onchainos agent draft create \
  --title "<title>" --description "<desc>" --description-summary "<summary>" \
  [--budget <num>] [--max-budget <num>] [--currency <USDT|USDG>] \
  [--provider <agentId>] \
  [--service-id <serviceId>] \
  [--service-params '<JSON>'] \
  [--service-token-address <addr>] \
  [--service-token-amount <amount>]
```

**约束**：传 `--provider` 时，`--service-id`/`--service-params`/`--service-token-address`/`--service-token-amount` 必须同时传入。

### 4.2 更新草稿

同理，更新 provider 时必须同时更新服务字段。

### 4.3 发布草稿

发布前校验：已有 provider → 检查服务字段是否完整；如缺失 → 调 `asp/match` 补全。

---

## 5. 新增事件处理

### 5.1 事件注册

需在 `state_machine.rs` 注册以下新事件：

| 事件名 | Event 枚举 | 接收方 | 含义 |
|--------|-----------|--------|------|
| `job_asp_selected` | `JobAspSelected` | ASP | User 选定了该 ASP，ASP 决策是否接单 |
| `job_provider_reject` | `JobProviderReject` | User | ASP 拒单了，User 需换 ASP |
| `job_user_reject` | `JobUserReject` | ASP | User 拒了该 ASP |

### 5.2 事件 envelope

**job_asp_selected**（推送给 ASP）：

```json
{
  "agentId": "<asp_agentId>",
  "message": {
    "event": "job_asp_selected",
    "source": "system",
    "jobId": "<jobId>",
    "jobStatus": "open",
    "jobTitle": "<title>",
    "serviceParams": "<JSON>",
    "tokenAmount": "<amount>",
    "tokenSymbol": "<symbol>",
    "isDirectCommunication": true,
    "providerAgentId": "<asp_agentId>"
  }
}
```

**job_provider_reject**（推送给 User）：

```json
{
  "agentId": "<user_agentId>",
  "message": {
    "event": "job_provider_reject",
    "source": "system",
    "jobId": "<jobId>",
    "jobStatus": "open",
    "jobTitle": "<title>",
    "providerAgentId": "<拒单的 asp agentId>"
  }
}
```

**job_user_reject**（推送给 ASP）：

```json
{
  "agentId": "<asp_agentId>",
  "message": {
    "event": "job_user_reject",
    "source": "system",
    "jobId": "<jobId>",
    "jobStatus": "open",
    "jobTitle": "<title>",
    "userAgentId": "<user_agentId>"
  }
}
```

### 5.3 事件处理逻辑

**JobAspSelected（ASP 侧）**：
1. 拉取任务详情获取 serviceParams
2. 通知 ASP 用户：有新任务选中你，展示任务信息 + 服务参数
3. 引导 ASP 决策：接单 → `apply`；拒单 → `asp/reject`

**JobProviderReject（User 侧）**：
1. 通知用户：ASP 拒单了
2. 调 `reset/asp` 清理 ASP/服务信息
3. 引导用户：重新选择 ASP（调 `asp/match`）或关闭任务
4. 用户选定新 ASP 后 → 调 `set/asp` 设置新 ASP + 服务

**JobUserReject（ASP 侧）**：
1. 通知 ASP 用户：User 不需要你了
2. 静默结束

---

## 6. 新增 CLI 命令

| CLI 命令 | 后端接口 | 用途 |
|---------|---------|------|
| `onchainos agent asp-match` | `POST /priapi/v1/aieco/task/asp/match` | 查询匹配 ASP + 服务 |
| `onchainos agent asp-reject <jobId>` | `POST /priapi/v1/aieco/task/{jobId}/asp/reject` | ASP 拒单 |
| `onchainos agent user-reject <jobId>` | `POST /priapi/v1/aieco/task/{jobId}/user/reject` | User 拒单 |
| `onchainos agent reset-asp <jobId>` | `POST /priapi/v1/aieco/task/{jobId}/reset/asp` | 清除 ASP/服务信息 |
| `onchainos agent set-asp <jobId>` | `POST /priapi/v1/aieco/task/{jobId}/set/asp` | 重新设置 ASP + 服务 |

### 6.1 asp-match 参数设计

```
onchainos agent asp-match \
  [--task-desc "<描述>"] \
  [--job-id <jobId>] \
  [--provider <agentId>] \
  [--page <N>]
```

- `--task-desc` 和 `--job-id` 至少传一个，都传以 `--job-id` 为准
- `--provider` 非空 → 取该 ASP 最高分服务；为空 → 取推荐列表
- `--page` 默认 1

### 6.2 set-asp 参数设计

```
onchainos agent set-asp <jobId> \
  --provider <agentId> \
  --service-id <serviceId> \
  --service-params '<JSON>' \
  --service-token-address <addr> \
  --service-token-amount <amount> \
  [--currency <symbol>] \
  [--budget <amount>] \
  [--max-budget <amount>]
```

---

## 7. 删除项

| 删除项 | 原因 |
|--------|------|
| `onchainos agent recommend` 命令 | 被 `asp-match` 替代 |
| `buyer/recommend.rs` 模块 | 被 `asp_match` 替代 |
| `POST {jobId}/setProviderAndAgentId` 接口调用 | 后端删除 |
| `POST {jobId}/setTokenAndBudget` 接口调用 | 后端删除 |
| `job_created` 后自动触发 recommend 逻辑 | 创建前已选好 ASP |
| `job_created` playbook 中的 recommend 分支 | 不再需要 |

---

## 8. serviceParams 准确度保障方案

### 8.1 方案：扩展现有确认表单

**一次确认覆盖全部字段**（基础字段 + ASP + 服务 + serviceParams），不新增额外交互轮次。

### 8.2 选择理由

| 备选方案 | 优点 | 缺点 | 结论 |
|---------|------|------|------|
| A. 扩展确认表单（推荐） | 成本低（现有机制扩展）；一次确认全部字段；用户可纠正 | 表单略长 | **采用** |
| B. 独立 serviceParams 确认轮次 | serviceParams 独立确认，更聚焦 | 多一轮交互延迟；用户体验割裂 | 不采用 |
| C. 无确认直接创建 | 最快 | serviceParams 无 schema 约束，LLM 推理出错无兜底 | 不采用 |

### 8.3 多层校验保障

| 层次 | 校验内容 | 时机 |
|------|---------|------|
| 第一层：LLM 推理 | 从 serviceDescription + taskDesc 提取参数 | Step 5 |
| 第二层：确认表单 | 用户肉眼确认 serviceParams，可纠正 | Step 6 |
| 第三层：后端校验 | 币种一致性、max-budget ≥ 服务价格 | Step 7 create-task 调用时 |

---

## 9. 约束与校验汇总

| 校验规则 | 执行时机 | 失败处理 |
|---------|---------|---------|
| max-budget 必传 | Step 1 字段采集 | 提示用户必须设置 |
| currency 仅 USDT / USDG | Step 1 字段采集 | 提示用户选择 |
| 任务 currency 与服务 feeTokenSymbol 一致 | Step 3A/4B 选定 ASP 后 | 拦截，提示修改币种或换 ASP |
| max-budget ≥ feeAmount | Step 3A/4B 选定 ASP 后 | 拦截，提示提高最高预算 |
| 指定 provider 时，服务字段必须同时传入 | Step 7 / 草稿创建 | CLI 校验拦截 |
| description ≥ 20 chars | Step 1 字段采集 | 提示用户补充 |
| title ≤ 30 chars | Step 1 Agent 生成后 | Agent 自动缩短 |

---

## 10. 与现有流程对比

| 维度 | 现有流程 | 新流程 |
|------|---------|--------|
| ASP 选择时机 | 创建后（job_created → recommend） | **创建前**（Step 3） |
| 服务发现 | 协商阶段 provider 自报 | **创建前**后端查询 + 用户选择 |
| 服务入参 | 不携带；协商阶段自然语言交换 | **创建时即携带**，结构化传入 |
| 创建接口 | 仅基础字段 | 基础字段 + provider + serviceId + serviceParams + serviceToken |
| 指定 ASP 场景 | 创建后走 designated negotiation | 创建前查服务、提参数 |
| 未指定 ASP 场景 | 创建后 recommend → 选 ASP → negotiate | 创建前 asp/match → 选 ASP → 提参数 → 创建 |
| ASP 拒单 | 协商阶段 [intent:reject] | 新事件 job_provider_reject + reset/asp + set/asp |
| 事件驱动 | job_created → recommend → negotiate → apply | job_created / job_asp_selected → ASP accept/reject |
