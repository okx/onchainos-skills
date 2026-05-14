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
好，开始注册新卖家身份。先收集身份基本信息：
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

The `Q1 / Q2 / Q3` labels in the column below are **maintainer-internal only** — they help this document index questions but **MUST NOT** appear in the prompt strings the AI sends to the user. The prompts in the Chinese/English columns are the literal text rendered to the user; they carry no `Q1：` / `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` (no Q/S/Phase leakage) and `references/ux-lexicon.md` for the canonical rule. Each prompt inlines the four-segment field spec from `field-specs.md` in the user's language only. Skip any Q whose field was already captured via §One-shot capture.

| Q | Chinese prompt | English prompt | Validation |
|---|---|---|---|
| Q1 | `这个卖家身份叫什么名字？` + 4 segments | `What's the name of this provider?` + 4 segments | non-empty, ≤ 64 chars |
| Q2 | `用一句话描述这个卖家身份。` + 4 segments | `Describe this provider in a sentence.` + 4 segments | non-empty, ≤ 500 chars |
| Q3 | `头像呢？用默认还是上传一张？` + Choice prompt (see `avatar-upload.md`) | `Avatar? Default, or upload one?` + Choice prompt | — |

**Strict phase boundary**: Phase 1 only captures `name` / `description` / `picture`. Even if the user mentions service info ("收 10 USDT"), do NOT capture it here — see `SKILL.md §One-shot capture rule 4`.

After Q3 answered, render the Phase-1 confirmation card (identity only, no service rows yet — but note: that is **not** the final `create` — final confirmation happens after Phase 2). Or alternatively, hold identity in-memory and show one combined confirmation at the end of Phase 2. **This skill does the latter**: one final confirmation card after all services are collected. Phase-1 end transitions directly to Phase-2 preview.

## Phase 2 — service Q&A (loop once per service)

### Phase 2 preview (render BEFORE the first service's Q1)

Once Phase 1 is complete, render the Phase-2 preview **once** (not repeated for subsequent services in the loop). Then start service[1]'s Q1.

Chinese:
```
身份信息收到。接下来给这个卖家身份配服务，每条服务会问：
  1. 名称
  2. 描述
  3. 类型（"API 接口式" 按次付费 / "agent 通信式" 议价 — 后面会展开问）
  4. 价格（API 接口式必填，agent 通信式选填，单位 USDT）
  5. 接口地址（仅 API 接口式需要）
加完一条后会问是否继续加下一条。可以加一条或多条。
```

English:
```
Identity info captured. Next we'll add services for this provider. For each service we'll ask:
  1. Name
  2. Description
  3. Type (A2MCP = API-interface, pay-per-call / A2A = agent-to-agent, off-chain pricing — explained again when we ask)
  4. Fee in USDT (A2MCP required, A2A optional)
  5. Endpoint (A2MCP only)
After each service we'll ask whether to add another. One or more services, your choice.
```

Preview is declarative, not imperative — see `role-playbook.md §STRICT`.

### Per-service Q&A

For each service, ask the fields in this exact order. The reason: name + description apply to both types, so they come first; type is the branching switch; fee is required for A2MCP and optional for A2A (when an A2A user skips, the wire payload still carries `"fee": ""` because `cli/src/commands/agent_commerce/identity/models.rs:21` declares `fee: String` without `skip_serializing_if`); endpoint is only needed for A2MCP.

The `Q1 / Q2 / ... / Q5` column labels in the per-service tables below are **maintainer-internal indexes only** — they reset per service iteration but **MUST NOT** appear as prefixes in the prompt strings the AI sends to the user. The prompts in the Chinese/English columns are the literal text rendered to the user; they carry no `Q1：` / `Q1:` prefix. See `SKILL.md §UX Output Red Lines Red line 3` and `references/ux-lexicon.md`. The preamble for service `[N]` ("接下来是服务[N]：" / "Now service [N]:") contextualizes which service is being collected. The loop gate is a numbered-options pattern, not a Q-labelled question.

