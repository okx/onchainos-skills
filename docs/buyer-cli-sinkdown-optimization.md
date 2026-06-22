# Buyer 侧 CLI 下沉优化方案

> 核心原则：凡是规则清晰、输入输出确定的逻辑，都不应该消耗 token 来"推理"。
> 基于 buyer 三种 session（User / Backup / Job）全流程分析，识别出 10 项可下沉到 CLI 的逻辑。

---

## 背景：三种 Session 架构

一个任务生命周期涉及三种 session，理解它们的职责和 context 规模是优化的前提：

### Session 类型

| Session | 可见性 | sessionKey 特征 | 职责 | RESUME 次数 | context 规模 |
|---|---|---|---|---|---|
| **User Session（主 session）** | 用户可见 | 不含 `:group:` | 用户意图解析、任务发布/修改、决策卡片展示与 relay | 1-3 次 | 低 |
| **Backup Session** | 用户不可见 | 含 `backup:<jobId>` | 接收 job_created → 推荐/指定服务商 → 创建 Job session → 终态通知 | 2-5 次 | 中 |
| **Job Session** | 用户不可见 | 含 `&job=<jobId>` | buyer↔provider 协商、系统事件处理、决策 relay | **10-20 次** | **高（累积 100K+）** |

### 系统通知路由

```
系统通知 → 有匹配 Job session? → 推送到 Job session（最常见）
         → 无 Job session，有 Backup session? → 推送到 Backup（边界）
         → 无 Backup? → 创建 Backup session → 推送（边界）
```

### Agent-Agent 协商消息路由

```
协商消息 → 有匹配 Job session? → 推送到 Job session（最常见）
         → 买家曾有该 jobId 沟通记录（本地删除）? → 重建 Job session → 推送（边界）
         → 买家从未与卖家在该 job 沟通（卖家主动沟通 public）? → Backup dispatch 新沟通请求（常见）
```

### 事件→Session 映射

| 事件 | 目标 Session | 处理者 |
|---|---|---|
| job_created | Backup | match_provider.rs |
| designated_a2a/x402 (route_only) | Backup | designated.rs Phase 1 |
| designated_a2a (branch_a2a) | **新建 Job → 切换到 Job** | designated.rs Phase 2 |
| job_payment_mode_changed | Backup 或 Job | events.rs |
| negotiate_reply/ack/counter | Job | events.rs |
| provider_applied | Job | core.rs |
| job_accepted | Job | core.rs |
| job_submitted | Job | core.rs |
| deliverable_received | Job | core.rs |
| job_completed/refunded/expired/closed | Backup（终态） | terminal.rs |
| dispute_resolved | Backup（终态） | dispute.rs |
| user_decision_* | Job 或 Backup | flow.rs:421-506 |

### 优化重点

**Job Session** 是优化主战场：RESUME 10-20 次，每减少 1K token 初始 context = 节省 10-20K token/task。
**Backup Session** 次之：RESUME 2-5 次，但 job_created 处理链路有大量确定性逻辑。
**User Session** 优先级最低：RESUME 1-3 次，倍增效应小。

---

## 概述

| # | 优化项 | 类型 | 目标 Session | 预估节省 | 优先级 | 依赖 | 状态 |
|---|---|---|---|---|---|---|---|
| 1.1 | 终态事件自动处理 | CLI 下沉 | Backup | ~3K token + 1 轮/terminal | P2 | 无 | 待设计 |
| 1.2+1.3 | negotiate_ack → confirm 链路合并 | CLI 下沉 | Job | ~61K token/task（加权） | P0 | 无 | **方案已定** |
| 1.4 | confirm-accept 自动化 | CLI 下沉 | Job | ~180K token + 1 轮/task | P1 | 1.2+1.3 | 待设计 |
| 1.5 | 入站消息路由下沉 | CLI 下沉 | User | ~1.6K token/session | P2 | 无 | 待设计 |
| 1.6 | LOCALIZATION_PREFIX 去重 | 输出优化 | Job + Backup | ~3K token/task | P1 | 无 | 待实施 |
| 1.7 | Preamble 压缩 | 输出优化 | Job + Backup | ~6K token/task | P1 | 无 | 待实施 |
| 1.8 | user_decision 路由下沉 | CLI 下沉 | Job + Backup | ~3K token/task | P1 | 无 | 待设计 |
| **1.9** | **job_created 推荐链路自动化** | **CLI 下沉** | **Backup** | **~15K token + 1 轮** | **P1** | **无** | **新增** |
| **1.10** | **SKILL_PREFETCH 自动化** | **CLI 下沉** | **Backup** | **~2K token** | **P2** | **无** | **新增** |

