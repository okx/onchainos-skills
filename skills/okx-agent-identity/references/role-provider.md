# Role: provider (服务方)

> Registers a seller identity **with at least one service**. Longest Q&A — take it one step at a time.

## STRICT — one question per turn

No listing "请提供 1. 名字 2. 描述 3. 服务名称 ..." / "Please provide 1. Name 2. Description 3. Service Name ...". Every field, including every service sub-field, is a separate turn in the user's language.

Field definitions live in `field-specs.md`. Inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese; `Purpose / Visibility / Please note / Example` for English) in the user's language only, so they don't need to read a separate file to answer.

## Phase 1 — identity Q&A

### Phase 1 preview (render BEFORE Q1)

Once role is `provider` and pre-check resolved (either "no existing provider" or user chose "1. 再开一个新的 provider" on the pre-check numbered prompt), render the Phase-1 preview, then start Q1.

Chinese:
```
好，开始新 provider 的 create 流程。先收集身份基本信息：
  1. 名称
  2. 描述
  3. 头像（可选）
（服务列表会在身份信息确认后再继续收集。）
```

English:
```
Got it — starting a new provider create. First we'll collect identity info:
  1. Name
  2. Description
  3. Picture (optional)
(The service list is collected after identity is confirmed.)
```

The preview is declarative; Q1 follows after a blank line. See `role-playbook.md §STRICT — Preview ≠ multi-field ask`.

### Q&A

Questions labelled `Q1：` / `Q1:`. Each Q inlines the four-segment field spec from `field-specs.md` in the user's language only. Skip any Q whose field was already captured via §One-shot capture.

| Q | Chinese prompt | English prompt | Validation |
|---|---|---|---|
| Q1 | `Q1：这个 provider 叫什么名字？` + 4 segments | `Q1: What's the name of this provider?` + 4 segments | non-empty, ≤ 64 chars |
| Q2 | `Q2：用一句话描述这个 provider。` + 4 segments | `Q2: Describe this provider in a sentence.` + 4 segments | non-empty, ≤ 500 chars |
| Q3 | `Q3：要设置头像吗？` + Choice prompt (see `avatar-upload.md`) | `Q3: Want to set an avatar?` + Choice prompt | — |

**Strict phase boundary**: Phase 1 only captures `name` / `description` / `picture`. Even if the user mentions service info ("收 10 USDT"), do NOT capture it here — see `SKILL.md §One-shot capture rule 4`.

After Q3 answered, render the Phase-1 confirmation card (identity only, no service rows yet — but note: that is **not** the final `create` — final confirmation happens after Phase 2). Or alternatively, hold identity in-memory and show one combined confirmation at the end of Phase 2. **This skill does the latter**: one final confirmation card after all services are collected. Phase-1 end transitions directly to Phase-2 preview.

## Phase 2 — service Q&A (loop once per service)

### Phase 2 preview (render BEFORE the first service's Q1)

Once Phase 1 is complete, render the Phase-2 preview **once** (not repeated for subsequent services in the loop). Then start service[1]'s Q1.

Chinese:
```
身份信息收到。接下来为这个 provider 添加服务，每条服务会问：
  1. 名称
  2. 描述
  3. 类型（A2MCP 或 A2A）
  4. 价格（仅 A2MCP）
  5. 接口地址（仅 A2MCP）
加完一条后会问是否继续加下一条。可以加一条或多条。
```

English:
```
Identity info captured. Next we'll add services for this provider. For each service we'll ask:
  1. Name
  2. Description
  3. Type (A2MCP or A2A)
  4. Fee (A2MCP only)
  5. Endpoint (A2MCP only)
After each service we'll ask whether to add another. One or more services, your choice.
```

Preview is declarative, not imperative — see `role-playbook.md §STRICT`.

### Per-service Q&A

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee + endpoint are only needed for A2MCP.

