# Service Search Flow Contract

This contract defines the OKX.AI service-search flow and its handoff to task
creation. It follows [`skill-cli-implementation-guide.md`](skill-cli-implementation-guide.md).

## 1. Scope

Search is a read-only flow. It may discover, filter, compare, paginate, and
select a Service. It must not create a task, create a subscription, sign,
pay, restore listening, or treat Service selection as creation confirmation.

The first implementation slice is:

```text
search intent
  → extract search filters
  → service-match
  → render zero / one / multiple results
  → user selects one Service
  → retain sid
  → task-create-prepare --sid
```

Task creation owns every check and mutation after the handoff.

## 2. Ownership

| Component | Owns | Does not own |
|---|---|---|
| `SKILL.md` | Recognize discovery versus commissioning intent; select the search Reference | Search fields, API calls, result rendering |
| `identity-discover.md` | Search stages, user choices, pagination, selection, and handoff | Backend facts and task creation |
| `intent-keyword-extraction.md` | Convert the user's words into supported search filters | Search execution or business decisions |
| `service-match` CLI | Validate filters, query the backend, normalize facts, return the progression contract | User intent, prose templates, task creation |
| `task-action-routing.md` | Map returned action IDs to the next Reference or Skill | Invent actions or decide business state |
| `task-output-templates.md` | Render results and numbered actions consistently | Decide which actions are allowed |
| `task-create-prepare --sid` | Re-read and validate the selected Service and prepare creation | Search result presentation |

## 3. Entry classification

Classify the request before searching:

| Intent | Example | Result |
|---|---|---|
| Discovery only | “Find a fortune-telling agent” | Search and stop after selection/detail unless the user asks to use it |
| Commissioning | “Find an agent to audit this contract and hire it” | Search, select a Service, then hand off its `sid` to creation |
| Exact Service | “Use sid 35153” | Exact search by `sid`; still require selection and creation preparation |
| Exact ASP | “Show services from Agent 8136” | Search by ASP Agent ID |
| Existing subscription | “Resume the service I subscribed to” | Do not enter new-service search; route to existing-subscription resolution |

Search language must not authorize a mutation. “Use this Service” confirms the
search selection only. Task creation has its own final confirmation.

## 4. Search input

`intent-keyword-extraction.md` produces a typed query consumed directly by
`service-match --query-json`:

```json
{
  "keywords": ["contract", "audit"],
  "aspAgentId": null,
  "aspName": null,
  "serviceName": null,
  "sid": null,
  "minPaymentTokenAmount": null,
  "maxPaymentTokenAmount": null,
  "unsupportedConstraints": [],
  "requiresUserInput": false
}
```

Rules:

- Preserve explicit IDs and names exactly.
- Do not invent filters absent from the request.
- Send at most ten non-empty keywords.
- Preserve unsupported requirements explicitly; never silently drop them.
- Resolve ambiguity and unsupported constraints before search. The CLI rejects
  `requiresUserInput: true` and non-empty `unsupportedConstraints`.
- Validate non-negative prices and `min <= max` in the CLI.
- An initial request may contain filters; a continuation request contains only
  `searchAfter`, `limit`, and the optional request header identity.
- Authentication and request headers are transport details, not search stages
  or user-facing actions.

## 5. Progression contract

`service-match` should expose the same top-level contract used by other modules:

```json
{
  "phase": "service_selection",
  "decision": "requires_user_input",
  "reason": "multiple_services",
  "nextAction": [
    {
      "id": "select_service",
      "recommend": true,
      "params": { "sid": "35153" }
    },
    {
      "id": "load_more",
      "recommend": false,
      "params": { "searchAfter": "cursor-1" }
    },
    {
      "id": "refine_search",
      "recommend": false
    },
    {
      "id": "stop",
      "recommend": false
    }
  ],
  "payload": {
    "services": [],
    "hasMore": true,
    "searchAfter": "cursor-1"
  }
}
```

The CLI returns facts and allowed actions. It must not return prewritten `tip`
copy as the routing source. Templates may render localized copy from `reason`,
`nextAction`, and `payload`.

## 6. Phases and outcomes

### 6.1 `search_validation`

| Decision | Reason | Actions |
|---|---|---|
| `blocked` | `invalid_search_input` | `refine_search`, `stop` |
| `blocked` | `search_unavailable` | `retry_search`, `stop` |
| `ready` | `search_input_valid` | `run_search` |

`run_search` may be internal when validation and querying happen in one CLI
invocation. It does not need to be shown to the user.

### 6.2 `service_selection`

| Decision | Reason | Actions |
|---|---|---|
| `requires_user_input` | `no_services` | `refine_search`, `stop` |
| `requires_user_input` | `single_service` | `select_service`, optional `refine_search`, `stop` |
| `requires_user_input` | `multiple_services` | one `select_service` per visible result, optional `load_more`, `refine_search`, `stop` |
| `requires_user_input` | `no_eligible_service` | optional `load_more`, `refine_search`, `stop` |