---

## 1.1 终态事件自动处理

### Session 归属：Backup Session

终态事件（job_completed, job_refunded, job_expired, job_closed, dispute_resolved）路由到 **Backup Session**，而非 Job Session。Backup 的 context 规模小于 Job，但终态处理本身是 100% 确定性的。

### 现状

```
Turn N: 系统事件 → next-action(job_completed) → CLI 返回 playbook
  → LLM 读 preamble_slim (~500 token) + playbook body (~300 token)
  → LLM 调 xmtp_dispatch_user（预格式化模板）
  → LLM 调 terminal_session_hint cleanup
  → 结束
```

### 问题

LLM 在这里是纯粹的**工具调用转发器**：读模板 → 调 xmtp_dispatch_user → 结束。没有任何判断或分支。但 RESUME 上下文中，Backup session 此时已累积了 job_created + 推荐/指定 + 协商确认等前序 turn 的 context。

### 代码位置

| 文件 | 位置 | 事件 |
|---|---|---|
| `flow_lifecycle/terminal.rs:5-23` | `job_refunded()` | Step 1 xmtp_dispatch_user + Step 2 terminal_session_hint |
| `flow_lifecycle/terminal.rs:48-61` | `job_expired()` | Step 1 xmtp_dispatch_user |
| `flow_lifecycle/terminal.rs:63-82` | `job_closed()` | Step 1 xmtp_dispatch_user + terminal_session_hint |
| `flow_lifecycle/core.rs` | `job_completed()` | Step 1 xmtp_dispatch_user + terminal_session_hint |
| `flow_lifecycle/dispute.rs` | `dispute_resolved()` | 分支 buyer-wins/seller-wins + xmtp_dispatch_user |

### 可自动化的 vs 需要 LLM 的

| 事件 | 可自动化? | 原因 |
|---|---|---|
| job_refunded | ✅ 完全确定性 | 模板通知 + 结束 |
| job_expired | ✅ 完全确定性 | 模板通知 + 结束 |
| job_closed | ✅ 完全确定性 | 模板通知 + 结束 |
| job_completed (escrow) | ✅ 完全确定性 | 模板通知 + 结束 |
| dispute_resolved | ✅ 完全确定性 | 按 buyer-wins/seller-wins 分支通知 + 结束 |
| job_auto_refunded | ✅ 完全确定性 | 模板通知 + 结束 |
| submit_expired | ⚠️ 部分 | 先调 `claim-auto-refund` CLI 再通知 |
| reject_expired | ⚠️ 部分 | 先调 `claim-auto-refund` CLI 再通知 |
| job_auto_completed | ❌ 需要 LLM | auto-rate 需要 LLM 评估交付质量 |

### 方案

**方案 A（推荐）：CLI 内部自动发送通知**

CLI 的 next-action 检测到纯终态事件时，内部直接调用 xmtp_dispatch_user，返回：

```jsonc
{ "terminal": true, "notified": true, "event": "job_refunded", "action": "session_cleanup_only" }
```

LLM 只需执行 terminal_session_hint cleanup。

**改动量**: ~150 行 Rust
**前提**: CLI 需要能直接调用 xmtp_dispatch_user（当前仅 LLM 通过 MCP tool 调用）

**方案 B（最小改动）：preamble_minimal**

新增 `preamble_minimal`（~100 token），仅含 Rule 9 + Rule 7，用于纯终态事件。

**改动量**: ~20 行 Rust

### 收益

| 方案 | Token 节省 | 轮次节省 | 改动量 |
|---|---|---|---|
| A (CLI 自动通知) | ~3K token + 1 轮/terminal event | 每任务 1-2 轮 | 150 行 Rust |
| B (preamble_minimal) | ~2.4K token | 0 轮 | 20 行 Rust |

---

