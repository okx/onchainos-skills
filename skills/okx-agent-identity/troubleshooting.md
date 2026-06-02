# Troubleshooting — Errors → user-friendly translation

Two classes of errors to translate:

1. **CLI-emitted `bail!`** — raw strings emitted from `cli/src/commands/agent_commerce/identity/*.rs` (and a couple from shared modules). These are stable: if the CLI source changes, this table must change in the same commit. Each row cites the source file + line.
2. **Backend-originated errors** — surfaced by `format_api_error` / the API client. The exact wording can drift without a CLI code change; match on keywords, not equality. Treat the "CLI error" column here as best-effort.

If you encounter a string that isn't in either table, surface the raw message in the error card footer and ask the user how to proceed — do NOT auto-retry or auto-translate.

---

## 1. CLI-emitted `bail!` (verified)

> **Note on ordering:** this table is "if you see error X, do action Y"; it is **NOT** a guarantee that X is the first error a misuse will trigger. The CLI runs `auth refresh → network setup → parameter validation` in that order, so a user who is both unauth'd **and** missing params will see `session expired` first, then `missing required parameter: <flag>` only after they re-login. The skill should normally catch missing params upfront (`SKILL.md §Step 2: Collect Parameters`) before invoking the CLI, so end users rarely see the CLI-emitted param errors at all; this table is mostly for skill debugging and direct-CLI scripting.

| CLI error (exact) | Source | User-facing translation | Skill action |
|---|---|---|---|
| `session expired, please login again: onchainos wallet login` | `signing.rs:66/68/74/139/141` (shared: `agentic_wallet/auth.rs:44/76/285`) | "Session expired." | Hand off to `okx-agentic-wallet` → `wallet login`, then retry the original command. |
| `no XLayer address found in current account` | `signing.rs:33/42` | "No XLayer address found in the current account." | Hand off to `okx-agentic-wallet` → `wallet add` / `wallet switch`. |
| `missing required parameter: <flag>` | `utils.rs:238` | "Parameter `<flag>` cannot be empty." | Re-ask that specific field. For `--agent-id`, ask the user which agent; run `agent get` if needed. For `--file`, ask for the file path. |
| `error: unexpected argument '<value>' found` (positional rejected by clap) | clap default | "This command requires named flags; bare values are not accepted." | The user passed something like `agent update 42`; tell them to use `agent update --agent-id 42`. Same for `activate` / `deactivate` / `service-list` / `feedback-list` (`--agent-id`) and `upload` (`--file`). |
| `missing required field in --service: name` | `utils.rs:200` | "Service name cannot be empty." | Return to `playbooks/provider.md` Phase 2 per-service Q1 (`name`). |
| `missing required field in --service: servicedescription` | `utils.rs:203` | "Service description cannot be empty." | Return to `playbooks/provider.md` Phase 2 per-service Q2 (`servicedescription`). |
| `missing required field in --service for A2MCP: fee` | `utils.rs:212` | "API-interface service (pay-per-call, fixed price) requires a fee (USDT numeric, ≤ 6 decimal places)" — Pattern A (long form inline gloss) per `core/ux-lexicon.md §Service-type`, since error messages are teaching contexts | Return to `playbooks/provider.md` Phase 2 per-service Q4 (A2MCP branch — internal label). |
| `missing required field in --service for A2MCP: endpoint` | `utils.rs:215` | "API-interface service (pay-per-call, fixed price) requires an endpoint (HTTPS URL)" — Pattern A per `core/ux-lexicon.md §Service-type` | Return to `playbooks/provider.md` Phase 2 per-service Q5 (A2MCP branch — internal label). |
| `invalid servicetype in --service: <value>` | `utils.rs:218` | "Service type must be one of: API-interface service (pay-per-call, fixed price) or agent-to-agent service (negotiated / off-chain pricing)" — Pattern A (long form inline gloss) per `core/ux-lexicon.md §Service-type`; no raw `A2MCP` / `A2A` to the user | Return to `playbooks/provider.md` Phase 2 per-service Q3 (numbered prompt). |
| `invalid value for --role: <value>` | `utils.rs:229` | "Role must be one of: User Agent / Agent Service Provider (ASP) / Evaluator Agent" — never render the raw ERC-8004 enum (`requester` / `provider` / `evaluator`) to the user; the wire mapping happens skill-side | Return to role selection (SKILL.md §Core Flow). |
| `invalid value for <flag>: expected integer` | `utils.rs:267` | "`<flag>` must be an integer." | Re-ask that field. |
| `invalid value for <flag>: must be >= <min>` | `utils.rs:270` | "Minimum value for `<flag>` is `<min>`." | Re-ask that field. |
| `invalid value for <flag>: must be <= <max>` | `utils.rs:278` | "Maximum value for `<flag>` is `<max>`." | Re-ask that field. |
| `provider agents require at least one service; provide --service` | `utils.rs:248` | "An Agent Service Provider (ASP) needs at least one service." — no raw `provider` literal in user text (Red line 4 + `core/ux-lexicon.md §Role` localizes both languages) | Return to role-playbook `provider` service Q&A loop. |
| `invalid value for --sort-by: <value>` | `queries.rs:234` | "Sort value must be `time_desc` or `score_desc`." | Re-map via `core/cli-reference.md` §10 natural-language table. |
| `failed to read file: <path>` | `mutations.rs:286` (`fs::read` context) | "Cannot read this file." | Ask the user to recheck the path; in terminal mode switch to AI-gen / skip (see `modules/avatar-upload.md`). |
| `upload response missing url` | `mutations.rs:334/337` | "Upload succeeded but the backend returned no URL." | Retry once; if persists, surface and ask. |
| `xmtp-sign response missing signature` | `mutations.rs:489` | (not user-facing — `xmtp-sign` is not exposed by this skill) | Log; do not route here. |

