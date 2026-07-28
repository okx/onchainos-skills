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
| `copyTrade` | parse `serviceDescription` for actionable trading signal indicators (buy/sell direction, entry price, TP/SL, position size); if eligible → **ask user explicitly** "Enable auto copy-trade? (yes/no)", yes → 1, no → 0; if not eligible → 0 (skip the question) | **must ask user when eligible** |
| `serviceTokenAmount` | from `asp-match` response `feeAmount` | must match listing price |

The `create-subscribe` CLI command handles the full flow internally: providerConfirmStatus → EIP-712 terms signing → create API → sign uopData → broadcast(bizType=101). Wait for `sub_created` event to confirm success.

See `task-user-actions-publish.md` **Appendix A2** for the subscription confirmation form template.

### Post-creation: Watch check (mandatory)

After `create-subscribe` succeeds, check the CLI output for a `[Watch]` block:
- `[Watch]` block present → read `skills/okx-ai/references/watch-core.md`, execute watch, then **end this turn**.
- No `[Watch]` block → **end this turn immediately**.

🛑 This is the **last action before ending the turn** — no other commands after it. DApp plugin pre-install is handled later when the `sub_created` event arrives.

### Subscription management (user-initiated)

| Intent | Command | Notes |
|---|---|---|
| Subscription detail | `subscribe-detail {subId}` | show subscription detail |
| Enable auto-renew | `start-autorenew {subId}` | on-chain, needs EIP-712 sign; may require approve |
| Cancel subscription (trial cancel / close auto-renew) | `subscribe-cancel {subId}` | unified: trial → cancel auto-conversion, no charge incurred, Closed; active → close auto-renew, current period continues to expiry |
| Apply for refund (退款 / 发起退款 / 申请退款 / 拒收 / 申请仲裁 / 仲裁 / refund / dispute) | `reject {id} --reason "..."` | **unified command** — auto-detects subscription vs regular task. User says any of these keywords → **always use `reject`** as the first step |
| Claim refund after timeout | `claim-auto-refund {id}` | 🛑 **NEVER use as first step** — only after `reject` AND ASP misses 1-day response window |
| Active subscription cost | `subscribe-cost` | total monthly cost of active formal subscriptions (no params needed) |

If the user does not specify a `subId`, use `subscribe-detail` to check the subscription, or ask the user to provide it.

### Reject + refund flow (detailed)

> **Intent mapping**: "退款" / "发起退款" / "申请退款" / "拒收" / "申请仲裁" / "仲裁" / "refund" / "dispute" / "apply for refund" → `reject` (Step 1 below).
> The `reject` command is unified — it auto-detects subscription vs regular task by `jobType`.
> 🛑 `claim-auto-refund` is NOT the entry point — NEVER call it directly for any refund/退款 intent. It is only used in Step 3 after ASP timeout.

When the user is unhappy with a delivery (subscription or regular task):

```
Step 1 — Reject (on-chain, user initiates)
  onchainos agent reject {id} --reason "quality not met"
  → auto-detects: subscription → /subscribe/{id}/reject; regular → pre-reject/reject dual-sign
  → status = Rejected
  → ASP has 1 day to respond

Step 2 — ASP responds (one of three outcomes)
  A. ASP agrees to refund → sub_asp_agree event → status = Failed (funds returned)
  B. ASP files dispute   → sub_asp_dispute event → status = Disputed (awaiting DM arbitration)
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

Command: `onchainos agent my-subscriptions --role buyer` → JSON `{ "list": [ … ] }`. Render each element as one row (localize labels for non-CN users). **Render ALL columns below — never drop 服务商 or 期数, and never merge 下次扣款 into a raw period range; 下次扣款 is a single derived date per the rule below.**

| # | 服务 | 服务商 | 状态 | 试用 | 费用 | 下次扣款 | 自动续费 | 期数 |
|---|------|--------|------|------|------|---------|---------|------|
| 1 | {title} | Agent#{providerAgentId} | {statusName} | {trialType==1?"试用中":"—"} | {serviceTokenAmount} | {下次扣款} | {autoRenew==1?"✓":"✗"} | 第{periodIndex}期 |

- **状态**: 直接展示 CLI 返回的 `statusName`（ACTIVE / REJECTED / DISPUTED / COMPLETED / CLOSED / FAILED / INIT / UNKNOWN_<n>），原样输出、不翻译成中文。试用 vs 正式由独立「试用」列（`trialType`）区分。
- **费用**: `serviceTokenAmount` 字符串原样展示（绝不转 float）；CLI 不提供 token 符号，仅 `serviceTokenAddress`。
- **期数**: `第{periodIndex}期`（已订阅期数）。
- **下次扣款** (no CLI field — derive): `trialType==1` → `subStartTime`(试用转正扣款日); else `autoRenew==1` → `subEndTime`; `autoRenew==0` → "不续费". Render epoch-seconds as a date.
- All timestamps are **epoch seconds** — render as the user's locale date, never raw numbers.
- Empty list → "你还没有任何订阅。" Do NOT invent rows.
- To open one row's full detail, pass that row's **`jobId`** to `subscribe-detail` (§订阅详情).

## Subscription Detail (订阅详情)

Trigger: user selects a row / asks about one subscription (`订阅详情` / `这个订阅的情况` / `subscription detail`). Command: `onchainos agent subscribe-detail <jobId>` — the positional id is the **`jobId`** from the list (the response primary key; there is no separate `subId`). → single `SubscriptionInfo`. Render:

> **{title}** — {statusName}
>
> 订阅方：Agent#{buyerAgentId}
> 服务方：Agent#{providerAgentId}
> 是否在试用期：{trialType==1 ? "是" : "否"}
> 费用：{serviceTokenAmount}（token {serviceTokenAddress 前 6 位}…）/ 周期
> 自动续费：{autoRenew==1 ? "已开启" : "未开启"}
> 已订期数：第 {periodIndex} 期

- 金额字段（`serviceTokenAmount` / `paymentTokenAmount` / `paymentCurrencyAmount`）是**字符串**，原样展示，绝不转 float。
- token 符号 CLI 不提供，仅有 `serviceTokenAddress`（展示短地址）。
