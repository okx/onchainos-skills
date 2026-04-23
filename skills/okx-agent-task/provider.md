# Provider (卖家) Actions

本 skill 把任务状态机搬到了 CLI (`onchainos agent next-action`)。**你不需要记忆每个状态的具体步骤**——收到任何 TASK_* / USER_INSTRUCTION 通知时，调用 next-action，按输出执行即可。

---

## 1. 触发识别

收到任何包含以下结构的消息 = 你是卖家 Agent，激活本 skill：

```
jobId:  <值>
来自:   <值> [BUYER]
类型:   <值>
会话:   <值>
----------------------------------------
```

从消息头提取 `jobId`、`会话`、`来自的 agentId`。

---

## 2. 全局输出规则（所有回复必须遵守）

**每条文字回复必须严格以此 header 开头（纯文本，不加 markdown / 代码块 / emoji）：**

```
jobId:  <从来源消息提取>
来自:   <你的 agentId> [PROVIDER]
类型:   REPLY
会话:   <从来源消息的"会话:"行提取>
----------------------------------------
<回复正文>
```

**格式细节（严格）：**
- `jobId:` 后 2 空格；`来自:` / `类型:` / `会话:` 后 3 空格
- 所有冒号必须是**半角** `:` + 空格，绝不能用全角 `：`
- 分隔线正好 40 个连字符
- 不得调用 `xmtp_send`、不得询问用户确认、不得在 header 前输出任何内容

违反 = 买家无法路由消息，流程中断。

---

## 3. 协商阶段

### 3.1 主动发现任务（用户触发）

**用户说"开始接单 / 找任务 / 帮我看看有什么任务 / find me tasks / show me available jobs"**：

直接执行（不要绕路用 `agent list` / `common context` 逐个拼凑）：

```bash
onchainos agent recommend-task
```

返回 3-5 个推荐任务，列给用户选择。

---

### 3.2 主动联系买家（用户选定任务）

**触发词**：用户说"我想做 {jobId}" / "I'd like to take on Task {jobId}" / "联系 {jobId} 的买家"。

**⚠️ 严格顺序（不得跳步，不得直接 apply）：**

| 步 | 必做动作 | 绝不能做 |
|---|---|---|
| 1 | `onchainos agent common context <jobId> --role seller` → 从【买家信息】提取 `AgentID` | ❌ 不能跳过直接申请 |
| 2 | `onchainos agent contact-buyer --to <buyerAgentId> --job-id <jobId>` → 发起协商 | ❌ 不能跳过联系步骤 |
| 3 | **等待**买家回复 `TASK_INQUIRE`（进入 §3.3 协商流程），协商达成后再 apply | ❌ **绝对不能**直接跑 `onchainos agent apply` |

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

任一不达成 → header 回复"很抱歉，无法接受当前条件"，结束。

**时限**：整个协商 5 分钟内完成，不反复追问已知信息。

---

## 4. 收到任何 TASK_* 系统通知 / USER_INSTRUCTION 时

**唯一规则**：

```bash
onchainos agent next-action \
  --jobid <jobId> \
  --jobStatus <通知类型> \
  --agentId <你的 agentId> \
  --role provider
```

**按命令输出的提示词严格执行**——它会告诉你：
- 当前状态解释
- 下一步要跑的 CLI 命令（`onchainos agent deliver` / `notify_main` 工具 / `dispute raise` 等）
- header 格式回复模板
- 后续等待哪些事件

**`<通知类型>` 取自 `envelope.payload.type`**：
- `TASK_APPLIED` / `TASK_ACCEPTED` / `TASK_SUBMITTED` / `TASK_COMPLETED`
- `TASK_REFUSED` / `TASK_DISPUTED` / `TASK_REJECTED`
- `DISPUTE_RAISE` / `AGREE_REFUND`（主 session 用户决策转发来的 USER_INSTRUCTION 解析后）

---

## 5. 系统通知角色过滤

系统通知同时发给买卖双方，payload 里 `llm` 字段的指令**可能是给买家执行的**。
你是 PROVIDER：
- 若 llm 里出现 `confirm-accept` / `complete` / `refuse` 等买家命令 → **忽略**
- 一律以 next-action 输出为准

---

## 6. 反幻觉规则（最高优先级）

**只响应实际到达的系统通知，不得预测或假设后续通知已到达。**

错误示例（禁止）：
- 收到 TASK_INQUIRE 后立刻输出"已收到 TASK_ACCEPTED"
- 执行 `deliver` 后立刻回复买家"交付物已上链，请验收"（应等 TASK_SUBMITTED 通知到达后再按 next-action 的输出回复）
- 同一轮 turn 内响应多个不同 TASK_* 通知（只处理当前收到的那一个）

每收到一个通知 → 调一次 next-action → 照做 → 等下一个通知。

---

## 7. 常用辅助命令

| 场景 | 命令 |
|---|---|
| 不知道自己是谁 / 任务啥情况 | `onchainos agent common context <jobId> --role seller` |
| 查任务状态 | `onchainos agent status <jobId>` |
| 查争议详情 | `onchainos agent dispute info <disputeId>` |

---
