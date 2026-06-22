# Buyer 侧 Token 消耗优化方案

> 两部分独立可并行：Part 1 协商确认链路代码优化 + Part 2 Skill 文件组织优化

---

# Part 1: negotiate_ack → [intent:confirm] 链路合并

> save-agreed → set-payment-mode → job_payment_mode_changed → [intent:confirm] 的 2 个 LLM turn 合并为条件 1 turn

## 1. 现状分析

### 1.1 当前 escrow 协商确认链路（2 个 LLM turn）

```
Turn N: LLM 收到 [intent:ack]
  → next-action(negotiate_ack) → 读 playbook
  → Step 1: 比对 ACK 与 PROPOSE 字段
  → Step 2: save-agreed (CLI)
  → Step 3: set-payment-mode (CLI, 链上签名 + 广播)
  → 结束 turn, 等待 job_payment_mode_changed 系统事件

Turn N+1: LLM 收到 job_payment_mode_changed
  → next-action(job_payment_mode_changed) → 读 playbook
  → Step 1: 读信封中 paymentMode
  → Step 2: xmtp_send [intent:confirm] (从会话历史回溯字段)
  → Step 3: xmtp_dispatch_user 通知用户
  → 结束 turn, 等待 provider apply

[整个过程: 2 个 LLM turn + 1 个链上等待 (5-30s) + 3 个 CLI 调用]
```

### 1.2 代码现状审计

| 文件 | 位置 | 发现 |
|---|---|---|
| `create.rs:236` | `"paymentMode": 0` | 创建任务时固定 paymentMode=0（unset）— 设计决策：paymentMode 属于协商阶段产物，不属于创建阶段（commit `eb491244`） |
| `accept.rs:134-140` | `let already_set = ...` | CLI **已有** `alreadySet` 检测：`explicitly_provided && current_mode == payment_mode && current_mode != PaymentMode::None` |
| `accept.rs:160` | `if !already_set { ... }` | `alreadySet=true` 时跳过链上 setPaymentMode 调用 |
| `accept.rs:164` | `{ "paymentMode": mode_int }` | setPaymentMode API **只传 paymentMode**，不含 token/amount |
| `accept.rs:227-231` | `crate::output::success(...)` | `alreadySet=true` 时返回 `"next": "Call next-action --event job_payment_mode_changed"` |
| `events.rs:214-222` | negotiate_ack playbook | **未适配 `alreadySet`**，硬编码"无论如何都调 set-payment-mode + 结束等 event" |
| `designated.rs:256-258` | branch_x402 | **已适配 `alreadySet`**，有 `alreadySet` vs `confirming` 分支指导 |
| `negotiate.rs:168-227` | save-agreed | 持久化到本地文件，含 provider/tokenSymbol/tokenAmount/maxBudget |

### 1.3 触发场景分析

因为 create-task 固定 paymentMode=0，所以首次协商 `alreadySet` 永远不会触发。它只在切换 provider 等二次协商场景中生效：

| 场景 | 到达 negotiate_ack 时链上 paymentMode | 是否触发 alreadySet |
|---|---|---|
| **首次 A2A 协商**（标准 happy path） | 0 (unset) | 否，需 set-payment-mode |
| **切换 provider 后重新协商** | 1 (escrow, 前一轮已 set) | **是**，可跳过 |
| **x402 → A2A 降级后协商** | 3 (x402, 前一轮已 set) | 否，mode 不同需 set |
| **用户手动 set-payment-mode 后协商** | 1 (escrow) | **是**，可跳过 |

> 切换 provider 是第二常见路径（推荐服务商不合适 → 换下一个）。每次切换后重新协商都走完整 set-payment-mode + 等链上事件是不必要的。

## 2. 优化方案

### 2.1 总体思路

将 negotiate_ack → [intent:confirm] 的 2 个 LLM turn 合并为 **条件 1 个 LLM turn**，通过：

1. **CLI 层新增 `ack-to-confirm` 复合命令**，一次调用完成 save-agreed + paymentMode 检测 + 条件跳过
2. **CLI 层新增 `get-agreed` 辅助命令**，让 job_payment_mode_changed 从持久化数据读字段，不再回溯会话历史
3. **Playbook 层适配 `confirmNow` 分支**，在同一 turn 内直接发送 [intent:confirm]

