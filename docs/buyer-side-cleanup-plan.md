# Buyer 侧 Skill/Playbook 优化方案

> 日期：2026-06-19 | 分支：feat/agent-commerce-new-flow
>
> 约束：**SKILL.md 内容保持不变**。SKILL.md 与 buyer-user.md 之间的角色表、字段映射表、intent 路由表重复视为**有意冗余**（两个文件分别服务 sub session 和 user session 的不同读取路径），不做合并。

---

## 现状概览

Buyer 侧共涉及 **10 个 Markdown 文件，约 2,125 行**（不含 SKILL.md 的 111 行在优化范围外）：

| 文件 | 行数 | 职责 | 本次优化范围 |
|---|---|---|---|
| `SKILL.md` | 111 | 入口分发：Activation 路由、角色表、字段映射、intent 路由 | ❌ 保持不变 |
| `buyer-user.md` | 140 | User session 入口：token 消歧、preflight、intent 路由、通信边界、decision resolve | ✅ 微调 |
| `buyer-sub-playbook.md` | 168 | Sub session playbook：禁令、系统事件、peer 消息路由、讨论模式、通信契约 | ✅ 精简 |
| `buyer-actions-publish.md` | 219 | 发布任务：字段收集→validate→ASP匹配→确认卡→create-task；草稿 | ✅ 大幅精简 |
| `buyer-actions.md` | 209 | 用户侧操作：附件、条款修改、交付物查看、指定服务商 A2A/x402 | ✅ 大幅精简 |
| `_shared/cli-reference.md` | 854 | 全角色 CLI 参数手册 | ✅ 替换为按需生成 |
| `_shared/user-intent-routing.md` | 132 | 用户自由文本→子会话路由决策树 | ✅ 精简 |
| `_shared/exception-escalation.md` | 99 | 异常升级规则（buyer/provider 共用） | ❌ 保持不变 |
| `_shared/state-machine.md` | 33 | 状态机枚举（11 status + 37 events） | ❌ 保持不变 |
| `_shared/preflight.md` | 60 | 版本检查 + 安装逻辑 | ❌ 保持不变 |

CLI 侧 buyer flow 代码约 **10,000+ 行 Rust**（flow.rs + flow_lifecycle/ + flow_negotiate/ + 各命令文件），已实现 `next-action` 输出完整 playbook script。

---

## 核心问题

1. **Skill 与 CLI 大面积重叠**：CLI 的 `next-action` 已能输出完整 playbook（含字段收集、校验、ASP 匹配、确认卡模板），但 `buyer-actions-publish.md` 仍维护一套平行描述，两边经常不同步。
2. **Peer 消息路由是纯条件逻辑**：`buyer-sub-playbook.md §3.5` 的 6 优先级路由表是 if-else 链，LLM 每次都要解析整张表来做分发，容易出错（历史 incident 多次）。
3. **cli-reference.md 体量过大**：854 行，skill 加载时吃大量 token；内容等价于 clap `--help` + 少量 LLM 提示。
4. **用户侧操作未走 `next-action`**：附件添加、条款修改、指定服务商等操作在 skill 里写了完整多步流程，但没有对应的 `next-action` 事件，导致 LLM 需要自行编排多个 CLI 调用。

---

## 优化方案

### 第一类：可大幅精简的 Skill 文件

#### 1.1 `buyer-actions-publish.md` — 删除与 CLI 重叠的流程描述

**现状**：219 行，描述了完整的发布流程（字段收集 → validate → ASP 匹配 → 确认卡 → create-task + 错误处理 + 草稿）。

**问题**：CLI 的 `next-action --message '{"event":"create_task","jobId":"_"}'` 已输出等价的完整 playbook（见 `flow_lifecycle/manage.rs::create_task()`，~560 行 Rust，涵盖 Step 1-6 全部）。两套描述不同步时 LLM 不知该听谁的。

**方案**：
- **删除** §1.1–§1.5 的流程描述（~170 行），改为一行指引："run `next-action --message '{"event":"create_task","jobId":"_"}'` for the full publishing flow"
- **保留** §1.6 草稿操作 → 压缩为 ~30 行速查表（draft create/list/update/delete/publish 的 CLI 命令速查）
- **保留** Appendix A 确认卡模板 → 未来随 3.1（确认卡内嵌 CLI）一起删除；当前先保留作为 LLM 拼卡片的参考