---

## 2. Backend-originated (CLI passes through)

> ⚠️ Wording may drift without a CLI code change — match on keywords, not equality. None of these correspond to a CLI `bail!` in `identity/*.rs`. If the backend returns a string you don't recognize, show it verbatim in the error card footer and ask the user.

| Typical backend string (keyword match) | User-facing translation | Skill action |
|---|---|---|
| `user is not in approved agent whitelist` / `not in approved agent whitelist` / `approved agent whitelist` / backend code `10016` | "Your account is not in the agent beta whitelist yet. Apply here: `<URL extracted verbatim from the backend msg field>`. We'll email you when you're approved; come back to register the agent then." | Render error card (`core/display-formats.md §7`). **URL extraction**: use regex `https?://\S+?(?=[\s)）"'.,;]|$)` to grab the first URL from the backend `msg` field (the lookahead treats trailing punctuation such as periods / commas / semicolons as terminators, avoiding dirty values like `https://x.com/y.`). Render the URL verbatim — do NOT rewrite language path segments such as `/zh-hans/`; keep the URL exactly as the backend returned it even if the user is interacting in English. **Never auto-retry** — the user must apply first, then return after receiving an approval email; do NOT issue any further `agent create` / `agent update` calls. If no recognizable URL is found in `msg` (rare), place the entire `msg` verbatim in the error card footer `raw:` line and replace the "Apply here: …" sentence with "Contact OKX support for the application portal." |
| `agent not found` / any 404-shaped response | "Agent not found." | Verify the id with `agent get`; maybe the user misread. |
| `agent already active` | "Agent is already active." | No-op; show detail card. |
| `agent already inactive` | "Agent is already inactive." | No-op; show detail card. |
| `pending settlements` / `cannot deactivate` | "There's still an unsettled task on this agent; we need to close that out first before deactivating — want me to take you there?" | If user agrees, hand off to the task marketplace flow internally (do not name the skill in user text — Red line 1). |
| `stake` / `staking` / `insufficient` (**not expected** on `agent create --role evaluator` — `create` doesn't consume the stake; if it ever appears it's a backend anomaly) | "The backend returned a staking-related error. This is not a normal create failure path — agent registration does not require staking." | Surface the raw message verbatim in the error card footer; point the user at `/skills/okx-agent-task/references/evaluator-staking.md` for the staking flow; do NOT cache drafts or invent a resume keyword. |
| `score out of range` | "Rating must be 0.00–5.00 with at most 2 decimal places." (skill speaks stars; do not echo the raw 0–100 bound from the backend message — see `modules/feedback.md` Step 3) | Return to `modules/feedback.md` step 3. |
| `self-rating not allowed` | "You cannot rate your own agent." | Return to `modules/feedback.md` step 1 (target). |
| `creator agent not owned by caller` | "The reviewer must be an agent owned by your current wallet — let me re-check which of your agents under the current wallet can act as reviewer." (no `--creator-id` flag in user text — Red line 2; use `core/ux-lexicon.md §Field` mapping `creator-id` → reviewer; deliberately neutral — does NOT promise "pick one" or "selection step" because ladder 2's next move depends on the count) | Return to `modules/feedback.md §Step 2` and re-run ladder 2 from the top — the next user-visible message is whichever of the **3 branches** ladder 2 lands in: **0 agents** under current wallet → STOP and offer registration (do NOT promise to auto-pick); **1 agent** → silently use it and mention the choice in the next confirmation; **multiple agents** → ask the user with the numbered-options prompt and wait — `Do not auto-pick`. ⛔ The error-line wording above MUST stay neutral ("re-check which … can act as reviewer") — do NOT commit to "pick one and retry" or "redo the selection step", because for the 0-agent branch there is nothing to pick and for the multi-agent branch the user has to choose. |
| `Wallet API server error (HTTP 500)` | "Backend temporarily unavailable." | Retry once (network-transient policy, §General principles). If persists, surface and move on. |
| Region-restriction codes `50125` / `80001` | "Service is not available in your region." | Do NOT echo the raw code. Do NOT suggest VPNs. |
| TEMP MOCK empty `txHash` on pre-transaction | "Transaction not yet on-chain (temporary mock path taken); please check status again later." | Log event; once the CLI mock path is removed, delete this row. |
| `agent activate` returns `success: false, approvalStatus: 2` — already under review | "Your agent is currently under review — results are usually ready within 24 hours; once approved, your agent will appear on the marketplace." | **Stop.** Do NOT call `submit-approval` — a review is already in flight. No `§Step 5` / `§Step 6`. |
| `agent activate` returns `success: false, approvalStatus: 5` — review rejected | "Listing review failed." + (when `rejectReason` non-empty) "Reason: `<rejectReason>`." + "You can update the agent's name or services based on the feedback, then say "activate #\<id\>" and I'll resubmit." | Render the rejection card with `rejectReason` (omit the reason line if `rejectReason` is empty or null). **Stop.** Do NOT auto-retry; do NOT call `submit-approval` again. No `§Step 5` / `§Step 6`. |
| `agent activate` / `agent submit-approval` returns top-level `code: "81602"` (keyword: `81602` or `blocked`) — agent blacklisted | "This agent has been blocked by the platform and cannot be operated at this time." | **Stop.** Do NOT suggest re-activating or updating. No `§Step 5` / `§Step 6`. |
| `agent submit-approval` returns `success: true` — submission accepted, review now pending | "Done — your agent has been submitted for listing review. Results are usually ready within 24 hours; once approved, your agent will appear on the marketplace." | **Stop.** No `§Step 5` / `§Step 6`. |
| `agent submit-approval` returns `success: false` with a non-blacklist error | "Failed to submit for listing review." + (show `msg` verbatim in error card footer) + "You can try again later." | Render error card. **Stop.** |
| Backend code `40020` — `AGENT_CONSENT_AGREED_REQUIRED` (consentKey was passed but `agreed` field was omitted) | "Consent parameters incomplete — registration failed. Please restart the registration flow." | Render error card with `raw:` message. **Stop.** Do NOT auto-retry. |
| Backend code `40021` — `AGENT_CONSENT_INVALID` (key invalid / already finalized, or `agreed` passed without `consentKey`) | "Consent token is invalid or already used — registration failed. Please restart the registration flow." | Render error card with `raw:` message. **Stop.** Do NOT auto-retry. |
| Backend code `40022` — `AGENT_CONSENT_REJECTED` (user already declined consent in a prior session) | "You previously declined the terms of service — registration cannot proceed. To register, please restart the full registration flow." | **Complete stop.** Do NOT offer a retry or a way to re-agree in this same flow. The user must restart from scratch. No `§Step 5` / `§Step 6`. |

---

## 3. Not errors — actions that never reach the CLI

Some conditions the user might hit are enforced by the **skill itself** before the CLI runs. They do not produce a CLI bail!.

| Skill-side guard | Trigger | What the skill does |
|---|---|---|
| "At least one field must change on update" | User submitted nothing / every field unchanged | Refuse to call `onchainos agent update`; render "No changes to submit." and re-enter update Q&A. The CLI (`mutations.rs:156-228`) does NOT validate this. See `core/cli-reference.md` §2. |
| "Query must be non-empty" | `agent search` with empty query | The CLI will bail with `missing required parameter: --query` (§1 above); the skill should catch it first and ask. |
| Stars outside 0.00–5.00 or over 2 decimal places | `feedback-submit` invoked with the user's intended star count outside `0.00..=5.00` or with more than 2 decimal places (e.g. `3.333`, `6`, `-1`, `abc`) | Reject with "Rating must be 0.00–5.00 with at most 2 decimal places (e.g. `5`, `4.5`, `3.33`)". Skill validates before sending and never invokes the CLI in this case. The backend's `score out of range` (§2 above) is the secondary safety net for the 0–100 wire format only. |
| `fee` on a servicetype=A2A entry not matching the internal validation pattern (**internal pattern, never echoed to user**: `^\d+(\.\d{1,6})?$`) — **wire-level enum `A2A` ↔ user-visible long form `agent-to-agent service` per `core/ux-lexicon.md §Service-type` Pattern A; do NOT leak `A2A` to the user** | User answered Q4 on an agent-to-agent service with something other than empty / number-with-≤6-decimals (e.g. `5 USDT`, `approx 10`, `-1`) | Reject with "agent-to-agent service (negotiated / off-chain pricing) fee is optional — leave it empty or supply a USDT number with up to 6 decimal places" — Pattern A (long form inline gloss) per `core/ux-lexicon.md §Service-type`. Re-ask Q4 (A2A branch — internal label). The CLI (`utils.rs::normalize_service` A2A arm) does NOT validate the fee format on A2A — this is skill-side only. |
| `endpoint` on a servicetype=A2MCP entry exceeds the skill-side length limit (> 512 chars) — **the 512 limit is hidden from the Q5 prompt; mention it only here, after the user's input failed**; wire-level `A2MCP` ↔ user-visible `API service` | User Q5 reply on an API service endpoint is longer than 512 chars | Reject with "The endpoint URL must be at most 512 chars; this one exceeds it. Please use a shorter URL." Re-ask Q5 (A2MCP branch — internal label). The CLI (`utils.rs::normalize_service`) does NOT validate endpoint length — this is skill-side only. |

---

## General handling principles

1. **Translate, don't parrot.** Always show the user the friendly translated version; the raw message goes into the footer of the error card (`core/display-formats.md` §7) for debuggability.
2. **Recover, don't abort.** For every row above, there is a concrete "resume at which step" action. Keep the user in the flow.
3. **Do not retry silently** for business errors (4xx-class). Render the error card and stop — the user decides the next step. See .
4. **Retry once** for transient 5xx/network errors. If it fails a second time, surface the error and move on. Never loop.
5. **Do not chase failures with a `get`.** If `activate` fails, do NOT run `agent get` to "see what happened" — the error message is authoritative. Render the card and wait.
6. **Update this file** the moment `cli/src/commands/agent_commerce/identity/**` changes a `bail!` string, or the moment you observe a backend message whose keywords don't match any row here — otherwise translations will silently rot.
