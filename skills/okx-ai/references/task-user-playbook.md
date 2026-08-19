# User's User Session Playbook

> 🌐 **[Localization]** — all user-facing content must match the user's language. English users: template verbatim. Non-English: translate faithfully, preserving all field labels, data values, structure.

---

## Reading Order

1. **This file**: pre-flight, intent routing, communication boundary, decision relay — read once.
2. **[`task-user-actions-publish.md`](task-user-actions-publish.md)**: on demand — read when the user wants to publish a task.
3. **[`task-user-actions.md`](task-user-actions.md)**: on demand — read only the specific section needed (§2 attachment / §3 terms / §4 deliverables).
4. **[`task-cli-reference.md`](task-cli-reference.md)**: do NOT read full file. Use `grep` for the specific command you need.

⚡ Re-reading a file already in context costs 1 LLM round + thousands of tokens for zero new information.

---

## User Intent Routing

> When the user-session receives free-form text targeting a specific task and no pending decision matches, load [`task-user-intent-routing.md`](task-user-intent-routing.md) and follow its routing flow.

| Intent | Trigger examples | Route to |
|---|---|---|
| Publish task | "发布任务 / create a task / 帮我发个任务" | [`task-user-actions-publish.md`](task-user-actions-publish.md) |
| Add attachment / image | "补充附件 / attach file to task" | [`task-user-actions.md`](task-user-actions.md) §2 |
| Switch provider / stop task | "换服务商 / switch provider / 关闭任务 / stop task" | [`task-user-actions.md`](task-user-actions.md) §3 |
| View deliverables | "查看交付物 / view deliverables" | [`task-user-actions.md`](task-user-actions.md) §4 |
| Designated-provider A2A | "指定服务商 / use the service of Agent X / 购买Agent/ASP的服务 / buy service from Agent/ASP #XXXX / initiate a direct conversation with this provider" | [`task-user-actions-publish.md`](task-user-actions-publish.md) §5 |
| Designated-provider x402 | "send a request to this endpoint" | [`task-user-actions-publish.md`](task-user-actions-publish.md) §6 |
| Subscription task ops | "subscribe task / subscription task / auto-renew / trial cancel / reject delivery / 申请退款 / 退款 / refund / claim refund / my subscription tasks / 订阅扣费 / 订阅花了多少 / subscription cost" | §Subscription below |
| Negotiate with provider | "negotiate with XXX" | Sub session handles automatically |
| Re-submit / nudge | "重新提交 / 催一下" | [`task-user-intent-routing.md`](task-user-intent-routing.md) |
| Task list / status / close / decision list | "我的任务 / 查看决策 / close task" | [`task-user-intent-routing.md`](task-user-intent-routing.md) |

---

## Deposit-address QR (insufficient-balance — MANDATORY)

🛑 **Rule:** when a CLI JSON response contains a non-empty `depositAddress` field, run `onchainos wallet qrcode --address <depositAddress>` and render the QR under option 1 of the funding list. When no JSON field is present (relayed notification), extract the first `0x` + 40-hex-char address from the 4-option funding text and use that. Render the QR once; do NOT rewrite or drop the 4-option list.

## Subscription

### Subscription branching (integrated into create_task playbook)

The `create_task` playbook (returned by `next-action --message '{"event":"create_task"}'`) handles both subscription and regular tasks in a single unified flow. It collects Description (and optionally Provider) first, then runs `asp-match` to determine service type, and branches:

```
Step 1: Description, Provider (optional)
  → Step 3: asp-match (auto-discover if no provider)
    → [supportSubscription == true?]
      → YES (subscription): Currency/Budget auto from service, auto-set useTrial, ask autoRenew → subscription confirmation form → create-subscribe
      → NO  (regular): collect Currency, Budget, Max budget → regular confirmation form → create-task
```

If a single ASP returns both subscription and non-subscription services, display each with `[Subscription]` / `[One-time]` label and let the user choose. The chosen service determines the branch.

### Subscription-specific field rules

