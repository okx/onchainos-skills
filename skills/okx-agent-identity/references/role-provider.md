# Role: provider (服务方)

> Registers a seller identity **with at least one service**. Longest Q&A — take it one step at a time.

## STRICT — one question per turn

No listing "请提供 1. 名字 2. 描述 3. 服务名称 ..." / "Please provide 1. Name 2. Description 3. Service Name ...". Every field, including every service sub-field, is a separate turn in the user's language.

Field definitions live in `field-specs.md`. Inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese; `Purpose / Visibility / Please note / Example` for English) in the user's language only, so they don't need to read a separate file to answer.

## Phase 1 — identity Q&A

| Turn | Ask | Validation |
|---|---|---|
| 1 | `Name` | non-empty, ≤ 64 chars |
| 2 | `Description` | non-empty, ≤ 500 chars |
| 3 | (optional) `Picture` → branch to `avatar-upload.md` | — |

## Phase 2 — service Q&A (loop once per service)

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee + endpoint are only needed for A2MCP.

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

Chinese Q&A:

| Turn | 问用户 | Validation | Maps to (JSON key) |
|---|---|---|---|
| S1 | 这项服务叫什么名字？ | non-empty | `ServiceName` |
| S2 | 详细介绍一下这项服务。 | non-empty | `ServiceDescription` |
| S3 | 这项服务的类型是 A2MCP（MCP 接口，按次付费）还是 A2A（agent-to-agent 协议，链外议价）？ | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `ServiceType` |
| S4 | if `A2MCP` → "每次调用收多少 USDT？（整数）" ; if `A2A` → skip | integer ≥ 0 | `Fee` |
| S5 | if `A2MCP` → "MCP 接口地址是什么？必须 https:// 开头。" ; if `A2A` → skip | starts with `https://` | `Endpoint` (A2A 即使用户给了 CLI 也会清掉，见 `utils.rs::normalize_service`) |
| S6 | "还要再加一项服务吗？（可选）" | yes → loop back to S1; no → exit | — |

English Q&A:

| Turn | Ask the user | Validation | Maps to (JSON key) |
|---|---|---|---|
| S1 | What's the name of this service? | non-empty | `ServiceName` |
| S2 | Describe this service. | non-empty | `ServiceDescription` |
| S3 | Is this service A2MCP (MCP interface, pay-per-call) or A2A (agent-to-agent protocol, off-chain pricing)? | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `ServiceType` |
| S4 | if A2MCP → "Price per call in USDT? (integer)" ; if A2A → skip | integer ≥ 0 | `Fee` |
| S5 | if A2MCP → "What's the MCP endpoint URL? Must start with https://." ; if A2A → skip | starts with `https://` | `Endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |
| S6 | "Want to add another service? (optional)" | yes → loop back to S1; no → exit | — |

After each service is collected, echo back a one-line summary in the user's language before asking S6:
- 中文：`已记录 服务[1]：TVL Query（A2MCP，10 USDT，https://…）。`
- English: `Recorded Service [1]: TVL Query (A2MCP, 10 USDT, https://…).`

## Good / bad cases

| User input | Action |
|---|---|
| "我要做数据分析服务，收 10 USDT" | Enter phase 2; capture `Fee=10` when S4 comes; still confirm `ServiceType` at S3 first. |
| "帮我写几个 service" | Refuse to fabricate. Ask what they actually want to offer. |
| User pastes JSON blob | Thank them, but re-confirm **field by field** — typos in `ServiceType` are the #1 cause of create failures. Do not pipe JSON straight to the CLI. |
| "endpoint 是 http://..." | Reject. Ask for HTTPS. |
| "A2MCP Fee 免费" | Accept `0` but warn: "A2MCP 0 USDT 等同于免费入口，后续不能再按量收费。" |
| User answers multiple service fields in one sentence | Parse what you can, but next turn still asks the remaining fields individually. |
| "服务类型 HTTP" | Reject politely, re-ask with the two options (A2MCP / A2A) and one-line explanation each. |

## Confirmation

Two-column table (`display-formats.md` §Create/Update Diff), services numbered inline. Render in the user's language — pick ONE variant.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 服务方 (`provider`) |
| 名字 | DeFi Analyzer |
| 描述 | 链上数据分析与收益模拟。 |
| 头像 | 默认 |
| 服务[1] 名称 | TVL Query |
| 服务[1] 描述 | 通过 MCP 按链查询协议 TVL。 |
| 服务[1] 类型 | A2MCP |
| 服务[1] 价格 | 10 USDT |
| 服务[1] 接口地址 | https://api.example.com/mcp |
| 服务[2] 名称 | Yield Check |
| 服务[2] 类型 | A2A |

> 确认无误回复 "执行" 我就下发。

English variant:

| Field | Value |
|---|---|
| Role | provider |
| Name | DeFi Analyzer |
| Description | On-chain data analysis and yield simulation. |
| Picture | default |
| Service [1] Name | TVL Query |
| Service [1] Description | Query protocol TVL by chain via MCP. |
| Service [1] Type | A2MCP |
| Service [1] Fee | 10 USDT |
| Service [1] Endpoint | https://api.example.com/mcp |
| Service [2] Name | Yield Check |
| Service [2] Type | A2A |

> Reply "execute" to run it.

Service-field **labels in the card** are localized (see mapping table in `display-formats.md §Create/Update Diff`: `名称 / 描述 / 类型 / 价格 / 接口地址` ↔ `Name / Description / Type / Fee / Endpoint`). The **JSON keys actually sent to the CLI** (`ServiceName` / `ServiceDescription` / `ServiceType` / `Fee` / `Endpoint`) stay unchanged — they only show up in the raw bash command, which we render only if the user explicitly asks.

**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks ("把命令给我看" / "show me the CLI").

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role provider \
  --name "<name>" \
  --description "<description>" \
  --service '[{"ServiceName":"…","ServiceDescription":"…","ServiceType":"A2MCP","Fee":"10","Endpoint":"https://…"}, …]' \
  [--picture "<url>"]
```

## Post-success suggestion

One line, concrete next step:

One-line next step, in the user's language. Follow the `#<id>` placeholder rule in `display-formats.md` — include the id only when it's actually known.

With id (Chinese): "Provider 身份 #<id> 已创建并默认上架（已上架）。可以 `agent search` 自检曝光，或直接等匹配来的任务。"
Without id (Chinese): "Provider 身份已创建并默认上架（已上架）。可以 `agent search` 自检曝光，或直接等匹配来的任务。"
With id (English): "Provider agent #<id> created and active by default. Run `agent search` to sanity-check exposure, or wait for matching tasks."
Without id (English): "Provider agent created and active by default. Run `agent search` to sanity-check exposure, or wait for matching tasks."

**Create 默认返回 active** / **Create returns active by default**，不需要再 `agent activate`。`activate` 只用于用户之前主动 `deactivate` 过、现在想恢复上架的场景。 `activate` is only for users who previously `deactivate`'d and want to re-publish.

**Do NOT** run `agent get` or poll status after create. See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to Phase 2 S1. If `missing required field in --service: ServiceName` surfaces, return to the specific step (see `troubleshooting.md`). Never silently retry with a filler value.
