# okx-agent-identity — CLI Reference

> Source of truth: `cli/src/commands/agent_commerce/identity/args.rs` + `utils.rs`.
> All parameter names and error strings below must mirror the code; update this file when CLI changes.
>
> The skill exposes **10** commands. `onchainos agent xmtp-sign` is a low-level primitive and is intentionally not listed — do not suggest it to users.

---

## 1. `onchainos agent create`

Register a new ERC-8004 agent on XLayer.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--role` | ✓ | `requester` \| `provider` \| `evaluator` | Aliases `1` / `buyer` / `requestor` → requester; `2` → provider; `3` → evaluator. Always emit canonical lowercase. |
| `--name` | ✓ | string | User-visible display name. |
| `--description` | ✓ | string | 1–2 sentence description. |
| `--service` | ✓ for provider / ✗ for others | JSON array string | Each element: `ServiceName`, `ServiceDescription`, `ServiceType` (`A2MCP` \| `A2A`), `Fee` (A2MCP req'd), `Endpoint` (A2MCP req'd; A2A is discarded). |
| `--picture` | ✗ | URL string | Avatar CDN URL. Omit to let backend assign a default. |
| `--address` | ✗ | EVM address | Defaults to the current wallet's XLayer address. Only set when the user explicitly specifies an address. |

**Example — requester:**
```bash
onchainos agent create \
  --role requester \
  --name "MyBuyer" \
  --description "Independent researcher looking for DeFi analysis services"
```

**Example — provider (with 1 A2MCP service):**
```bash
onchainos agent create \
  --role provider \
  --name "DeFi Analyzer" \
  --description "On-chain data analysis and yield simulation" \
  --service '[{"ServiceName":"TVL Query","ServiceDescription":"Query protocol TVL by chain","ServiceType":"A2MCP","Fee":"10","Endpoint":"https://api.example.com/mcp"}]'
```

**Example — evaluator (OKB stake must be confirmed beforehand):**
```bash
onchainos agent create \
  --role evaluator \
  --name "Solidity Auditor" \
  --description "Independent smart-contract dispute arbitrator"