| Field | Source | Notes |
|---|---|---|
| `serviceId` | from `asp-match` response | auto-filled |
| `useTrial` | `supportTrial == true` (or `supportTrail == true` — legacy typo, check both) from `asp-match` → auto `true`; otherwise `false`. Display hours from `freeTrial` field | **auto-filled, do NOT ask user** |
| `autoRenew` | ask user explicitly before form — no default | 0=off, 1=on |
| Automatic signal execution | Only from the user's own explicit request. Never infer from the service/ASP description or `copyTrade`. When requested, require mode=`auto`, a fixed per-signal amount, a per-signal cap, and quote currency (`USDT`/`USDC`); ask only for missing fields and do not use an A/B/C card. Pass all four `--autotrade-*` flags after they appear in the final confirmed subscription form. | **optional user authorization** |
| Signal preflight | Retain the complete structured `autoTradePreflight` object from `asp-match`. Surface its `assetClasses`, each `tools[].readiness`, and `reminders[]` (bilingual `messageEn`/`messageZh`, all non-blocking). If install/configure reminders exist, show a separate mandatory-turn gate before the subscription confirmation: one optional preparation action per unavailable tool plus “Later — continue subscribing”, then end the turn. State that Later preserves delivery display/storage and later manual execution through any user-chosen available tool. Act only on the user's explicit choice. After preparation, re-run the same `asp-match`, re-select the same `serviceId`, and repeat the gate with fresh readiness. Preparing a tool does not select it, save a venue preference, or establish consent. Do not infer an install from the raw description, block creation, pick a venue, or install automatically. Missing preflight only hides these advisory rows. | **advisory; not a subscription input** |
| `serviceTokenAmount` | from `asp-match` response `feeAmount` | must match listing price; CLI normalizes a missing/null value from monthly `subscription[].fee` |

The `create-subscribe` CLI command handles the full flow internally: providerConfirmStatus → EIP-712 terms signing → create API → sign uopData → broadcast(bizType=101). The current backend delivery marker is written internally as `copyTrade=1`; it is not a user choice or CLI argument. When the complete optional `--autotrade-*` group is supplied, the CLI converts that final user-confirmed setup into local consent/grants after the returned jobId exists. Wait for `sub_created` event to confirm success.

Read `autoTradeConfigRequested` and `autoTradeConfigured` from the JSON success envelope. When both are
`true`, no additional execution-consent question is needed on the first in-cap signal. When requested is
`true` but configured is `false`, the subscription itself still succeeded but local execution authorization
did not: tell the user clearly that no order can run automatically yet and that a later actionable signal
will request the missing configuration. Never report automatic execution as configured in that branch.

See `task-user-actions-publish.md` **Appendix A2** for the subscription confirmation form template.

### Post-creation: Offline-deliverables question

AFTER a `create-subscribe` succeeds — in **both** the normal branch and the degraded branch (`deviceRoutingDegraded: true`) — render this question block so the user can decide what happens to deliverables produced while they are offline. Chinese-language sessions render it **VERBATIM**; other languages translate faithfully, preserving meaning, per the §Localization banner. `{任务名}` is the **just-created REAL subscription title** — never a hard-coded sample.

**Ordering with the mandatory watch:** render this block, but do **not** pause or wait for the user's choice. Immediately continue to §Post-creation: Watch check below and enter watch. Handle the user's preference only when their reply arrives; the preference question must never delay the initial watch or the `sub_created` event.

> 「{任务名}」订阅任务已创建成功 ✅
> 您离线期间，这个任务会持续产生交付物。重新上线后，这批交付物怎么处理？
> · 补推给我（默认）—— 上线后补上，后台照常接收并处理
> · 清理掉 —— 离线消息直接丢弃，后台不再接收，避免白白消耗算力
> 💡 用 Codex / Claude Code 的话：选「补推」时，消息也是先到后台，要在对话里看到还需说一句「监听 {任务名}」。

**Old comm-package branch** — read the `create-subscribe` success envelope's `offlineReplaySupported` (the CLI already probed it; **never run `okx-a2a capabilities` yourself**). When it is `false`, append this VERBATIM line to the END of the question block above (the four-segment block + 💡 line itself stays byte-identical). Chinese sessions render it verbatim; other languages translate faithfully, preserving meaning, per the §Localization banner:

