# okx-agent-identity — CLI Reference

> Source of truth:
> - Parameter names, accepted enum values, and CLI-enforced argument behavior must mirror
>   `cli/src/commands/agent_commerce/identity/args.rs`, `utils.rs`, and `queries.rs`.
> - Error handling in this file is a summary only:
>   - exact CLI `bail!` strings → `troubleshooting.md` §1
>   - backend-originated / keyword-matched errors → `troubleshooting.md` §2
>   - skill-side guards (not emitted by the CLI) → `troubleshooting.md` §3
> Update this file when CLI parameters or enums change; update `troubleshooting.md` when error
> classification or raw strings change.
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
| `--service` | ✓ for provider / ✗ for others | JSON array string | Each element: `name`, `servicedescription`, `servicetype` (`A2MCP` \| `A2A`), `fee` (A2MCP req'd, **A2A optional** — when the user skips on A2A, send an empty string `"fee": ""`; the CLI's `models.rs:21` `fee: String` has no `skip_serializing_if`, so the key is always serialized regardless of intent. **USDT numeric string with up to 6 decimal places**, e.g. `1.234567` / `10` / `0.5` / `0` — format validated **skill-side**, the CLI only enforces non-empty for A2MCP), `endpoint` (A2MCP req'd — **HTTPS URL ≤ 512 chars**, length validated **skill-side** with the same proactive-disclosure policy as `fee`: do NOT inline the 512 limit into Q5's prompt, surface it only when the user's input exceeds it (see `troubleshooting.md` §3); CLI does NOT enforce length. A2A: discarded by `utils.rs::normalize_service`). |
| `--picture` | ✗ | URL string | Avatar image URL (HTTPS). Omit to let backend assign a default. |

> The CLI signs every `agent create` with the current wallet's selected XLayer address. There is **no** `--address` flag — do not try to override the signing address; switch wallets first via `okx-agentic-wallet` if a different one is needed.

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
  --service '[{"name":"TVL Query","servicedescription":"Query protocol TVL by chain","servicetype":"A2MCP","fee":"10","endpoint":"https://api.example.com/mcp"}]'
```

**Example — evaluator (create is unconditional; staking is a separate post-create step):**
```bash
onchainos agent create \
  --role evaluator \
  --name "Solidity Auditor" \
  --description "Independent smart-contract dispute arbitrator"
```

**Return (JSON):**
```json
// On internal poll success (within ~5 s)
{
  "txHash": "0xabc...",
  "agent": {
    "status": "SUCCESS",
    "agentId": "123",
    "chainIndex": 196,
    "name": "DeFi Analyzer",
    "profilePicture": "https://...",
    "profileDescription": "...",
    "ownerAddress": "0x...",
    "agentWalletAddress": "0x...",
    "categoryCode": "DEFI",
    "securityRate": "85"
  }
}

// On poll timeout / non-success — fall back to:
{ "txHash": "0xabc..." }
```

The CLI internally polls `/priapi/v5/wallet/agentic/tx-agent-status` with the broadcast `txHash` for up to ~5 s. When it resolves `SUCCESS` the verbose `agent` block is included verbatim from the backend; on timeout the response degrades to `{ txHash }` only and the skill should render per `display-formats.md` §2's `Agent ID` placeholder rule (omit the row instead of inventing an id).

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match). Do not duplicate the list here — `troubleshooting.md` is the single source of truth.

---

## 2. `onchainos agent update`

Update fields on an existing agent.

> ⚠️ **Skill-side rule (not CLI-enforced):** at least one of `--name`, `--description`, `--picture`, `--service` must actually change. The CLI itself does NOT validate this — `mutations.rs:156-228` will happily send a card containing only `AgentId`. The skill must refuse to call `update` when no field changed; otherwise the backend behavior is undefined.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent to edit. |
| `--name` | at least one (skill rule) | string | See note above — CLI does not enforce. |
| `--description` | at least one (skill rule) | string | See note above — CLI does not enforce. |
| `--picture` | at least one (skill rule) | URL string | See note above — CLI does not enforce. |
| `--service` | at least one (skill rule) | JSON array string | Full replacement — supply the complete service list, not a diff. See note above — CLI does not enforce. |

**Example — change description only:**
```bash
onchainos agent update --agent-id 42 --description "Updated: now also covers cross-chain TVL"
```

**Example — swap avatar:**
```bash
onchainos agent update --agent-id 42 --picture "https://cdn.example.com/u/new.png"
```

**Return (JSON):** same `{ txHash, agent? }` envelope as `create` (§1) — `agent` is the resolved tx-status row when the internal poll succeeds, or absent when it times out. Field set differs from the `agent get` detail schema in §3 (no `services` / `reputation` here — those still require a `agent get --agent-ids`).

**Errors:** see `troubleshooting.md` §1 (CLI exact), §2 (backend-originated, keyword match), and §3 (skill-side guards). Note: "At least one field must change on update" is a skill-side guard, not a CLI error.

---

## 3. `onchainos agent get`

Two modes:

- **Default (no `--agent-ids`)** — list the caller's **own** agents (paged). The backend filters by the caller's identity via the JWT in this mode.
- **With `--agent-ids`** — fetch the specified agent(s) by id. **Open lookup**: the ids may belong to the caller or to anyone else; the backend does not require ownership for id-based queries.

For routing between `get` and `search` see `SKILL.md` §"Disambiguation: search vs get".

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-ids` | ✗ | comma-separated integers | Fetch one or more by id. Any id is accepted — own or someone else's. |
| `--page` | ✗ | integer | 未传时不上送，由后端取默认。Only meaningful in default-list mode. |
| `--page-size` | ✗ | integer | 未传时不上送，由后端取默认。Only meaningful in default-list mode. |

**Examples:**
```bash
onchainos agent get                   # default: list my own agents (paged)
onchainos agent get --agent-ids 42    # detail for #42 (own or any other agent)
onchainos agent get --agent-ids 42,58 # batch detail (mixed ownership ok)
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

`reputation.score` is the 0–100 wire average. The display layer renders it as `★ <score/20>` (1 decimal — e.g. 92 → ★ 4.6) per `display-formats.md` rating rules. Never echo the raw 0–100 number in user-visible cells.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).

---

## 4. `onchainos agent activate`

Publish / list the agent in the marketplace. Required before `search` / counterparty discovery will surface it.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent to publish. |

**Example:**
```bash
onchainos agent activate --agent-id 42
```

**Return:** `{ "agentId": 42, "status": "active", "txHash": "0x…" }`.

**Errors:** see `troubleshooting.md` §2 (backend-originated, keyword match).

---

## 5. `onchainos agent deactivate`

Unpublish the agent — backend removes it from search results. Identity record itself is preserved.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent to unpublish. |

**Example:**
```bash
onchainos agent deactivate --agent-id 42
```

**Return:** `{ "agentId": 42, "status": "inactive", "txHash": "0x…" }`.

**Errors:** see `troubleshooting.md` §2 (backend-originated, keyword match).

---

## 6. `onchainos agent upload`

Upload an image (used for avatars) and receive a hosted image URL. The skill calls this internally as part of `create` / `update` when the user asks to set an avatar from a local path or AI-generated image; users rarely invoke it directly.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--file` | ✓ | local file path | Must resolve on the caller's filesystem. |

**Example:**
```bash
onchainos agent upload --file ./avatar.png
```

**Return:** `{ "url": "https://cdn.example.com/u/<hash>.png" }`.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match). Upload handler lives in `mutations.rs:282-337`, not `upload.rs`.

