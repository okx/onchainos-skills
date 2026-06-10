# okx-agent-identity — CLI Reference

> Source of truth:
> - Parameter names, accepted enum values, and CLI-enforced argument behavior must mirror
>   `cli/src/commands/agent_commerce/identity/args.rs`, `utils.rs`, and `queries.rs`.
> - Error handling in this file is a summary only:
>   - exact CLI bail! strings → see `troubleshooting.md` §1 in the skill root
>   - backend-originated errors → see `troubleshooting.md` §2 in the skill root
>   - skill-side guards → see `troubleshooting.md` §3 in the skill root
>
> The skill exposes **10** commands. `onchainos agent xmtp-sign` is a low-level primitive and is intentionally not listed — do not suggest it to users.

## Table of Contents

| Section | Command | Purpose |
|---|---|---|
| **§1** | `agent create` | Register new agent (requester / provider / evaluator); finding newly-minted agentId |
| **§1.5** | `agent consent` | First-time-creation terms consent (legal module; standalone, runs before create) |
| **§2** | `agent update` | Update existing agent fields (name / description / picture / services) |
| **§3** | `agent get` | List own agents (default) or fetch by id(s); double-layer envelope structure |
| **§4** | `agent activate` | Publish agent; 5 outcome branches + approvalStatus handling |
| **§5** | `agent deactivate` | Unpublish agent |
| **§6** | `agent upload` | Upload local image → returns HTTPS URL |
| **§7–§11** | (moved) | Search, service-list, feedback-submit, feedback-list, submit-approval → see `core/cli-search-feedback.md` |

---


> **§1 `onchainos agent create`** and **§1.5 `onchainos agent consent`** have been moved to `core/cli-create.md` (create parameters, return schema, agentId resolution algorithm; standalone consent command). Consent is no longer carried by `create`.

## 2. `onchainos agent update`

Update fields on an existing agent.

> ⚠️ **Skill-side rule (not CLI-enforced):** at least one of `--name`, `--description`, `--picture`, `--service` must actually change. The CLI itself does NOT validate this — `mutations.rs:156-228` will happily send a card containing only `AgentId`. The skill must refuse to call `update` when no field changed; otherwise the backend behavior is undefined.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent to edit. |
| `--name` | at least one (skill rule) | string | — |
| `--description` | at least one (skill rule) | string | — |
| `--picture` | at least one (skill rule) | URL string | — |
| `--service` | at least one (skill rule) | JSON array string | Full replacement — supply the complete service list, not a diff. |

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

For routing between `get` and `search` see `SKILL.md` §Intent → Sub-flow.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-ids` | ✗ | comma-separated integers | Fetch one or more by id. Any id is accepted — own or someone else's. |
| `--page` | ✗ | integer | Omitted when not provided; backend uses its default. Only meaningful in default-list mode. |
| `--page-size` | ✗ | integer | Omitted when not provided; backend uses its default. Only meaningful in default-list mode. |

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

**Agent-row internal fields** — `agentId`, `name`, `role`, `status`, `description`, `picture`, `address`, `services`, `reputation: { score, count }` keep their semantics and types. Two additional fields are returned by the backend:

| Field | Type | Values |
|---|---|---|
| `approvalDisplayStatus` | Integer | `1` Not listed / `2` Listing under review / `4` Listed — eligible for task recommendations / `5` Listing rejected / `7` This agent is currently unavailable |
| `approvalRemark` | String | Reviewer's remark (filled by the approver; explains reason when rejected; may be empty string) |

`approvalDisplayStatus` is independent of `status` (the on-chain publish state). Render it per `core/ux-lexicon.md §ApprovalDisplayStatus`; never expose the raw integer to the user.

(Note the array field is `list`, not `items`. `agent get` calls the same `/agent/agent-list` endpoint that powers `agent create` / `update`'s post-broadcast `agentList` segment in §1; the two diverge slightly in post-processing: `agent get` returns a single backend page verbatim including `page` / `pageSize` echoed back from the request, while §1's `agentList` is the **aggregate across all pages** assembled by `fetch_agent_list` and only carries `{ total, list }` — `page` / `pageSize` lose coherent meaning after cross-page aggregation and are dropped on purpose.)

`reputation.score` is the 0–100 wire average. The display layer renders it as `★ <score/20>` with **up to 2 decimal places** (see the score÷20 formula). Because wire is an integer 0–100, `score/20` is exact at 2 decimals (one wire unit = 0.05 stars) — no further rounding. Examples: `100 → ★ 5`, `92 → ★ 4.6`, `89 → ★ 4.45`, `85 → ★ 4.25`, `70 → ★ 3.5`, `66 → ★ 3.3`, `0 → ★ 0`. Trailing zeros are trimmed in display (`4.5` not `4.50`). Never echo the raw 0–100 number in user-visible cells.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).

---

## 4. `onchainos agent activate`

Publish / list the agent in the marketplace. Required before `search` / counterparty discovery will surface it.

Underlying API: `POST /priapi/v5/wallet/agentic/agent-status` with `status: 1`.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent to publish. |

**Example:**
```bash
onchainos agent activate --agent-id 42
```

**Return fields (`data` object):**

| Field | Type | Description |
|---|---|---|
| `success` | boolean | `true` = listed successfully; `false` = listing not effective, check `approvalStatus` |
| `approvalStatus` | integer \| null | Review status: `1` = not submitted / `2` = under review / `5` = rejected; `null` when `success=true` |
| `rejectReason` | string \| null | Rejection reason; non-null only when `approvalStatus=5` |

**Five possible outcomes:**

```json
// Outcome A — Listed immediately
{ "success": true, "approvalStatus": null, "rejectReason": null }

