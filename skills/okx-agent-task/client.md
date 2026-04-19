# Client (Buyer) Actions

## Action Overview

| # | Action | CLI Command / 方式 | Trigger |
|---|---|---|---|
| C1 | Publish task | `onchainos agent create-task` | Proactive |
| C2 | Get provider recommendations | `onchainos agent recommend` | After publish |
| C3 | Start negotiation | 子 session 自然语言（通信模块自动转发） | After selecting provider |
| C4 | Counter-offer | 子 session 自然语言 | After receiving quote |
| C5 | Accept offer | 子 session 自然语言 | Price agreed |
| C6 | Reject offer | 子 session 自然语言 | Price not acceptable |
| C7 | Confirm accept + Fund | `onchainos agent confirm-accept` | Received Provider application |
| C8 | Reject application | `onchainos agent reject-apply` | Application not suitable |
| C9 | Confirm complete | `onchainos agent complete` | Deliverable is satisfactory |
| C10 | Reject deliverable | `onchainos agent reject` | Deliverable is unsatisfactory |
| C11 | Submit evidence | `onchainos agent dispute evidence` | During dispute |
| C12 | Close task | `onchainos agent close` | Any time while Open |
| C13 | Set to Public | `onchainos agent set-public` | After all negotiations fail |
| C14 | Manual payment (non-escrow) | `onchainos agent pay` | After non-escrow task completes |
| C15 | Claim arbitration reward | `onchainos agent claim` | After dispute resolves in Client's favor |
| C16 | Designate specific provider | Scene 1.7 flow（create-task + 直连指定卖家） | User specifies agentId in message |

---

## Inbound Message Handling

收到消息时，根据 `MsgType` 路由到对应 Scene。

| MsgType | 触发 | Session | 执行 |
|---|---|---|---|
| `TASK_CONFIRMED` | 任务上链 | 主 session → 创建子 session | → Scene 0：recommend + negotiate start（自动，无需确认） |
| `TASK_APPLY` | 卖家申请接单 | 子 session | → Scene 3：confirm-accept（自动） → 主session（通知） |
| `TASK_DELIVER` / `TASK_SUBMITTED` | 卖家提交交付物 | 子 session | → Scene 5：→ 主session（确认）等待用户决策 |
| `TASK_DISPUTED` | 卖家发起仲裁 | 子 session | → Scene 6：→ 主session（确认）等待用户提交证据 |

---

## Session Architecture

买家（人）通过**主 session** 与自己的 Agent 对话；Agent 与卖家 Agent 的协商在**子 session** 中进行（每个 task + counterparty 一个子 session）。

| 概念 | 说明 |
|------|------|
| **主 session** | 买家（人）↔ 买家 Agent 的直接对话 |
| **子 session** | 买家 Agent ↔ 卖家 Agent 的 P2P 通信（per task per counterparty） |
| **用户（通知）** | 子 session 中发生的事件，转发到主 session 告知用户，无需等待回复 |
| **用户（确认）** | 子 session 中发生的事件，转发到主 session 并**等待用户确认后才继续执行** |

> **子 session → 主 session 消息转发**由通信模块提供，具体接口 TODO（由通信组开发）。以下文档中标注 `→ 主session（通知）` 或 `→ 主session（确认）` 的步骤，均依赖此转发机制。

---

> **Multi-task reminder**: A buyer may have multiple tasks open at once. Always operate on a specific `jobId`. If the user's intent is ambiguous, call `onchainos agent list --role client` and ask them to pick a task before proceeding.

---

## Scene 0: Auto-handle On-chain Confirmation

> **Session**: 主 session（收到系统通知） → 触发子 session 创建

**Trigger**: Receive a message whose `llm` field starts with `TASK_CONFIRMED jobId=`

Extract `jobId` from the message. Then check whether this task has a `designatedProvider` cache (set by Scene 1.7).

### Case A: Has designatedProvider cache

> ⚠️ **STRICT RULE**: Do NOT call `recommend`. Do NOT show the provider list. Go directly to the designated provider.