## 1.2 + 1.3 negotiate_ack → [intent:confirm] 链路合并

### Session 归属：Job Session（核心优化）

negotiate_ack 事件发生在 **Job Session**，这是 RESUME 次数最多（10-20 次）、context 最大的 session。每减少 1 轮 = 节省一次完整 context 读取（可能 100K+ token input）。

### 现状

#### 当前 escrow 协商确认链路（2 个 LLM turn）

```
Turn N: LLM 收到 [intent:ack]（在 Job session）
  → next-action(negotiate_ack) → 读 playbook
  → Step 1: 比对 ACK 与 PROPOSE 字段
  → Step 2: save-agreed (CLI)
  → Step 3: set-payment-mode (CLI, 链上签名 + 广播)
  → 结束 turn, 等待 job_payment_mode_changed 系统事件

Turn N+1: LLM 收到 job_payment_mode_changed（可能在 Job 或 Backup session）
  → next-action(job_payment_mode_changed) → 读 playbook
  → Step 1: 读信封中 paymentMode
  → Step 2: xmtp_send [intent:confirm] (从会话历史回溯字段)
  → Step 3: xmtp_dispatch_user 通知用户
  → 结束 turn, 等待 provider apply

[整个过程: 2 个 LLM turn + 1 个链上等待 (5-30s) + 3 个 CLI 调用]
```

#### 代码现状审计

| 文件 | 位置 | 发现 |
|---|---|---|
| `create.rs:236` | `"paymentMode": 0` | 创建任务时固定 paymentMode=0（unset）— 设计决策 |
| `accept.rs:134-140` | `let already_set = ...` | CLI **已有** `alreadySet` 检测 |
| `accept.rs:160` | `if !already_set { ... }` | `alreadySet=true` 时跳过链上 setPaymentMode |
| `accept.rs:164` | `{ "paymentMode": mode_int }` | setPaymentMode API 只传 paymentMode |
| `accept.rs:227-231` | `crate::output::success(...)` | `alreadySet=true` 时返回指引 |
| `events.rs:214-222` | negotiate_ack playbook | **未适配 `alreadySet`**，硬编码 set-payment-mode |
| `designated.rs:256-258` | branch_x402 | **已适配 `alreadySet`** |
| `negotiate.rs:168-227` | save-agreed | 持久化到本地文件 |

#### 触发场景分析

| 场景 | 到达 negotiate_ack 时链上 paymentMode | 是否触发 alreadySet |
|---|---|---|
| **首次 A2A 协商**（标准 happy path） | 0 (unset) | 否，需 set-payment-mode |
| **切换 provider 后重新协商** | 1 (escrow, 前一轮已 set) | **是**，可跳过 |
| **x402 → A2A 降级后协商** | 3 (x402, 前一轮已 set) | 否，mode 不同需 set |
| **用户手动 set-payment-mode 后协商** | 1 (escrow) | **是**，可跳过 |

### 方案

#### 总体思路

将 negotiate_ack → [intent:confirm] 的 2 个 LLM turn 合并为 **条件 1 个 LLM turn**：

1. **CLI 层新增 `ack-to-confirm` 复合命令**，一次调用完成 save-agreed + paymentMode 检测 + 条件跳过
2. **CLI 层新增 `get-agreed` 辅助命令**，从持久化数据读字段
3. **Playbook 层适配 `confirmNow` 分支**

#### CLI 改动 A：新增 `ack-to-confirm` 子命令

**文件**: `cli/src/commands/agent_commerce/task/buyer/accept.rs`

```
onchainos agent ack-to-confirm <jobId> \
  --provider-agent-id <providerAgentId> \
  --token-symbol <tokenSymbol> \
  --token-amount <tokenAmount> \
  --agent-id <agentId>
```

**内部逻辑**：

```
1. save-agreed(jobId, providerAgentId, tokenSymbol, tokenAmount, agentId)
   - 持久化 + 校验 tokenAmount <= maxBudget
   - 失败 → 返回错误

2. 查询 task detail → 获取当前链上 paymentMode

3. 判断:
   if 当前链上 paymentMode == escrow(1):
     → 跳过 set-payment-mode
     → 返回 { "confirmNow": true, ... }
   else:
     → 调用 set-payment-mode(escrow)
     → 返回 { "confirmNow": false, ... }
```

