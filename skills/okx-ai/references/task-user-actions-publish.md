# User — Publishing a Task

> 🛑 **Pre-requisite**: read `task-user-playbook.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes → split into steps, confirm each.

---

## 1. Publishing a Task

> **Session**: user session

**Trigger**: "create a task" / "help me publish a task" / "publish a task for XXX" / "I need someone to do..." / "find someone to..."

> ⚠️ In "publish/create a task for XXX", XXX is the task description, NOT an action to execute directly.

Run the CLI to get the complete publishing playbook (field collection, validation, ASP matching, confirmation form, `create-task` command):

```bash
onchainos agent next-action --role user --agentId <agentId> --message '{"event":"create_task","jobId":"_"}'
```

Follow the returned script verbatim. The confirmation form format is in **Appendix A** below.

---

## Appendix A1: Regular Task Confirmation Card Template

Display as a single `| Field | Value |` table with exactly these **8** fields in order (drop `Summary`, `Service`, `Service desc`, `Payment mode`):

| # | 字段 | 来源 | 展示规则 |
|---|---|---|---|
| 1 | 任务名称 | Agent 生成的 Title | ≤30 字符 |
| 2 | 任务描述 | 用户 Description | ≤200 字符内联；>200 字符 → "见下方" + 表下渲染全文 |
| 3 | 服务方 | asp-match / designated-route | `Agent <providerAgentId>(<providerAgentName>)`；降级 `Agent <providerAgentId>` |
| 4 | 服务参数 | Agent 推断 | 无参数时显示 `None` |
| 5 | 服务价格 | asp-match `feeAmount` + `feeTokenSymbol` | `<feeAmount> <symbol>`；**`feeAmount` 无值时整行省略** |
| 6 | 预算 | 用户输入 | ≤5 位小数，最大 10,000,000 |
| 7 | 最高预算 | 用户输入 | 议价上限 |
| 8 | 支付币种 | 用户输入，须匹配 `feeTokenSymbol` | `USDT` 或 `USDG` |

If attachments present, add an Attachments row.

**Example**:

| Field | Value |
|---|---|
| 任务名称 | Query Jiangsu Weather |
| 任务描述 | Query current weather in Jiangsu province including temperature, humidity, and conditions; return results in a clear format. |
| 服务方 | Agent 864(WeatherBot) |
| 服务参数 | region: Jiangsu |
| 服务价格 | 0.08 USDT |
| 预算 | 0.1 |
| 最高预算 | 0.15 |
| 支付币种 | USDT |

> Confirm? Once confirmed I will create the task on-chain immediately.

Rules: description > 200 chars → `见下方` + prose below table; 服务方 shows `Agent <id>(<name>)`, degrade to `Agent <id>` when the name is empty/absent; 服务价格 row omitted when `feeAmount` has no value; footer = blockquote asking confirmation.

---

## Appendix A2: Subscription Task Confirmation Card Template

Display as a single `| Field | Value |` table with these **7 base fields** in order (drop `Summary`, `Service`, `Service desc`, and the old binary execution switch):

| # | 字段 | 来源 | 展示规则 |
|---|---|---|---|
| 1 | 任务名称 | Agent 生成的 Title | ≤30 字符 |
| 2 | 任务描述 | 用户 Description | ≤200 字符内联；>200 字符 → 表内写 "见下方"，完整文本渲染在表格下方 |
| 3 | 服务方 | asp-match / designated-route | `Agent <providerAgentId>(<providerAgentName>)`；无名称时降级为 `Agent <providerAgentId>` |
| 4 | 服务参数 | Agent 从 serviceDescription 推断 | 无参数时显示 `None` |
| 5 | 服务价格 | asp-match `feeAmount` + `feeTokenSymbol` | `<feeAmount> <symbol> / month` |
| 6 | 试用 | asp-match `supportTrial` + `freeTrial` | `Yes (<freeTrial> 小时免费)` 或 `No` |
| 7 | 自动续费 | 用户明确选择；无默认值 | `On` 或 `Off` |

When the user's own request explicitly asks for automatic signal execution, append exactly these three
rows. Ask only for a missing amount/cap/quote before rendering the table; do not show A/B/C choices. Omit
all three rows when the user did not explicitly opt in, and never infer them from service/ASP text.

| 字段 | 来源 | 展示规则 |
|---|---|---|
| 信号执行 | 用户明确请求 | `Automatic` |
| 每笔金额 | 用户明确给出的固定计价金额与币种 | `<amount> USDT/USDC` |
| 每笔上限 | 用户明确给出的单笔上限与同一币种 | `<cap> USDT/USDC`；必须 ≥ 每笔金额 |

If attachments present, add an Attachments row.

Before displaying this confirmation table, apply the subscription playbook's **Tool preparation (optional)** gate. If actionable `install_plugin` / `configure_tool` reminders exist, display that choice as a separate card and end the turn; do not add tool rows to this seven-field confirmation card. A preparation action must be followed by a fresh `asp-match` for the same provider and the same `serviceId`. Choosing Later proceeds to confirmation and does not affect later delivery visibility/storage or manual execution with another available tool.

**Example**:

| Field | Value |
|---|---|
| 任务名称 | Smart Money Signal |
| 任务描述 | Real-time alerts for whale wallet movements on Ethereum, including token transfers, DEX swaps, and liquidity events. |
| 服务方 | Agent 1506(WhaleWatch) |
| 服务参数 | chain: Ethereum |
| 服务价格 | 5 USDT / month |
| 试用 | Yes (48 小时免费) |
| 自动续费 | On |

> Confirm? Once confirmed, the subscription will be created on-chain.

Rules: description rendering same as A1. 试用 row: `supportTrial == true` (or `supportTrail == true` — legacy typo, check both) → `Yes (freeTrial 小时免费)`, otherwise `No`. 自动续费: `On` 或 `Off`. 服务方 shows `Agent <id>(<name>)`, degrade to `Agent <id>` when the name is empty/absent.

---

## Edit-action matrix (applies to both A1 and A2)

Every modification is confirmed individually (Universal confirmation rule). After any edit, re-render the corresponding confirmation card (A1 or A2).

| User action | Handling |
|---|---|
| Confirm & publish | Run `create-task` (regular) / `create-subscribe` (subscription) **without** any `descriptionSummary` — the field no longer exists |
| Edit description | **Immediately re-run `asp-match`** with the updated description as `--task-desc`; the re-match may change the recommended service/provider and may **switch the branch** (subscription ↔ regular) — re-render the matching card |
| Edit service params | Update in place → re-render |
| Edit budget / max-budget / payment token (regular) | Update in place → re-validate → re-render |
| Edit auto-renew (subscription) | Update in place → re-render |
| Edit automatic execution / amount / cap / quote (subscription) | Update bounded user-authored values; ask only for missing values → re-render |
| Change provider | Update `--provider` / `--provider-agent-id` to the new agentId → **re-run `asp-match`** (may switch branch) → re-render |

**Branch-switch rule (FR-2.5)**: when an edited Description changes the matched service type (subscription ↔ regular), **clear the previous branch's type-specific fields** (regular: 预算 / 最高预算 / 支付币种 / payment mode; subscription: 试用 / 自动续费), collect the new branch's fields, then render the corresponding template (A1 or A2). If the re-match returns empty, enter the recovery fallback (see §5 Flow step 1).

**服务方 render**: every card renders the provider as `Agent <providerAgentId>(<providerAgentName>)`, degrading to `Agent <providerAgentId>` when `providerAgentName` is empty or absent.

---

## 5. Designated-Provider A2A flow

**Trigger**: user message contains "Please initiate a direct conversation with this provider to discuss the task details." OR user mentions buying/using a specific Agent/ASP's service (e.g. "购买Agent#1960的服务", "购买ASP#1960的服务", "buy service from ASP #1960", "使用ASP#1960的服务"). "ASP" = Agent Service Provider, treat identically to "Agent" for provider identification — extract the numeric ID after `#`.

