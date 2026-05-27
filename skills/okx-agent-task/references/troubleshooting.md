# Troubleshooting

> **When to open this document**: on CLI errors / when the agent sees an unexpected return, look up by error code / error message.

> **Retry strategy** — first read [`_shared/exception-escalation.md`](../_shared/exception-escalation.md): business errors = 0 retries → push to user session; the only two exceptions that auto-retry once are transient network blips and JWT expiry; on a cumulative 3 failures, stop immediately.

---

## 0. Backend unified error codes (class-level)

The backend task / dispute / evaluator APIs all follow this 5-class error-code scheme. **`code` is only the class**; the **specific error** is distinguished by the returned `msg` field (the same `1001` can be a missing required field, an out-of-range value, a bad format, etc.).

| code | Class | Meaning | Retry? |
|---|---|---|---|
| **0** | Success | API returned normally | — |
| **1001** | Parameter validation failed | Required field missing / wrong type / out of range / business pre-check failed | ❌ Do not retry; push to user session |
| **2001** | Risk-control sensitive content | Text content triggered risk-control filters | ❌ Do not retry; push to user session so the user can rewrite the text |
| **3001** | Permission problem | JWT invalid / not logged in / empty `agenticId` header / wallet session expired | ⚠️ **JWT expired** (msg contains `JWT verification failed` / `JWT expired` / `unauthorized`) — allowed to **auto-retry once** after refreshing the login state; other 3001 (empty `agenticId` header / wallet not logged in) push to user session |
| **4001** | Internal server error | Backend panic / DB error / external dependency down | ❌ Do not retry; push to user session |
| **5001** | Retryable code | Backend explicitly indicates the client may retry (which scenarios exactly is to be confirmed with backend) | ❌ Do not retry; push to user session (even if the backend says retryable, on the agent side we let the user make the call) |

**Iron rules for agent handling** (stacking on top of SKILL.md Layer 1.5 + [`_shared/exception-escalation.md`](../_shared/exception-escalation.md)):

- On `code != 0` → **push to user session on the first failure**, **do not retry the same command**
- The one general exception is JWT expiry (3001 + specific msg) → refresh + auto-retry once; on failure again, push to user
- Network timeout / connection error is NOT an exception — treat as business error and push to user, **do not blind-retry inside the sub**
- **Role-specific exception**: `vote-commit` / `vote-reveal` / `arbitration-claim` get a 0.3% stake slash for missing the window, so the sub may internally retry up to 3 times (see `references/evaluator-decision-rubric.md` §6). Other evaluator commands still follow the 0-retry rule. Buyer / provider have no such exception.

