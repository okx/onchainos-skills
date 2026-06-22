# Sub Session Output 精简优化方案

> 目标：Sub session 每轮 output tokens 从 ~1.1K 降到 ~400-600，总 output 从 ~46K → ~25K/task
> 基于当前代码分析（2026-06-13，分支 feat/agent-commerce-faster-lydia）

---

## 一、现状分析

### 1.1 Sub session output 构成

一个标准 buyer escrow 任务（15~20 轮 LLM），sub session 每轮 output 构成：

```
┌─────────────────────────────────────────────────────────┐
│ ① 流程说明文字        ~200-400 token                    │  ← "我收到了 provider 的报价，让我..."
│ ② 推理/分析过程        ~200-500 token                    │  ← "根据 budget=0.1, max_budget=0.15..."
│ ③ 工具调用参数          ~150-300 token                    │  ← session_status / xmtp_send / next-action
│ ④ 后续状态描述          ~100-200 token                    │  ← "等待 provider 回复 [intent:ack]..."
├─────────────────────────────────────────────────────────┤
│ 单轮总输出             ~650-1400 token（均值 ~1100）      │
└─────────────────────────────────────────────────────────┘
```

### 1.2 冗余来源

| 冗余类型 | 占比 | 示例 | 可消除？ |
|----------|------|------|---------|
| ① 流程说明（narration） | ~30% | "我将执行 next-action 命令来获取下一步操作" | ✅ 完全可消 |
| ② 重复性推理 | ~25% | 每轮都重新分析 budget/max_budget/paymentMode | ✅ 大部分可压缩 |
| ③ 工具调用参数 | ~20% | 必要输出，不可压缩 | ❌ |
| ④ 状态描述 | ~15% | "任务当前处于 created 状态，等待..." | ✅ 可消除 |
| ⑤ 本地化翻译推理 | ~10% | "用户使用中文，我需要将模板翻译..." | ✅ 可压缩 |

**核心发现**：①④ 是 Rule 9 早已禁止的行为（sub 直出文字对用户不可见），但模型仍习惯性生成。Rule 15（zero-narration）已有规则但执行率不足。

### 1.3 各阶段 output 分布

| 阶段 | 轮次 | 当前 output/轮 | 冗余占比 | 优化后 output/轮 |
|------|------|---------------|---------|----------------|
| job_created（备份 → 推荐） | 2-3 | ~1.5K | ~60% | ~600 |
| 协商阶段（negotiate_reply × 3-5） | 3-5 | ~1.2K | ~50% | ~600 |
| ack-to-confirm | 1 | ~1.0K | ~40% | ~600 |
| provider_applied → confirm-accept | 1 | ~0.8K | ~30% | ~550 |
| job_accepted → dispatch_user | 1 | ~0.9K | ~40% | ~500 |
| job_submitted → 审核卡 | 1-2 | ~1.3K | ~50% | ~650 |
| approve/reject → complete/reject | 1-2 | ~1.0K | ~40% | ~600 |
| 终态（completed/refunded/closed） | 1 | ~0.8K | ~30% | ~550 |
| **合计** | **15-20** | **~21K** | **~45%** | **~11K** |

---

## 二、优化方案

### 方案 A：Skill 文件添加 `output_mode: terse` 指令（ROI 最高）

**原理**：在 SKILL.md 和 buyer.md/provider.md 中添加 sub session 专属的输出约束指令，让模型在 sub session 中强制精简输出。

**当前现状**：
- Rule 15（zero-narration）已存在于 preamble 中：`EVERY response MUST contain ≥1 tool_use. No text-only responses.`
- Rule 9（sub 不可见）已存在
- 但两者都是"禁止型"约束，没有给出"应该怎样输出"的正面模板

**改进**：从"禁止冗余"升级为"规定格式"。

#### A-1. SKILL.md `Sub-session agent state machine` 段落新增输出格式约束

```markdown
### Sub-session output format

🛑 **Terse mode** — sub session 的 LLM output 仅用于驱动工具调用，不产生用户可见文本。

每轮 output 的**全部**内容应当且仅当包含：
1. **Tool calls** — `session_status` / `xmtp_send` / `onchainos agent ...` / `xmtp_dispatch_user` 等
2. **最小决策推理**（仅当需要分支判断时）— 单句，≤30 token，格式：`// quote=0.1 ≤ budget=0.15 → auto-propose`