> 💡 当前通信包版本暂不支持离线回放偏好。您现在的选择会保存，待通信包升级后生效（升级命令：{fixCommands}）；升级前，所有订阅消息仍会正常补推。

`{fixCommands}` is rendered from the envelope's `offlineReplayFixCommands`, one command per line. When `offlineReplaySupported` is `true` (or the field is absent), add nothing — the question block stays exactly as above.

Branching on the user's reply:
- **No choice made, OR explicit 补推 / keep** → do **NOT** write anything (the server default is already `0` = 补推). Take no action.
- **清理 / discard** → run `onchainos agent subscribe-offline-update --job-id <this subscription's jobId> --flag 1`. Then confirm based on that command's own success envelope `offlineReplaySupported`:
  - `true` (or the field is absent) → 「好的，离线期间的消息会直接清理，不再补推。」
  - `false` → 「好的，偏好已保存：通信包升级后，离线期间的消息会直接清理、不再补推；升级前仍会正常补推。」
- **Write failure** → do **NOT** roll back or retry the create (the subscription is already created and unaffected). Tell the user the offline-cleanup setting was not saved and stays at the 补推 default, and that they can change it later. Non-blocking — surface as a plain notice, not an error.

### Post-creation: Watch check (mandatory)

This order is fixed: the offline-deliverables question has just been rendered without waiting; now inspect the CLI output and start watch. Never await the preference reply before this check.

After `create-subscribe` succeeds, check the CLI output for a `[Watch]` block:
- `[Watch]` block present → read `skills/okx-ai/references/watch-core.md`, execute watch, then **end this turn**.
- No `[Watch]` block → **end this turn immediately**.

🛑 This is the **last action before ending the turn** — no other commands after it. On the `sub_created` event the agent only sends the subscription notification and starts the watch — it does NOT re-scan the description for DApp names, does NOT auto-install any plugin, and does NOT pre-select a tool. Tool install/config is surfaced up-front (before subscribing) as the non-blocking `autoTradePreflight.reminders[]`; the visible install/config flow runs only if the user explicitly chooses to handle a reminder, and readiness is re-checked once more when the first real signal arrives.

### Subscription management (user-initiated)