> ⚠️ Sub-codes like `2004` / `4000`: some error messages in the tables below mention `2004 / 4000` — those are **business sub-codes embedded in `msg`** (e.g. staking module's own sub-error), NOT class-level codes. Class-level codes always come from the 5 values in the table above.

---

## 1. Authentication / identity errors

| Error code / message | Trigger | Handling |
|---|---|---|
| `code=3001` + `msg` containing `auth fail` / `unauthorized` / `agenticId` | Beta backend rejects an empty `agenticId` header; virtually every task API command will hit this if `--agent-id` is missing | Check the top-level `agentId` of the envelope and pass it **verbatim** to `--agent-id` (the CLI already enforces it as required, so this should bail earlier) |
| `code=3001` + `msg` containing `JWT verification failed` / `JWT expired` | JWT expired | The only auth error allowed to auto-retry once (refresh the login state before retrying); on failure → push to user session to log back in via `okx-agentic-wallet` |
| `msg` containing `agentId 无效` / `session 丢失` (business sub-code 4000) | Wallet session expired / agentId does not belong to the current wallet | Push to user session to re-login the wallet; after re-login retry once |
| `msg` containing `agentId 没有 evaluator 身份` (business sub-code 2004) | Calling `stake` etc. with a buyer / provider agentId | Go back to the identity skill (`okx-agent-identity`), register the evaluator role, then come back |
| `bail: --agent-id 必填...` (CLI-layer bail, never reaches backend) | CLI layer detected an empty agentId and bailed directly | Fetch the agentId from envelope / context and call again; if the envelope lacks an agentId, abort this turn — **do not default to empty** |

## 2. Task query / status precondition errors

| Error code / message | Trigger | Handling |
|---|---|---|
| `code=1001` + `msg` containing `task not found` / `jobId not exists` | jobId does not exist / mistyped / cleaned up | Run `agent tasks` to let the user pick; envelope-triggered cases cannot be mistyped — in that case push to user session reporting "task X not found" |
| `code=1001` + `msg` containing `invalid status transition` | Current status does not permit this action (e.g. `complete` while status=disputed) | Run `agent status <jobId>` to fetch the real status; have the user resolve the dispute first, etc. |
| `code=2001` + `msg` containing `sensitive` / `风控` | Text content triggered the risk-control filter | **Do not retry**; push to user to rewrite the text (task description / refusal reason / dispute reason / dispute upload text and other user-input fields all go through risk control) |
| `bail: deliver bails immediately when status != accepted` (CLI-layer bail) | Provider called deliver right after `apply` (status is still created — must wait for `job_accepted`) | Do not retry; wait for the `job_accepted` chain event to arrive, then deliver. See provider.md §5.1 |
| `dispute window closed` / `review window closed` (business sub-code) | The 24h decision / 1h evidence-prep window has elapsed | No remedy; follow the automatic flow for the current status (`claim-auto-refund` / `claim-auto-complete`, etc.) |

## 3. Payment / balance errors

> ⚠️ **The task system is fully gas-free** — every on-chain action goes through the platform paymaster. The "balance" errors in this section refer **only to business tokens** used to pay the task itself; they are **never** about native / OKB / gas. **Do not** prompt the user to top up native / OKB / gas, and **do not** attribute any on-chain failure to "insufficient gas".

| Error code / message | Trigger | Handling |
|---|---|---|
| `Insufficient balance: current XLayer USDT balance is X, need Y USDT. Please top up before proceeding` | `create-task` / `confirm-accept` etc. — the CLI auto-runs `wallet balance` for a self-check before broadcasting | Push the user to top up USDT/USDG via `okx-dex-swap`; do not retry the same CLI |
| `unsupported currency` | The user's quote is not USDT / USDG | The task system **only** supports these two tokens; ask the user to change the quote |
| `endpoint missing` (x402 `confirm-accept`) | The x402 path requires the service endpoint URL | The CLI has a 3-level fallback (CLI > recommend cache > service-list API); if all fail → push the user to specify it manually |

## 4. Dispute errors (shared by both parties)

| Error code / message | Trigger | Handling |
|---|---|---|
| `code=1001` + `msg` containing `text or images required` | `dispute upload` was called without `--text` or `--image` (parameter validation failed) | At least one of text / image is required; text length cap is 16 KB (CLI has a pre-check); single image cap is 20 MB (CLI has a pre-check) |
| `Unsupported image format` | `dispute upload --image` extension is not in `jpg/jpeg/png/gif/webp` | Convert format and re-upload |
| No `dispute_approved` received after Phase 1 completes | Chain event delay | **Do not** preemptively run `dispute confirm`; wait for the notification to arrive before calling (anti-hallucination rule in provider.md) |
| `evidence-info` / `vote-commit` / `vote-reveal` can't find jobId / backend 1001 | Task status is not disputed / there is no active arbitration round | Run `agent status <jobId>` to confirm status=disputed; the CLI argument is `jobId` (**no longer needs `disputeId`**) — the backend automatically locates the current active round |

## 5. Evaluator voting / reward-claim errors

| Error code / message | Actual meaning | Handling |
|---|---|---|
| `voter has already committed` | You have already committed in this round | **Treat as success** — duplicate triggers from the agent are a common race; the outcome is consistent |
| `voter has not committed` | Received `reveal_started` but did not commit in this round | Skipping reveal is normal (you may not have been selected / commit timed out and was kicked); **do not** treat as an error |
| `canReveal=false` | CLI auto-precheck: commit window not yet closed / already revealed / already settled | **Do not retry**; wait for the `dispute_resolved` notification; if already settled → switch to `arbitration-claim` (account-level pull) |
| Commit / Reveal timeout slash (`slashTimeoutBps`) | Missed the submission deadline | Accept the slash; serve the `slashedCooldownHours` cooldown during which you will not be selected; resume normally after cooldown ends. **Read ratio / duration from `staking-config` — never hard-code** |
| `code=1001` | Stake amount insufficient | |
| `request-unstake` contract revert | Currently has `activeDisputes > 0`; unstaking is forbidden during an active arbitration | Have the user wait until the arbitration settles (`dispute_resolved`) before unstaking |

## 6. XMTP / tool errors

| Error code / message | Trigger | Handling |
|---|---|---|
| `forbidden` (any XMTP tool) | A tool blocked by `tools.sessions.visibility=tree` was called (e.g. `Session Send` / `sessions.send`) | Switch to one of the 10 whitelisted XMTP tools (see SKILL.md `Session Communication Contract §4`); **do not** fall back to other tools |
| `xmtp_dispatch_user` / `xmtp_prompt_user` `timeout` | XMTP infra jitter | Push to user: "Dispatch failed, please retry" — **do not** switch to `Session Send` (it will be rejected) |
| `xmtp_send` was not preceded by `session_status` | Missing `sessionKey` parameter | Strict two-step: `session_status` → get `sessionKey` → `xmtp_send`; do not re-call `session_status` within the same turn |
| `xmtp_file_upload` file path does not exist | `--file` points to a file that does not exist on the user's machine | Have the user confirm the file path; do not guess a substitute |
| `xmtp_file_download` `localPath` does not exist | The CLI tried 3 times and failed; `info` returns with a `downloadError` field | **Do not** use `ls` / `find` to search for a substitute file (violates Layer 0 security gate); vote per "insufficient evidence" (decision principle #5) |
| `[USER_DECISION_RELAY]` prefix detection failed | The user agent wrote "用户决策" instead of "用户决定", or used ASCII `:` instead of Chinese `：` | Strictly follow the 22-char prefix `[USER_DECISION_RELAY] decision: <user's verbatim words>` (with Chinese colon) |

## 7. Region restriction

Error codes `50125` / `80001` — **do not** echo the raw error code to the user. Show a unified message:

> "Service is not available in your region. Please switch to a supported region and try again."

Do not retry.

## 8. Easily misread: looks like an error, actually normal

| Symptom | Actual state | Handling |
|---|---|---|
| Status is still `created` after `apply` is on-chain | apply is a transient event — it **does not** change the status | Wait for the buyer's `confirm-accept` to fire `job_accepted` — only then does it enter `accepted` |
| Did not receive `reveal_started` after `vote-commit` | The reveal phase only starts after the commit window closes (commit + reveal total 24h) | Silently wait — do not retry commit |
| Received `provider_applied` but the User Agent did not | Backend rule: the `provider_applied` system notification is **only sent to the ASP** | The User Agent learns via inbound a2a-agent-chat (the "I've applied" message from the ASP) and immediately calls `confirm-accept` (see SKILL.md `Session Communication Contract §6 Anti-hallucination rules` User-Agent exception) |
| Status is still `rejected` after `dispute_approved` | dispute approve is a transient event (arbitration phase 1; not truly disputed yet) | Wait for phase 2 `dispute confirm` + `job_disputed` notification |

## 9. Diagnostics collection

When the issue cannot be resolved, ask the user via the user session for:

```
- Command + full flags
- jobId
- Error message (full text, including error code)
- onchainos --version
- Current task status: onchainos agent status <jobId> --agent-id <id>
- Wallet address (public portion only — do not leak private key / mnemonic)
- Timestamp when the issue occurred
```

After collecting, call `xmtp_dispatch_user` to push to the user; **do not** write to chat logs or attempt a fix yourself.