**预期效果**：219 行 → ~70 行（-68%）

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| CLI playbook 出 bug 时 skill 层没有兜底描述 | 中 | CI 测试覆盖 `next-action` 各事件分支输出完整性 |
| 非 Claude Code 的 runtime 不走 `next-action`，直接读 skill 文件 | 中 | 精简后保留 "run next-action" 指引；长期推动所有 runtime 统一走 CLI |

---

#### 1.2 `buyer-actions.md` — 按 section 精简 / 下沉

**现状**：209 行，5 个 section。

| Section | 当前行数 | 处理方式 | 优化后行数 | 理由 |
|---|---|---|---|---|
| §2 附件 | ~25 | **下沉 CLI**（见 3.3） | ~5 | 固定流程：task-attach → session send，无 LLM 决策点 |
| §3 条款修改 | ~60 | **下沉 CLI** | ~10 | 仅保留 `set-asp`（换服务商）和 stop task |
| §4 交付物查看 | ~20 | **保留原样** | ~20 | 只有 2 条 CLI，无下沉必要 |
| §5 指定服务商 A2A | ~40 | **下沉 CLI**（见 3.5） | ~10 | 多步验证 + 路由已可由 CLI 一步完成 |
| §6 指定服务商 x402 | ~55 | **下沉 CLI**（见 3.5） | ~15 | 同上，x402 路径更复杂但确定性高 |

**预期效果**：209 行 → ~60 行（-71%）

---

#### 1.3 `_shared/cli-reference.md` — 替换为 CLI 按需生成

**现状**：854 行，全角色 CLI 参数文档。skill 标注 "do not read the whole file, grep the heading"。

**问题**：
- 即使 grep，上下文中仍需加载整个文件（854 行 × ~30 token = ~25,000 token）
- 内容与 CLI 的 clap 定义高度重复
- 更新时需手动同步，经常滞后

**方案**：
- CLI 新增 `onchainos agent help-reference [command]` 子命令，从 clap 定义直接生成 markdown 参数表
- skill 文件替换为 ~20 行的速查索引（命令名 + 一句话用途 + "run `help-reference <cmd>` for params"）

**预期效果**：854 行 → ~20 行（-98%）

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| CLI 开发工作量（~200 行 Rust） | 低 | 遍历 clap 定义生成 markdown，工程量可控 |
| 部分 LLM 提示（"do NOT pass --agentId to create-task"）无法从 clap 自动生成 | 中 | 利用 `#[arg(help = "...")]` 中的注释附加；或在速查索引中保留少量 ⚠️ 提示 |
| 旧版 CLI 没有 `help-reference` | 低 | 版本检查 fallback 到文件；分阶段推进 |

---

### 第二类：可精简的内容

#### 2.1 `buyer-sub-playbook.md` — peer 消息路由下沉后精简

**现状**：168 行。§3.5 Peer Message Routing 占 ~50 行（6 优先级匹配表 + 大量注释 + fallback 的 4 子分支）。

**问题**：纯 if-else 条件分支，LLM 每条 peer 消息都要扫整张表做分发。历史 incident：漏判 `[intent:deliver]`、`[ATTACHMENT_ADDED]` 走错分支、status=0 vs status=1 判断错误。

**方案（下沉 CLI，见 3.2）**：
- §3.5 从 ~50 行缩减到 ~10 行："对 peer 消息执行 `next-action --message '{...peer_message...}'`，按返回的 playbook 执行"
- §3.6 Discussion Mode 保留（status=1 的自由对话仍需 LLM 判断）
- 其他 section（Critical Prohibitions / System Event Handling / Communication Contract / Communication Boundary / Anti-hallucination / Backup Sub Notes）**保留不变**

**预期效果**：168 行 → ~120 行（-29%）

---

#### 2.2 `_shared/user-intent-routing.md` — 精简决策树

**现状**：132 行。决策树 + 多任务消歧 + task list + close + 状态查询 + decision list。

**问题**：6 步决策树（active-tasks → 列表 → 选择 → session query → dispatch）是固定流程，LLM 自行编排容易出错。

**方案（下沉 CLI，见 3.6）**：
- 决策树的 6 步流程下沉为 CLI 命令
- **保留**的内容：trigger phrases 表（skill 层 intent 匹配仍需）、task list/close/status 速查（独立操作）、decision list（独立操作）、entry intents 表

**预期效果**：132 行 → ~70 行（-47%）

---

#### 2.3 `buyer-user.md` — 微调