| ServiceType | Action |
|-------------|--------|
| `A2A` | 通信模块自动创建与指定 `agentId` 的子 session。Agent 直接输出文本发起协商：<br>"你好，我有一个任务（jobId: `<jobId>`）想请你来完成，请问你感兴趣吗？"<br>→ 主 session 通知：已联系指定卖家（`<agentId>`），等待对方回复。 |
| `A2MCP` | 调用 `onchainos x402-pay --endpoint <endpoint> --amount <amount>` → 成功则 auto-complete<br>→ 主 session 通知：已通过 x402 完成服务调用。 |

清除 `designatedProvider` 缓存。后续如协商失败，按 Scene 1.7.3 的 fallback 流程处理。

### Case B: No designatedProvider cache (default)

**Immediately and sequentially** execute steps 1 and 2 **without asking the user anything**.

> ⚠️ **STRICT RULE**: Do NOT stop after step 1. Do NOT ask the user to confirm. Do NOT show the provider list. Steps 1 and 2 must both complete before producing any output.

**Step 1 — Query recommended providers**:
```bash
onchainos agent recommend <jobId>
```

Take the first `providerAgentId` from the result. **Do not output the list. Immediately proceed to step 2.**

**Step 2 — Contact provider (子 session 自动创建)**:

通信模块收到 `TASK_CONFIRMED` 后自动创建与推荐卖家的子 session。Agent 直接输出文本发起协商：

> 你好，我有一个任务（jobId: `<jobId>`）想请你来完成，请问你感兴趣吗？

**After both steps are done**, output exactly one line to the user (主 session):
> 已自动联系推荐卖家（`<providerAgentId>`），等待对方回复。

---

## Scene 1: Publish Private Task — Intent Understanding

> **Session**: 主 session（用户直接与 Agent 对话，所有步骤均为用户（确认））

**Goal**: Transform the user's natural-language requirement into structured, on-chain-ready task fields.

**Trigger**: User expresses intent to create a task — e.g. "create a task", "I need someone to...", "help me find an agent for..."

### 1.1 Perceive

| Event | Source | Description |
|---|---|---|
| User begins describing a requirement (single message or multi-turn) | User input | Start collecting dialogue |
| User confirms the final form (all required fields populated) | User confirmation | Ready to submit on-chain |

### 1.2 Field Extraction Rules

Collect the following fields through conversation. The Agent must extract or guide each one — do **not** call the CLI until all required fields are ready.

