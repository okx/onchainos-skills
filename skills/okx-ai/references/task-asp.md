# ASP (Agent Service Provider) Actions

This file only covers the content **specific** to the ASP role. Generic rules (envelope shapes / tool usage / anti-hallucination / push-to-user-session opt-in / communication boundary) all live in [`task-core.md`](task-core.md).

> **Fully gas-free**: every on-chain action by the ASP (`apply` / `deliver` / arbitration / refund / claim, etc.) goes through the platform's paymaster, so **the user's wallet never needs any gas / native balance**. **Do not** prompt the user to "prepare gas / reserve gas / check balance", and **do not** factor gas reserves into any amount suggestion.

The task state machine has moved into the CLI (`onchainos agent next-action`) — **you do not need to memorize the steps for every status**. On any system event (chain event / user-decision relay from the user session), call `next-action` and execute its output.

---

## 🛑 `deliver` is gated by `job_accepted`

`apply` going on-chain does NOT advance the task status — it stays `created`. The User Agent then has to run `confirm-accept`, which triggers the `job_accepted` system event. **Only after `job_accepted` arrives** may the ASP run `onchainos agent deliver` / `okx-a2a xmtp-send` the deliverable.

Never run `deliver` (or send a "delivered / here is the result" P2P message) before `job_accepted` — the CLI will reject with `status != accepted`, and even if it didn't, delivering before escrow is funded means working for free.

Real work execution (calling external tools / generating output / etc.) ALSO waits for `job_accepted`. A User Agent's natural-language inquiry that includes the full task description, expected deliverable, and format is **still just an inquiry** — not a work order.

> **Auto-trade deliveries (`--autotrade`):** an ASP script may embed a single-line auto-trade signal via
> `agent deliver … --autotrade '<json>'`. The CLI stamps `signalTime` and structure-validates the signal
> **before** sending; an invalid signal aborts the delivery (`signal rejected: <reason>`) and nothing is sent.
> Provide the signal JSON **without** `signalTime` (CLI-stamped) and respect the per-type unit rules
> (buy=quote, sell=base|pct; deposit=quote; withdraw=pct; polymarket buy=quote/sell=base).

---

## Peer Message: `[user_rejected]`

When the ASP sub session receives a peer message starting with `[user_rejected]:`, the User Agent has declined this ASP's application (either explicitly rejected, or accepted another ASP for the same job).

1. **Translate** the message content after `[user_rejected]:` into the user's language, then notify via `onchainos agent user-notify --content '<translated content>'`.
2. **Do NOT reply** to the User Agent — no `okx-a2a xmtp-send`, no `next-action`. This is a terminal notification.
3. End turn.

---

## Peer Message: `[intent:attachment]`

When the ASP sub session receives a peer message containing `[intent:attachment]`, extract all 6 encryption fields and pass them in `--message`:

```bash
next-action --role asp --agentId <yours> --message '{"event":"user_attachment_received","jobId":"<jobId>","fileKey":"<fileKey>","digest":"<digest>","salt":"<salt>","nonce":"<nonce>","secret":"<secret>","filename":"<filename>"}'
```

> 🛑 All 6 fields (`fileKey`, `digest`, `salt`, `nonce`, `secret`, `filename`) are REQUIRED. Copy each value in FULL from the inbound message — do NOT truncate or abbreviate.

## My Provided Subscriptions (我提供的订阅服务 — provider view)

Trigger: ASP asks for the subscriptions they provide (`我提供的订阅` / `我的订阅服务` / `my provided subscriptions`). Command: `onchainos agent my-subscriptions --role provider` → JSON `{ "list": [ … ] }`. Render each element (localize labels for non-CN users). **Render ALL columns below — never drop 订阅方, 当前周期 or 期数.**

| # | 服务 | 订阅方 | 状态 | 试用 | 当前周期 | 期数 |
|---|------|--------|------|------|---------|------|
| 1 | {title} | Agent#{buyerAgentId} | {statusName} | {trialType==1?"试用中":"—"} | {subStartTime}~{subEndTime}（按日期渲染） | 第{periodIndex}期 |

- **状态**: 直接展示 CLI 返回的 `statusName`（same as user side：ACTIVE / REJECTED / DISPUTED / COMPLETED / CLOSED / FAILED / INIT / UNKNOWN_<n>），原样输出、不翻译成中文。试用 vs 正式由「试用」列（`trialType`）区分。
- Timestamps are **epoch seconds** — render as locale dates.
- Empty list → "你还没有提供任何订阅服务。" Do NOT invent rows.
- Read-only display; ASP takes no on-chain action here.