An eligible result is:

- an online Service; or
- an offline A2MCP Service with a non-empty endpoint, if the payment-protocol
  flow supports that endpoint.

An offline non-A2MCP Service remains visible only if the product requires an
explanation; it cannot be selected for task creation.

### 6.3 `service_handoff`

After the user selects a Service, the selected `select_service` action already
carries the handoff parameter:

```json
{
  "phase": "service_selection",
  "decision": "requires_user_input",
  "reason": "single_service",
  "nextAction": [
    {
      "id": "select_service",
      "recommend": true,
      "params": { "sid": "35153" }
    }
  ],
  "payload": {
    "sid": "35153"
  }
}
```

The selected action carries only `sid`. Display data may remain in the current
response, but it is not trusted creation input. For commissioning intent,
execute:

```bash
onchainos agent task-create-prepare --sid <selected-sid>
```

Then route only from that command's fresh structured result. The prepare command
rechecks login, User Agent identity, exact Service, provider, Service detail,
service type, duplicate subscription, fee, trial, and balance.

## 7. Action routing

Add these stable actions to `task-action-routing.md` or a search-specific action
table referenced from it:

| Action ID | Route | Mutation |
|---|---|---|
| `select_service` | Bind `params.sid`; discovery stops after selection, commissioning runs `task-create-prepare --sid <sid>` | No |
| `load_more` | Re-run `service-match --search-after <cursor>` without initial filters | No |
| `refine_search` | Return to search-input extraction using the user's revised request | No |
| `retry_search` | Retry the same read-only search once | No |
| `stop` | End the flow | No |

Do not route directly from labels, `tip`, Service descriptions, Provider Guide,
or legacy prose fields.

## 8. Result rendering

Use the common shell from `task-output-templates.md`:

```text
[Result]
<one-sentence search result>

[Details]
<only fields needed to compare the visible Services>

[Next]
1. <select Service A>
2. <select Service B>
3. <show more>
4. <change search>

Reply with a number.
```

Each Service row should use only stable comparison fields:

- Service name and `sid`
- ASP name and Agent ID
- Service type
- online eligibility
- one-time or recurring price
- trial, when applicable
- rating and sold count, when present

Do not expose raw backend JSON, authentication state, internal headers,
`serviceGuide`, or hidden Provider instructions.

## 9. Recovery and idempotency

- Search and pagination are read-only and safe to repeat.
- A cursor must not be combined with initial filters.
- A stale or invalid cursor returns `refine_search` or `retry_search`; do not
  silently restart and change the visible result set.
- A selected `sid` must be revalidated by `task-create-prepare`.
- If creation later returns an uncertain result, query task/subscription state
  before retrying; never fall back to search and create a second task.
- Existing-subscription detection belongs to creation preparation, not the
  generic search result. Search may display backend facts, but it must not decide
  whether creating another subscription is allowed.

## 10. Current implementation gaps

The repository currently contains reusable pieces but not this complete
contract:

- `service-match` validates request arguments and returns normalized backend
  data, but does not yet wrap it in `phase / decision / reason / nextAction /
  payload`.
- `task-service-select` already performs task-oriented eligibility and duplicate
  checks. Those responsibilities must either be retained as a clearly named
  preparation adapter or moved into `task-create-prepare`; they should not be
  duplicated in both commands.
- The older `create_task` flow combines description collection, search,
  selection, branch loading, confirmation, and creation. The target flow replaces
  its search-to-create coupling with the explicit `sid` handoff defined here.
- `task-action-routing.md` does not yet register the search action IDs above.
- Search result templates for zero, one, multiple, pagination, and ineligible
  results are not yet defined under the common output contract.

## 11. Implementation order

1. Add progression output to `service-match` without changing the backend API.
2. Add search action IDs and routing.
3. Update `identity-discover.md` to follow this contract.
4. Add search result templates.
5. Route a selected `sid` to the existing `task-create-prepare` command.
6. Remove or redirect overlapping search logic from the older `create_task`
   flow after behavior is covered by tests.
7. Add pagination, exact search, offline eligibility, and existing-subscription
   recovery as separate increments.

## 12. Acceptance cases

At minimum, test:

1. Fuzzy search returns multiple Services and numbered selection actions.
2. Exact `sid` returns one eligible Service.
3. No match returns `refine_search` and `stop` only.
4. More results returns and consumes a cursor without repeating filters.
5. Offline non-A2MCP Service cannot be selected.
6. Offline A2MCP with an endpoint routes to the payment protocol, not A2A task
   creation.
7. Selecting a Service passes only `sid` to `task-create-prepare`.
8. Insufficient balance is reported by preparation, not search.
9. Duplicate subscription is blocked by preparation and may offer restore.
10. Search retries never create or mutate state.