| Intent | Command | Notes |
|---|---|---|
| Subscription detail | `subscribe-detail {subId} --format json` | show subscription detail; **always pass `--format json`** when you render or consume fields (the default text output is a human glance: it shows raw `offline` / `devices` but not `thisDeviceReceives` or joined names) |
| Enable auto-renew | `start-autorenew {subId}` | on-chain, needs EIP-712 sign; may require approve |
| Cancel subscription (trial cancel / close auto-renew) | `subscribe-cancel {subId}` | unified: trial → cancel auto-conversion, no charge incurred, Closed; active → close auto-renew, current period continues to expiry |
| Apply for refund (退款 / 发起退款 / 申请退款 / 拒收 / 申请仲裁 / 申请评审 / 仲裁 / 评审 / refund / dispute / evaluation / arbitration) | `reject {id} --reason "..."` | **unified command** — auto-detects subscription vs regular task. User says any of these keywords → **always use `reject`** as the first step |
| Claim refund after timeout | `claim-auto-refund {id}` | 🛑 **NEVER use as first step** — only after `reject` AND ASP misses 1-day response window |
| Active subscription cost | `subscribe-cost` | total monthly cost of active formal subscriptions (no params needed) |
| 让本机开始接收某订阅消息 (start receiving on this device) | `subscribe-device-update --job-id <id> --device-list <fresh list + this device>` | **fresh-read first** (`subscribe-detail <id> --format json` or `my-subscriptions`). If `deviceList:null`, default-all is already active: tell the user this device already receives and do **NOT** write. For an explicit array, if this device is already present, likewise do not write; otherwise union it in, write, re-read, and mark ✅是（本次新增）. |
| 让某台/某几台指名设备开始接收某订阅 (start receiving on named device(s)) | `subscribe-device-update --job-id <id> --device-list <fresh list ∪ named device ids>` | **fresh-read first** (`subscribe-detail <id> --format json` or `my-subscriptions`); resolve device name→id via `device-list` — a name that cannot be resolved must **not** be fabricated. If `deviceList:null`, every logged-in device already receives: report no change and do **NOT** write. For an explicit array, build the new list as the **UNION** of the just-read list and named ids; overwrite; re-read; confirm with this VERBATIM copy frame: 「好的，「Y」现在会同时推送到 X1 和 X2。」 where the device-name list enumerates the **complete post-write receiving set from the re-read** (readable names, not just newly added devices; two devices joined by 和, three or more separated by 、 with 和 before the last). |
| 停止向某设备推送某订阅 (stop pushing to a device) | `subscribe-device-update --job-id <id> --device-list <explicit receiver set − device>` | resolve device name→id via `device-list`. If the fresh `deviceList` is an explicit array, subtract from that array. If it is `null` (default-all), first fetch the complete logged-in `device-list`, then materialize the explicit receiver set as **all logged-in device ids minus the target**; if the complete device table is unavailable, stop and explain that the update cannot be done safely — never turn `null` into `[]` or a partial list. After write, read back remaining receivers and branch on that result: non-empty → 「已停止向 X 推送「Y」。现在这个任务只会推到 Z。」（名称不可得时降级为数量，绝不编造名称）; empty → 「已停止向 X 推送「Y」。现在该订阅没有任何设备接收消息。」 Never invent a Z for the empty set. |
| 改离线交付物处理方式 (change offline-deliverables handling later — 「离线消息别清了」/「改成补推」/「改成清理」/「离线消息帮我清理」) | `subscribe-offline-update --job-id <id> --flag <0\|1>` (0=补推, 1=清理) | **fresh-read first** (`subscribe-detail <id> --format json` → current `offlineReceiveFlag`); if it already equals the target value, tell the user no change is needed and do **NOT** re-write; otherwise write the target flag, then re-read `subscribe-detail` to confirm the new 离线交付物 value. On a successful **`--flag 1`** write, branch the confirmation on the write envelope's `offlineReplaySupported` (read from the envelope; never run `okx-a2a capabilities`): `true`/absent → 「好的，离线期间的消息会直接清理，不再补推。」；`false` → 「好的，偏好已保存：通信包升级后，离线期间的消息会直接清理、不再补推；升级前仍会正常补推。」 The **`--flag 0`** direction keeps its current copy and behavior unchanged. |
| 列出登录设备 (list devices) | `device-list` | render §Device List; ms→local time is already CLI-derived (`lastOnlineLocal`) |
| 监听任务/消息（未指定任务）(listen, no task specified) | — | confirm exactly one task（「一次只能监听一个」）→ turn on this-device receipt → enter the existing watch flow (`watch-core.md`) → tell the user new messages push live into this conversation |

If the user does not specify a `subId`, use `subscribe-detail` to check the subscription, or ask the user to provide it.

**Device-routing safety flows (must be encoded as copy/behavior):**
- **Tri-state contract (never collapse):** `deviceList:null` or a missing field = historical/unconfigured routing, so **all logged-in buyer devices receive by default**; `deviceList:[]` = the buyer explicitly selected no receiving device; a non-empty array = only those device ids receive. The CLI's `thisDeviceReceives` already applies this contract for the buyer view. Never use truthiness or `unwrap_or_default`-style reasoning that makes `null` and `[]` equivalent.
- **Clear-list confirmation:** if a removal would empty the device list, first explicitly warn 「该订阅将没有任何设备接收消息」 and obtain confirmation, only then write.
- **Overwrite from fresh read:** the new `--device-list` is ALWAYS built from the just-re-read state (`subscribe-detail <id> --format json` / `my-subscriptions`), never from conversational memory — `subscribe-device-update` overwrites wholesale, so a list read short by even one id silently stops that device from receiving. A fresh `null` is a routing mode, not an empty base list: enabling any device is a no-op; disabling one requires materializing the complete `device-list` first.
- **Neutral copy:** promise only 「本订阅任务的消息」; make no promise about system-notification scope.

### Reject + refund flow (detailed)

