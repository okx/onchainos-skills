# Escrow Flow Simplify - Requirements

> Source: [Lark whiteboard](https://okg-block.sg.larksuite.com/docx/H0KldsZsmooVeKxPMEKlS4FKgpg) + [API doc](https://okg-block.sg.larksuite.com/docx/Mq9Vdmf8goEIjCx5xK4lbsCjg8e)
> Flow diagram: `docs/whiteboard_exported_image.pdf`
> Scope: escrow payment mode, from publish to delivery
> Branch: `feat/agent-commerce-new-flow`
> Date: 2026-06-16 (updated)

---

## Overview

Simplify the escrow flow from the current multi-round 3-step handshake
(propose → ack/counter → confirm) to a streamlined **accept/reject + auto-pay** model.

Key design changes vs the old flow:
- **No serviceHasPrice branching** — ASP always responds with accept+price or reject
- **Backend judges price** — CLI auto-accepts based on backend's price check, User model does NOT evaluate price
- **A2A session created at job_created / job_asp_selected** — simplified to `session_create` only; no negotiation messages sent to ASP (backend notifies ASP directly)
- **ASP must use User's token** — no cross-token negotiation; backend validates token consistency
- **Post-accept param negotiation** — if ASP's required params are incomplete, A2A negotiation handles it after accept

### Five-phase architecture

| Phase | Name | On/Off-chain | Funds | Session |
|---|---|---|---|---|
| 1 | Select ASP + fill inputParams | off-chain, no jobId | none | User session |
| 2 | createTask + createJob | on-chain | none (no escrow yet) | User session → system |
| 3 | ASP accept/reject (off-chain) | off-chain | none | Backend → ASP push |
| 4 | Price check + User accept + escrow | on-chain | funds locked | Buyer job session |
| 5 | ASP execution + delivery | off-chain (A2A) | escrowed | A2A session |

---

## Phase 1: Pre-publish service discovery

### Flow (User Session)

```
开始 → 发布任务(草稿) → 区分是否指定ASP?
  ├─ 是 → 调后端接口取回ASP最高分服务
  └─ 否 → 调后端接口取回ASP列表 → Skill引导用户选择ASP
                                    (列表中ASP已经包含唯一最高分服务)
      ↓
  基于服务提取所需入参 (User Agent思考)
      ↓
  Skill引导用户补充入参 (识别到即可)
      ↓
  把用户任务字段和入参传给后端，创建任务
```

### API: `POST /priapi/v1/aieco/task/asp/match`

Used both pre-publish (Phase 1) and post-publish (switch ASP). Replaces `recommend` + `designated-route`.

```
Request:  {
  "page": 1,
  "taskDesc": "...",              // required when no jobId
  "jobId": "0x...",               // required when task already exists (post-publish)
  "providerAgentId": "887877"     // optional — narrows to this ASP's services
}
Response: { "recommendations": [{ "providerAgentId", "services": [{ "serviceId", "serviceType", "feeAmount", "feeTokenSymbol", "endpoint", ... }] }] }
```

Parameter rules: if `jobId` is available (task already created), pass it; if not (pre-publish), `taskDesc` is required. At least one of `jobId` / `taskDesc` must be present.

- Designated ASP → returns that ASP's single highest-scored A2MCP/A2A service
- Undesignated → returns matched ASP list (each with only the highest-scored service)
- `serviceType: "A2MCP"` = x402; `serviceType: "A2A"` = escrow

### serviceParams format

`serviceParams` is a **natural language string**, not structured JSON. The required fields
are extracted from the ASP's `serviceDescription` in the `asp/match` response.

Example: ASP service requires "meme image + name". User fills in concrete values:
```
"meme 图片：稍后通过通信组件发给你；\n名称：xxxx。"
```

The Skill/Agent reads `serviceDescription` to identify what the ASP needs, guides the user
to provide the values, then assembles them into a human-readable `serviceParams` string
passed to `create-task`.

### Skill responsibilities

1. Extract required input fields from `serviceDescription` in asp/match response
2. Guide user to fill missing params (recognize completion)
3. Assemble filled values into `serviceParams` (natural language string)
4. Validate: **User max_budget >= ASP service price** (comparison only; see validation rules below)
5. Validate: **User balance >= max_budget** (warn if insufficient)

### Validation rules at create time

| Check | Enforced? | Notes |
|---|---|---|
| Task token == ASP service token | **NO** | "省去User思考，币种让ASP匹配User" — ASP adapts to User's token at apply time |
| Task budget vs ASP service price | **NO** | No price validation at creation |
| User max_budget >= ASP service price | **YES** (Skill-side) | Skill warns if max_budget < service price |
| User balance >= max_budget | **YES** (Skill-side) | Skill warns if balance insufficient |

### CLI command: `asp-match`

Wraps the API. Used in Phase 1 (pre-publish) and when switching ASP (post-publish).

```
onchainos agent asp-match --task-desc "..." [--job-id 0x...] [--provider-agent-id 887877] [--page 1]
```

- Pre-publish: `--task-desc` required (no `--job-id`)
- Post-publish: `--job-id` required (backend derives desc); `--task-desc` optional

---

## Phase 2: createTask (on-chain)

### Modified API: `POST /priapi/v1/aieco/task/create`

New fields in request body:

| Field | Type | Description |
|---|---|---|
| `serviceParams` | String | Service input parameters (natural language, see Phase 1) |
| `serviceId` | String | Service ID from asp/match |
| `serviceTokenAddress` | String | Service token contract address |
| `serviceTokenAmount` | String | Service price |

Rules:
- `visibility=private(1)` → `providerAgentId` required
- `visibility=public(0)` → `providerAgentId` optional
- ASP binding is off-chain until Phase 4 (acceptJob)
- `inputParams` are off-chain (not in createJob calldata)
- `paymentMostTokenAmount` (max budget) is required

### Post-create: buyer handler (`job_created`)

```
job_created event → buyer handler:
  CLI mode:
    1. session_query_exists(job_id, my_agent_id, provider_agent_id)
    2. session_create(job_id, my_agent_id, provider_agent_id)  → returns sessionKey
    3. session_send_by_job(job_id, provider_agent_id, "任务已创建，等待ASP响应")
    4. 通知用户"任务已创建" (print to stdout)
    5. 结束 (end turn, wait for provider_applied / job_provider_reject)

  Playbook mode:
    1. xmtp_dispatch_user: 通知用户"任务已创建"
    2. session_create (via okx_a2a.rs) with provider
    3. 结束 (end turn)
```

**Key change**: `job_created` handler creates the A2A session with the provider and notifies
the user. It does NOT send negotiation messages, does NOT call `designated-route` or
`recommend`. The **backend** sends the order notification to ASP directly.

### Draft API changes

Create/update draft must also carry `serviceId`, `serviceParams`, `serviceTokenAddress`,
`serviceTokenAmount` when `providerAgentId` is specified.

---

## Phase 3: ASP accept/reject (off-chain)

### ASP decision

After receiving the backend notification, ASP decides:
- **Accept** — calls `POST /{jobId}/asp/accept` with a price (using User's token)
- **Reject** — calls `POST /{jobId}/asp/reject`

### ASP accept rules

| Rule | Description |
|---|---|
| ASP must use User's token | ASP Apply uses the token specified by User's task |
| Backend validates token consistency | If Apply token != task token → error code returned |
| Reason | If tokens mismatch, User cannot successfully pay at accept time |

### Backend price check (automatic)

After ASP accepts, backend automatically checks:
```
ASP apply price <= User max_budget (paymentMostTokenAmount)?
  ├─ YES → system notification to buyer (price OK)
  └─ NO  → system notification to buyer (price exceeds budget)
```

This is a **backend-side judgment** — the buyer's LLM does NOT evaluate price.

### Events

#### `provider_applied` (ASP accepts)

Old: on-chain event from provider's `applyJob` tx.
New: **off-chain notification** — provider calls `POST /{jobId}/asp/accept`.

Event carries:
- `providerAgentId` — ASP that accepted
- `tokenAmount` — ASP's apply price
- `tokenSymbol` — token (must match User's task token)
- `overMostBudget` — **boolean**, backend price check result (`true` = price exceeds max_budget)

Buyer next-action (CLI auto-decides, LLM does NOT evaluate price):
- `overMostBudget == false` → proceed to Phase 4 (auto-accept + pay via CLI)
- `overMostBudget == true` → notify user "price exceeds budget" → switch ASP (see "Switch ASP flow")

#### `job_provider_reject` (ASP rejects)

Triggered when ASP calls `POST /{jobId}/asp/reject`.

Event payload:
```json
{
  "event": "job_provider_reject",
  "jobId": "0x...",
  "jobStatus": "open",
  "providerAgentId": "xxxx",
  "paymentMode": 1, "visibility": 1
}
```

Buyer next-action:
1. Notify user that ASP rejected
2. Present switch-ASP options (see "Switch ASP flow")

---

## Phase 4: User accept + escrow (on-chain)

### Decision: accept or not (automatic)

```
provider_applied event with overMostBudget field:
  ├─ overMostBudget == false → CLI auto-accepts: 创建A2A session → 上传附件(如有) → accept打款
  └─ overMostBudget == true  → CLI auto-rejects: notify user → switch ASP (无限换)
```

**Key design**: "User模型不思考，端上直接使用后端判断结果，调CLI打款"
— This is NOT a user decision point. The CLI reads the `overMostBudget` field from
the `provider_applied` event and auto-routes. No LLM deliberation, no user prompt.
- `overMostBudget == false` → CLI proceeds to accept + pay
- `overMostBudget == true` → CLI notifies user and enters switch-ASP flow

### Accept flow (single CLI call)

After deciding to accept, buyer executes a single CLI call that internally:
1. Calls `prePayTaskInfo` — get payment info (escrow contract, amounts, etc.)
2. Calls `preAccept` (EIP-712 signature)
3. Calls `accept` (ERC-3009 signature)
4. Broadcasts on-chain (`acceptJob` — binds final provider + escrow funds)

acceptJob allows one on-chain ASP change: the final provider can differ from createJob's.

### Pre-accept steps

**A2A session already exists** (created during `job_created` / `job_asp_selected`).

Steps before the accept CLI call:
1. If there are attachments → upload attachments to the existing A2A session
2. Execute accept CLI call (prePayTaskInfo → preAccept → accept → broadcast)

---

## Phase 5: ASP execution + delivery (post-accept)

> **Scope**: Phase 5 is **provider-side (ASP Skill) logic only**. Buyer does NOT implement
> any Phase 5 handlers. The buyer's flow ends at Phase 4 (accept + pay) and resumes when
> the provider submits a deliverable (`job_submitted` event → existing review lifecycle).

### Param check (provider-side)

After accept + escrow locked, ASP Skill checks:

```
ASP Skill引导确认前，判断所需参数是否满足?
  ├─ 是 → 系统通知ASP，干活 → A2A 交付结果
  └─ 否 → 两条路径:
       ├─ 在当前A2A协商流程里，沟通所需参数
       └─ 发起条款不明确A2A沟通流程 (完底流程，完全靠A2A协商)
```

### Three sub-paths (all provider-side)

| Condition | Path | Description |
|---|---|---|
| Params satisfied | Happy path | System notifies ASP to execute → ASP delivers result via A2A |
| Params partially missing | In-session negotiation | ASP communicates missing params within the existing A2A session |
| Terms unclear / complex | Full A2A negotiation | Initiate a full terms-negotiation A2A flow (bottom-up, entirely A2A-driven) |

### Delivery

ASP delivers via A2A → existing lifecycle continues (submit → review → complete/reject/dispute).

---

## Switch ASP flow (unlimited switching)

### Trigger scenarios

| Trigger | Action |
|---|---|
| `provider_reject` received | Present options to user |
| Backend price check fails (price > max_budget) | Present options to user |
| User declines to accept | Present options to user |
| User proactively says "switch ASP" | `user/reject` (if current ASP exists) |

### User choices (presented as decision prompt)

```
完整选项:
1. 获取ASP列表         → asp-match (with jobId) → user selects → fill params → set/asp
2. 指定ASP             → asp-match --provider <id> (with jobId) → fill params → set/asp
3. 把任务设置为Public   → setVisibility (off-chain, no signature)
   让ASP主动接单
4. 关闭任务             → close
```

**Unlimited switching**: "User继续找ASP（无限换）" — there is no limit on how many times
the user can switch ASP. Each switch re-enters Phase 3.

### `asp-match` parameter rules

| Context | `jobId` | `taskDesc` | Notes |
|---|---|---|---|
| Phase 1 (pre-publish, no task yet) | not available | **required** | No jobId exists yet |
| Switch ASP (task already exists) | **required** | optional | Backend can derive desc from jobId |
| General rule | pass if available | **required when no jobId** | At least one must be present |

### Events

#### `job_user_reject` (buyer rejects ASP)

Triggered when buyer calls `POST /{jobId}/user/reject`.

```json
{
  "event": "job_user_reject",
  "jobId": "0x...",
  "jobStatus": "open",
  "userAgentId": "xxxx",
  "paymentMode": 1, "visibility": 1
}
```

Notifications: sent to both ASP (you've been rejected) and user (rejection confirmed).

#### `job_asp_selected` (ASP switched via set/asp)

Triggered when buyer calls `POST /{jobId}/set/asp`.

Buyer handler (same pattern as `job_created`):
```
job_asp_selected event → buyer handler:
  CLI mode:
    1. session_query_exists(job_id, my_agent_id, new_provider_agent_id)
    2. session_create(job_id, my_agent_id, new_provider_agent_id)  → returns sessionKey
    3. session_send_by_job(job_id, new_provider_agent_id, "已切换ASP，等待响应")
    4. 通知用户"ASP已切换" (print to stdout)
    5. 结束 (end turn, wait for provider_applied / job_provider_reject)

  Playbook mode:
    1. xmtp_dispatch_user: 通知用户"ASP已切换"
    2. session_create (via okx_a2a.rs) with new provider
    3. 结束 (end turn)
```

Backend sends new order notification to the new ASP. Re-enters Phase 3.

---

## CLI commands — new/modified

### New commands

| Command | API | Description |
|---|---|---|
| `asp-match` | `POST /priapi/v1/aieco/task/asp/match` | Search matching ASPs (pre-publish, no jobId) |
| `set-asp` | `POST /{jobId}/set/asp` | Set/replace ASP + service; body: `{serviceId, serviceParams, servicePrice}` (no tokenSymbol/tokenAmount) |
| `reset-asp` | `POST /{jobId}/reset/asp` | Clear ASP + service fields |
| `user-reject` | `POST /{jobId}/user/reject` | User rejects current ASP |
| `asp-reject` | `POST /{jobId}/asp/reject` | ASP rejects a task (provider-side) — **already exists** |

### Modified commands

| Command | Change |
|---|---|
| `create-task` | Add serviceId/serviceParams/serviceTokenAddress/serviceTokenAmount fields |
| `setVisibility` | Off-chain now (remove signature/broadcast) |

### Deleted commands

| Command | Reason |
|---|---|
| `set-token-and-budget` | Not modifiable after creation |
| `set-max-budget` | Not modifiable after creation |
| `set-provider` | Replaced by `set/asp` |
| `save-agreed` | No more multi-round negotiation |
| `ack-to-confirm` | No more propose/ack/confirm handshake |

---

## Field modification rules

**Old**: token, budget, max_budget, ASP all modifiable before accept.

**New**: only ASP modifiable (via `set/asp`). Token/budget/max_budget are **NOT modifiable** after creation.

`set/asp` body: `{serviceId, serviceParams, servicePrice}` — does NOT carry `tokenSymbol` / `tokenAmount`.
Switching ASP does not change the task's payment token or budget.

---

## Removed logic

| Item | Replacement |
|---|---|
| 3-step handshake (`[intent:propose]`/`[intent:ack]`/`[intent:counter]`/`[intent:confirm]`) | ASP accept/reject model |
| `serviceHasPrice` branching in job_created handler | Single path — backend notifies ASP, ASP decides |
| Buyer-side price evaluation (LLM judges quote vs budget) | Backend auto-checks price <= max_budget |
| `negotiate_ack` / `negotiate_counter` event handlers | Removed |
| A2A session creation during job_created with negotiation messages | Simplified to `session_create` only (no negotiation, no designated-route) |
| `designated-route` for service discovery | `asp/match` (Phase 1) |

---

## setVisibility off-chain

Old: required wallet signature + on-chain broadcast.
New: direct DB update, no signature needed.

API: `POST /priapi/v1/aieco/task/{jobId}/setVisibility` — request body unchanged,
but response no longer contains `uopData`.

---

## x402 impact (minimal)

This change scopes to **escrow mode only**. x402 impact:

| Aspect | Impact |
|---|---|
| Phase 1 service discovery | **Affected** — `asp/match` replaces `recommend` + `designated-route`; `serviceType: "A2MCP"` identifies x402 |
| Phase 2 createTask | **Affected** — new fields (serviceId etc.) apply to x402 too |
| x402 execution (validate → pay → direct-accept) | **Not affected** — stays as-is |
| `set-payment-mode --payment-mode x402` | **Not affected** |
| `task-402-pay` / `direct-accept` | **Not affected** |

Adaptation needed: x402 entry path currently goes through `designated-route` → `branch_x402`.
The service discovery part (`designated-route`) gets replaced by `asp/match` (Phase 1),
but the execution part (`x402-validate` → `set-payment-mode` → `task-402-pay`) stays.

---

## Negotiation timeout

Backend strategy: **asp-deadline-warn** — triggers every **5 minutes** if ASP has not accepted/rejected.
Re-triggers `job_created` / `job_asp_selected` notification to ASP.

No CLI-side change needed; these events re-enter existing Phase 3 handlers.

---

## Implementation rules

1. **Playbook 精简** — 只写必要步骤，不写解释性文字、上下文回顾、注意事项堆砌。每一步只含本步骤的指令。
2. **确定性逻辑下沉 CLI** — 不需要 LLM 思考的步骤（价格判断、session 创建、字段填充）在 Rust CLI 里完成，playbook 只调一条 CLI 命令拿结果。
3. **Content 模板用 `fn(params) -> String`** — 已知值在 Rust 侧 `format!()` 填入，不留 `<placeholder>` 让 agent 填。
4. **CLI mode 优先** — `is_cli_mode()` 时尽量在 Rust 内完成（in-process），减少 playbook 步骤数。非 CLI mode 的 playbook 也要最小化。
5. **用户可见文字考虑多语言** — canonical English + `L10N_DISPATCH_SHORT` 提示 agent 翻译。

---

## Implementation plan — CLI/Skill changes

### P0 — New flow core

| # | Change | Files |
|---|---|---|
| 1 | New CLI: `asp-match` | `buyer/asp_ops.rs` → wire into TaskCommand + AgentCommand |
| 2 | New CLI: `set-asp` | `buyer/asp_ops.rs` → wire into TaskCommand + AgentCommand |
| 3 | New CLI: `reset-asp` | `buyer/asp_ops.rs` → wire into TaskCommand + AgentCommand |
| 4 | New CLI: `user-reject` | `buyer/asp_ops.rs` → wire into TaskCommand + AgentCommand |
| 5 | CLI: `asp-reject` (**exists**) | `provider/asp_reject.rs` — already wired into ProviderCommand + AgentCommand |
| 6 | Modify `createTask` — add service fields | `create.rs`, `buyer/mod.rs`, `agent_commerce/mod.rs` |
| 7 | Events: `JobAspSelected` (exists), rename `ProviderReject` parse → `job_provider_reject`, add `JobUserReject` parse `job_user_reject` | `state_machine.rs` |
| 8 | Rewrite `job_created` handler — session_create only | `flow_negotiate/match_provider.rs` |
| 9 | New handler: `job_asp_selected` (same pattern as job_created) | `flow_negotiate/match_provider.rs` + `flow.rs` routing |
| 10 | Modify `provider_applied` handler — `overMostBudget` field + auto-accept (**mostly exists**) | `flow_lifecycle/core.rs` |
| 11 | Handler: `job_provider_reject` (**exists** as `provider_reject`, rename event parse) | `flow_negotiate/events.rs` |
| 12 | New handler: `job_user_reject` | `flow.rs` + new handler |
| 13 | CLI: `confirm-accept` (**exists**, escrow signing flow) | `buyer/accept.rs` |

### P1 — Simplify/delete old flow

| # | Change | Files |
|---|---|---|
| 14 | Remove 3-step handshake (propose/ack/counter/confirm) | `flow_negotiate/events.rs` |
| 15 | Remove `negotiate_ack` / `negotiate_counter` handlers | `flow.rs`, `events.rs` |
| 16 | Delete CLI: `set-token-and-budget` | command + `available_actions` |
| 17 | Delete CLI: `set-max-budget` | command + `available_actions` |
| 18 | Delete CLI: `set-provider` (replaced by `set-asp`) | command + `available_actions` |
| 19 | Delete CLI: `save-agreed` / `ack-to-confirm` | command + routing |
| 20 | Simplify `setVisibility` — remove signature/broadcast | `changepublic.rs` |

### P2 — Adaptation

| # | Change | Files |
|---|---|---|
| 21 | Draft API: add service fields | existing command |
| 22 | Update content.rs templates (new events) | `content.rs` |
| 23 | x402 entry path: adapt service discovery from `designated-route` to `asp/match` | `designated.rs` |
| 24 | Update `available_actions(Status::Created)` | `flow.rs` |
| 25 | Post-accept param negotiation (Phase 5 logic) | new handler / skill update |

### Not changed

| Item | Reason |
|---|---|
| x402 execution (x402-validate → task-402-pay → direct-accept) | Out of scope (escrow only) |
| Post-submit lifecycle (submit → review → complete/reject/dispute) | "原有逻辑" per whiteboard |

---

## Appendix: Annotations from whiteboard

### Service params (Jeff Shan)

> 1. 服务描述新增ASP所需服务入参，User的Agent识别参数后，需要用户填完才能发布。
> 2. 后端需要加一个字段来保存，同一任务。
> 3. User发任务请求的时候，需要数据字段匹配。
> 3. User发任务的预算不与ASP所需币种，服务价格进行校验（省去User思考，币种让ASP匹配User）。
> 4. User最高预算需要和ASP的服务价格进行比较，User余额和最高预算需要进行比较，提示用户余额不足。

### ASP token matching (Jeff Shan)

> ASP接应User币种。ASP Apply使用User币种。
> 后端，接口对Apply币种进行校验，不一致返回错误码（因为如果apply不同币种最后，user无法成功付款）。

### Auto-accept (Jeff Shan)

> ASP apply价格小于等于User最高预算（User模型不思考，端上直接使用后端判断结果，调CLI打款）。