**现状**：140 行。因为 SKILL.md 保持不变，buyer-user.md 与 SKILL.md 之间的重复内容（角色表、字段映射表）保留。

**可做的微调**：
- Reading Order 中对 buyer-actions-publish.md 的描述更新（精简后该文件内容变化）
- 如果 cli-reference.md 被替换为按需生成，Reading Order 中的引用更新

**预期效果**：140 行 → ~130 行（-7%，仅措辞微调）

---

### 第三类：固定逻辑下沉 CLI

> 原则：**确定性的条件分支**（无需 LLM 推理）应在 CLI 中执行并输出 playbook，而非写成 prompt 让 LLM 解析执行。

#### 3.1 确认卡模板内嵌 CLI

**现状**：确认卡模板在 `buyer-actions-publish.md` Appendix A（~40 行），CLI 的 `create_task()` playbook 引用模板但不直接生成卡片。

**方案**：CLI 的 `prepare-create` 输出中直接包含格式化的确认卡（已有所有字段值），LLM 直接输出即可。

**CLI 改动**：`prepare-create` 的 JSON 输出增加 `confirmationCard` 字段（Markdown table 格式）。

**效益**：
- 消除模板与实际字段不同步风险
- Appendix A 可删（~40 行）
- LLM 不需要自行拼表格，减少格式错误

**工作量**：~50 行 Rust（`content.rs` 中生成 markdown table）

---

#### 3.2 Peer 消息路由下沉 CLI（P0 最高优先级）

**现状**：buyer-sub-playbook.md §3.5 定义 6 优先级路由：

| # | Match | 当前处理 |
|---|---|---|
| # | Match | 当前处理 |
|---|---|---|
| 2 | `[intent:deliver]` | LLM 解析 → 拼 deliverable_received 事件 → next-action |
| 3 | `[intent:reject]` | LLM 解析 → mark-failed → decision card |
| 4 | `[ATTACHMENT_ADDED]` | LLM 解析 → 拼 attachment_added 事件 → next-action |
| 4b | 原始文件/base64 | LLM 判断 → user notify 报错 |
| 5 | Fallback | LLM 查 status → 4 子分支（negotiate_reply / provider_conversation / Discussion Mode / ignore） |

**方案**：CLI 新增 `next-action --message '{"event":"peer_message","jobId":"<jid>","content":"<msg>","senderRole":2,"senderAgentId":"<aid>"}'`

CLI 内部逻辑：
1. 按优先级 `contains` 匹配 content 中的 intent 标签
2. `[intent:deliver]` → 提取 fileKey/digest/salt/nonce/secret/filename，走 `deliverable_received` 分支
3. `[intent:reject]` → 走 `mark-failed` + decision card 分支
4. `[ATTACHMENT_ADDED]` → 走 `attachment_added` 分支
5. Fallback → CLI 内部调 `agent status` 判断 status，返回对应 playbook
6. status=1 → 返回 "enter Discussion Mode" + 锁定规则（LLM 自主对话）

**效益**：
- 消除 LLM 解析 intent 标签的出错率（最常见 incident 源头）
- skill §3.5 从 ~50 行 → ~10 行
- 统一 sub session 处理模式：**所有事件 → `next-action` → 执行 playbook**

**工作量**：~300 行 Rust（flow.rs 增加 `peer_message` 事件分支，复用现有 playbook 生成逻辑）

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| Peer 消息格式不规范（intent 标签在 content 中间） | 低 | CLI 用 `contains` 匹配，和 skill 描述一致 |
| Discussion Mode 无法完全下沉 | 低 | CLI 返回 "enter Discussion Mode" + 行为约束，LLM 自主对话 |
| `[intent:deliver]` 的 fileKey 等字段提取 | 中 | CLI 解析 message content JSON 结构；提取失败则 fallback 返回错误 playbook |

---

#### 3.3 附件添加下沉 CLI

**现状**：buyer-actions.md §2 定义的 2 步流程：
1. `task-attach`（CLI 已有，含状态检查）
2. `okx-a2a session send --content "[ATTACHMENT_ADDED] <path>"`（需 LLM 手动拼）

**方案**：`task-attach` 成功后自动 dispatch [ATTACHMENT_ADDED] 到对应 sub session。

**CLI 改动**：`task-attach` 增加 `--auto-dispatch` flag（或默认开启），成功时内部调 `okx-a2a session send`。