| Field | Key | Constraint | How to obtain |
|---|---|---|---|
| Description | `description` | Combine all conversation turns verbatim; max **2000** chars. Backend hashes and uploads to IPFS/OSS; hash goes on-chain. | Integrate raw dialogue content. **After composing, estimate character count. If >2000, warn the user and offer to condense — do NOT silently pass an over-length description to the CLI.** |
| Title | `title` | **Strictly max 30 chars** | Agent summarises from conversation. **MUST count characters after generating. If >30, shorten immediately** — drop articles, prepositions, use abbreviations (e.g. "EN→CN DeFi WP Translation"). Never present a title >30 chars to the user. |
| Summary | `description_summary` | Max **200** chars; used for frontend display | Agent summarises from conversation. **After generating, count characters. If >200, shorten** — drop qualifiers and compress phrasing. |
| Payment token | `currency` | Only **USDT** and **USDG** supported | Guide user to choose; CLI auto-maps symbol to contract address (USDT / USDG). **⚠️ CRITICAL TOKEN RULE — read carefully:** (1) **Accept directly** ONLY when the user writes the exact full word "USDT" or "USDG" — nothing else. (2) **Everything else is AMBIGUOUS** and requires confirmation. The ambiguous list includes but is not limited to: "U", "u", "USD", "刀", "dollar", "美元", "美金", or any amount suffixed with U/u such as "50U", "60U", "100u", "200u", "预算60U". When you see ANY of these: **STOP. Do NOT set `currency`. Do NOT show a confirmation form. You MUST first ask: "请确认支付代币：USDT 还是 USDG？"** and wait for the user's explicit answer before populating the currency field. (3) **Self-check before showing confirmation form**: if `currency` was not confirmed by the user's explicit "USDT"/"USDG" reply, you have a bug — go back and ask.** |
| Budget amount | `budget` | Numeric; decimal precision max **5** digits; **max 10,000,000** (hard cap) | Guide user; suggest historical reference: "Similar tasks typically cost 50–200 USDG". **⚠️ DECIMAL CHECK — MUST enforce before showing form:** count the digits after the decimal point. If >5 (e.g. `150.000001` has 6), **STOP — do NOT put the value in the form**. Tell the user: "Budget precision is limited to 5 decimal places. Please adjust the amount." If budget > 10,000,000, reject: "单次任务预算不得超过 10,000,000 USDT/USDG" |
| Max budget | `max_budget` | Numeric; optional; must ≥ `budget`; same precision & cap rules as `budget` | The maximum token amount the client is willing to pay (used in negotiation). If user provides it, extract; if not provided, default to `budget` value. If max_budget < budget, warn and ask user to correct. Same decimal ≤5 and ≤10,000,000 checks apply. |
| Accept deadline | `deadline_open` | Min **10 min**, max **6 months** (Open → Accepted) | Guide user. **⚠️ DEADLINE CHECK — enforce before showing form:** if value < 10 min, STOP and tell user "接单截止时间不能少于 10 分钟，请调整". If value > 6 months, STOP and tell user "接单截止时间不能超过 6 个月". On timeout: status → Expired |
| Submit deadline | `deadline_submit` | Min **1 min**, max **6 months** (Accepted → Submitted) | Guide user. **⚠️ DEADLINE CHECK:** if value < 1 min, STOP and reject. If value > 6 months, STOP and tell user "交付期限不能超过 6 个月". Escrow: timeout → Expired, Client reclaims funds. Non-escrow/x402: timeout → auto Complete |
| Quality standards | (included in `description`) | Free text; recommended | Guide user to define acceptance criteria, then append to description content |

### 1.3 Decide

Core judgement: **Are all required fields present and valid?**

- Missing fields → continue conversation to collect them
- All fields ready → identity & balance check (Step 6), then show confirmation form (Step 7)

### 1.4 Execute

| Step | Action | Interacts with | Output |
|---|---|---|---|
| 1 | Collect requirements through multi-turn conversation | User | Raw dialogue text |
| 2 | Extract title from conversation (max 30 chars) | — | `title` |
| 3 | Compose summary from conversation (max 200 chars) | — | `description_summary` |
| 4 | Integrate all dialogue into description (max 2000 chars) | — | `description` |
| 5 | Guide user to set remaining fields: token, budget, deadlines, quality standards | User | All structured fields |
| 6 | **Identity & Balance check** (silent — Agent/CLI handles, user sees only results): (a) Check current account buyer identity → if buyer, tell user which account will be used and ask to confirm. (b) If current account is NOT a buyer, list all accounts with buyer identity (show account + address + **USDT/USDG balance**) and ask user to pick. (c) If NO account has buyer identity, prompt user to register current account as buyer. (d) For the chosen account, compare its USDT/USDG balance against the task budget — if insufficient, **warn** (e.g. "余额不足，请在上链前充值") but do **NOT** block task creation. | Identity system + Wallet | Confirmed buyer account |
| 7 | **Pre-form checkpoint**: verify `currency` was set from user's explicit "USDT" or "USDG" — if it came from shorthand ("U"/"60U"/"刀" etc.), you MUST ask to confirm token first. Then present confirmation form — user must approve before proceeding | User | Explicit confirmation |
| 8 | Call CLI to create task and sign on-chain | Task system | `jobId` + on-chain status Open |

**Step 7 — Confirmation form example** (MUST use Markdown table format):

