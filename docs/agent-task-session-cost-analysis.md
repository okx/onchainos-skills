# Agent Task 会话成本深度分析与优化方案

> 基于一次完整任务（使用 Agent 1445 SoulMirror 钱包行为分析）的 3 个 session 实际日志分析。
> 分析日期：2026-06-13

## 一、总览

| 维度 | 值 |
|---|---|
| 任务描述 | 使用 Agent 1445 (SoulMirror) 做钱包行为分析与运势解读 |
| 会话数 | 3（S1 主会话 + S2 buyer 子会话 + S3 backup 子会话） |
| 任务相关 Turns | 78（S1:31 + S2:38 + S3:9） |
| 任务耗时 | S2 约 17.5min（核心协议），S1 约 9min（用户侧发布+监听） |
| 总 token | 8.4M cache\_read + 246K cache\_write + 30K output |
| 估算成本 | **$22.20**（仅任务部分，Opus 4 定价） |
| 模型 | claude-opus-4-6 全程 |

## 二、会话架构

```
S1 (主会话, cwd=/Users/oker, 31 turns, $13.65)
├── 发布任务 (turns 1-13)
│   ├── Skill 加载 + 预检 (turns 1-5)
│   ├── 用户输入收集 (turns 6-11)
│   └── 创建任务 + 启动监听 (turns 12-13)
├── Watch 轮询 (turns 14-25) ← ⚠️ 最大瓶颈
│   └── 12 轮 okx-a2a user watch --once 循环
└── 验收 + 完成 (turns 26-31)
    └── context continuation 后处理用户决策

S2 (buyer 子会话, cwd=~/.okx-agent-task-workspace, 38 turns, $9.22)
├── Skill 加载 (turns 1-3)
├── 协商 negotiate (turns 4-14)
├── 接受 accept (turns 15-20)
├── 交付 deliver (turns 21-25)
├── 审核 review (turns 26-29)
├── 批准 approve (turns 30-33)
└── 完成 complete (turns 34-38)

S3 (backup 子会话, cwd=~/.okx-agent-task-workspace, 9 turns, $2.29)
├── Skill 加载 (turns 1-2)
├── 通知用户 + designated-route (turns 3-4)
└── 建立通信 + 首次询价 (turns 5-9)
```

## 三、成本分布

| 会话 | cache\_read 费 | cache\_write 费 | output 费 | 合计 | 占比 |
|---|---|---|---|---|---|
| S1 (主-任务 turns) | $10.28 | $2.60 | $0.77 | $13.65 | 61% |
| S2 (buyer 子) | $6.08 | $2.01 | $1.13 | $9.22 | 42% |
| S3 (backup 子) | $0.79 | $1.18 | $0.33 | $2.29 | 10% |

> S1 主会话成本占 61%，主要因为每轮上下文 ~160K tokens（含系统提示、全部 MCP tool schema、CLAUDE.md 等），而 S2 子会话只有 ~80K/轮。

## 四、逐瓶颈分析

### 瓶颈 1：Watch 轮询循环（S1 turns 14-25）🔴 P0

| 指标 | 值 |
|---|---|
| Turns | 12 |
| 耗时 | 543s（9min） |
| 上下文消耗 | 1,951,840 tokens（1.95M） |
| 输出 | 4,192 tokens |
| 每轮平均上下文 | 162,653 tokens |
| 每轮平均输出 | 349 tokens |
| 估算成本 | ~$3.66 |

**问题**：每次 `okx-a2a user watch --once` 返回一条消息后，LLM 花 162K tokens 的上下文来生成 349 tokens 的格式化输出（render-verbatim）。LLM 在这里充当了一个极其昂贵的 `printf`。

**实际流程**：
```
Turn 14: watch → [正在连接服务商]
Turn 15: watch → 📤 首次询价发送
Turn 16: watch → 📥 SoulMirror 回复报价
Turn 17: watch → 📤 propose 发送
Turn 18: watch → 📥 ACK 收到
Turn 19: watch → 📤 confirm 发送
Turn 20: watch → [支付方式已设置]
Turn 21: watch → 📥 applied 收到
Turn 22: watch → [任务已接受]
Turn 23: watch → 📥 deliverable 收到
Turn 24: watch → [交付物已收到]
Turn 25: watch → (等待验收决策)
```

**优化方案**：

**方案 A — CLI 侧长轮询 + 直接渲染（推荐）**：
- `okx-a2a user watch --stream` 在 CLI 侧持续轮询并直接输出格式化文本到 stdout
- 只在收到 `decision_request` 类型消息时才返回让 LLM 处理用户交互
- 修改位置：`okx-a2a` CLI (`user watch` 子命令)
- 收益：12 轮 → 1-2 轮，节省 ~$3.4 + 9min
- 风险：低。render-verbatim 本来就是原样输出

