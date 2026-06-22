# Agent Task 系统 Token 优化方案

> 目标：减少 token 消耗、减少 LLM 轮次、减少端到端耗时
> 基于当前代码分析（2026-06-12，分支 feat/agent-commerce-faster-service-match）

---

## 一、现状数据

### 1.1 Prompt 文档体量

| 文件 | 行数 | 估算 token |
|------|------|-----------|
| SKILL.md | 404 | ~6,000 |
| buyer.md | 366 | ~5,500 |
| provider.md | 262 | ~3,900 |
| buyer-actions.md | 290 | ~4,400 |
| _shared/cli-reference.md | 824 | ~12,400 |
| _shared/ 其余 8 文件 | 851 | ~12,800 |
| references/ 5 文件 | 925 | ~13,900 |
| **合计** | **4,204** | **~59,000** |

### 1.2 CLI playbook 输出体量

| 组件 | 字节 | 说明 |
|------|------|------|
| buyer/flow.rs + flow_lifecycle/ + flow_negotiate/ | 234K | 全量 playbook 输出逻辑 |
| provider/flow.rs | 108K | ASP 端 playbook 输出逻辑 |
| content.rs (buyer + provider) | 39K | 模板字符串 |
| **合计** | **~381K** | 约 95K token 的 Rust format! 字符串 |

### 1.3 单次 next-action 输出构成

```
┌──────────────────────────────────────────┐
│ [Localization] prefix         ~200 token │  ← 每次都发
│ [Version] prefix              ~30 token  │  ← 每次都发
│ IRON RULES preamble      800~2800 token  │  ← 每次都发（4 档）
│ [Pre-fetched context]     200~500 token  │  ← 有时缺失
│ Playbook body            500~3000 token  │  ← 事件特定
├──────────────────────────────────────────┤
│ 单次总输出               1700~6500 token │
└──────────────────────────────────────────┘
```

### 1.4 典型任务生命周期 Token 消耗

一个标准 buyer 指定服务商 + escrow 支付 + 一次交付 + 验收 的任务：

| 阶段 | LLM 轮次 | 输出 token | 输入 token |
|------|----------|-----------|-----------|
| SKILL.md 加载 + role 判断 | 1 | ~500 | ~6,000 |
| buyer.md 加载 | 1 | ~200 | ~5,500 |
| next-action job_created（全量 preamble） | 1 | ~4,000 | ~6,500 |
| common context（冗余获取） | 1 | ~300 | ~1,000 |
| recommend/service-match | 1 | ~500 | ~2,000 |
| pending-decisions-v2（推决策给用户） | 1 | ~600 | ~1,500 |
| user_decision → next-action designated | 1 | ~3,500 | ~6,000 |
| x402 校验 + set-payment-mode | 1-2 | ~1,000 | ~3,000 |
| confirm-accept | 1 | ~800 | ~2,000 |
| negotiate 阶段（3-5 轮） | 3-5 | ~6,000 | ~15,000 |
| job_submitted（交付通知） | 1 | ~2,000 | ~4,000 |
| 用户审批 → approve_review | 1-2 | ~1,500 | ~4,000 |
| job_completed（终态） | 1 | ~1,500 | ~3,500 |
| **合计** | **15~20** | **~22K** | **~60K** |
| **总计（输入+输出）** | | **~82K token / 任务** | |

---

## 二、优化方案（按 ROI 排序）

### P0：Prompt Cache 友好设计（工程改动小，收益立竿见影）

**问题**：每次 next-action 调用，LLM 都重新读取 system prompt + tool 列表 + SKILL.md + preamble。Claude 的 Prompt Cache TTL 5 分钟，相同前缀只计费 0.1x。但当前 preamble 嵌入了 `job_id`、`agent_id` 等变量，前缀在每个任务间不同，缓存命中为 0。

**方案**：

#### P0-1. Preamble 静态化 + 变量后置

```
当前结构（缓存不友好）:
  "[Localization] ... {job_id} ... IRON RULE ... {escalation_template_with_job_id} ... {body}"
  
优化结构（缓存友好）:
  "[STATIC PREAMBLE — 所有规则，无变量]"     ← 稳定前缀，可缓存
  +
  "[TASK CONTEXT] job_id={}, agent_id={}"   ← 变量区，附在尾部
  +
  "{body}"                                   ← 事件特定内容
```

