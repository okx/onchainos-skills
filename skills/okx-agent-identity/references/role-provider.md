# Role: provider (服务方)

> Registers a seller identity **with at least one service**. Longest Q&A — take it one step at a time.

## STRICT — one question per turn

No listing "请提供 1. name 2. description 3. ServiceName ...". Every field, including every service sub-field, is a separate turn.

Field definitions (用途 / 可见范围 / 约束 / 示例) live in `field-specs.md`. Inline them with the prompt so the user doesn't need to read a separate file to answer.

## Phase 1 — identity Q&A

| Turn | Ask | Validation |
|---|---|---|
| 1 | `Name` | non-empty, ≤ 64 chars |
| 2 | `Description` | non-empty, ≤ 500 chars |
| 3 | (optional) `Picture` → branch to `avatar-upload.md` | — |

## Phase 2 — service Q&A (loop once per service)

For each service, ask the fields in this exact order. The reason: `ServiceName` / `ServiceDescription` apply to both types, so they come first; `ServiceType` is the branching switch; `Fee` / `Endpoint` are only needed for `A2MCP`.

| Turn | Ask | Validation | Maps to | Notes |
|---|---|---|---|---|
| S1 | `ServiceName` | non-empty | `ServiceName` | — |
| S2 | `ServiceDescription` | non-empty | `ServiceDescription` | — |
| S3 | `ServiceType` — "A2MCP（MCP 接口）还是 A2A（agent-to-agent 协议）？" | one of `A2MCP` / `A2A` (case-insensitive, skill emits uppercase) | `ServiceType` | branching step |
| S4 | if `A2MCP` → `Fee` — "每次调用收多少 USDT？整数" ；if `A2A` → skip | integer ≥ 0 | `Fee` | A2A does not ask |
| S5 | if `A2MCP` → `Endpoint` — "MCP endpoint URL？必须 https://" ；if `A2A` → skip | starts with `https://` | `Endpoint` | A2A even if given is cleared by CLI (`utils.rs::normalize_service`) |
| S6 | "要再加一个服务吗？（可选）" | yes → loop back to S1; no → exit | — | always ask once, don't auto-stop after the first service |

> After collecting each service, echo back a one-line summary before moving to the "再加一个吗" question: `已记录 [1] TVL Query (A2MCP, 10 USDT, https://…)。`

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

Two-column table (`display-formats.md` §Update/Create Diff), services numbered inline:

| Field | Value |
|---|---|
| role | provider (服务方) |
| name | DeFi Analyzer |
| description | On-chain data analysis and yield simulation. |
| picture | 默认 |
| services[1] ServiceName | TVL Query |
| services[1] ServiceDescription | Query protocol TVL by chain via MCP. |
| services[1] ServiceType | A2MCP |
| services[1] Fee | 10 USDT |
| services[1] Endpoint | https://api.example.com/mcp |
| services[2] ServiceName | Yield Check |
| services[2] ServiceType | A2A |

> 确认无误回复 "执行" 我就下发。

**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks "把命令给我看".

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

> Provider agent #<id> 已创建，状态 `inactive`。要现在 `agent activate <id>` 上架吗？

**Do NOT** run `agent get` or poll status after create. See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to Phase 2 S1. If `missing required field in --service: ServiceName` surfaces, return to the specific step (see `troubleshooting.md`). Never silently retry with a filler value.