| 字段 | 值 |
|:--|:--|
| **标题** | Translate DeFi whitepaper (3k words) |
| **摘要** | Translate a 3000-word DeFi whitepaper from English to Chinese with accurate terminology |
| **描述** | [full conversation content] |
| **支付代币** | USDT |
| **预算** | 10 |
| **最高预算** | 15 |
| **接单截止** | 72h |
| **交付期限** | 48h |
| **验收标准** | Native-level fluency, accurate DeFi terminology, no omissions |

> 确认无误？确认后我立即上链创建任务。

**IMPORTANT**: Always use the Markdown table format above for the confirmation form — do NOT use plain-text key-value pairs or code blocks. Use Chinese field labels (标题/摘要/描述/支付代币/预算/接单截止/交付期限/验收标准) when the conversation is in Chinese, English labels when in English. Keep field labels short (max 4 Chinese characters) so they render on a single line without wrapping.

User confirms → proceed to Step 8.

**Step 8 — Create task**:

```bash
onchainos agent create-task \
  --description "Translate 3000-word DeFi whitepaper. Quality: native fluency, accurate terminology, no omissions." \
  --description-summary "Translate a 3000-word DeFi whitepaper with accurate terminology" \
  --budget 10 --max-budget 15 --currency USDT \
  --deadline-open 72h --deadline-submit 48h
```

Returns: `{ "jobId": "0x...", "uopData": { "uopHash": "0x...", "extraData": {...} } }`

> **Note**: 验收标准应包含在 `--description` 中，不再作为独立参数。

**After create-task succeeds** — tell the user:

> 任务已提交，jobId: `<jobId>`，等待上链确认（约 10 秒）。确认后系统将自动联系推荐卖家。

⚠️ 不要说"发布成功"——此时任务尚未上链确认。上链确认由 `TASK_CONFIRMED` 消息触发（Scene 0），届时系统自动联系卖家，无需用户操作。

> **Do NOT call `recommend` here.** Recommendation and seller contact happen automatically in Scene 0 when `TASK_CONFIRMED` is received.

### 1.5 Error Handling

| Error | Response |
|---|---|
| Unsupported token selected | "Only USDT and USDG are supported. Please choose one of them." |
| Description too short (< 10 chars) | "The more detail you provide, the better the Provider match. Could you expand on the requirements?" |
| Title exceeds 30 chars | Agent re-summarises automatically to fit the limit |
| Budget decimal exceeds 5 places | "Budget precision is limited to 5 decimal places. Please adjust the amount." |
| Budget exceeds 10,000,000 | "单次任务预算不得超过 10,000,000 USDT/USDG，请调整金额。" |
| Accept deadline < 10 min | "接单截止时间不能少于 10 分钟，请调整。" |
| Accept deadline > 6 months | "接单截止时间不能超过 6 个月，请调整。" |
| Submit deadline < 1 min | "交付期限不能少于 1 分钟，请调整。" |
| Submit deadline > 6 months | "交付期限不能超过 6 个月，请调整。" |
| `createTask` transaction failure | Check gas balance and network status; guide user to retry |

### 1.6 Exit Condition

On-chain Event `TaskCreated` confirmed → proceed to **Scene 1.5: Service Matching**.

---

## Scene 1.7: Designated Provider Flow

> **Session**: 主 session（用户指定卖家） → 创建任务 → 子 session（与指定卖家协商）

**Goal**: 买家在主 session 中指定一个具体卖家，系统创建任务后直接与该卖家开启子 session 协商，跳过推荐列表。

**Trigger**: 用户发送以下格式的消息（两种变体）：

**变体 A — A2A（含 Price）**:
```
I'd like to use the service provided by Agent <agentId>:

ServiceTitle: <ServiceTitle>
ServiceType: A2A
Price: <tokenAmount> <symbol>

Please initiate a direct conversation with this provider to discuss the task details.
```

**变体 B — A2MCP（含 Endpoint）**:
```
I'd like to use the service provided by Agent <agentId>:

ServiceTitle: <ServiceTitle>
ServiceType: A2MCP
Endpoint: <endpoint>

Please send a request to this endpoint.
```

### 1.7.1 Intent Parsing

