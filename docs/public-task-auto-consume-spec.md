# Public Task Auto-Consume Spec (Buyer Side)

> **Source**: [Lark 文档](https://okg-block.sg.larksuite.com/docx/Yopbdg5fCowL8cxk88IluWxxgGg)
> **Date**: 2026-06-26
> **Status**: Reviewed (4 rounds)

---

## 1. Background & Motivation

### 1.1 Current Flow (Before)

Public 任务（visibility=0）发布后，ASP 主动搜索并联系 Buyer。当前流程需要 **人工确认** 每一步：

```
ASP 联系 → provider_conversation 事件 → 弹 decision card 给用户
→ 用户回复"接受" → provider_conversation_pick → 开始协商
→ 协商失败 → 弹 decision card 给用户 → 用户选"下一个" → 手动循环
```

**问题**：
- 每个 ASP 都需要用户手动确认是否接单，延迟高
- 协商失败后也需要用户手动触发下一个 ASP
- 多个 ASP 同时找过来时，用户需要逐个处理
- 与 "Agent 自主行动" 的产品定位不符

### 1.2 Optimized Flow (After)

User Agent 自动消费 ASP 消息队列（FIFO），无需人工介入：

```
ASP 联系 → provider_conversation 事件
→ 自动取队列第一条 → 自动 set-asp → 自动创建 session → 自动协商
→ 协商成功 → confirm-accept → Accepted
→ 协商失败 → 消费该消息 → 自动取下一条 → 循环
→ 队列耗尽 → 静默等待新 ASP
```

**核心原则**：Public 任务的 ASP 匹配和初始协商完全由 User Agent 自主完成，全程不打扰用户。

---

## 2. Detailed Design

### 2.1 Flow Diagram

```
                              Public Task Auto-Consume Flow
                              ==============================

  ┌───────────┐     ┌────────────────┐     ┌────────────────┐     ┌──────────┐
  │    ASP    │     │  Communication │     │  Task System   │     │   User   │
  │           │     │    System      │     │  (Buyer Agent) │     │  Agent   │
  └─────┬─────┘     └───────┬────────┘     └───────┬────────┘     └────┬─────┘
        │                   │                      │                   │
        │  Find Public Task │                      │                   │
        │──────────────────>│                      │                   │
        │                   │                      │                   │
        │                   │  provider_conversation                   │
        │                   │  event (notify)      │                   │
        │                   │─────────────────────>│                   │
        │                   │                      │                   │
        │                   │                      │ ┌──────────────────────┐
        │                   │                      │ │ Check conditions:    │
        │                   │                      │ │ 1. Is task Public?   │
        │                   │                      │ │ 2. Is status Open?   │
        │                   │                      │ │ 3. Has active session│
        │                   │                      │ └──────────┬───────────┘
        │                   │                      │            │
        │                   │                      │   ┌────────▼────────┐
        │                   │                      │   │ Active session? │
        │                   │                      │   └───┬─────────┬──┘
        │                   │                      │   YES │         │ NO
        │                   │                      │   ┌───▼──┐  ┌──▼──────────────┐
        │                   │                      │   │ Skip │  │ Fetch FIFO queue│
        │                   │                      │   │ (end │  │ task_requests() │
        │                   │                      │   │ turn)│  └──┬──────────────┘
        │                   │                      │   └──────┘     │
        │                   │                      │         ┌──────▼──────┐
        │                   │                      │         │ Queue empty? │
        │                   │                      │         └──┬───────┬──┘
        │                   │                      │        YES │       │ NO
        │                   │                      │   ┌────────▼──┐ ┌──▼──────────────┐
        │                   │                      │   │ End turn  │ │ Take first item │
        │                   │                      │   │ (silent   │ │ auto-pick ASP   │
        │                   │                      │   │  wait)    │ └──┬──────────────┘
        │                   │                      │   └───────────┘    │
        │                   │                      │            ┌──────▼───────────┐
        │                   │                      │            │ asp-match → route│
        │                   │                      │            │ (A2A/x402/error) │
        │                   │                      │            └─┬──────┬──────┬──┘
        │                   │                      │          A2A │  x402│ ERROR│
        │                   │                      │              │      │  ┌───▼─────────────┐
        │                   │                      │              │      │  │ task_reject()   │
        │                   │                      │              │      │  │ → retry next    │
        │                   │                      │              │      │  │   from queue    │
        │                   │                      │              │      │  └─────────────────┘
        │                   │                      │     ┌────────▼──┐   │
        │                   │                      │     │ set-asp   │   │
        │                   │                      │     │ create    │   │
        │                   │                      │     │ session   │   │
        │                   │                      │     └────┬──────┘   │
        │                   │                      │          │     ┌────▼─────────────┐
        │                   │                      │   ┌──────▼───┐ │ Auto-infer       │
        │                   │                      │   │Negotiation│ │ serviceParams    │
        │                   │                      │   │(2 rounds) │ │ from task desc   │
        │                   │                      │   └──┬─────┬─┘ └──┬───────────┬───┘
        │                   │                      │ OK   │FAIL │  OK  │    FAIL   │
        │                   │                      │  ┌───▼──┐┌─▼───┐┌─▼────┐ ┌────▼──────┐
        │                   │                      │  │ Wait ││mark ││x402  │ │task_reject│
        │                   │                      │  │ for  ││fail ││flow  │ │→ retry    │
        │                   │                      │  │apply ││retry││(auto)│ │  next     │
        │                   │                      │  └──┬───┘└─────┘└──┬──┘ └───────────┘
        │                   │                      │     │              │
        │                   │                      │ ┌───▼──────────────▼─┐
        │                   │                      │ │  provider_applied  │
        │                   │                      │ │  within budget?    │
        │                   │                      │ └──┬──────────────┬──┘
        │                   │                      │ YES│           NO │
        │                   │                      │ ┌──▼───────┐  ┌──▼────────────┐
        │                   │                      │ │ confirm  │  │ auto-reject   │
        │                   │                      │ │ accept   │  │ task_reject() │
        │                   │                      │ │→Accepted │  │ → retry next  │
        │                   │                      │ └──┬───────┘  │   from queue  │
        │                   │                      │    │          └───────────────┘
        │                   │                      │    │
        │                   │                      │ ┌──▼────────────────┐
        │                   │                      │ │ Drain queue (R14) │
        │                   │                      │ │ okx-a2a task      │
        │                   │                      │ │ reject --job-id   │
        │                   │                      │ └──┬────────────────┘
        │                   │                      │    │                   │
        │                   │                      │    │   user-notify     │
        │                   │                      │    │──────────────────>│
        │                   │                      │    │  "Job accepted"   │
```

### 2.2 State Transitions

```
                    Public Task Negotiation State Machine
                    =====================================

  ┌─────────────────────────────────────────────────────────────────────┐
  │                                                                     │
  │  ┌──────────┐  provider_conversation  ┌───────────────────┐        │
  │  │          │ ────────────────────────>│                   │        │
  │  │  IDLE    │                          │ CHECK_CONDITIONS  │        │
  │  │ (waiting │ <────────────────────────│ (public? open?    │        │
  │  │  for ASP)│  has_active_session=true │  active session?) │        │
  │  └──────────┘                          └─────────┬─────────┘        │
  │       ^                                          │                  │
  │       │                               no active session             │
  │       │                                          │                  │
  │       │                                ┌─────────▼─────────┐        │
  │       │                                │  FETCH_QUEUE      │        │
  │       │ queue_empty                    │  task_requests()   │        │
  │       │ (silent wait)                  └─────────┬─────────┘        │
  │       │                                          │                  │
  │       │                                 ┌────────▼────────┐         │
  │       │◄────────────────────────────────│  PICK_FIRST_ASP │         │
  │       │                       empty     │  (auto, no user)│         │
  │       │                                 └────────┬────────┘         │
  │       │                                          │ has items        │
  │       │                                 ┌────────▼────────┐         │
  │       │                                 │   ROUTE_CHECK   │         │
  │       │                                 │ (asp-match →    │         │
  │       │                                 │  A2A/x402/err)  │         │
  │       │                                 └───┬─────────┬───┘         │
  │       │                              OK     │   error │             │
  │       │                          ┌──────────▼──┐  ┌───▼──────┐     │
  │       │                          │ NEGOTIATING │  │RETRY_NEXT│─────┘
  │       │                          │ (set-asp,   │  │(reject,  │ loop
  │       │                          │  session,   │  │ re-fetch)│ back
  │       │                          │  2 rounds)  │  └──────────┘
  │       │                          └───┬─────┬───┘
  │       │                    success   │     │ fail (2-round / timeout)
  │       │                   ┌──────────▼┐  ┌─▼───────────┐
  │       │                   │WAIT_APPLY │  │ RETRY_NEXT  │────────────┘
  │       │                   │(provider  │  │(mark-failed,│  loop back
  │       │                   │ applied?) │  │ reject,     │
  │       │                   └─────┬─────┘  │ re-fetch)   │
  │       │                         │        └─────────────┘
  │       │              ┌──────────▼──────────┐
  │       │              │ BUDGET_CHECK        │
  │       │              │ within maxBudget?   │
  │       │              └────┬──────────┬─────┘
  │       │           YES     │     NO   │
  │       │         ┌─────────▼──┐  ┌────▼───────┐
  │       │         │ CONFIRM    │  │ RETRY_NEXT │─────────────────────┘
  │       │         │ ACCEPT     │  │(auto-reject│  loop back
  │       │         │ → Accepted │  │ re-fetch)  │
  │       │         └────────────┘  └────────────┘
  │       │
  └───────┘
```

### 2.3 Key Rules

| # | Rule | Detail |
|---|------|--------|
| R1 | **FIFO Queue** | ASP 消息按先进先出顺序处理 |
| R2 | **Strict Serial** | 同一任务同一时间只能与一个 ASP 协商；有活跃 session 时跳过新的 provider_conversation 事件 |
| R3 | **Auto-Pick** | 自动取队列第一条，不弹 decision card 给用户 |
| R4 | **Auto-Reject on Fail** | 协商失败（2轮超限/超时/ASP 离线/route error/ASP 主动拒绝）→ 自动 task_reject → 取下一个 |
| R5 | **Auto-Reject on Over-Budget** | provider_applied 报价超 maxBudget → 自动 reject-apply → 取下一个 |
| R6 | **Message Consumed** | 不管协商成功与否，该消息从队列中消费（成功时 accept 即消费；失败时 task_reject 消费） |
| R7 | **Silent Exhaustion** | 队列耗尽后静默等待，不通知用户，不弹选项卡片；等新 ASP 进来再自动消费 |
| R8 | **No User Notify During Match** | 整个 auto-consume 过程中不发 user-notify / pending-decisions-v2 request |
| R9 | **Condition Guard** | 每次取队列前检查：task 是 Public + 状态 Open + 无活跃 session |
| R10 | **Scope Boundary** | 只影响 Public 任务 (visibility=0)；Private 任务 (visibility=1) 保持现有指定 provider 流程不变。判定标准仅用 `visibility == 0`，不考虑 `service_id.is_none()` |
| R11 | **x402 Auto-Infer** | x402 route 也自动走：serviceParams 从 task description 推断，全部推断出 → 自动执行；存在无法推断的必填字段 → 跳过该 ASP，try next |
| R12 | **Timeout = Fail** | 协商超时（5分钟无回复）与 2-round 超限同等处理：mark-failed + auto-advance to next |
| R13 | **Audit Log** | 每次 auto-pick / reject / skip / route-error 均记录 audit log，便于排查 |
| R14 | **Accept 后清理队列** | confirm-accept 成功后，调用 `okx-a2a task reject --job-id <jobId>` 批量清空该 job 的所有剩余接单消息 |
| R15 | **Failed List Guard** | auto-pick 时检查 `get_failed_list(job_id)`；若 ASP 已在 failed list 中 → 直接 `task_reject` 跳过，取下一条 |
| R16 | **is_public 判定标准** | 仅用 `visibility == 0` 判定 public，不使用 `service_id.is_none()` 回退逻辑。代码中 `events.rs` 的 `is_public = visibility == Some(0) \|\| service_id.is_none()` 仅用于 negotiate_reply 的价格协商规则，auto-consume 路径统一用 `visibility == 0` |
| R17 | **x402 支付失败区分** | x402 支付执行失败时按错误类型分流：buyer 余额不足 → 通知用户（所有 ASP 都会遇到同样问题，advance 无意义）；ASP 端点错误/链上错误等 → mark-failed + auto-advance |
| R18 | **confirm-accept 失败区分** | confirm-accept 失败时按错误类型分流：buyer 余额不足 → 保持 cli_failed 行为，通知用户；其他错误（网络/链上临时错误等）→ mark-failed + auto-advance 到下一个 ASP |
| R19 | **Loop Safety** | auto-consume 循环设 `MAX_AUTO_CONSUME_ATTEMPTS = 20` 硬上限；超过上限 → 静默等待 + audit log 告警 |
| R20 | **LLM-Driven Step Fail = Auto-Advance** | A2A 路径中 LLM 执行的步骤（asp-match 无 services / asp-match API 错误 / set-asp 失败）对 public task 一律 auto-advance，不通知用户 |
| R21 | **A2A serviceParams Auto-Infer** | A2A 路径的 serviceParams 也从 task description 自动推断（与 x402 R11 相同规则）；推断出 → 传入 set-asp；推断不出 → 传空字符串，由 2 轮协商补充。不推 decision card 问用户 |

---

## 3. Impact Analysis

### 3.1 File-Level Impact Map

```
cli/src/commands/agent_commerce/task/buyer/
├── flow_negotiate/
│   ├── match_provider.rs        ★★★ [MAJOR] auto-consume 核心逻辑
│   ├── events.rs                ★★  [MEDIUM] negotiate_reply 失败后自动续接 + provider_reject 自动续接
│   └── designated.rs            ─   [NO CHANGE]
├── flow_lifecycle/
│   ├── core.rs                  ★★  [MEDIUM] provider_applied over-budget 自动拒绝
│   ├── terminal.rs              ─   [NO CHANGE]
│   ├── manage.rs                ─   [NO CHANGE]
│   └── dispute.rs               ─   [NO CHANGE]
├── flow.rs                      ★★  [MEDIUM] 事件路由调整
├── content.rs                   ★   [MINOR] 可能移除 public task 的 decision card 模板
├── changepublic.rs              ─   [NO CHANGE]
├── create.rs                    ─   [NO CHANGE]
├── draft.rs                     ─   [NO CHANGE]
├── negotiate.rs                 ─   [NO CHANGE]
└── accept.rs / reject.rs / ...  ─   [NO CHANGE]

skills/okx-agent-task/
├── SKILL.md                     ★   [MINOR] 更新 public task 事件说明
├── buyer-sub-playbook.md        ★   [MINOR] 更新 negotiate_reply 超限行为
└── buyer-user.md                ★   [MINOR] 更新 provider_conversation 处理说明
```

### 3.2 Detailed Change Points

#### 3.2.1 `match_provider.rs` — Auto-Consume Core [MAJOR]

**Current `provider_conversation()`** (L210-335):
```
1. 检查是否有重复 pending decision
2. 调用 task_requests() 获取 ASP 列表
3. 取第一个 ASP 构造 accept/reject decision card
4. 推送给用户 (pending-decisions-v2 request)
5. 等用户回复
```

**New behavior**:
```
1. 检查是否有活跃 session (R2: strict serial)
   → 有 → 跳过，结束 turn（消息留在队列）
2. 调用 task_requests() 获取 FIFO 队列
3. 队列为空 → 结束 turn（静默等待 R7）
4. 取第一个 ASP (R3: auto-pick, R1: FIFO)
5. 调用 designated_route_inner 判断路由 (A2A/x402/error)
   → error (ASP 离线/不是 provider) → task_reject → 回到步骤 2 (R4)
6. A2A 路由:
   a. asp-match 获取 service info
      → 无 services → task_reject → 回到步骤 2 (R4, R20)
      → API 错误 → task_reject → 回到步骤 2 (R20)
   b. 自动推断 serviceParams（从 task description 推断，不问用户，R21）
   c. set-asp
      → 失败 → task_reject → 回到步骤 2 (R20)
   d. 创建 sub session + SKILL_PREFETCH
   e. 上传 pending attachments
   f. 等 provider_applied
7. x402 路由:
   a. x402-check → set-payment-mode → task-402-pay
   b. 同 designated x402 分支
```

**Specific changes**:
- Remove `pending-decisions-v2 request` for accept/reject card
- Remove `provider_pending` source_event handling
- Replace with in-process auto-pick → auto-route logic
- Add active session check at the top
- Add loop: on route error → reject + re-fetch + retry next
- x402 route: add serviceParams auto-infer check — 推断成功走 x402 flow，推断失败 reject + retry next
- A2A route: `provider_conversation_pick_a2a()` prompt 需按 public/private 分叉 (R20, R21):
  - asp-match 无 services / API 错误: public → auto-advance（不通知用户）；private → 通知用户
  - serviceParams 收集: public → 自动推断，不推 decision card (R21)；private → 保持现有推卡片流程
  - set-asp 失败: public → auto-advance；private → 推 cli_failed 卡片

**Current `provider_conversation_reject_cli()`** (L340-360):
- 目前由用户触发
- 新逻辑中变成内部自动调用（协商失败时自动 reject + advance）

**New `provider_conversation_pick_cli()`** (L111-204):
- 基本保持不变，但调用方从用户手动 pick 变为自动 pick
- 无需修改函数本身

#### 3.2.2 `events.rs` — Negotiate Reply Over-Limit [MEDIUM]

**Current `negotiate_reply()` over-limit path** (L213-229):
```
Rounds sent ≥ 2 → mark-failed → push decision card (no_asp_found)
→ 等用户选 A/B/C
```

**New behavior (public task only)**:
```
Rounds sent ≥ 2 → mark-failed → task_reject(groupId) → 
→ re-fetch task_requests() → 
→ 有下一个 ASP → 自动 pick, 重新开始协商
→ 无更多 ASP → 结束 turn (静默等待 R7)
```

**Specific changes**:
- 在 `negotiate_reply()` 中判断 `is_public`
- Public task: over-limit → 不弹卡片，改为生成 auto-advance prompt
- Private task: 保持现有行为不变

#### 3.2.3 `flow_lifecycle/core.rs` — Provider Applied Over-Budget [MEDIUM]

**Current `provider_applied()` over-budget path** (L117-162):
```
over_most_budget=true → reject-apply → push decision card (apply_over_budget)
→ 等用户选 A/B/C/D
```

**New behavior (public task only)**:
```
over_most_budget=true → reject-apply →
→ re-fetch task_requests() →
→ 有下一个 ASP → 自动 pick, 重新匹配
→ 无更多 ASP → 结束 turn (静默等待 R7)
```

**Specific changes**:
- 在 `provider_applied()` 中判断 `visibility == 0` (public)
- Public task + over_budget → 不弹卡片，改为自动 reject + advance
- Public task + within_budget + confirm-accept 成功 → 追加 `okx-a2a task reject --job-id <jobId>` 清空剩余队列 (R14)
- Public task + within_budget + confirm-accept 失败 → 按错误类型分流 (R18)：
  - Buyer 余额不足 → 保持 `cli_failed` 行为，通知用户（advance 无意义，所有 ASP 都会遇到同样问题）
  - 其他错误（网络/链上临时错误等）→ mark-failed + auto-advance 到下一个 ASP
- Private task: 保持现有行为不变

#### 3.2.4 `flow.rs` — Event Routing [MEDIUM]

**Current event routing**:
```rust
Event::Other("provider_conversation")     → show decision card
Event::Other("provider_conversation_pick") → user triggered pick
Event::Other("provider_conversation_reject") → user triggered reject
Event::ProviderApplied                    → budget check + decision card
```

**New routing additions**:
- `provider_conversation` 对 public task 走 auto-consume 路径
- 可能需要新伪事件 `auto_advance_next`：协商失败后自动推进到下一个 ASP
- `provider_conversation_pick` 和 `provider_conversation_reject` 对 public task 仍需保留（作为 auto-consume 的内部机制）

**Specific changes**:
- `provider_conversation` handler 需要根据 visibility 分叉
- 新增伪事件处理（如果用 event 驱动自动续接）
- 或在 match_provider.rs 中直接循环处理（不走 event 驱动）

#### 3.2.5 `content.rs` — Templates [MINOR]

**可能被废弃的模板** (public task 路径不再需要):
- `provider_pending_single_user_card()` — accept/reject card（public 不再展示）
- `pending_list_empty_user_notify()` — 空列表通知（public 改为静默）
- `no_more_sellers_user_notify()` — 无更多 ASP 的 A/B/C 选项卡片（public 改为静默）

**注意**: 这些模板在 private task 路径中可能仍然使用，不能直接删除，只是 public task 路径不再调用。

#### 3.2.6 `events.rs` — Provider Reject (ASP 主动拒绝) [MEDIUM]

**Current `provider_reject()` (L242-293)**:
```
ASP 调用 asp/reject API → JobProviderReject 事件 → reset/asp (in-process)
→ push decision card (A/B/C/D) 给用户 → 等用户选
```

**New behavior (public task only)**:
```
ASP 调用 asp/reject → JobProviderReject 事件 → reset/asp (in-process)
→ 不弹卡片 → 生成 auto_advance_next prompt
→ re-fetch task_requests() → 有下一个 → auto-pick
→ 无更多 ASP → 静默等待 (R7)
```

**Specific changes**:
- 在 `provider_reject()` 中判断 `visibility == 0` (public)
- Public task: reset/asp 完成后，不推 decision card，改为触发 `auto_advance_next`
- Private task: 保持现有 A/B/C/D 决策卡片行为不变

#### 3.2.7 `flow.rs` — User Decision Relay [MINOR]

**当前 `user_decision_*` handlers 受影响的**:
- `user_decision_provider_pending` — public task 不再需要（没有 provider_pending 卡片了）
- `user_decision_apply_over_budget` — public task 不再需要（auto-reject）
- `user_decision_no_asp_found` — public task 不再需要（静默等待）
- `user_decision_negotiate_over_budget` — public task 不再需要（auto-advance）
- `user_decision_job_provider_reject` — public task 不再需要（auto-advance）

这些 handler 在 private task 路径中仍然需要，不能删除。

### 3.3 Skill Files Impact

#### `SKILL.md`
- §Activation 中 `provider_conversation` 事件说明需更新
- 添加 public task auto-consume 行为描述

#### `buyer-sub-playbook.md`
- negotiate_reply 超限行为更新：public task 不推卡片，自动推进
- 可能需要新增 "auto-advance" 行为说明

#### `buyer-user.md`
- provider_conversation 处理逻辑更新
- 移除 public task 的手动 pick/reject 说明
- 添加 auto-consume 行为说明

---

## 4. Edge Cases & Boundary Conditions

### 4.1 Active Session Guard (R2)

| Scenario | Current | New |
|----------|---------|-----|
| ASP #2 arrives while negotiating with ASP #1 | 弹 decision card，两个卡片并行 | 跳过 ASP #2 的事件，消息留在队列 |
| ASP #1 协商失败后 | 用户手动选"下一个" | 自动 re-fetch queue，ASP #2 会被取到 |
| ASP #1 accept 成功后 | 任务进入 Accepted | ASP #2 消息留在队列，但任务已不是 Open，condition guard 会跳过 |

**Implementation**: 检查 session 是否存在：
```rust
// 方案: 检查 designated-provider.json 是否存在
// 如果存在 = 有 ASP 在处理 = 有活跃 session
let has_active = negotiate::get_designated_provider(job_id).ok().flatten().is_some();
```

### 4.2 Visibility Toggle During Auto-Consume

| Scenario | Behavior |
|----------|----------|
| 用户在 auto-consume 过程中 set-private | 下次 condition check 发现不是 public → 停止 auto-consume |
| Private 任务被 set-public | 下次 provider_conversation 事件触发 auto-consume |

### 4.3 Route Error (ASP offline / not provider)

当 `designated_route_inner` 返回 `error` 路由时：
- **Current**: 弹 A/B/C 卡片让用户选
- **New**: 自动 task_reject 该 ASP → re-fetch queue → 尝试下一个
- 如果 ASP 只是暂时离线，消息被消费后就没了

### 4.4 x402 Route in Auto-Consume

Public task + ASP 提供的是 x402 服务：
- x402 也自动走，serviceParams 从 task description 自动推断
- 推断不出来（serviceDescription 含必填字段但 task description 中无对应信息）→ 跳过该 ASP，task_reject → 尝试下一个
- 推断成功 → 正常走 x402 flow（set-payment-mode → task-402-pay → accept）

**x402 auto-consume 判定逻辑**:
```
1. asp-match → 获取 serviceDescription
2. 从 serviceDescription + task description 推断 serviceParams
3. 推断结果:
   a. 全部字段可推断 → set-asp + x402 flow（自动）
   b. serviceDescription 为空（无需 params）→ set-asp + x402 flow（自动）
   c. 存在 <to be provided> 字段 → 跳过该 ASP → task_reject → next
```

**与 A2A 的区别**: A2A 可以通过 2 轮协商来补充信息，x402 无协商环节——要么一次推断成功，要么跳过。

### 4.5 x402 Payment Failure (R17)

x402 serviceParams 推断成功后，支付执行本身也可能失败：

| 错误类型 | 行为 |
|----------|------|
| Buyer 余额不足 | 通知用户（所有 ASP 都会遇到同样问题，advance 无意义）。不 auto-advance，等用户充值或操作 |
| ASP 端点错误（502/503/timeout/invalid response） | mark-failed + auto-advance 到下一个 ASP |
| 链上错误（gas estimation failed/revert） | mark-failed + auto-advance |

**判断依据**: `task-402-pay` 的返回中可区分错误来源——余额不足通常来自签名/escrow 阶段的 insufficient funds 错误；端点错误来自 replay 阶段的 HTTP status。

### 4.6 Provider Reject (ASP 主动拒绝)

ASP 通过 `asp/reject` API 主动拒绝接单：
- **Current**: `provider_reject()` 调用 `reset/asp` → 弹 A/B/C/D 决策卡片
- **New (public)**: `reset/asp` → 不弹卡片 → 触发 `auto_advance_next` → re-fetch queue → 自动取下一个
- ASP 的拒绝消息不转达给用户（R8: 全程不打扰用户）

### 4.7 Failed List Guard (R15)

Auto-pick 时检查 failed list，防止重复尝试已失败的 ASP：

```
ASP #1 negotiate 失败 → mark-failed(ASP #1) → task_reject → 消息移出队列
... 稍后 ...
ASP #1 再次发消息 → 新消息进入队列 → auto-pick 取到 ASP #1
→ 检查 failed list → ASP #1 在列表中 → 直接 task_reject → 取下一条
```

### 4.8 confirm-accept Failure (R18)

`provider_applied()` within-budget 分支调用 `handle_confirm_accept()` 失败时：

| 错误类型 | 行为 |
|----------|------|
| Buyer 余额不足（escrow fund 失败） | 保持 `cli_failed` 行为，通知用户。advance 无意义 |
| 其他错误（网络超时、链上 gas 不足、RPC 错误等） | mark-failed + auto-advance 到下一个 ASP |

**判断依据**: `handle_confirm_accept()` 的错误信息中包含 `insufficient` / `balance` 等关键词 → 余额不足；其他 → 可重试/可 advance。

### 4.9 Concurrent provider_conversation Events

多个 ASP 短时间内同时联系：
```
t=0s: provider_conversation (ASP #1) → auto-pick ASP #1, 开始协商
t=1s: provider_conversation (ASP #2) → active session → skip
t=2s: provider_conversation (ASP #3) → active session → skip
t=30s: ASP #1 协商失败 → task_reject → re-fetch → 取 ASP #2 → 开始协商
```

ASP #2 和 #3 的消息留在 FIFO queue 中，按序消费。

### 4.10 Task Status Guard

Auto-consume 只在以下条件下进行：
- `visibility == 0` (Public)
- `status == Created` (Open)
- 无活跃 negotiation session

如果任务已经进入 `Accepted` / `Completed` / `Failed` 等终态，新到的 provider_conversation 事件自动跳过。

---

## 5. Implementation Approach

### 5.1 Option A: In-Process Loop (Recommended)

在 `match_provider.rs::provider_conversation()` 中直接实现循环：

```rust
const MAX_AUTO_CONSUME_ATTEMPTS: usize = 20; // R19: loop safety

pub(crate) async fn provider_conversation_auto_consume(ctx: &FlowContext<'_>) -> String {
    // Guard: active session? (R2: strict serial)
    let has_active = negotiate::get_designated_provider(ctx.job_id).ok().flatten().is_some();
    if has_active {
        return "[provider_conversation] Active session exists; skip (R2).\n".to_string();
    }

    let failed_list = negotiate::get_failed_list(ctx.job_id); // R15: failed list guard

    for attempt in 0..MAX_AUTO_CONSUME_ATTEMPTS {
        // Fetch FIFO queue
        let items = okx_a2a::task_requests()...;
        if items.is_empty() {
            return "[provider_conversation] Queue empty; silent wait (R7).\n".to_string();
        }

        // Auto-pick first item (R1: FIFO, R3: auto-pick)
        let first = &items[0];
        let asp_agent_id = extract_agent_id(first);
        let group_id = extract_group_id(first);

        // R15: skip if ASP already in failed list
        if failed_list.contains(&asp_agent_id) {
            audit::log("auto_consume", "skip_failed_asp", ...);
            okx_a2a::task_reject(&group_id);
            continue; // try next
        }

        // Route check
        let route = designated_route_inner(&asp_agent_id, None).await;
        match route {
            "a2a" => {
                // asp-match → set-asp → create session → SKILL_PREFETCH
                return provider_conversation_pick_a2a(job_id, agent_id, short_id, &asp_agent_id);
            }
            "x402" => {
                let infer_ok = can_auto_infer_params(service_description, task_description);
                if infer_ok {
                    return branch_x402_auto(job_id, agent_id, short_id, &asp_agent_id, &route_json);
                } else {
                    // 无法推断 → skip this ASP
                    okx_a2a::task_reject(&group_id);
                    continue; // try next
                }
            }
            "error" => {
                // ASP offline / not provider → reject, try next (R4)
                okx_a2a::task_reject(&group_id);
                continue;
            }
        }
    }
    // R19: max attempts reached
    audit::log("auto_consume", "max_attempts_reached", ...);
    return "[provider_conversation] Max auto-consume attempts reached; silent wait.\n".to_string();
}
```

**Pros**: 逻辑集中，迭代 loop 无 stack overflow 风险，有安全上限
**Cons**: 无

### 5.2 Option B: Event-Driven Chain

通过新伪事件 `auto_advance_next` 驱动：

```
provider_conversation → auto-pick → 协商
negotiate_reply (fail) → 生成 prompt 调用 next-action --event auto_advance_next
auto_advance_next → re-fetch queue → auto-pick next → 协商
```

**Pros**: 符合现有 event-driven 架构
**Cons**: 每次推进需要一个 LLM round-trip

### 5.3 Recommendation

**Option A** for the initial route check loop (reject + retry on error is fast).
**Option B** for negotiate failure → advance（因为协商结束是从 sub-session 返回的 event，天然是 event-driven）.

Hybrid approach:
1. `provider_conversation` 内部 loop: route error → reject → next（in-process，毫秒级）
2. `provider_conversation` 内部 loop: x402 route + serviceParams 推断失败 → reject → next（in-process）
3. `provider_conversation` 内部 loop: failed list 命中 → reject → next（in-process，R15）
4. negotiate_reply over-limit/timeout: mark-failed → 返回 prompt 指示 auto-advance（event-driven，跨 session）
5. provider_applied over-budget: reject-apply → in-process auto-advance（user-session 内）
6. provider_reject (ASP 主动拒绝): reset/asp → in-process auto-advance（user-session 内）
7. x402 支付失败 (ASP 端点错误): mark-failed → in-process auto-advance（R17）
8. confirm-accept 失败 (非余额不足): mark-failed → in-process auto-advance（R18）
9. x402 支付失败 (buyer 余额不足): 通知用户，不 advance（R17）
10. confirm-accept 失败 (buyer 余额不足): cli_failed 通知用户，不 advance（R18）
11. x402 route + serviceParams 推断成功: 正常走 x402 flow（set-payment-mode → task-402-pay → accept），与 designated x402 分支相同

---

## 6. Auto-Advance Prompt Design

协商失败后，auto-advance 的 prompt 需要在 **同一 turn** 内完成以下步骤：

```
1. task_reject(groupId)          — 消费当前消息
2. task_requests()               — 重新获取队列
3. 队列为空 → 结束 turn
4. 队列非空 → 取第一个 ASP → 走 provider_conversation_pick_cli 逻辑
```

这个 prompt 由 `negotiate_reply` / `provider_applied` 在判断 `is_public` 后生成。

---

## 7. NOT Changed (Explicitly Out of Scope)

| Area | Reason |
|------|--------|
| Private task (visibility=1) 全流程 | 保持现有 designated provider 流程不变 |
| Task creation / publish flow | 不受影响，visibility 设置在发布时确定 |
| Post-accept lifecycle (submitted/rejected/disputed/completed) | Accept 之后的流程与 provider 匹配方式无关 |
| Provider side flow | Provider 侧不受 buyer 侧 auto-consume 影响 |
| x402 payment flow (after route determined) | 支付流程本身不变，只是触发方式从手动变自动 |
| Evaluator / dispute flow | 不受影响 |
| User actions (close/set-public/attachment) | 用户主动操作不受影响 |

---

## 8. Testing Plan

### 8.1 Happy Path

| # | Case | Expected |
|---|------|----------|
| T1 | Public task, 1 ASP contacts, A2A, within budget | auto-pick → negotiate → accept → Accepted |
| T2 | Public task, 3 ASPs contact sequentially | auto-pick #1, negotiate, success → Accepted; #2/#3 ignored |
| T3 | Public task, ASP #1 negotiate fail, ASP #2 success | #1 fail → auto-reject → auto-pick #2 → success |

### 8.2 Error Paths

| # | Case | Expected |
|---|------|----------|
| T4 | ASP route error (offline) | auto-reject → try next |
| T5 | ASP route error (not provider) | auto-reject → try next |
| T6 | Negotiate 2-round limit exceeded | mark-failed → auto-reject → try next |
| T7 | provider_applied over-budget | auto-reject-apply → try next |
| T8 | All ASPs fail | queue empty → silent wait |
| T9 | Active session exists when new ASP arrives | skip event, end turn |

### 8.3 Edge Cases

| # | Case | Expected |
|---|------|----------|
| T10 | Task set-private during auto-consume | next condition check → stop auto-consume |
| T11 | Task already Accepted when new ASP arrives | condition guard → skip |
| T12 | Multiple ASPs arrive simultaneously | serial: pick first, skip rest, re-fetch after current done |
| T13 | x402 ASP, serviceParams 可推断 | auto-infer params → x402 flow → Accepted |
| T14 | x402 ASP, serviceParams 不可推断 | skip ASP → task_reject → try next |
| T15 | Accept 成功后队列仍有同 job 消息 | confirm-accept 后 `okx-a2a task reject --job-id` 批量清空 |
| T16 | ASP 主动拒绝 (provider_reject) | reset/asp → auto-advance → try next |
| T17 | 已在 failed list 的 ASP 再次发消息 | auto-pick → failed list check → 直接 task_reject → skip |
| T18 | x402 支付失败：buyer 余额不足 | 通知用户，不 auto-advance |
| T19 | x402 支付失败：ASP 端点错误 | mark-failed + auto-advance |
| T20 | confirm-accept 失败：buyer 余额不足 | cli_failed 通知用户，不 auto-advance |
| T21 | confirm-accept 失败：网络/链上临时错误 | mark-failed + auto-advance |
| T22 | Auto-consume 达到 20 次上限 | 静默等待 + audit log 告警 |
| T23 | A2A ASP, asp-match 返回无 services | task_reject → auto-advance (R20) |
| T24 | A2A ASP, set-asp API 失败 | task_reject → auto-advance (R20) |
| T25 | A2A ASP, serviceDescription 非空 | 自动推断 serviceParams，不弹卡片 (R21) |
| T26 | Private task, negotiate 2-round 超限 | 弹 decision card (A/B/C)，不 auto-advance |
| T27 | Private task, provider_applied over-budget | 弹 decision card (A/B/C/D)，不 auto-advance |
| T28 | Private task, provider_reject | 弹 decision card (A/B/C/D)，不 auto-advance |

---

## 9. Open Questions

| # | Question | Status |
|---|----------|--------|
| Q1 | x402 route 的 ASP 在 auto-consume 中如何处理？ | **Resolved**: 自动走，serviceParams 从 task description 推断；推断不出来则跳过试下一个 |
| Q2 | Auto-consume 过程中是否需要 audit log 记录每次尝试？ | **Resolved**: yes，每次 auto-pick / reject / skip 均记录 audit log |
| Q3 | 协商超时（5分钟无回复）的处理是否也要改为自动推进？ | **Resolved**: yes，超时与 2-round 超限同等处理：mark-failed + auto-advance |
| Q4 | 是否需要限制 auto-consume 的最大尝试次数？ | **Resolved**: 设 MAX_AUTO_CONSUME_ATTEMPTS=20 硬上限 (R19)，防止 API 异常导致无限循环 |

---

## 10. Review — 补充遗漏点

> 以下是对 §1-§9 的系统审查中发现的遗漏，需要补充到设计中。

### 10.1 跨 session 的 auto-advance 机制 [CRITICAL]

**问题**: 协商失败发生在 **sub-session**（buyer 与 ASP 的一对一 session），但 auto-advance（re-fetch queue → pick next）需要在 **user-session** 层执行。当前文档没有说清跨 session 信号如何传递。

**现有机制**: sub-session 通过 `pending-decisions-v2 request --source-event no_asp_found` 向 user-session 推卡片，user-session 收到 `user_decision_no_asp_found` relay 后等用户操作。

**新机制设计**:

| 触发场景 | 发生位置 | auto-advance 信号方式 |
|----------|----------|----------------------|
| negotiate_reply 2-round 超限 | sub-session | `mark-failed` → 不推 decision card，改为调用 `onchainos agent next-action --role buyer --agentId <id> --message '{"event":"auto_advance_next","jobId":"<id>","failedProvider":"<aspAgentId>","reason":"negotiate_over_limit"}'` |
| negotiate_reply 5-min 超时 | sub-session | 同上，reason=`negotiate_timeout` |
| provider_applied over-budget | user-session (flow_lifecycle) | 直接在 `provider_applied()` 内 in-process 处理，无需跨 session |
| provider_reject (ASP 主动拒绝) | user-session (flow_negotiate) | reset/asp 后直接 in-process 触发 auto-advance，无需跨 session |
| route error (ASP offline/not provider) | user-session (match_provider) | 直接在 `provider_conversation()` 内 in-process loop，无需跨 session |
| x402 serviceParams 推断失败 | user-session (match_provider) | 同上 |
| A2A asp-match 无 services / API 错误 | user-session (match_provider) | 直接在 auto-consume loop 内 reject + next（in-process，R20） |
| A2A set-asp 失败 | user-session (match_provider) | 同上 |
| x402 支付失败 (ASP 端点错误) | user-session | mark-failed + in-process auto-advance |
| confirm-accept 失败 (非余额不足) | user-session | mark-failed + in-process auto-advance |

**需要新增**: `auto_advance_next` 伪事件

- 在 `flow.rs` 的 event routing 中新增 `Event::Other("auto_advance_next")` 分支
- handler 逻辑: re-fetch `task_requests()` → 队列非空 → auto-pick first → 走 `provider_conversation_pick_cli` / `branch_x402` 逻辑 → 队列为空 → 静默结束

**event payload 格式**:
```json
{
  "event": "auto_advance_next",
  "jobId": "<jobId>",
  "failedProvider": "<failed ASP agentId>",
  "reason": "negotiate_over_limit | negotiate_timeout | over_budget | provider_reject | x402_endpoint_error | confirm_accept_error"
}
```
`failedProvider` 和 `reason` 用于 audit log (R13)。

### 10.2 Sub-session 清理 [CRITICAL]

**问题**: Auto-advance 到下一个 ASP 时，与前一个 ASP 的 sub-session 需要清理。当前 `session_cleanup` 仅在终态（completed/failed/close）调用。

**补充设计**:

```
auto-advance 触发时:
1. mark-failed (已有逻辑，会清除 designated-provider.json)
2. 关闭当前 sub-session:
   - okx-a2a session delete --job-id <jobId> --to-agent-id <old ASP agentId>
   - 或 okx-a2a session close（如果需要保留历史）
3. 清除 negotiate-state.json 中该 ASP 的状态
4. 开始下一个 ASP 的流程
```

**注意**: `mark_failed()` (negotiate.rs:243-252) 已经会自动清除 `designated-provider.json`（如果 match），这部分不需要额外处理。但 sub-session 需要显式关闭。

### 10.3 `set-public` 后的 designated-provider.json 清理

**问题**: `changepublic.rs` 的 `handle_set_public()` 调用 `reset/asp` + `setVisibility`，但 **没有** 删除本地的 `designated-provider.json`。

**影响**: auto-consume 的 active session guard 检查 `get_designated_provider(job_id)` — 如果旧的文件还在，guard 会误判为 "有活跃 session"，导致 auto-consume 被跳过。

**修复**: 在 `handle_set_public()` 末尾追加 `clear_designated_provider(job_id)`。

### 10.4 用户主动操作的优先级

**问题**: 文档 §7 提到 "User actions (close/set-public/attachment) 不受影响"，但没有说明 **冲突场景**。

**补充**:

| 用户操作 | auto-consume 中的行为 |
|----------|----------------------|
| `close` | 任务进入 Close 终态 → condition guard 终止 auto-consume → 清理所有 session |
| `set-private` | visibility 变为 1 → 下次 condition check 终止 auto-consume |
| 手动 `designate-provider` | 写入 designated-provider.json → active session guard 视为已有 session → auto-consume 暂停；手动指定的 provider 走 designated flow |
| 添加 attachment | 不冲突，attachment 会在下次 set-asp 时自动上传 |

**规则**: 用户主动操作始终优先于 auto-consume。auto-consume 在每次 condition check 时都会重新评估状态。

### 10.5 `okx-a2a task reject --job-id` 需要新增 Rust wrapper

**问题**: 当前 `okx_a2a.rs` 中的 `task_reject(group_id: &str)` 只支持 `--group-id` 参数。R14 需要的按 `--job-id` 批量清空是另一个调用形式。

**补充**: 需要在 `okx_a2a.rs` 中新增：
```rust
pub fn task_reject_by_job(job_id: &str) -> Result<()> {
    // okx-a2a task reject --job-id <jobId> --json
}
```

### 10.6 `job_created` 后是否立即尝试消费队列

**问题**: Public 任务的 `job_created_non_designated_provider()` 当前只通知用户 "task on-chain, waiting for ASPs"，然后结束 turn。如果 ASP 在 `job_created` 事件到达前就已经发送了消息（队列里已有消息），当前设计依赖下一次 `provider_conversation` 事件才会触发 auto-consume。

**决定**: **不主动 poll，等事件**。`job_created` 后不调用 `task_requests()`，依赖 daemon 推送 `provider_conversation` 事件触发 auto-consume。

**理由**: 逻辑更简单，避免引入主动轮询的复杂度。daemon 推送是标准路径，首次匹配延迟在可接受范围内。

> **✅ Resolved**: 不主动 poll，等 daemon 推送事件

### 10.7 协商超时检测机制

**问题**: R12 规定 5 分钟超时要 auto-advance。但当前 5 分钟超时是 **LLM prompt 层面** 的指导（"⏱ 5-minute timeout: if the ASP does not reply within 5 minutes, treat as over-limit"），不是 CLI/Rust 层面的定时器。

**决定**: **保持现状**，超时检测在 LLM prompt 中说明。不在 CLI/Rust 层增加定时器机制。

**理由**: CLI 中没有现成的 per-task 定时器基础设施（`wakeup_notify` 是系统级重启广播，不是可编程定时器）。新建定时器机制复杂度高、收益有限。当前 LLM prompt 层面的超时指导在 sub-session 仍活跃时可工作；若 ASP 回复则 `negotiate_reply` 自然触发；若 ASP 完全不回复，等下一个 `provider_conversation` 事件到来时可被动检测。

> **✅ Resolved**: 保持 LLM prompt 层面超时指导，不增加 CLI 层定时器

---

## 11. Review Round 2 — 补充遗漏点

> 以下是对 §1-§10 的第二轮审查中发现的遗漏，已内联补充到对应章节。此处汇总索引。

### 11.1 `provider_reject` 事件遗漏 → 已补充

- R4 增加 "ASP 主动拒绝" 作为失败原因
- §3.2.6 新增 `provider_reject` 变更说明
- §4.6 新增边界条件
- §10.1 auto_advance_next 表格增加 provider_reject 行
- T16 新增测试用例

### 11.2 Auto-pick 未检查 failed list → 已补充

- R15 新增规则：auto-pick 时检查 failed list
- §4.7 新增边界条件和示例
- §5.1 伪代码增加 failed_list 检查
- T17 新增测试用例

### 11.3 `is_public` 判定逻辑不一致 → 已补充

- R16 新增规则：仅用 `visibility == 0`，不使用 `service_id.is_none()` 回退

> **✅ Resolved**: auto-consume 路径统一用 `visibility == 0`

### 11.4 x402 支付失败后的处理 → 已补充

- R17 新增规则：按错误类型分流（buyer 余额不足 → 通知用户；ASP 端点错误 → auto-advance）
- §4.5 新增边界条件
- §5.3 hybrid approach 增加第 7/9 条
- T18/T19 新增测试用例

> **✅ Resolved**: 区分错误类型，余额不足通知用户，端点错误 auto-advance

### 11.5 `confirm-accept` 失败的回退策略 → 已补充

- R18 新增规则：按错误类型分流（buyer 余额不足 → cli_failed；其他 → auto-advance）
- §3.2.3 增加 confirm-accept 失败分流说明
- §4.8 新增边界条件
- §5.3 hybrid approach 增加第 8/10 条
- T20/T21 新增测试用例

> **✅ Resolved**: 余额不足不 advance，其他错误可 advance

### 11.6 Auto-consume 循环安全上限 → 已补充

- R19 新增规则：MAX_AUTO_CONSUME_ATTEMPTS = 20
- §5.1 伪代码改为迭代 loop + 上限检查
- Q4 更新为包含上限
- T22 新增测试用例

> **✅ Resolved**: 20 次硬上限 + audit log 告警

### 11.7 `auto_advance_next` 事件 payload 不明确 → 已补充

- §10.1 增加 event payload JSON 格式说明
- 包含 `failedProvider` 和 `reason` 字段用于 audit log

> **✅ Resolved**: payload 包含 jobId/failedProvider/reason

### 11.8 Accept 成功后的用户通知内容 → 已确认

- Accept 成功后用户收到的通知走 `job_accepted` 通用模板（`core.rs:175-208`），无需为 public task 定制
- 该模板已包含 title/description/ASP agentId/payment mode/amount 等关键信息

> **✅ Resolved**: 使用 job_accepted 通用通知模板，不额外定制

---

## 12. Review Round 3 — 终态分叉完整性检查

> 重点审查：匹配阶段每个可能的失败/终态是否都对 public task 判断了 auto-advance，private task 不受影响。

### 12.1 完整终态分叉矩阵

下表列举匹配/协商阶段所有可能导致当前 ASP 失败的路径，以及 public/private 的行为差异：

| # | 终态场景 | 发生位置 | Public 行为 | Private 行为 | Spec 覆盖 |
|---|---------|----------|------------|-------------|----------|
| F1 | route error (ASP offline / not provider) | match_provider in-process loop | auto reject + next (R4) | 弹 A/B/C 卡片 | ✅ §3.2.1, §5.1 |
| F2 | failed list 命中 | match_provider in-process loop | auto reject + next (R15) | N/A (无 auto-pick) | ✅ §4.7, §5.1 |
| F3 | x402 serviceParams 推断失败 | match_provider in-process loop | auto reject + next (R11) | 弹 decision card 问用户 | ✅ §4.4, §5.1 |
| F4 | **asp-match 无 services** | **match_provider / LLM prompt** | **auto reject + next (R20)** | 通知用户 | ✅ §3.2.1 (Round 3 补充) |
| F5 | **asp-match API 错误** | **match_provider / LLM prompt** | **auto reject + next (R20)** | 推 cli_failed 卡片 | ✅ §3.2.1 (Round 3 补充) |
| F6 | **set-asp 失败** | **match_provider / LLM prompt** | **auto reject + next (R20)** | 推 cli_failed 卡片 | ✅ §3.2.1 (Round 3 补充) |
| F7 | **A2A serviceParams 需用户输入** | **provider_conversation_pick_a2a prompt** | **自动推断，不问用户 (R21)** | 弹 decision card 问用户 | ✅ §3.2.1 (Round 3 补充) |
| F8 | negotiate_reply 2-round 超限 | sub-session (events.rs) | mark-failed + auto_advance_next (R4, R12) | 弹 no_asp_found 卡片 | ✅ §3.2.2 |
| F9 | negotiate_reply 5-min 超时 | sub-session (events.rs) | 同 F8 (R12) | 同 F8 | ✅ §3.2.2 |
| F10 | provider_reject (ASP 主动拒绝) | user-session (events.rs) | reset/asp + auto-advance (R4) | 弹 A/B/C/D 卡片 | ✅ §3.2.6 |
| F11 | provider_applied over-budget | user-session (core.rs) | reject-apply + auto-advance (R5) | 弹 A/B/C/D 卡片 | ✅ §3.2.3 |
| F12 | confirm-accept 失败 (余额不足) | user-session (core.rs) | cli_failed 通知用户，不 advance (R18) | cli_failed 通知用户 | ✅ §4.8 |
| F13 | confirm-accept 失败 (其他错误) | user-session (core.rs) | mark-failed + auto-advance (R18) | cli_failed 通知用户 | ✅ §4.8 |
| F14 | x402 支付失败 (buyer 余额不足) | user-session | 通知用户，不 advance (R17) | 通知用户 | ✅ §4.5 |
| F15 | x402 支付失败 (ASP 端点错误) | user-session | mark-failed + auto-advance (R17) | 推 cli_failed / 通知用户 | ✅ §4.5 |
| F16 | auto-consume 达 20 次上限 | match_provider loop | 静默等待 + audit log (R19) | N/A | ✅ §5.1 |
| F17 | `task_requests()` API 失败 | auto-consume loop 内 | 跳出循环 + audit log（R4 补充） | N/A | ✅ §13.1, §13.5 |
| F18 | `task_reject()` 失败 | auto-consume loop 内 | 加入 skip set + continue（R4 补充） | N/A | ✅ §13.1, §13.5 |
| F19 | `reject_apply` 失败（over-budget） | core.rs provider_applied | 不 advance，推 cli_failed（apply 仍 active） | 推 cli_failed（不变） | ✅ §13.1 |
| F20 | x402 子流程失败（x402-check / price / inputRequired） | auto-consume x402 分支 | 见 §13.2 分类处理 | 弹 decision card（不变） | ✅ §13.2 |

### 12.2 Private task 不受影响验证

所有 auto-consume / auto-advance 代码路径都以 `visibility == 0` (R16) 为前提条件。Private task (`visibility == 1`) 始终走现有的 decision card + 用户手动选择流程：

| 检查点 | 验证 |
|--------|------|
| `provider_conversation()` 入口 | visibility 检查 → private 走原逻辑（弹 accept/reject 卡片） |
| `negotiate_reply()` over-limit | is_public 检查 → private 推 no_asp_found 卡片 |
| `provider_reject()` | visibility 检查 → private 推 A/B/C/D 卡片 |
| `provider_applied()` over-budget | visibility 检查 → private 推 A/B/C/D 卡片 |
| `provider_applied()` confirm-accept 失败 | visibility 检查 → private 推 cli_failed 卡片 |
| `provider_conversation_pick_a2a()` 各步骤 | visibility 检查 → private 保持原有 notify/decision 行为 |
| `user_decision_*` handlers | private 仍然需要全部保留，不删除 |

### 12.3 `negotiate_reply` 中 `is_public` 判定需更新

**代码现状** (`events.rs:136, 152`):
```rust
let is_public = p.visibility == Some(0) || p.service_id.is_none();
```

**需改为**（auto-consume 分叉部分）:
```rust
// 价格协商规则仍用原逻辑（service_id.is_none() 意味着无锁定价格）
let is_price_negotiable = p.visibility == Some(0) || p.service_id.is_none();
// auto-consume 分叉仅用 visibility
let is_auto_consume = p.visibility == Some(0);
```

- L136 的 recovery fallback（provider_agent_id 为空时回退到 provider_conversation）：用 `is_auto_consume` 决定是走 auto-consume 版本还是原版本
- L152 的价格协商规则：保持 `is_price_negotiable`
- Over-limit 分叉（L214-229）：用 `is_auto_consume` 决定是推卡片还是 auto-advance

---

## 13. Review Round 4 — 端到端链路完整性 + Private 影响检查

> 逐条追踪 F1-F16 每条失败路径在实际代码中的完整调用链，验证 auto-advance 链路无断点。同时检查是否存在遗漏的失败场景、Private 任务是否受影响。

### 13.1 新增失败场景（F17-F20）

| # | 终态场景 | 发生位置 | Public 行为 | Private 行为 | 说明 |
|---|---------|----------|------------|-------------|------|
| F17 | `task_requests()` API 失败 | auto-consume loop 内 | 跳出循环 + audit log 告警，不 advance（无法判断队列状态） | N/A | §5.1 伪代码缺少此分支 |
| F18 | `task_reject(group_id)` 失败 | auto-consume loop 内 | 将该 ASP 加入本次循环的 skip set + audit log，`continue` 避免对同一条消息无限重试 | N/A | 防止 reject 失败导致无限循环 |
| F19 | `reject_apply` 失败（over-budget 场景） | `core.rs provider_applied()` L200-205 | 不 auto-advance（reject 未成功 → ASP 的 apply 仍活跃，切换下一个 ASP 会冲突）；推 `cli_failed` 给用户 | 推 `cli_failed` 给用户（现有行为不变） | apply 仍然 on-chain active，不能跳过 |
| F20 | x402 子流程失败（3 种细分） | auto-consume x402 分支 | 见 §13.2 | 弹 decision card / cli_failed（现有行为不变） | x402 auto-consume 内部步骤 |

### 13.2 x402 auto-consume 子流程补充

x402 路线在 auto-consume 中的完整步骤和失败处理：

```
x402 auto-consume 子流程:
1. designated_route_inner → route="x402"，返回 endpoint + fee 信息
2. can_auto_infer_params(serviceDescription, taskDescription)
   → 推断失败: task_reject → continue (已在 §5.1 覆盖)
   → 推断成功: 进入下一步

3. x402-check --endpoint <endpoint>
   → 失败 (endpoint 不可达/无效): task_reject → continue → try next ASP
   → 成功: 获得 acceptsJson, actualAmount, tokenSymbol

4. 价格校验:
   a. actualAmount > maxBudget → task_reject → auto-advance (超预算，换下一个)
   b. |actualAmount - registeredFee| / registeredFee > 1% (price mismatch):
      → Public: 如果 actualAmount ≤ maxBudget，自动接受实际价格（不弹卡片）
      → Public: 如果 actualAmount > maxBudget，同 4a
   c. 价格一致 → 继续

5. set-payment-mode (如果尚未设为 x402)
   → 失败: task_reject → auto-advance

6. task-402-pay
   → buyer 余额不足: 通知用户，不 advance (R17)
   → endpoint 返回 inputRequired:
      → 尝试从 task description 推断 body 参数
      → 推断成功: 带 --body 重试 task-402-pay → 成功则继续
      → 推断失败 / 重试仍 inputRequired: task_reject → auto-advance
   → endpoint 错误 (5xx / timeout): mark-failed → auto-advance (R17)
   → 成功: confirm-accept → Accepted
```

### 13.3 `auto_advance_next` 事件 handler 逻辑补充

§10.1 原描述过于简略（"re-fetch → auto-pick first"）。补充完整逻辑：

`auto_advance_next` handler 在 `flow.rs` 中的实现应 **复用** `provider_conversation_auto_consume()` 的完整逻辑，而非只是简单 re-fetch：

```rust
Event::Other(ref s) if s == "auto_advance_next" => {
    // 从 sub-session 跨 session 到达 user-session
    // 复用完整的 auto-consume 逻辑：
    // 1. active session guard
    // 2. failed list check (R15)
    // 3. FIFO queue fetch (task_requests)
    // 4. route check (designated_route_inner)
    // 5. auto-pick / reject / next
    // 6. loop safety (R19)
    super::flow_negotiate::provider_conversation_auto_consume(&ctx)
}
```

这样确保跨 session 返回后的 auto-advance 和初始 auto-consume 走完全相同的逻辑，避免遗漏 failed list / route check 等步骤。

### 13.4 Visibility 默认值风险

`flow.rs` 中 `provider_applied` 和 `provider_reject` 事件路由，visibility 从 `--message` JSON 中读取，默认值为 `1`（private）：

```rust
// flow.rs L385-387
let visibility = message
    .and_then(|m| m.get("visibility"))
    .and_then(|v| v.as_i64())
    .unwrap_or(1);  // ← 默认 private
```

**风险**: 如果 daemon/backup session 推送事件时未携带 `visibility` 字段，public 任务会被当作 private 处理 → 不触发 auto-advance。

**防范措施**: 实现时需确保所有推送 `provider_applied` / `provider_reject` 事件的调用方（daemon、next-action caller）都从 prefetched context 中读取 visibility 并显式传入。可在 handler 入口增加回退：

```rust
let visibility = message.and_then(|m| m.get("visibility")).and_then(|v| v.as_i64())
    .or_else(|| ctx.prefetched.and_then(|p| p.visibility))
    .unwrap_or(1);
```

### 13.5 §5.1 伪代码更新（补充 F17/F18 处理）

```rust
const MAX_AUTO_CONSUME_ATTEMPTS: usize = 20;

pub(crate) async fn provider_conversation_auto_consume(ctx: &FlowContext<'_>) -> String {
    // Guard: active session?
    let has_active = negotiate::get_designated_provider(ctx.job_id).ok().flatten().is_some();
    if has_active {
        return "[provider_conversation] Active session exists; skip (R2).\n".to_string();
    }

    let failed_list = negotiate::get_failed_list(ctx.job_id);
    let mut skip_set: HashSet<String> = HashSet::new(); // F18: reject 失败时的临时 skip

    for attempt in 0..MAX_AUTO_CONSUME_ATTEMPTS {
        // F17: task_requests 失败 → 跳出循环
        let items = match okx_a2a::task_requests_for_job(ctx.job_id) {
            Ok(v) => v,
            Err(e) => {
                audit::log("auto_consume", "task_requests_failed", ...);
                return format!("[auto_consume] task_requests failed: {e}; end turn.\n");
            }
        };
        if items.is_empty() {
            return "[auto_consume] Queue empty; silent wait (R7).\n".to_string();
        }

        let first = &items[0];
        let asp_agent_id = extract_agent_id(first);
        let group_id = extract_group_id(first);

        // F18: skip set (reject 失败过的消息)
        if skip_set.contains(&group_id) {
            // 已经尝试过 reject 但失败了，跳过避免无限循环
            // 实际上不应该到这里（reject 失败的消息还在队列里会被再次取到）
            // → 退出循环，等下次事件触发
            return "[auto_consume] Stuck on unrejectable message; end turn.\n".to_string();
        }

        // R15: failed list check
        if failed_list.contains(&asp_agent_id) {
            if let Err(e) = okx_a2a::task_reject(&group_id) {
                skip_set.insert(group_id.clone());
                audit::log("auto_consume", "reject_failed_asp_reject_error", ...);
            }
            continue;
        }

        // Route check
        let route = designated_route_inner(&asp_agent_id, None).await;
        match route {
            Ok(json) => {
                let r = json.get("route").and_then(|v| v.as_str()).unwrap_or("error");
                match r {
                    "a2a" => {
                        // asp-match → 如果无 services 或 API 错误 → reject → next (R20)
                        // set-asp → 如果失败 → reject → next (R20)
                        // 成功 → 创建 session → 退出循环
                        return provider_conversation_pick_a2a_auto(ctx, &asp_agent_id, &group_id).await;
                    }
                    "x402" => {
                        // 见 §13.2 完整子流程
                        return branch_x402_auto_consume(ctx, &asp_agent_id, &group_id, &json).await;
                    }
                    _ => { // "error"
                        let _ = okx_a2a::task_reject(&group_id);
                        continue;
                    }
                }
            }
            Err(e) => {
                let _ = okx_a2a::task_reject(&group_id);
                continue;
            }
        }
    }
    // R19: max attempts reached
    audit::log("auto_consume", "max_attempts_reached", ...);
    "[auto_consume] Max auto-consume attempts reached; silent wait.\n".to_string()
}
```

### 13.6 Private task 不受影响确认

逐一追踪 F17-F20 和 §13.2 新增的代码路径，确认 **所有** auto-consume / auto-advance 入口都以 `visibility == 0` 为前提：

| 入口点 | Private 行为 | 验证 |
|--------|------------|------|
| `provider_conversation()` 入口的 visibility 分叉 | 走原版 `provider_conversation_cli_inner()` 弹卡片 | ✅ visibility check 在入口 |
| `provider_applied()` over-budget 的 auto-advance | visibility check → private 弹 A/B/C/D 卡片 | ✅ 已有 visibility 参数 |
| `provider_reject()` 的 auto-advance | visibility check → private 弹 A/B/C/D 卡片 | ✅ 已有 visibility 参数 |
| `negotiate_reply()` over-limit 的 auto-advance | `is_auto_consume` check → private 推 no_asp_found 卡片 | ✅ §12.3 split |
| `auto_advance_next` handler | 调用 `provider_conversation_auto_consume()` 前检查 visibility | ✅ handler 内 guard |
| F17/F18 错误路径 | 仅在 auto-consume 函数内，private 不会进入 | ✅ 上层 visibility guard |
| F19 reject-apply 失败 | 走 `cli_failed`（public/private 相同行为） | ✅ 不影响 |
| F20 x402 子流程 | 仅在 auto-consume 函数内 | ✅ 上层 visibility guard |

**结论**: 所有新增路径都在 `visibility == 0` guard 之后，Private 任务逻辑不受影响。

### 13.7 测试用例补充（T29-T34）

| # | Case | Expected |
|---|------|----------|
| T29 | `task_requests()` API 在 auto-consume 中间失败 | 跳出循环 + audit log，结束 turn |
| T30 | `task_reject()` 在 auto-consume 中失败 | 加入 skip set，不对同一消息重试 |
| T31 | reject-apply 在 over-budget 场景失败 | 不 auto-advance，推 cli_failed |
| T32 | x402 endpoint x402-check 失败（不可达） | task_reject → auto-advance 到下一个 ASP |
| T33 | x402 price mismatch，actualAmount ≤ maxBudget | 自动接受实际价格，继续 x402 流程 |
| T34 | x402 task-402-pay 返回 inputRequired，参数可推断 | 自动推断 body 参数 → 带 --body 重试 |

### 13.8 F1-F16 追踪结果（代码验证）

| # | 代码中的实际调用链 | 是否有断点 | 备注 |
|---|-------------------|-----------|------|
| F1 | `designated_route_inner()` → route="error" → `task_reject()` → loop continue | ✅ 无断点 | in-process loop 内 |
| F2 | `get_failed_list()` check → `task_reject()` → loop continue | ✅ 无断点 | in-process loop 内 |
| F3 | x402 route + `can_auto_infer_params()` 失败 → `task_reject()` → loop continue | ✅ 无断点 | in-process loop 内 |
| F4 | `asp-match` 无 services → `task_reject()` → loop continue (R20) | ✅ 无断点 | 需在 `provider_conversation_pick_a2a_auto` 内处理 |
| F5 | `asp-match` API 错误 → `task_reject()` → loop continue (R20) | ✅ 无断点 | 同上 |
| F6 | `set-asp` 失败 → `task_reject()` → loop continue (R20) | ✅ 无断点 | 同上 |
| F7 | serviceParams 推断 → auto-infer (R21)，不弹卡片 | ✅ 无断点 | 改变行为但不中断链路 |
| F8 | over-limit → `mark-failed` → `auto_advance_next` event → `provider_conversation_auto_consume()` | ✅ 无断点 | 跨 session，需要 event 传递 |
| F9 | 同 F8 | ✅ 无断点 | LLM prompt 层面检测 |
| F10 | `reset/asp` → auto-advance → `provider_conversation_auto_consume()` | ✅ 无断点 | user-session 内 in-process |
| F11 | `reject_apply` → auto-advance → `provider_conversation_auto_consume()` | ✅ 无断点 | user-session 内 in-process |
| F12 | `confirm-accept` 余额不足 → 通知用户，不 advance | ✅ 无断点 | 正确终止 |
| F13 | `confirm-accept` 其他错误 → mark-failed → auto-advance | ✅ 无断点 | user-session 内 |
| F14 | x402 余额不足 → 通知用户，不 advance | ✅ 无断点 | 正确终止 |
| F15 | x402 endpoint 错误 → mark-failed → auto-advance | ✅ 无断点 | user-session 内 |
| F16 | 20 次上限 → 静默等待 + audit log | ✅ 无断点 | loop 自然终止 |

**F1-F16 全部追踪通过，auto-advance 链路无断点。**