**实现**：
- `buyer/flow.rs` 中 `context_preamble` / `preamble_medium` / `preamble_negotiate` / `preamble_slim` 的 `format!()` 宏中包含 `{escalation_protocol_misread}` / `{escalation_cli_failed}` / `{cli_failed_request_block}` 等内嵌 `job_id` 的模板
- 改为：将 escalation 模板从 preamble 中剥离，放在 `[TASK CONTEXT]` 段落中
- preamble 本体变成 `const` 静态字符串（当前 `preamble_slim` / `preamble_medium` 已接近这个形态，仅 `context_preamble` 和 `preamble_negotiate` 嵌入了变量）

**收益估算**：
- 当前 preamble (含变量) ~2800 token → 每次都是 1x input 计费
- 静态化后 ~2500 token 进入 prompt cache → 0.1x 计费 → **每次 next-action 省 ~2250 个 input token 的费用**
- 一个任务 15~20 次 next-action → **单任务省 ~33K~45K input token 费用**

**改动量**：~50 行 Rust 代码修改

---

#### P0-2. Preamble 分级精简（已有雏形，需强化）

**现状**：已有 4 档 preamble（`context_preamble` 2800 token / `preamble_medium` ~800 token / `preamble_negotiate` ~900 token / `preamble_slim` ~500 token），通过事件类型选择。

**问题**：
- `context_preamble`（2800 token 全量）仍用于 `job_created`（首次事件，合理）和一些未明确分类的事件（fallback）
- 很多事件其实只需要 `preamble_slim`（500 token），但当前 fallback 是全量 preamble
- 某些终态事件（`job_completed`, `dispute_resolved`）使用 `preamble_slim`，但其实这些事件只需 Rule 9（sub 不可见）+ Rule 7（无技术术语），连 Rule 0（步骤不可跳）都不关键

**方案**：
1. 新增 `preamble_minimal`（~200 token）: 仅 Rule 9 + Rule 7 + Rule 15，用于终态和简单通知事件
2. 将 fallback 从 `context_preamble` 降级为 `preamble_medium`
3. `context_preamble` 仅用于 `job_created`（第一次）+ 明确需要全量规则的异常恢复事件

**收益**：
- 15~20 次 next-action 中 ~10 次可从 medium/slim 降到 minimal → 省 ~3000~6000 token/任务

**改动量**：~30 行 Rust

---

### P1：确定性逻辑脚本化（最高优先级的结构性优化）

**核心原则**：凡是规则清晰、输入输出确定的逻辑，都不应该消耗 token 来"推理"。

#### P1-1. user-intent-routing 下沉到 CLI

**现状**：`_shared/user-intent-routing.md`（124 行）是一个决策树，告诉 LLM 如何将用户自由文本映射到任务操作（发布/修改/查询/取消...）。LLM 每次处理用户消息都要读取并推理。

**方案**：
- CLI 新增子命令 `onchainos agent intent-classify --text "用户说的话" --context "当前状态"`
- 内部使用关键词匹配 + 正则 + 简单 embedding 分类器
- 返回 `{intent: "publish_task", confidence: 0.95, fallback: "modify_budget"}`
- LLM 只在 confidence < 0.7 时才需要自行判断

**收益**：~1,860 token → ~300 token（省 ~1,560 token/会话）

**改动量**：新增 CLI 子命令 ~200 行 Rust

---

#### P1-2. user_decision 语义路由下沉到 CLI

**现状**：`buyer/flow.rs:427-506` 中，每个 `user_decision_*` 事件都输出一个 ~400 token 的路由表，告诉 LLM："如果用户说 yes/好的/通过 → event A；说 no/拒绝 → event B"。LLM 读表 → 推理 → 映射。

```
[User decision relay] source_event=`job_submitted`, user reply: `good job`

Two options:
  • `approve_review` — user accepts (A / 通过 / 同意 / ... / OK)
  • `reject_review` — user rejects (B / 拒绝 / 不通过 / ...)
```

**方案**：
- `next-action --event user_decision_job_submitted --data "good job"` 直接在 CLI 内部做语义分类
- CLI 返回已选定的路由 `{routed_event: "approve_review", confidence: 0.95}`
- 高置信度直接执行，低置信度才让 LLM 参与

**收益**：~400 token/决策 × 5~10 决策/任务 = **2,000~4,000 token/任务**

**改动量**：在现有 `user_decision_*` handler 中增加路由逻辑 ~150 行 Rust

---

#### P1-3. x402 校验逻辑完全内置

**现状**：designated.rs 中 x402 校验输出一个 5 分支决策树（~1000 token），LLM 依次判断 valid/input_required/price_mismatch/over_budget/pass，每一步都是确定性的。

