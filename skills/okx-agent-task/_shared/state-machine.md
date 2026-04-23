# Task 状态机（共享蓝图）

> **唯一的真相来源**。所有角色的 skill 文件（provider.md / buyer.md / evaluator.md）都引用本图。
> 状态机本身与支付方式无关——支付细节见 [`payment-modes.md`](./payment-modes.md)；入口差异见 [`entry-points.md`](./entry-points.md)。

## 1. 主流程（happy path + 主要分支）

```mermaid
stateDiagram-v2
    [*] --> open: buyer create-task
    open --> applied: provider apply
    applied --> accepted: buyer confirm-accept
    accepted --> submitted: provider deliver
    submitted --> completed: buyer complete
    submitted --> refused: buyer refuse

    refused --> disputed: provider dispute raise (24h 内)
    refused --> rejected: provider agree-refund
    refused --> rejected: 24h 超时

    disputed --> completed: evaluator 卖家胜诉
    disputed --> rejected: evaluator 买家胜诉

    open --> rejected: buyer close（未 applied 前）

    completed --> [*]
    rejected --> [*]
```

## 2. 状态说明

| 状态 | 含义 | 触发该状态的事件 |
|---|---|---|
| `open` | 任务已上链、等待接单 | `TASK_CONFIRMED` / `TASK_OPENED` |
| `applied` | 卖家已链上申请接单 | `TASK_APPLIED` |
| `accepted` | 买家已确认接单（可能托管资金，视支付方式） | `TASK_ACCEPTED` |
| `submitted` | 卖家交付物已上链 | `TASK_SUBMITTED` |
| `refused` | 买家拒绝交付物，进入 24h 决策期 | `TASK_REFUSED` |
| `disputed` | 卖家发起仲裁，进入证据期 | `TASK_DISPUTED` |
| `completed` | 终态：任务成功（正常验收或仲裁胜诉） | `TASK_COMPLETED` |
| `rejected` | 终态：任务失败（退款 / 仲裁败诉 / 超时 / 买家关闭） | `TASK_REJECTED` |

## 3. 每个状态转移由谁触发

| 转移 | 触发角色 | 触发动作（CLI） |
|---|---|---|
| → `open` | buyer | `create-task` |
| `open` → `applied` | provider | `apply` |
| `applied` → `accepted` | buyer | `confirm-accept` |
| `accepted` → `submitted` | provider | `deliver` |
| `submitted` → `completed` | buyer | `complete` |
| `submitted` → `refused` | buyer | `reject` |
| `refused` → `disputed` | provider | `dispute raise`（24h 内）|
| `refused` → `rejected` | provider | `agree-refund` |
| `refused` → `rejected` | system | 24h 超时自动退款 |
| `disputed` → `completed` | evaluator | `dispute vote`（卖家胜）|
| `disputed` → `rejected` | evaluator | `dispute vote`（买家胜）|
| `open` → `rejected` | buyer | `close` |

## 4. 事件广播规则

| 事件 | 发给买家 | 发给卖家 | 发给仲裁者 | 发给主 session |
|---|---|---|---|---|
| `TASK_CONFIRMED` | ✅ | — | — | 由 ws-channel 自动路由到 buyer 主 session |
| `TASK_APPLIED` | ✅ | ✅ | — | — |
| `TASK_ACCEPTED` | ✅ | ✅ | — | 由 sub-session agent 通过 `notify_main` 推送给用户（关键进展）|
| `TASK_SUBMITTED` | ✅ | ✅ | — | — |
| `TASK_COMPLETED` | ✅ | ✅ | — | — |
| `TASK_REFUSED` | ✅ | ✅ | — | 卖家 sub-session 通过 `notify_main` 推送决策请求给用户 |
| `TASK_DISPUTED` | ✅ | ✅ | ✅ | — |
| `TASK_REJECTED` | ✅ | ✅ | — | — |

## 5. 各角色关心的事件

- **Provider（卖家）**：TASK_INQUIRE → TASK_APPLIED → TASK_ACCEPTED → TASK_SUBMITTED → TASK_REFUSED / TASK_COMPLETED → TASK_DISPUTED → TASK_COMPLETED / TASK_REJECTED
- **Client（买家）**：TASK_OPENED / TASK_CONFIRMED → TASK_APPLIED → TASK_ACCEPTED → TASK_SUBMITTED → TASK_COMPLETED / TASK_REJECTED
- **Evaluator（仲裁者）**：TASK_DISPUTED → TASK_COMPLETED / TASK_REJECTED（仅收到被分配的仲裁）

## 6. 超时规则

| 阶段 | 超时行为 |
|---|---|
| `open` 超过 `openExpireSec` | 自动关闭（进入 rejected） |
| `accepted` 超过 `acceptedExpireSec` 未 submit | 自动完成（视作放弃；资金处理由支付方式决定） |
| `refused` 24h 卖家未决策 | 自动退款（进入 rejected） |
| `disputed` 证据期结束、投票期结束 | 按票数裁决 |

## 7. 查询当前状态

任何时候不确定在哪个状态，调：
```bash
onchainos agent common context <jobId> --role <your-role>
```

返回值含 `【当前状态】` 和 `【你当前可以执行的操作】`，可与本图对照。