**方案 B — 批量拉取**：
- `okx-a2a user watch --batch --until decision_request` 一次性拉取所有待处理消息
- 返回一个 JSON 数组，LLM 一次性渲染
- 修改位置：同上
- 收益：12 轮 → 2-3 轮
- 风险：低

### 瓶颈 2：S1 主会话上下文膨胀（120K vs 30K）🟡 P1

| 会话 | 首轮上下文 | 主要组成 |
|---|---|---|
| S1 (主) | 120,043 tokens | 系统提示 + 100+ MCP tool schema + CLAUDE.md + 19 skill 描述 |
| S2 (子) | 30,669 tokens | 系统提示 + 少量工具 |
| S3 (backup) | 30,772 tokens | 同上 |

**问题**：S1 每轮多出 ~90K tokens 的"噪声上下文"。31 轮 × 90K = 2.79M tokens 的无效上下文开销。

噪声来源分解：
- claude.ai 远程 MCP 的 100+ tool schema（大头）
- 全局 CLAUDE.md 中的 iOS 编码规范、Swift 最佳实践（~2.8KB）
- 19 个不相关 skill 的描述（okx-dex-swap、okx-dex-token 等）

**优化方案**：

**方案 A — 任务模式下临时卸载不相关 MCP（推荐）**：
- 任务 skill 启动时，检测当前 MCP 列表，临时 remove 不相关的远程 MCP
- 任务结束后恢复
- 修改位置：`okx-agent-task` skill 的前置/后置脚本
- 收益：估计可减少 40-60K/轮 → 31 轮节省 ~1.5M tokens (~$2.8)
- 风险：中。需要可靠的恢复机制

**方案 B — 专用 workspace CLAUDE.md**：
- 在 `~/.okx-agent-task-workspace/CLAUDE.md` 放一个精简版指令（仅任务相关路由）
- 对 S2/S3 子会话生效（它们的 cwd 是这个目录）
- 修改位置：workspace CLAUDE.md
- 收益：对子会话影响小（已经只有 30K），但可去掉 iOS 规范等噪声
- 风险：低

### 瓶颈 3：Backup Session (S3) 重叠 🟡 P1

S3 完成了 9 turns 的初始化工作（job_created → designated-route → 首次询价），然后 S2 从 negotiate 开始接手。

**S3 执行的操作**：
```
Turn 1-2: 加载 Skill
Turn 3:   通知用户 [正在连接服务商] + designated-route
Turn 4:   next-action for designated_match
Turn 5-7: 建立 XMTP 通信会话
Turn 8:   发送首次询价 + 检查附件
Turn 9:   结束，等待 sub session 接手
```

**问题**：S3 和 S2 的职责边界不清晰，S3 做了初始化后停止，S2 从中途接手。$2.29 的开销。

**优化方案**：

**方案 A — 合并到单一子会话**：
- job_created 事件直接路由到 sub session 处理，消除 backup session
- 修改位置：会话路由机制（okx-a2a 或 task skill 的 session 管理）
- 收益：消除 S3 = 节省 $2.29 + 9 turns + 1.8min
- 风险：中。需验证 sub session 创建时机是否能覆盖 backup 的场景

**方案 B — 缩小 backup 职责**：
- backup 只做通知用户（1 turn），不做 designated-route 和首次询价
- 修改位置：backup session 的 playbook
- 收益：9 turns → 1-2 turns，节省 ~$1.8
- 风险：低

### 瓶颈 4：Skill 加载开销 🟢 P2

| 会话 | Skill 加载 turns | 上下文消耗 | 耗时 |
|---|---|---|---|
| S2 | 3 turns（SKILL\_PREFETCH → Skill() → Read buyer.md） | ~162K | 14s |
| S3 | 2 turns（Skill() → next-action） | ~72K | 18s |

**问题**：每个子会话启动时花 2-3 轮来加载 SKILL.md + buyer.md。

**当前加载流程**：
```
Turn 1: SKILL_PREFETCH 指令 → 调用 Skill(okx-agent-task) → 加载 SKILL.md
Turn 2: SKILL.md 内容返回 → Read(buyer.md)
Turn 3: buyer.md 内容加载完成 → Ready
```

**优化方案**：

