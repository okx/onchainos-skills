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
| `--description` | ✓ for provider / ✗ for others | string | 1–2 sentence description. **CLI enforces non-empty for `--role provider` only** (`mutations.rs::create_impl` role-conditional gate); requester / evaluator may omit it, in which case the wire payload sends `ProfileDescription: ""` (same shape as `picture` when skipped). Skill renders the empty value as `未填` / `(not set)` per `field-specs.md §Description`. |
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
// On WS push match within 30 s of broadcast — agent + agentList both present
{
  "txHash": "0xabc...",
  "agent": {
    "agentId": "12345",
    "chainIndex": 196,
    "status": "SUCCESS",
    "name": "DeFi Analyzer",
    "profilePicture": "https://...",
    "profileDescription": "...",
    "txHash": "0xabc..."
  },
  "agentList": {
    "total": 2,
    "list": [
      {
        "ownerAddress": "0xfa3...",
        "accountName": "wallet-1",
        "agentList": [
          { "agentId": 12345, "name": "DeFi Analyzer", "role": "provider", "status": "active", "...": "..." }
        ]
      },
      {
        "ownerAddress": "0xfa4...",
        "accountName": "wallet-2",
        "agentList": [ /* agents owned under this derived wallet */ ]
      }
    ]
  }
}

// On WS timeout / connect failure — `agent` absent, `agentList` still attempted
{ "txHash": "0xabc...", "agentList": { "total": 2, "list": [ /* 2 accountName wrappers */ ] } }

// On both WS and agent-list failing — degrades to:
{ "txHash": "0xabc..." }
```

After broadcasting, the CLI keeps the WS subscription it opened *before* broadcast (`wallet-agentic-identity` channel; default URL `wss://wsdex.okx.com:8443/ws/v5/private`) and waits up to **30 s** for a push whose top-level `txHash` matches the broadcast hash (case-insensitive, `0x` prefix optional). When matched, the push payload — `{agentId, chainIndex, status, name, profilePicture, profileDescription, txHash}` — is included verbatim under `agent`. After WS resolves (match or timeout), the CLI also pages `GET /agent/agent-list?chainIndex=196&page=N&pageSize=100` until `total` is satisfied (or a 20-page safety cap is hit, in which case the partial aggregate is logged) and attaches the assembled `{ total, list }` under `agentList` (note the field is `list`, not `items` — backend's `/agent/agent-list` response uses `list`; this was empirically confirmed on 2026-05-10 after an earlier doc-only mismatch). Both segments are **best-effort and independent**: `agent` is present iff the WS push matched in time; `agentList` is present iff every paginated HTTP call succeeded (any single page failure short-circuits to absent rather than emitting a misleading partial). Either may be absent without affecting the other; both absent degrades to `{ txHash }` only — and in that case the skill should render per `display-formats.md` §2's `Agent ID` placeholder rule (omit the row instead of inventing an id).

⚠️ **agentList envelope shape (double-layer).** `agentList.list[*]` is **not** an agent row — it is an **accountName wrapper** `{ownerAddress, accountName, agentList:[agent_row, ...]}` (one wrapper per derived wallet that the JWT caller has visibility into). The actual agent rows are nested one level deeper at `agentList.list[*].agentList[*]`. `agentList.total` counts wrappers (= accountName groups), **not** total agent rows; `fetch_agent_list`'s page-termination compares aggregated wrapper count against this `total`, which is correct as long as the consumer treats `list[*]` as wrappers. Agent-row internal fields (`agentId`, `name`, `role`, `status`, `description`, `picture`, `services`, `reputation`, etc.) are unchanged from prior revisions — only the outer envelope grew the wrapper layer.

**Finding the newly-minted `agentId` from this envelope:** because the envelope is **double-layer** (see ⚠️ above), `ownerAddress` lives on the **wrapper** (`list[*].ownerAddress`), **NOT** on individual agent rows (agent rows under `list[*].agentList[*]` carry `agentId` / `name` / `role` / `status` / `description` / `picture` / `services` / `reputation` — no `ownerAddress` key). The correct filter is therefore **two steps, in this order**:

1. **Wrapper layer (filter):** locate the single wrapper in `agentList.list[*]` whose `list[*].ownerAddress == <currently selected XLayer wallet address>` (the address that signed this `create`). At most one wrapper matches; if none matches, the envelope carries no rows for the signing wallet — skip step 2 and fall back to each role file's omit-`#<id>` branch.
2. **Agent-row layer (diff):** inside that wrapper's `agentList[*]` only, pick the `agentId` that did **not** exist in the pre-check `agent get` snapshot.

❌ **Common mistake — do NOT write the filter as `agentList[*].ownerAddress == ...`.** That phrasing treats `ownerAddress` as an agent-row field, which it is not; the comparison silently fails for every row, the diff yields no candidate, and the role file's "diff yielded no new candidate" branch fires — i.e. the model omits `#<id>` even when the data is present. The layer matters.

Do **not** cross-account aggregate — other wrappers' `agentList` belong to other derived wallets and must not be conflated with the caller's own.

**WS URL override**: production uses `WS_URL_PROD = wss://wsdex.okx.com:8443/ws/v5/private` from `cli/src/commands/agent_commerce/identity/utils.rs` (mirrors the `WS_URL_PROD` + `ONCHAINOS_WS_URL` env-override pattern in `cli/src/watch/daemon.rs`). For dev / pre / forked envs, set the `OKX_AGENTIC_WS_URL` env var to the **full** WS URL (including the `/ws/v5/private` path); the CLI uses the env value verbatim, no scheme swap or path forcing.