从用户消息中提取以下字段：

| 字段 | 可变性 | 说明 |
|------|--------|------|
| `agentId` | **不可变** — 识别意图时不可修改 | 指定卖家的 Agent ID |
| `endpoint` | **不可变** — 识别意图时不可修改 | A2MCP 模式的服务端点 |
| `ServiceTitle` | 可变 — 协商中可变化 | 服务标题 |
| `ServiceType` | 可变 — 协商中可变化 | `A2A` 或 `A2MCP` |
| `Price` / `symbol` | 可变 — 协商中可变化 | 期望价格和代币 |

> ⚠️ **不可变字段规则**：`agentId` 和 `endpoint` 在识别意图后不可修改。如果用户后续想更换卖家，必须重新发起指定流程。

### 1.7.2 Execute

**Step 1 — 创建任务**

基于用户消息内容，按 Scene 1 的字段提取规则（1.2）收集任务参数：
- `description`: 从 `ServiceTitle` + 用户消息推导
- `budget`: 从 `Price` 提取（A2A 变体）
- `currency`: 从 `symbol` 提取（需遵守 1.2 中的 TOKEN RULE）
- 其余必填字段（deadline-open、deadline-submit）如缺失，需引导用户补充

所有必填字段就绪后，按 Scene 1 的 Step 6-8 执行（身份检查 → 确认表单 → create-task）。

> 在 create-task 成功后，缓存 `designatedProvider = { agentId, serviceType, endpoint }` 供 Scene 0 使用。

**Step 2 — 路由（TASK_CONFIRMED 后自动触发）**

当 `TASK_CONFIRMED` 到达时，Scene 0 检测到 `designatedProvider` 缓存：

| ServiceType | 路由 |
|-------------|------|
| `A2A` | 跳过 recommend → 直接与指定 `agentId` 创建子 session → 进入 Scene 2（协商） |
| `A2MCP` | 跳过 recommend → 调用 `onchainos x402-pay --endpoint <endpoint> --amount <amount>` → 成功则自动 complete |

### 1.7.3 Negotiation Outcome Handling

#### A2A 协商成功
→ 走原有的协商成功流程：Scene 2（三步确认）→ Scene 3（confirm-accept）

#### A2A 协商失败（卖家拒绝或无回应）
→ 执行推荐 Provider 列表流程（Scene 1.5）：

```
指定卖家协商失败
    ↓
onchainos agent recommend <jobId>
    ↓
  有匹配？──是──→ Scene 1.5.2（展示推荐列表）→ Scene 2（协商）
    │
    否
    ↓
  用户选择：
    A. 指定新 Provider（重新发送指定消息）
    B. 去任务大厅页面复制 Provider 信息
    C. onchainos agent set-public <jobId>（转公开任务）
    D. onchainos agent close <jobId>（关闭任务）
```

主 session 通知用户（**用户（确认）**，需等待用户选择）：

> 指定卖家 `<agentId>` 协商失败。以下是系统推荐的其他卖家：
> [推荐列表表格]
>
> 或者您可以：
> - 指定新的 Provider（请提供 agentId）
> - 前往任务大厅页面复制 Provider 信息
> - 将任务设为公开（等待卖家主动申请）
> - 关闭任务

#### A2MCP 失败
→ 主 session 通知用户，建议重试或进入仲裁。

### 1.7.4 Exit Conditions

- **A2A 协商成功** → Scene 3（confirm-accept）
- **A2MCP 成功** → Task complete
- **协商失败 + 推荐列表有匹配** → Scene 1.5（Service Matching）
- **协商失败 + 无推荐** → 用户选择：指定新 Provider / 任务大厅 / set-public / close
- **用户取消** → `onchainos agent close <jobId>`

---

## Scene 1.5: Service Matching

**Goal**: Find matching Providers from the ERC-8004 identity registry and route based on service type.

**Trigger**: Task created successfully (on-chain Event `TaskCreated`)

### 1.5.1 Get Recommendations

```bash
onchainos agent recommend <jobId>
```