**效益**：
- §2 从 ~25 行变成 "run `task-attach <jobId> --file <path>`" 一句话
- 消除 "step 2 成功但 step 3 漏执行" 的历史 incident

**工作量**：~60 行 Rust

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| session send 失败时需区分 attached vs dispatched | 低 | 返回结构化结果 `{ attached, dispatched, reason }` |
| 无 sub session 时不应 dispatch | 低 | CLI 内部检查 session 是否存在，不存在则只保存 |

---

#### 3.4 set-asp auto-match 下沉 CLI

**set-asp 下沉方案**（唯一保留的条款修改）：
- `set-asp` 增加 `--auto-match` flag，自动调 asp-match 填充 service 参数（单 service 自动选，多 service 返回列表让 LLM 选）
- **效益**：§3 从 ~60 行缩减到 ~10 行（仅 set-asp 速查 + stop task）
- **工作量**：~100 行 Rust

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| set-asp 的 multi-service 选择需 LLM | 中 | 单 service 自动选；多 service 返回列表 + playbook |

---

#### 3.5 指定服务商路由下沉 CLI

**现状**：buyer-actions.md §5 A2A + §6 x402，共 ~95 行。多步流程：

- §5：profile 校验 → asp-match → serviceType 判断 → A2A 走发布流 / x402 走 §6
- §6：profile 校验 → x402-check → 定价确认 → inputRequired 字段收集 → create-task → set-payment-mode → task-402-pay

**方案**：CLI 新增 `next-action --message '{"event":"designate_provider","agentId":"X","endpoint":"<optional>"}'`

CLI 内部执行：profile 校验 + asp-match + serviceType 判断，返回：
- 路由结果（A2A vs x402）
- 下一步 playbook（A2A → 进发布流程；x402 → 定价信息 + 字段列表 + create-task 命令模板）

**效益**：
- §5 + §6 从 ~95 行合并为 ~20 行指引
- CLI 做的 profile + asp-match 校验不会被 LLM 跳过

**工作量**：~200 行 Rust

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| x402 的 inputRequired 字段收集仍需 LLM 与用户交互 | 低 | CLI 返回字段列表，LLM 收集后传 `--body` |
| A2A + x402 两条路径合并为单一入口复杂度高 | 中 | CLI 内部分拆，对外单一入口 |

---

#### 3.6 用户 intent 路由下沉 CLI

**现状**：_shared/user-intent-routing.md 的 6 步决策树：
1. `active-tasks` → 2. 展示列表 → 3. 用户选 → 4. `session query` → 5. dispatch

**方案**：CLI `next-action --message '{"event":"user_intent","text":"<verbatim>"}'`

CLI 内部逻辑：
- 调 `active-tasks` 获取非终态任务列表
- 单任务 → 直接返回 "dispatch to this task" + session info
- 多任务 → 返回 "show numbered list and ask user to pick"
- Session 不存在 → 返回 "no active conversation"

**效益**：减少 LLM 自行编排 3-4 步 CLI 调用的出错机会

**工作量**：~150 行 Rust

**风险**：
| 风险 | 等级 | 缓解 |
|---|---|---|
| 用户 intent 语义模糊（"催一下" 催谁？）| 中 | CLI 只做 task disambiguation + session lookup，intent 语义仍由 LLM 判断 |
| 与现有 `active-tasks` 功能部分重叠 | 低 | 内部复用 |

---

## 多语言（l10n）影响分析

### 当前 l10n 架构

CLI 的 `next-action` playbook 已建立成熟的 l10n 模式：
- **canonical English 模板**：CLI 输出英文模板（占位符 `<...>` 已填充实际值）
- **LLM 翻译指令**：每个用户可见的输出点附带 `L10N_DISPATCH_SHORT` / `L10N_PROMPT` 常量，要求 LLM 按用户语言翻译后再发送
- **buyer flow 中有 145+ 处 l10n 标记**（跨 flow.rs / flow_lifecycle/ / flow_negotiate/ / content.rs）
- **content.rs**：~70 个预填充模板函数（`job_accepted_user_notify()`、`x402_paying_user_notify()` 等），canonical English，LLM 翻译

Skill 文件侧：
- buyer-user.md / buyer-sub-playbook.md 头部有 `🌐 [Localization]` 全局声明
- buyer-actions-publish.md Appendix A 提供了**中英双语**确认卡示例

### 每个下沉项的 l10n 处理