**输出 JSON**：

```jsonc
// Case A: paymentMode 已是 escrow → 跳过链上，当前 turn 直接 confirm
{
  "ok": true,
  "confirmNow": true,
  "confirmContent": "jobId: 0x...\npaymentMode: escrow\ntokenSymbol: USDT\ntokenAmount: 0.1\n[intent:confirm]",
  "userNotifyContent": "[Escrow Confirmed] ...",
  "savedAgreed": true
}

// Case B: 需要链上 set-payment-mode → 等 event
{
  "ok": true,
  "confirmNow": false,
  "waitFor": "job_payment_mode_changed",
  "txHash": "0x...",
  "savedAgreed": true
}
```

**预估改动量**: ~120 行 Rust

#### CLI 改动 B：新增 `get-agreed` 辅助命令

```
onchainos agent get-agreed <jobId>
```

返回 save-agreed 持久化的本地文件数据，无网络请求。**预估改动量**: ~40 行 Rust

#### Playbook 改动

negotiate_ack 适配 `confirmNow` 双分支，job_payment_mode_changed 用 `get-agreed` 替代会话历史回溯。

**预估改动量**: ~40 行 playbook 文本

### 收益分析

#### Token 节省估算

| 场景 | 概率 | 节省 tokens | 加权节省 |
|---|---|---|---|
| 首次协商（paymentMode=0） | ~60% | ~10K (playbook 精简 + 可靠性) | 6K |
| 切换 provider（paymentMode=1） | ~30% | ~180K (消除 1 轮 RESUME 上下文) | 54K |
| 其他（x402 降级等） | ~10% | ~10K | 1K |
| **加权平均** | | | **~61K/task** |

### 风险评估

| 风险 | 等级 | 缓解措施 |
|---|---|---|
| save-agreed 失败 → set-payment-mode 不该执行 | 不存在 | 内部严格先 save 后 set |
| save-agreed 成功 → set-payment-mode 失败 | 低 | 文件幂等可重写；LLM 推送用户决策卡 |
| alreadySet 误判（竞态） | 极低 | CLI 内部用同一个 task_resp |
| LLM 忽略 `confirmNow` 分支 | 低 | MANDATORY 标记 + designated.rs 已验证 |

### 实施计划

- Phase 1 (~0.5 天): CLI 新增 `ack-to-confirm` + `get-agreed`
- Phase 2 (~0.5 天): Playbook 改动
- Phase 3 (~0.5 天): 测试验证
- Phase 4 (~0.5 天): Skill 文件同步

---

## 1.4 confirm-accept 自动化

### Session 归属：Job Session

provider_applied 事件在 **Job Session**，context 已累积到 80K+。

### 现状

```
Turn N: LLM 收到 provider_applied（Job session）
  → next-action(provider_applied) → preamble_medium + playbook
  → Step 1: common context 获取参数
  → Step 2: onchainos agent confirm-accept {jobId} ...
  → 结束 turn
```

### 问题

100% 确定性逻辑：到达 provider_applied 时所有参数已通过 save-agreed 持久化。LLM 仅"读参数 → 拼命令 → 调 CLI"。

### 方案

新增 `auto-confirm-accept`，从 save-agreed 本地文件读取：

```
onchainos agent auto-confirm-accept <jobId> --agent-id <agentId>
```

**改动量**: ~80 行 Rust。**依赖**: 1.2+1.3 的 save-agreed 持久化。

### 收益

| 维度 | 优化前 | 优化后 | 节省 |
|---|---|---|---|
| CLI 调用 | 2 次 | 1 次 | 1 次 |
| Playbook 输出 | ~1.2K token | ~200 token | ~1K token |
| providerAgentId 来源 | 会话历史（有丢失风险） | 本地文件（确定性） | 可靠性↑ |

---

## 1.5 入站消息路由下沉

### Session 归属：User Session

`user-intent-routing.md`（123 行）指导 user session 将用户自由文本映射到任务操作。

### 方案

CLI 新增 `intent-classify` 子命令，关键词+正则预分类，confidence < 0.7 才 fallback 到 LLM。

### 收益

~1,560 token/session

### 改动量

~200 行 Rust

---

## 1.6 LOCALIZATION_PREFIX 去重