**方案**：
- `onchainos agent x402-validate` 已经内部执行了全部校验逻辑
- 改为直接返回最终结论 + 下一步命令，而非决策树
- 返回格式：`{result: "pass", nextCommand: "onchainos agent set-payment-mode ...[参数已填好]"}`

**收益**：~1,000 token/校验，通常 1~2 次/任务 = **1,000~2,000 token/任务**

**改动量**：~80 行 Rust 修改

---

#### P1-4. preflight 检查脚本化

**现状**：`_shared/preflight.md`（50 行）描述了 4 步前置检查（钱包登录、agent 创建、通信就绪、余额检查），LLM 每次都要读取并逐步执行。

**方案**：
- CLI 新增 `onchainos agent preflight --agent-id <id>` 子命令
- 内部依次检查 login → agent exists → xmtp ready → balance
- 返回 `{pass: true}` 或 `{pass: false, failStep: "wallet_login", instruction: "请先执行 wallet login"}`

**收益**：~750 token → ~50 token = **省 ~700 token/会话**

**改动量**：~120 行 Rust

---

### P2：减少 Agent 协商轮次（架构层改动）

#### P2-1. Playbook 合并连续步骤

**现状**：很多 playbook 拆成了顺序独立的步骤，每步 = 1 个 LLM 轮次：

```
轮次 1: session_status → 获取 sessionKey
轮次 2: xmtp_send（发消息给对方）
轮次 3: xmtp_dispatch_user（通知用户）
```

**方案**：在 playbook 输出中显式标注可并行的步骤：

```
[Parallel Steps]
  Step A: xmtp_send(sessionKey=<从session_status获取>, content="...")
  Step B: xmtp_dispatch_user(content="...")
[Note] Step A and B can be called in the same LLM response.
```

或更进一步：将 `session_status` 的结果预置到 playbook 中（CLI 在生成 playbook 时先查询）。

**收益**：
- 每个可合并的步骤对省 1 轮 × ~1500 token（context 重传）
- 每任务约 5~8 个可合并对 = **7,500~12,000 token/任务 + 5~8 轮延迟**

**改动量**：中等，需要 CLI 在生成 playbook 时执行预查询（~200 行 Rust）

---

#### P2-2. 协商阶段自动驾驶

**现状**：buyer.md §3.5 定义了协商自治区（status=0 时 sub 自动处理 provider 消息），但每轮协商仍需：
1. 收到 provider 消息 → 触发 next-action
2. LLM 读 preamble + playbook
3. LLM 调 common context 获取 budget
4. LLM 评估报价
5. LLM 发 xmtp_send 回复

**方案**：CLI 实现协商自动驾驶模式：
- `onchainos agent negotiate-auto --job-id <id> --agent-id <id> --incoming-message "provider 的消息"`
- CLI 内部：解析 intent marker → 对比 budget → 生成回复 → 调用 xmtp_send → 返回结果
- 仅当需要 LLM 判断（模糊报价、技能评估、复杂条款）时才返回 `{needLLM: true, context: "..."}`

**收益**：
- 协商阶段 3~5 轮 × ~3,000 token/轮 = **9,000~15,000 token/任务**
- 轮次减少 3~5 轮 = **3~5 次 API 调用延迟**

**改动量**：大，新增 CLI 子命令 + 移植协商逻辑 ~500 行 Rust

---

#### P2-3. service-match + designated 路由合并（部分已实现）

**现状**：commit 38894f48 已将双 subprocess 替换为 `/asp/service/match` API。但目前 service-match 返回后，LLM 仍需要再调用 `next-action --event designated_*` 来获取下一步 playbook。

**方案**：
- service-match 成功时直接返回 designated 路由的 playbook（而非仅返回匹配结果让 LLM 再调一次）
- 省掉 LLM 的一个 "读结果 → 决定调 next-action → 再读 playbook" 轮次

**收益**：省 1 轮 × ~3,000 token = **~3,000 token/任务**

**改动量**：~100 行 Rust 修改

---

### P3：输出压缩（注意保留推理链）

#### P3-1. 消除 preamble 中的重复规则

**现状**：通过对 flow.rs 的分析，以下内容在 buyer/provider 的 preamble 中**逐字重复**：

