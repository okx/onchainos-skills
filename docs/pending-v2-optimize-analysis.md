# Pending Decisions V2 + 推荐卡片优化分析

> 分析日期：2026-06-11
> 范围：`cli/src/commands/agent_commerce/task/common/pending_v2.rs`、`buyer/flow_negotiate/match_provider.rs`、`buyer/flow.rs`、`buyer/recommend.rs`

---

## 一、问题背景

当前 pending-decisions-v2 的 `xmtp_prompt_user` 推送链路中，AI（包括 sub session 和 user session）存在大量不必要的推理开销：

- **通用 llmContent 模板过于冗长**：每张决策卡都嵌入 25 行指令，包含多卡扫描、前缀匹配、消歧等逻辑，即使只有一张卡也要全部评估
- **llmContent 与 SKILL.md 指令冲突**：两处对多卡场景的指导互相矛盾，AI 被迫做取舍推理
- **推荐列表场景中 sub 端做了 user session 本可完成的翻译工作**
- **handle_request 中存在约 200 行死代码**

---

## 二、架构现状

### 2.1 推送链路总览

```
Sub session                          User session
    │                                     │
    │ pending-decisions-v2 request        │
    │ ──→ CLI: handle_request()           │
    │      ├─ CLI mode → playbook_push_cli
    │      └─ MCP mode → playbook_push_prompt_user
    │           │                         │
    │    stdout: "call xmtp_prompt_user"  │
    │           │                         │
    │ xmtp_prompt_user(llmContent,        │
    │                  userContent)        │
    │ ─────────────────────────────────→  │
    │                                     │ 渲染 userContent（翻译）
    │                                     │ 读 llmContent（决策树）
    │                                     │ END TURN
    │                                     │
    │                          用户回复 "1" │
    │                                     │
    │                                     │ 评估 llmContent 决策树
    │                                     │ → resolve-prompt
    │                                     │ → xmtp_dispatch_session
    │  ←──────────────────────────────── │
    │ relay envelope 到达                  │
    │ next-action → semantic mapping      │
    │ 执行对应指令                         │
```

### 2.2 推荐列表场景链路（public job, `job_created_public`）

```
Sub: job_created → next-action
       ↓ playbook: job_created_public (5 Actions)
Sub: Action 1  session_status → <SUB_KEY>
Sub: Action 2  xmtp_dispatch_user (通知用户任务上链)
Sub: Action 3  onchainos agent recommend → CLI 写 recommend-cards.txt（英文）
Sub: Action 4  Read 卡片文件 → 翻译 → <LOCALIZED_CARD>        ← 开销点
Sub: Action 5  pending-decisions-v2 request --user-content "<LOCALIZED_CARD>"
       ↓ CLI stdout: playbook_push_prompt_user
Sub: call xmtp_prompt_user(llmContent, userContent)
       ↓
User session: 渲染 userContent → END TURN
用户回复: "1"
User session: 评估 llmContent 决策树 → resolve-prompt          ← 开销点
       ↓ relay envelope
Sub: next-action --event user_decision_recommend_pick --data "1"
Sub: 读 semantic mapping (4分支) → "1"=pick ASP                ← 开销点
```

### 2.3 关键代码位置

| 代码 | 文件 | 行号 | 职责 |
|------|------|------|------|
| `handle_request` | `pending_v2.rs` | 527-702 | 入队 + 选择 playbook |
| `resolve_llm_content_prompt_user` | `pending_v2.rs` | 1460-1491 | 生成通用 llmContent（25行决策树） |
| `playbook_push_prompt_user` | `pending_v2.rs` | 1519-1529 | 包装 xmtp_prompt_user 调用指令 |
| `job_created_public` | `match_provider.rs` | 39-78 | 推荐列表 5-Action playbook |
| `recommend_pick` handler | `flow.rs` | 452-468 | 用户回复 semantic mapping |
| `write_cards_file` | `recommend.rs` | 222-285 | 生成英文推荐卡片文件 |

---

## 三、发现的问题

### 3.1 handle_request 死代码（~200 行）

`handle_request` 有两个 early return（L547-564 CLI 模式、L570-603 非 CLI 模式），L605 之后的全部代码**永远不会执行**，已标注 `#[allow(unreachable_code)]`。