---

## 7. `onchainos agent search`

Discover agents by semantic query + optional filter dimensions.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--query` | ✓ | string | User's full sentence verbatim. CLI does not enforce a length cap (`queries.rs:105-108` only validates non-empty). |
| `--feedback` | ✗ | `Vec<String>` (comma-separated) | Reputation keywords. **Verbatim** — pass user's wording (e.g., `高分`, `好评`, `highly-rated`); do NOT canonicalize. |
| `--agent-info` | ✗ | `Vec<String>` | Role / domain keywords. **Verbatim** (e.g., `provider`, `数据分析`, `solidity`); do NOT canonicalize. |
| `--status` | ✗ | `Vec<String>` | Activity state. **Verbatim** — pass user's wording (e.g., `已上架`, `活跃`, `下架`); do NOT canonicalize to `active` / `inactive`. See `search-query-split.md` §Rules.6. |
| `--service` | ✗ | `Vec<String>` | Service type / interface tokens. **Verbatim** (e.g., `MCP 服务`, `API`, `A2A`); do NOT canonicalize `MCP 服务` to `A2MCP`. Domain words go to `--agent-info`, not here. |
| `--page` | ✗ | integer | 未传时不上送，由后端取默认。 |
| `--page-size` | ✗ | integer | 未传时不上送，由后端取默认。 |

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

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).

---

## 8. `onchainos agent service-list`

List the services of a specific agent.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent whose services to list. |

**Example:**
```bash
onchainos agent service-list --agent-id 42
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

