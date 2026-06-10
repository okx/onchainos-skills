# CLI Reference — agent create

> Part of `core/cli-reference.md`. Covers `onchainos agent create` in full detail.
> For §2–§6 (update / get / activate / deactivate / upload) see `core/cli-reference.md`.
> For §7–§11 (search / service-list / feedback-submit / feedback-list / submit-approval) see `core/cli-search-feedback.md`.

## 1. `onchainos agent create`

Register a new ERC-8004 agent on XLayer.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--role` | ✓ | `requester` \| `provider` \| `evaluator` | Aliases `1` / `buyer` / `requestor` → requester; `2` → provider; `3` → evaluator. Always emit canonical lowercase. |
| `--name` | ✓ | string | User-visible display name. |
| `--description` | ✓ for provider / ✗ for others | string | 1–2 sentence description. **CLI enforces non-empty for `--role provider` only** (`mutations.rs::create_impl` role-conditional gate); requester / evaluator may omit it, in which case the wire payload sends `ProfileDescription: ""` (same shape as `picture` when skipped). Skill renders the empty value as `(not set)` per `core/field-specs.md §Description`. |
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

After broadcasting, the CLI keeps the WS subscription it opened *before* broadcast (`wallet-agentic-identity` channel; default URL `wss://wsdex.okx.com:8443/ws/v5/private`) and waits up to **30 s** for a push whose top-level `txHash` matches the broadcast hash (case-insensitive, `0x` prefix optional). When matched, the push payload — `{agentId, chainIndex, status, name, profilePicture, profileDescription, txHash}` — is included verbatim under `agent`. After WS resolves (match or timeout), the CLI also pages `GET /agent/agent-list?chainIndex=196&page=N&pageSize=100` until `total` is satisfied (or a 20-page safety cap is hit, in which case the partial aggregate is logged) and attaches the assembled `{ total, list }` under `agentList` (note the field is `list`, not `items` — backend's `/agent/agent-list` response uses `list`; this was empirically confirmed on 2026-05-10 after an earlier doc-only mismatch). Both segments are **best-effort and independent**: `agent` is present iff the WS push matched in time; `agentList` is present iff every paginated HTTP call succeeded (any single page failure short-circuits to absent rather than emitting a misleading partial). Either may be absent without affecting the other; both absent degrades to `{ txHash }` only — and in that case the skill should render per `core/display-formats.md` §2's `Agent ID` placeholder rule (omit the row instead of inventing an id).

⚠️ **agentList envelope shape (double-layer).** `agentList.list[*]` is **not** an agent row — it is an **accountName wrapper** `{ownerAddress, accountName, agentList:[agent_row, ...]}` (one wrapper per derived wallet that the JWT caller has visibility into). The actual agent rows are nested one level deeper at `agentList.list[*].agentList[*]`. `agentList.total` counts wrappers (= accountName groups), **not** total agent rows; `fetch_agent_list`'s page-termination compares aggregated wrapper count against this `total`, which is correct as long as the consumer treats `list[*]` as wrappers. Agent-row internal fields (`agentId`, `name`, `role`, `status`, `description`, `picture`, `services`, `reputation`, etc.) are unchanged from prior revisions — only the outer envelope grew the wrapper layer.

**Finding the newly-minted `agentId` from this envelope:** because the envelope is **double-layer** (see ⚠️ above), `ownerAddress` lives on the **wrapper** (`list[*].ownerAddress`), **NOT** on individual agent rows (agent rows under `list[*].agentList[*]` carry `agentId` / `name` / `role` / `status` / `description` / `picture` / `services` / `reputation` — no `ownerAddress` key). The correct filter is therefore **two steps, in this order**:

1. **Wrapper layer (filter):** locate the single wrapper in `agentList.list[*]` whose `list[*].ownerAddress == <currently selected XLayer wallet address>` (the address that signed this `create`). At most one wrapper matches; if none matches, the envelope carries no rows for the signing wallet — skip step 2 and fall back to each role file's omit-`#<id>` branch.
2. **Agent-row layer (diff):** inside that wrapper's `agentList[*]` only, pick the `agentId` that did **not** exist in the pre-check `agent get` snapshot.