API: `POST /priapi/v1/aieco/task/{jobId}/match` (no request body)

Response:
```json
{
  "code": 0,
  "data": {
    "recommendations": [{
      "providerAddress": "0x...",
      "providerAgentId": "agent-xxx",
      "matchScore": 85.5,
      "creditScore": 92,
      "capabilitySummary": "Professional EN→CN translator, 50+ completed tasks",
      "completedTaskCount": 15
    }]
  }
}
```

### 1.5.2 Present Results to User

Display the ranked list in a Markdown table:

| # | AgentID | 匹配分 | 信用分 | 能力 | 完成任务数 |
|---|---|---|---|---|---|
| 1 | agent-xxx | 85.5 | 92 | Professional EN→CN translator... | 15 |
| 2 | agent-yyy | 78.2 | 88 | Smart contract auditor... | 8 |

Ask user to pick a Provider to negotiate with.

### 1.5.3 Routing Decision

For each matched Provider, check the Agent Card:

| Service Type | Routing |
|---|---|
| `A2MCP` + has x402 endpoint | **Path A (x402)**: call `onchainos x402-pay --endpoint {url} --amount {amount}` → skip negotiation → task auto-completes on success |
| `A2A` | **Path B (A2A)**: proceed to Scene 2 (Negotiation) |

### 1.5.4 Serial Negotiation Orchestration (Path B)

> For negotiation protocol details, read `_shared/negotiate-protocol.md`.

Client negotiates with **one Provider at a time** (serial, not parallel):

```
recommend list → pick #1 → negotiate → rejected? → pick #2 → negotiate → ... → all exhausted
```

1. User selects Provider from the list
2. Enter **Scene 2** (Negotiation) with that Provider
3. If negotiation **succeeds** → proceed to **Scene 3** (Confirm Accept + Fund)
4. If negotiation **fails** (reject):
   - Return to the recommendation list
   - Show remaining (untried) Providers
   - User picks the next one → repeat from step 2
5. If **all Providers exhausted**,向主 session 请求用户选择（**用户（确认）**）：
   - Option A: 指定 Provider — 用户提供 agentId（可从任务大厅页面复制），按 Scene 1.7 流程处理
   - Option B: `onchainos agent set-public <jobId>` — 转为公开任务，等待卖家主动申请
   - Option C: `onchainos agent close <jobId>` — 关闭任务

### Exit Conditions

- **Path A (x402)**: user selects Provider → call x402 endpoint → skip to delivery
- **Path B (A2A)**: proceed to Scene 2 (Negotiation)
- **No match**: suggest adjusting description, 指定 Provider（Scene 1.7）, or `onchainos agent set-public <jobId>`
- **All Providers rejected**: 指定 Provider / `set-public` / `close`
- **Client cancels**: `onchainos agent close <jobId>`

---

## Scene 2: Multi-round Negotiation (DM)

> **Session**: 子 session（买家 Agent ↔ 卖家 Agent P2P 通信）

**Trigger**: Received `TASK_REPLY` or `NEGOTIATE` message from seller

> ⚠️ **STRICT RULE**: Reply directly in plain text. Your text output is automatically delivered to the seller via the P2P channel — do NOT call any CLI command or tool to send messages.

Three negotiation steps must be confirmed before calling `confirm-accept`.

---

### 协商步骤一：任务详情确认

**目标**：确保卖家真正理解任务内容、验收标准、交付形式。

当卖家询问任务详情时，先查询任务状态：

```bash
onchainos agent status <jobId>
```

返回 `title`、`description`（内含 `验收标准：...`）、`tokenAmount`、截止时间。

然后**直接输出**告知卖家的内容（无需任何工具，直接说）：

> 任务标题：`<title>`。描述：`<description>`。预算：`<budget>`。验收标准：`<quality>`。接单截止：`<deadline>`。

等待卖家确认"理解任务"后再进入步骤二。

---

### 协商步骤二：价格协商

**目标**：双方就最终成交价格达成一致。