**方案 A — SKILL\_PREFETCH 内联注入（推荐）**：
- 将 SKILL.md 和 buyer.md 的关键内容直接嵌入第一条 system message
- 不需要通过 Skill() → Read() 两步加载
- 修改位置：session 创建逻辑（SKILL\_PREFETCH 机制）
- 收益：每个子会话节省 1-2 turns（~30-50K context + 4-8s）
- 风险：低

**方案 B — buyer.md 合并进 SKILL.md**：
- 将 buyer.md 的核心内容合入 SKILL.md，减少一次 Read
- 修改位置：skill 文件结构
- 收益：节省 1 turn
- 风险：低，但会增大 SKILL.md 体积

### 瓶颈 5：next-action 调用频率与返回格式 🟢 P2

S2 中 10 次 `onchainos agent next-action` 调用：

| Turn | 事件 | output tokens | 返回内容 |
|---|---|---|---|
| 4 | negotiate | 1,603 | 完整 playbook 指令 |
| 8 | negotiate\_ack | 212 | 步骤指引 |
| 11 | job\_payment\_mode\_changed | 315 | 步骤指引 |
| 15 | provider\_applied | 309 | 步骤指引 |
| 18 | job\_accepted | 219 | 步骤指引 |
| 21 | deliverable\_received | 324 | 步骤指引 |
| 26 | job\_submitted | 219 | 步骤指引 |
| 30 | user\_decision | 241 | 步骤指引 |
| 31 | approve\_review | 208 | 步骤指引 |
| 34 | job\_completed | 229 | 步骤指引 |
| **合计** | **10 calls** | **3,879** | — |

**问题**：每次 next-action 返回人类可读的 playbook 文本，LLM 再解析成 CLI 命令执行。这是 "LLM → CLI (获取指令) → LLM (解析执行) → CLI (执行)" 的间接链路。

**优化方案**：

**方案 A — 结构化 JSON 返回（推荐）**：
```json
{
  "action": "ack-to-confirm",
  "params": {"provider": 1445, "jobId": "0x..."},
  "notify": {"template": "payment_set", "target": "user"}
}
```
- 修改位置：`onchainos agent next-action` CLI + `--format json`
- 收益：output 从平均 ~400 → ~100 tokens/次，10 次节省 ~3K output ($0.23)
- 风险：低

**方案 B — CLI 自动执行确定性步骤（激进）**：
- 对完全确定性步骤（如 job\_payment\_mode\_changed → get-agreed → confirm），CLI 直接执行到下一个需要 LLM 决策的点
- `onchainos agent auto-advance --job-id xxx --until-decision`
- 修改位置：CLI 新增子命令
- 收益：可消除 5-6 个 turns (~$1.5)
- 风险：高。错误处理复杂度大增

### 瓶颈 6：通知消息由 LLM 生成 🟢 P2

S2 中 5 次 `xmtp_dispatch_user` 通知：

| Turn | 通知类型 | output tokens |
|---|---|---|
| 13 | 支付方式已设置 | 618 |
| 19 | 任务已接受 | 581 |
| 24 | 交付物已收到 | 733 |
| 35 | 任务已完成 | 656 |
| 37 | 评分已提交 | 419 |
| **合计** | **5 次** | **3,007** |

**问题**：通知消息高度模板化（`[任务已接受] 任务 xxx 已被接受…`），但由 LLM 花 ~600 output tokens "创作"。

**优化方案**：

**方案 A — CLI 模板通知（推荐）**：
```bash
onchainos agent notify --event job_accepted --job-id xxx --lang zh
```
- CLI 侧维护通知模板，直接生成并发送
- 修改位置：`onchainos agent` CLI 新增 `notify` 子命令 + buyer.md 指引
- 收益：每条节省 ~500 output tokens × 5 = 2,500 ($0.19)
- 风险：低

### 瓶颈 7：Context Continuation 🔵 P3

S1 任务期间（turns 1-31）有 1 次 context continuation：

| Turn | 耗时 | cache\_write | 触发原因 |
|---|---|---|---|
| 26 | 91s | 20,476 | 25 轮 watch + 协议消息撑满 context |

**问题**：用户等待验收决策被延迟了 91 秒。

**根因**：Watch 轮询的 12 轮消息（含完整聊天记录）占满了 context window。

**优化方案**：解决瓶颈 1（Watch 下沉 CLI）后，此问题自动消失。Watch 消息不再进入 LLM context，context 不会被撑满。

## 五、S2 阶段耗时与上下文增长曲线

