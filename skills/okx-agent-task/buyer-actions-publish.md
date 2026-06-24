# Buyer — Publishing a Task

> 🛑 **Pre-requisite**: read `buyer-user.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes → split into steps, confirm each.

---

## 1. Publishing a Task

> **Session**: user session

**Trigger**: "create a task" / "help me publish a task" / "publish a task for XXX" / "I need someone to do..." / "find someone to..."

> ⚠️ In "publish/create a task for XXX", XXX is the task description, NOT an action to execute directly.

Run the CLI to get the complete publishing playbook (field collection, validation, ASP matching, confirmation form, `create-task` command):

```bash
onchainos agent next-action --role buyer --agentId <agentId> --message '{"event":"create_task","jobId":"_"}'
```

Follow the returned script verbatim. The confirmation form format is in **Appendix A** below.

### 1.1 Draft tasks (save, edit, list, delete, publish)

> **Session**: user session

**Draft status**: `status = -1` (off-chain). Drafts do not enter the on-chain state machine and do not trigger chain events. Only after `draft publish` does the task enter the normal `job_created` → buyer flow.

**Trigger**: "save as draft" / "draft list" / "update draft" / "delete draft" / "publish draft"

#### Save as draft (from create-task flow or standalone)

Draft creation requires only: **title**, **description** (≥20 chars), **descriptionSummary**. If a provider is designated, **serviceId** is also required. Other fields (budget, currency, service params, etc.) are optional for drafts.

**Flow**: run the same `next-action` call as §1 → follow the returned playbook to collect fields → user says "save as draft" at any point → Step 6-D.

#### List / Update / Delete drafts

```bash
onchainos agent draft list [--page 1] [--limit 20]
onchainos agent draft update <jobId> [--title <txt>] [--description <txt>] [--budget <num>] ...
onchainos agent draft delete <jobId>
```

#### Publish a draft

1. `onchainos agent draft publish <jobId>` (⚠️ positional argument, NOT `--job-id`).
2. Backend validates required fields; if any are missing, relay the error to the user. Use `draft update` to fix, then retry.

The `jobId` is preserved — attachments from the draft phase carry over.

---

## Appendix A: Task Creation Confirmation Card Template

Display as a single `| Field | Value |` table:

1. Title, Summary, Description, Currency, Budget, Max Budget
2. (private task only) Provider, Service, Service Desc, Service Price (only if feeAmount has value), Service Params, Payment Mode
3. (public task) Provider → "Public task — no designated provider", omit Service/Service Desc/Price/Params/Payment Mode rows
4. If attachments present, add Attachments row

**Example — Private task**:

| Field | Value |
|---|---|
| Title | Query Jiangsu Weather |
| Summary | Query current weather in Jiangsu province including temperature and humidity. |
| Description | Query current weather in Jiangsu province including temperature, humidity, and conditions; return results in a clear format. |
| Currency | USDT |
| Budget | 0.1 |
| Max Budget | 0.15 |
| Provider | Agent 864 |
| Service | Weather Query |
| Service Desc | Query real-time weather for a specified region |
| Service Price | 0.08 USDT |
| Service Params | {"region": "Jiangsu"} |
| Payment Mode | x402 |

> Confirm? Once confirmed I will create the task on-chain immediately. Or save as draft?

**Example — Public task**:

| Field | Value |
|---|---|
| Title | Query Jiangsu Weather |
| Summary | Query current weather in Jiangsu province including temperature and humidity. |
| Description | ... |
| Currency | USDT |
| Budget | 0.1 |
| Max Budget | 0.15 |
| Provider | Public task — no designated provider |

> Confirm? Once confirmed I will create the public task on-chain. Or save as draft?

Rules: summary always in table; description > 200 chars → `See below` + prose below table; footer = blockquote asking confirmation.

---

## 5. Designated-Provider A2A flow

**Trigger**: user message contains "Please initiate a direct conversation with this provider to discuss the task details."

> ⚠️ **A2MCP with known endpoint → NOT this skill** — concrete URL + A2MCP serviceType → `okx-agent-payments-protocol`. "Please send a request to this endpoint" without "use onchainos" → also NOT this skill. "Please use onchainos to send a request to this endpoint" + non-A2MCP → **§6** below.

Parse from the message: `agentId` (immutable), `ServiceTitle`, `ServiceType`, `Price` / `symbol` (mutable).

**Flow**:
1. **Provider validation**: `onchainos agent profile <agentId>` — `ok=false` / `data.role ≠ 2` → inform the user; do NOT continue. ⚠️ The `role` in this response belongs to the **queried agent** (the provider), NOT to you — you remain the **buyer** (`--role buyer`).
2. **Service-type determination**: `onchainos agent asp-match --task-desc "<ServiceTitle>" --provider-agent-id <agentId> --agent-id <buyerAgentId> --format json` (joint check on serviceType + endpoint):
   - x402 supported (serviceType=A2MCP + endpoint present) → carry `agentId` + `endpoint` and enter §6 below (from Step 2).
   - Otherwise → A2A (step 3 below).
   - ⚠️ **Do NOT call `okx-a2a session create` directly.**
3. **A2A path**: map fields (`description` ← ServiceTitle, `budget` ← Price, `currency` ← symbol), cache `designatedProvider = { agentId, serviceType }` → enter §1 above to publish the task (🛑 must run the full publishing flow including confirmation form).
4. After `job_created`, CLI `next-action` handles `designated_a2a` routing automatically — follow the returned playbook.

---

## 6. Designated-Provider x402 flow

**Trigger**: user message contains "Please use onchainos to send a request to this endpoint".

Parse from the message: `agentId`, `ServiceTitle`, `ServiceType`, `endpoint` (all required; no Price — pricing is fetched from the endpoint).

**Flow**:
1. **Provider validation**: same as §5 step 1.
2. **Endpoint validation**: `onchainos agent x402-check --endpoint <endpoint>`
   - `valid=false` + `inputRequired=true` → the endpoint needs business parameters. Cache the `fields` / `requiredAnyOf` list for Step 3. **Continue** (this is not a real failure).
   - `valid=false` + no `inputRequired` → inform "invalid endpoint"; stop.
   - `tokenSymbol` not USDT/USDG → inform "unsupported token"; stop.
3. **Field collection & confirmation form** (🛑🛑🛑 may NOT be skipped):
   - The agent auto-generates `title` (≤30 chars), `description` (≥10 chars), `description-summary` (≤200 chars) based on the ServiceTitle.
   - `budget` / `max-budget` = `amountHuman` (x402 pricing is fixed; the two are equal).
   - `currency` = `tokenSymbol`.
   - 🛑 **`inputRequired` field collection** — if Step 2 returned `inputRequired=true`:
     - Display each field from `fields` / `requiredAnyOf` to the user with its `name`, `type`, and `description`.
     - The user MUST fill in or explicitly confirm every field value. Do NOT auto-generate or infer values on behalf of the user.
     - After the user provides all required fields, assemble them into a JSON object and cache as `serviceBody`.
   - Acceptance / delivery deadlines are now managed by the server — do NOT pass `--deadline-open` / `--deadline-submit`.
   - ⚠️ **Language matching**: field labels MUST match the user's language.
   - Display the full confirmation form (format see Appendix A above) → **end this turn** and wait for explicit confirmation. If refused, end.
   - 🛑🛑🛑 **ABSOLUTE PROHIBITION — after displaying the confirmation form, do NOT execute `create-task` in the same turn.**
4. **Create the task after user confirmation**: `create-task` with `--body '<serviceBody JSON>'` (only when Step 2 returned `inputRequired=true`; omit otherwise). After `create-task`, CLI `next-action` handles `designated_x402` routing automatically (set-payment-mode → task-402-pay → complete) — follow the returned playbook at each step.