| 重复内容 | buyer 中 | provider 中 | token 浪费 |
|---------|---------|------------|-----------|
| IRON RULE 0 | 4 个 preamble 变体各 1 次 | context_preamble 1 次 | ~200×5 |
| Rule 7（无技术术语） | ~120 token × 4 变体 | ~120 token × 1 | ~600 |
| Rule 9（sub 不可见） | ~100 token × 4 变体 | ~100 token × 1 | ~500 |
| Rule 15（zero-narration） | ~50 token × 4 变体 | ~50 token × 1 | ~250 |
| Localization prefix | buyer: 9 行 const | provider: inline 8 行 | ~400 |

**方案**：
- 将重复规则提取为编号引用：buyer/provider 的 preamble 引用 SKILL.md 中的规则编号
- 例如：`"遵循 SKILL.md IRON RULES #0 #7 #9 #15"` 替代 ~400 token 的全文重复
- 前提：SKILL.md 已在 session 开始时加载到 context 中

**收益**：
- 每次 next-action 省 ~200~500 token（取决于 preamble 级别）
- 15~20 次 × ~300 avg = **~4,500~6,000 token/任务**

**风险**：模型可能因为没有"看到"完整规则而违反。需要验证模型是否能可靠引用已加载的 SKILL.md 规则。

**缓解**：保留关键安全规则（Rule 9 sub 不可见、Rule 11 不自动审批）的内联，仅压缩低风险规则的重述。

**改动量**：~100 行 Rust 字符串修改

---

#### P3-2. Localization prefix 去重

**现状**：
- `buyer/flow.rs:20-28` 定义了 `LOCALIZATION_PREFIX` const（~200 token），**每次 next-action 都附加**
- 另有 `L10N_DISPATCH_SHORT` / `L10N_PROMPT` / `L10N_PROMPT_BOLD` 3 个 const
- provider 侧有近乎相同的内联版本（`provider/flow.rs:79-93`）

**方案**：
- 首次 next-action（job_created）输出完整 Localization prefix
- 后续 next-action 输出单行引用：`"[Localization] 规则同首次输出，未变更。"`
- 在 CLI 中记录 `is_first_event` flag（基于 session state 或 event history）

**收益**：~200 token × 14~19 次 = **~2,800~3,800 token/任务**

**改动量**：~30 行 Rust

---

#### P3-3. 压缩模板但保留结构化字段

**原则**（来自图片分析）：
- **可压缩**：冗余重述、格式化包装、礼貌性语言
- **不可压缩**：推理链、关键判断依据、结构化输出

**实施**：
- content.rs 中的通知模板（`job_accepted_user_notify`, `deliverable_received_notify` 等）：去掉解释性文字，保留结构化数据
- 例如将 `"恭喜！任务 <title> 已完成。服务商 Agent #<id> 的交付成果已通过您的验收，资金已自动释放给服务商。任务编号：<jobId>"` 
- 压缩为 `"✅ 任务完成 | <title> | 服务商 #<id> | 资金已释放 | 编号 <jobId>"`

**收益**：~100~200 token/模板 × 8~12 模板/任务 = **~800~2,400 token/任务**

**风险**：用户体验下降。需要 A/B 测试确认压缩后用户理解无障碍。

---

### P4：减少重试消耗（最容易被低估）

#### P4-1. Playbook 输出增加 JSON Schema 强约束

**现状**：playbook 用自然语言描述期望的 LLM 行为（"调用 xmtp_send，参数 sessionKey=...，content=..."）。LLM 偶尔参数格式错误 → 工具调用失败 → 错误信息回填 context → 重试 → 消耗 4x token。

**方案**：
- 为关键工具调用提供 JSON Schema 示例
- 例如 playbook 中：
  ```
  Call tool: xmtp_send
  Args schema: {"sessionKey": "<从 session_status 获取>", "content": "<翻译后的模板>"}
  ```
- 减少参数拼写错误 / 格式错误导致的重试

**收益**：重试率从 ~10% 降到 ~2% → 每次重试 ~3000 token → 15~20 次调用中 1~2 次重试 → **省 ~3,000~6,000 token/任务**

**改动量**：~200 行 Rust（为每个工具调用添加 schema hint）

---

#### P4-2. 前置校验（早失败比晚失败便宜）

**现状**：某些 CLI 命令在参数不合法时才报错（如 `confirm-accept` 余额不足、`deliver` 状态不对），错误信息回填 context 后 LLM 需要理解错误 → 推给用户 → 再重试。

