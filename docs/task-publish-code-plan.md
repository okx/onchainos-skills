# 任务发布新流程 — 代码修改方案

基于 `docs/task-publish-new-flow.md` 需求文档。

## 待确认项（已确认）

1. **`set-asp` 字段**：后端 `POST {jobId}/set/asp` body = `{providerAgentId, serviceId, serviceParams, serviceTokenAddress, serviceTokenAmount, paymentTokenSymbol?(可空), paymentTokenAmount?(可空), paymentMostTokenAmount?(可空)}`
2. **`provider_reject` → `job_provider_reject`**：后端只发 `job_provider_reject`，旧 `provider_reject` 不再使用
3. **recommend 彻底删除**：不需要向后兼容旧任务
4. **草稿发布**：不需要强制 asp-match。未指定 ASP 时，任务发布成功后（job_created），走 asp-match 获取推荐列表

---

## Phase 1: State Machine — 事件变更

**文件: `cli/src/commands/agent_commerce/task/common/state_machine.rs`**

### 1.1 Event 枚举
- `ProviderReject` 重命名为 `JobProviderReject`（枚举名+字符串改为 `"job_provider_reject"`）
- 新增 `JobUserReject`（字符串 `"job_user_reject"`）

### 1.2 Event::parse()
- 删 `"provider_reject" => Event::ProviderReject`
- 加 `"job_provider_reject" => Event::JobProviderReject`
- 加 `"job_user_reject" => Event::JobUserReject`

### 1.3 Event::as_str()
- `Event::ProviderReject => "provider_reject"` → `Event::JobProviderReject => "job_provider_reject"`
- 加 `Event::JobUserReject => "job_user_reject"`

### 1.4 status_when_event()
- `ProviderReject` → `JobProviderReject`（仍映射 `Status::Created`）
- 加 `Event::JobUserReject` → `Status::Created`

### 1.5 failure_label()
- `Event::ProviderReject` → `Event::JobProviderReject`

**文件: `cli/src/commands/agent_commerce/mod.rs`**

### 1.6 SKIP_ALL_EVENTS
- 加 `"job_user_reject"`

### 1.7 PREFETCH_ONLY_EVENTS
- 加 `"job_provider_reject"`

**文件: `cli/src/commands/agent_commerce/task/buyer/flow.rs`**

### 1.8 event 匹配分支
- `Event::ProviderReject` → `Event::JobProviderReject`（handler 函数调用不变，后续 Phase 2 再改内部逻辑）

**文件: `cli/src/commands/agent_commerce/task/provider/flow.rs`**

### 1.9 event 匹配分支
- `Event::ProviderReject` → `Event::JobProviderReject`
- 新增 `Event::JobUserReject` 分支（通知 ASP 用户 + 结束）

---

## Phase 2: Buyer `job_provider_reject` handler

**文件: `buyer/flow.rs`** — `Event::JobProviderReject` handler
**文件: `buyer/flow_negotiate/`** — `provider_reject()` 内部改用 `reset-asp` + 引导 `asp-match`
**文件: `buyer/content.rs`** — 更新文案模板

处理逻辑：
1. 调 `reset-asp` CLI 清除 ASP/服务信息
2. 通知用户 ASP 拒单
3. 引导用户：A. asp-match 重选 B. set-public C. close

---

## Phase 3: Provider `job_user_reject` handler

**文件: `provider/flow.rs`** — `Event::JobUserReject` 分支
**文件: `provider/content.rs`** — `job_user_reject_user_notify()` 模板

处理：通知 ASP "买家已选其他服务商" → 静默结束

---

## Phase 4: `set-asp` 全面改造

**文件: `buyer/mod.rs`** — `SetAsp` 子命令参数：

