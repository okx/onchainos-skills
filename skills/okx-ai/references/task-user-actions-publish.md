# User — Publishing a Task

> 🛑 **Pre-requisite**: read `task-user-playbook.md` first. 🌐 All user-facing content must match the user's language.
> 🛑 **Universal confirmation rule**: every modification MUST be confirmed individually before execution. Multiple changes → split into steps, confirm each.

---

## 1. Publishing a Task

> **Session**: user session

**Trigger**: "create a task" / "help me publish a task" / "publish a task for XXX" / "I need someone to do..." / "find someone to..."

> ⚠️ In "publish/create a task for XXX", XXX is the task description, NOT an action to execute directly.

Resolve `<agentId>` from the current User-role context; if missing, run `onchainos agent get-my-agents --role user`. No result → route to User Agent registration and stop; otherwise use the returned `agentId`.

Run the CLI to get the complete publishing playbook (field collection, validation, service matching, confirmation form, `create-task` command):

```bash
onchainos agent next-action --role user --agentId <agentId> --message '{"event":"create_task","jobId":"_"}'
```

Follow the returned script verbatim. The confirmation form format is in **Appendix A** below.

---

## Appendix A1: Regular Task Confirmation Card Template

Display as a single `| Field | Value |` table with exactly these **8** fields in order (drop `Summary`, `Service`, `Service desc`, `Payment mode`):

| # | Field | Source | Render Rule |
|---|---|---|---|
| 1 | Task Name | Agent-generated Title | ≤30 characters |
| 2 | Task Description | User Description | Inline when ≤200 characters; when >200, show `See below` and render the full text below the table |
| 3 | Provider | task-service-select / designated-route | `Agent <providerAgentId>(<providerAgentName>)`; fall back to `Agent <providerAgentId>` |
| 4 | Service Parameters | Agent-inferred | `None` when empty |
| 5 | Service Price | task-service-select `feeAmount` + `feeTokenSymbol` | Zero (number or numeric string) → localized `Free`; otherwise `<feeAmount> <symbol>`; **omit the row when `feeAmount` is absent** |
| 6 | Budget | User input | ≤5 decimals; maximum 10,000,000 |
| 7 | Maximum Budget | User input | Negotiation cap |
| 8 | Payment Currency | User input; must match `feeTokenSymbol` | `USDT` or `USDG` |

If attachments present, add an Attachments row.

End with a localized confirmation blockquote and wait for explicit confirmation.

---

## Appendix A2: Subscription Task Confirmation Card Template

Display as a single `| Field | Value |` table with these **7 base fields** in order (drop `Summary`, `Service`, `Service desc`, and the old binary execution switch):

| # | Field | Source | Render Rule |
|---|---|---|---|
| 1 | Task Name | Agent-generated Title | ≤30 characters |
| 2 | Task Description | User Description | Inline when ≤200 characters; when >200, show `See below` and render the full text below the table |
| 3 | Provider | task-service-select / designated-route | `Agent <providerAgentId>(<providerAgentName>)`; fall back to `Agent <providerAgentId>` when unnamed |
| 4 | Service Parameters | Agent-inferred from `serviceDescription` | `None` when empty |
| 5 | Service Price | `task-service-select`: `subscriptionInfo.feeAmount`; `service-list`: selected `subscription[].fee` | `<fee> <symbol> / month`; never use the one-time `fee` / `feeAmount` field |
| 6 | Trial | `task-service-select`: `subscriptionInfo.supportTrial/freeTrial`; `service-list`: selected service `freeTrial` | A positive `freeTrial` → `Yes (<freeTrial> hours free)`; otherwise `No` |
| 7 | Auto-Renew | Explicit user choice; no default | `On` or `Off` |

Append these execution rows for a trading-signal subscription. Automatic execution is the default. The ASP
description may define which fields to ask about, but only user-authored replies supply persisted values.
Ask any ASP-required missing fields in one natural-language question without a choice card.

| Field | Source | Render Rule |
|---|---|---|
| Signal Execution | Default or explicit user choice | `Automatic` by default; `Manual` after explicit opt-out |
| Per-Signal Amount | Optional user-provided fixed quote amount and currency | `<amount> USDT/USDC` or `Not set` |
| Per-Signal Cap | Optional user-provided cap in the same currency | `<cap> USDT/USDC` or `Not set`; stored only |

If attachments present, add an Attachments row.

Before displaying this confirmation table, inspect advisory readiness. If actionable `install_plugin` /
`configure_tool` reminders exist, show a concise notice without choices and continue to confirmation. Do
not install, configure, retry, or block the subscription in this flow.

End with a localized confirmation blockquote and wait for explicit confirmation.