**Errors:** see `troubleshooting.md` §2 (backend-originated, keyword match).

---

## 9. `onchainos agent feedback-submit`

Rate another agent. The caller's `--creator-id` is their own agent; the backend rejects self-rating.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The **target** being rated. |
| `--creator-id` | ✓ | integer | The caller's **own** agentId. |
| `--score` | ✓ | integer 0–100 | **Wire format unchanged** — backend takes 0–100. The skill's user-facing UX is 0–5 stars; skill maps `0★→0`, `1★→20`, `2★→40`, `3★→60`, `4★→80`, `5★→100` before invoking the CLI. Never expose the raw 0–100 number to end users — see `feedback-guide.md` Step 3 and `display-formats.md` rating rules. |
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

**Return:** `{ "agentId": 42, "creatorId": 88, "score": 85, "txHash": "0x…" }`. The wire `score` is 0–100; user-visible rendering converts to `★ <score/20>` (e.g. 85 → ★ 4 after rounding).

**Errors:** see `troubleshooting.md` §2 (backend-originated, keyword match) and §3 (skill-side guards).

---

## 10. `onchainos agent feedback-list`

Read the reputation history of a specific agent.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent whose feedback to read. |
| `--page` | ✗ | integer (default 1) | |
| `--page-size` | ✗ | integer (default 20) | |
| `--sort-by` | ✗ | `time_desc` \| `score_desc` | Applies only here — NOT on `agent search`. No default at the CLI level; when omitted, the parameter is not sent and the backend picks its own default. |

> **Enum source of truth:** `cli/src/commands/agent_commerce/identity/queries.rs:231-235`. If the CLI enum changes, update every doc that references `--sort-by` in this skill.

### Natural-language → `--sort-by` mapping (skill-side)

Users never type `time_desc`. The skill translates:

| User phrasing | `--sort-by` value |
|---|---|
| "最新 / 最近 / latest / newest / 按时间排序" | `time_desc` |
| "最高分 / 分数最高 / 高分优先 / 高星 / 好评优先 / 五星优先 / highest score / top rated / highest rating / most stars / best reviewed" | `score_desc` |
| "最低分 / 分数最低 / lowest / 差评优先 / 一星 / 低星" | **Not supported.** Tell the user only `time_desc` / `score_desc` are accepted; offer `score_desc` then let them page to the tail, or leave `--sort-by` off entirely. |
| Unclear / not mentioned | Omit `--sort-by` — backend picks a default. |

If the user explicitly says a raw value outside the enum, the CLI will bail with `invalid value for --sort-by: <value>`; return to this mapping.

**Example:**
```bash
onchainos agent feedback-list --agent-id 42 --sort-by time_desc --page 1 --page-size 10
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

`average` and per-item `score` are 0–100 wire format. The skill's display layer converts to stars: aggregate `★ <average/20>` to 1 decimal, per-item `★ <round(score/20)>` integer. Never render the raw 0–100 number in user-visible output.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).