### 2.2 CLI 改动 A：新增 `ack-to-confirm` 子命令

**文件**: `cli/src/commands/agent_commerce/task/buyer/accept.rs` (新增函数)
**注册**: `cli/src/commands/agent_commerce/mod.rs` (新增 clap variant)

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
   - 持久化协商结果到本地文件
   - 校验 tokenAmount <= maxBudget（复用已有逻辑）
   - 失败 → 返回错误，不执行后续步骤

2. 查询 task detail → 获取当前链上 paymentMode
   - 复用 save-agreed 内部已查询的 task_resp（避免重复请求）

3. 判断是否需要链上操作:
   if 当前链上 paymentMode == escrow(1):
     → 跳过 set-payment-mode
     → 返回 { "confirmNow": true, ... }
   else:
     → 调用 set-payment-mode(escrow)
     → 成功: 返回 { "confirmNow": false, ... }
     → 失败: 返回错误（save-agreed 文件已写入，幂等可重入;
              LLM 按 exception-escalation §2 推送错误决策卡给用户）
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

### 2.3 CLI 改动 B：新增 `get-agreed` 辅助命令

**目的**: 消除 job_payment_mode_changed playbook 中"从会话历史回溯 [intent:ack] 字段"的要求 — 这在 context 膨胀时有字段丢失风险。

```
onchainos agent get-agreed <jobId>
```

**返回**: `{ "providerAgentId": "...", "tokenSymbol": "...", "tokenAmount": "..." }`

从 save-agreed 持久化的本地文件读取，无网络请求。

**预估改动量**: ~40 行 Rust

### 2.4 Playbook 改动 A：negotiate_ack 适配双分支

**文件**: `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/events.rs`
**函数**: `negotiate_ack()`

将 Step 2 + Step 3 替换为：

    **Step 2 - ack-to-confirm (save-agreed + 条件 set-payment-mode):**

    onchainos agent ack-to-confirm {job_id} \
      --provider-agent-id <providerAgentId> \
      --token-symbol <tokenSymbol from ACK> \
      --token-amount <tokenAmount from ACK> \
      --agent-id {agent_id}

    **Step 2 result branch (MANDATORY):**
    Inspect the CLI output JSON:

    - "confirmNow": true → paymentMode 已是 escrow，在本 turn 直接发送 [intent:confirm]:
      1. xmtp_send: content = <confirmContent 字段内容>
      2. xmtp_dispatch_user: 通知用户（L10N translate userNotifyContent）
      → end this turn, wait for ASP apply via a2a-agent-chat

    - "confirmNow": false → 需要链上确认:
      → end this turn, wait for job_payment_mode_changed system notification

**删除规则**: "Whatever the on-chain paymentType currently is, you MUST execute this"
**条件化规则**: "in THIS turn [intent:confirm] is absolutely forbidden" → 仅 `confirmNow=false` 时生效

**预估改动量**: ~30 行 playbook 文本

### 2.5 Playbook 改动 B：job_payment_mode_changed 字段来源优化

**文件**: `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/events.rs`
**函数**: `job_payment_mode_changed()`

将 escrow 路径 Step 2 从"回溯会话历史"改为：

    **Step 2 - send [intent:confirm]:**

    onchainos agent get-agreed {job_id}

    Read the returned JSON, then xmtp_send:
      content = "jobId: {jobId}\npaymentMode: escrow\ntokenSymbol: {tokenSymbol}\ntokenAmount: {tokenAmount}\n[intent:confirm]"

**预估改动量**: ~10 行 playbook 文本

### 2.6 Clap 注册

**文件**: `cli/src/commands/agent_commerce/mod.rs`

```rust
#[command(name = "ack-to-confirm")]
AckToConfirm {
    job_id: String,
    #[arg(long)] provider_agent_id: String,
    #[arg(long)] token_symbol: String,
    #[arg(long)] token_amount: String,
    #[arg(long)] agent_id: Option<String>,
},

#[command(name = "get-agreed")]
GetAgreed {
    job_id: String,
},
```

## 3. 收益分析