❌ **Common mistake — do NOT write the filter as `agentList[*].ownerAddress == ...`.** That phrasing treats `ownerAddress` as an agent-row field, which it is not; the comparison silently fails for every row, the diff yields no candidate, and the role file's "diff yielded no new candidate" branch fires — i.e. the model omits `#<id>` even when the data is present. The layer matters.

Do **not** cross-account aggregate — other wrappers' `agentList` belong to other derived wallets and must not be conflated with the caller's own.

**WS URL override**: production uses `WS_URL_PROD = wss://wsdex.okx.com:8443/ws/v5/private` from `cli/src/commands/agent_commerce/identity/utils.rs` (mirrors the `WS_URL_PROD` + `ONCHAINOS_WS_URL` env-override pattern in `cli/src/watch/daemon.rs`). For dev / pre / forked envs, set the `OKX_AGENTIC_WS_URL` env var to the **full** WS URL (including the `/ws/v5/private` path); the CLI uses the env value verbatim, no scheme swap or path forcing.

⚠️ **Breaking change from earlier revisions**: the HTTP base URL (`--base-url`, runtime `OKX_BASE_URL`, or compile-time `OKX_BASE_URL`) **no longer affects the WS connect**. When you switch HTTP targets (`--base-url https://pre.example.com`, etc.), you must **also** set `OKX_AGENTIC_WS_URL` to the corresponding WS endpoint, otherwise the WS subscription still hits `wss://wsdex.okx.com:8443/ws/v5/private` (prod). The failure mode is **silent**: `agent create` / `agent update` still succeed (broadcast + agentList both work via HTTP), but the `agent` field in the response envelope is absent because the WS push never reaches the matching host.

**Errors:** see `troubleshooting.md` §1 (CLI exact) and §2 (backend-originated, keyword match). Do not duplicate the list here — `troubleshooting.md` is the single source of truth.

> **Consent is no longer part of `agent create`.** Earlier revisions piggy-backed the legal-module terms flow on the create call (`--consent-key` / `--agreed` request fields + a `consent` response field). That has been removed: `agent create` neither sends nor returns consent fields. The terms flow is now the standalone `agent consent` command (§1.5), run **before** identity info is collected. See `playbooks/consent.md` for the skill-side flow.

---

## 1.5 `onchainos agent consent`

First-time-creation terms consent (legal module). Standalone two-step flow, decoupled from
`create`, run at Core Flow gate 3 — **after pre-check, before any identity Q&A**. No signing,
no broadcast. `fromAddr` + `chainIndex` are auto-filled from the current selected XLayer wallet.

| Parameter | Required | Type | Notes |
|---|---|---|---|
| `--consent-key` | ✗ (step 2 only) | string | The one-time `consentKey` returned by step 1; pass back on step 2 together with `--agreed`. |
| `--agreed` | ✗ (step 2 only) | bool | `true` = user agreed, `false` = user declined. Must be paired with `--consent-key`. |

**Step 1 — fetch terms (no flags):**
```bash
onchainos agent consent
```
Return:
```json
// First-time user → terms issued
{ "required": true, "consent": { "consentKey": "550e8400-...-446655440000", "terms": "Agent service terms ..." } }

// Returning user (already owns an agent) / feature flag off → nothing to show
{ "required": false, "consent": null }
```

**Step 2 — finalize decision (`--consent-key` + `--agreed`):**
```bash
onchainos agent consent --consent-key "550e8400-...-446655440000" --agreed true   # agree
onchainos agent consent --consent-key "550e8400-...-446655440000" --agreed false  # decline
```
Return on finalize: `{ "required": false, "consent": null }`.

Behavior summary:
- `required: false` from step 1 → skip the consent card, continue straight to Role Q&A.
- `required: true` → render the consent card (`playbooks/consent.md`), collect agree/decline,
  then call step 2. Decline → stop, do NOT enter `agent create`.
- The CLI normalizes the backend's `data: []` (existing-agent / flag-off / post-finalize) to
  `required: false` / `consent: null`; a non-null `consent` object only appears on step 1 for a
  first-time wallet.

**Endpoint:** `POST /priapi/v5/wallet/agentic/pre-transaction/agent-consent` (auth required; no
signature check; `userId` injected from JWT server-side).

**Errors:** `81001` (chainIndex invalid) / `40020` (consentKey without agreed) / `40021`
(agreed without consentKey, or key invalid/finalized) / `40022` (already declined) — see
`troubleshooting.md`.

---