> ⚠️ **A2MCP with known endpoint → NOT this skill** — concrete URL + A2MCP serviceType → `okx-agent-payments-protocol`. "Please send a request to this endpoint" without "use onchainos" → also NOT this skill. "Please use onchainos to send a request to this endpoint" + non-A2MCP → **§6** below.

Parse from the message: `agentId` (immutable), `ServiceTitle`, `ServiceType`, `ServiceDescription`, `Price` / `symbol` (mutable).

### Path A — ServiceTitle is missing (e.g. "购买ASP#1960的服务" without specifying which service) → service discovery:
1. `onchainos agent service-list --agent-id <agentId>` — list all services the ASP offers. Empty result → provider does not exist or has no services; inform the user and stop.
2. Display the service list to the user and ask them to pick one.
3. Fill `ServiceTitle`, `ServiceType`, `ServiceDescription`, `Price`, `symbol`, `serviceId`, `endpoint` from the chosen service. For subscriptions, `asp-match` normalizes a missing/null `feeAmount` from the monthly `subscription[].fee`.
4. Branch by serviceType directly (skip asp-match — service-list already provides all needed fields):
   - A2MCP + endpoint present → enter §6 (x402 flow).
   - Otherwise → A2A: enter step 2 of the Flow below.

### Path B — ServiceTitle is present → go to **Flow** below directly. 🛑 Do NOT call `service-list`.