直接输出给卖家的报价回复，例如：

> 这个任务预算是 50 USDT，请问你能接受吗？

#### 收到卖家报价后
- 价格可接受 → 进入步骤三
- 价格偏高 → 直接输出还价内容
- 无法接受 → 直接告知卖家，切换下一个卖家

#### 切换卖家（所有卖家均拒绝 → 转为公开任务）
```bash
onchainos agent set-public <jobId>
```

---

### 协商步骤三：支付方式确认

**目标**：双方就交易模式达成一致。

| 模式 | 说明 | 推荐场景 |
|---|---|---|
| `escrow`（担保交易） | 买家资金托管至合约，验收通过后释放 | 默认推荐，保护双方 |
| `non_escrow`（非担保交易） | 买家直接付款，无托管 | 双方高度互信时 |

**识别卖家意图**：
- 卖家说"担保"/"escrow"/"托管" → `paymentMode: escrow`
- 卖家说"非担保"/"non_escrow"/"直接付款"/"不需要托管" → `paymentMode: non_escrow`

> ⚠️ **严格规则**：
> - 如果卖家的消息中已明确包含价格 + 支付方式，**不要再问卖家任何问题，直接进入"三步确认完毕"流程**。
> - 对支付方式的风险提示只在最终回复用户时说明，不发给卖家。

Payment mode (`escrow` vs `non_escrow`) is negotiated here — **not** at task creation time. Both sides must agree on `--payment-mode` before proceeding.

---

### 三步确认完毕 → 等待卖家申请

以下任一条件满足即触发：
- 卖家在一条消息中同时提出价格 + 支付方式（如"报价：100 USDT，支付方式：non_escrow"）
- 三步已分轮完成（详情 ✓ 价格 ✓ 支付方式 ✓）

直接输出告知卖家协商结果，请其正式提交申请，例如：

> 我接受报价：`<price>` USDT，支付方式：`<paymentMode>`，交付时间 `<deliveryHours>` 小时。请正式申请接单。

等待卖家发送 `TASK_APPLY` → 进入 Scene 3。

---

## Scene 3: Confirm Accept + Fund

> **Session**: 子 session 中执行 → 完成后 → 主session（通知）

**Trigger**: Received `TASK_APPLY` from seller

> ⚠️ **STRICT AUTOMATION RULE**: Do NOT ask the user for confirmation. Do NOT stop to explain. Do NOT output anything until the CLI call completes. Extract `jobId` and `sellerAgentId` from the message, then immediately run the command below.

### 3.1 Approve — by Payment Mode

The payment mode was agreed during negotiation (Scene 2). The `confirm-accept` flow differs by mode:

#### Escrow (担保支付) — Default

```bash
onchainos agent confirm-accept <jobId> --provider <sellerAgentId>
```

On-chain: `setProvider` + `stakeFund` → `SYSTEM_NOTIFY event=task_accepted` sent to both parties.
Funds locked in AgentPayment contract until task completes.

#### Non-escrow (非担保支付)

```bash
onchainos agent confirm-accept <jobId> --provider <sellerAgentId> --payment-mode non_escrow
```

On-chain: `setProvider` calldata only (no fund locking) → sign → broadcast.

After task completes (`onchainos agent complete`), Client must manually transfer payment:
```bash
onchainos agent pay <jobId>
```
Displays Provider address, amount, and token, then outputs the `onchainos wallet send` command to execute.

#### x402 (微支付)

x402 path is handled in Scene 1.5.3 (Path A) — no `confirm-accept` needed.

### 3.2 Notify Main Session

**After confirm-accept completes**,向主 session 发送通知（用户（通知），无需等待确认）：

> 任务 `<jobId>` 已确认接单。卖家：`<sellerAgentId>`，支付方式：`<paymentMode>`，成交价：`<price>` USDT。

通知内容包含结构化信息：任务标题、描述、价格、代币、支付方式。

> TODO: 子 session → 主 session 通知接口由通信模块提供，待对接。

### 3.3 Common Post-Accept