### 3.1 标准 happy path（首次协商，paymentMode=0）

| 维度 | 优化前 | 优化后 | 节省 |
|---|---|---|---|
| LLM 轮次 | 2 轮 | 2 轮 (不变, 需等链上) | 0 |
| CLI 调用 | 2 次 (save-agreed + set-payment-mode) | 1 次 (ack-to-confirm) | 1 次 |
| 工具调用 | 2 次 (xmtp_send + xmtp_dispatch_user) | 2 次 (不变) | 0 |
| next-action 调用 | 2 次 | 2 次 (不变) | 0 |
| job_payment_mode_changed 字段来源 | 回溯会话历史 (有丢失风险) | get-agreed 读本地文件 (确定性) | 可靠性提升 |

### 3.2 切换 provider 场景（paymentMode 已是 escrow）

| 维度 | 优化前 | 优化后 | 节省 |
|---|---|---|---|
| LLM 轮次 | 2 轮 | **1 轮** | **1 轮** |
| CLI 调用 | 2 次 | 1 次 (ack-to-confirm) | 1 次 |
| next-action 调用 | 2 次 | **1 次** | 1 次 |
| 链上等待时间 | 5-30s | **0s** | **5-30s** |
| 系统事件等待 | 等 job_payment_mode_changed | **无需等待** | 1 次 |

### 3.3 Token 节省估算

| 场景 | 概率 | 节省 tokens | 加权节省 |
|---|---|---|---|
| 首次协商（paymentMode=0） | ~60% | ~10K (playbook 精简 + 可靠性) | 6K |
| 切换 provider（paymentMode=1） | ~30% | ~180K (消除 1 轮 RESUME 上下文) | 54K |
| 其他（x402 降级等） | ~10% | ~10K | 1K |
| **加权平均** | | | **~61K/task** |

## 4. 风险评估

| 风险 | 等级 | 缓解措施 |
|---|---|---|
| save-agreed 失败 → set-payment-mode 不该执行 | 不存在 | 内部严格先 save-agreed 后 set-payment-mode；save-agreed 失败直接 bail |
| save-agreed 成功 → set-payment-mode 失败 | 低 | save-agreed 文件幂等可重写；LLM 收到错误后按 exception-escalation §2 推送用户决策卡，用户可重试 |
| alreadySet 误判（竞态） | 极低 | CLI 内部用同一个 task_resp 做判断，无网络延迟窗口 |
| LLM 忽略 `confirmNow` 分支 | 低 | playbook 使用 MANDATORY 标记 + designated.rs 已验证的同模式 |
| confirm 字段与 save-agreed 不一致 | 不存在 | `ack-to-confirm` 内部生成 confirmContent，字段来源唯一 |
| provider 在 set-payment-mode 链上确认前收到 [intent:confirm] | 不存在 | `confirmNow=true` 仅在链上 paymentMode **已经是** escrow 时触发 |

## 5. Part 1 实施计划

### Phase 1: CLI 改动（~0.5 天）

- [ ] 1.1 在 `accept.rs` 新增 `handle_ack_to_confirm()` 函数
  - 复用 `save_agreed()` 逻辑
  - 复用 `handle_set_payment_mode()` 的 task_resp 查询 + alreadySet 判断
  - `confirmNow=true` 时生成 confirmContent 和 userNotifyContent
  - `confirmNow=false` 时调用 set-payment-mode 现有逻辑
- [ ] 1.2 在 `accept.rs` 新增 `handle_get_agreed()` 函数（读本地文件，~40 行）
- [ ] 1.3 在 `mod.rs` 注册 `AckToConfirm` + `GetAgreed` clap variant

### Phase 2: Playbook 改动（~0.5 天）

- [ ] 2.1 修改 `events.rs::negotiate_ack()`: Step 2/3 → ack-to-confirm + confirmNow 双分支
- [ ] 2.2 修改 `events.rs::job_payment_mode_changed()` escrow 路径: get-agreed 替代"回溯会话历史"

### Phase 3: 测试验证（~0.5 天）