**禁止输出**：
- ❌ 流程说明："我收到了..." / "让我调用..." / "接下来我将..."
- ❌ 状态复述："任务当前处于 created 状态"
- ❌ 翻译推理："用户使用中文，我需要翻译..."
- ❌ 等待描述："等待 provider 回复 [intent:ack]"
- ❌ 规则引用："根据 Rule 12..."
- ❌ 计划陈述："Step 1 做什么，Step 2 做什么"

**正例**（negotiate_reply，对方报价 0.1 USDG）：
```
// 0.1 ≤ budget 0.15 → propose
[tool_use: xmtp_send → content with [intent:propose]]
```

**反例**（同场景，当前模型常见输出）：
```
我收到了 provider 的报价。让我先分析一下：
- Provider 报价：0.1 USDG
- 我的预算范围：0.1-0.15 USDG
- 报价在预算范围内，可以接受

根据协商流程，我需要发送 [intent:propose] 给 provider。
让我先获取 sessionKey，然后发送消息。

[tool_use: session_status]
...接下来我将发送 propose 消息...
[tool_use: xmtp_send]

消息已发送，等待 provider 回复 [intent:ack]。
```
```

**预计收益**：每轮省 ~400-600 token output → 15-20 轮 × 500 avg = **~7.5K-10K output tokens/task**

**风险**：模型可能因指令过于严格而在需要推理的场景（如报价评估、x402 分支判断）省略关键推理，导致错误决策。

**缓解**：对需要推理的场景（negotiate_reply、user_decision_*）显式允许 ≤50 token 的决策推理行。

#### A-2. Preamble 中的 Rule 15 增强 ✅ 已完成

当前 Rule 15：
```
⚡ **Zero-narration rule**: EVERY response MUST contain at least one tool_use block.
Do NOT produce text-only or empty responses.
```

**增强为**：
```
⚡ **Zero-narration rule**: EVERY response MUST contain ≥1 tool_use block AND ≤2 lines of non-tool text.
- ✅ Allowed: `// decision: X` (single-line reasoning anchor, ≤30 tokens)
- ❌ Forbidden: narrating what you are about to do, recapping state, explaining rules, describing wait conditions
- The tool call IS the action; no surrounding prose is needed.
```

**改动位置**：
- `buyer/flow.rs` L286：`context_preamble` 中 Rule 15 的文本
- `buyer/flow.rs` L304：`preamble_medium` 中 Rule 15
- `buyer/flow.rs` L319：`preamble_negotiate` 中 Rule 15
- `buyer/flow.rs` L331：`preamble_slim` 中 Rule 15
- `provider/flow.rs` 对应 preamble 段落

**预计收益**：与 A-1 联合作用，总 output 减少 ~30%

---

### 方案 B：模板化场景使用预定义响应模板（中等 ROI） ✅ 已完成

**原理**：对 CLI 操作完全确定的场景，在 playbook 中直接给出"你应输出的全部内容"，消除推理空间。

#### B-1. 识别模板化场景

以下场景的 sub session output 100% 确定性，不需要 LLM 推理：

| 场景 | 当前 output | 最优 output | 节省 |
|------|------------|------------|------|
| provider_applied → xmtp_send [intent:applied] | ~800 token | 2 tool calls (~200 token) | ~600 |
| job_submitted → observe only | ~500 token | 0 (end turn) | ~500 |
| job_completed → dispatch_user + cleanup | ~800 token | 2 tool calls (~250 token) | ~550 |
| job_refunded → dispatch_user + cleanup | ~700 token | 2 tool calls (~200 token) | ~500 |
| job_expired / job_closed → dispatch_user | ~600 token | 1 tool call (~150 token) | ~450 |
| attachment_added → upload + send | ~900 token | 3 tool calls (~300 token) | ~600 |

#### B-2. Playbook 添加 `[OUTPUT_TEMPLATE]` 标记

在 flow.rs 的 playbook body 中，对确定性场景添加显式输出模板：

```rust
// 当前 provider_applied playbook 结尾：
// "After xmtp_send returns → end this turn immediately..."