Questions inside each service iteration are labelled `Q1：` / `Q2：` / … / `Q5：` (reset per iteration). The preamble for service `[N]` ("接下来是服务[N]：" / "Now service [N]:") contextualizes which service is being collected. Q6 is the loop gate and uses the numbered-options pattern (not a "Q" label).

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

Chinese per-service Q&A (render `接下来是服务[N]：` as a one-line preamble before Q1):

| Step | 问用户 (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `Q1：这个服务叫什么名字？` + 4 segments (see `field-specs.md`) | non-empty, ≤ 64 chars | `name` |
| Q2 | `Q2：详细介绍一下这项服务。` + 4 segments | non-empty, ≤ 500 chars | `servicedescription` |
| Q3 | `Q3：这项服务是哪种类型？` + numbered-options (`SKILL.md §Choice prompts`):<br>&nbsp;&nbsp;`1. A2MCP — 标准 MCP 接口，按次付费`<br>&nbsp;&nbsp;`2. A2A — agent-to-agent 协议，链外议价`<br>`回复 1 或 2。`<br>收到数字后**在 skill 层映射** `1→A2MCP` / `2→A2A` 再下发；CLI 没有数字别名，直接传 `1` 会 bail `invalid servicetype`。用户直接写 `A2MCP` / `A2A` 也接受。 | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if `A2MCP` → `Q4：每次调用收多少 USDT？（整数）` + 4 segments ; if `A2A` → skip | integer ≥ 0 | `fee` |
| Q5 | if `A2MCP` → `Q5：MCP 接口地址是什么？必须 https:// 开头。` + 4 segments ; if `A2A` → skip | starts with `https://` | `endpoint` (A2A 即使用户给了 CLI 也会清掉，见 `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt (no `Q` label, it's a flow switch):<br>`还要再加一项服务吗？`<br>&nbsp;&nbsp;`1. 再加一项`<br>&nbsp;&nbsp;`2. 不加了，到此为止`<br>`回复 1 或 2。` | reply 1 or 2 | — |

English per-service Q&A (render `Now service [N]:` as a one-line preamble before Q1):

| Step | Ask the user (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `Q1: What's the name of this service?` + 4 segments | non-empty, ≤ 64 chars | `name` |
| Q2 | `Q2: Describe this service.` + 4 segments | non-empty, ≤ 500 chars | `servicedescription` |
| Q3 | `Q3: Which type is this service?` + numbered-options:<br>&nbsp;&nbsp;`1. A2MCP — standard MCP interface, pay-per-call`<br>&nbsp;&nbsp;`2. A2A — agent-to-agent protocol, off-chain pricing`<br>`Reply 1 or 2.`<br>Once user replies, **map in skill** `1→A2MCP` / `2→A2A` before invoking the CLI — the CLI has no numeric alias and sending raw `1` bails with `invalid servicetype`. Writing `A2MCP` / `A2A` directly is also accepted. | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if A2MCP → `Q4: Price per call in USDT? (integer)` + 4 segments ; if A2A → skip | integer ≥ 0 | `fee` |
| Q5 | if A2MCP → `Q5: What's the MCP endpoint URL? Must start with https://.` + 4 segments ; if A2A → skip | starts with `https://` | `endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt:<br>`Want to add another service?`<br>&nbsp;&nbsp;`1. Add another`<br>&nbsp;&nbsp;`2. No more, finish here`<br>`Reply 1 or 2.` | reply 1 or 2 | — |

After each service is collected, echo back a one-line summary in the user's language before the loop gate:
- 中文：`已记录 服务[1]：TVL Query（A2MCP，10 USDT，https://…）。`
- English: `Recorded Service [1]: TVL Query (A2MCP, 10 USDT, https://…).`

## Good / bad cases

| User input | Action |
|---|---|
| "我要做数据分析服务，收 10 USDT"（**在 Phase 1 说的**） | Do **NOT** capture `fee=10` at Phase 1 — phase boundary is strict (`SKILL.md §One-shot capture` rule 4). Continue Phase 1 Q&A; when Phase 2 starts fresh, ask Q3 (`servicetype`) first, then Q4 (`fee`) where the user can re-supply 10 if still relevant. |
| "我要做数据分析服务，收 10 USDT"（**在 Phase 2 的某条服务中说的**） | Capture `fee=10` when Q4 asks it; still confirm `servicetype` at Q3 first. |
| "帮我写几个 service" | Refuse to fabricate. Ask what they actually want to offer. |
| User pastes JSON blob | Thank them, but re-confirm **field by field** — typos in `servicetype` are the #1 cause of create failures. Do not pipe JSON straight to the CLI. |
| "endpoint 是 http://..." | Reject. Ask for HTTPS. |
| "A2MCP Fee 免费" | Accept `0` but warn: "A2MCP 0 USDT 等同于免费入口，后续不能再按量收费。" |
| User answers multiple service fields in one sentence | Parse what you can, but next turn still asks the remaining fields individually. |
| "服务类型 HTTP" / "service type HTTP" | Reject politely and re-render the Q3 numbered prompt verbatim (see `SKILL.md §Choice prompts`) — do not fabricate a new phrasing. |

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

Service-field **labels in the card** are localized (see mapping table in `display-formats.md §Create/Update Diff`: `名称 / 描述 / 类型 / 价格 / 接口地址` ↔ `Name / Description / Type / Fee / Endpoint`). The **JSON keys actually sent to the CLI** (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) are lowercase schema per `models.rs::AgentService` — they only show up in the raw bash command, which we render only if the user explicitly asks.

**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks ("把命令给我看" / "show me the CLI").

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role provider \
  --name "<name>" \
  --description "<description>" \
  --service '[{"name":"…","servicedescription":"…","servicetype":"A2MCP","fee":"10","endpoint":"https://…"}, …]' \
  [--picture "<url>"]
```

## Post-success suggestion

Render **one visible line** in the user's language (one of the four variants below), then follow the **agent directive** block (internal — not rendered to the user). Follow the `#<id>` placeholder rule in `display-formats.md` — include the id only when it's actually known.

### Visible line (four variants)

With id (Chinese): "Provider 身份 #<id> 已创建并默认上架（已上架）。可以 `agent search` 自检曝光，或直接等匹配来的任务。"
Without id (Chinese): "Provider 身份已创建并默认上架（已上架）。可以 `agent search` 自检曝光，或直接等匹配来的任务。"
With id (English): "Provider agent #<id> created and active by default. Run `agent search` to sanity-check exposure, or wait for matching tasks."
Without id (English): "Provider agent created and active by default. Run `agent search` to sanity-check exposure, or wait for matching tasks."

**Create 默认返回 active** / **Create returns active by default**，不需要再 `agent activate`。`activate` 只用于用户之前主动 `deactivate` 过、现在想恢复上架的场景。 `activate` is only for users who previously `deactivate`'d and want to re-publish.

Do NOT mention the `okx-agent-chat/ensure-installed.md` path to the user in the visible line — the same-turn handoff below loads that skill's own prompt, which speaks for itself.

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. In the same response, load `/skills/okx-agent-chat/ensure-installed.md` and continue with its `Execution Flow` inside the same response — a fresh provider is immediately discoverable and matchable, so the XMTP messaging layer must be ready before the first inbound DM lands.

**Skip the handoff** (render visible line only, then stop) if the user has already declined chat / messaging setup earlier in this conversation (e.g., "不用聊天 / no chat / skip messaging" or similar).

This is one of the documented exceptions to `SKILL.md §Step 4`'s "Stop. Wait for the user." rule; see that section's whitelist for the carve-out.

**Do NOT** run `agent get` or poll status after create (that is about querying chain state — different from the same-turn handoff above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to Phase 2 Q1 of service[1]. If `missing required field in --service: name` surfaces, return to the specific Q (see `troubleshooting.md`). Never silently retry with a filler value.
