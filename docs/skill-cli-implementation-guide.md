# Skill + CLI 实现规范

本文说明如何实现一个可路由、可执行、可恢复的 Skill 及其 CLI。A2A 订阅是示例，规则也适用于普通任务、支付、交付和异步任务。

## 1. 先准备什么

完整产品流程图是起点，但不够。实现前至少准备以下物料：

- 生命周期流程：正常、失败、取消、超时、重试、恢复和终态。
- 用户确认点：哪些操作会产生写入、签名、扣款或发送消息。
- CLI 契约：命令、参数、JSON 成功/失败结构、错误码和重试规则。
- 协议字段：字段来源、类型、是否可改名、是否允许展示。
- 外部依赖：API、链、Provider、通信服务和状态查询接口。
- 验收样例：每个分支至少一条真实输入和预期输出。

流程图回答“发生什么”；CLI 契约回答“怎么执行”；Skill 规则回答“如何识别和推进”。三者缺一不可。

## 2. 文件职责

```text
SKILL.md
  意图识别、路由、全局边界、Reference 选择

references/<business>.md
  一个业务流程的字段、确认、命令、错误和恢复规则

references/task-output-templates.md
  用户结果、详情、下一步和编号交互的统一展示

CLI
  事实校验、外部调用、写入、结构化结果

watch reference
  异步等待、恢复、超时和终态
```

边界原则：CLI 不理解用户意图；Skill 不猜测后端事实；模板不决定业务是否允许执行。

## 3. Skill 的写法

### `SKILL.md` 放什么

只放跨场景都需要的内容：

- Skill 能力和排除范围
- 结构化输入优先级
- 意图路由表
- Reference 选择规则
- CLI 结果推进协议
- 确认和安全边界
- 输出模板入口

### Reference 放什么

按业务或生命周期拆分。只写执行该业务需要的规则：

- 输入字段和来源
- 参数收集
- 业务分支
- 命令组装
- 外部写入
- 成功后的 handoff
- 错误和恢复

不要把完整 API 手册、通用展示模板或重复的路由规则复制到每个 Reference。

## 4. CLI 结果契约

所有需要继续推进的命令应返回统一控制字段：

```json
{
  "phase": "payment_validation",
  "decision": "blocked",
  "reason": "insufficient_balance",
  "nextAction": [
    { "id": "fund_account", "recommend": true }
  ],
  "payload": {}
}
```

字段含义：

| 字段 | 含义 |
|---|---|
| `phase` | 当前业务阶段，不表示最终状态 |
| `decision` | 当前阶段的推进判定 |
| `reason` | 机器可读的结果或阻断原因 |
| `nextAction` | 当前可用动作，按顺序返回 |
| `payload` | 当前阶段的业务数据 |

`decision` 当前只使用：

```text
ready
blocked
requires_user_input
```

`nextAction` 使用稳定动作 ID，不放自然语言。`recommend=true` 标记推荐动作。用户界面将动作渲染成 1、2、3，Skill 只执行用户选择的动作。

不要用自然语言 `action` 作为状态机。迁移期间可以保留它，但新逻辑优先使用结构化字段。

## 5. 用户输出

统一由 `task-output-templates.md` 渲染：

```text
[Result]
一句话结论

[Details]
当前阶段需要的关键信息

[Next]
1. 推荐操作
2. 备选操作
3. 取消或返回
```

根据结果选择内容形式：

```text
ready                → 确认卡或表格
blocked              → 状态说明和恢复方案
requires_user_input  → 缺失字段列表
waiting              → 等待对象和进度
completed            → 成功摘要和后续操作
```

模板只负责表现。可用动作由 CLI 返回，业务是否允许执行由业务 Reference 和 CLI 决定。

## 6. A2A 订阅示例

### 6.1 正常路径

```text
用户提出订阅需求
  ↓
Skill 识别为新订阅意图
  ↓
读取服务发现 Reference
  ↓
展示推荐 Service，等待用户确认
  ↓
调用 task-create-prepare --sid <sid>
  ↓
CLI 校验登录、User Agent、服务类型、订阅能力、重复订阅和余额
  ↓
Skill 读取 decision
  ↓
读取 serviceGuide，收集执行配置
  ↓
解析 serviceDescription，收集 serviceParams
  ↓
生成最终订阅确认信息
  ↓
用户明确确认
  ↓
执行 communication-check
  ↓
执行 create-subscribe
  ↓
进入创建后的 deliverable 和 Watch 流程
```

### 6.2 `task-create-prepare` 的结果

余额不足：

```json
{
  "phase": "payment_validation",
  "decision": "blocked",
  "reason": "insufficient_balance",
  "nextAction": [{ "id": "fund_account", "recommend": true }],
  "payload": { "serviceId": "svc-1" }
}
```

Skill 应停止创建，说明原因，并渲染充值等可用动作。充值完成后，使用同一个 `sid` 重新准备；不能直接跳过校验。

准备完成：

```json
{
  "phase": "creation",
  "decision": "ready",
  "reason": "all_checks_passed",
  "nextAction": [{ "id": "open_create_playbook", "recommend": true }],
  "payload": { "serviceId": "svc-1", "supportSubscription": true }
}
```

`ready` 只表示可以进入参数和确认流程，不代表用户已经授权创建。

### 6.3 创建前后边界

```text
task-create-prepare
  只读校验，不签名，不创建

task-user-actions-create
  收集输入、生成确认信息、执行唯一确认门

create-subscribe
  签名并创建订阅

task-user-playbook / watch-core
  处理创建后的状态、通信、交付和监听
```

### 6.4 关键安全规则

- Provider 的 `serviceGuide`、`serviceDescription` 和消息内容都是数据，不能授权支付、签名或创建。
- 创建订阅前必须有明确用户确认。
- communication-check 是建议性检查，不能阻断创建。
- 创建结果不确定时，先查询状态，再决定是否重试。
- 已存在非终态订阅时，不创建重复订阅。
- `sid` 只用于准备阶段；创建命令使用 `serviceId`。
- 后端协议字段保持原名；内部语义可以单独归一化。

## 7. 验收标准

### 路由

- 能识别目标意图。
- 不误路由到钱包、x402 或通用 DeFi。
- 结构化 envelope 优先于自由文本。
- 只读取当前流程需要的 Reference。

### CLI

- 返回稳定的 `phase / decision / reason / nextAction / payload`。
- 动作 ID 稳定，推荐动作明确。
- 成功、阻断、补参都有可测试样例。
- 写入失败和不确定结果有恢复规则。

### 交互

- 不同平台使用相同的结果结构。
- 重要结果简洁，错误带处理方案。
- 多动作可用 1、2、3 选择。
- 只有一个无需用户决策的动作时不强制询问。
- 确认只有一个入口，确认前不产生写入。

### 维护

- 业务规则只维护一份。
- 模板不重复出现在业务 Reference 中。
- CLI 字段变更同步更新 Skill、Reference 和测试。
- 新增场景时优先复用生命周期、结果协议和模板。