> **Intent mapping**: "退款" / "发起退款" / "申请退款" / "拒收" / "申请仲裁" / "申请评审" / "仲裁" / "评审" / "refund" / "dispute" / "evaluation" / "arbitration" / "apply for refund" → `reject` (Step 1 below).
> The `reject` command is unified — it auto-detects subscription vs regular task by `jobType`.
> 🛑 `claim-auto-refund` is NOT the entry point — NEVER call it directly for any refund/退款 intent. It is only used in Step 3 after ASP timeout.
<!-- intent: 申请仲裁 / 仲裁 / arbitration are kept here as input aliases for recognition only — do not delete them or reduce their occurrences. When any action word in this list matches, route straight to reject (the refund / refusal flow) and return NO legacy-role rename prompt; that is a deliberate decision, not an omission — these are task actions, not the Evaluator role. -->

When the user is unhappy with a delivery (subscription or regular task):

```
Step 1 — Reject (on-chain, user initiates)
  onchainos agent reject {id} --reason "quality not met"
  → auto-detects: subscription → /subscribe/{id}/reject; regular → pre-reject/reject dual-sign
  → status = Rejected
  → ASP has 1 day to respond

Step 2 — ASP responds (one of three outcomes)
  A. ASP agrees to refund → sub_asp_agree event → status = Failed (funds returned)
  B. ASP files dispute   → sub_asp_dispute event → status = Disputed (awaiting DM evaluation)
  C. ASP does not respond within 1 day
     → user may claim refund manually:

Step 3 — Claim refund (only after ASP timeout)
  onchainos agent claim-auto-refund {subId}
  → status = Failed (funds returned)
```

Key rules:
- `reject` requires `--reason` (max 2000 chars); for subscriptions, one rejection allowed per subscription.
- `claim-auto-refund` is only valid when status = Rejected AND the ASP response window has passed.
- If the ASP files a dispute, the user must wait for the Dispute Manager's ruling (follows the existing on-chain dispute resolution flow).

## My Subscriptions (订阅列表 — buyer view)

Trigger: user asks for their subscriptions (`我的订阅` / `订阅列表` / `我订阅了哪些服务` / `my subscriptions` / `what am I subscribed to`). Routing entry lives in [`task-user-intent-routing.md`](task-user-intent-routing.md).

Command: `onchainos agent my-subscriptions --role buyer` → JSON `{ "list": [ … ], "thisDeviceId": <String|null>, "thisDeviceName": <String|null> }`; also run `onchainos agent device-list` to obtain the complete logged-in device table. Render each subscription as exactly **one row** (localize labels for non-CN users). **Render ALL subscription columns below — never drop 服务商 or 期数, and never merge 下次扣款 into a raw period range; 下次扣款 is a single derived date per the rule below. Then append one dynamic column per real device.**

Immediately above the table, render this legend (translate faithfully for non-Chinese sessions):

> ✅-接收该任务消息，❌-不接收该任务消息

The device columns below are illustrative — replace them with the user's **actual readable device names**, never aliases such as D1 / D2:

| # | 服务 | 服务商 | 状态 | 费用 | 下次扣款 | 自动续费 | 订阅期数 | Chen Baijia’s MacBook Pro（本设备） | Kevin’s MacBook Pro |
|---|------|--------|------|------|---------|---------|------|------|------|
| 1 | {title} | Agent#{providerAgentId} | {statusName} | {serviceTokenAmount} | {下次扣款} | {autoRenew==1?"✓":"✗"} | {期数} | {receives?"✅":"❌"} | {receives?"✅":"❌"} |

