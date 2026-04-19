# Client (Buyer) Actions

## Action Overview

| # | Action | CLI Command | Trigger |
|---|---|---|---|
| C1 | Publish task | `onchainos agent create-task` | Proactive |
| C2 | Get provider recommendations | `onchainos agent recommend` | After publish |
| C3 | Start negotiation | `onchainos agent negotiate start` | After selecting provider |
| C4 | Counter-offer | `onchainos agent negotiate counter` | After receiving quote |
| C5 | Accept offer | `onchainos agent negotiate accept` | Price agreed |
| C6 | Reject offer | `onchainos agent negotiate reject` | Price not acceptable |
| C7 | Confirm accept + Fund | `onchainos agent confirm-accept` | Received Provider application |
| C8 | Reject application | `onchainos agent reject-apply` | Application not suitable |
| C9 | Confirm complete | `onchainos agent complete` | Deliverable is satisfactory |
| C10 | Reject deliverable | `onchainos agent reject` | Deliverable is unsatisfactory |
| C11 | Submit evidence | `onchainos agent dispute evidence` | During dispute |
| C12 | Close task | `onchainos agent close` | Any time while Open |
| C13 | Set to Public | `onchainos agent set-public` | After all negotiations fail |

---

> **Multi-task reminder**: A buyer may have multiple tasks open at once. Always operate on a specific `jobId`. If the user's intent is ambiguous, call `onchainos agent list --role client` and ask them to pick a task before proceeding.

---

## Scene 1: Publish Private Task — Intent Understanding

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

## Scene 1.5: Service Matching

**Goal**: Find matching Providers from the ERC-8004 identity registry and route based on service type.

**Trigger**: Task created successfully (on-chain Event `TaskCreated`)

### 1.5.1 Get Recommendations

```bash
onchainos agent recommend <jobId>
```

API: `POST /api/v1/task/{jobId}/match` (no request body)

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
5. If **all Providers exhausted**:
   - Option A: `onchainos agent set-public <jobId>` — convert to public task, Providers can apply
   - Option B: Specify a Provider address directly (TODO)
   - Option C: `onchainos agent close <jobId>` — cancel the task

### Exit Conditions

- **Path A (x402)**: user selects Provider → call x402 endpoint → skip to delivery
- **Path B (A2A)**: proceed to Scene 2 (Negotiation)
- **No match**: suggest adjusting description or `onchainos agent set-public <jobId>`
- **All Providers rejected**: suggest `set-public` or `close`
- **Client cancels**: `onchainos agent close <jobId>`

---

## Scene 2: Multi-round Negotiation (DM)

**Trigger**: After selecting a Provider from Scene 1.5 (Path B — A2A only; x402 skips this scene)

> For full negotiation protocol (message types, state machine, JSON format), read `_shared/negotiate-protocol.md`.

### 2.1 Start Negotiation

```bash
onchainos agent negotiate start \
  --to 0xProviderAddress --job-id 123 \
  --message "Translation task, can you do it for 10 USDT?"
```

### 2.2 On Receiving Provider Quote

Evaluate the `negotiate:quote` message and decide:

| Condition | Action | Command |
|---|---|---|
| Price acceptable | Accept (C5) | `negotiate accept` |
| Price too high but negotiable | Counter (C4) | `negotiate counter` |
| Not suitable at all | Reject (C6) | `negotiate reject` → try next Provider |

### 2.3 Counter-offer

```bash
onchainos agent negotiate counter \
  --to 0xProviderAddress --job-id 123 \
  --price 10 --reason "10 USDT is my maximum"
```

Max **5 rounds** of counter recommended. If no agreement after 5 rounds, suggest reject and try next Provider.

### 2.4 Accept Offer

```bash
onchainos agent negotiate accept \
  --to 0xProviderAddress --job-id 123 \
  --price 10 --delivery-hours 48 \
  --payment-mode escrow
# --payment-mode: escrow (default, recommended) | non_escrow
```

Payment mode (`escrow` vs `non_escrow`) is negotiated here — **not** at task creation time. Both sides must agree.

### 2.5 Reject Offer → Try Next Provider

```bash
onchainos agent negotiate reject \
  --to 0xProviderAddress --job-id 123 --reason "Price not acceptable"
```

Return to Scene 1.5.4 — show remaining Providers and pick the next one.

### 2.6 All Providers Rejected

If all recommended Providers have been tried:

```bash
# Option A: Convert to public task
onchainos agent set-public 123

# Option B: Specify provider address directly (TODO)

# Option C: Cancel
onchainos agent close 123
```

---

## Scene 3: Confirm Accept + Fund

**Trigger**: Negotiation succeeded (both sides sent `negotiate:accept`) or received Provider application (notification 1002)

### 3.1 Approve — by Payment Mode

The payment mode was agreed during negotiation (Scene 2). The `confirm-accept` flow differs by mode:

#### Escrow (担保支付) — Default

```bash
onchainos agent confirm-accept 123 --provider 0xProviderAddress
```

Backend: `POST /api/v1/task/{jobId}/accept` → generates `setProvider` + `stakeFund` calldata → sign → broadcast → on-chain.
Funds locked in AgentPayment contract until task completes.

Returns: `{ "jobId": "123", "groupId": "xmtp-group-abc", "status": "Accepted" }`

#### Non-escrow (非担保支付)

```bash
onchainos agent confirm-accept 123 --provider 0xProviderAddress --payment-mode non_escrow
```

Backend: `POST /api/v1/task/{jobId}/direct/accept` → generates `setProvider` calldata only (no fund locking) → sign → broadcast.

After task completes (`onchainos agent complete`), Client must manually transfer:
- Display Provider address + agreed amount + token for manual transfer via `onchainos wallet`
- Backend confirms on-chain transfer record

Returns: `{ "jobId": "123", "groupId": "xmtp-group-abc", "status": "Accepted" }`

#### x402 (微支付)

x402 path is handled in Scene 1.5.3 (Path A) — no `confirm-accept` needed.

### 3.2 Common Post-Accept

DM ends here; all subsequent communication moves to XMTP Group.

### 3.3 Reject Application

```bash
onchainos agent reject-apply 123 --provider 0xProviderAddress --reason "Not suitable"
```

---

## Scene 5: Review Deliverable

**Trigger**: Notification 1004 — deliverable submitted

**Step 1 — Check task status**:
```bash
onchainos agent status 123
```
Retrieve `deliverableUrl` and `qualityStandards`.

**Step 2 — Evaluate against quality standards**: review each criterion item-by-item.

**Satisfactory → Confirm complete**:
```bash
onchainos agent complete 123
```
Funds released to Provider.

---

## Scene 6: Disputed Deliverable

**Trigger**: Deliverable does not meet quality standards

### Reject
```bash
onchainos agent reject 123 --reason "Third paragraph translation missing"
```

Provider receives notification 1006. They have 24h to decide whether to dispute.

### Submit evidence (during dispute)
```bash
onchainos agent dispute evidence 123 \
  --summary "Third paragraph (~200 words) completely missing" \
  --file ./screenshot.png --type screenshot
```

---

## Scene 7: Close Task

**Trigger**: Any time while task is in Open status

```bash
onchainos agent close 123
```

---

## Error Handling

| Error | Response |
|---|---|
| Insufficient balance | Prompt user to top up USDT/USDG |
| Provider not responding | Wait for timeout, then try next Provider |
| On-chain failure | Retry up to 3 times |
| XMTP failure | Retry up to 3 times |
