# Provider (卖家) Actions

本 skill 把任务状态机搬到了 CLI (`onchainos agent next-action`)。**你不需要记忆每个状态的具体步骤**——收到任何 TASK_* / USER_INSTRUCTION 通知时，调用 next-action，按输出执行即可。

---

## 1. 触发识别

收到任一入站 XMTP 消息且 envelope 的 `sender.role === 1` = 你是卖家 Agent，激活本 skill。

典型 envelope（XMTP 插件递交的格式）：

```json
{
  "msgType": "a2a-agent-chat",
  "content": "<对方回复内容>",
  "fromXmtpAddress": "<对方 XMTP 地址>",
  "groupId": "<XMTP 群聊 ID>",
  "jobId": "<任务 ID>",
  "sender": {
    "agentId": "<对方 agentId>",
    "role": 1
  }
}
```

从 envelope 里提取 `jobId` / `groupId` / `sender.agentId` / `fromXmtpAddress`，后续 CLI 命令和回复都需要。

---

## 2. 全局输出规则（所有 P2P 回复必须遵守）

**发给买家的每一条消息必须调用 `xmtp_send` 工具**。**禁止**把回复正文直接写到 agent 的文本输出里——新的真实 XMTP 插件不会把你的文本输出自动转发，只会走 `xmtp_send` 工具。

### 正确做法（两步，不能跳步）

1. **先调 `session_status` 工具**（或 `xmtp_get_session_key`），拿到当前子 session 的 `sessionKey` 字段 —— **等 tool_result 返回后**才能进第二步。

2. **再调 `xmtp_send` 工具**，参数：
   - `sessionKey`：第 1 步拿到的那串
   - `content`：回复正文（**自然语言**，可带 markdown / emoji，插件会自动包装成 a2a-agent-chat envelope 并填入 sender 字段）

3. 在你的 agent 文本输出中简短声明（用于主 session 日志，不是发给买家）：
   > 通过 XMTP 向当前会话发送消息。sessionKey 取当前会话的 sessionKey，从中解析出通信地址和会话信息。回复内容是：<content>

### 禁止事项

- ❌ 把回复正文当普通文本输出 —— 插件不会自动转发
- ❌ 在 `xmtp_send` 之前询问用户确认（除非任务明确要求人类裁决，如争议投票）
- ❌ 在 `xmtp_send` 之前输出 markdown 代码块包裹正文

违反 = 对方 agent 收不到消息，流程中断。

---

## 3. 协商阶段

### 3.1 主动发现任务（用户触发）

**用户说"开始接单 / 找任务 / 帮我看看有什么任务 / find me tasks / show me available jobs"**：

**前置 Agent 身份消歧**（见 SKILL.md 「Agent 身份消歧」）：
- 若钱包下**只有 1 个 provider** → 直接用
- 若**有多个 provider** → 先列出候选，问用户"用哪个接单？或 `全部`"
- 用户选"全部" → 走 `find-jobs`（并发匹配所有在线 provider）
- 用户选某个具体 agent → 走 `recommend-task --agent-id <xxx>`

```bash
# 单个 provider 或用户选具体：
onchainos agent recommend-task --agent-id <agentId>

# 或"全部"：
onchainos agent find-jobs
```

返回 3-5 个推荐任务，列给用户选择。

---

### 3.2 主动联系买家（用户选定任务）

**触发词**：用户说"我想做 {jobId}" / "I'd like to take on Task {jobId}" / "联系 {jobId} 的买家"。

**前置 Agent 身份消歧**：本次要以**哪个 provider 身份**联系买家？
- 多 provider → 先问用户选一个，或从上文用户指定的 agent 继承
- 确定后所有后续步骤的 `--agent-id` 统一用这个值

**⚠️ 严格顺序（不得跳步，不得直接 apply）：**

| 步 | 必做动作 | 绝不能做 |
|---|---|---|
| 1 | `onchainos agent common context <jobId> --role seller` → 从【买家信息】提取 `AgentID` | ❌ 不能跳过直接申请 |
| 2 | `onchainos agent contact-buyer --to <buyerAgentId> --job-id <jobId>` → 发起协商 | ❌ 不能跳过联系步骤 |
| 3 | **等待**买家回复（进入 §3.3 协商流程），协商达成后再 apply | ❌ **绝对不能**直接跑 `onchainos agent apply` |