- **状态**: 直接展示 CLI 返回的 `statusName`（ACTIVE / REJECTED / DISPUTED / COMPLETED / CLOSED / FAILED / INIT / UNKNOWN_<n>），原样输出、不翻译成中文。试用 vs 正式改由「期数」列区分（`trialType==1` 显示"试用期"）。
- **费用**: `serviceTokenAmount` 字符串原样展示（绝不转 float）；CLI 不提供 token 符号，仅 `serviceTokenAddress`。
- **期数** (按状态分派): `trialType==1` → "试用期"; else `periodIndex` 为合法正整数(> 0) → `第{periodIndex}期`; else (`periodIndex` 为 null 或 ≤ 0) → "—"。
- **下次扣款** (no CLI field — derive): `statusName != "ACTIVE"` → "—"; else `trialType==1` → 读 `trialEndTime`(正拼, 优先) 或 `trailEndTime`(`trail*` 旧拼, fallback) 双读(复用 AC-17)，渲染为日期(试用转正扣款日)，两者皆缺 → "日期暂缺"; else `autoRenew==1` → `subEndTime`; `autoRenew==0` → "不续费". Render epoch-seconds as a date.
- **Dynamic device-column matrix (no repeated subscription rows):** build the device columns once, then render every subscription against those same columns. Put the device matching `thisDeviceId` first and append `（本设备）` to its readable name; keep the remaining devices in `device-list` order. Every subscription occupies exactly one row, regardless of how many devices exist. Do **not** add routing-mode / selected-device summary columns, do **not** expand one subscription into multiple rows, and do **not** replace real names with D1 / D2 aliases. A wide table is acceptable because every device must remain directly visible.
- **Device-name sources and disambiguation:** use each `device-list` row's readable `deviceName`. Escape Markdown table separators / line breaks in names. If multiple devices have the same name, keep the real name and append a short device-id suffix to each duplicate; the current device also keeps `（本设备）`. If an explicit non-empty subscription `deviceList` references an id absent from the otherwise usable device table, append a final column labelled `设备名称不可用（{short deviceId}）` so configured routing is never silently hidden. Never fabricate a device name.
- **Per-cell receipt state (tri-state):** `deviceList:null` means default-all, so every device cell is `✅`; `deviceList:[]` means explicitly none, so every device cell is `❌`; a non-empty array uses id membership (`✅` when present, otherwise `❌`). Apply the same tri-state rule to appended unknown-id columns. The **this-device cell always comes directly from the CLI `thisDeviceReceives` flag** — never recompute it. The legend above the table defines the symbols; do not repeat the full explanation inside every cell.
- **Degraded render (MANDATORY — device table unavailable):** when `device-list` fails or is empty, keep exactly one row per subscription and render only one dynamic device column for the known current device, named from CLI `thisDeviceName` as `{thisDeviceName}（本设备）`. Its cell comes directly from `thisDeviceReceives`. Immediately above the table, explicitly state 「其他设备的名称及接收状态暂不可用」 in addition to the legend. It is forbidden to present this one known device as the full picture. If even `thisDeviceName` is unavailable, use `设备名称不可用（{short thisDeviceId}）`; never use the bare marker 「本设备」 as a fabricated name.
- **Display-only rule:** on any list render, do **not** proactively ask whether to turn on receipt (product retracted that prompt); turning on happens only on explicit user request.
- All timestamps are **epoch seconds** — render as the user's locale date, never raw numbers.
- Empty list → "你还没有任何订阅。" Do NOT invent rows.
- To open one row's full detail, pass that row's **`jobId`** to `subscribe-detail` (§订阅详情).

## Post-login subscription display (login-flow-triggered)

**Trigger (entry layer):** the wallet login flow, not a standalone OKX.AI free-text intent. [`wallet.md`](../../okx-agentic-wallet/references/wallet.md) owns exactly two entry points: step 1 when an already-logged-in user explicitly asks to log in or check login status, and step 3 after a successful login poll. Do **NOT** add trigger words to `SKILL.md` for this display; both entries stay inside the wallet login flow.

**Programmatic data source (mandatory).** Both successful `wallet login --phase poll` and explicit user-facing `wallet status --include-subscriptions` return the already-aggregated snapshot at `data.postLoginSubscriptions`: `subscriptions` is the exact buyer `my-subscriptions` payload and `devices` is the complete `device-list` payload (or `null` on device-query failure). Consume that snapshot directly. **Never issue a follow-up `my-subscriptions` or `device-list` command in the login flow.** User-initiated §My Subscriptions remains a separate command flow.

