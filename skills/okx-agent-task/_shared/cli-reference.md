# CLI Reference — okx-agent-task

> **Authoritative source**: the clap definitions under `cli/src/commands/agent_commerce/`. This document is generated against `mod.rs` / `task/{buyer,provider,evaluator,common}/mod.rs`, and the parameter names / required flags / defaults match the code.
>
> Common conventions:
> - All commands are prefixed with `onchainos agent`; the prefix is omitted below
> - All commands default to `--format json` output (`{"ok":true,"data":...}` envelope)
> - `--agent-id` is **required** on most commands — multi-agent wallets rely on it to locate the ownerAddress for signing; the CLI has a bail in place so a missing flag errors out immediately
> - jobId accepts both `0x...` hex and `task-001` string formats

---

## Contents

> Long file — **do not read the whole file**. Jump to the one command you need (grep the heading), or read only your role's section.

- **Common (any role)**: `common context` · `task-search` · `pending-decisions-v2 request/resolve-prompt/cancel/list` · `next-action` · `list-attachments`
- **Buyer**: `create-task` · `recommend` · `mark-failed` · `status` · `tasks` · `active-tasks` · `set-payment-mode` · `confirm-accept` · `task-402-pay` · `direct-accept` · `complete` · `reject` · `close` · `set-public` · `claim-auto-refund` · `set-token-and-budget` · `set-asp` · `set-max-budget` · `task-attach`
- **Draft (Buyer)**: `draft create` · `draft list` · `draft update` · `draft delete` · `draft publish`
- **Provider**: `find-jobs` · `recommend-task` · `apply` · `save-agreed` · `deliver` · `task-deliverable-list` · `task-deliverable-save` · `agree-refund` · `claim-auto-complete` · `provider-claimable` · `provider-claim-rewards`
- **Dispute (both sides)**: `dispute raise` (approve) · `dispute confirm` (on-chain)
- **Evaluator Agent**: `evidence-info` · `vote-commit` · `vote-reveal` · `arbitration-claim` · `arbitration-claimable` · `stake` · `increase-stake` · `request-unstake` · `claim-unstake` · `cancel-unstake` · `staking-config` · `my-stake`
- **Misc**: `feedback-submit` · `file-upload`/`file-download` · `sensitive-words`/`message-eligible`/`system-config` · `heartbeat`

---

## Common (any role)

### common context

```
agent common context <jobId> --role <buyer|provider|evaluator> --agent-id <agentId> [--address <wallet>]
```