### Session 归属：Job Session + Backup Session

`LOCALIZATION_PREFIX`（~200 token）**每次 next-action 都附加**。Job session 15-20 次 next-action 调用中每次都输出相同内容。

### 方案

首次输出完整，后续输出单行引用 `[Localization] 规则同首次输出，未变更。`

### 收益

~200 token × 14-19 次 = **~2,800-3,800 token/task**

### 改动量

~30 行 Rust

### 代码位置

| 文件 | 位置 | 内容 |
|---|---|---|
| `buyer/flow.rs:20-28` | `LOCALIZATION_PREFIX` | buyer 端 L10N const |
| `buyer/flow.rs:29-38` | `L10N_DISPATCH_SHORT` 等 | 翻译指令 const |

---

## 1.7 Preamble 压缩

### Session 归属：Job Session + Backup Session

4 档 preamble 在每次 next-action 输出：

| 变体 | Token 数 | 使用场景 | 主要 Session |
|---|---|---|---|
| `context_preamble` | ~2,800 | job_created + fallback | Backup |
| `preamble_medium` | ~800 | provider_applied / job_accepted | Job |
| `preamble_negotiate` | ~900 | negotiate_reply / counter | Job |
| `preamble_slim` | ~500 | 终态 + 超时 | Backup |

### 方案

A. **规则去重引用化** — 跨 preamble 重复规则引用 SKILL.md 编号（~200-500 token/次节省）
B. **Fallback 降级** — 未匹配事件从 context_preamble 降为 preamble_medium
C. **preamble_minimal** — ~100 token 最小变体，用于纯终态

### 收益

**~6,000-8,000 token/task**

### 改动量

~100 行 Rust

---

## 1.8 user_decision 路由下沉

### Session 归属：Job Session + Backup Session

`flow.rs:421-506` 每个 `user_decision_*` 事件输出 ~400-800 token 语义映射表。

user_decision 在哪个 session 处理取决于用户决策的来源：
- recommend_pick → **Backup**（用户选了推荐的 provider）
- job_submitted (approve/reject) → **Job**
- not_provider/over_budget 等 → **Backup 或 Job**

### 方案

CLI 内部做关键词/模式匹配预分类，高置信度（~80%）直接返回路由结果。

### 收益

~400 token × 80% × 5-10 决策 = **~1,600-3,200 token/task**

### 改动量

~150 行 Rust

---

## 1.9 job_created 推荐链路自动化（新增）

### Session 归属：Backup Session

### 现状

`match_provider.rs:36-75` — `job_created_non_designated_provider()` 在 Backup session 执行：

```
Turn N: Backup 收到 job_created
  → next-action(job_created) → CLI 返回 context_preamble (~2,800 token) + playbook (~800 token)
  → LLM 执行 5 个步骤:
    Step 1: session_status → 获取 Backup sessionKey
    Step 2: xmtp_dispatch_user → 通知用户任务已上链
    Step 3: onchainos agent recommend <jobId> → 获取推荐列表
    Step 4: 翻译推荐卡片为用户语言 ← 唯一需要 LLM 的步骤
    Step 5: pending-decisions-v2 request → 入队决策卡
```

### 问题

5 个步骤中 4 个是确定性的（session_status / dispatch / recommend / request），只有 **Step 4 翻译** 需要 LLM 参与。context_preamble（2,800 token）是最重的 preamble，但 job_created 是 Backup 的第一个事件，此时 context 还不大。

### 方案

**方案 A（推荐）：CLI 合并 Step 1-3 + 5，LLM 只做 Step 4**

新增 `job-created-prepare` 复合命令：

```
onchainos agent job-created-prepare <jobId> --agent-id <agentId>
```

内部执行：
1. session_status → 获取 sessionKey
2. xmtp_dispatch_user → 通知用户
3. recommend → 获取推荐列表
4. 返回推荐列表原文（供 LLM 翻译）

LLM 只需：翻译 → 调 `pending-decisions-v2 request`

**方案 B（更激进）：CLI 内部完成翻译**

如果推荐卡片模板是固定的，CLI 可以内置 L10N 模板，完全绕过 LLM。但当前翻译依赖 LLM 的语言能力。

### 收益