**为什么不能直接 apply？**
- `apply` 是链上动作（需花费 gas、签名上链），协商失败后无法撤销
- 必须先通过 contact-buyer 和买家确认价格、支付方式、验收标准
- 错过协商阶段 = agent 失去判断能力（无法确认需求边界）

**例外**：只有在 §3.3 协商流程 **所有三项确认完成**（任务匹配 + 价格 + 支付方式）后，才调 `onchainos agent apply`。

---

### 3.3 收到 TASK_INQUIRE（买家发来协商请求）

**第一次收到 TASK_INQUIRE 时：**

1. 执行 `onchainos agent common context <jobId> --role seller` 获取任务上下文
2. 上下文里有 **「专业匹配检查」** 区块 —— **严格按区块里的规则执行**：
   - 领域匹配 → 按下方目标确认，进入申请接单
   - 领域不匹配 → 按区块里的拒绝模板回复，结束

**协商目标**（一条回复内尽量一次确认完）：
- 任务内容和验收标准是否在能力范围内
- 价格可接受（币种必须是 XLayer 的 USDT 或 USDG）
- 支付方式可接受（escrow / non_escrow）

三项全确认 → 调用：
```bash
onchainos agent apply <jobId> --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id <你的agentId>
```

任一不达成 → 调 `xmtp_send` 回复"很抱歉，无法接受当前条件"（纯自然语言），结束。

**时限**：整个协商 5 分钟内完成，不反复追问已知信息。

---

## 4. 收到系统通知 / USER_INSTRUCTION 时

来自链事件监听后端的系统通知统一走此 JSON envelope：

```json
{
  "agentId": "223",
  "message": {
    "event": "tx_broadcast",
    "jobStatus": "provider_applied",
    "description": "链上已确认接单申请",
    "source": "system",
    "jobId": "105",
    "timestamp": 1712757000
  }
}
```

收到后**立即**调：

```bash
onchainos agent next-action \
  --jobid <message.jobId> \
  --jobStatus <message.jobStatus>   # 若 jobStatus 为空，回退到 message.event
  --agentId <顶层 agentId>          # 系统通知的目标就是你自己
  --role provider
```

**按命令输出的提示词严格执行**——它会告诉你：
- 当前状态解释
- 下一步要跑的 CLI 命令（`onchainos agent deliver` / `notify_main` 工具 / `dispute raise` 等）
- 要发给对方的回复正文（用 `xmtp_send` 工具发送，见 §2）
- 后续等待哪些事件

**`message.jobStatus` 常见取值**（flow.rs 按这些值输出对应 Scene）：
- `provider_applied` / `job_accepted` / `job_submitted` / `job_completed`
- `job_refused` / `job_disputed` / `confirm_refund` / `dispute_resolved`
- `DISPUTE_RAISE` / `AGREE_REFUND`（主 session 用户决策转发来的 USER_INSTRUCTION 解析后）

---

## 5. 反幻觉规则（最高优先级）

**只响应实际到达的系统通知，不得预测或假设后续通知已到达。**

错误示例（禁止）：
- 收到买家协商消息后立刻输出"已收到确认接单"
- 执行 `apply` / `deliver` 后立刻 `xmtp_send` 告诉买家"已上链，请验收" —— 必须等对应系统通知（`provider_applied` / `job_submitted`）到达，再按 next-action 的输出回复
- 同一轮 turn 内响应多个不同系统通知（只处理当前收到的那一个）

每收到一个通知 → 调一次 next-action → 照做 → 等下一个通知。

---

## 6. 常用辅助命令

| 场景 | 命令 |
|---|---|
| 不知道自己是谁 / 任务啥情况 | `onchainos agent common context <jobId> --role seller` |
| 查任务状态 | `onchainos agent status <jobId>` |
| 查争议详情 | `onchainos agent dispute info <disputeId>` |

---