**死代码内容**（L605-701）：
- `validate_sub_key` 校验
- Active/Queued 状态判断逻辑
- `active_for_reprompt` 重新提示逻辑
- `playbook_push` / `playbook_wait` / `playbook_wait_with_reprompt` 输出

**连带死函数**（仅在死代码中调用）：

| 函数 | 行号 | 唯一调用点 |
|------|------|-----------|
| `playbook_push` | L1416 | 死代码 L682 |
| `resolve_llm_content` | L1399 | `playbook_push` L1417 |
| `playbook_wait` | L1531 | 死代码 L698 |
| `playbook_wait_with_reprompt` | L1554 | 死代码 L694 |
| `validate_sub_key` | L1370 | 死代码 L611 |

### 3.2 llmContent 与 SKILL.md 指令冲突

**SKILL.md L238**：
> scope rule — the LATEST `[USER_DECISION_REQUEST]` is the ONLY active card; blocks above the stale line are already consumed / expired, **do NOT scan them and do NOT ask the user to pick among them**.

**llmContent**（`resolve_llm_content_prompt_user` L1470-1480）：
> Step 2 — **Scan your current context for OTHER [USER_DECISION_REQUEST] blocks**. If you find any, render the warning...
> 🔁 No prefix + **multiple** blocks in context → **ask user which jobId**

SKILL.md 说"别扫描、别让用户选"，llmContent 说"请扫描、让用户选"。AI 同时收到两条矛盾指令，被迫做取舍推理。

### 3.3 通用 llmContent 过于冗长

`resolve_llm_content_prompt_user` 生成 ~25 行指令，嵌入在每张决策卡的 llmContent 中。包含：

```
Step 1 — Card just delivered.
Step 2 — Scan context for OTHER blocks...           ← 每次都扫描
Step 3 — END THE TURN NOW.
🛑 future turn only:
  · defer keyword → END TURN                        ← 分支1
  · 0x prefix → strip + match                       ← 分支2
  · single block → resolve directly                 ← 分支3
  · 🔁 multiple blocks → ask user → wait → resolve  ← 分支4
Command template: resolve-prompt ...
```

用户说 "1"（选 ASP），AI 的推理路径：
1. "1" 是 defer keyword 吗？→ 检查 18 个关键词 → 不是 ✓（无用）
2. "1" 以 "0x" 开头吗？→ 不是 ✓（无用）
3. 上下文中有其他 `[USER_DECISION_REQUEST]` 块吗？→ 扫描 → 没有 ✓（无用）
4. 单卡 → 调用 resolve-prompt ✓

步骤 1-3 全是浪费。但多任务并行时多张卡是常态，不能简单删除多卡逻辑。

### 3.4 推荐卡片 sub 端翻译开销（Action 4）

`job_created_public` 的 Action 4 要求 sub 端做翻译：

```
Action 4 — Read the card file and translate ONCE to the user's language.
Use Read on the path from Action 3. Translate the card body to the user's
chat language; preserve every data value...
```

**开销**：一次 Read 工具调用 + 一轮完整 AI 翻译推理（2-4s + 数百 tokens）+ 翻译后的内容占据 sub context。

**此步骤存在的原因**：OpenClaw 运行时不自动翻译 `xmtp_prompt_user.userContent`。

**但在 Claude Code 中**：SKILL.md L237 明确 user session 在 Rendering 状态下会 "translate to user's language" — user session 本身就会翻译，sub 端预翻译是重复劳动。

### 3.5 recommend_pick semantic mapping 由 AI 完成

flow.rs L452-467 的 `recommend_pick` handler 返回 4 分支 + 1 个 ambiguous 兜底的文本指令，让 sub AI 做 semantic mapping：

```
• Pick an ASP — index or agentId → map → next-action --provider <X>
• Next page → recommend --next-page → re-push or no_asp_found
• Make public → set-public
• Close → close
⚠️ If ambiguous: re-ask
```

Sub AI 需要：读 4 个分支 → 判断 "1" = pick ASP → 从推荐列表查 index→agentId 映射（可能已不在上下文中）。

### 3.6 handle_resolve 功能性退化

当前非 CLI 模式下，`handle_request` 只写 `Status::Queued`，从不写 `Status::Active`。而 `handle_resolve`（L819-937）查找 `Status::Active` 作为 happy path — 在非 CLI 模式下永远走不到。实际流程已全部走 `resolve-prompt`。

---