| 维度 | 优化前 | 优化后 | 节省 |
|---|---|---|---|
| LLM 工具调用 | 5 次 | 2 次 (翻译 + request) | 3 次 |
| Playbook 输出 | ~3,600 token | ~1,200 token | ~2,400 token |
| 后续 RESUME 中本 turn output 的累积 | 3,600 × 4 次 RESUME | 1,200 × 4 | **~9,600 token** |
| 总计 | | | **~12,000-15,000 token/task** |

### 改动量

~100 行 Rust（复用现有 session_status + dispatch + recommend 逻辑）

### 风险

低 — 合并确定性步骤，不改变业务逻辑。唯一风险是 recommend API 失败时的错误路径需要在 CLI 内处理。

### 代码位置

| 文件 | 位置 | 内容 |
|---|---|---|
| `flow_negotiate/match_provider.rs:36-75` | `job_created_non_designated_provider()` | 当前 playbook |
| `flow_negotiate/match_provider.rs:7-17` | `job_created()` | 入口分支 |

---

## 1.10 SKILL_PREFETCH 自动化（新增）

### Session 归属：Backup Session → Job Session 过渡

### 现状

当 Backup session 创建新 Job session 时（`designated.rs:36-40`），需要发送 warm-up 消息：

```
Step 1: xmtp_start_conversation → 创建 group（返回新 sessionKey）
Step 1.5: xmtp_dispatch_session → 发送 [SKILL_PREFETCH] 到新 session
Step 2: xmtp_send → 发送首条协商消息
```

Step 1.5 是 100% 确定性的固定消息：
```
[SKILL_PREFETCH] Read okx-agent-task/SKILL.md then okx-agent-task/buyer.md.
No action needed for this message — but process all subsequent messages normally via buyer.md §3.5 routing.
```

### 问题

LLM 需要读 playbook 了解 SKILL_PREFETCH 格式 → 拼消息 → 调 xmtp_dispatch_session。完全确定性。

### 方案

将 `xmtp_start_conversation` + `SKILL_PREFETCH` 合并为一个 CLI 命令：

```
onchainos agent create-job-session <jobId> --peer-agent-id <providerId> --agent-id <agentId>
```

内部执行：
1. xmtp_start_conversation → 创建 group
2. 自动发送 SKILL_PREFETCH
3. 返回新 sessionKey

LLM 直接从返回的 sessionKey 发首条协商消息。

### 收益

| 维度 | 优化前 | 优化后 | 节省 |
|---|---|---|---|
| LLM 工具调用 | 3 次 | 2 次 (create-job-session + xmtp_send) | 1 次 |
| Playbook 输出 | PREFETCH 模板 ~150 token | 0 | ~150 token |
| 后续 RESUME 累积 | 150 × 10+ 次 | 0 | **~1,500-2,000 token/task** |

### 改动量

~60 行 Rust

### 风险

极低 — SKILL_PREFETCH 消息格式是固定的，不依赖任何动态数据。

---

## 依赖关系与实施顺序

### 依赖图（按 Session 分组）

```
┌─ Backup Session 优化 ─────────────────────────────────────────────┐
│  1.9 job_created 推荐链路 (独立)                                    │
│  1.10 SKILL_PREFETCH 自动化 (独立)                                  │
│  1.1 终态事件自动处理 (独立)                                         │
└───────────────────────────────────────────────────────────────────┘

┌─ Job Session 优化 ────────────────────────────────────────────────┐
│  1.2+1.3 negotiate_ack → confirm 链路合并 (独立)                    │
│         │                                                          │
│         ▼                                                          │
│  1.4 confirm-accept 自动化 (依赖 1.2+1.3)                          │
│  1.8 user_decision 下沉 (独立)                                      │
└───────────────────────────────────────────────────────────────────┘

┌─ 跨 Session 优化 ────────────────────────────────────────────────┐
│  1.6 L10N 去重 (独立)                                              │
│  1.7 Preamble 压缩 (独立)                                          │
└───────────────────────────────────────────────────────────────────┘

┌─ User Session 优化 ──────────────────────────────────────────────┐
│  1.5 入站路由下沉 (独立)                                            │
└───────────────────────────────────────────────────────────────────┘
```

### 建议实施顺序

