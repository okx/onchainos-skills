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
| `missing required field in --service for A2MCP: fee` | `utils.rs:212` | "API 接口式服务（按次调用、固定价格）必须给价格（USDT 数字，最多六位小数）" / "API-interface service (pay-per-call, fixed price) requires a fee (USDT numeric, ≤ 6 decimal places)" — Pattern A (long form inline gloss) per `references/ux-lexicon.md §Service-type`, since error messages are teaching contexts | Return to `role-provider.md` Phase 2 per-service Q4 (A2MCP branch — internal label). |
| `missing required field in --service for A2MCP: endpoint` | `utils.rs:215` | "API 接口式服务（按次调用、固定价格）必须给接口地址（HTTPS URL）" / "API-interface service (pay-per-call, fixed price) requires an endpoint (HTTPS URL)" — Pattern A per `ux-lexicon.md §Service-type` | Return to `role-provider.md` Phase 2 per-service Q5 (A2MCP branch — internal label). |
| `invalid servicetype in --service: <value>` | `utils.rs:218` | "服务类型只能是 API 接口式服务（按次调用、固定价格）或 agent（智能体）通信式服务（议价 / 灵活协作）这两种" / "Service type must be one of: API-interface service (pay-per-call, fixed price) or agent-to-agent service (negotiated / off-chain pricing)" — Pattern A (long form inline gloss) per `references/ux-lexicon.md §Service-type`; no raw `A2MCP` / `A2A` to the user | Return to `role-provider.md` Phase 2 per-service Q3 (numbered prompt). |
| `invalid value for --role: <value>` | `utils.rs:229` | CN: "角色只能选 用户 / 服务提供商 / 仲裁者 其中之一" / EN: "Role must be one of: User Agent / Agent Service Provider (ASP) / Evaluator Agent" — never render the raw ERC-8004 enum (`requester` / `provider` / `evaluator`) to the user; the wire mapping happens skill-side | Return to role selection (SKILL.md §Core Flow). |
| `invalid value for <flag>: expected integer` | `utils.rs:267` | "`<flag>` 要填整数" | Re-ask that field. |
| `invalid value for <flag>: must be >= <min>` | `utils.rs:270` | "`<flag>` 最小值是 `<min>`" | Re-ask that field. |
| `invalid value for <flag>: must be <= <max>` | `utils.rs:278` | "`<flag>` 最大值是 `<max>`" | Re-ask that field. |
| `provider agents require at least one service; provide --service` | `utils.rs:248` | "服务提供商身份至少要配一个服务" / "An Agent Service Provider (ASP) needs at least one service" — no raw `provider` literal in user text (Red line 4 + `ux-lexicon.md §Role` localizes both languages) | Return to role-playbook `provider` service Q&A loop. |
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
| `score out of range` | "评分要在 0.00–5.00 之间，最多保留 2 位小数" (skill speaks stars; do not echo the raw 0–100 bound from the backend message — see `feedback-guide.md` Step 3) | Return to `feedback-guide.md` step 3. |
| `self-rating not allowed` | "不能给自己的 agent 打分" | Return to `feedback-guide.md` step 1 (target). |
| `creator agent not owned by caller` | "评价发起人必须是你当前钱包名下的 agent — 我重新按当前钱包帮你确认可用发起人。" / "The reviewer must be an agent owned by your current wallet — let me re-check which of your agents under the current wallet can act as reviewer." (no `--creator-id` flag in user text — Red line 2; use `ux-lexicon.md §Field` mapping `creator-id` → 发起人 / reviewer; deliberately neutral — does NOT promise "pick one" or "selection step" because ladder 2's next move depends on the count) | Return to `feedback-guide.md §Step 2` and re-run ladder 2 from the top — the next user-visible message is whichever of the **3 branches** ladder 2 lands in: **0 agents** under current wallet → STOP and offer registration (do NOT promise to auto-pick); **1 agent** → silently use it and mention the choice in the next confirmation; **multiple agents** → ask the user with the numbered-options prompt and wait — `Do not auto-pick`. ⛔ The error-line wording above MUST stay neutral ("确认可用发起人" / "re-check which … can act as reviewer") — do NOT commit to "pick one and retry" or "redo the selection step", because for the 0-agent branch there is nothing to pick and for the multi-agent branch the user has to choose. |
| `Wallet API server error (HTTP 500)` | "后端暂时不可用" | Retry once (network-transient policy, §General principles). If persists, surface and move on. |
| Region-restriction codes `50125` / `80001` | "Service is not available in your region." | Do NOT echo the raw code. Do NOT suggest VPNs. |
| TEMP MOCK empty `txHash` on pre-transaction | "交易还没正式上链（走了临时 mock 路径），请稍后复查状态" | Log event; once the CLI mock path is removed, delete this row. |
| `agent activate` returns `success: false, approvalStatus: 2` — already under review | 中文："你的 agent 正在审核中，一般 24 小时内出结果，审核通过后你的 agent 就会在市场上出现了。" / English: "Your agent is currently under review — results are usually ready within 24 hours; once approved, your agent will appear on the marketplace." | **Stop.** Do NOT call `submit-approval` — a review is already in flight. No `§Step 5` / `§Step 6`. |
| `agent activate` returns `success: false, approvalStatus: 5` — review rejected | 中文："上架审核未通过。" + (当 `rejectReason` 非空时) "原因：`<rejectReason>`。" + 建议："你可以根据反馈修改 agent 的名称或服务信息，改好后跟我说"上架 #<id>"，我重新帮你提交。" / English: "Listing review failed." + (when `rejectReason` non-empty) "Reason: `<rejectReason>`." + "You can update the agent's name or services based on the feedback, then say "activate #<id>" and I'll resubmit." | Render the rejection card with `rejectReason` (omit the reason line if `rejectReason` is empty or null). **Stop.** Do NOT auto-retry; do NOT call `submit-approval` again. No `§Step 5` / `§Step 6`. |
| `agent activate` / `agent submit-approval` returns top-level `code: "81602"` (keyword: `81602` or `blocked`) — agent blacklisted | 中文："这个 agent 已被平台封禁，当前无法操作。" / English: "This agent has been blocked by the platform and cannot be operated at this time." | **Stop.** Do NOT suggest re-activating or updating. No `§Step 5` / `§Step 6`. |
| `agent submit-approval` returns `success: true` — submission accepted, review now pending | 中文："好的，已帮你提交上架审核，一般 24 小时内出结果。审核通过后你的 agent 就会在市场上出现了。" / English: "Done — your agent has been submitted for listing review. Results are usually ready within 24 hours; once approved, your agent will appear on the marketplace." | **Stop.** No `§Step 5` / `§Step 6`. |
| `agent submit-approval` returns `success: false` with a non-blacklist error | 中文："上架审核申请提交失败。" + (将 `msg` 原文放在错误卡 footer 的 `raw:` 行) + "你可以稍后再试。" / English: "Failed to submit for listing review." + (show `msg` verbatim in error card footer) + "You can try again later." | Render error card. **Stop.** |

---

## 3. Not errors — actions that never reach the CLI

Some conditions the user might hit are enforced by the **skill itself** before the CLI runs. They do not produce a CLI bail!.

| Skill-side guard | Trigger | What the skill does |
|---|---|---|
| "At least one field must change on update" | User submitted nothing / every field unchanged | Refuse to call `onchainos agent update`; render `没有需要提交的更改` and re-enter update Q&A. The CLI (`mutations.rs:156-228`) does NOT validate this. See `cli-reference.md` §2. |
| "Query must be non-empty" | `agent search` with empty query | The CLI will bail with `missing required parameter: --query` (§1 above); the skill should catch it first and ask. |
| Stars outside 0.00–5.00 or over 2 decimal places | `feedback-submit` invoked with the user's intended star count outside `0.00..=5.00` or with more than 2 decimal places (e.g. `3.333`, `6`, `-1`, `abc`) | Reject with "评分要在 0.00–5.00 之间，最多 2 位小数（例如 `5 星` / `4.5 星` / `3.33 星`）" / "Rating must be 0.00–5.00 with at most 2 decimal places (e.g. `5`, `4.5`, `3.33`)". Skill validates before sending and never invokes the CLI in this case. The backend's `score out of range` (§2 above) is the secondary safety net for the 0–100 wire format only. |
| `fee` on a servicetype=A2A entry not matching the internal validation pattern (**internal pattern, never echoed to user**: `^\d+(\.\d{1,6})?$`) — **wire-level enum `A2A` ↔ user-visible long form `agent（智能体）通信式服务` / `agent-to-agent service` per `ux-lexicon.md §Service-type` Pattern A; do NOT leak `A2A` to the user** | User answered Q4 on an agent-通信式 service with something other than empty / number-with-≤6-decimals (e.g. `5 USDT`, `约 10`, `-1`) | Reject with "agent（智能体）通信式服务（议价 / 灵活协作）的价格是选填的，要么留空，要么填 USDT 数字最多六位小数（例如 `1.234567` / `10` / `0.5`）" / "agent-to-agent service (negotiated / off-chain pricing) fee is optional — leave it empty or supply a USDT number with up to 6 decimal places" — Pattern A (long form inline gloss) per `ux-lexicon.md §Service-type`. Re-ask Q4 (A2A branch — internal label). The CLI (`utils.rs::normalize_service` A2A arm) does NOT validate the fee format on A2A — this is skill-side only. |
| `endpoint` on a servicetype=A2MCP entry exceeds the skill-side length limit (> 512 chars) — **the 512 limit is hidden from the Q5 prompt; mention it only here, after the user's input failed**; wire-level `A2MCP` ↔ user-visible `API 接口` / `API service` | User Q5 reply on an API-接口 service is longer than 512 chars | Reject with "接口地址最长 512 字符，这个超了，麻烦换个短点的 URL。" / "The endpoint URL must be at most 512 chars; this one exceeds it. Please use a shorter URL." Re-ask Q5 (A2MCP branch — internal label). The CLI (`utils.rs::normalize_service`) does NOT validate endpoint length — this is skill-side only. |

---

## General handling principles

1. **Translate, don't parrot.** Always show the user the 中文 friendly version; the raw message goes into the footer of the error card (`display-formats.md` §7) for debuggability.
2. **Recover, don't abort.** For every row above, there is a concrete "回到哪一步" action. Keep the user in the flow.
3. **Do not retry silently** for business errors (4xx-class). Render the error card and stop — the user decides the next step. See `_shared/no-polling.md`.
4. **Retry once** for transient 5xx/network errors. If it fails a second time, surface the error and move on. Never loop.
5. **Do not chase failures with a `get`.** If `activate` fails, do NOT run `agent get` to "see what happened" — the error message is authoritative. Render the card and wait.
6. **Update this file** the moment `cli/src/commands/agent_commerce/identity/**` changes a `bail!` string, or the moment you observe a backend message whose keywords don't match any row here — otherwise translations will silently rot.