| 下沉项 | 用户可见输出 | l10n 处理方式 | 注意事项 |
|---|---|---|---|
| 3.1 确认卡内嵌 | 确认卡表格（字段标签 + 值） | CLI 输出 canonical English table → playbook 附 `L10N_PROMPT` 要求 LLM 翻译字段标签 | 当前 Appendix A 有中英示例，下沉后字段标签翻译由 LLM 负责（已有规则 6：Chinese → 标题/摘要/描述...） |
| 3.2 Peer 消息路由 | `okx-a2a user notify` 内容、`pending-decisions-v2 request` 的 `--user-content` | 复用现有 l10n 常量：`L10N_DISPATCH_SHORT`（notify 路径）、`L10N_PROMPT`（decision 路径） | 新增的 `peer_message` 分支需逐条输出点附带 l10n 指令，和现有事件分支一致 |
| 3.3 附件自动 dispatch | "附件已保存并转发" / "附件保存失败" | CLI 的 `task-attach` 结果输出 canonical English → LLM 翻译后通过 `okx-a2a user notify` 告知用户 | CLI 不应直接输出用户可见文字；应返回结构化结果 + 模板，由 playbook 指导 LLM 翻译 |
| ~~3.4 max-budget sync~~ | ~~无用户可见输出~~ | **已删除**——发布后不允许修改预算/最高预算/币种 | — |
| 3.4 set-asp auto-match | ASP service 信息展示 + 确认提示 | CLI 返回 service 信息（JSON）→ LLM 组装确认表格时自行翻译标签 | 和 `asp-match` 现有行为一致 |
| 3.5 指定服务商路由 | 定价确认表格、inputRequired 字段收集提示 | CLI 返回路由结果 + canonical English playbook（附 l10n 指令）→ LLM 翻译 | x402 的 `x402-check` 返回的 `fields[].description` 是英文，LLM 需翻译给用户 |
| 3.6 用户 intent 路由 | 任务编号列表、"请选择任务" 提示 | CLI 返回列表数据 → playbook 指示 LLM 用 `okx-a2a user notify` 翻译后展示 | 列表展示已有标准格式（shortJobId + status + role + title），LLM 翻译 status label |
| 1.3 help-reference | CLI 帮助文档（开发者参考） | **不涉及 l10n**（面向 LLM 的参数手册，非用户可见） | — |

### l10n 设计原则（下沉项必须遵循）

1. **CLI 永不直接生成用户可见的最终文本**：所有用户可见输出通过 canonical English template + l10n 指令交给 LLM 翻译。CLI 只输出数据和模板。
2. **每个 `okx-a2a user notify` / `pending-decisions-v2 request` 输出点必须附带 l10n 常量**：新增的 `peer_message` 分支和 `task-attach` 结果 playbook 都需要逐条检查。
3. **content.rs 模板函数是统一出口**：新增的用户可见模板（如附件成功/失败通知）应添加到 `content.rs`，保持 l10n 一致性。
4. **不引入 CLI 内部的语言检测/翻译**：翻译始终由 LLM 完成，CLI 不做 i18n（Rust 侧不打包多语言字符串）。

### l10n 风险

| 风险 | 等级 | 缓解 |
|---|---|---|
| 下沉项的 playbook 漏加 l10n 指令，LLM 直接输出英文给中文用户 | **中** | CI 检查：所有 playbook 输出中包含 `user notify` 或 `--user-content` 的分支必须附带 `L10N` 常量 |
| 确认卡字段标签翻译不一致（当前 Appendix A 有中英对照表，删除后 LLM 可能翻译错） | **低** | CLI 的 `LOCALIZATION_PREFIX` 规则 6 已列出标准中文标签映射（标题/摘要/描述/支付代币/预算/最高预算） |
| `x402-check` 返回的 `fields[].description` 是英文第三方内容，LLM 翻译质量不可控 | **低** | 可接受——和当前行为一致，LLM 已在处理 |
| `task-attach --auto-dispatch` 内部静默 dispatch 成功后，无 playbook 指导 LLM 通知用户 | **中** | CLI 返回值必须包含 `notify_template` 字段 + l10n 指令，不能仅返回 `{ ok: true }` |

---

## 总量效益汇总

