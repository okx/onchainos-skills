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
| `session expired, please login again: onchainos wallet login` | `signing.rs:66/68/74/139/141` (shared: `agentic_wallet/auth.rs:44/76/285`) | "登录态过期了" | Hand off to `okx-agentic-wallet` → `wallet login`, then retry the original command. |
| `no XLayer address found in current account` | `signing.rs:33/42` | "当前账号没有 XLayer 地址" | Hand off to `okx-agentic-wallet` → `wallet add` / `wallet switch`. |
| `missing required parameter: <flag>` | `utils.rs:238` | "参数 `<flag>` 不能留空" | Re-ask that specific field. For `--agent-id`, ask the user which agent; run `agent get` if needed. For `--file`, ask for the file path. |
| `error: unexpected argument '<value>' found` (positional rejected by clap) | clap default | "这个命令需要显式带参数名，不接受裸值" | The user passed something like `agent update 42`; tell them to use `agent update --agent-id 42`. Same for `activate` / `deactivate` / `service-list` / `feedback-list` (`--agent-id`) and `upload` (`--file`). |
| `missing required field in --service: name` | `utils.rs:200` | "服务名不能留空" | Return to `role-provider.md` Phase 2 per-service Q1 (`name`). |
| `missing required field in --service: servicedescription` | `utils.rs:203` | "服务描述不能留空" | Return to `role-provider.md` Phase 2 per-service Q2 (`servicedescription`). |
| `missing required field in --service for A2MCP: fee` | `utils.rs:212` | "A2MCP 服务必须给 fee（USDT 数字，最多六位小数）" | Return to `role-provider.md` Phase 2 per-service Q4 (A2MCP branch). |
| `missing required field in --service for A2MCP: endpoint` | `utils.rs:215` | "A2MCP 服务必须给 endpoint（HTTPS URL）" | Return to `role-provider.md` Phase 2 per-service Q5 (A2MCP branch). |
| `invalid servicetype in --service: <value>` | `utils.rs:218` | "服务类型必须是 A2MCP 或 A2A" | Return to `role-provider.md` Phase 2 per-service Q3 (numbered prompt). |
| `invalid value for --role: <value>` | `utils.rs:229` | "role 只能是 requester / provider / evaluator 之一" | Return to role selection (SKILL.md §Core Flow). |
| `invalid value for <flag>: expected integer` | `utils.rs:267` | "`<flag>` 要填整数" | Re-ask that field. |
| `invalid value for <flag>: must be >= <min>` | `utils.rs:270` | "`<flag>` 最小值是 `<min>`" | Re-ask that field. |
| `invalid value for <flag>: must be <= <max>` | `utils.rs:278` | "`<flag>` 最大值是 `<max>`" | Re-ask that field. |
| `provider agents require at least one service; provide --service` | `utils.rs:248` | "provider 必须有至少一个 service" | Return to role-playbook `provider` service Q&A loop. |
| `invalid value for --sort-by: <value>` | `queries.rs:234` | "排序值只能是 `time_desc` 或 `score_desc`" | Re-map via `cli-reference.md` §10 natural-language table. |
| `failed to read file: <path>` | `mutations.rs:286` (`fs::read` context) | "读不到这个文件" | Ask the user to recheck the path; in terminal mode switch to AI-gen / skip (see `avatar-upload.md`). |
| `upload response missing url` | `mutations.rs:334/337` | "上传成功但后端没返回 URL" | Retry once; if persists, surface and ask. |
| `xmtp-sign response missing signature` | `mutations.rs:489` | (not user-facing — `xmtp-sign` is not exposed by this skill) | Log; do not route here. |

---

## 2. Backend-originated (CLI passes through)

> ⚠️ Wording may drift without a CLI code change — match on keywords, not equality. None of these correspond to a CLI `bail!` in `identity/*.rs`. If the backend returns a string you don't recognize, show it verbatim in the error card footer and ask the user.