## 四、优化方案

### 方案 1：llmContent 按队列深度分发单/多卡模板

**核心思路**：`handle_request` 在写入新 entry 后已知 `q.entries.len()`。利用这个值分发两种模板，消除单卡场景下的多卡推理开销。

**改动点**：
- `resolve_llm_content_prompt_user` 加 `pending_count: usize` 参数
- `playbook_push_prompt_user` 透传 count
- `handle_request` 非 CLI 路径传入 `q.entries.len()`

**单卡模板**（`pending_count == 1`）：

```
[USER_DECISION_REQUEST]
[sub_key: {sub}][job: {job}][role: {role}]
(Anything above is stale.)

→ END TURN. Wait for user reply.

On reply:
  · defer (稍后/later/skip) → END TURN
  · else → `onchainos agent pending-decisions-v2 resolve-prompt
      --user-reply "<verbatim>" --sub-key "{sub}" --job-id "{job}"
      --role "{role}" --agent-id "{agent}" --source-event "{src}"`
    Follow returned playbook.
```

~8 行。无扫描、无消歧、无条件分支。

**多卡模板**（`pending_count > 1`）：

```
[USER_DECISION_REQUEST]
[sub_key: {sub}][job: {job}][role: {role}]
(Anything above is stale.)

⚠️ {count} decisions pending. Tell user to prefix reply with
jobId hash, e.g. `0x7091: approve`.
→ END TURN. Wait for user reply.

On reply:
  · defer (稍后/later/skip) → END TURN
  · `0x<hash>: <reply>` → match [job:] header in context
    → run THAT block's template with stripped reply
  · no prefix → ask user which job (list [job:] headers) → wait → run

This block's template:
  `onchainos agent pending-decisions-v2 resolve-prompt
     --user-reply "<verbatim, no prefix>" --sub-key "{sub}"
     --job-id "{job}" --role "{role}"
     --agent-id "{agent}" --source-event "{src}"`
Follow returned playbook.
```

~15 行。消歧保留但精简：
- 去掉"扫描"指令 — CLI 已告知 "{count} decisions pending"
- 去掉"single block"分支 — 这就是多卡模板
- 3 个分支代替 4 个

**竞态安全性**：

| 场景 | 卡A模板 | 卡B模板 | 用户回复时 | 结果 |
|------|---------|---------|-----------|------|
| A 先到（单卡），B 后到 | 单卡 | 多卡 | AI 看 B 的多卡模板（最新块）→ 消歧 | 安全 |
| A、B 同时存在 | 多卡 | 多卡 | 两张都有消歧 | 安全 |
| A 多卡模板，B 已 resolve | 多卡 | — | A 的多卡模板说"多决策"但只剩一张 → 用户直接回复即可 | 微浪费但不出错 |

**效果**：单卡场景（占多数）AI 上下文减少 ~60%，消除 3 步无用推理。多卡场景指令更精准（count 由 CLI 提供，无需 AI 扫描）。

### 方案 2：修正 SKILL.md L238 与 llmContent 矛盾

**现状**：
- SKILL.md："LATEST block is the ONLY active card; do NOT scan; do NOT ask the user to pick"
- llmContent："scan for OTHER blocks; ask user to prefix with jobId"

**改法**：将 SKILL.md L238 的 "scope rule" 修正为与多卡消歧一致的描述。例如：

> scope rule — when a single `[USER_DECISION_REQUEST]` block exists above the stale line, it is the active card; run its pre-filled `resolve-prompt` directly. When multiple blocks exist, follow the latest block's disambiguation instructions (jobId prefix / ask user to pick).

**效果**：消除 AI 同时收到"别扫描"和"请扫描"的冲突。

### 方案 3：删除 handle_request 死代码 + 5 个死函数

**改动**：
- 删除 `handle_request` L605-701 死代码
- 删除 `#[allow(unreachable_code)]`
- 删除 5 个死函数：`playbook_push`、`resolve_llm_content`、`playbook_wait`、`playbook_wait_with_reprompt`、`validate_sub_key`

**效果**：减少 ~200 行代码噪音。

### 方案 4：Claude Code 下跳过推荐卡 sub 端翻译（Action 4）

**核心思路**：Claude Code 的 user session 在 Rendering 状态下会自动翻译 userContent（SKILL.md L237），sub 端预翻译是重复劳动。