| 文件 | 当前行数 | 优化后行数 | 削减 | 说明 |
|---|---|---|---|---|
| `SKILL.md` | 111 | 111 | 0% | 保持不变 |
| `buyer-user.md` | 140 | ~130 | -7% | 微调措辞 |
| `buyer-sub-playbook.md` | 168 | ~115 | -32% | peer 路由下沉 |
| `buyer-actions-publish.md` | 219 | ~70 | -68% | CLI 已覆盖 |
| `buyer-actions.md` | 209 | ~60 | -71% | 操作下沉 |
| `_shared/cli-reference.md` | 854 | ~20 | -98% | 按需生成 |
| `_shared/user-intent-routing.md` | 132 | ~70 | -47% | 决策树下沉 |
| `_shared/exception-escalation.md` | 99 | 99 | 0% | 保持不变 |
| `_shared/state-machine.md` | 33 | 33 | 0% | 保持不变 |
| `_shared/preflight.md` | 60 | 60 | 0% | 保持不变 |
| **合计** | **2,125** | **~768** | **-64%** | |

### Token 节省估算

按每行约 30 token 计算：

| 场景 | 当前加载量 | 优化后 | 节省 |
|---|---|---|---|
| Buyer user session | ~500 行 | ~320 行 | ~5,400 token/次 |
| Buyer sub session | ~400 行 | ~250 行 | ~4,500 token/次 |
| cli-reference.md grep 查询 | ~854 行全量 | ~20 行索引 + 按需 CLI 输出 | ~25,000 token/次 |

### CLI 开发工作量

| 下沉项 | 对应 Skill 精简 | 预估 Rust 代码量 | 优先级 |
|---|---|---|---|
| 3.2 Peer 消息路由 | buyer-sub-playbook §3.5 | ~280 行 | **P0**（历史 incident 最多，收益最大） |
| 3.1 确认卡内嵌 | buyer-actions-publish Appendix A | ~50 行 | P1（简单，收益明确） |
| 3.3 附件自动 dispatch | buyer-actions §2 | ~60 行 | P1（消除常见 incident） |
| 3.4 set-asp auto-match | buyer-actions §3（唯一保留的条款修改） | ~100 行 | P2（有 multi-service 边界） |
| 3.5 指定服务商路由 | buyer-actions §5+§6 | ~200 行 | P2（复杂度较高） |
| 3.6 用户 intent 路由 | user-intent-routing.md | ~150 行 | P2（NLP 部分仍需 LLM） |
| 1.3 help-reference 命令 | cli-reference.md | ~200 行 | P2（工具链改造） |
| **合计** | | **~1,040 行** | |

---

## 风险总结

| 风险 | 等级 | 缓解措施 |
|---|---|---|
| CLI playbook 出 bug，skill 层没有兜底 | **中** | CI 测试覆盖 `next-action` 各事件分支的输出完整性 |
| 非 Claude Code 的 agent runtime 直接读 skill 而不走 CLI | **中** | 精简后的 skill 保留 "run `next-action`" 指引；长期推动所有 runtime 统一走 CLI |
| 旧版 CLI 没有新增命令（help-reference / peer_message 等） | **低** | 版本检查 + fallback；分阶段推进，先 skill 精简，CLI 版本普及后再删旧内容 |
| Peer 消息格式不规范导致 CLI 误判 | **低** | CLI 用 `contains` 匹配；保留 fallback 分支 |
| Discussion Mode 无法完全下沉 | **低** | CLI 返回 "enter Discussion Mode" + 行为约束，LLM 自主对话 |

---

## 推荐执行顺序

### Phase 1（1-2 周）— Skill 精简 + 业务规则清理 + P0 CLI 下沉

> SKILL.md 保持不变。

1. **buyer-actions-publish.md 精简**（1.1）：删除 §1.1-§1.5 流程描述，保留草稿速查 + Appendix A
2. **Peer 消息路由下沉 CLI**（3.2）：flow.rs 增加 `peer_message` 事件分支
3. **buyer-sub-playbook.md 同步精简**（2.1）：§3.5 替换为 next-action 指引

### Phase 2（2-3 周）— P1 下沉

4. **附件自动 dispatch**（3.3）
5. **确认卡内嵌 CLI**（3.1）
6. **buyer-actions.md 同步精简**（1.2）：§2 精简
7. **buyer-user.md 微调**（2.3）：更新 Reading Order 引用

### Phase 3（3-4 周）— P2 下沉 + 工具链

8. **help-reference 命令**（1.3）→ cli-reference.md 替换
9. **set-asp auto-match**（3.4）
10. **指定服务商路由**（3.5）→ buyer-actions.md §5/§6 精简
11. **用户 intent 路由**（3.6）→ user-intent-routing.md 精简