```
Phase                                       Turns   Duration   Ctx/turn    Output
─────────────────────────────────────────────────────────────────────────────────
Skill Loading                                   3       14s      41,902       350
Negotiate (propose)                             3       66s      59,321     2,506
Negotiate (ack→confirm)                         8       93s      70,554     2,571
Accept (applied→escrow)                         6       51s      83,458     1,367
Deliver (receive→save→notify)                   5      100s      96,576     3,401
Review (submit→decision)                        4       75s     105,908     2,661
Approve→Complete                                4       31s     114,202       659
Post-complete (notify+rate+cleanup)             5       57s     123,428     1,552
```

上下文从 42K 增长到 123K（~3x），每轮累积约 2.4K tokens。主要来自：
- 每轮的 tool\_use + tool\_result 记录
- 通知消息文本
- playbook 返回内容

## 六、优化优先级矩阵

| 优先级 | 优化项 | 节省 tokens | 节省成本 | 节省时间 | 难度 | 修改位置 |
|---|---|---|---|---|---|---|
| 🔴 P0 | Watch 轮询下沉 CLI | ~1.8M | ~$3.4 | ~9min | 中 | `okx-a2a` CLI + `okx-task-watch` skill |
| 🟡 P1 | S1 上下文裁剪 | ~1.5M | ~$2.8 | — | 低 | workspace CLAUDE.md + MCP 配置 |
| 🟡 P1 | 消除/缩小 backup session | ~480K | ~$2.3 | ~2min | 中 | 会话路由机制 |
| 🟢 P2 | Skill 内联注入 | ~120K | ~$0.5 | ~12s | 低 | SKILL\_PREFETCH 机制 |
| 🟢 P2 | next-action 结构化返回 | ~3K out | ~$0.23 | ~5s | 中 | `onchainos agent` CLI |
| 🟢 P2 | 通知模板化 | ~2.5K out | ~$0.19 | ~3s | 低 | `onchainos agent` CLI + buyer.md |
| 🔵 P3 | 确定性步骤批量执行 | ~5 turns | ~$1.5 | ~25s | 高 | CLI + buyer playbook |
| 🔵 P3 | Context continuation | — | — | ~91s | — | P0 解决后自动消除 |

## 七、预期收益

| 场景 | 成本 | Turns | 耗时 |
|---|---|---|---|
| 当前基线 | $22.20 | 78 | ~17min (S2) |
| P0 实施后 | ~$18.8 | ~67 | ~8min |
| P0+P1 实施后 | ~$13.7 | ~56 | ~6min |
| 全部实施后 | ~$10.5 | ~45 | ~5min |

**最高 ROI**：Watch 轮询下沉 CLI（P0）——12 轮 LLM 调用做的事情完全可以在 CLI 侧用几十行代码完成，每次任务可节省 $3.4 和 9 分钟等待。

## 八、关于 `~/.okx-agent-task-workspace/CLAUDE.md`

- **性质**：用户运行时文件，非开发文档
- **加载时机**：S2/S3 子会话的 cwd 是 `~/.okx-agent-task-workspace/`，Claude Code 自动加载该目录下的 CLAUDE.md 作为项目级 system prompt
- **当前状态**：不存在（目录为空），子会话只加载全局 `~/.claude/CLAUDE.md`
- **建议**：可以创建一个精简版，仅包含任务系统相关指令，排除 iOS 编码规范等无关内容。对子会话的上下文优化效果有限（已经只有 30K），但能提高指令精准度

---

## 九、S1 Watch 轮询下沉 CLI — 具体实施方案

### 9.1 现状分析

`okx-task-watch/SKILL.md` 定义的 watch 循环：

```
okx-a2a user watch --once --json → 返回 items[]
  ↓
对每个 item 按 kind 分发：
  - kind=notification → 粘贴 userContent 为 blockquote → 再次调用 watch（新的 LLM turn）
  - kind=decision_request → 粘贴 userContent → 等用户回复
  ↓
处理完所有 items → 再次调用 watch → 循环
```

**核心问题**：每次 watch 返回后都需要一个完整的 LLM turn 来执行"粘贴 userContent 为 blockquote"这个纯机械操作。12 条 notification 消息 = 12 个 LLM turns = 1.95M tokens。

### 9.2 方案设计：`--auto-render` 模式

在 `okx-a2a user watch` 命令中新增 `--auto-render` 标志：

```bash
okx-a2a user watch --once --json --poll-ms 1000 --limit 50 --auto-render
```

**行为**：
1. CLI 拉取 items，遍历每个 item
2. `kind=notification` → CLI 直接输出 `> {userContent}` 到 stdout（不需要 LLM 处理）
3. `kind=decision_request` → CLI 停止自动渲染，将该 item 及其后续 items 作为 JSON 返回给 LLM
4. 如果所有 items 都是 notification → CLI 自动重新进入长轮询（不返回）
5. 长轮询超时（无新事件）→ CLI 自动重新进入长轮询（不返回）