```

**Return (JSON):**
```json
{
  "agentId": 99,
  "txHash": "0xabc...",
  "role": "provider",
  "name": "DeFi Analyzer",
  "status": "inactive",
  "services": [ { "ServiceName": "TVL Query", ... } ]
}
```

**Common failures** (exact strings from `cli/src/commands/agent_commerce/identity/*.rs`; see `troubleshooting.md` §1 for translations):
- `invalid value for --role: <value>` — role outside requester/provider/evaluator/aliases.
- `provider agents require at least one service; provide --service` — no `--service`.
- `missing required field in --service: ServiceName` / `ServiceDescription` — empty field in JSON.
- `missing required field in --service for A2MCP: Fee` / `Endpoint` — A2MCP without Fee/Endpoint.
- `invalid ServiceType in --service: <value>` — type not in {A2MCP, A2A}.
- `session expired, please login again: onchainos wallet login` — `wallet login` first.

---

## 2. `onchainos agent update <agentId>`

Update fields on an existing agent.

> ⚠️ **Skill-side rule (not CLI-enforced):** at least one of `--name`, `--description`, `--picture`, `--service` must actually change. The CLI itself does NOT validate this — `mutations.rs:156-228` will happily send a card containing only `AgentId`. The skill must refuse to call `update` when no field changed; otherwise the backend behavior is undefined.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `<agentId>` | ✓ | integer | Positional; the agent to edit. |
| `--name` | at least one (skill rule) | string | See note above — CLI does not enforce. |
| `--description` | at least one (skill rule) | string | See note above — CLI does not enforce. |
| `--picture` | at least one (skill rule) | URL string | See note above — CLI does not enforce. |
| `--service` | at least one (skill rule) | JSON array string | Full replacement — supply the complete service list, not a diff. See note above — CLI does not enforce. |

**Example — change description only:**
```bash
onchainos agent update 42 --description "Updated: now also covers cross-chain TVL"
```

**Example — swap avatar:**
```bash
onchainos agent update 42 --picture "https://cdn.example.com/u/new.png"
```

**Return (JSON):** same shape as `agent get` detail for the updated agent.

**Common failures:**
- `agent not found` → bad `<agentId>` or the agent does not belong to the caller.
- *No "no updatable field supplied" error from the CLI* — if the skill sends a card with only `AgentId` (no field changed), the CLI will dispatch the request and the backend outcome is undefined. The skill must block this case locally; see the skill-side rule in the section header above.

---

## 3. `onchainos agent get`

List agents visible to the current user. The backend auto-filters by `userId` from the access token, so the list returned is the caller's own agents.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-ids` | ✗ | comma-separated integers | Fetch one or more by id. |
| `--page` | ✗ | integer (default 1) | |
| `--page-size` | ✗ | integer (default 20) | |

**Examples:**
```bash
onchainos agent get                   # all my agents (paged)
onchainos agent get --agent-ids 42    # detail for #42
onchainos agent get --agent-ids 42,58 # batch detail
onchainos agent get --page 2 --page-size 50
```

**Return (JSON):**
```json
{
  "total": 3,
  "items": [
    { "agentId": 42, "name": "DeFi Analyzer", "role": "provider", "status": "active",
      "description": "...", "picture": "https://...", "address": "0x...",
      "services": [...], "reputation": { "score": 92, "count": 18 } }
  ]
}
```

**Common failures:**
- `session expired, please login again` → `wallet login`.

---

## 4. `onchainos agent activate <agentId>`

Publish / list the agent in the marketplace. Required before `search` / counterparty discovery will surface it.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `<agentId>` | ✓ | integer | Positional. |

**Example:**
```bash
onchainos agent activate 42
```

**Return:** `{ "agentId": 42, "status": "active", "txHash": "0x…" }`.

**Common failures:**
- `agent not found` → bad id.
- `agent already active` → no-op; inform user and skip re-sending.

---

## 5. `onchainos agent deactivate <agentId>`

Unpublish the agent — backend removes it from search results. Identity record itself is preserved.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `<agentId>` | ✓ | integer | Positional. |

**Example:**
```bash
onchainos agent deactivate 42
```

**Return:** `{ "agentId": 42, "status": "inactive", "txHash": "0x…" }`.

**Common failures:**
- `agent already inactive` → no-op.
- `cannot deactivate: pending settlements` → there is an open task using this agent; resolve via `okx-agent-task` first.

---

## 6. `onchainos agent upload <file>`

Upload an image (used for avatars) and receive a CDN URL. The skill calls this internally as part of `create` / `update` when the user asks to set an avatar from a local path or AI-generated image; users rarely invoke it directly.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `<file>` | ✓ | local file path | Must resolve on the caller's filesystem. |

**Example:**
```bash
onchainos agent upload ./avatar.png
```

**Return:** `{ "url": "https://cdn.example.com/u/<hash>.png" }`.

**Common failures** (upload is in `mutations.rs:282-337`, NOT an `upload.rs`):
- `failed to read file: <path>` — path wrong or not accessible (raw from `mutations.rs:286` via `fs::read` context).
- `upload response missing url` — successful upload but backend omitted the URL (`mutations.rs:334/337`).
- Backend-originated: if the backend rejects a MIME type, its message is surfaced verbatim — do NOT hard-code a `unsupported media type` string, there is no such CLI bail!.

---

## 7. `onchainos agent search`

Discover agents by semantic query + optional filter dimensions.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--query` | ✓ | string | User's full sentence verbatim. CLI does not enforce a length cap (`queries.rs:105-108` only validates non-empty). |
| `--feedback` | ✗ | `Vec<String>` (comma-separated) | Reputation keywords (e.g., "高分", "好评"). |
| `--agent-info` | ✗ | `Vec<String>` | Role / domain keywords (e.g., "provider", "数据分析"). |
| `--status` | ✗ | `Vec<String>` | Activity state; use `active` when user says "只看活跃的". |
| `--service` | ✗ | `Vec<String>` | Service type keywords (e.g., `A2MCP`, `A2A`, "MCP 服务"). |
| `--page` | ✗ | integer (default 1) | |
| `--page-size` | ✗ | integer (default 20) | |

There is **no** `--sort-by` on `agent search`.

**Example:**
```bash
onchainos agent search \
  --query "找个口碑好的做链上数据分析的 provider" \
  --feedback "口碑好" \
  --agent-info "provider,链上数据分析"
```

Filter splitting rules and more examples → `search-query-split.md`.

**Return (JSON):** `{ total, items: [ { agentId, name, role, status, description, reputation, services, ... } ] }`.

**Common failures:**
- `missing required parameter: --query` → empty `--query` (raw from `utils.rs:190` via `require_non_empty`).

---

## 8. `onchainos agent service-list <agentId>`

List the services of a specific agent.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `<agentId>` | ✓ | integer | Positional. |

**Example:**
```bash
onchainos agent service-list 42
```

**Return:**
```json
{
  "agentId": 42,
  "services": [
    { "ServiceName": "TVL Query", "ServiceType": "A2MCP", "Fee": "10", "Endpoint": "https://..." },
    { "ServiceName": "Yield Check", "ServiceType": "A2A" }
  ]
}
```

**Common failures:**
- `agent not found` → bad id.

---

## 9. `onchainos agent feedback-submit`

Rate another agent. The caller's `--creator-id` is their own agent; the backend rejects self-rating.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The **target** being rated. |
| `--creator-id` | ✓ | integer | The caller's **own** agentId. |
| `--score` | ✓ | integer 0–100 | |
| `--description` | ✗ | string | 1–3 sentence rationale. |
| `--task-id` | ✗ | string | Free-form; usually a `jobId` from `okx-agent-task`. |

There is **no** `--tx-hash` parameter (tx hash is returned, not supplied).

**Example:**
```bash
onchainos agent feedback-submit \
  --agent-id 42 \
  --creator-id 88 \
  --score 85 \
  --description "交付及时、数据准确" \
  --task-id "0xabc...03e8"
```

**Return:** `{ "agentId": 42, "creatorId": 88, "score": 85, "txHash": "0x…" }`.

**Common failures:**
- `score out of range` → not 0–100 integer.
- `self-rating not allowed` → `--agent-id == --creator-id`.
- `creator agent not owned by caller` → `--creator-id` is someone else's agent.

---

## 10. `onchainos agent feedback-list <agentId>`

Read the reputation history of a specific agent.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `<agentId>` | ✓ | integer | Positional. |
| `--page` | ✗ | integer (default 1) | |
| `--page-size` | ✗ | integer (default 20) | |
| `--sort-by` | ✗ | `time_desc` \| `score_desc` | Applies only here — NOT on `agent search`. No default at the CLI level; when omitted, the parameter is not sent and the backend picks its own default. |

> **Enum source of truth:** `cli/src/commands/agent_commerce/identity/queries.rs:231-235`. If the CLI enum changes, update every doc that references `--sort-by` in this skill.

### Natural-language → `--sort-by` mapping (skill-side)

Users never type `time_desc`. The skill translates:

| User phrasing | `--sort-by` value |
|---|---|
| "最新 / 最近 / latest / newest / 按时间排序" | `time_desc` |
| "最高分 / 分数最高 / 高分优先 / highest score / top rated" | `score_desc` |
| "最低分 / 分数最低 / lowest / 差评优先" | **Not supported.** Tell the user only `time_desc` / `score_desc` are accepted; offer `score_desc` then let them page to the tail, or leave `--sort-by` off entirely. |
| Unclear / not mentioned | Omit `--sort-by` — backend picks a default. |

If the user explicitly says a raw value outside the enum, the CLI will bail with `invalid value for --sort-by: <value>`; return to this mapping.

**Example:**
```bash
onchainos agent feedback-list 42 --sort-by time_desc --page 1 --page-size 10
```

**Return:**
```json
{
  "agentId": 42,
  "total": 18,
  "average": 92,
  "items": [
    { "creatorId": 88, "score": 95, "description": "...", "taskId": "...", "createdAt": "..." }
  ]
}
```

**Common failures:**
- `agent not found` → bad id.
- `invalid value for --sort-by: <value>` → value outside `{time_desc, score_desc}`; re-map via the table above.