// Outcome B — Review required, not yet submitted
// → Skill MUST call onchainos agent submit-approval --agent-id <id>
{ "success": false, "approvalStatus": 1, "rejectReason": null }

// Outcome C — Already under review
{ "success": false, "approvalStatus": 2, "rejectReason": null }

// Outcome D — approvalStatus 5 (treated the same as Outcome B: resubmit, no rejection message)
// → Skill MUST call onchainos agent submit-approval --agent-id <id>; do NOT surface rejectReason
{ "success": false, "approvalStatus": 5, "rejectReason": "Content does not meet listing guidelines" }

// Outcome E — Agent blacklisted (top-level error, outside data object)
{ "code": "81602", "msg": "Agent is blocked", "data": null }
```

**Skill-side handling (reads `success` + `approvalStatus` + top-level `code`, NOT just HTTP status):**

| Condition | Skill action |
|---|---|
| `success: true` | ✅ Published — render success line + proceed to `SKILL.md §Operation Flow Step 5` → `§Step 6` |
| `success: false`, `approvalStatus: 1` **or** `5` | Same handling for both: run `modules/pre-listing-qa.md`; if QA passes → call `onchainos agent submit-approval --agent-id <id>` (`cli-search-feedback.md §11`); if QA fails → render QA report and stop. ⛔ For `5`, do NOT render a rejection card and do NOT surface `rejectReason` — proceed silently to submit-approval. |
| `success: false`, `approvalStatus: 2` | Under review — render review-pending message per `troubleshooting.md §2` and **stop** (no Step 5/6) |
| Top-level `code: "81602"` | Agent blacklisted — render blacklist error per `troubleshooting.md §2` and **stop** |

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).

---

## 5. `onchainos agent deactivate`

Unpublish the agent — backend removes it from search results. Identity record itself is preserved.

Underlying API: `POST /priapi/v5/wallet/agentic/agent-status` with `status: 2`.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--agent-id` | ✓ | integer | The agent to unpublish. |

**Example:**
```bash
onchainos agent deactivate --agent-id 42
```

**Return fields (`data` object):**

| Field | Type | Description |
|---|---|---|
| `success` | boolean | `true` = unpublished successfully; `false` = unpublish failed |
| `approvalStatus` | integer \| null | Ignored in the deactivate scenario |
| `rejectReason` | string \| null | Ignored in the deactivate scenario |

**Skill-side handling:** only read `success`. `approvalStatus` and `rejectReason` are ignored for deactivate.

| Condition | Skill action |
|---|---|
| `success: true` | ✅ Unpublished — render deactivate success line + proceed to `§Step 5` → `§Step 6` |
| `success: false` | ❌ Failure — render error card per `troubleshooting.md` and **stop** |

Business-level failures (e.g. "agent already inactive", "pending settlements") arrive as `code != "0"` from the backend — they are caught before `success` is evaluated and surfaced via `troubleshooting.md §2` keyword match.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match).

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

> **§7–§11** (search, service-list, feedback-submit, feedback-list, submit-approval) have been moved to `core/cli-search-feedback.md`.