- [ ] 3.1 单元测试: ack-to-confirm 在 paymentMode=0/1/3 三种状态下的行为
- [ ] 3.2 单元测试: get-agreed 读取 / 文件不存在 / 文件损坏
- [ ] 3.3 集成测试: 首次协商 → negotiate_ack → set-payment-mode → event → confirm
- [ ] 3.4 集成测试: 切换 provider → 二次协商 → alreadySet → 同 turn confirm
- [ ] 3.5 回归测试: designated x402 路径不受影响

### Phase 4: Skill 文件同步（~0.5 天）

- [ ] 4.1 更新 cli-reference 新增 ack-to-confirm + get-agreed 命令文档
- [ ] 4.2 更新 buyer.md §3.5 #3 `[intent:ack]` routing 指引
- [ ] 4.3 更新 buyer.md §3.4 Key prohibitions，移除与新流程冲突的约束

---

# Part 2: Skill 文件组织优化

> 聚焦 `_shared/` 和 `references/` 下 14 个文件（2841 行）在 buyer sub session 中的 token 消耗。独立于 Part 1，可并行实施。

## 6. 现状：buyer sub 的文件需求分析

### 6.1 `_shared/` (9 个文件，1916 行)

| 文件 | 行数 | buyer sub 需要? | 说明 |
|---|---|---|---|
| cli-reference.md | 824 | 部分 | buyer 命令 ~400 行；SKILL.md 已指示 grep，但 LLM 可能全读 |
| message-types.md | 341 | 部分 | §1/§2 信封格式 ~130 行有用；§3.1+ USER_DECISION_REQUEST 反模式 ~160 行纯 user session |
| state-machine.md | 175 | 偶尔 | 查状态映射时参考；flow.rs event routing 已内化核心逻辑 |
| xmtp-tools.md | 154 | 部分 | Path 6/8/9 约 100 行有用；Path 5/7 分别是 terminal cleanup 和 provider-only |
| user-intent-routing.md | 123 | **否** | 纯 user session 路由逻辑，sub session 永远不会用 |
| exception-escalation.md | 100 | **是** | flow.rs preamble 直接引用；4 条规则全都需要 |
| entry-points.md | 85 | 低 | 仅 job_created 首次有参考价值 |
| payment-modes.md | 65 | 低 | flow.rs 已内化 escrow/x402 分支 |
| preflight.md | 49 | **否** | user session 首次激活时跑，sub 不用 |

### 6.2 `references/` (5 个文件，925 行)

| 文件 | 行数 | buyer sub 需要? | 说明 |
|---|---|---|---|
| display-formats.md | 324 | **否** | 渲染模板全部是 user session 用的（task list / detail card / decision prompt） |
| incidents.md | 213 | 部分 | 21 个事故中 buyer 相关 ~8 个约 100 行 |
| evaluator-staking.md | 180 | **否** | evaluator 专用 |
| troubleshooting.md | 125 | 部分 | §1-4 共享约 100 行；§5 evaluator 专用 |
| evaluator-decision-rubric.md | 83 | **否** | evaluator 专用 |

### 6.3 误读风险汇总

buyer sub 永远不需要的文件总计 **~919 行**：

| 文件 | 行数 | 原因 |
|---|---|---|
| display-formats.md | 324 | 纯 user session |
| evaluator-staking.md | 180 | evaluator 专用 |
| message-types.md §3.1+ | 160 | 纯 user session（USER_DECISION_REQUEST 反模式防范） |
| user-intent-routing.md | 123 | 纯 user session |
| evaluator-decision-rubric.md | 83 | evaluator 专用 |
| preflight.md | 49 | user session 首次激活 |

SKILL.md Additional Resources 列出全部文件但未标注 session scope，LLM 不确定时可能全量读取。按 RESUME 倍增效应（10 次 RESUME），单次误读的最坏 context 成本 = 行数 × 10。

## 7. 优化方案（5 项）

### 7.1 [S2] SKILL.md Additional Resources 增加 scope 标注

**优先级**: P0 — 零风险、最小改动、立即可做

在每个条目后追加粗体 scope 标注：