**返回给 LLM 的时机只有**：
- 收到 `decision_request` item
- CLI 进程被外部 kill
- 连接错误需要恢复

### 9.3 修改清单

#### 9.3.1 `okx-a2a` CLI 修改

`okx-a2a` 是一个独立的 Node.js CLI 工具（不在 onchainos-skills/cli Rust 项目中）。

**修改文件**：`okx-a2a` 的 `user watch` 命令实现

**新增逻辑**（伪代码）：
```javascript
// user-watch.js（新增 --auto-render 分支）
if (args.autoRender) {
  while (true) {
    const items = await pollItems(args);
    const notifications = items.filter(i => i.kind === 'notification');
    const decisions = items.filter(i => i.kind === 'decision_request');

    // 直接渲染 notification items 到 stdout
    for (const n of notifications) {
      const lines = n.userContent.split('\n').map(l => `> ${l}`).join('\n');
      console.log(lines);
      console.log(''); // 空行分隔
    }

    // 遇到 decision_request 时，返回 JSON 让 LLM 处理
    if (decisions.length > 0) {
      console.log(JSON.stringify({
        autoRenderedCount: notifications.length,
        pendingDecisions: decisions
      }));
      break; // 退出循环，交给 LLM
    }

    // 没有 decision_request，继续长轮询
    // （stdout 的 notification 渲染已经完成，用户看到了进展）
  }
}
```

**关键设计**：
- notification 渲染格式与当前 SKILL.md §kind==notification 规定的 `> <userContent>` 完全一致
- decision\_request 仍然以 JSON 返回，由 LLM 处理用户交互
- 自动循环长轮询，直到遇到 decision\_request 或终止信号
- **多语言无需额外处理**：`userContent` 的翻译在上游完成（S2 子会话的 `xmtp_dispatch_user` 调用处，由 buyer.md 的 LOCALIZATION 规则指导 LLM 翻译），到达 watch 时已经是目标语言。SKILL.md 明确规定 notification 处理为 "paste verbatim, no translation of body content"，`--auto-render` 只是将这个粘贴操作从 LLM 侧移到 CLI 侧，不改变翻译链路

#### 9.3.2 `okx-task-watch/SKILL.md` 修改

**修改内容**：

1. §Run watch 中的命令改为：
```bash
okx-a2a user watch --once --json --poll-ms 1000 --limit 50 --auto-render
```

2. 新增 §Auto-render dispatch：
```
当使用 --auto-render 模式时：
- notification items 已由 CLI 直接渲染到 stdout，无需 LLM 处理
- CLI 返回时只有两种情况：
  a) JSON 包含 pendingDecisions[] → 按 §kind==decision_request 处理
  b) CLI 退出（错误/kill）→ 重新调用 watch
```

3. 保留非 auto-render 路径作为 fallback（兼容性）。

#### 9.3.3 Scoped session 终止检测

当前 SKILL.md 有 scoped session 终止规则：当 notification 的 userContent 包含 `[Job Completed]` 等终止标记时停止 watch。

**在 `--auto-render` 模式下**，终止检测需要下沉到 CLI：
- CLI 在渲染 notification 后检查 userContent 是否包含终止标记
- 如果包含且是 scoped session（`--job-id` 存在）→ CLI 退出并在 JSON 中标记 `{"stopped": true, "reason": "terminal_state"}`
- LLM 收到后不再重新调用 watch

### 9.4 优化前后对比

**优化前**：
```
Turn 14: LLM → Bash(watch) → 1 notification → LLM 生成 "> xxx"
Turn 15: LLM → Bash(watch) → 1 notification → LLM 生成 "> xxx"
...（重复 12 次）
Turn 25: LLM → Bash(watch) → 1 notification → LLM 生成 "> xxx"
Turn 26: watch 返回 decision_request → LLM 处理用户验收决策
```

**优化后**：
```
Turn 14: LLM → Bash(watch --auto-render)
          → CLI 直接渲染 12 条 notification（stdout 输出 12 段 blockquote）
          → CLI 遇到 decision_request → 返回 JSON
Turn 15: LLM 处理 decision_request → 等用户验收决策
```

**节省**：11 个 LLM turns，~1.8M tokens context，~$3.4，~8min。

### 9.5 风险与兜底

| 风险 | 缓解 |
|---|---|
| Bash tool 超时（默认 120s） | 设置 `timeout: 600000`（10min），或检测到超时后自动重调 |
| Claude Code 的 run\_in\_background 误判 | SKILL.md 已明确禁止 run\_in\_background，保持不变 |
| stdout 过长被截断 | 单条 notification 一般 <500 字符，12 条 <6KB，不会触发截断 |
| 向后兼容 | `--auto-render` 是新增 flag，不影响现有 `--once --json` 行为 |