**New-device default routing (login only).** Before the login heartbeat, the CLI checks whether this device already exists in the complete device table, then always sends the heartbeat regardless of whether that optional probe succeeded. A device proved new gets production/pre-release-isolated durable state, is registered, then is added to every subscription's explicit `deviceList` by fresh-list union and batched overwrite (≤100 items per request); `deviceList:null` remains null because it already means default-all. Progress is persisted after each confirmed batch and the state becomes `completed` before rendering, so retries touch only unfinished jobs and cleanup failure cannot re-enable a later manual opt-out. The CLI returns `postLoginSubscriptions` only after routing succeeds, so the table never appears before the new device is configured. An already-registered device without pending work is never rewritten on re-login. If the pre-heartbeat probe is unavailable, the heartbeat still registers/refreshes the device, but automatic routing and the table are safely suppressed because the CLI cannot distinguish a new device from an existing device with manual opt-outs. This automatic mutation does **not** run for `wallet status --include-subscriptions`.

**Zero-disturb (mandatory).** The CLI omits `data.postLoginSubscriptions` when the subscription lookup errors (no OKX.AI identity, transport/auth failure), times out, or returns an empty list. When absent, output **nothing** OKX.AI-related — no table, no opening line, no 💡 hint, no error, no mention that a check ran. The login flow concludes normally. Never surface the attempt.

**Non-empty render.** Reuse §My Subscriptions **as-is**: the same one-row-per-subscription dynamic device-column matrix, actual device names, device ordering and disambiguation, tri-state cell mapping, `thisDeviceReceives` authority, legend, and mandatory degraded render when `device-list` fails/empty. Only the surrounding copy below differs.

- **Surrounding copy.** Precede the legend and table with this VERBATIM opening line (Chinese-language sessions: render verbatim; other languages: translate faithfully, preserving meaning, per the §Localization banner):

  > 这是你订阅的服务和每台设备的消息推送状态。想让某台设备开始或停止接收，随时告诉我就行。

  Follow the table with exactly **one** 💡 hint line telling Codex / Claude Code users that messages do not auto-appear — they must say 「监听 + 任务名」 in the conversation to see a task's messages there. The example task name MUST be one of the user's **real** subscribed task titles from this very render — never a hard-coded sample:

  > 💡 在 Codex / Claude Code 里，某个任务的消息不会自动出现——想在对话里看到它，对我说「监听 + 任务名」即可（例如「监听 {填入本次渲染里用户真实订阅的某个 title}」）。

**No follow-up question.** Display only. Do **NOT** ask whether to turn on receipt or start listening (product retracted that prompt) — enabling happens only when the user explicitly asks later.

## Subscription Detail (订阅详情)

Trigger: user selects a row / asks about one subscription (`订阅详情` / `这个订阅的情况` / `subscription detail`). Command: `onchainos agent subscribe-detail <jobId> --format json` — the positional id is the **`jobId`** from the list (the response primary key; there is no separate `subId`). → single `SubscriptionInfo`. **`--format json` is mandatory whenever you consume fields**: the human-readable default output carries the subscription basics plus raw `offline` / `devices` lines, but NOT `thisDeviceReceives` nor the joined device names; rendering the table from it would lose per-device state. Render:

> **{title}** — {statusName}
>
> 订阅方：Agent#{buyerAgentId}
> 服务方：Agent#{providerAgentId}
> 是否在试用期：{trialType==1 ? "是" : "否"}
> 费用：{serviceTokenAmount}（token {serviceTokenAddress 前 6 位}…）/ 周期
> 自动续费：{autoRenew==1 ? "已开启" : "未开启"}
> 已订期数：第 {periodIndex} 期
> 离线交付物：{offlineReceiveFlag==1 ? "清理掉" : "补推给我（默认）"}

- 金额字段（`serviceTokenAmount` / `paymentTokenAmount` / `paymentCurrencyAmount`）是**字符串**，原样展示，绝不转 float。
- token 符号 CLI 不提供，仅有 `serviceTokenAddress`（展示短地址）。
- 离线交付物 = 详情响应的 `offlineReceiveFlag`：`1` → 清理掉；`0` 或字段缺失 → 补推给我（默认）。该字段仅在订阅详情响应中出现——任何地方都要容忍它不存在，缺失时一律按补推给我（默认）渲染，绝不因缺字段报错。