The **Ask column below shows what the skill says to the user, in user language**. The **Maps to column shows the CLI JSON key** that the collected value lands under in the `--service` payload — that stays English and unchanged regardless of user language.

Chinese per-service Q&A (render `接下来是服务[N]：` as a one-line preamble before Q1):

| Step | 问用户 (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `这个服务叫什么名字？` + 4 segments (see `field-specs.md`) | non-empty, ≤ 64 chars | `name` |
| Q2 | `详细介绍一下这项服务。` + 4 segments | non-empty, ≤ 500 chars | `servicedescription` |
| Q3 | `这项服务是哪种类型？` + numbered-options (`SKILL.md §Choice prompts`):<br>&nbsp;&nbsp;`1. API 接口式服务（按次付费，标准 MCP 接口）`<br>&nbsp;&nbsp;`2. agent 通信式服务（agent-to-agent，价格默认链外议价；可选填上链参考价）`<br>`回复 1 或 2。`<br>收到数字后**在 skill 层映射** `1→A2MCP` / `2→A2A` 再下发；CLI 没有数字别名，直接传 `1` 会 bail `invalid servicetype`。用户直接写 `A2MCP` / `A2A` 也接受（已经认识术语）。⛔ Never render bare `A2MCP` / `A2A` to first-time user without the gloss — see `references/ux-lexicon.md` service-type table. | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if `A2MCP` → `每次调用收多少 USDT？（最多六位小数，例如 1.234567 / 10 / 0.5 / 0）` + 4 segments ; if `A2A` → `这项服务的参考价是多少 USDT？（选填，最多六位小数；不填表示链外按次议价。直接回车 / 回复 "跳过" / "skip" 即可跳过）` + 4 segments | A2MCP: number ≥ 0，最多六位小数。**Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})?$`，非空必填。A2A: 空 或 满足同一 pattern | `fee` (A2A 跳过时仍会以 `"fee":""` 进入 wire payload——`models.rs:21` 的 `fee: String` 没有 `skip_serializing_if`。skill 渲染时按 `空 → 免费/free`；后端是否区分"空串 vs 缺失键"由产品 spec 决定，本地代码不可证实) |
| Q5 | if `A2MCP` → `MCP 接口地址是什么？必须 https:// 开头，且公网可达（买家会从公网来调你）。` + 4 segments ; if `A2A` → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (A2A 即使用户给了 CLI 也会清掉，见 `utils.rs::normalize_service`) |
| Loop gate | Numbered-options prompt (no `Q` label, it's a flow switch):<br>`还要再加一项服务吗？`<br>&nbsp;&nbsp;`1. 再加一项`<br>&nbsp;&nbsp;`2. 不加了，到此为止`<br>`回复 1 或 2。` | reply 1 or 2 | — |

English per-service Q&A (render `Now service [N]:` as a one-line preamble before Q1):

| Step | Ask the user (label and prompt) | Validation | Maps to (JSON key) |
|---|---|---|---|
| Q1 | `What's the name of this service?` + 4 segments | non-empty, ≤ 64 chars | `name` |
| Q2 | `Describe this service.` + 4 segments | non-empty, ≤ 500 chars | `servicedescription` |
| Q3 | `Which type is this service?` + numbered-options:<br>&nbsp;&nbsp;`1. API-interface service (pay-per-call, standard MCP interface)`<br>&nbsp;&nbsp;`2. agent-to-agent service (off-chain pricing by default; optional on-chain reference fee)`<br>`Reply 1 or 2.`<br>Once user replies, **map in skill** `1→A2MCP` / `2→A2A` before invoking the CLI — the CLI has no numeric alias and sending raw `1` bails with `invalid servicetype`. Writing `A2MCP` / `A2A` directly is also accepted (user already speaks the term). ⛔ Never render bare `A2MCP` / `A2A` to a first-time user without the gloss — see `references/ux-lexicon.md` service-type table. | one of `A2MCP` / `A2A` (case-insensitive; skill emits uppercase) | `servicetype` |
| Q4 | if A2MCP → `Price per call in USDT? (up to 6 decimal places, e.g., 1.234567 / 10 / 0.5 / 0)` + 4 segments ; if A2A → `Reference price in USDT for this service? (optional, up to 6 decimal places; leave empty to signal off-chain per-call negotiation. Press enter / reply "skip" / "跳过" to skip)` + 4 segments | A2MCP: number ≥ 0, ≤ 6 decimal places. **Internal validation pattern, do NOT show to user**: `^\d+(\.\d{1,6})?$`, must be non-empty. A2A: empty OR matches the same pattern | `fee` (when A2A is left empty, the wire payload still carries `"fee": ""` — `models.rs:21` `fee: String` has no `skip_serializing_if`. The skill renders empty fee as `免费` / `free`; whether the backend distinguishes empty-string from absent-key is governed by the product spec and cannot be verified from this repo) |
| Q5 | if A2MCP → `What's the MCP endpoint URL? Must start with https:// and be reachable from the public internet (buyers will call it from anywhere).` + 4 segments ; if A2A → skip | starts with `https://`; **Internal length limit, do NOT proactively show to user**: ≤ 512 chars (mention only when user input exceeds it); also reject any host matching `SKILL.md §Endpoint Anti-Pattern` blacklist (localhost / 127.0.0.1 / 192.168 / 10.* / 172.16-31.* / *.local / *.internal / Mock URL / `http://`). | `endpoint` (for A2A the CLI clears this even if supplied — `utils.rs::normalize_service`) |
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

> ⛔ Mandatory before invoking the CLI — applies to both single-service and multi-service provider creates. See `SKILL.md §⛔ MANDATORY confirmation gate (non-overridable)` for the canonical rule + the rationalizations (`auto-execute` / plan-mode exit / one-shot capture / urgency / "intent obvious") that do **NOT** bypass it.

Two-column table (`display-formats.md` §Create/Update Diff), services numbered inline. Render in the user's language — pick ONE variant.

> ⛔ The `<user-provided-endpoint>` token in the example below is a **doc-only placeholder** — at runtime substitute it with the **literal URL the user gave you in Phase 2 Q5** (or, on `update`, the new value the user just typed). **Never** copy any `https://api.example.com/...` / `https://cdn.example.com/...` / any other sample URL from these docs into the user's confirmation card. See `display-formats.md` top "URL literals are doc-only" rule.

Chinese variant:

| 字段 | 值 |
|---|---|
| 角色 | 服务方 |
| 名字 | DeFi Analyzer |
| 描述 | 链上数据分析与收益模拟。 |
| 头像 | 默认 |
| 服务[1] 名称 | TVL Query |
| 服务[1] 描述 | 通过 MCP 按链查询协议 TVL。 |
| 服务[1] 类型 | A2MCP |
| 服务[1] 价格 | 10 USDT |
| 服务[1] 接口地址 | `<user-provided-endpoint>` |
| 服务[2] 名称 | Yield Check |
| 服务[2] 类型 | A2A |
| 服务[2] 价格 | （未填，链外议价） |

> 确认无误回复 "执行" 即可。
> A2A 价格行：用户填了的话就照填的值显示（例如 `5 USDT`）；用户跳过的话显示`（未填，链外议价）`。

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
| Service [1] Endpoint | `<user-provided-endpoint>` |
| Service [2] Name | Yield Check |
| Service [2] Type | A2A |
| Service [2] Fee | (skipped — off-chain negotiation) |

> Reply "execute" to run it.
> A2A Fee row: when the user supplied a value, show it verbatim (e.g., `5 USDT`); when they skipped, show `(skipped — off-chain negotiation)`.

Service-field **labels in the card** are localized (see mapping table in `display-formats.md §Create/Update Diff`: `名称 / 描述 / 类型 / 价格 / 接口地址` ↔ `Name / Description / Type / Fee / Endpoint`). The **JSON keys actually sent to the CLI** (`name` / `servicedescription` / `servicetype` / `fee` / `endpoint`) are lowercase schema per `models.rs::AgentService` — they only show up in the raw bash command, which we render only if the user explicitly asks.

**Do NOT show bash** in the confirmation card. Only render the bash command if the user explicitly asks ("把命令给我看" / "show me the CLI").

## Execute (maintainer reference — not shown to user)

```bash
onchainos agent create \
  --role provider \
  --name "<name>" \
  --description "<description>" \
  --service '[{"name":"…","servicedescription":"…","servicetype":"A2MCP","fee":"10","endpoint":"https://…"}, {"name":"…","servicedescription":"…","servicetype":"A2A","fee":""}, {"name":"…","servicedescription":"…","servicetype":"A2A","fee":"5"}]' \
  [--picture "<url>"]
```

## ⛔ Post-success — MANDATORY template (do NOT paraphrase)

Render **one visible line** using the template below — **verbatim except for the `#<id>` substitution rule**. Then follow the §Agent directive block (internal — not rendered to the user). Paraphrasing, adding fields (txHash, agentList, activeClients, refresh-agents output), omitting fields, adding follow-up questions, or summarizing the CLI's other JSON output are all violations of `SKILL.md §⛔ MANDATORY post-execute gate`.

### Visible line (template)

Pick the variant matching the user's language. Render **one line, declarative, no question mark, no pre-announcement of the chat handoff** (the chat flow is a silent no-op outside an OpenClaw runtime; pre-announcing would mislead users in Claude Code / Claude Desktop):

- Chinese: `卖家身份 #<id> 注册完成，默认已上架可以接单。想看看市场上同类卖家长什么样、或确认你自己的曝光，跟我说"找做 ... 的卖家"我帮你搜；否则就等买家上门。`
- English: `Provider identity #<id> is live and active by default. Say "find providers doing X" if you want me to scan the marketplace for similar agents or confirm your exposure; otherwise just wait for matching tasks.`

**`#<id>` substitution rule** (per `display-formats.md` top, `#<id>` placeholder rule, **with provider-specific carve-out**):

- The legitimate sources of `#<id>` for this post-success line are, in priority order:
  1. **CLI response (direct):** the `create` call's response directly contains the new agent id — substitute it verbatim.
  2. **Post-create envelope diff:** the response envelope is double-layer (see `cli-reference.md §3`), so the filter is **wrapper-level**, not agent-row-level — **two steps, in order**: (a) locate the single wrapper in `envelope.agentList.list[*]` whose `list[*].ownerAddress == <currently selected XLayer wallet address>` (the address that signed this `create`), then (b) inside **that wrapper's** `agentList[*]` only, **diff against the pre-check `agent get` snapshot** captured by §⛔ MANDATORY pre-check gate — pick the agentId that's **newly present** (in the post-create envelope but not in the pre-check snapshot). This works regardless of whether pre-check returned K=0 or K≥1 existing providers; the diff isolates the freshly-minted id either way. ❌ Do NOT write the filter as `agentList[*].ownerAddress == ...` — agent rows have no `ownerAddress` field; that phrasing always misses. See `cli-reference.md §1` "Finding the newly-minted `agentId`" for the canonical algorithm.
  3. (Future) a follow-up `agent get` in a later turn — irrelevant for this immediate response.
- ⚠️ **Provider-specific danger zone — DO NOT pick any id directly from the pre-check list as `#<id>`.** Pre-check reflects state *before* this `create`, so its rows are all older providers, never the newly minted one. Source 2 above is **diff-based** (post-create envelope MINUS pre-check snapshot), not "borrow from pre-check"; it picks the id that's in the post-create envelope but **not** in the pre-check snapshot. Conflating the two is a real failure mode — the agent that does "I see provider #88 in pre-check, must be the new one" instead of running the diff will surface an older provider's id as if it were freshly created, which is misleading.
- If **both** source 1 (CLI direct id) and source 2 (envelope diff) miss — i.e. CLI returned `txHash` only **AND** the post-create `agentList` segment is also absent (WS + HTTP both failed, per `cli-reference.md §1`) **OR** the diff yielded no new candidate under the current wallet — **omit the `#<id> ` substring entirely**: do NOT render `#`, `#<id>`, `# ?`, do NOT invent a number, do NOT borrow from the pre-check list. Fallback lines:
  - Chinese: `卖家身份注册完成，默认已上架可以接单。想看看市场上同类卖家长什么样跟我说"找做 ... 的卖家"我帮你搜；否则就等买家上门。`
  - English: `Provider identity is live and active by default. Say "find providers doing X" if you want me to scan the marketplace; otherwise wait for matching tasks.`

**Create returns active by default** / **Create 默认返回 active** — no need to follow up with `agent activate`. `activate` is only for users who previously ran `deactivate` and now want to re-publish.

Do NOT mention the `okx-agent-chat/after-agent-list-changed.md` path to the user in the visible line — the same-turn handoff below loads that skill's own prompt, which decides on its own whether to surface anything (silent in non-OpenClaw runtimes).

### ❌ Anti-pattern (real incident, jobId=961) → ✅ Correct

❌ Agent paraphrased:
> "✅ 第二个 provider 已上链 / agentId 961 / 4 个活跃客户端 / 你现在有 4 个 agent"

Why this is a violation of `SKILL.md §⛔ MANDATORY post-execute gate`:

- Not the documented template wording — "已上链" / "第二个 provider" / "4 个活跃客户端" / "你现在有 4 个 agent" are all paraphrases.
- Mentions `活跃客户端` — that's internal `xmtp_refresh_agents` output, not user-relevant. Internal CLI fields (`agentList`, `activeClients`, refresh-agents counts, the full tx receipt) are NEVER user-facing; the template defines exactly what to expose.
- Re-renders / counts the agent list (`你现在有 4 个 agent`) — violates the §Agent directive's "do NOT run `agent get`" rule. Even if the count is technically derivable from a prior response, surfacing it on the post-success line is template drift.
- The natural-language "想看市场上同类卖家就跟我说…" half is missing — the suggested next action got dropped in favor of the inflated-success preamble.
- Uses the raw English `Provider` and the `agent search` CLI literal in Chinese user-visible text — violates `SKILL.md §UX Output Red Lines Red lines 1/2/4` and `references/ux-lexicon.md` (Chinese must localize role term to `卖家`, never paste CLI command for user to run).

✅ Correct (with id):
> 卖家身份 #961 注册完成，默认已上架可以接单。想看看市场上同类卖家长什么样、或确认你自己的曝光，跟我说"找做 ... 的卖家"我帮你搜；否则就等买家上门。

✅ Correct (id unknown, txHash-only return):
> 卖家身份注册完成，默认已上架可以接单。想看看市场上同类卖家长什么样跟我说"找做 ... 的卖家"我帮你搜；否则就等买家上门。

### Agent directive (internal — do NOT render to the user)

After emitting the visible line above, **do not stop the turn**. In the same response, load `/skills/okx-agent-chat/after-agent-list-changed.md` and continue with its `Execution Flow` inside the same response — the local a2a agent list just changed (a fresh provider was added and is immediately discoverable), so the chat skill keeps the OpenClaw runtime side in sync. The flow self-gates on `OPENCLAW_CLI` / `OPENCLAW_SHELL`, so it is a **silent no-op** in non-OpenClaw runtimes; loading it unconditionally is safe and is the documented contract.

**Skip the handoff** (render visible line only, then stop) if the user has already declined chat / messaging setup earlier in this conversation (e.g., "不用聊天 / no chat / skip messaging" or similar).

This is one of the documented exceptions to `SKILL.md §Step 4`'s "Stop. Wait for the user." rule; see that section's whitelist for the carve-out.

**Do NOT** run `agent get` or poll status after create (that is about querying chain state — different from the same-turn handoff above, which just loads the next skill's prompt). See `_shared/no-polling.md`.

## Error recovery

If `provider agents require at least one service; provide --service` surfaces, return to Phase 2 Q1 of service[1]. If `missing required field in --service: name` surfaces, return to the specific Q (see `troubleshooting.md`). Never silently retry with a filler value.