---

## 十、S2 Buyer 子会话优化 — 具体实施方案

### 10.1 现状代码架构

S2 子会话的核心执行链路：

```
                  inbound message (a2a-agent-chat / system event)
                         ↓
              buyer.md §3.5 路由 → 识别事件类型
                         ↓
              onchainos agent next-action --event <X>
                         ↓
              flow.rs::generate_next_action()
                ├── localization_prefix (首次 ~1.2KB / 后续 ~0.3KB)
                ├── version_prefix (~0.1KB)
                ├── preamble (micro/slim/medium/negotiate/full, 0.1-3KB)
                ├── prefetched_block (可选, ~0.5KB)
                └── event body (来自 flow_negotiate/ 或 flow_lifecycle/)
                         ↓
              LLM 解析 playbook → 执行 CLI 命令
                         ↓
              xmtp_dispatch_user / xmtp_send / pending-decisions-v2
```

**关键代码文件**：

| 文件 | 职责 | 行数 |
|---|---|---|
| `buyer/flow.rs` | 主调度器 + preamble 定义 + L10N 常量 | 598 |
| `buyer/flow_negotiate/match_provider.rs` | job_created 事件（指定/非指定 provider） | 246 |
| `buyer/flow_negotiate/designated.rs` | 指定 provider 的 a2a/x402/error 分支 | — |
| `buyer/flow_negotiate/events.rs` | negotiate_reply/ack/counter 事件 | — |
| `buyer/flow_lifecycle/core.rs` | 执行阶段：accepted/submitted/delivered/completed | ~550 |
| `buyer/flow_lifecycle/terminal.rs` | 终止状态 + 期限警告 | ~340 |
| `buyer/flow_lifecycle/dispute.rs` | 争议仲裁 | ~120 |
| `buyer/content.rs` | 通知消息模板（canonical English） | — |

### 10.2 优化 A：Preamble 分级缩减（flow.rs 修改）

#### 10.2.1 现状

flow.rs 定义了 5 级 preamble：

| 级别 | 大小 | 使用事件 |
|---|---|---|
| `preamble_micro` | ~0.2KB | 终止状态、评估人事件、管理操作 |
| `preamble_slim` | ~1.2KB | negotiate_ack、approve/reject_review、user_decision_* |
| `preamble_medium` | ~1.5KB | payment_mode_changed、applied、accepted、delivered、submitted 等 |
| `preamble_negotiate` | ~1.8KB | negotiate_reply、negotiate_counter |
| `context_preamble`（full） | ~3.0KB | job_created（仅首次） |

**preamble 每轮都在 next-action 输出中**，被 LLM 读取。随着对话轮数增加，这些重复指令在 LLM 上下文中累积。

#### 10.2.2 方案：进一步降级 preamble

对于高频的确定性事件，将 preamble 从 medium → slim 或 slim → micro：

```rust
// flow.rs 修改点 (line ~554-571)

// 当前: use_medium_preamble 包含 provider_applied / job_accepted
// 优化: provider_applied 是纯确定性的（只调用 confirm-accept），降级为 slim
let use_slim_preamble = matches!(event_str,
    "negotiate_ack" |
    "provider_applied" |      // ← 从 medium 下移
    "approve_review" | "reject_review" |
    "review_deadline_warn" |
    "job_auto_completed" |
    "dispute_resolved" |
    "wakeup_notify"
) || event_str.starts_with("user_decision_");

// job_payment_mode_changed 也是确定性的（get-agreed → confirm），降级为 slim
```

**收益**：每次降级节省 ~0.3KB output tokens，10 次 next-action 共节省 ~3K output。
**风险**：低。确定性步骤不需要完整的规则集。

### 10.3 优化 B：通知模板下沉 CLI（content.rs → notify 子命令）

#### 10.3.1 现状

`buyer/content.rs` 已经定义了所有通知模板（canonical English）：

```rust
// content.rs（示例）
pub fn job_accepted_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!("[Job Accepted] Task `{title}` ({job_id}) has been accepted, ...")
}
```

但这些模板嵌入在 next-action 的 playbook 输出中，由 LLM 执行翻译 + 参数填充 + `xmtp_dispatch_user` 调用。

#### 10.3.2 方案：新增 `onchainos agent notify` 子命令

**新文件**：`cli/src/commands/agent_commerce/task/buyer/notify.rs`

