# Subscription Notification Copy — Human-Readable Mirror (NOT runtime)

> **Canonical source = the CLI** (`cli/src/commands/agent_commerce/task/user/content.rs` and the
> ASP-side content module): the text a user sees is whatever `next-action` returns in its action
> result. This file is a **review/debug/localization reference only** — it is not
> loaded by any activation flow, and it must never be used to hand-compose a notification
> (see [`task-core.md` §Activation](task-core.md#activation): `sub_*` events always run
> `next-action`; User-side supplements live in
> [`task-user-sub-playbook.md`](task-user-sub-playbook.md)).
> If this table and the CLI disagree, the CLI is right and this file is stale.

| # | Event (`event`) | Target | Rendered notification (English canonical, from the authoritative copy doc) |
|---|---|---|---|
| 0 | `sub_open` | user | Create-and-fund confirmation sent to both Buyer and ASP. `trialType=1` → waiting for ASP acceptance; the free trial has not started. Otherwise → amount funded, waiting for ASP acceptance; explicitly state that the subscription is not Active yet. |
| 1 | `sub_created` | user | ASP acceptance notice. trialType=1 → "[Trial Subscription Accepted] The ASP accepted the subscription; your free trial is active ({trialStartTime}–{trialEndTime}). After it ends, {tokenAmount} {tokenSymbol} will be auto-charged on {trialEndTime} to convert to a paid subscription (attempted once, within the final hour before the trial ends — it will not retry if missed)." (trial is charge-free — never the first-charge copy). Otherwise → "[Subscription Accepted] The ASP accepted Job {jobId} (subscribing to {jobTitle}); the subscription is Active and service has started, current period {subStartTime}–{subEndTime}. First charge of {tokenAmount} {tokenSymbol} completed. Auto-renew is on; next charge date: {subEndTime}." (nextChargeAt = periodEnd = subEndTime; next-charge clause omitted only if subEndTime absent.) |
| 2 | `sub_asp_selected` | asp | Subscription acceptance notification (called `sub_accepted` in Lark §1.5; backend event is `sub_asp_selected`): "[New Subscription] You have a new subscriber for {jobTitle}. Buyer: {buyerAgentId}. Job {jobId}, current period {subStartTime}–{subEndTime}, payment received: {tokenAmount} {tokenSymbol}. Please begin delivering the service." After notifying the ASP owner, start the existing Service AI/Skill workflow. |
| 3 | `sub_cancel` | user | Branches on `trialType`; both are NON-terminal because the current trial/period continues. `trialType=1` (trial conversion cancellation) → "[Cancelled] Auto-conversion for the {jobTitle} free trial has been cancelled. This trial continues unaffected until {trialEndTime}; no charge will occur after it ends." `trialType=0` / absent (formal-period cancellation) → "[Auto-Renew Cancelled] Auto-renew for {jobTitle} has been cancelled. Current service continues until {subEndTime}; job {jobId} will then move to Completed." On `cancelResult=fail` (either branch) show `failReason` verbatim. Never emit terminal cleanup from `sub_cancel`. |
| 4 | `sub_trial_into_active` | user | "[Trial Converted] Your free trial has ended; the first charge of {tokenAmount} {tokenSymbol} for {jobTitle} is complete, current period {subStartTime}–{subEndTime}. Job {jobId} status: Active. Next charge date: {subEndTime}." (period range + nextChargeAt = subEndTime; both omitted if the period fields are absent.) |
| 5 | `sub_renew` | user | `renewResult=success` → "[Renewed] {jobTitle} — this cycle's renewal of {tokenAmount} {tokenSymbol} is complete. Job {jobId} status: Active. Next charge date: {subEndTime}." (the period range is intentionally NOT repeated — renewal keeps the same billing cycle; nextChargeAt = subEndTime, omitted if absent.) `renewResult=fail` → "[⚠️ Renewal Failed] {jobTitle} — this cycle's charge failed: {failReason}. A grace period is in effect (until {subBufferEndTime}); service continues normally and the system will keep retrying. Please add funding / increase allowance as soon as possible." (NON-terminal either way: a failed renewal only enters the grace period; later close/failure events still require their own fresh role/status and settlement handling.) |
| 6 | `sub_user_reject` | user | "[Rejection Submitted] Your rejection for {jobTitle}'s current period ({subStartTime}–{subEndTime}) has been submitted. The ASP must respond, or a full refund of {tokenAmount} {tokenSymbol} will be issued automatically." (`rejectWindowEndsAt` response-deadline clause if present — not in the event body.) |
| 7 | `sub_asp_agree` | user | This legacy event may describe the ASP-agree branch but cannot create refund proof. Render the completed full-original-token refund copy and terminal hint only when a durable local Refund V2 `request-refund` record binds the same job, Buyer, formal `jobType=1` subscription, exact positive original amount, and token address, and fresh composed detail proves Buyer ownership and `FAILED(9)`. Event-only or bare `FAILED(9)` stays `[Refund Settlement Detail Incomplete]`. Provider/Service, period, token-symbol, and `paymentMode` mismatches veto only when both sides exist; missing values reduce display/detail but do not invalidate the core binding. Tx Hash is optional, and no `refundTxHash` / `settlementTxHash` field is required. |
| 8 | `sub_asp_dispute` | user | "[Dispute Filed] The ASP has disputed your rejection of {jobTitle}'s current period ({subStartTime}–{subEndTime}) and escalated to evaluation. Job {jobId} status: Disputed." (user side only — the ASP's own action gets no ASP-side push; the subscribe-dispute action flow owns that lifecycle.) |
| 9 | `sub_complete_notify` | user + asp | "[Subscription Complete] {jobTitle} has completed all scheduled renewals. Job {jobId} status: Completed; service ends normally at {subEndTime} with no further renewal." (user side; the ASP side keeps its own ASP-perspective copy.) (terminal) |
| 10 | `sub_close_notify` | user + asp | Require fresh role ownership and Closed(7). Without `aspRejectReason`: "[Service Closed] "{jobTitle}"'s current period ({subStartTime}–{subEndTime}) has ended. Job {jobId} status: Closed." With `aspRejectReason`: identify the pre-activation ASP-decline branch and show the reason verbatim; state that the closure notice alone does not prove a subscription refund transition. Never reuse the renewal-charge-failure copy for this branch. (period range omitted when absent; user and ASP use their own perspective; no auto-rating.) ASP delivery may clean up after its assignment ends. Buyer handling emits no refund-terminal marker because subscription Closed(7) does not establish refund settlement; only matching durable local `request-refund` provenance plus a later fresh refund terminal can do so. |
| 11 | `sub_failed_notify` | user + asp | The legacy copy describes trial-conversion or renewal-charge failure, but the current caller-supplied/replayable event plus overloaded Failed(9) does not provide trustworthy cause provenance. Fail closed even when no matching durable local `request-refund` intent exists: show `[Refund Settlement Detail Incomplete]`, emit no `[Trial Ended]` / `[Subscription Ended]` terminal claim or terminal marker, perform no Buyer/ASP cleanup, and retain only read-only reconciliation. Matching refund intent remains read-only as well. The normal charge-failure terminal copy may be used only if the CLI independently establishes trustworthy event provenance/cause. |
| 12 | `sub_expire_warn` | user | **Selected by `autoRenew`.** `autoRenew=true` (or missing/legacy → treated as true) → existing copy (`content::sub_expire_warn_user_notify`), unchanged. `autoRenew=false` → "[Subscription Ending Soon] Subscription job {job_id} (period {periodStart}–{periodEnd}) will expire and close on {periodEnd}. To continue using it, please enable auto-renew in time." The CLI English literal is canonical; localize faithfully at render time. |
| 13 | `sub_reject_refund_notify` | user | This legacy event describes the backend automatic-refund branch; no client-side `claim-auto-refund` follows it, but the event cannot create proof. Render `[Auto-Refund Settled] [Auto-Refund]` with the terminal hint only when durable local `request-refund` provenance binds the same job, Buyer, formal `jobType=1` subscription, exact positive original amount, and token address, and fresh composed detail proves Buyer ownership and `FAILED(9)`. Event-only or bare `FAILED(9)` remains incomplete. Optional Provider/Service, period, token-symbol, and `paymentMode` values veto only on a two-sided mismatch; absence affects detail/display only. Tx Hash is optional, with no required refund-specific hash field. |

`sub_expire_warn` is split by `autoRenew` and mirrored in row 12 above; its authoritative copy is the English `content.rs` literal, localized at render time.

Field notes (mirror of the CLI's reading rules, same non-authoritative caveat):
absent optional field → the CLI omits that line (never errors). `failReason` is free backend text
(may be non-English) — kept verbatim, not interpreted. Trial-window fields are `trialStartTime` /
`trialEndTime` (the CLI falls back to the legacy `trailStartTime` / `trailEndTime` spelling during
the backend transition). There is no `periodIndex`; the period comes from `subStartTime` /
`subEndTime`. The user side handles events 0,1,3,4,5,6,7,8,9,10,11,13; the ASP side renders 2,9,10,11 as
display notifications, handles 6 (`sub_user_reject`) as a **decision** (task-asp.md), and silently
ignores 1,3,4,5 (buyer-only) plus 7,8 (the ASP's own actions — no ASP-side push; their lifecycles
live in the subscribe-agree-refund / subscribe-dispute action flows).

Refund V2 applies the same provenance-plus-fresh-state rule to
`job_refunded`, `job_auto_refunded`, `job_asp_reject_expire`, and
`dispute_resolved`. The legacy event may describe a branch, but cannot create
proof. A User-requested or provider refund-decision-timeout Failed(9) path
requires durable local `request-refund` provenance. It must bind job, Buyer,
formal job type, exact positive original amount, and token address and be paired
with fresh Buyer-owned Failed(9) facts before closing that subscription refund.
An acceptance/delivery-timeout path instead reaches finality directly from
fresh Buyer-owned paid non-trial Expired(8), task kind, and exact original
payment; no Failed(9), Tx Hash, request provenance, or local observation journal
is required. A scoped lifecycle/watch event emits the terminal marker and
cleans up without a Buyer claim/finalize write; a direct read follows `stop`.
Trial and zero-amount Expired(8) instead use terminal
`expired_without_refundable_payment` with `settlement.state=not_required` and
no fund-movement claim. Optional Provider/Service, period, token-symbol, and
`paymentMode` fields only veto on a two-sided mismatch. For `dispute_resolved`,
fresh status 9 identifies the User-winning
branch and fresh status 6 the ASP-winning/no-refund branch only after durable
local `request-refund` provenance, job type, Buyer ownership, and the composed
facts are verified. Without that proof, render no verdict and perform no
rating, notification, or cleanup side effects. Event-only and bare subscription
Failed(9) remain ambiguous.