| Typical backend string (keyword match) | User-facing translation | Skill action |
|---|---|---|
| `user is not in approved agent whitelist` / `not in approved agent whitelist` / `approved agent whitelist` / backend code `10016` | 中文："当前账户还没有获取 Agent 公测资格。申请链接：`<URL，从后端 msg 字段里原样抓出>`。审核通过后我们会通过邮箱通知你，再回来注册 Agent。" / English: "Your account is not in the agent beta whitelist yet. Apply here: `<URL extracted verbatim from the backend msg field>`. We'll email you when you're approved; come back to register the agent then." | 渲染 error card（`display-formats.md §7`）。**URL 提取**：用正则 `https?://\S+?(?=[\s)）"'.,;]|$)` 从后端 `msg` 抓第一个 URL（lookahead 把句号 / 逗号 / 分号 这些常见尾随标点也作为终止符，避免抓到 `https://x.com/y.` 之类带标点的脏值），**原样渲染**（不做语言路径替换，不去掉 `/zh-hans/` 等子路径；即便用户用英文交互，URL 也保持后端给的那一份）。**Never auto-retry** —— 用户必须先去申请、收到通过邮件后再回来；本 skill 不要再发任何 `agent create` / `agent update` 调用。如果 `msg` 里没有可识别的 URL（罕见），把整段 `msg` 原样放在错误卡 footer 的 `raw:` 一行，正文用上面的中/英文模板但把 `申请链接：…` / `Apply here: …` 那一句改成"申请入口请联系 OKX support / Contact OKX support for the application portal."。 |
| `agent not found` / any 404-shaped response | "找不到该 agent" | Verify the id with `agent get`; maybe the user misread. |
| `agent already active` | "这个 agent 已经在上架状态，不用再上架。" / "Agent is already active." | No-op; show detail card. |
| `agent already inactive` | "这个 agent 已经在下架状态。" / "Agent is already inactive." | No-op; show detail card. |
| `pending settlements` / `cannot deactivate` | "这个 agent 上还有任务没结清，得先把那边的事处理完才能下架 — 我帮你切过去看看？" / "There's still an unsettled task on this agent; we need to close that out first before deactivating — want me to take you there?" | If user agrees, hand off to the task marketplace flow internally (do not name the skill in user text — Red line 1). |
| `stake` / `staking` / `insufficient` / `质押` (**not expected** on `agent create --role evaluator` — `create` doesn't consume the stake; if it ever appears it's a backend anomaly) | "后端返回了和质押相关的报错。这不是正常的 create 失败路径 —— agent 注册本身不需要质押。" | Surface the raw message verbatim in the error card footer; point the user at `/skills/okx-agent-task/references/evaluator-staking.md` for the staking flow; do NOT cache drafts or invent a resume keyword. |
| `score out of range` | "评分要在 0–5 星之间的整数" (skill speaks stars; do not echo the raw 0–100 bound from the backend message — see `feedback-guide.md` Step 3) | Return to `feedback-guide.md` step 3. |
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
| Stars outside 0-5 | `feedback-submit` invoked with the user's intended star count outside `0..=5` (or non-integer, decimal, etc.) | Reject with "评分要在 0–5 星之间的整数（例如 `5 星` / `4 星` / `1 星`）" / "Rating must be an integer 0–5 stars". Skill validates before sending and never invokes the CLI in this case. The backend's `score out of range` (§2 above) is the secondary safety net for the 0–100 wire format only. |
| A2A `fee` not matching the internal validation pattern (**internal pattern, never echoed to user**: `^\d+(\.\d{1,6})?$`) | User answered Q4 on an A2A service with something other than empty / number-with-≤6-decimals (e.g. `5 USDT`, `约 10`, `-1`) | Reject with "A2A 价格选填，要么留空，要么填 USDT 数字最多六位小数（例如 `1.234567` / `10` / `0.5`）" / "A2A fee is optional — leave it empty or supply a USDT number with up to 6 decimal places". Re-ask Q4 (A2A branch). The CLI (`utils.rs::normalize_service` A2A arm) does NOT validate the fee format on A2A — this is skill-side only. |
| A2MCP `endpoint` exceeds the skill-side length limit (> 512 chars) — **the 512 limit is hidden from the Q5 prompt; mention it only here, after the user's input failed** | User Q5 reply on an A2MCP service is longer than 512 chars | Reject with "endpoint 最长 512 字符，这个超了，麻烦换个短点的 URL。" / "The endpoint URL must be at most 512 chars; this one exceeds it. Please use a shorter URL." Re-ask Q5 (A2MCP branch). The CLI (`utils.rs::normalize_service`) does NOT validate endpoint length — this is skill-side only. |

---

## General handling principles

1. **Translate, don't parrot.** Always show the user the 中文 friendly version; the raw message goes into the footer of the error card (`display-formats.md` §7) for debuggability.
2. **Recover, don't abort.** For every row above, there is a concrete "回到哪一步" action. Keep the user in the flow.
3. **Do not retry silently** for business errors (4xx-class). Render the error card and stop — the user decides the next step. See `_shared/no-polling.md`.
4. **Retry once** for transient 5xx/network errors. If it fails a second time, surface the error and move on. Never loop.
5. **Do not chase failures with a `get`.** If `activate` fails, do NOT run `agent get` to "see what happened" — the error message is authoritative. Render the card and wait.
6. **Update this file** the moment `cli/src/commands/agent_commerce/identity/**` changes a `bail!` string, or the moment you observe a backend message whose keywords don't match any row here — otherwise translations will silently rot.
