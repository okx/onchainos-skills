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

Follow the returned script verbatim. For any route where `next-action` returns a confirmation form, that
returned form is the sole field authority: do not supplement or merge it with Appendix A. Appendix A is
only a fallback render contract for a direct route that does not receive a CLI-provided form.

---

## Appendix A1: Regular Task Confirmation Card Template

> **Scope:** fallback/direct routes only. If `next-action` returned a confirmation form, use that form
> verbatim and do not add any Appendix A1 fields to it.

Display as a single `| Field | Value |` table with exactly these **5** fields in order (drop `Summary`, `Service`, `Service desc`, `Payment mode`, `Payment Currency`, `Budget`, `Maximum Budget`):

| # | Field | Source | Render Rule |
|---|---|---|---|
| 1 | Task Name | Agent-generated Title | ≤30 characters |
| 2 | Task Description | User Description | Inline when ≤200 characters; when >200, show `See below` and render the full text below the table |
| 3 | Provider | task-service-select / designated-route | `Agent <providerAgentId>(<providerAgentName>)`; fall back to `Agent <providerAgentId>` |
| 4 | Service Parameters | Agent-inferred | `None` when empty |
| 5 | Service Price | task-service-select `feeAmount` + `feeTokenSymbol` | Zero (number or numeric string) → localized `Free`; otherwise `<feeAmount> <symbol>`; **omit the row when `feeAmount` is absent** |

If attachments present, add an Attachments row.

Execution mode, per-signal amount, and per-signal cap are internal execution configuration. Never add them
to this or any other confirmation form, even when they appear in the user request, service description, or
retained context.

Initialize internal `budget` and `max-budget` from the selected service `feeAmount`; a zero service fee produces `budget=0` and `max-budget=0` and remains publishable. Never ask for them initially and never show them in this card. Continue collecting and validating Payment Currency internally for A2A and x402, but do not show it because Service Price already includes the currency. A user may explicitly edit budget fields to zero before `create-task`, subject to `max-budget >= budget`; validate and confirm the proposed values separately, then re-render this card without budget rows.

End with a localized confirmation blockquote and wait for explicit confirmation.

---

## Appendix A2: Subscription Task Confirmation Card Template

> **Scope:** fallback/direct routes only. If `next-action` returned a confirmation form, use that form
> verbatim and do not add any Appendix A2 fields to it.

Display as a single `| Field | Value |` table with these **7 base fields** in order (drop `Summary`, `Service`, `Service desc`, and the old binary execution switch):

| # | Field | Source | Render Rule |
|---|---|---|---|
| 1 | Task Name | Agent-generated Title | ≤30 characters |
| 2 | Task Description | User Description | Inline when ≤200 characters; when >200, show `See below` and render the full text below the table |
| 3 | Provider | task-service-select | `Agent <providerAgentId>(<providerAgentName>)`; fall back to `Agent <providerAgentId>` when unnamed |
| 4 | Service Parameters | Agent-inferred from `serviceDescription` | `None` when empty |
| 5 | Service Price | task-service-select `subscriptionInfo.feeAmount` | `<fee> <symbol> / month`; never use the one-time `fee` / `feeAmount` field |
| 6 | Trial | task-service-select `subscriptionInfo.supportTrial/freeTrial` | A positive `freeTrial` → `Yes (<freeTrial> hours free)`; otherwise `No` |
| 7 | Auto-Renew | Explicit user choice; no default | `On` or `Off` |

Execution mode, per-signal amount, and per-signal cap are internal execution configuration. Never add them
to this or any other confirmation form, even for a trading-signal subscription. Automatic execution remains
the default. The ASP description may define which fields to ask about, but only user-authored replies supply
persisted values. Ask any ASP-required missing fields in one natural-language question without a choice card,
retain the answers outside the form, and pass them through the existing `--autotrade-*` arguments.

If attachments present, add an Attachments row.

