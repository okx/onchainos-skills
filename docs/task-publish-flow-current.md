# 任务发布流程 — 当前代码逻辑 (Phase 1+4+5+7+8 完成 + switch-asp 修复)

> 截止 2026-06-17，Phase 1 (State Machine)、Phase 4 (set-asp)、Phase 5 (create_task playbook)、Phase 7 (删除 recommend)、Phase 8 (Skill docs) 已完成。切换 ASP 流程已从复用 `job_created` 改为 `user-reject` → `asp-match` → `set-asp`。

## 1. 意图识别

用户在 user session 发出发布任务的意图：
- "发布一个任务"、"publish a task"、"create a task"、"帮我找人做..."

路由规则 (CLAUDE.md)：
- 用户 session 自由意图 → 读 `skills/okx-agent-task/buyer-user.md` ONLY
- buyer-user.md 内部路由到 `create_task()` playbook (flow_lifecycle/manage.rs)

## 2. 字段采集 (manage.rs Step 1)

| 字段 | CLI flag | 来源 |
|------|---------|------|
| Description | --description | 用户必须提供，≥20 字符 |
| Title | --title | Agent 生成，≤30 字符 |
| Summary | --description-summary | Agent 生成，≤200 字符 |
| Currency | --currency | 用户确认 USDT/USDG |
| Budget | --budget | 用户明确给出 |
| Max budget | --max-budget | 用户明确给出，≥ budget |
| Acceptance window | --deadline-open | 用户明确给出，格式 `<n>h`/`<n>m` |
| Delivery window | --deadline-submit | 用户明确给出 |
| Designated provider | --provider | 可选，用户主动指定 |

🛑 绝对禁止自动填充用户字段。

## 3. 校验 (Step 2) → 身份检查 (Step 3) → 通信检查 (Step 4)

- Step 2: token合法性、budget一致性、description长度
- Step 3: `onchainos agent get` 检查 buyer 身份，无则引导注册
- Step 4: 读并执行 `ensure-okx-a2a-communication-ready.md`

## 4. ASP 匹配 (Step 4.5 — 发布前选服务商)

> **核心变更**：ASP 匹配从 job_created 后下移到 create-task 之前，用 `--task-desc` 而非 `--job-id`。

### 有指定服务商

```bash
onchainos agent asp-match --task-desc "<description>" --provider-agent-id <agentId>
```

取返回的第一个服务 → 校验 currency 一致性 + budget ≥ feeAmount。

### 无指定服务商

```bash
onchainos agent asp-match --task-desc "<description>"
```

- 有结果 → 展示编号列表 → 用户选择 → 校验
- **空列表** → 三选一：
  - A. 修改描述重试
  - B. 指定 ASP (用户给 agentId)
  - C. **公开任务** → 跳过 Step 4.6，`visibility=0`，不传 provider/service

## 4.1 serviceParams 推断 (Step 4.6)

如果选中的服务有 `serviceParams` schema，LLM 根据 description 自动推断参数值（不再询问用户）。

## 5. 确认表单 (Step 5)

展示包含所有字段的表单，等用户确认。

- **私有任务**（ASP 已选）：含服务商 + 服务 + 服务价格 + 服务参数
- **公开任务**（ASP 列表为空，用户选择公开）：服务商显示"公开任务"，省略服务相关行

路由：
- 确认 → Step 6 (create-task)
- 草稿 → Step 6-D (draft create)
- 修改字段 → 回 Step 5

## 6. 发布上链 (Step 6)

### 私有任务（默认 — ASP 已选）

```bash
onchainos agent create-task \
  --description "<description>" \
  --description-summary "<summary>" \
  --title "<title>" \
  --budget <budget> --max-budget <max_budget> \
  --currency <USDT|USDG> \
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \
  --provider <providerAgentId> \
  --service-id <serviceId> --service-params '<serviceParams JSON>' \
  --service-token-address <feeToken> --service-token-amount <feeAmount> \
  --payment-mode <escrow|x402>
```

### 公开任务（ASP 列表为空，用户选择公开）

```bash
onchainos agent create-task \
  --description "<description>" \
  --description-summary "<summary>" \
  --title "<title>" \
  --budget <budget> --max-budget <max_budget> \
  --currency <USDT|USDG> \
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \
  --visibility 0
```

⚠️ 公开任务不传 `--provider`、`--service-*` 和 `--payment-mode` 字段。

关键规则：
- `visibility` 由用户决策决定，不从 `providerAgentId` 推导
- `visibility=1` (私有) 必须传 `providerAgentId`；`visibility=0` (公开) 可不传
- `--visibility` 默认值为 `1`，省略时为私有
- `--payment-mode` 由 `serviceType` 决定：A2A → `escrow`，A2MCP → `x402`，不问用户
- 成功后通知用户（user session 直接输出，不调 xmtp_dispatch_user）
- 结束当前 turn

## 7. 等待链上确认

链上确认后，后端发 `job_created` system event → backup session 收到。

## 8. job_created 事件处理

### 入口 (flow.rs)

```
Event::JobCreated → match_provider.rs::job_created() / job_created_cli()
```

### 判断：是否有指定服务商？

```rust
let has_designated = negotiate::get_designated_provider(job_id).is_some();
```

---

### 分支 A: 无指定服务商 → 公开任务，等待 ASP 主动联系

> 公开任务（visibility=0）创建时无 provider。不再 post-publish 调 asp-match，直接等 ASP 主动发现并联系。

#### CLI 路径 (`job_created_non_designated_provider_cli`)

1-action playbook：

1. **Action 1** — 通知用户任务已上链、等待 ASP 主动申请 (`okx-a2a user notify`)