---

## Edit-action matrix (applies to both A1 and A2)

Every modification is confirmed individually (Universal confirmation rule). After any edit, re-render the corresponding confirmation card (A1 or A2).

| User action | Handling |
|---|---|
| Confirm & publish | Run `create-task` (regular) / `create-subscribe` (subscription) **without** any `descriptionSummary` — the field no longer exists |
| Edit description | Re-parse search intent and **immediately re-run `task-service-select`**; the re-match may change the recommended service/provider and may **switch the branch** (subscription ↔ regular) — re-render the matching card |
| Edit service params | Update in place → re-render |
| Edit budget / max-budget / payment token (regular) | Update in place → re-validate → re-render |
| Edit auto-renew (subscription) | Update in place → re-render |
| Edit automatic execution / amount / cap / quote (subscription) | Update user-authored values; cap remains informational → re-render |
| Change provider | Update `--asp-agent-id` to the new agentId → **re-run `task-service-select`** (may switch branch) → re-render |

**Branch-switch rule (FR-2.5)**: when an edited Description changes the matched service type (subscription ↔ regular), **clear the previous branch's type-specific fields** (regular: Budget / Maximum Budget / Payment Currency / payment mode; subscription: Trial / Auto-Renew), collect the new fields, then render A1 or A2. If re-match is empty, use §5 Flow step 1 recovery.

**Provider render**: use `Agent <providerAgentId>(<providerAgentName>)`, falling back to `Agent <providerAgentId>` when the name is empty/absent.

---

## 5. Designated-Provider A2A flow

**Trigger**: "Please initiate a direct conversation with this provider to discuss the task details," or a request to buy/use a specific Agent/ASP service (e.g. "buy service from ASP #1960" / "use Agent #1960's service"). Treat ASP and Agent identically; extract the numeric ID after `#`.

> ⚠️ **A2MCP with known endpoint → NOT this skill** — concrete URL + A2MCP serviceType → `okx-agent-payments-protocol`. "Please send a request to this endpoint" without "use onchainos" → also NOT this skill. "Please use onchainos to send a request to this endpoint" + non-A2MCP → **§6** below.

Parse from the message: `agentId` (immutable), `ServiceTitle`, `ServiceType`, `ServiceDescription`, `Price` / `symbol` (mutable).

### Path A — ServiceTitle is missing (e.g. "buy service from ASP #1960" without naming a service) → service discovery:
1. `onchainos agent service-list --agent-id <agentId>` — list all services the ASP offers. Empty result → provider does not exist or has no services; inform the user and stop.
2. Display the service list to the user and ask them to pick one.
3. Fill `ServiceTitle`, `ServiceType`, `ServiceDescription`, `Price`, `symbol`, `serviceId`, `endpoint` from the chosen service. For services chosen from `service-list`, derive subscription status and billing details from that service's `subscription` / `freeTrial` fields. For services chosen from `task-service-select`, use `supportSubscription` for branch selection and `subscriptionInfo` for billing interval / trial details.
4. Branch by serviceType directly (skip task-service-select — service-list already provides all needed fields):
   - A2MCP + endpoint present → enter §6 (x402 flow).
   - Otherwise → A2A: enter step 2 of the Flow below.

### Path B — ServiceTitle is present → go to **Flow** below directly. 🛑 Do NOT call `service-list`.

**Flow** (run step 1 and gate-check in **parallel** — they are independent):
1. **Provider validation + service-type determination**:
   Pass the user's original utterance verbatim to [`intent-keyword-extraction.md`](intent-keyword-extraction.md), then use its output unchanged as `<args>` in:
   `onchainos agent task-service-select <args> --agentic-id <buyerAgentId> --limit 1 --format json`
   Serialize `keywords` exactly like `service-match`: emit `--keywords` once, followed by all extracted keyword values in order. Do not preprocess or enrich the input or output.
   - `matchStatus=no_match` → the specified ASP has no matching service. Ask the user to revise the description or specify a different provider, then **end this turn**. After the user responds, re-parse the search intent and re-run `task-service-select`.
   - `matchStatus=no_online_service` → matching services exist but are offline. Ask the user whether to view alternatives or revise the description/provider, then **end this turn**. Offer alternatives only when `hasMore == true` and `searchAfter` is a non-empty string.
   - `matchStatus=matched` → read `data.services[0]`. Only in this branch inspect the selected service's `serviceType` and `endpoint`:
     - A2MCP + endpoint present → carry `agentId` + `endpoint` and enter §6 below (from Step 1).
     - Otherwise → A2A (step 2 below).
   - Any unrecognized `matchStatus` or missing `data.services[0]` → stop; do not continue to either service-type branch.
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
