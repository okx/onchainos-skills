# Troubleshooting — CLI errors → user-friendly translation

All strings in the first column are raw `bail!` messages emitted by `cli/src/commands/agent_commerce/identity/*.rs`. If CLI text changes, update this table in the same commit.

| CLI error | Source | User-facing translation | Skill action |
|---|---|---|---|
| `session expired, please login again` | `signing.rs` | "登录态过期了" | Hand off to `okx-agentic-wallet` → `wallet login`, then retry the original command. |
| `no XLayer address found in current account` | `signing.rs` | "当前账号没有 XLayer 地址" | Hand off to `okx-agentic-wallet` → `wallet add` / `wallet switch`. |
| `missing required field in --service: ServiceName` | `utils.rs:136` | "服务名不能留空" | Return to role-playbook `provider` service Q&A, step 1. |
| `missing required field in --service: ServiceDescription` | `utils.rs:139` | "服务描述不能留空" | Return to role-playbook `provider` service Q&A, step 2. |
| `missing required field in --service for A2MCP: Fee` | `utils.rs:148` | "A2MCP 服务必须给 Fee（USDT 整数）" | Return to role-playbook `provider` service Q&A, step 4 (A2MCP branch). |
| `missing required field in --service for A2MCP: Endpoint` | `utils.rs:151` | "A2MCP 服务必须给 endpoint（HTTPS URL）" | Return to role-playbook `provider` service Q&A, step 5 (A2MCP branch). |
| `invalid ServiceType in --service` | `utils.rs:154` | "服务类型必须是 A2MCP 或 A2A" | Return to role-playbook service Q&A, step 3. |
| `invalid value for --role` | `utils.rs:165` | "role 只能是 requester / provider / evaluator 之一" | Return to role selection (SKILL.md §Core Flow). |
| `provider agents require at least one service` | `mutations.rs` | "provider 必须有至少一个 service" | Return to role-playbook `provider` service Q&A loop. |
| `no updatable field supplied` | `mutations.rs` | "至少要改一个字段（name / description / picture / service）" | Go back to update Q&A, ask which field. |
| `agent not found` | `queries.rs` / `mutations.rs` | "找不到该 agent" | Verify the id with `agent get`; maybe the user misread. |
| `agent already active` | `mutations.rs` | "Agent 已经是 active 状态，无需再次 activate" | No-op; show detail card. |
| `agent already inactive` | `mutations.rs` | "Agent 已经是 inactive 状态" | No-op; show detail card. |
| `cannot deactivate: pending settlements` | `mutations.rs` | "有未完结的任务引用这个 agent，需要先去 `okx-agent-task` 处理完" | Hand off to `okx-agent-task`. |
| `score out of range` | `mutations.rs` | "分数要在 0-100 之间的整数" | Return to `feedback-guide.md` step 3. |
| `self-rating not allowed` | `mutations.rs` | "不能给自己的 agent 打分" | Return to `feedback-guide.md` step 1 (target). |
| `creator agent not owned by caller` | `mutations.rs` | "`--creator-id` 必须是你自己的 agent id" | Return to `feedback-guide.md` step 2 (re-resolve). |
| `query is required` | `queries.rs` | "搜索必须给一句话描述" | Ask the user what they want to find. |
| `query too long, truncated to 200 chars` | `queries.rs` | "搜索语句超过 200 字，已截断" | Informational — results still returned; offer to split into multiple searches if needed. |
| `invalid sort-by` | `queries.rs` | "排序值只能是 newest / highest / lowest" | Return to the `feedback-list` prompt. |
| `file not found` | `upload.rs` | "找不到文件" | Ask the user to recheck the path; in terminal mode switch to AI-gen / skip (see `avatar-upload.md`). |
| `unsupported media type` | `upload.rs` | "头像格式不支持" | Ask user to convert to PNG / JPEG / WebP. |
| `Wallet API server error (HTTP 500)` | runtime | "后端暂时不可用" | Wait and retry once; if still failing, suggest trying again later. |
| Region-restriction codes `50125` / `80001` | runtime | "Service is not available in your region." | Do NOT echo the raw code. Do NOT suggest VPNs. |
| TEMP MOCK empty `txHash` on pre-transaction | runtime (CLI mock path) | "交易还没正式上链（走了临时 mock 路径），请稍后复查状态" | Log event; once the mock path is removed, update this row. |

---

## General handling principles

1. **Translate, don't parrot.** Always show the user the 中文 friendly version; the raw CLI message goes into the footer of the error card (`display-formats.md` §6) for debuggability.
2. **Recover, don't abort.** For every row above, there is a concrete "回到哪一步" action. Keep the user in the flow.
3. **Do not retry silently** for business errors (4xx-class). Report to the user and ask.
4. **Retry once** for transient 5xx/network errors. If it fails a second time, surface the error and move on.
5. **Update this file** the moment `cli/src/commands/agent_commerce/identity/**` changes a `bail!` string — otherwise translations will silently rot.
