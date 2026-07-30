# Subscription Notification Copy — Human-Readable Mirror (NOT runtime)

> **Canonical source = the CLI** (`cli/src/commands/agent_commerce/task/user/content.rs` and the
> ASP-side content module): the text a user sees is whatever `next-action` embeds in its returned
> `user-notify` script. This file is a **review/debug/localization reference only** — it is not
> loaded by any activation flow, and it must never be used to hand-compose a notification
> (see `task-core.md` §Subscription Notifications: sub_* events always run `next-action`).
> If this table and the CLI disagree, the CLI is right and this file is stale.

| # | Event (`event`) | Target | Rendered notification (English canonical, from the authoritative copy doc) |
|---|---|---|---|
| 1 | `sub_created` | user | trialType=1 → "[Trial Started] Your free trial is active ({trialStartTime}–{trialEndTime}). After it ends, {tokenAmount} {tokenSymbol} will be auto-charged on {trialEndTime} to convert to a paid subscription (attempted once, within the final hour before the trial ends — it will not retry if missed)." (trial is charge-free — never the first-charge copy). Otherwise → "[Subscribed] Job {jobId} (subscribing to {jobTitle}) is on-chain, status: Active, current period {subStartTime}–{subEndTime}. First charge of {tokenAmount} {tokenSymbol} completed. Auto-renew is on; next charge date: {subEndTime}." (nextChargeAt = periodEnd = subEndTime; next-charge clause omitted only if subEndTime absent.) |
| 2 | `sub_asp_selected` | asp | "[New Subscription] You have a new subscriber for {jobTitle}. Buyer: {buyerAgentId}. Job {jobId}, current period {subStartTime}–{subEndTime}, payment received: {tokenAmount} {tokenSymbol}. Please begin delivering the service." |
| 3 | `sub_cancel` | user | Branches on `trialType`. `trialType=1` (trial cancel) → terminal: "[Cancelled] Auto-conversion for the {jobTitle} free trial has been cancelled. This trial continues unaffected until {trialEndTime}; no charge will occur after it ends." `trialType=0` / absent (formal-period cancel) → NON-terminal: "[Auto-Renew Cancelled] Auto-renew for {jobTitle} has been cancelled. Current service continues until {subEndTime}; job {jobId} will then move to Completed." On `cancelResult=fail` (either branch) show `failReason` verbatim. |
| 4 | `sub_trial_into_active` | user | "[Trial Converted] Your free trial has ended; the first charge of {tokenAmount} {tokenSymbol} for {jobTitle} is complete, current period {subStartTime}–{subEndTime}. Job {jobId} status: Active. Next charge date: {subEndTime}." (period range + nextChargeAt = subEndTime; both omitted if the period fields are absent.) |
| 5 | `sub_renew` | user | `renewResult=success` → "[Renewed] {jobTitle} — this cycle's renewal of {tokenAmount} {tokenSymbol} is complete. Job {jobId} status: Active. Next charge date: {subEndTime}." (the period range is intentionally NOT repeated — renewal keeps the same billing cycle; nextChargeAt = subEndTime, omitted if absent.) `renewResult=fail` → "[⚠️ Renewal Failed] {jobTitle} — this cycle's charge failed: {failReason}. A grace period is in effect (until {subBufferEndTime}); service continues normally and the system will keep retrying. Please add funding / increase allowance as soon as possible." (NON-terminal either way: a failed renewal only enters the grace period — sub_close/sub_failed_notify own the terminal cleanup.) |
| 6 | `sub_user_reject` | user | "[Rejection Submitted] Your rejection for {jobTitle}'s current period ({subStartTime}–{subEndTime}) has been submitted. The ASP must respond, or a full refund of {tokenAmount} {tokenSymbol} will be issued automatically." (`rejectWindowEndsAt` response-deadline clause if present — not in the event body.) |
| 7 | `sub_asp_agree` | user | "[Refund Complete] The ASP has acknowledged the issue with {jobTitle}'s current period ({subStartTime}–{subEndTime}). A full refund of {tokenAmount} {tokenSymbol} has been sent directly to your wallet, and auto-renew has been turned off." (user side only — the ASP's own action gets no ASP-side push; the subscribe-agree-refund action flow owns that lifecycle.) (terminal) |
| 8 | `sub_asp_dispute` | user | "[Dispute Filed] The ASP has disputed your rejection of {jobTitle}'s current period ({subStartTime}–{subEndTime}) and escalated to arbitration. Job {jobId} status: Disputed." (user side only — the ASP's own action gets no ASP-side push; the subscribe-dispute action flow owns that lifecycle.) |
| 9 | `sub_complete_notify` | user + asp | "[Subscription Complete] {jobTitle} has completed all scheduled renewals. Job {jobId} status: Completed; service ends normally at {subEndTime} with no further renewal." (user side; the ASP side keeps its own ASP-perspective copy.) (terminal) |
| 10 | `sub_close_notify` | user + asp | "[Service Closed] "{jobTitle}"'s current period ({subStartTime}–{subEndTime}) has ended. Job {jobId} status: Closed. The system will automatically complete the rating for this job." (period range omitted when absent; user side; the ASP side keeps its own ASP-perspective copy.) (terminal) |
| 11 | `sub_failed_notify` | user + asp | Branches on `trialType`. `trialType=1` → "[Trial Ended] "{jobTitle}" — the conversion charge could not be completed before the trial ended ({failReason}); conversion failed with no retry. Job {jobId} status: Closed. Subscribe again to continue." (reason clause omitted when absent.) Otherwise → "[Subscription Ended] "{jobTitle}" — the charge still failed after the grace period; the service ended at {subBufferEndTime}. Job {jobId} status: Closed. Subscribe again to continue." (the "service ended at" clause appended only when subBufferEndTime present; NO reason slot in this branch.) (user side; the ASP side keeps its own ASP-perspective copy.) (terminal) |

The CLI additionally renders `sub_expire_warn` and `sub_reject_refund_notify` (added after this
table's last sync) — their copy lives only in the CLI; read the `content.rs` functions directly.
This gap is itself the reason the mirror is non-authoritative.

Field notes (mirror of the CLI's reading rules, same non-authoritative caveat):
absent optional field → the CLI omits that line (never errors). `failReason` is free backend text
(may be non-English) — kept verbatim, not interpreted. Trial-window fields are `trialStartTime` /
`trialEndTime` (the CLI falls back to the legacy `trailStartTime` / `trailEndTime` spelling during
the backend transition). There is no `periodIndex`; the period comes from `subStartTime` /
`subEndTime`. The user side handles events 1,3,4,5,6,7,8,9,10,11; the ASP side renders 2,9,10,11 as
display notifications, handles 6 (`sub_user_reject`) as a **decision** (task-asp.md), and silently
ignores 1,3,4,5 (buyer-only) plus 7,8 (the ASP's own actions — no ASP-side push; their lifecycles
live in the subscribe-agree-refund / subscribe-dispute action flows).