⚠️ **Breaking change from earlier revisions**: the HTTP base URL (`--base-url`, runtime `OKX_BASE_URL`, or compile-time `OKX_BASE_URL`) **no longer affects the WS connect**. Prior revisions derived the WS URL from the HTTP base via scheme swap + `/ws/v5/private` append; that coupling has been removed. When you switch HTTP targets (`--base-url https://pre.example.com`, etc.), you must **also** set `OKX_AGENTIC_WS_URL` to the corresponding WS endpoint, otherwise the WS subscription still hits `wss://wsdex.okx.com:8443/ws/v5/private` (prod). The failure mode is **silent**: `agent create` / `agent update` still succeed (broadcast + agentList both work via HTTP), but the `agent` field in the response envelope is absent because the WS push never reaches the matching host.

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

**Return (JSON):** same `{ txHash, agent?, agentList? }` envelope as `create` (§1) — `agent` is the matched `wallet-agentic-identity` push when one arrives within 30 s of broadcast, or absent on timeout / WS failure; `agentList` carries the paginated `{ total, list }` aggregate (note the field is `list`, not `items`) and may also be absent on HTTP failure. Field set on `agent` differs from the `agent get` detail schema in §3 (no `services` / `reputation` here — those still require a `agent get --agent-ids`).

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

**Return (JSON, double-layer envelope — both list mode and detail mode):**
```json
{
  "total": 2,
  "list": [
    {
      "ownerAddress": "0xfa3...",
      "accountName": "wallet-1",
      "agentList": [
        { "agentId": 42, "name": "DeFi Analyzer", "role": "provider", "status": "active",
          "description": "...", "picture": "https://...", "address": "0x...",
          "services": [...], "reputation": { "score": 92, "count": 18 } },
        { "agentId": 58, "name": "MyBuyer", "role": "requester", "status": "active", "...": "..." }
      ]
    },
    {
      "ownerAddress": "0xfa4...",
      "accountName": "wallet-2",
      "agentList": [ /* agents under this derived wallet */ ]
    }
  ],
  "page": 2,
  "pageSize": 50
}
```

⚠️ **Envelope is double-layer in BOTH modes.** The outer `list[*]` is an **accountName wrapper** (one per derived wallet the JWT caller has visibility into), not an agent row. The actual agent rows live at `list[*].agentList[*]`. `total` counts wrappers (= accountName groups), **not** agent rows. Even in `--agent-ids <N>` (detail) mode the envelope keeps this shape — `list[0].agentList[0]` is typically where the single matched agent sits (the backend still groups by accountName).

**Agent-row internal fields are unchanged** from prior revisions — `agentId`, `name`, `role`, `status`, `description`, `picture`, `address`, `services`, `reputation: { score, count }` keep their semantics and types. This envelope change only adds the outer wrapper layer; nothing inside an agent row was renamed or reshaped.

(Note the array field is `list`, not `items`. `agent get` calls the same `/agent/agent-list` endpoint that powers `agent create` / `update`'s post-broadcast `agentList` segment in §1; the two diverge slightly in post-processing: `agent get` returns a single backend page verbatim including `page` / `pageSize` echoed back from the request, while §1's `agentList` is the **aggregate across all pages** assembled by `fetch_agent_list` and only carries `{ total, list }` — `page` / `pageSize` lose coherent meaning after cross-page aggregation and are dropped on purpose.)

`reputation.score` is the 0–100 wire average. The display layer renders it as `★ <score/20>` to 1 decimal place via the canonical **round-half-up** rule (see `SKILL.md §Amount Display Rules` reputation block — e.g. `92 → ★ 4.6`, `89 → ★ 4.5`, `85 → ★ 4.3`). Never echo the raw 0–100 number in user-visible cells.

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
| `--score` | ✓ | integer 0–5 (stars) | CLI accepts 0–5 stars and multiplies by 20 internally to produce the 0–100 backend wire value (`utils::stars_to_score`). Out-of-range / non-integer input rejected by `parse_u32_arg`. The 0–100 wire format is encapsulated by the CLI; callers / skill code pass user stars directly. |
| `--description` | ✗ | string | 1–3 sentence rationale. |
| `--task-id` | ✗ | string | Free-form; usually a `jobId` from `okx-agent-task`. |

There is **no** `--tx-hash` parameter (tx hash is returned, not supplied).

**Example:**
```bash
onchainos agent feedback-submit \
  --agent-id 42 \
  --creator-id 88 \
  --score 4 \
  --description "交付及时、数据准确" \
  --task-id "0xabc...03e8"
```

**Return:** `{ "txHash": "0x…" }`. The submitted star count is not echoed back; if the skill needs to confirm what was just submitted, it should track the user's star input itself.

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
  "average": 4.6,
  "items": [
    { "creatorId": 88, "score": 5, "description": "...", "taskId": "...", "createdAt": "..." }
  ]
}
```

`average` and per-item `score` are already in **0–5 stars** when the CLI surfaces them. The CLI applies `utils::convert_feedback_list_scores` to the backend response before returning: `average` becomes a 1-decimal float (e.g. backend `89` → `4.5`) and per-item `score` becomes an integer 0–5 (e.g. backend `70` → `4`). The skill just renders `★ <average>` / `★ <score>` directly. Backend wire format is still 0–100 — encapsulated by `utils::score_to_stars_decimal` / `utils::score_to_stars_int` with the canonical **round-half-up** rule pinned in `SKILL.md §Amount Display Rules`.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).
