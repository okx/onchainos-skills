# Phase 7 — recommend 模块删除总结

## 完成日期: 2026-06-17

## 删除范围

### 文件级删除
- `buyer/recommend.rs` (353 行) — 整个文件删除

### 函数/类型删除
- `TaskCommand::Recommend` 枚举变体 (buyer/mod.rs)
- `AgentCommand::Recommend` 枚举变体 (agent_commerce/mod.rs)
- `run_task()` 中 `Recommend` dispatch 分支 (buyer/mod.rs)
- `AgentCommand::Recommend` dispatch 行 (agent_commerce/mod.rs)
- `AgentCommand::Recommend` audit 映射 (audit.rs)
- `enqueue_recommend_decision()` 函数 (common/pending_v2.rs)

### 事件重命名
- `recommend_pick` → `asp_match_pick` (source_event, 全局)
- `user_decision_recommend_pick` → `user_decision_asp_match_pick` (flow.rs SKIP_ALL_EVENTS)

### 命令替换 (playbook 文本)
| 旧命令 | 新命令 |
|--------|--------|
| `onchainos agent recommend {job_id} --agent-id {agent_id}` | `onchainos agent asp-match --job-id {job_id}` |
| `onchainos agent recommend {job_id} --next-page` | `onchainos agent asp-match --job-id {job_id} --next-page` |
| `[Recommend {short_id}]` list-label | `[ASP {short_id}]` list-label |

### match_provider.rs 重写
- **CLI 路径** (`job_created_non_designated_provider_cli`): 去掉了 recommend 的 in-process 调用 (`handle_recommend`)，改为 4-action playbook: notify → `asp-match` CLI → build card → enqueue decision
- **非 CLI 路径** (`job_created_non_designated_provider`): 5-action playbook, 去掉了 `--emit-decision` 相关注释和禁止项

### 文本/注释清理 (14 个文件)
| 文件 | 修改内容 |
|------|---------|
| `flow.rs` | 9 处: preamble IRON RULEs, available_actions, user_decision routing, 各 source_event handler |
| `flow_negotiate/events.rs` | 5 处: set-public 提示, 5min timeout 指令 ×2, provider_reject 选项 |
| `flow_negotiate/designated.rs` | 2 处: x402 路由注释, fallback_cmd |
| `content.rs` | 6 处: notify 模板, doc comment, over_budget 选项, create/draft 通知, escalation |
| `negotiate.rs` | 5 处: 模块 doc, save/load/mark_failed 注释, error message |
| `common/util.rs` | 7 处: resolve_x402_params 注释 + 所有 eprintln 日志 ("recommend cache" → "negotiate cache") |
| `buyer/mod.rs` | 3 处: 模块文件列表, provider doc, endpoint doc |
| `agent_commerce/mod.rs` | 1 处: provider doc comment |
| `create.rs` | 1 处: "recommendations" → "matching" |
| `draft.rs` | 1 处: "recommendations" → "matching" |
| `flow_lifecycle/manage.rs` | 3 处: --provider 提示, create-task/draft-publish 禁止项 |
| `flow_lifecycle/core.rs` | 2 处: provider_applied_over_budget 选项 |

## Review 结论

### Playbook 精简度
- match_provider.rs CLI 路径从 recommend in-process 调用改为直接 `asp-match` CLI 命令，playbook 变得更线性（4 步 vs 之前的 3 步但有 in-process 预抓取）
- 非 CLI 路径去掉了 `--emit-decision` 的冗长解释注释（约 10 行），playbook 更紧凑
- flow.rs user_decision handler 中的命令替换保持了相同复杂度（这些是 LLM 语义路由必需的）

### 固定逻辑下沉情况
- `asp-match` CLI 已经是 Rust 实现（`asp_ops.rs::handle_asp_match`）
- negotiate cache 读写已下沉到 Rust (`negotiate.rs::save/load/current`)
- `pending-decisions-v2 request` 的 enqueue 逻辑仍在 playbook（因为需要 LLM 翻译 card 内容），这是合理的
- **待优化**: CLI 路径的 `job_created_non_designated_provider_cli` 中仍让 LLM 读 asp-match 输出并构建 card — 如果 `asp-match` CLI 能直接输出格式化的 card 文件（像旧 recommend 的 `write_cards_file`），可以进一步下沉

### 编译状态
- `cargo build` ✅ 成功
- 仅有预存在的 warnings（unused variables，来自其他 Phase 待实现的代码）

## 未完成 Phase

| Phase | 状态 | 说明 |
|-------|------|------|
| Phase 1 (State Machine) | ✅ 已完成 | 事件枚举 + parse/as_str/status 映射 |
| Phase 4 (set-asp 改造) | ✅ 已完成 | 新参数 + API body |
| Phase 7 (删除 recommend) | ✅ 已完成 | 本轮 |
| Phase 2 (job_provider_reject) | ❌ 待做 | handler 内部逻辑改用 reset-asp + asp-match |
| Phase 3 (job_user_reject) | ❌ 待做 | provider 侧通知 |
| Phase 5 (create_task 重写) | ❌ 待做 | ASP 前置选择 + serviceParams |
| Phase 6 (job_created 深度改造) | ❌ 待做 | CLI 路径 asp-match in-process 优化 |
| Phase 8 (Skill 文档) | ❌ 待做 | buyer-actions-publish.md, buyer-user.md |
