# Troubleshooting — Errors → user-friendly translation

Two classes of errors to translate:

1. **CLI-emitted `bail!`** — raw strings emitted from `cli/src/commands/agent_commerce/identity/*.rs` (and a couple from shared modules). These are stable: if the CLI source changes, this table must change in the same commit. Each row cites the source file + line.
2. **Backend-originated errors** — surfaced by `format_api_error` / the API client. The exact wording can drift without a CLI code change; match on keywords, not equality. Treat the "CLI error" column here as best-effort.

If you encounter a string that isn't in either table, surface the raw message in the error card footer and ask the user how to proceed — do NOT auto-retry or auto-translate.

---

## 1. CLI-emitted `bail!` (verified)

| CLI error (exact) | Source | User-facing translation | Skill action |
|---|---|---|---|
| `session expired, please login again: onchainos wallet login` | `signing.rs:66/68/74/139/141` (shared: `agentic_wallet/auth.rs:44/76/285`) | "登录态过期了" | Hand off to `okx-agentic-wallet` → `wallet login`, then retry the original command. |
| `no XLayer address found in current account` | `signing.rs:33/42` | "当前账号没有 XLayer 地址" | Hand off to `okx-agentic-wallet` → `wallet add` / `wallet switch`. |
| `missing required parameter: agentId` | `utils.rs:184` | "这个命令必须带 agent id" | Ask the user which agent; run `agent get` if needed. |
| `missing required parameter: <flag>` | `utils.rs:190` | "参数 `<flag>` 不能留空" | Re-ask that specific field. |
| `missing required field in --service: name` | `utils.rs:136` | "服务名不能留空" | Return to `role-provider.md` Phase 2 per-service Q1 (`name`). |
| `missing required field in --service: servicedescription` | `utils.rs:139` | "服务描述不能留空" | Return to `role-provider.md` Phase 2 per-service Q2 (`servicedescription`). |
| `missing required field in --service for A2MCP: fee` | `utils.rs:148` | "A2MCP 服务必须给 fee（USDT 整数）" | Return to `role-provider.md` Phase 2 per-service Q4 (A2MCP branch). |
| `missing required field in --service for A2MCP: endpoint` | `utils.rs:151` | "A2MCP 服务必须给 endpoint（HTTPS URL）" | Return to `role-provider.md` Phase 2 per-service Q5 (A2MCP branch). |
| `invalid servicetype in --service: <value>` | `utils.rs:154` | "服务类型必须是 A2MCP 或 A2A" | Return to `role-provider.md` Phase 2 per-service Q3 (numbered prompt). |
| `invalid value for --role: <value>` | `utils.rs:165` | "role 只能是 requester / provider / evaluator 之一" | Return to role selection (SKILL.md §Core Flow). |
| `invalid value for <flag>: expected integer` | `utils.rs:219` | "`<flag>` 要填整数" | Re-ask that field. |
| `invalid value for <flag>: must be >= <min>` | `utils.rs:222` | "`<flag>` 最小值是 `<min>`" | Re-ask that field. |
| `invalid value for <flag>: must be <= <max>` | `utils.rs:230` | "`<flag>` 最大值是 `<max>`" | Re-ask that field. |
| `provider agents require at least one service; provide --service` | `utils.rs:200` | "provider 必须有至少一个 service" | Return to role-playbook `provider` service Q&A loop. |
| `invalid value for --sort-by: <value>` | `queries.rs:234` | "排序值只能是 `time_desc` 或 `score_desc`" | Re-map via `cli-reference.md` §10 natural-language table. |
| `failed to read file: <path>` | `mutations.rs:286` (`fs::read` context) | "读不到这个文件" | Ask the user to recheck the path; in terminal mode switch to AI-gen / skip (see `avatar-upload.md`). |
| `upload response missing url` | `mutations.rs:334/337` | "上传成功但后端没返回 URL" | Retry once; if persists, surface and ask. |
| `xmtp-sign response missing signature` | `mutations.rs:489` | (not user-facing — `xmtp-sign` is not exposed by this skill) | Log; do not route here. |