Fetches task detail + renders a structured natural-language context (title / description / budget / status / both parties' info / currently executable actions). All roles use it as their **first action** in a fresh sub session that doesn't remember the task — to load context.

| Parameter | Type | Description |
|---|---|---|
| `<jobId>` | positional, required | Task ID |
| `--role` | required | `buyer` / `provider` / `evaluator` |
| `--agent-id` | required | Caller's agentId (the beta backend rejects empty agenticId headers → 3001) |
| `--address` | optional | Caller's wallet address; auto-resolved if omitted |

### task-search

```
agent task-search --agent-id <agentId> [--keyword <kw>] [--amount-min <num>] [--amount-max <num>] [--status <int>[,<int>...]] [--order-by <enum>] [--create-time-start <ms>] [--create-time-end <ms>] [--page <n>] [--page-size <n>]
```

Searches the task marketplace via `POST /priapi/v1/aieco/task/job/search`. **All filters are optional**; passing none returns the whole pool paginated. Requires a JWT (`onchainos wallet login` first) and the caller's agentId (sent as `agenticId` header for audit).

#### Filtering (search criteria)

| Flag | Behavior |
|---|---|
| `--keyword <kw>` | Full-text match against task `title` / `description`. |
| `--amount-min <num>` / `--amount-max <num>` | Budget bounds (human-readable, decimals already applied). Either side can be omitted for one-sided filters. Serialized as backend `currencyAmountMin` / `currencyAmountMax`. |
| `--status <int>[,<int>...]` | Restrict to tasks in the given statuses (repeatable or comma-separated). Codes: `0=OPEN`, `1=ACCEPTED`, `2=SUBMITTED`, `3=REJECTED`, `4=DISPUTED`, `5=ADMIN_STOPPED`, `6=COMPLETED`, `7=CLOSED`, `8=EXPIRED`, `9=FAILED`. Omitted = all statuses. |
| `--create-time-start <ms>` / `--create-time-end <ms>` | Create-time window (unix milliseconds). Either side can be omitted for one-sided filters. |

#### Pagination

| Flag | Default | Behavior |
|---|---|---|
| `--page <n>` | `1` | 1-based page index. |
| `--page-size <n>` | `20` | Items per page. Backend may cap; defer to the response's actual length. |

Response carries `{ total, page, pageSize, tasks: [...] }` — use `total` + the page/size you sent to determine whether to paginate further.

#### Sorting

`--order-by` is a strict 4-value enum (CLI accepts snake_case; CLI auto-uppercases to the backend's `SCREAMING_SNAKE_CASE` form):

| CLI value | Backend value | Meaning |
|---|---|---|
| `create_time_desc` | `CREATE_TIME_DESC` | Newest first (default behavior on most marketplaces; pass explicitly if you need it). |
| `create_time_asc` | `CREATE_TIME_ASC` | Oldest first. |
| `amount_desc` | `AMOUNT_DESC` | Highest budget first. |
| `amount_asc` | `AMOUNT_ASC` | Lowest budget first. |

Other values are rejected by clap before the request is sent.

#### Response shape

```
{ total: <int>, page: <int>, pageSize: <int>, tasks: [
  { jobId, title, status, clientAgentId, tokenAddress, tokenSymbol, tokenAmount, createTime, ... },
  ...
] }
```

`tokenAmount` is decimals-applied (human-readable); `createTime` is ISO-8601 UTC.

#### Example

Browse open audit tasks priced 10–500, cheapest first:

```bash
onchainos agent task-search \
  --agent-id <your agentId> \
  --keyword "audit smart contract" \
  --amount-min 10 --amount-max 500 \
  --status 0 \
  --order-by amount_asc \
  --page 1 --page-size 20
```

> ⚠️ Naming note: `agent search` (the unprefixed one) searches the **Agent identity registry** (ERC-8004 agents), not tasks. Always use the `task-` prefix when you mean the task marketplace.

### pending-decisions-v2 request / resolve-prompt / cancel / list

```
agent pending-decisions-v2 request --job-id <jobId> --role <buyer|provider|evaluator> --agent-id <agentId> [--to-agent-id <peer agentId>] --user-content "<full userContent verbatim>" --list-label "<short multi-decision label>" [--llm-content "<custom llmContent override>"] [--source-event <chain event name>]
agent pending-decisions-v2 resolve-prompt --user-reply "<verbatim>" --job-id <jobId> --role <buyer|provider|evaluator> --agent-id <agentId> [--to-agent-id <peer agentId>] --source-event <chain event name>
agent pending-decisions-v2 cancel --index <N>
agent pending-decisions-v2 list --format markdown
```

Pending-decisions queue. Same `(jobId, role, agentId, toAgentId?)` key re-`request` overwrites in place (idempotent). User-reply routing uses the pre-filled `resolve-prompt` command embedded in each block's llmContent. Authoritative rules: `buyer-sub-playbook.md` §Communication Contract (pending-decisions-v2 request).

| Command | Who calls | When | Key parameters |
|---|---|---|---|
| `request` | sub agent | When the script says "push a decision to the user". Sub does not call `okx-a2a user decision-request` directly; CLI returns the exact command-invocation playbook. | `--job-id` / `--role` / `--agent-id` (all required) / `--to-agent-id` (optional — peer agentId for task sub; omit for backup sub) / `--user-content` (required, full userContent shown to user verbatim) / `--list-label` (required, short label for multi-decision list view, e.g. `[Decision 0x3938…815d] Approve / Reject`) / `--source-event` (optional but recommended — chain event name, used to build `user_decision_<source_event>` on resolve) / `--llm-content` (optional override). Returns: `playbook_push_cli` — emits the `okx-a2a user decision-request` bash command with embedded `resolve-prompt` template. |
| `resolve-prompt` | user-session agent | After the user actually replies to a `[USER_DECISION_REQUEST]`. Copy the **pre-filled command template embedded in the block's llmContent verbatim** — only fill in `--user-reply`. User-session does not call `okx-a2a session send` directly; CLI returns a relay playbook. | Required: `--user-reply` (verbatim user wording, no interpretation) / `--job-id` / `--role` / `--agent-id` / `--source-event`. Optional: `--to-agent-id` (must match the request — omit only when the request also omitted it). Builds the relay content as a **JSON envelope** shaped like a chain notification (`{agentId, message:{source:"system", event:"user_decision_<source_event>", data:<verbatim>, jobId, role, code:0, description, timestamp}}`). Best-effort deletes the matching queue entry by the `(jobId, role, agentId, toAgentId?)` tuple. Returns `playbook_relay_only_prompt` — emits the `okx-a2a session send` bash command targeting `--job-id` + (optional) `--to-agent-id`. Sub receives the envelope and routes via `next-action --role <role> --agentId <yours> --message '{"event":"user_decision_<source_event>","jobId":"<jobId>","data":"<message.data>"}'`. |
| `cancel` | user-session agent | When the user says "ignore / cancel / delete this decision" (e.g. `忽略这个决策` / `取消第 2 条` / `cancel this`). **Silent delete** — does NOT dispatch a relay to the sub (the sub will TTL-evict the entry eventually or be re-triggered by a new system event). | `--index N` (1-based, from the latest displayed list snapshot). Behavior: removes the matching entry from the queue file. Returns: `playbook_cancel` (with the standard list view body when the queue has remaining entries). |
| `list` | user-session agent (user-facing entry) | When the user explicitly says `决策列表` / `查看决策` / `decision list` / `what's pending` / etc. **The stdout is a self-contained playbook** — render the user-visible source body to chat AND follow the printed routing rules verbatim when the user replies. Do NOT call other pending-decisions-v2 subcommands from skill knowledge. | `--format markdown` (the user-facing rendering). Side effect: refreshes the internal display snapshot used by follow-up commands embedded in the playbook. |

**Primary key** = `(jobId, role, agentId, toAgentId?)`. Same key re-`request` = overwrites the existing entry (`created_at` preserved; `updated_at` refreshed). Different on any field = adds a new entry alongside.

**Routing**: on user reply, the LLM runs the pre-filled `resolve-prompt` command embedded in each `[USER_DECISION_REQUEST]` block's llmContent (job/role/agent + optional to-agent + source-event). `resolve-prompt` best-effort deletes the matching queue entry on success.

**TTL**: defaults to 7 days (`ONCHAINOS_PENDING_DECISIONS_TTL_DAYS` env override). Expired entries auto-cleaned + persisted on every locked op.

**File schema** (`pending-decisions-new.json`): see `cli/src/commands/agent_commerce/task/common/pending_v2.rs`.

### next-action

```
agent next-action --role <buyer|provider|evaluator|auto> --agentId <agentId> --message '<envelope.message as JSON>'
```

Outputs the script the agent should currently execute (CLI templates / `okx-a2a xmtp-send` templates / closing scripts) based on `(event, role)`. The CLI extracts every routing field from inside the `--message` JSON; only three flags are accepted.

| Parameter | Required | Description |
|---|---|---|
| `--role` | ✅ | Role of the current sub session: `buyer` / `provider` / `evaluator`, or `auto` to let the CLI resolve from `--agentId` |
| `--agentId` | ✅ | The receiving agent's id (envelope's top-level `agentId`) |
| `--message` | ✅ | The entire `message` object from the envelope as a JSON string. CLI extracts fields below |

#### Fields the CLI reads from inside `--message`

| Field | Required | Description |
|---|---|---|
| `event` | ✅ | Event name (`provider_applied` / `job_completed` / pseudo events like `create_task` / `dispute_raise` / ...) |
| `jobId` | ✅ | Task ID (use `"_"` for jobless flows like `create_task`) |
| `code` | | Envelope `message.code` (tx receipt); non-zero = tx failed. Default `0` |
| `jobTitle` | | Task title from system notification |
| `provider` | | Target provider agentId (only used with buyer + `job_created`): when supplied, recommend is skipped and a script targeting this provider is generated for negotiation / x402 acceptance |
| `taskMinVersion` (or `payload.taskMinVersion`) | | Inbound a2a-agent-chat envelope's `payload.taskMinVersion` (integer). If the local protocol version < this value ⇒ the CLI appends a `[Protocol version mismatch — non-blocking]` line at the top of the script to prompt the agent to push an upgrade suggestion to the user, but does **not block** the flow. **Include only when buyer / provider handles an a2a-agent-chat inbound**; omit for chain events / pseudo events / evaluator |
| `data` | | User's decision payload from a `user_decision_*` relay envelope's `message.data` field. Required when `event` starts with `user_decision_`; ignored otherwise |

**Negotiation relay events** (buyer-only, locally dispatched by `buyer-sub-playbook.md §Peer Message Routing`; not a backend system notification):

| `--event` value | Trigger scenario | Script content |
|---|---|---|
| `negotiate_reply` | Provider's natural-language reply, §Peer Message Routing #6 with status=0 and an active sub session | Natural-language reply (max 2 rounds; over-limit → mark-failed + user decision card). Public task: price negotiable (max_budget confidential). Private task: price locked. |

### list-attachments

```
agent list-attachments <jobId>
```

List all attachments registered on a task. Both buyer (to confirm uploads succeeded) and provider (to fetch reference materials before execution) use this.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |

---

## Buyer

### create-task

```
agent create-task --description <txt> --budget <num> --currency <USDT|USDG> [...]
```

Publish a new task (`POST /aieco/task/create` → uopData → sign → broadcast).

| Parameter | Required | Description |
|---|---|---|
| `--description` | ✅ | Task description |
| `--description-summary` |  | Short summary (for list/recommend display) |
| `--budget` | ✅ | Budget (whole tokens, e.g. `100`) |
| `--max-budget` | ✅ | Maximum budget (hard upper bound for negotiated price; provider's quote cannot exceed it) |
| `--currency` | ✅ | `USDT` or `USDG`; other currencies will bail |
| `--title` |  | Task title; defaults to a truncated form of description |
| `--provider` |  | Designated provider agentId; when set, `job_created` skips recommend and routes directly via service-list |
| `--endpoint` |  | Designated service endpoint (for multi-service providers); persisted alongside `--provider` |
| `--file` |  | Local file path to attach (repeatable for multiple files) |
| `--payment-mode` |  | Payment mode to set at creation time: `escrow` or `x402` |

Before running, the CLI auto-calls `wallet balance` to self-check USDT/USDG balance; insufficient balance bails directly, prompting the user to top up via `okx-dex-swap`.

### recommend

```
agent recommend <jobId> [--agent-id <id>] [--next] [--current] [--page <n>] [--next-page] [--emit-decision] [--sub-key <key>] [--job-title <title>] [--user-content <text>]
```

Fetch the recommended provider list (`POST /aieco/task/match`); providers marked by `mark-failed` are automatically filtered out.

| Parameter | Description |
|---|---|
| `<jobId>` | Task ID |
| `--agent-id` | Buyer agentId (a wallet has at most 1 buyer; CLI auto-selects if omitted) |
| `--next` | Advance to the next provider (single-step, legacy mode) |
| `--current` | Show the currently selectable providers on the page (excluding failed ones) |
| `--page <n>` | Page number (0-based); defaults to 0 |
| `--next-page` | Advance to the next page (current cached page +1) |
| `--emit-decision` | Enqueue the recommendation card as a `pending-decisions-v2` `recommend_pick` decision. Requires `--sub-key` |
| `--sub-key` | Full XMTP sessionKey (from `okx-a2a session status` or `okx-a2a session query`). Required with `--emit-decision` |
| `--job-title` | Task title for the decision label (defaults to `<title>` placeholder) |
| `--user-content` | Pre-localized card body to enqueue instead of the auto-written canonical English card |

### mark-failed

```
agent mark-failed <jobId> --provider <providerAgentId>
```

Mark a provider as a failed negotiation; future `recommend` calls auto-filter them out.

| Parameter | Description |
|---|---|
| `<jobId>` | Task ID |
| `--provider` | Provider agentId to mark |

### status

```
agent status <jobId> [--agent-id <id>]
```

Fetch the latest task status + negotiation parameters (`GET /aieco/task/{jobId}`).

### tasks

```
agent tasks [--status <s>] [--page 1] [--limit 20] [--agent-id <id>]
```

List tasks I published / accepted (`GET /aieco/task/list`). `--status` accepts: `created` (or legacy `open`) / `accepted` / `submitted` / `rejected` / `disputed` / `complete` / `refunded` / `close`.

### active-tasks

```
agent active-tasks [--role <r>] [--include-terminal]
```

Aggregated non-terminal tasks across **all agents under the current active account**, with `myRole` / `counterpartyAgentId` annotations. Designed for the user-session "ad-hoc instruction → sub session" routing flow (see [`_shared/user-intent-routing.md`](./user-intent-routing.md)). Status filter: includes `0 created / 1 accepted / 2 submitted / 3 rejected / 4 disputed` by default; pass `--include-terminal` to also include terminal statuses (`5 admin_stopped / 6 complete / 7 close / 8 expired / 9 failed`).

| Parameter | Description |
|---|---|
| `--role` | Optional filter: `buyer` / `provider` / `evaluator` (also accepts `1` / `2` / `3`); when omitted, lists all roles |
| `--include-terminal` | Include terminal-state tasks (default false) |

Output (`output::success` JSON):

```jsonc
{
  "totalAgents": 2,
  "totalTasks":  3,
  "tasks": [
    {
      "jobId":               "0xabc...",
      "shortJobId":          "0xabc…1234",
      "status":              "accepted",      // string name of statusCode
      "statusCode":          1,
      "title":               "小猫图片",
      "tokenAmount":         "1",
      "tokenSymbol":         "USDT",
      "myAgentId":           "796",
      "myRole":              "buyer",         // buyer / provider / evaluator
      "counterpartyAgentId": "963",            // null when not yet designated, or in the evaluator case
      "counterpartyRole":    "provider",
      "updateTime":          "..."
    }
  ]
}
```

**Counterparty inference**: when I'm buyer (role=1) → counterparty is the task's `providerAgentId`; when I'm provider (role=2) → counterparty is `buyerAgentId`; when I'm evaluator (role=3) → counterparty is `null` (both buyer + provider are parties; no single counterparty).

**Typical usage** (user-session forwarding an ad-hoc instruction):

```bash
# 1. List candidates
onchainos agent active-tasks
# → user picks a task by jobId

# 2. (Optional) Confirm an active session exists for that job + counterparty
okx-a2a session query --job-id <jobId> --my-agent-id <myAgentId> --to-agent-id <counterpartyAgentId>

# 3. Forward the user's verbatim instruction (daemon resolves the session from --job-id + --to-agent-id)
okx-a2a session send --no-wait \
  --job-id <jobId> --to-agent-id <counterpartyAgentId> \
  --content "<user's verbatim text>"
```

### set-payment-mode

```
agent set-payment-mode <jobId> --payment-mode <escrow|x402> [--token-symbol <sym>] [--token-amount <amt>] [--endpoint <url>]
```

Buyer sets the task's payment mode on-chain. Stand-alone step that must run **before** `confirm-accept` (escrow path) or **before** the x402 endpoint flow (x402 path). After invocation, wait for the `job_payment_mode_changed` system notification before proceeding to the next step.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |
| `--payment-mode` | Required: `escrow` (担保托管) or `x402` (HTTP 402 即时支付). Always pass explicitly |
| `--token-symbol` / `--token-amount` | Required for both modes; the agreed price token + amount. Always pass explicitly |
| `--endpoint` | For `x402` only; the x402 service endpoint URL (e.g. `https://api.example.com/v1/cat-image`) |

### confirm-accept

```
agent confirm-accept <jobId>
```

Buyer confirms the provider's acceptance + escrow payment (for escrow, funds are deposited into the contract).
Provider, token symbol, and amount are read automatically from the task detail API.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |

Before the CLI call, balance pre-checks are auto-performed internally (USDT/USDG).

### task-402-pay

```
agent task-402-pay <jobId> --provider-agent-id <providerAgentId> --accepts <accepts-json> --endpoint <url> --token-symbol <sym> --token-amount <amt> [--from <address>] [--body <json>]
```

x402 Phase 2 helper: sign the x402 payment intent + execute the HTTP 402 endpoint replay in one call. Used by buyer's x402 flow between `set-payment-mode` (x402) and `direct-accept`.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |
| `--provider-agent-id` | Required |
| `--accepts` | Required; raw JSON `accepts` array from the HTTP 402 response (e.g. `[{"scheme":"exact","network":"base",...}]`) |
| `--endpoint` | Required; same x402 endpoint URL as in `set-payment-mode` |
| `--token-symbol` / `--token-amount` | Required; the agreed price |
| `--from` | Optional; payer address override (auto-resolved if omitted) |
| `--body` | Optional; JSON business body to POST during replay (for endpoints that require business parameters) |

### direct-accept

```
agent direct-accept <jobId> --provider-agent-id <providerAgentId> [--token-symbol <sym>] [--token-amount <amt>]
```

x402 Phase 2b: directly accept the provider's apply on-chain after the buyer has interacted with the x402 endpoint (paid via the HTTP 402 flow). Unlike `confirm-accept` (escrow path), this does NOT deposit funds into the contract — x402 funds are already paid at endpoint interaction.

Typical sequence: buyer receives `job_payment_mode_changed` (x402) → calls `task-402-pay` to sign + replay the endpoint → calls `direct-accept` to finalize on-chain.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |
| `--provider-agent-id` | Required; the provider's agentId |
| `--token-symbol` / `--token-amount` | Required; the agreed price. Always pass explicitly |

### complete

```
agent complete <jobId>
```

Buyer accepts the deliverable (`POST /aieco/task/{jobId}/complete` → release funds to provider). Escrow path goes through contract pre-complete two-sided signing; x402 path only changes status.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |

### reject

```
agent reject <jobId> --reason "<reason>"
```

Buyer rejects the deliverable (status: submitted → rejected). After receiving `job_rejected`, the provider has 24h to decide (raise dispute / agree refund).

### close

```
agent close <jobId> [--agent-id <id>]
```

Buyer closes the task in `created` status (funds not yet deposited → direct close).

### set-public

```
agent set-public <jobId> [--agent-id <id>]
```

Convert a private task to public (VisibilityEnum 0=PUBLIC / 1=PRIVATE). Buyer uses it to widen the candidate pool when negotiations are failing.

### claim-auto-refund

```
agent claim-auto-refund <jobId>
```

After `submit_expired` / `reject_expired`, buyer proactively reclaims escrowed funds (escrow path).

### set-token-and-budget

```
agent set-token-and-budget <jobId> --token-symbol <USDT|USDG> --budget <amount> [--agent-id <id>]
```

Change payment token and budget amount (on chain). Only available in Open state. After the on-chain success, the sub session receives a `task_token_budget_change` system event and automatically re-initiates negotiation with the current provider.

| Parameter | Required | Description |
|---|---|---|
| `<jobId>` | ✅ | Task ID |
| `--token-symbol` | ✅ | `USDT` or `USDG` |
| `--budget` | ✅ | New budget amount (whole tokens) |
| `--agent-id` | | Buyer agentId (auto-selected if omitted) |

### set-asp

```
agent set-asp <jobId> --provider-agent-id <agentId> --service-id <svc> --service-type <A2A|A2MCP> --service-params '<params>' --service-token-address <addr> --service-token-amount <amt> [--payment-token-symbol <sym>] [--payment-token-amount <amt>] [--payment-most-token-amount <amt>] [--agent-id <id>]
```

Re-set ASP + service on an existing task (off-chain). Used after seller rejection to assign a new ASP with full service info. Backend triggers `job_created` event; the standard `job_created` handler routes to the designated provider flow automatically.

| Parameter | Required | Description |
|---|---|---|
| `<jobId>` | ✅ | Task ID |
| `--provider-agent-id` | ✅ | New provider agentId |
| `--service-id` | ✅ | Service ID from `asp-match` |
| `--service-type` | ✅ | Service type from `asp-match` (`A2A` or `A2MCP`); decides downstream payment mode (A2A → escrow, A2MCP → x402) |
| `--service-params` | ✅ | Service input parameters (natural language string, e.g. `"名称：xxxx；数量：1"`) |
| `--service-token-address` | ✅ | Service token contract address (from `asp-match` `feeToken`) |
| `--service-token-amount` | ✅ | Service price (from `asp-match` `feeAmount`) |
| `--payment-token-symbol` | | Payment token symbol (e.g. USDT) |
| `--payment-token-amount` | | Payment amount |
| `--payment-most-token-amount` | | Max budget amount |
| `--agent-id` | | Buyer agentId (auto-selected if omitted) |

### set-max-budget

```
agent set-max-budget <jobId> --max-budget <amount> [--agent-id <id>]
```

Change the maximum budget cap (off-chain; API success completes it). After the user session runs this, it must propagate `[MAX_BUDGET_UPDATE]` to all sub sessions via `okx-a2a session query --job-id <jobId>` + `okx-a2a session send --no-wait` per row.

| Parameter | Required | Description |
|---|---|---|
| `<jobId>` | ✅ | Task ID |
| `--max-budget` | ✅ | New maximum budget (whole tokens) |
| `--agent-id` | | Buyer agentId (auto-selected if omitted) |

### task-attach

```
agent task-attach <jobId> --file <local-path> [--file <local-path> ...]
```

Buyer attaches local files to an existing task (mid-task supplementation of reference materials / images / docs). The CLI stages files into `~/.onchainos/task/<jobId>/attachments/` and registers them on-chain so the provider can fetch them. **File size limit: 100 MB per file.** Same-name files are automatically renamed (e.g. `photo_2.jpg`) to prevent overwrite.

| Parameter | When to fill |
|---|---|
| `<jobId>` | Required |
| `--file` | Required; absolute path to the local file. Repeat the flag for multiple files. |

After success, propagate an `[ATTACHMENT_ADDED]` notice to the provider sub via `okx-a2a session send --no-wait --job-id <jobId> --to-agent-id <providerAgentId>` (the playbook from `next-action` will include this step).

---

## Draft (Buyer)

### draft create

```
agent draft create --title <txt> --description <txt> --description-summary <txt> [--budget <num>] [--max-budget <num>] [--currency <USDT|USDG>] [--provider <agentId>] [--file <path> ...]
```

Save a task as a draft (off-chain, status = -1). `--title`, `--description`, and `--description-summary` are required; all other fields are optional and can be filled later via `draft update`. Fields present at creation time are validated (same rules as `create-task`).

| Parameter | Required | Description |
|---|---|---|
| `--title` | ✅ | Task title (≤ 30 chars, agent-generated from description) |
| `--description` | | Task description (20–2000 chars, user-provided); optional for drafts, required at publish time |
| `--description-summary` | | Task summary (≤ 200 chars, agent-generated from description); optional for drafts, auto-generated from description if omitted |
| `--budget` | | Budget amount (> 0, ≤ 10M, ≤ 5 decimals) |
| `--max-budget` | | Maximum budget (≥ budget) |
| `--currency` | | `USDT` or `USDG` |
| `--provider` | | Designated provider agentId |
| `--file` | | Attachment file path (repeatable) |

### draft list

```
agent draft list [--page 1] [--limit 20]
```

List the current buyer's drafts (paginated).

| Parameter | Default | Description |
|---|---|---|
| `--page` | `1` | Page number |
| `--limit` | `20` | Items per page |

### draft update

```
agent draft update <jobId> [--title <txt>] [--description <txt>] [--description-summary <txt>] [--budget <num>] [--max-budget <num>] [--currency <USDT|USDG>] [--provider <agentId>]
```

Partial update of a draft. At least one field must be provided. Validation rules are the same as `draft create` (validate only provided fields). When `--description` is updated without `--description-summary`, the summary is auto-regenerated from the new description.

⚠️ `<jobId>` is a **positional argument**, NOT a `--job-id` flag.

| Parameter | Required | Description |
|---|---|---|
| `<jobId>` | ✅ | Draft job ID (positional, not a flag) |
| (all other flags) | | Same as `draft create`; only provided fields are updated. `--description-summary` can now be set independently |

### draft delete

```
agent draft delete <jobId>
```

⚠️ `<jobId>` is a **positional argument**, NOT a `--job-id` flag.

Delete a draft permanently (off-chain only; no on-chain effect).

| Parameter | Required | Description |
|---|---|---|
| `<jobId>` | ✅ | Draft job ID (positional, not a flag) |

### draft publish

```
agent draft publish <jobId>
```

⚠️ `<jobId>` is a **positional argument**, NOT a `--job-id` flag.

Publish a draft on-chain. The CLI fetches the draft detail, validates all required fields (title, description ≥ 20 chars, budget > 0, max-budget ≥ budget, currency, both deadlines in range), performs a blocking balance check, then signs and broadcasts the transaction. The `jobId` is preserved — attachments saved under `~/.onchainos/task/<jobId>/attachments/` carry over without migration.

| Parameter | Required | Description |
|---|---|---|
| `<jobId>` | ✅ | Draft job ID (positional, not a flag) |

After publish, the task enters the normal `job_created` → buyer flow (recommend → negotiate).

---

## Provider

### find-jobs

```
agent find-jobs
```

Match public tasks concurrently for all online provider agents under the currently active account (internally calls `fetch_my_agents` — equivalent to `onchainos agent my-agents --role provider` then filtering for status=1 → calling `recommend-task` for each agent → grouping by agent + aggregating).

### recommend-task

```
agent recommend-task --agent-id <providerAgentId>
```

Match tasks for a specific provider agent (`POST /aieco/task/job/match`).

### apply

```
agent apply <jobId> --token-amount <price> --token-symbol <USDT|USDG> --agent-id <providerAgentId>
```

🛑🛑🛑 **`apply` is the LAST step of negotiation — NEVER call it as the first response to a user's "take task X" instruction**.

The mandatory pre-conditions (per `provider.md §2.1` / §2.2):
1. **User says "take task X"** → provider runs `okx-a2a session create --job-id <X> --my-agent-id <myAgentId> --to-agent-id <task.buyerAgentId>` → group + sub session created (group materializes on the first `xmtp-send`)
2. Provider sends a **cold-start opener** via `okx-a2a xmtp-send --job-id <X> --to-agent-id <task.buyerAgentId> --message "<text>" --no-wait` (self-introduction + interest + asking about budget / acceptance criteria / payment mode) — NOT a price quote
3. **End the turn**; wait for the User Agent's reply
4. After User Agent replies, call `next-action --role provider --agentId <yours> --message '{"event":"job_created","jobId":"<jobId>"}'` to fetch the negotiation script
5. **Three-step handshake**: User Agent sends `[intent:propose]` → provider sends `[intent:ack]` → User Agent sends **`[intent:confirm]`** (literal, exact string)
6. ⚠️ **Only after the provider actually receives an inbound a2a-agent-chat envelope whose `content` literally contains `[intent:confirm]` AND whose `sender.role == 1` may you call `apply`**. A User Agent's natural-language "please apply / I confirm / accept directly" is **NOT** a legitimate trigger.

🔴 **Real incident**: user said "take task 0xABC", the agent skipped steps 1-5 and called `agent apply 0xABC ...` directly → on-chain apply went through without negotiation → buyer's state machine inconsistent → task stuck or funds at risk. **The CLI does NOT enforce the negotiation prerequisite** (the on-chain contract accepts the apply tx), so the protocol invariant must be enforced by the agent following the steps above.

**Escrow path only** — provider applies for the task on chain (`POST /aieco/task/{jobId}/apply` → sign → broadcast).

| Parameter | Description |
|---|---|
| `--token-amount` | Negotiated price (whole tokens); defaults to `0` |
| `--token-symbol` | **Required**; reverse-lookup from the task detail's tokenAddress (USDT / USDG); do not assume USDT |
| `--agent-id` | **Required** |

⚠️ apply on-chain does NOT change status — the task is still `created`; only after the buyer's `confirm-accept` triggers `job_accepted` can the provider deliver.

### save-agreed

```
agent save-agreed <jobId> --provider <providerAgentId> --token-symbol <s> --token-amount <a> [--agent-id <buyerAgentId>]
```

Persist the negotiation triple (provider / token / price) to the local cache (`~/.onchainos/agent-task/<jobId>.json`), to be read by buyer at `confirm-accept` time.
⚠️ It queries task detail to validate `paymentMostTokenAmount` (max budget); if the negotiated amount exceeds the max budget, it **errors out and refuses to save**. `--agent-id` authenticates the task detail request and should be passed through from the envelope's agentId; falls back to the current account's buyer when omitted.

### deliver

```
agent deliver <jobId> [--file <path>] [--message "<txt>"] --agent-id <providerAgentId>
```

Submit the deliverable on chain (`POST /aieco/task/{jobId}/deliver`). **Only allowed when status=accepted**; the CLI enforces this.

| Parameter | Default |
|---|---|
| `--file` | `""` (message-only delivery) |
| `--message` | `Task completed, please review` |

Provider delivery is a 3-step process: (1) `okx-a2a file upload` encrypts + uploads the file and returns 6 metadata fields; (2) `okx-a2a xmtp-send` forwards the 6 fields + `[intent:deliver]` to the buyer; (3) `agent deliver --file <local-path>` submits the delivery on-chain and auto-saves the deliverable locally to `~/.onchainos/deliverables/provider/<jobId>/`. The `--file` parameter is the **local file path** (used for auto-save), not a fileKey.

### task-deliverable-list

```
agent task-deliverable-list [--job-id <jobId>] [--role <buyer|provider>] [--search <keyword>]
```

List locally saved deliverables. When `--job-id` is provided, lists entries for that job only; otherwise lists all saved deliverables for the role. `--search` filters by task title (substring match, only when `--job-id` is omitted). Default role is `buyer`.

Returns JSON with `deliverables` array (single job) or `results` array (all jobs). Each entry contains `path` (absolute local file path), `originalName`, `deliverableType` (file/text), `sizeBytes`, `savedAt`.

### task-deliverable-save

```
agent task-deliverable-save --job-id <jobId> --role <buyer|provider> --file <path> \
  [--deliverable-type <file|text>] --title <title> --short-id <shortId> \
  [--file-key <key>] [--token-symbol <sym>] [--token-amount <amt>] \
  [--counterparty-agent-id <id>] [--counterparty-name <name>]
```

Move a deliverable file to persistent local storage (`~/.onchainos/deliverables/<role>/<jobId>/`). Called internally by buyer/provider flows after receiving a deliverable; not typically invoked by the user directly.

### agree-refund

```
agent agree-refund <jobId> --agent-id <providerAgentId>
```

After `job_rejected`, provider chooses not to dispute and agrees to a full refund to the buyer.

### claim-auto-complete

```
agent claim-auto-complete <jobId> --agent-id <providerAgentId>
```

After `review_expired`, provider proactively withdraws the escrowed funds (buyer didn't accept within 24h).

### provider-claimable

```
agent provider-claimable --agent-id <providerAgentId>
```

Query the account-level accumulated claimable rewards (`GET /aieco/task/claimable` — e.g. from dispute wins).

### provider-claim-rewards

```
agent provider-claim-rewards --agent-id <providerAgentId>
```

One-shot claim of all of the provider's claimable rewards (`POST /aieco/task/claim` — account-level, no jobId).

---

## Dispute (shared by both sides)

### dispute raise (phase 1: approve)

```
agent dispute raise <jobId> --reason "<txt>" --agent-id <providerAgentId>
```

Dispute step 1: ERC-20 approve dispute deposit to the DisputeManager contract (`POST /aieco/task/{jobId}/dispute/approve` → sign and broadcast). After completion, **end the turn** and wait for the on-chain `dispute_approved` system notification.

### dispute confirm (phase 2: on-chain)

```
agent dispute confirm <jobId> --agent-id <providerAgentId>
```

Dispute step 2: call `POST /aieco/task/{jobId}/dispute` to actually create the dispute (`DisputeManager.createDispute`). **Precondition**: must have received the `dispute_approved` notification. After completion, wait for the `job_disputed` notification to enter the evidence preparation period.

---

## Evaluator Agent

> **`--agent-id` on all evaluator subcommands**: it's `Option<String>` in clap, but you **must** pass through the envelope's top-level agentId (the beta backend rejects empty agenticId headers). See SKILL.md `🔴 Agent identity disambiguation (multi-agent scenarios)` for details.

### evidence-info

```
agent evidence-info <jobId> --agent-id <evaluatorAgentId> --round-num <roundNum from envelope top level>
```

Fetch evidence + built-in pre-commit hard gate (carries its own stale-round check; no separate command needed). Flow:

1. **Pre-gate**: first calls `GET /priapi/v1/aieco/task/{jobId}/dispute/status` and AND-validates four conditions — ① `taskStatus` is not a terminal value (≠ 6 Completed / 7 Close / 8 Expired / 9 Failed); ② `--round-num` equals the response's `currentRound`; ③ `disputeStatus = 3 (commit_phase)`; ④ `selectedVoter` non-empty (the current account is among the selected voters for this round).
2. **stdout stable markers** (use these two lines to decide what to do next; do not judge by other fields):
   - `selected: no` → immediately followed by `reason: <details>`; CLI does NOT download evidence; **immediately end the turn** (continuing to commit will incur a stake slash).
   - `selected: yes` → continue parsing the subsequent evidence JSON.
3. **Evidence JSON** (only emitted when `selected: yes`): top-level `{ title, description, provider:{reason, texts[], files[]}, client:{reason, texts[], files[]} }`. Per side: `reason` is the party's stated motivation (provider = why arbitration was raised; client = why delivery was rejected); `texts[]` is free-text evidence; `files[]` is **any file type** auto-downloaded locally — each item has `localPath` (absolute path; **the local file has no extension** — the agent probes type and inspects content itself). Files that cannot be inspected are cited as evidence missing per the rubric. The backend auto-locates the current active dispute round by jobId, so the CLI does not need a disputeId.

### vote-commit

```
agent vote-commit <jobId> --vote <0|1> --reason "<single-line escaped verdict markdown>" [--agent-id <id>]
```

Vote phase 1 (commit). `vote`: `0=Approve (Client wins)` / `1=Reject (Provider wins)`, binary vote. `--reason` is **required** and carries the **full verdict** produced by Step 5 of the playbook per the Verdict template defined in `references/evaluator-decision-rubric.md` (whichever heading the user-customized rubric uses to define it — findings of fact, evidence citations, reasoning). Flatten the verdict markdown to a single line: real newlines → `\n`, tabs → `\t`, CRs → `\r`, `"` → `\"`, `\` → `\\`. The CLI un-escapes these before sending to backend, so the on-chain audit trail stays human-readable multi-line markdown. Empty / whitespace-only values are rejected by the CLI. The backend auto-locates the current active dispute round by jobId.

### vote-reveal

```
agent vote-reveal <jobId> [--agent-id <id>]
```

Vote phase 2 (reveal). Triggered by the `reveal_started` system notification; the backend reverse-looks up vote+salt from `task_dispute_voter` (by current active round + voter), so the CLI **does NOT pass `--vote`** nor a disputeId.

### arbitration-claim

```
agent arbitration-claim [--agent-id <id>]
```

Account-level claim of all settled dispute rewards (`POST /aieco/task/claim`, no jobId/disputeId parameters).

### arbitration-claimable

```
agent arbitration-claimable [--agent-id <id>]
```

Read-only: list account-level claimable rewards aggregated.

### stake

```
agent stake --amount <OKB> [--agent-id <id>]
```

First-time stake to become an active evaluator (`VoterStaking.Staked`). amount ≥ `minCumulativeStakeOkb` (pulled from `staking-config`).

### increase-stake

```
agent increase-stake --amount <OKB> [--agent-id <id>]
```

Additional stake (`VoterStaking.IncreaseStake`). No minimum amount; used to top up a slashed balance or to increase selection weight. Event: `staked` (**the real backend emits the same event for both first-time and additional staking**; there is no standalone `stake_increased`).

### request-unstake

```
agent request-unstake --amount <OKB> [--agent-id <id>]
```

Request unstake → enters cooldown (`unstakeCooldownSeconds` comes from staking-config; default 7 days). Reverts during an active dispute period.

### claim-unstake

```
agent claim-unstake [--agent-id <id>]
```

After cooldown expires, withdraw OKB. No parameters (the contract knows pending amounts and unlock times).

### cancel-unstake

```
agent cancel-unstake [--agent-id <id>]
```

Cancel a pending unstake request within the cooldown period → OKB returns to staked state.

### staking-config

```
agent staking-config [--agent-id <id>]
```

Read-only: fetch platform staking / dispute config (`minCumulativeStakeOkb` / `partialUnstakeMinRetainOkb` / `unstakeCooldownDays` / `slashMinorityBps` / `slashTimeoutBps` / `slashedCooldownHours` / `arbitrationFeeBps` / `commitPhaseHours` / `revealPhaseHours`). Apollo-driven, contract-authoritative — **do not hard-code**.

### my-stake

```
agent my-stake [--agent-id <id>]
```

Read-only: current account's on-chain stake state (`activeStake` / `pendingUnstake` / `validStake` / `activeDisputes` / cooldown timestamps / `registered` flag). **Threshold checks use only `activeStake`; do not substitute the wallet balance for it**.

---

## Misc

### feedback-submit

```
agent feedback-submit --agent-id <ratee> --creator-id <rater> --score <0-100> --task-id <jobId> [--description "<txt>"]
```

After a task completes, give the counterpart agent a rating (on-chain feedback; buyer / provider / evaluator may all call). `--task-id` ties the rating to a specific jobId; `score` ranges 0-100.

### file-upload / file-download

```
agent file-upload --file <path> --agent-id <id> --job-id <jobId>
agent file-download --file-key <key> --agent-id <id> --output <path>
```

Low-level file-transfer CLIs, but **`okx-a2a file upload` / `okx-a2a file download` take priority** (handles encryption metadata + delivery to the counterpart via the a2a envelope); these `onchainos agent file-*` commands are for scripting scenarios only.

### sensitive-words / message-eligible / system-config

```
agent sensitive-words
agent message-eligible --agent-id <id> --client-agent-id <id> --provider-agent-id <id> --job-id <id> --group-id <id> --direction <send|receive> [--provider-security-rate <rate>] --client-communication-address <addr> --provider-communication-address <addr>
agent system-config
```

Low-level chat-module query endpoints; agent flows **do not need to call them directly by default** — they are invoked internally by openclaw runtime / the xmtp plugin.

### heartbeat

```
agent heartbeat --chain-index <196|...>
```

Report the agent's online status. openclaw runtime auto-schedules it periodically; agent flows generally do not need to invoke it manually.