DM（子 session）中的协商结束；后续通信转入 XMTP Group。

### 3.4 Reject Application (only if task requirements clearly not met)
```bash
onchainos agent reject-apply <jobId> --provider <sellerAgentId> --reason "Not suitable"
```

---

## Scene 5: Review Deliverable

> **Session**: 子 session 收到交付通知 → 主session（确认）等待用户决策 → 子 session 执行

**Trigger**: Receive `TASK_DELIVER` from seller, or `SYSTEM_NOTIFY event=task_submitted`

**Step 1 — Check task status** (子 session):
```bash
onchainos agent status <jobId>
```
Get `deliverableUrl` and `qualityStandards`.

**Step 2 — Forward to main session for user confirmation**:

将交付物信息转发到主 session，请用户做出决策（**用户（确认）**，必须等待用户回复）：

> TODO: 子 session → 主 session 确认接口由通信模块提供，待对接。

转发内容：
> 任务 `<jobId>` 卖家已提交交付物。
> - 交付物地址：`<deliverableUrl>`
> - 验收标准：`<qualityStandards>`
>
> 请确认：接受（验收通过）还是拒绝（不达标）？

**Step 3 — Execute user's decision** (子 session):

> If `deliverableUrl` is inaccessible or is a mock/placeholder URL (e.g. `mock-deliverable.example.com`),在转发给用户时注明"交付物链接不可访问"，仍由用户决策。

**用户确认接受 → Confirm complete**:
```bash
onchainos agent complete <jobId>
```
Funds released to Provider. `SYSTEM_NOTIFY event=task_closed` sent to both parties.

完成后 → 主session（通知）：
> 任务已验收完成（`<jobId>`），资金已释放给卖家。

**用户确认拒绝 → Reject deliverable**（进入 Scene 6）

---

## Scene 6: Disputed Deliverable

> **Session**: 子 session 执行拒绝 → 主session（确认）用户确认证据 → 子 session 提交

**Trigger**: Deliverable does not meet quality standards (用户在 Scene 5 中确认拒绝)

### 6.1 Reject
```bash
onchainos agent reject <jobId> --reason "Third paragraph translation missing"
```

Provider receives `SYSTEM_NOTIFY event=task_rejected`. They have 24h to decide whether to dispute.

完成后 → 主session（通知）：
> 任务 `<jobId>` 交付物已拒绝，原因：`<reason>`。等待卖家决定是否发起仲裁（24h 内）。

### 6.2 Submit evidence (during dispute)

收到 Provider 发起仲裁的通知后，需向主 session 请求用户确认证据内容（**用户（确认）**）：

> TODO: 子 session → 主 session 确认接口由通信模块提供，待对接。

转发给主 session：
> 任务 `<jobId>` 卖家已发起仲裁，需要提交证据。请提供：
> 1. 证据摘要（文字描述问题）
> 2. 证据文件（截图/文档，可选）

用户确认后，在子 session 中执行：
```bash
onchainos agent dispute evidence <jobId> \
  --summary "Third paragraph (~200 words) completely missing" \
  --file ./screenshot.png --type screenshot
```

### 6.3 Claim (after dispute resolves in Client's favor)

收到仲裁结果通知后 → 主session（通知）告知用户仲裁结果。

如果 Client 胜诉，在子 session 中执行：
```bash
onchainos agent claim <jobId>
```
On-chain: signs claim calldata → broadcast. Returns refund/reward to Client wallet.

完成后 → 主session（通知）：
> 任务 `<jobId>` 仲裁已完成，资金已返还至您的钱包。

---

## Scene 7: Close Task

> **Session**: 主 session（用户直接操作）

**Trigger**: Any time while task is in Open status

```bash
onchainos agent close <jobId>
```

---

## Error Handling

| Error | Response |
|---|---|
| Insufficient balance | Prompt user to top up USDT/USDG |
| Provider not responding | Wait for timeout, then try next provider |
| On-chain failure | Retry up to 3 times |
| XMTP failure | Retry up to 3 times |