// 优化后 — 添加输出约束：
"[OUTPUT_TEMPLATE]\n\
Your entire response for this event should be:\n\
1. `session_status` tool call\n\
2. `xmtp_send` tool call with the content above\n\
No other text output. End turn after tool calls complete.\n"
```

**改动文件**：
- `provider/flow.rs`：`ProviderApplied`、`JobSubmitted`、`JobCompleted`、`JobRefunded` 等 6+ 事件
- `buyer/flow_lifecycle/terminal.rs`：`job_completed`、`job_refunded`、`job_expired`、`job_closed`、`job_auto_refunded`、`job_auto_completed` 等 6+ 事件
- `buyer/flow_lifecycle/core.rs`：`provider_applied`、`deliverable_received`

**预计收益**：12+ 个确定性事件 × ~500 token avg = **~6K output tokens/task**

**风险**：低。这些场景本身就不需要推理，模板只是显式化了已有行为。

---

### 方案 C：CLI `next-action` 添加 `--terse` 标志（中等 ROI，工程量中）

**原理**：CLI 在 `--terse` 模式下输出更紧凑的 playbook，省略冗余的说明文字、重复的格式化模板、和"Follow-up events"等信息性段落。

#### C-1. `--terse` 模式精简内容

| 内容 | 正常模式 | `--terse` 模式 | 节省 |
|------|---------|--------------|------|
| `[Follow-up events]` 段落 | ~100-200 token | 删除 | ~150 |
| `⚠️` 警告重述（已在 preamble 中） | ~50-150 token | 删除 | ~100 |
| xmtp_send 格式说明（重复） | ~100-200 token | 简化为单行 ref | ~150 |
| `[Current state]` + `[Role]` 头 | ~30-50 token | 合并为 `[S:job_accepted R:buyer]` | ~30 |
| 步骤编号说明 | ~50-100 token | 仅保留命令 | ~75 |
| **单次 playbook 平均** | | | **~500 token** |

#### C-2. 实现方式

```rust
// cli/src/commands/agent_commerce/task/mod.rs (NextAction subcommand)
#[arg(long, help = "Terse output mode for sub sessions")]
terse: bool,