→ 结束 turn。ASP 主动联系时触发 `provider_conversation` 事件。

#### 非 CLI 路径 (`job_created_non_designated_provider`)

2-action playbook：

1. **Action 1** — `xmtp_dispatch_user` 通知用户
2. **Action 2** — 结束 turn

---

### 分支 B: 有指定服务商 → designated-route 流程

#### CLI 路径 (`job_created_with_designated_provider_cli`)

Rust 层预执行：
1. `session_status()` → sessionKey
2. `designated_route_inner()` → 查询指定 ASP 路由类型

路由分支：
- **a2a** → `designated::branch_a2a_cli()` — 内联执行 B-Step 0/1/1.5（session 检查 + 创建 + SKILL_PREFETCH），生成协商 playbook
- **x402** → `designated::branch_x402()` — x402 直接支付流程
- **error** → `designated::branch_error()` — 错误处理

#### 非 CLI 路径 (`job_created_with_designated_provider`)

LLM 调用 `next-action --event designated_*` 获取分支 playbook。

---

## 9. 用户决策路由 (asp_match_pick)

用户收到 ASP 列表后的决策：

| 用户回复 | 动作 |
|---------|------|
| 选择 ASP (数字/agentId) | **set-asp flow**: 提取服务信息 → 展示 serviceDescription → 用户补全 serviceParams → `set-asp` |
| 下一页 | `asp-match --job-id {job_id} --next-page` → 重新入队 asp_match_pick |
| 公开 | `set-public {job_id}` |
| 关闭 | `close {job_id}` |

## 9.1 切换 ASP 流程 (switch-asp)

> **核心变更**：不再复用 `next-action --event job_created --provider <X>`，改用 `user-reject` → `asp-match` → serviceParams → `set-asp`。

### Path A: 用户从推荐列表选 ASP (asp_match_pick)

1. 用户选择 ASP → 从 asp-match 列表提取 top service 信息
2. 展示 `serviceDescription`，用户补全 `serviceParams`（入队 `set_asp_params` decision）
3. 用户回复 serviceParams → 调 `set-asp`
4. `set-asp` 成功 → 后端触发 `job_asp_selected` 通知 ASP

### Path B: 用户主动指定新 ASP (specify another ASP)

1. `user-reject` — 拒绝当前 ASP（不上链，buyer 直接从接口获取结果）
2. `asp-match --job-id <jobId> --provider-agent-id <newId>` — 获取新 ASP 服务信息
3. 展示 `serviceDescription`，用户补全 `serviceParams`（入队 `set_asp_params` decision）
4. 用户回复 serviceParams → 调 `set-asp`
5. `set-asp` 成功 → 后端触发 `job_asp_selected` 通知 ASP

### 关键规则

- `user-reject` 不上链，buyer 直接从 API 响应获取成功/失败
- `job_user_reject` 事件只推送给 ASP 侧
- `set-asp` 调用后，ASP 收到 `job_asp_selected` 决定是否接单
- `serviceParams` 由用户根据 `serviceDescription` 补全（不再 LLM 自动推断）
- 从推荐列表选 ASP (Path A) 不需要 `user-reject`（没有在协商的 ASP）

## 状态流转

```
用户意图
  ↓
create_task playbook (user session)
  ↓  采集 → 校验 → 身份 → 通信
  ↓
ASP 匹配 (asp-match --task-desc, 发布前)
  ├── 有匹配 → 选 ASP + 服务 → serviceParams 推断 → 确认
  │     ↓
  │   create-task (visibility=1, --provider + --service-*)
  │     ↓
  │   job_created → designated-route
  │     ├── a2a → negotiate (escrow)
  │     ├── x402 → direct payment
  │     └── error → fallback
  │
  └── 空列表 → 三选一 (修改描述 / 指定ASP / 公开)
        ↓ 选公开
      create-task (--visibility 0, 无 provider/service)
        ↓
      job_created → 通知用户 → 等待 ASP 主动联系
        ↓
      provider_conversation → 用户选 ASP → negotiate
```

## 关键变更点 (vs 旧代码)

| 旧 | 新 |
|----|-----|
| `recommend` CLI 命令 | `asp-match` CLI 命令 |
| `recommend.rs` in-process handle_recommend | asp_ops.rs handle_asp_match (已实现) |
| `recommend_pick` source_event | `asp_match_pick` source_event |
| `[Recommend {id}]` list-label | `[ASP {id}]` list-label |
| `enqueue_recommend_decision()` | 直接调 `pending-decisions-v2 request` |
| `ProviderReject` event | `JobProviderReject` event |
| 无 `JobUserReject` | 新增 `JobUserReject` event |
| ASP 匹配在 job_created 后 | ASP 匹配在 create-task 前 (Step 4.5) |
| create-task 不含 service 字段 | create-task 含 --service-id/params/token-address/token-amount |
| 确认表单仅基本字段 | 确认表单含服务商+服务+服务价格+服务参数 |
| visibility 硬编码 1 | visibility 由用户决策：1=私有(需 provider) / 0=公开(ASP 列表为空时用户可选) |
| ASP 列表为空 → 仅提示修改描述 | ASP 列表为空 → 三选一 (修改描述 / 指定ASP / 公开任务) |
| 切换 ASP 复用 `next-action --event job_created --provider <X>` | 切换 ASP 使用 `user-reject` → `asp-match` → serviceParams → `set-asp` |
| serviceParams 由 LLM 自动推断 (create 流程) | 切换 ASP 时 serviceParams 由用户根据 serviceDescription 补全 |
| 无 `set_asp_params` source_event | 新增 `set_asp_params` decision + handler |