**方案**：
- CLI 在 playbook 输出阶段就校验前置条件
- 例如生成 `confirm-accept` playbook 时先检查余额 → 余额不足直接在 playbook 中输出 "余额不足，需先充值" → LLM 只需要 dispatch_user 通知用户
- 避免 LLM 执行 → 失败 → 理解错误 → 通知用户 的 3 轮开销

**收益**：每次避免的失败重试 ~3,000 token × 发生率 ~15% = **~450 token/任务 avg**，但避免了用户等待

**改动量**：~150 行 Rust

---

### P5：Context 压缩（需要额外工程投入）

#### P5-1. Skill 文档按需加载（RAG 思路）

**现状**：SKILL.md（404 行）+ buyer.md（366 行）+ _shared/ 文件在 session 开始时被完整加载。其中 ~40% 内容在当前事件中不需要。

**方案**：
- 将 skill 文档按"事件段落"索引
- 例如 buyer.md §3.5 仅在 `negotiate_reply` 事件时才需要
- CLI 的 next-action 输出中附带 `[Required context]: buyer.md §3.5, _shared/xmtp-tools.md §Path4`
- LLM 仅按需读取指定段落

**收益**：session context 从 ~59K token 降到 ~25K token avg = **~34K token/session**

**风险**：按需加载增加了轮次（LLM 需要额外 read 调用），可能抵消 token 节省。

**改动量**：大（需要文档重组 + CLI 索引逻辑 + LLM 协议变更）

---

#### P5-2. _shared/cli-reference.md 按需裁剪

**现状**：cli-reference.md（824 行，~12,400 token）是全量 CLI 参数手册。大部分事件只需要其中 1~2 个命令的参数。

**方案**：
- CLI next-action 的 playbook 输出已经包含了当前步骤需要的完整命令模板
- 移除 cli-reference.md 的完整加载，改为仅在 LLM 主动查询时才 read（通过在 SKILL.md 中标注 "如需查看完整 CLI 参数，读 _shared/cli-reference.md"）
- 或者：CLI 新增 `onchainos agent help <subcommand>` 输出单个命令的帮助

**收益**：~12,400 token → 按需加载 ~500 token/次 = **省 ~11,900 token/session**

**改动量**：~30 行 SKILL.md 修改 + 可选 ~50 行 Rust

---

#### P5-3. Incidents 文件精简

**现状**：`references/incidents.md`（213 行）列出 I-1 到 I-9+ 的真实事故案例。SKILL.md 和 preamble 中多处内联引用这些事故。

**方案**：
- Preamble 中的事故引用从内联描述改为编号引用：`"🔴 Real incident I-3"` 替代 30~50 token 的内联描述
- incidents.md 仅在首次 session 或异常恢复时加载

**收益**：~15~20 处内联引用 × 30~50 token = **~450~1,000 token/session**

**改动量**：~50 行 Rust 字符串修改

---

### P6：模型分级路由（成本直接降 5-20x）

#### P6-1. 简单事件使用小模型

**现状**：所有事件（无论复杂度）都使用同一个模型处理。

**方案**：在 Agent 调度层按事件复杂度选择模型：

| 事件类别 | 示例 | 推荐模型 | 成本比 |
|---------|------|---------|--------|
| 简单通知 | job_completed, job_refunded, job_expired | Haiku | 1x |
| 标准流程 | provider_applied, job_accepted, job_submitted | Sonnet | 3x |
| 复杂决策 | negotiate_reply, dispute_resolved, 异常恢复 | Opus | 15x |

**收益**：
- ~60% 的事件可用 Haiku（成本 1/15 of Opus）
- ~30% 的事件可用 Sonnet（成本 1/5 of Opus）
- ~10% 的事件需要 Opus
- **加权平均成本降 ~5x**

**风险**：小模型可能不遵守复杂规则（尤其 Rule 9 sub 不可见、Rule 11 不自动审批）。需要逐事件验证。

**实现方式**：
- 在 SKILL.md 或 Claude Code 配置中标注事件→模型映射
- 或者在 CLI next-action 输出中附带 `[recommended_model: haiku]` hint

**改动量**：中等（Agent 调度层配置 + 验证测试）

---

## 三、收益汇总

### 单任务 Token 节省