Before displaying this confirmation table, inspect advisory readiness. A non-ready or authorization-not-
checked Trade Kit must show the separate optional two-choice preparation card from the CLI playbook:
install/configure Trade Kit, or Later and continue subscribing. On prepare, first load `okx-cex-auth`
directly when it is already installed. Only when it is unavailable, scope the required security scan to
`okx/agent-skills`; after a passing scan, run `npx skills add okx/agent-skills --yes --global` and load
`okx-cex-auth`. Delegate all CLI/OAuth/API-key setup to that skill, then re-run readiness.
Never duplicate those auth steps here, auto-install, or block subscription creation. Other tool reminders
remain concise notices without choices.

End with a localized confirmation blockquote and wait for explicit confirmation.

---

## Edit-action matrix (applies to both A1 and A2)

Every modification is confirmed individually (Universal confirmation rule). After any edit, re-render the corresponding confirmation card (A1 or A2).

| User action | Handling |
|---|---|
| Confirm & publish | Run `create-task` (regular) / `create-subscribe` (subscription) **without** any `descriptionSummary` — the field no longer exists |
| Edit description | Re-parse search intent and **immediately re-run `task-service-select`**; the re-match may change the recommended service/provider and may **switch the branch** (subscription ↔ regular) — re-render the matching card |
| Edit service params | Update in place → re-render |
| Edit budget / max-budget (regular, pre-create only) | Validate without auto-adjusting the other field → separately confirm concrete value(s) → update existing fields → re-render without budget rows |
| Edit payment token (regular) | Update in place → re-validate → re-render |
| Edit auto-renew (subscription) | Update in place → re-render |
| Edit automatic execution / amount / cap / quote (subscription) | Update user-authored values; cap remains informational → re-render |
| Change provider / service before creation | Re-run `task-service-select` (may switch branch) → discard prior budget/max-budget edits → reset both fields from the newly selected service `feeAmount` → re-render |

**Branch-switch rule (FR-2.5)**: when an edited Description changes the matched service type (subscription ↔ regular), clear the previous branch's type-specific fields. On entry to the regular branch, reset existing `budget` / `max-budget` from the newly selected service fee and collect Payment Currency; for subscription, collect Trial / Auto-Renew. If re-match is empty, follow the `matchStatus` recovery in the common publishing playbook.

**Provider render**: use `Agent <providerAgentId>(<providerAgentName>)`, falling back to `Agent <providerAgentId>` when the name is empty/absent.

---

## 5. Designated-Provider x402 flow

**Trigger**: user message contains "Please use onchainos to send a request to this endpoint".

Parse `agentId` and `endpoint`; retain `serviceId` when the caller already supplied one.

**Flow**:
1. **Resolve the registered service**:
   `onchainos agent designated-route --provider <agentId> [--service-id <serviceId>] --endpoint <endpoint>`
   - `serviceId` is the primary selector; `endpoint` validates the same record. Without `serviceId`, an ambiguous endpoint requires the user to select a service; never pick the first.
   - Continue only for `route=x402` with valid registered service fields and `feeAmount`. Offline is allowed; stop on any error.
2. **Validate the endpoint using the original pre-create flow**:
   `onchainos agent x402-check --endpoint <endpoint>`
   - `valid=false` + `inputRequired=true` → retain `fields` / `requiredAnyOf` and continue.
   - `valid=false` without `inputRequired` → stop. A `tokenSymbol` other than USDT/USDG → stop.
   - Treat `amountHuman` / `tokenSymbol` as endpoint price/payment data only; never use them to initialize or silently overwrite task budget/max-budget/currency.
3. **Collect fields and confirm**:
   - Generate task fields from the registered service listing. Collect its declared inputs plus every retained `inputRequired` field; never infer required values.
   - Set `budget` and `max-budget` to registered `feeAmount`, including zero; set `currency` to the registered service `feeTokenSymbol`.
   - Follow Appendix A1 and the Edit-action matrix, then wait for explicit confirmation.
4. **Create after confirmation**:
   `onchainos agent create-task --description "<description>" --title "<title>" --budget <budget> --max-budget <max_budget> --currency <feeTokenSymbol> --provider <agentId> --service-id <serviceId> --endpoint <endpoint> --payment-mode x402 [--service-params "<params>"] [--service-token-address <feeToken>] --service-token-amount <feeAmount> [--body '<serviceBody JSON>']`
   - Include `--body` only when endpoint fields were collected. After creation, budget/max-budget are locked; follow CLI `next-action`.