---

## 2. Backend-originated (CLI passes through)

> ⚠️ Wording may drift without a CLI code change — match on keywords, not equality. None of these correspond to a CLI `bail!` in `identity/*.rs`. If the backend returns a string you don't recognize, show it verbatim in the error card footer and ask the user.

| Typical backend string (keyword match) | User-facing translation | Skill action |
|---|---|---|
| `agent not found` / any 404-shaped response | "找不到该 agent" | Verify the id with `agent get`; maybe the user misread. |
| `agent already active` | "Agent 已经是 active 状态，无需再次 activate" | No-op; show detail card. |
| `agent already inactive` | "Agent 已经是 inactive 状态" | No-op; show detail card. |
| `pending settlements` / `cannot deactivate` | "有未完结的任务引用这个 agent，需要先去 `okx-agent-task` 处理完" | Hand off to `okx-agent-task`. |
| `stake` / `staking` / `insufficient` / `质押` (**not expected** on `agent create --role evaluator` — `create` doesn't consume the stake; if it ever appears it's a backend anomaly) | "后端返回了和质押相关的报错。这不是正常的 create 失败路径 —— agent 注册本身不需要质押。" | Surface the raw message verbatim in the error card footer; point the user at `/skills/okx-agent-task/evaluator.md` for the staking flow; do NOT cache drafts or invent a resume keyword. |
| `score out of range` | "分数要在 0-100 之间的整数" | Return to `feedback-guide.md` step 3. |
| `self-rating not allowed` | "不能给自己的 agent 打分" | Return to `feedback-guide.md` step 1 (target). |
| `creator agent not owned by caller` | "`--creator-id` 必须是你自己的 agent id" | Return to `feedback-guide.md` step 2 (re-resolve). |
| `Wallet API server error (HTTP 500)` | "后端暂时不可用" | Retry once (network-transient policy, §General principles). If persists, surface and move on. |
| Region-restriction codes `50125` / `80001` | "Service is not available in your region." | Do NOT echo the raw code. Do NOT suggest VPNs. |
| TEMP MOCK empty `txHash` on pre-transaction | "交易还没正式上链（走了临时 mock 路径），请稍后复查状态" | Log event; once the CLI mock path is removed, delete this row. |

---

## 3. Not errors — actions that never reach the CLI

Some conditions the user might hit are enforced by the **skill itself** before the CLI runs. They do not produce a CLI bail!.

| Skill-side guard | Trigger | What the skill does |
|---|---|---|
| "At least one field must change on update" | User submitted nothing / every field unchanged | Refuse to call `onchainos agent update`; render `没有需要提交的更改` and re-enter update Q&A. The CLI (`mutations.rs:156-228`) does NOT validate this. See `cli-reference.md` §2. |
| "Query must be non-empty" | `agent search` with empty query | The CLI will bail with `missing required parameter: --query` (§1 above); the skill should catch it first and ask. |
| Score outside 0-100 | `feedback-submit` with bad score | Skill validates before sending (see `feedback-guide.md` step 3). The backend also rejects (§2 above) as a safety net. |

---

## General handling principles

1. **Translate, don't parrot.** Always show the user the 中文 friendly version; the raw message goes into the footer of the error card (`display-formats.md` §7) for debuggability.
2. **Recover, don't abort.** For every row above, there is a concrete "回到哪一步" action. Keep the user in the flow.
3. **Do not retry silently** for business errors (4xx-class). Render the error card and stop — the user decides the next step. See `_shared/no-polling.md`.
4. **Retry once** for transient 5xx/network errors. If it fails a second time, surface the error and move on. Never loop.
5. **Do not chase failures with a `get`.** If `activate` fails, do NOT run `agent get` to "see what happened" — the error message is authoritative. Render the card and wait.
6. **Update this file** the moment `cli/src/commands/agent_commerce/identity/**` changes a `bail!` string, or the moment you observe a backend message whose keywords don't match any row here — otherwise translations will silently rot.