**依据**：
- `is_cli_mode()` 已存在（`content.rs:32-37`），检测 `CLAUDECODE=1` 或 `CODEX_THREAD_ID`
- SKILL.md L237 Rendering 状态："Render `userContent` verbatim (translate to user's language)"
- 卡片内容结构简单（label + 数据值），user session 翻译质量等价于 sub 翻译

**改动点**：

| 文件 | 改动 |
|------|------|
| `match_provider.rs` L39-78 `job_created_public()` | `is_cli_mode()` 分支：true 时跳过 Action 4，Action 5 改用 `--user-content-file` |
| `flow.rs` L452-467 `recommend_pick` handler | next-page 分支同步：CC 下去掉 "read + translate" 指令，改用 `--user-content-file` |

**卡片内容翻译分析**（`write_cards_file` 输出）：

```
[Job 0x7091 — you are the User Agent] Recommended ASPs (page 1):
                                      ^^^^^^^^^^^^^^^^ 英文 header

━━━ 1. #864 | Translation Service ━━━
Description: Professional translation     ← 英文 label + API 值（保留）
Fee: 0.1 USDT                             ← 英文 label + 数字（保留）
Payment: x402                              ← 英文 label + 固定值（保留）

---
Please choose:                             ← 英文 footer
- Reply with a number (e.g. 1, 2, 3)...   ← 英文 footer
- See more recommendations                ← 英文 footer
- List the task on the open marketplace   ← 英文 footer
- Cancel the task                          ← 英文 footer
```

需翻译：label 词（Description/Fee/Payment）、header、footer。数据值全部保持原样。对 user session AI 来说是最基础的翻译任务。

**效果**：省一次 Read 工具调用 + 一轮翻译推理（2-4s + 数百 tokens）+ sub context 更干净。

### 方案 5：推荐卡使用定制 llmContent（`--llm-content`）

**核心思路**：推荐列表卡的用户回复模式高度确定（数字/agentId/下一页/公开/取消），不需要通用决策树。user session 的唯一职责是原样转发。

**实现方式**：在 `pending-decisions-v2 request` 时传入 `--llm-content`，覆盖默认的 `resolve_llm_content_prompt_user`。

**定制内容**：

```
[USER_DECISION_REQUEST]
[sub_key: {sub}][job: {job}][role: buyer]
(Anything above is stale.)

→ END TURN. Wait for user reply.

On reply:
  · defer (稍后/later/skip) → END TURN
  · else → `onchainos agent pending-decisions-v2 resolve-prompt
      --user-reply "<verbatim>" --sub-key "{sub}" --job-id "{job}"
      --role "buyer" --agent-id "{agent}" --source-event "recommend_pick"`
    Follow returned playbook.
```

8 行，去掉了扫描、前缀匹配、多卡消歧。User session AI 只做两件事：判断 defer → 不是 → 原样 relay。

**与方案 1 的关系**：方案 1 是通用模板按队列深度分版；方案 5 是特定卡类型直接覆盖。两者可叠加 — 方案 5 优先级更高（覆盖了默认模板），方案 1 兜底其他卡类型。

**多卡场景兼容**：如果同时有推荐卡（方案 5 的简化模板）和其他卡（方案 1 的多卡模板），最新卡的模板提供消歧逻辑。如果推荐卡是最新的但缺少消歧，用户回复没有 prefix 时，简化模板直接 resolve → relay 到 sub → sub 的 semantic mapping 处理。不会出错。

### 方案 6（远期）：recommend_pick semantic mapping 下沉到 CLI

**核心思路**：`next-action --event user_decision_recommend_pick --data "1"` 的路由逻辑（index→agentId 映射、"下一页"关键词匹配）可以在 CLI Rust 代码中确定性完成，不需要 AI 做 4 分支推理。

**实现方式**：在 flow.rs 的 `recommend_pick` 分支中，CLI 解析 `data` 参数：
- 纯数字 → 查 `negotiate::load()` 缓存 → 直接返回 `next-action --provider <agentId>` playbook
- 匹配"下一页"关键词表 → 直接返回 `recommend --next-page` playbook
- 匹配"公开" → 返回 `set-public` playbook
- 匹配"关闭" → 返回 `close` playbook
- 其他 → 返回 ambiguous re-ask playbook

**效果**：消除 sub 端 4 分支推理 + index→agentId 查找问题。