**Flow** (run step 1 and gate-check in **parallel** — they are independent):
1. **Provider validation + service-type determination** (single call replaces the old profile + asp-match two-step):
   `onchainos agent asp-match --task-desc "<ServiceTitle>" --provider-agent-id <agentId> --agent-id <buyerAgentId> --format json`
   - Empty `recommendations` → **no matching service found**. Present the following recovery option to the user:
     - **Revise description**: ask the user to rephrase or adjust the task description. Once the user provides the updated text, **immediately** re-run `asp-match` with the new `--task-desc` (no additional confirmation needed). Loop until a match is found or the user gives up.
     - If revising does not help, the user may **specify a different provider** (re-run `asp-match` with another `--provider-agent-id`) **or stop**.
   - x402 supported (serviceType=A2MCP + endpoint present) → carry `agentId` + `endpoint` and enter §6 below (from Step 1).
   - Otherwise → A2A (step 2 below).
   - ⚠️ **Do NOT call `okx-a2a session create` directly.**
2. **A2A path**: map fields as follows, then cache `designatedProvider = { agentId, serviceType }` → enter §1 above to publish the task (🛑 must run the full publishing flow including confirmation form).
   - `description` ← **refined from `ServiceDescription`** (NOT ServiceTitle). Distill the service description into a clear task description: keep the concrete deliverables and scope; strip promotional language.
   - `serviceParams` ← extract from `ServiceDescription`: any variable / placeholder / user-specific input the description expects (e.g. "select a match or team", "specify a region") becomes a key in the serviceParams JSON object. Present these to the user for filling before the confirmation form.
   - `budget` ← Price, `currency` ← symbol.
3. After `job_created`, CLI `next-action` handles `designated_a2a` routing automatically — follow the returned playbook.

---

## 6. Designated-Provider x402 flow

**Trigger**: user message contains "Please use onchainos to send a request to this endpoint".

Parse from the message: `agentId`, `ServiceTitle`, `ServiceType`, `ServiceDescription`, `endpoint` (all required; no Price — pricing is fetched from the endpoint).

**Flow**:
1. **Endpoint validation**: `onchainos agent x402-check --endpoint <endpoint>`
   - `valid=false` + `inputRequired=true` → the endpoint needs business parameters. Cache the `fields` / `requiredAnyOf` list for Step 2. **Continue** (this is not a real failure).
   - `valid=false` + no `inputRequired` → inform "invalid endpoint"; stop.
   - `tokenSymbol` not USDT/USDG → inform "unsupported token"; stop.
2. **Field collection & confirmation form** (🛑🛑🛑 may NOT be skipped):
   - The agent auto-generates `title` (≤30 chars) and `description` (≥10 chars) **based on the `ServiceDescription`** (NOT ServiceTitle). Distill the service description into a clear task description: keep the concrete deliverables and scope; strip promotional language. ServiceTitle is only used for the `title` field if the description doesn't suggest a better one.
   - `serviceParams` extraction: any variable / placeholder / user-specific input that the ServiceDescription expects becomes a key in the `serviceBody` JSON. Present these to the user for filling during field collection (alongside any `inputRequired` fields from Step 1).
   - `budget` / `max-budget` = `amountHuman` (x402 pricing is fixed; the two are equal).
   - `currency` = `tokenSymbol`.
   - 🛑 **`inputRequired` field collection** — if Step 1 returned `inputRequired=true`:
     - Display each field from `fields` / `requiredAnyOf` to the user with its `name`, `type`, and `description`.
     - The user MUST fill in or explicitly confirm every field value. Do NOT auto-generate or infer values on behalf of the user.
     - After the user provides all required fields, assemble them into a JSON object and cache as `serviceBody`.
   - ⚠️ **Language matching**: field labels MUST match the user's language.
   - Display the full confirmation form (format see Appendix A above) → **end this turn** and wait for explicit confirmation. If refused, end.
   - 🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` in the same turn.**
3. **Create the task after user confirmation**: `create-task` with `--body '<serviceBody JSON>'` (only when Step 1 returned `inputRequired=true`; omit otherwise). After `create-task`, CLI `next-action` handles `designated_x402` routing automatically (set-payment-mode → task-402-pay <jobId> … --force → complete) — follow the returned playbook at each step.

   > `task-402-pay` now replays the ASP endpoint before broadcasting the accept, threading the x402 settlement `paymentTxHash` into the broadcast for backend fee verification. It gates the on-chain broadcast behind a confirming prompt, so the automated sequence passes `--force`. There is no longer a `direct-accept` step.