| 批次 | 优化项 | 目标 Session | 理由 |
|---|---|---|---|
| **第 1 批** | 1.2+1.3 negotiate 链路合并 | Job | P0，方案已定，收益最高（~61K） |
| **第 1 批** | 1.6 L10N 去重 + 1.7 Preamble 压缩 | Job+Backup | 低风险，可并行 |
| **第 2 批** | 1.4 confirm-accept 自动化 | Job | 依赖第 1 批 |
| **第 2 批** | 1.9 job_created 推荐链路 | Backup | 独立，高收益（~15K） |
| **第 2 批** | 1.8 user_decision 下沉 | Job+Backup | 高频场景 |
| **第 3 批** | 1.1 终态自动处理 | Backup | 需评估 CLI 直接 dispatch 可行性 |
| **第 3 批** | 1.10 SKILL_PREFETCH 自动化 | Backup→Job | 小改动 |
| **第 3 批** | 1.5 入站路由下沉 | User | 倍增效应最小 |

---

## 收益汇总

| 优化项 | 目标 Session | Token 节省/task | 轮次节省 | 改动量 |
|---|---|---|---|---|
| 1.2+1.3 negotiate 合并 | Job | **~61K**（加权） | 0-1 轮 | ~200 行 Rust |
| 1.9 job_created 推荐 | Backup | **~15K** | 0 轮 | ~100 行 Rust |
| 1.7 Preamble 压缩 | Job+Backup | **~6-8K** | 0 轮 | ~100 行 Rust |
| 1.11 Quick Nav (→ Skill 文档) | Job+Backup | **~5K** | 0 轮 | 删 33 行 |
| 1.6 L10N 去重 | Job+Backup | **~3-4K** | 0 轮 | ~30 行 Rust |
| 1.8 user_decision 下沉 | Job+Backup | **~2-3K** | 0 轮 | ~150 行 Rust |
| 1.1 终态自动处理 | Backup | **~3K** | 1-2 轮 | ~150 行 Rust |
| 1.10 PREFETCH 自动化 | Backup→Job | **~2K** | 0 轮 | ~60 行 Rust |
| 1.4 confirm-accept | Job | **~1K** + 可靠性↑ | 0 轮 | ~80 行 Rust |
| 1.5 入站路由下沉 | User | **~1.6K** | 0 轮 | ~200 行 Rust |
| **合计** | | **~100-103K token/task** | 1-3 轮 | ~1,070 行 Rust |

---

## 代码位置索引

| 文件 | 行号 | 内容 |
|---|---|---|
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 94-241 | set-payment-mode (含 alreadySet) |
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 134-140 | alreadySet 检测 |
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 243-429 | confirm-accept 全流程 |
| `cli/src/commands/agent_commerce/task/buyer/negotiate.rs` | 168-227 | save-agreed 实现 |
| `cli/src/commands/agent_commerce/task/buyer/create.rs` | 236 | create-task paymentMode=0 |
| `cli/src/commands/agent_commerce/task/buyer/flow.rs` | 20-38 | L10N const |
| `cli/src/commands/agent_commerce/task/buyer/flow.rs` | 230-320 | preamble 4 档 |
| `cli/src/commands/agent_commerce/task/buyer/flow.rs` | 352-410 | 事件→handler 路由 |
| `cli/src/commands/agent_commerce/task/buyer/flow.rs` | 421-506 | user_decision 路由表 |
| `cli/src/commands/agent_commerce/task/buyer/flow.rs` | 512-540 | preamble 选择 |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/match_provider.rs` | 7-75 | job_created 处理 |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/designated.rs` | 1-167 | 指定服务商（Phase 1 backup / Phase 2 job） |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/designated.rs` | 36-40 | SKILL_PREFETCH |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/events.rs` | 190-225 | negotiate_ack playbook |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/events.rs` | 31-116 | job_payment_mode_changed playbook |
| `cli/src/commands/agent_commerce/task/buyer/flow_lifecycle/terminal.rs` | 5-82 | 终态事件 |
| `cli/src/commands/agent_commerce/task/buyer/flow_lifecycle/core.rs` | 7-46 | provider_applied |
| `cli/src/commands/agent_commerce/task/common/pending_v2.rs` | 220-285 | session 类型判定 |