```rust
// notify.rs（伪代码）
pub fn handle_notify(job_id: &str, event: &str, agent_id: &str, lang: &str) -> Result<()> {
    // 1. 从 task API 获取 title, provider, amount 等
    let ctx = fetch_task_context(job_id, agent_id)?;

    // 2. 根据 event 选择模板
    let template = match event {
        "job_accepted" => content::job_accepted_escrow_user_notify(job_id, &ctx.title),
        "job_completed" => content::job_completed_user_notify(job_id, &ctx.title, ...),
        "deliverable_received" => content::deliverable_received_user_notify(...),
        "payment_set" => content::payment_mode_set_user_notify(...),
        "rating_submitted" => content::rating_submitted_user_notify(...),
        _ => return Err("unknown event"),
    };

    // 3. 翻译（如果 lang != "en"）
    let content = if lang == "en" { template } else { translate(&template, lang) };

    // 4. 调用 xmtp_dispatch_user（通过已有的内部机制）
    dispatch_to_user(&content)?;

    println!("{{\"ok\": true, \"event\": \"{event}\", \"dispatched\": true}}");
    Ok(())
}
```

**CLI 接口**：
```bash
onchainos agent notify --job-id <jobId> --event job_accepted --agent-id <agentId> --lang zh
```

**playbook 修改**：将 flow_lifecycle 中的通知步骤从"LLM 翻译 + xmtp\_dispatch\_user"改为单条 CLI 命令。

**示例 — `job_accepted` 事件**（core.rs:30）：

```
当前 playbook:
  Step 1: Fetch task info (common context)
  Step 2: Branch by payment mode
  Branch A (escrow): Call xmtp_dispatch_user, content=<template>

优化后 playbook:
  Step 1: Use pre-fetched context (already available)
  Step 2 (escrow):
  ```bash
  onchainos agent notify --job-id <jobId> --event job_accepted --agent-id <agentId> --lang <detect>
  ```
```

**收益**：每条通知 LLM 不再需要：(a) 读取模板 ~200 tokens (b) 翻译 ~300 tokens (c) 组装 xmtp\_dispatch\_user 参数 ~100 tokens。5 条通知共节省 ~3K output tokens。

**挑战**：
- 翻译需要在 CLI 侧实现。可以用简单的 key-value 对照表（通知文案固定，不需要通用翻译）。
- 需要在 CLI 侧检测用户语言（可从 session metadata 或 SKILL.md LOCALIZATION 标记推断）。

### 10.4 优化 C：确定性事件合并执行

#### 10.4.1 可合并的事件链

分析 S2 的实际执行流程，以下事件链是完全确定性的（无需 LLM 决策）：

```
链 1: job_payment_mode_changed
  → next-action → get-agreed → xmtp_send [intent:confirm] + xmtp_dispatch_user [支付方式已设置]
  → 当前 3 turns (Turn 11-13)

链 2: provider_applied
  → next-action → confirm-accept
  → 当前 2 turns (Turn 15-16)

链 3: job_accepted
  → next-action → xmtp_dispatch_user [任务已接受]
  → 当前 3 turns (Turn 18-20)

链 4: job_completed
  → next-action → xmtp_dispatch_user [完成] + feedback-submit + session-cleanup
  → 当前 5 turns (Turn 34-38)
```

#### 10.4.2 方案：`next-action --auto-execute` 模式

对确定性事件，CLI 不仅返回 playbook，还直接执行：

```bash
onchainos agent next-action --jobid <X> --event provider_applied --auto-execute
```

CLI 内部流程：
1. 生成 playbook（同 generate\_next\_action）
2. 检测该事件是否为"确定性事件"（无分支、无用户决策）
3. 如果是 → 直接执行 CLI 命令（confirm-accept）并返回执行结果
4. 如果不是 → 正常返回 playbook 文本

**确定性事件白名单**：
```rust
fn is_deterministic(event: &str, payment_mode: Option<i64>) -> bool {
    matches!(event, "provider_applied") ||
    (event == "job_accepted" && payment_mode == Some(1)) || // escrow only
    event == "job_completed" ||
    event == "job_refunded" ||
    event == "job_expired" ||
    event == "job_closed"
}
```

**非确定性事件**（必须返回 playbook 给 LLM）：
- `negotiate_reply` / `negotiate_counter` — 需要 LLM 评估报价
- `job_submitted` — 需要用户决策（验收/拒绝）
- `deliverable_received` — 需要 LLM 解析交付物内容
- `job_accepted` (x402) — 需要检查 replaySuccess