```rust
SetAsp {
    job_id: String,
    #[arg(long = "provider-agent-id")]
    provider_agent_id: String,
    #[arg(long = "service-id")]
    service_id: String,
    #[arg(long = "service-params")]
    service_params: String,
    #[arg(long = "service-token-address")]
    service_token_address: String,
    #[arg(long = "service-token-amount")]
    service_token_amount: String,
    #[arg(long = "payment-token-symbol")]
    payment_token_symbol: Option<String>,
    #[arg(long = "payment-token-amount")]
    payment_token_amount: Option<String>,
    #[arg(long = "payment-most-token-amount")]
    payment_most_token_amount: Option<String>,
    #[arg(long = "agent-id")]
    agent_id: Option<String>,
}
```

**文件: `buyer/asp_ops.rs`** — `handle_set_asp` 签名和 body 全改：

```rust
pub async fn handle_set_asp(
    client: &mut TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
    service_id: &str,
    service_params: &str,
    service_token_address: &str,
    service_token_amount: &str,
    payment_token_symbol: Option<&str>,
    payment_token_amount: Option<&str>,
    payment_most_token_amount: Option<&str>,
    explicit_agent_id: Option<&str>,
) -> Result<()>
```

body:
```json
{
  "providerAgentId": provider_agent_id,
  "serviceId": service_id,
  "serviceParams": service_params,
  "serviceTokenAddress": service_token_address,
  "serviceTokenAmount": service_token_amount,
  "paymentTokenSymbol": optional,
  "paymentTokenAmount": optional,
  "paymentMostTokenAmount": optional
}
```

**文件: `buyer/mod.rs` `run_task()`** — `TaskCommand::SetAsp` 分支改传新参数

---

## Phase 5: `create_task` Playbook 重写

**文件: `buyer/flow_lifecycle/manage.rs`** — `create_task()` 函数整体重写

新流程步骤：
- Step 1~4: 基本不变（采集 → 校验 → 身份 → 通信检查）
- Step 4.5 (NEW): ASP 匹配
  - 指定 ASP → `asp-match --task-desc X --provider-agent-id Y`
  - 未指定 → `asp-match --task-desc X` → 展示列表 → 用户选择
  - 校验：币种一致性、max-budget ≥ feeAmount
- Step 4.6 (NEW): serviceParams 推理
- Step 5 (MODIFIED): 扩展确认表单（+ ASP/服务/serviceParams 行）
- Step 5.5 (MODIFIED): 路由扩展（修改 ASP → 回 4.5、修改 serviceParams → 更新）
- Step 6 (MODIFIED): create-task 追加 --service-id/--service-params/--service-token-address/--service-token-amount
- Step 6-D (MODIFIED): 草稿路径同步扩展

---

## Phase 6: `job_created` 后 recommend → asp-match

**文件: `buyer/flow_negotiate/`** — 所有 recommend 调用 → `asp-match --job-id`

```
if has_designated_provider {
    // 指定 ASP：已携带，等 job_asp_selected
} else {
    // 未指定 ASP：调 asp-match --job-id 获取推荐列表
    // 用户选定后 → set-asp → 等 job_asp_selected
}
```

**文件: `buyer/flow.rs`** — `user_decision_recommend_pick` 等决策路由改为 asp-match 版本

---

## Phase 7: 彻底删除 recommend

- 删除 `buyer/recommend.rs` 整个文件
- `buyer/mod.rs`: 删除 `mod recommend;`
- `buyer/mod.rs`: 删除 `TaskCommand::Recommend { ... }` 子命令
- `buyer/mod.rs` `run_task()`: 删除 `TaskCommand::Recommend` 分支
- 清理所有 `recommend` 引用（flow.rs / flow_negotiate / content.rs）

---

## Phase 8: Skill 文档

- 重写 `skills/okx-agent-task/buyer-actions-publish.md`
- 更新 `skills/okx-agent-task/buyer-user.md`

---

## 实施顺序

1. Phase 1 + Phase 4 — 基础设施（State Machine + set-asp）
2. Phase 7 — 删除 recommend（先清理再重建）
3. Phase 2 + Phase 3 — 新事件 handler
4. Phase 5 + Phase 6 — Playbook 重写 + job_created 改造
5. Phase 8 — Skill 文档