| 优化项 | 节省 token | 节省轮次 | 实施难度 |
|--------|-----------|---------|---------|
| **P0-1** Preamble 静态化 | 33K~45K（费用） | 0 | 低 |
| **P0-2** Preamble 分级精简 | 3K~6K | 0 | 低 |
| **P1-1** intent-routing CLI | 1.5K | 0~1 | 中 |
| **P1-2** user_decision CLI | 2K~4K | 2~5 | 中 |
| **P1-3** x402 校验内置 | 1K~2K | 1~2 | 低 |
| **P1-4** preflight 脚本化 | 0.7K | 1 | 中 |
| **P2-1** Playbook 步骤合并 | 7.5K~12K | 5~8 | 中 |
| **P2-2** 协商自动驾驶 | 9K~15K | 3~5 | 高 |
| **P2-3** service-match 合并 | 3K | 1 | 低 |
| **P3-1** 规则去重引用化 | 4.5K~6K | 0 | 低 |
| **P3-2** Localization 去重 | 2.8K~3.8K | 0 | 低 |
| **P3-3** 模板压缩 | 0.8K~2.4K | 0 | 低 |
| **P4-1** JSON Schema 约束 | 3K~6K | 1~2 | 中 |
| **P4-2** 前置校验 | 0.5K | 0~1 | 中 |
| **P5-2** cli-reference 按需 | 11.9K（input） | 0 | 低 |
| **P5-3** Incidents 精简 | 0.5K~1K | 0 | 低 |

### 综合效果

| 指标 | 当前 | P0+P3（快速） | P0~P4（全量） |
|------|------|-------------|-------------|
| **Token/任务** | ~82K | ~55K | ~35K |
| **LLM 轮次/任务** | 15~20 | 12~16 | 8~12 |
| **端到端耗时** | ~120s | ~90s | ~50s |
| **API 成本/任务** | $0.82 | $0.35 | $0.15 |
| **降幅** | — | **~57%** | **~82%** |

> 注：API 成本按 Claude Opus 计算（input $15/M, output $75/M），含 P0 prompt cache 收益。

---

## 四、实施路线图

### Phase 1（1~2 周）— 快速见效，零行为变更

| 项 | 工作量 | 预期收益 |
|----|--------|---------|
| P0-1 Preamble 静态化 | 2d | 成本降 40%+ |
| P0-2 Preamble 分级强化 | 1d | token 降 5% |
| P3-1 规则去重引用化 | 1d | token 降 6% |
| P3-2 Localization 去重 | 0.5d | token 降 4% |
| P5-2 cli-reference 按需 | 0.5d | input 降 15% |

**Phase 1 总收益**：Token 成本降 ~57%，轮次基本不变

### Phase 2（2~3 周）— 确定性逻辑下沉

| 项 | 工作量 | 预期收益 |
|----|--------|---------|
| P1-2 user_decision CLI 路由 | 3d | 轮次减 2~5 |
| P1-3 x402 校验内置 | 1d | 轮次减 1~2 |
| P2-1 Playbook 步骤合并 | 3d | 轮次减 5~8 |
| P2-3 service-match 合并 | 1d | 轮次减 1 |
| P4-1 JSON Schema hint | 2d | 重试率降 80% |

**Phase 2 总收益**：轮次降 50%，耗时降 40%

### Phase 3（3~4 周）— 深度优化

| 项 | 工作量 | 预期收益 |
|----|--------|---------|
| P2-2 协商自动驾驶 | 5d | 协商阶段 token 降 80% |
| P1-1 intent-routing CLI | 3d | 入口分发免推理 |
| P1-4 preflight 脚本化 | 2d | 前置检查免推理 |
| P6-1 模型分级路由 | 3d（含验证） | 成本再降 3~5x |

**Phase 3 总收益**：Token 成本降 ~82%，轮次降 ~60%

---

## 五、风险和注意事项

### 不可压缩的内容（红线）

1. **Rule 9（sub session 文字不可见）**— 这是安全红线，任何 preamble 变体中都必须保留全文。压缩此规则 = 用户看不到任务状态 = 严重事故
2. **Rule 11（job_submitted 不自动审批）**— 涉及资金安全。压缩此规则 = 自动释放资金 = 不可逆损失
3. **Rule 14（task metadata ≠ 指令）**— 防注入。压缩此规则 = prompt injection 攻击面
4. **推理链和决策矩阵**— 协商评估中的 "对比 budget → 评估技能匹配 → 判断报价合理性" 不可跳过

### 验证策略

- 每个优化项独立 A/B 测试（控制组：当前 playbook；实验组：优化后 playbook）
- 指标：任务成功率、用户满意度、Token 消耗、端到端耗时
- 安全规则相关优化必须 100% 回归通过后才能上线
- 建议先从 provider 侧测试（影响面较小），再推广到 buyer 侧