// buyer/flow.rs
pub fn generate_next_action(
    job_id: &str, event_str: &str, agent_id: &str,
    job_title: Option<&str>, data: Option<&str>,
    payment_mode: Option<i64>,
    prefetched: Option<&PreFetchedTaskContext>,
    terse: bool,   // 新增参数
) -> String {
    // ...
    let follow_up = if terse { "" } else { follow_up_events };
    let warnings = if terse { "" } else { warnings_block };
    // ...
}
```

**改动量**：
- `cli/src/commands/agent_commerce/task/mod.rs`：clap 参数新增 ~5 行
- `buyer/flow.rs`：`generate_next_action` 签名 + terse 条件分支 ~20 行
- `buyer/flow_negotiate/*.rs`：各 playbook 函数添加 terse 分支 ~60 行
- `buyer/flow_lifecycle/*.rs`：同上 ~80 行
- `provider/flow.rs`：同上 ~50 行

总改动量：~215 行 Rust

**预计收益**：15-20 轮 × 500 token avg = **~7.5K-10K input tokens/task**（注意：这省的是 playbook 的 _input_ tokens，因为 playbook 是 CLI 输出、作为 LLM input 的）

**风险**：
- `--terse` 模式下省略的信息可能恰好是模型需要的提示（如 Follow-up events 帮助模型理解后续预期）
- 需要逐事件验证 terse 输出不影响正确率

---

### 方案 D：Preamble 进一步分级 — 新增 `preamble_micro`（低 ROI，低风险） ✅ 已完成

**原理**：当前 4 档 preamble（full ~2800 / medium ~800 / negotiate ~900 / slim ~500 token），对终态/observe-only/确定性事件仍使用 slim（~500 token）。这些事件其实只需 Rule 9 + Rule 15 = ~100 token。

#### D-1. 新增 `preamble_micro`

```rust
let preamble_micro = "\
    🛑 **Core**: (1) Sub output invisible to user — push via `xmtp_dispatch_user` / `pending-decisions-v2 request` only. (2) No narration — tool calls only. (3) Follow playbook literally.\n\n";
```

~60 token，用于：
- `job_submitted`（observer-only）
- `job_completed` / `job_refunded` / `job_auto_refunded` / `job_expired` / `job_closed`（终态通知）
- `submit_expired` / `reject_expired` / `review_expired` / `job_auto_completed`（超时通知）
- `reward_claimed` / `wakeup_notify`
- `task_token_budget_change` / `task_provider_change`（变更通知）
- `staked` / `unstake_*` / `dispute_approved`（buyer 不处理的事件）

#### D-2. 修改 preamble 选择逻辑

```rust
// buyer/flow.rs L539-561
let use_micro_preamble = matches!(event_str,
    "job_completed" | "job_refunded" | "job_auto_refunded" | "job_expired" | "job_closed" |
    "submit_expired" | "reject_expired" | "review_expired" | "job_auto_completed" |
    "reward_claimed" | "wakeup_notify" |
    "task_token_budget_change" | "task_provider_change" |
    "staked" | "unstake_requested" | "unstake_claimed" | "unstake_cancelled" | "stake_stopped" | "dispute_approved"
);
let use_slim_preamble = matches!(event_str,
    "negotiate_ack" |
    "approve_review" | "reject_review" |
    "review_deadline_warn" | "submit_deadline_warn" |
    "close" | "set_public"
) || event_str.starts_with("user_decision_");
// ... micro → slim → negotiate → medium → full
```

**预计收益**：~18 个事件从 slim(500) 降到 micro(60) = **~440 token × 18 = ~7.9K input tokens/task**

**改动量**：~30 行 Rust

---

### 方案 E：翻译推理压缩 — 预置语言检测结果（低 ROI，实验性）

**原理**：当 localization_prefix 指示"非英语用户 → 翻译"时，模型经常花 100-200 token 推理用户语言 + 翻译策略。如果在 prefetched context 中预置 `userLanguage=zh-CN`，模型可以跳过检测推理。

#### E-1. 在 pre-fetched context 中添加 `userLanguage`

```rust
// common/util.rs — PreFetchedTaskContext
pub struct PreFetchedTaskContext {
    // ... existing fields
    pub user_language: Option<String>,  // 新增
}

impl PreFetchedTaskContext {
    pub fn format_inline(&self) -> String {
        let mut s = String::from("[Pre-fetched context]\n");
        // ... existing fields
        if let Some(lang) = &self.user_language {
            s.push_str(&format!("userLanguage: {lang}\n"));
        }
        s
    }
}
```

**语言检测来源**：
- CLI 可以从 `session_status` 返回的 session metadata 中读取
- 或从 pending-decisions-v2 的历史消息中推断
- 兜底：由 SKILL.md 加载时的 Claude Code 会话语言推断

**预计收益**：每轮省 ~50-100 token × 15-20 轮 = **~1K-2K output tokens/task**

**改动量**：~40 行 Rust

**风险**：中。语言检测不准确可能导致错误翻译。

---

## 三、执行计划

### Phase 1（Day 1）：高 ROI 零风险改动

| 编号 | 方案 | 改动 | 预计收益 | 风险 |
|------|------|------|---------|------|
| A-1 | SKILL.md 添加 terse output format | skill 文件 ~30 行 | ~7.5K output/task | 低 |
| A-2 | Preamble Rule 15 增强 ✅ | Rust ~20 行 | 与 A-1 联合 | 低 |
| D-1 | 新增 preamble_micro ✅ | Rust ~30 行 | ~7.9K input/task | 低 |

**Phase 1 合计收益**：~15K token/task（output 7.5K + input 7.9K）
**改动量**：Skill ~30 行 + Rust ~50 行

### Phase 2（Day 2）：中等 ROI 模板化改动

| 编号 | 方案 | 改动 | 预计收益 | 风险 |
|------|------|------|---------|------|
| B-2 | 确定性场景添加 OUTPUT_TEMPLATE ✅ | Rust ~100 行 | ~6K output/task | 低 |
| C-1 | --terse 模式 | Rust ~215 行 | ~7.5K input/task | 中 |

**Phase 2 合计收益**：~13.5K token/task
**改动量**：Rust ~315 行

### Phase 3（Day 3）：验证 + 实验性改动

| 编号 | 方案 | 改动 | 预计收益 | 风险 |
|------|------|------|---------|------|
| E-1 | userLanguage 预置 | Rust ~40 行 | ~1.5K output/task | 中 |
| — | A/B 测试框架搭建 | — | — | — |

### Phase 4（Day 4-5）：A/B 测试

对比指标：
1. **Output tokens/task**：目标从 ~46K → ~25K（-45%）
2. **任务成功率**：不低于当前基线（~92%）
3. **端到端耗时**：目标减少 ~20%（减少 LLM output 生成时间）
4. **关键事件正确率**：negotiate_reply / job_submitted review / x402 flow 的决策准确率

---

## 四、方案间关系

```
                    ┌──────────────────────────────────────────────────────┐
                    │                Token 流向                            │
                    │                                                      │
    CLI playbook ──→│  preamble (D: micro/slim/medium/full)                │
    (input)        │  + localization (A-2: 压缩重复引用)                    │
                    │  + body (C: --terse 压缩非关键段落)                    │
                    │  + prefetched context (E: 添加 userLanguage)          │
                    ├──────────────────────────────────────────────────────┤
    LLM output ───→│  推理文本 (A-1: terse mode 约束到 ≤2 行)              │
    (output)       │  + 工具调用 (不可压缩)                                │
                    │  + 状态描述 (A-1: 禁止)                              │
                    │  + 翻译推理 (E: 预置语言减少推理)                      │
                    │  + 确定性场景 (B: OUTPUT_TEMPLATE 消除推理)            │
                    └──────────────────────────────────────────────────────┘
```

**方案正交性**：
- A（output 约束）和 C/D（input 压缩）分别作用于不同方向，完全正交
- B（确定性模板）是 A 在特定场景的增强版，两者兼容
- E（语言预置）独立于其他方案

**联合收益估算**：
- Input 节省：D(7.9K) + C(7.5K) = ~15.4K input token/task
- Output 节省：A(7.5K) + B(6K) + E(1.5K) = ~15K output token/task
- **总计：~30K token/task 节省**（其中 output 从 ~46K → ~31K，再加上 input 压缩）

---

## 五、与已有优化的关系

| 已实施优化 | 本方案是否依赖 | 是否冲突 |
|-----------|-------------|---------|
| LOCALIZATION_PREFIX 去重（cf297db2） | 否 | 否 — l10n_emitted 逻辑与 terse 正交 |
| Preamble 降级（d88c74ee） | 是基础 | 否 — micro 是 slim 的进一步细分 |
| buyer.md 用户内容提取（本次 10efa05d） | 否 | 否 — 本次拆分的是 skill 文件，本方案优化的是 CLI 输出 + LLM 行为 |
| ack-to-confirm 合并（6c3250c0） | 否 | 否 — 减少轮次是另一维度的优化 |

---

## 六、关键代码位置索引

| 内容 | 文件 | 行号 |
|------|------|------|
| Buyer preamble 4 档定义 | `buyer/flow.rs` | L262-331 |
| Buyer preamble 选择逻辑 | `buyer/flow.rs` | L539-574 |
| Provider preamble 定义 | `provider/flow.rs` | L176-194 |
| Provider preamble 选择 | `provider/flow.rs` | L197-204 |
| Rule 15 (zero-narration) | `buyer/flow.rs` | L286 |
| LOCALIZATION_PREFIX | `buyer/flow.rs` | L21-29 |
| LOCALIZATION_PREFIX_SHORT | `buyer/flow.rs` | L31-32 |
| Sub-session state machine | `SKILL.md` | L251-263 |
| PreFetchedTaskContext | `common/util.rs` | (format_inline 方法) |
| Buyer content templates | `buyer/content.rs` | 全文 |
| Provider content templates | `provider/content.rs` | 全文 |