**但标记为远期**：需要在 Rust 中实现多语言关键词匹配（"下一页" / "next page" / "更多" 等），复杂度中等。

### 方案 7：handle_resolve 标注 deprecated

当前非 CLI 模式下，`handle_request` 只写 `Status::Queued`，`handle_resolve` 查找 `Status::Active` 的 happy path 永远走不到。实际流程已全走 `resolve-prompt`。

**改法**：给 `handle_resolve` 加 deprecated 注释 + trace_log 记录调用（监控是否还有调用方）。暂不删除（CLI 子命令是 public API）。

---

## 五、风险矩阵

| 方案 | 风险等级 | 风险描述 | 缓解措施 |
|------|---------|---------|---------|
| 方案 1（llmContent 单/多卡分版） | 低 | 竞态：单卡模板推送后又来一张卡 | 后到的卡用多卡模板，最新块的指令覆盖（见竞态分析表） |
| 方案 2（修正 SKILL.md 矛盾） | 低 | 修改 SKILL.md 影响所有 user session 行为 | 改为描述性规则而非行为性指令；与方案 1 的 llmContent 保持一致 |
| 方案 3（删除死代码） | 零 | 代码永远不会执行 | `#[allow(unreachable_code)]` 已证明不可达 |
| 方案 4（CC 跳过 Action 4） | 低 | user session 翻译质量不及 sub | 卡片结构简单（label+数值），Claude 模型翻译无压力；OpenClaw 不受影响（`is_cli_mode()` 门控） |
| 方案 4 附带 | 低 | next-page re-push 未同步 | flow.rs `recommend_pick` handler 同步加 `is_cli_mode()` 分支 |
| 方案 5（推荐卡定制 llmContent） | 低 | 多卡时推荐卡缺少消歧 | 最新非推荐卡的多卡模板提供消歧；推荐卡简化模板的直接 relay 也不出错（semantic mapping 在 sub 端） |
| 方案 6（CLI 端路由，远期） | 中 | 多语言关键词匹配的覆盖率 | 需要维护关键词表；未匹配的 fallback 到 ambiguous re-ask |
| 方案 7（deprecated handle_resolve） | 零 | 仅加注释和日志 | 不影响运行时行为 |

---

## 六、优先级与实施计划

| 优先级 | 方案 | 预计改动量 | 效果 |
|--------|------|-----------|------|
| **P0** | 方案 3：删除死代码 + 5 死函数 | ~200 行删除 | 消除代码噪音，去掉 `#[allow(unreachable_code)]` |
| **P0** | 方案 1：llmContent 单/多卡分版 | ~50 行改动（`pending_v2.rs`） | 单卡场景 AI 上下文 -60%，消除 3 步无用推理 |
| **P0** | 方案 2：修正 SKILL.md L238 矛盾 | ~5 行改动（`SKILL.md`） | 消除 user session 矛盾指令 |
| **P1** | 方案 4：CC 跳过推荐卡翻译 | ~20 行改动（`match_provider.rs` + `flow.rs`） | 省 sub 端 Read + 翻译推理（2-4s + 数百 tokens） |
| **P1** | 方案 5：推荐卡定制 llmContent | ~15 行改动（`match_provider.rs` 或 `flow.rs`） | 推荐场景 user session 推理量 -70% |
| **P2** | 方案 7：deprecated handle_resolve | ~5 行注释 | 防止新代码误用退化路径 |
| **P3** | 方案 6：CLI 端路由 | ~100 行新增（`flow.rs` Rust 逻辑） | 消除 sub 4 分支推理 + index 查找 |

---

## 七、方案依赖关系

```
方案 3（删死代码）──── 独立，无依赖

方案 1（单/多卡分版）── 依赖 → 方案 2（SKILL.md 修正，避免矛盾）
                       └── 叠加 → 方案 5（推荐卡覆盖默认模板）

方案 4（CC 跳翻译）──── 独立，无依赖
                       └── 附带改动：flow.rs next-page 分支同步

方案 6（CLI 路由）──── 独立，可独立于方案 1-5 实施
                       └── 受益于方案 5（定制 llmContent 减少了 user session 干扰）

方案 7（deprecated）── 独立，无依赖
```

建议实施顺序：**方案 3 → 方案 1+2 → 方案 4+5 → 方案 7 → 方案 6**