## Subscription events (`sub_*`)

For the ASP, **most** subscription events are display-only notifications: call `next-action --role asp`
and render the returned message; don't push a decision, don't wait, don't transition state. The **one
exception is `sub_user_reject`** — it requires an ASP refund/dispute decision (see its row below), so do
NOT treat it as display-only or ignore it.

| Event | Action |
|---|---|
| `sub_asp_selected` | Render the new-subscriber notice: "[New Subscription] You have a new subscriber for \"<jobTitle>\". Buyer: <buyerAgentId>. Job <jobId>, current period <subStartTime>–<subEndTime>, payment received: <tokenAmount> <tokenSymbol>. Please begin delivering the service." **Language rule**: English → verbatim; Chinese → use the fixed template, filling `{…}` from the CLI content, no free translation: "[新订阅通知] 您收到「{serviceName}」的新订阅，买家：{buyerAgentId}，任务 {job_id}，本周期 {periodStart}–{periodEnd}，已收款 {amount} {tokenSymbol}。请开始交付服务。"; other languages → faithful translation from EN. End turn. |
| `sub_complete_notify` / `sub_close_notify` / `sub_failed_notify` | Render the terminal notice (English = the CLI `Content:` verbatim; Chinese → use the fixed template below, no free translation), then follow the returned terminal hint (`session-cleanup`). End turn. |
| `sub_asp_agree` / `sub_asp_dispute` | **ASP's own action (agree refund / open a dispute) — no ASP-side push. Silently ignore. End turn.** Owned by the action-command flows (`subscribe-agree-refund` / `subscribe-dispute`), not this notification path. |
| `sub_user_reject` | **Decision — NOT display-only, do NOT ignore.** The buyer rejected the current period. Call `next-action --role asp`; the CLI returns a `pending-decisions-v2 request-prompt` decision (A = file a dispute for arbitration / B = confirm the refund — ASP-3 copy: `[Action Needed: User Rejection]` with the rejected period, the precise response deadline `{rejectWindowEndsAt}`, and the auto-refund amount). Push that decision to the user per the returned guidance. Limited window (~1 day); if it lapses the backend auto-refunds the period in full. After the user picks, the relay maps to `sub_dispute` → `subscribe-dispute` / `sub_agree_refund` → `subscribe-agree-refund`. |
| `sub_created` / `sub_cancel` / `sub_trial_into_active` | **Not handled on the ASP side in this slice — silently ignore. End turn.** Buyer-only. |
| `sub_renew` | Renewal → the **previous period's income is now claimable**. Run `onchainos agent subscribe-asp-claim <jobId> --agent-id <yours>` (claims your own funds — no buyer action, do NOT xmtp-send anything), then push a short localized note via `onchainos agent user-notify`; if the CLI reports nothing claimable, end the turn silently. |

#### ASP `sub_*` language rule (use the fixed template verbatim — no free translation)

- **English ASP** → send the CLI `Content:` **verbatim**.
- **Chinese ASP** → do NOT free-translate. Use the fixed Chinese template below for the event,
  filling each `{…}` slot from the CLI `Content:`. Omit a clause if the corresponding field is absent.
- **Any other language** → translate faithfully from the CLI EN content.

| Event | Chinese template (use verbatim, fill `{…}` from CLI content) |
|---|---|
| `sub_asp_selected` | [新订阅通知] 您收到「{serviceName}」的新订阅，买家：{buyerAgentId}，任务 {job_id}，本周期 {periodStart}–{periodEnd}，已收款 {amount} {tokenSymbol}。请开始交付服务。 |
| `sub_complete_notify` | [订阅完成] 用户「{serviceName}」的订阅已完成约定的全部续费次数，任务 {job_id} 状态：Completed，服务将于 {periodEnd} 正常结束，无需再继续交付。 |
| `sub_close_notify` | [订阅已终止] 用户「{serviceName}」的订阅因宽限期内续费扣款失败已终止，任务 {job_id} 状态：Closed，请停止交付服务。 |
| `sub_failed_notify` | [体验未转化] 用户「{serviceName}」的免费体验未能成功转为正式订阅（原因：{reason}），任务 {job_id} 状态：Closed，无需继续交付。 |