```markdown
**`_shared/`**:
- [`cli-reference.md`](...) — CLI argument table (**all sessions, grep only**)
- [`state-machine.md`](...) — 37 events + 8 statuses (**sub: on demand**)
- [`payment-modes.md`](...) — escrow / x402 (**sub: on demand**)
- [`entry-points.md`](...) — task entry types (**sub: on demand**)
- [`exception-escalation.md`](...) — shared exception rules (**sub: preamble-guided**)
- [`preflight.md`](...) — wallet + agent pre-flight (**user session only**)
- [`message-types.md`](...) — XMTP envelope shapes (**sub: §1-§2 only; §3+ user session only**)
- [`user-intent-routing.md`](...) — user session free-form routing (**user session only**)
- [`xmtp-tools.md`](...) — XMTP tool invocations (**sub: Path 6/8/9 only**)

**`references/`**:
- [`display-formats.md`](...) — display templates (**user session only**)
- [`evaluator-decision-rubric.md`](...) — decision methodology (**evaluator only**)
- [`evaluator-staking.md`](...) — staking flow (**evaluator only**)
- [`troubleshooting.md`](...) — error codes (**all roles, on error**)
- [`incidents.md`](...) — incident case studies (**all roles, grep by [buyer]/[provider] tag**)
```

**改动量**: SKILL.md 改 14 行
**收益**: LLM 扫描列表时直接跳过标注 "user session only" / "evaluator only" 的文件

### 7.2 [S3] message-types.md §3.1 标注 user session only

**优先级**: P1

在 §3.1 开头加 3 行标注：

```markdown
> **⚠️ User session only** — sub sessions NEVER handle `[USER_DECISION_REQUEST]`
> (sub sessions produce them via `pending-decisions-v2 request`; user sessions consume them).
> Sub sessions reading this file: **skip from here to §4.**
```

**收益**: 防止 sub session 读入 160 行（含 §3.1.1 反面示例、§3.2 relay 协议等）

### 7.3 [S1] cli-reference.md 按角色拆分

**优先级**: P1 — 最大单文件，物理拆分保证合规

拆分为 4 个文件：

| 新文件 | 内容 | 行数 |
|---|---|---|
| `cli-reference-common.md` | common context / task-search / pending-decisions-v2 / next-action / list-attachments / active-tasks | ~160 |
| `cli-reference-buyer.md` | create-task ~ task-attach + draft 全系列 | ~400 |
| `cli-reference-provider.md` | find-jobs ~ provider-claim-rewards + dispute raise/confirm | ~180 |
| `cli-reference-evaluator.md` | evidence-info ~ my-stake + feedback-submit + file-* + heartbeat | ~160 |

**改动**:
- 拆 `_shared/cli-reference.md` → 4 文件
- 更新 SKILL.md reading order §3: "读 cli-reference-common.md + cli-reference-{role}.md"
- 更新 SKILL.md Additional Resources 引用

**收益**: buyer sub 最坏情况从读 824 行降至 560 行（common + buyer），节省 264 行
**风险**: 低 — 纯文档拆分；维护 4 文件但变更频率低

### 7.4 [S4] incidents.md 按角色标签标注

**优先级**: P2

每个 incident 标题追加角色标签：

```markdown
## I-1 — ASP skipped `next-action` [provider]
## I-3 — Backup self-queried task history [buyer]
## I-9 — User typed "关闭" → cancel instead of resolve [user-session]
## I-19 — Same-wallet multi-role collision [buyer] [provider] [evaluator]
```

标签映射：

| 标签 | Incidents |
|---|---|
| `[provider]` | I-1, I-2, I-11, I-12, I-13, I-16, I-17, I-20, I-21 |
| `[buyer]` | I-3, I-5, I-6, I-7, I-8, I-10, I-14, I-18 |
| `[user-session]` | I-9, I-15 |
| `[all]` | I-4 (envelope routing miss), I-19 (multi-role collision) |

**收益**: LLM 可按角色过滤，buyer sub 只需读 ~100 行
**改动量**: 21 行标题修改

### 7.5 [S5] evaluator 专用文件移至子目录

**优先级**: P3

```
references/
  evaluator/
    decision-rubric.md     (原 evaluator-decision-rubric.md)
    staking.md             (原 evaluator-staking.md)
  display-formats.md
  incidents.md
  troubleshooting.md
```