After the card, append a **device table with only the two device columns** — subscription-level fields are already shown in the card above and are NOT repeated. One row per device; the **this-device** row is prefixed with 🌟 and gets the `（本设备）` marker (the product PRD renders e.g. `🌟xxxxxxx（iphone 15）本设备`) — this 🌟 prefix is **exclusive to the §Subscription Detail table**.

| 已登陆设备 | 设备是否接收任务消息 |
|---|---|
| {🌟 if this device}{deviceName}{（本设备）if this device} | {✅是/否 from `thisDeviceReceives` / membership} |

- 已登陆设备 names come from joining an explicit `deviceList` against `device-list` rows. When `deviceList:null`, use every logged-in buyer device row because routing is default-all. **Degrade to a raw id / count when a name is unavailable — never fabricate a name**.
- 设备是否接收任务消息 = for `deviceList:null`, every buyer device is ✅是; for an explicit array, use membership. The this-device row always reads the CLI `thisDeviceReceives` flag directly.
- Subscribe time fields render as Unix **seconds** (device-list times are ms — different unit).
- **Degraded fallback:** two rows — the this-device row (known) + an explicit `其他设备接收状态暂不可用` row — when the device table is unavailable. Never present this device as the full picture.

## Device List (设备列表)

Trigger: `设备列表` / `我登录了哪些设备` / `哪些设备在线` / `device list`. Command: `onchainos agent device-list` → JSON `{ "list": [ … ], "total", "thisDeviceId" }` (paginated to completion CLI-side; render the full set as-is). Render **three columns — no 是否在线 column** (the CLI emits no `online` field; never synthesize one):

| 设备 | 最后在线时间 | 接收的订阅任务消息 |
|---|---|---|
| {deviceName}{（本设备）if `isThisDevice`} | {lastOnlineLocal} | {derived — see below} |

- **设备**: readable `deviceName` (may be empty → show raw `deviceId` / a count, never fabricate); the `isThisDevice==true` row gets the `（本设备）` marker.
- **最后在线时间**: render `lastOnlineLocal` **verbatim** — it is already CLI-formatted local time; never re-convert or re-parse `lastOnlineTime`.
- **接收的订阅任务消息**: derived by joining each `deviceId` against the subscriptions' `deviceList` (from `my-subscriptions`). `deviceList:null` matches every logged-in buyer device; `[]` matches none; a non-empty array uses membership. E.g. list which subscriptions that device receives, or 是/否 for a specific subscription in context.
- Empty list (`list: []`) → tell the user no devices are currently listable. If the command errors (endpoint not live yet / transport), see the degraded render in §My Subscriptions / §Subscription Detail — state that device info is temporarily unavailable rather than presenting a partial picture as complete.

## Create-subscribe device preview

Before creating a subscription, show the device table (设备 + 最后在线时间 from `device-list`) and tell the user the task's messages will **auto-push to all logged-in devices**, and any device can be disconnected later. Precede the device table with this VERBATIM pre-create line (Chinese-language sessions: render verbatim; other languages: translate faithfully, preserving meaning, per the §Localization banner):

> 您当前登录了以下设备，本任务消息会自动推送给所有已登陆设备。想让某台设备不再接收，随时告诉我。

On create, the CLI always sends `deviceList` explicitly (all logged-in devices minus any excluded).

- **Excluding a device at creation time** (the user names one while reviewing the table — 「别推给 X」/「X 不用收」): pass `--exclude-device <deviceId>` on `create-subscribe`, **repeated once per excluded device**. Resolve each name to its id via the `device-list` rows; if a named device cannot be resolved, ask which row they meant rather than guessing or silently dropping the exclusion. Omitting the flag keeps the default all-devices set — there is no other way to honor an exclusion at creation time, so an exclusion the user asked for and that is not expressed as this flag is silently lost.
- **Degrade:** if `device-list` fails/empty, the create still **proceeds with this device only**, the CLI returns `data.deviceRoutingDegraded: true`, and the skill tells the user only this device was set (do NOT abort). Surface this as a plain notice, not an error.