**收益**：可将链 2 (provider\_applied) 从 2 turns 缩减为 1 turn，链 4 (job\_completed) 从 5 turns 缩减为 1-2 turns。预计节省 5-6 turns (~$1.5)。

**风险**：高。需要完善的错误处理（CLI 执行失败时如何回退）和全面测试。建议作为 P3 分阶段推进。

### 10.5 优化 D：Skill 加载提速

#### 10.5.1 现状流程（S2 的 3 轮加载）

```
Turn 1: [SKILL_PREFETCH] 消息到达 → Skill(okx-agent-task) → 加载 SKILL.md (~10K tokens)
Turn 2: SKILL.md 内容注入 → Read(buyer.md) (~6K tokens)
Turn 3: buyer.md 内容注入 → Ready (回复 "ready")
```

`SKILL_PREFETCH` 是在 `match_provider.rs` A-Step 1.5 中定义的：
```
content = "[SKILL_PREFETCH] Read okx-agent-task/SKILL.md then okx-agent-task/buyer.md.
           No action needed for this message — but process all subsequent messages
           normally via buyer.md §3.5 routing (#1–#6)."
```

#### 10.5.2 方案：合并为单轮加载

**修改 SKILL\_PREFETCH 消息格式**，在第一条消息中同时携带 SKILL.md 和 buyer.md 的内容：

```
[SKILL_PREFETCH]

=== SKILL.md ===
<SKILL.md 全文>

=== buyer.md ===
<buyer.md 全文>

Process all subsequent messages via buyer.md §3.5 routing.
```

**修改位置**：
1. `match_provider.rs` A-Step 1.5 — 修改 SKILL\_PREFETCH content 格式
2. `flow_negotiate/designated.rs` — 同上
3. `SKILL.md` §SKILL\_PREFETCH 识别逻辑 — 适配新格式

**实现方式**：在发送 SKILL\_PREFETCH 时，CLI 读取 SKILL.md 和 buyer.md 文件内容并嵌入消息体：

```rust
// match_provider.rs 修改
fn build_prefetch_content() -> String {
    let skill_md = std::fs::read_to_string(skill_path("SKILL.md")).unwrap_or_default();
    let buyer_md = std::fs::read_to_string(skill_path("buyer.md")).unwrap_or_default();
    format!(
        "[SKILL_PREFETCH]\n\n=== SKILL.md ===\n{skill_md}\n\n=== buyer.md ===\n{buyer_md}\n\n\
         Process all subsequent messages via buyer.md §3.5 routing."
    )
}
```

**注意**：
- SKILL.md + buyer.md 合计 ~16K tokens，作为单条消息发送。在 XMTP 消息大小限制内（一般 ~200KB）。
- 子会话收到后，第一个 LLM turn 直接具备完整上下文，不需要调用 Skill() 和 Read()。

**替代方案**（更简单但侵入性小）：

修改 SKILL\_PREFETCH 指令为 `"Read okx-agent-task/SKILL.md then okx-agent-task/buyer.md"` → **让 LLM 在同一轮内并行调用两个 Read**（而非先 Skill 再 Read），这样 3 轮 → 2 轮：

```
Turn 1: [SKILL_PREFETCH] → LLM 并行调用 Read(SKILL.md) + Read(buyer.md)
Turn 2: 两个文件内容注入 → Ready
```

但这依赖 LLM 的并行工具调用能力，不如 CLI 内联方案可靠。

**收益**：3 turns → 1 turn，节省 ~60K context tokens + ~8s。
**风险**：低。内容不变，只是传输方式变化。

### 10.6 S2 优化汇总

| 优化项 | 修改文件 | Turns 减少 | Output 节省 | 实现难度 |
|---|---|---|---|---|
| Preamble 降级 | `flow.rs` (3 行 match 条件) | 0 | ~3K | 低 |
| 通知模板 CLI 化 | 新增 `notify.rs` + `flow_lifecycle/core.rs` | 0 | ~3K | 中 |
| 确定性事件自动执行 | `mod.rs` + 新增 `auto_execute.rs` | 5-6 | ~2K | 高 |
| Skill 加载合并 | `match_provider.rs` + `designated.rs` | 2 | ~0.5K | 低 |
| **合计** | — | **7-8** | **~8.5K** | — |

S2 从 38 turns → ~30 turns，耗时从 17.5min → ~12min，成本从 $9.22 → ~$7.0。

---

## 附录：Session IDs

- S1 (主): `8161fc76-320d-4c35-9f1c-b7dfa156286b`
- S2 (buyer 子): `3f332aa2-e40b-42fd-b720-b8129aac1c00`
- S3 (backup 子): `b307f40c-0a62-4048-983a-3cfdd339dabe`