**改动**: 移 2 文件 + 更新 SKILL.md / evaluator.md 中的引用路径
**收益**: 物理隔离消除 263 行误读风险

## 8. Part 2 实施计划

### Phase A: 零风险标注（~0.5 天，可与 Part 1 并行）

- [ ] A.1 SKILL.md Additional Resources 增加 scope 标注 [S2]
- [ ] A.2 message-types.md §3.1 增加 "user session only" 标注 [S3]
- [ ] A.3 incidents.md 21 个事故标题增加角色标签 [S4]

### Phase B: 文件组织调整（~0.5 天，Phase A 之后）

- [ ] B.1 cli-reference.md 拆分为 4 个角色文件 [S1]
- [ ] B.2 更新 SKILL.md reading order §3
- [ ] B.3 evaluator 专用文件移至 `references/evaluator/` [S5]
- [ ] B.4 更新所有引用路径

### Phase C: 验证

- [ ] C.1 全文 grep 确认无断链引用
- [ ] C.2 buyer sub session 冒烟测试
- [ ] C.3 evaluator 流程冒烟测试

---

# 总体实施顺序与关系

## 9. Part 1 与其他优化项的关系

Part 1 是上一轮分析 **1.2 + 1.3** 的落地实施：

| 优化项 | 关系 | 可并行 |
|---|---|---|
| 1.1 终态事件自动处理 | 独立 | 是 |
| 1.4 confirm-accept 自动化 | 依赖 Part 1 的 save-agreed 持久化 | 否，Part 1 先行 |
| 1.5 入站消息路由下沉 | 独立 | 是 |
| 1.6 LOCALIZATION_PREFIX 去重 | 独立 | 是 |
| 1.7 Preamble 压缩 | 独立 | 是 |
| 1.8 user_decision 路由下沉 | 独立 | 是 |
| Part 2 Skill 文件组织优化 | 独立 | 是 |

## 10. 合并实施顺序

```
                 ┌─ Part 1 Phase 1-2 (CLI + Playbook: ack-to-confirm / get-agreed)
    并行启动 ─┤
                 └─ Part 2 Phase A   (零风险标注: S2 + S3 + S4)
                           ↓
                 ┌─ Part 1 Phase 3   (测试验证)
    并行执行 ─┤
                 └─ Part 2 Phase B   (文件拆分: S1 + S5)
                           ↓
                   Part 1 Phase 4 + Part 2 Phase C (Skill 同步 + 全量验证)
```

**建议后续顺序**: → 1.6(LOCALIZATION去重) → 1.7(Preamble压缩) → 1.5(inbound-route) → 1.1(终态自动) → 1.4(confirm-accept自动)

---

## 附录：代码位置索引

| 文件 | 行号 | 内容 |
|---|---|---|
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 94-241 | set-payment-mode 完整实现 (含 alreadySet) |
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 134-140 | alreadySet 检测逻辑 |
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 159-200 | alreadySet 分支处理 |
| `cli/src/commands/agent_commerce/task/buyer/accept.rs` | 224-231 | escrow alreadySet 输出 |
| `cli/src/commands/agent_commerce/task/buyer/negotiate.rs` | 168-227 | save-agreed 实现 |
| `cli/src/commands/agent_commerce/task/buyer/create.rs` | 236 | create-task paymentMode=0 |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/events.rs` | 190-225 | negotiate_ack playbook (待修改) |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/events.rs` | 31-116 | job_payment_mode_changed playbook (待修改) |
| `cli/src/commands/agent_commerce/task/buyer/flow_negotiate/designated.rs` | 256-258 | alreadySet 已有适配 (参考) |
| `cli/src/commands/agent_commerce/mod.rs` | 153-154 | SetPaymentMode clap 注册 (参考) |
| `skills/okx-agent-task/SKILL.md` | 386-404 | Additional Resources (待标注 scope) |
| `skills/okx-agent-task/_shared/cli-reference.md` | 1-824 | 待拆分为 4 个角色文件 |
| `skills/okx-agent-task/_shared/message-types.md` | 135-295 | §3.1+ 待标注 user session only |
| `skills/okx-agent-task/references/incidents.md` | 1-213 | 待增加角色标签 |
