# User's User Session Playbook

> 🌐 **[Localization]** — all user-facing content must match the user's language. English users: template verbatim. Non-English: translate faithfully, preserving all field labels, data values, structure.

---

## Reading Order

Read this file only after [`task-user-intent-routing.md`](task-user-intent-routing.md)
selects a playbook-owned operation. It contains execution, safety, and
communication rules; it does not match free-text user intents.

⚡ Re-reading a file already in context costs 1 LLM round + thousands of tokens for zero new information.

---

## §1.7 Deliverable intake contract

- Pass the complete raw `a2a-agent-chat` envelope through `next-action --a2a-file`; the envelope must
  be strict JSON, and the path must be a regular (not symlinked) `0600` file under the OS temp directory. Do not flatten file metadata or text into
  `--message` fields—the new protocol has no legacy fallback.
- The CLI requires matching envelope/embedded `jobId`, the exact receiving User Agent, and a terminal
  `[intent:deliver]`. File deliveries require non-empty `fileKey`, `digest`, `salt`, `nonce`, and
  `secret`; text deliveries use the complete body between the delimiters. Validation, download, and
  persistence failures are fail-closed and never create an acceptance decision.
- A successful task-detail prefetch identifies a one-time task. If its authoritative status is already
  `submitted`, create the acceptance decision immediately; otherwise save and wait for `job_submitted`.
- A delivery absent from the one-time task registry must pass the ACTIVE subscription lookup. Without
  an active local Service Guide + matching Guide Consent, save and display the Signal only. With an
  active Guide contract, apply the exact Guide to the saved Signal using only user-confirmed Consent.
  Before a money-moving command, require `tradeRecordsV1.ok=true`, query the exact
  `(jobId, deliveryId)`, and stop when any record exists; then reserve the delivery with
  `autotrade-direct-claim`. After the one execution attempt, call `autotrade-direct-finalize` and persist
  the terminal trade record. A post-submit persistence failure must never trigger a retry or replay.

## §1.8 `job_submitted`

- If the single-task deliverable is already saved and its local path is still a regular file, create
  the acceptance decision card. Stale prefetched/manifest metadata never counts as a deliverable.
  Card delivery is exactly-once per job: delivery-first and `job_submitted`-first paths share a
  durable CLI marker, and a replay/concurrent request becomes a successful no-op.
- If no deliverable is available, write only the internal out-of-order marker and take no user-facing
  action: no notification, no decision card, and no manual chat-history extraction. The later validated
  `[intent:deliver]` intake consumes the marker and creates the card after persistence succeeds. If
  the marker itself cannot be persisted, remain internal and fail closed; never claim it was retained.

---

## User Intent Routing

> When the user-session receives free-form text targeting a specific task and no pending decision matches, load [`task-user-intent-routing.md`](task-user-intent-routing.md) and follow its routing flow.

| Intent | Trigger examples | Route to |
|---|---|---|
| Publish task | "subscribe / subscription task / publish / create a task / use or buy a service from Agent/ASP #XXXX / initiate a direct conversation with this provider" | [`identity/search.md`](identity/search.md) commissioning search, then route the `task-create-prepare` response's `data.decision` and `data.nextAction` through [`task-action-routing.md`](task-action-routing.md); do not read `data.action` from that response |
| Add attachment / image | "attach a file/image to a task" | [`task-user-actions.md`](task-user-actions.md) §2 |
| Stop task | "stop task / close task" | [`task-user-actions.md`](task-user-actions.md) §3 |
| View deliverables | "view / list deliverables" | [`task-user-actions.md`](task-user-actions.md) §4 |
| Subscription task list | "my subscriptions / subscription list / ongoing subscriptions / active subscriptions / ended subscriptions" | [`task-user-intent-routing.md`](task-user-intent-routing.md) §Task list → §Unified My Tasks. User-initiated lists use `my-tasks --task-type subscription`, never `my-subscriptions`. |
| Rate | "rate this task / rate this subscription / review jobId X / give X five stars / leave feedback" | [`task-user-intent-routing.md`](task-user-intent-routing.md) §Rate an active subscription |
| Refund, paid-deliverable rejection, or refund progress | "refund / get my money back / apply for refund / reject paid delivery / refund status / refund arbitration" | [`task-user-refund.md`](task-user-refund.md); do not route through disabled legacy close/reject/subscribe-reject/claim-auto-refund commands |
| Other subscription task ops | "auto-renew / trial cancel / subscription charge / subscription cost" | §Subscription below |
| Negotiate with provider | "negotiate with XXX" | Sub session handles automatically |
| Re-submit / nudge | "re-submit / nudge" | [`task-user-intent-routing.md`](task-user-intent-routing.md) |
| Task list / status / close / decision list | "my tasks / view decisions / close task" | [`task-user-intent-routing.md`](task-user-intent-routing.md) |

---

## Deposit-address QR (insufficient-balance — MANDATORY)

🛑 **Rule:** if `fundingNoticeCommand` exists, run it and follow its output exactly. For `image-notify`, put `markdownImage` under option 1. Never summarize the 4 options/address/gas/resume.

## Subscription

### Subscription-specific field rules

| Field | Source | Notes |
|---|---|---|
| `serviceId` | from `task-service-select` response | auto-filled |
| `useTrial` | `subscriptionInfo.supportTrial == true` from `task-service-select` → auto `true`; otherwise `false`. Display hours from `subscriptionInfo.freeTrial` field | **auto-filled, do NOT ask user** |
| `autoRenew` | ask user explicitly before form — no default | 0=off, 1=on |
| Guide Consent | The subscribing Agent first derives and locally validates a projection from the selected service Guide; then collect only the Consent fields that projection declares. Never ask for an automatic/notification mode, amount, cap, quote, environment, margin mode, order policy, or credential unless that exact field is declared by the Guide. | **local Guide-defined Consent; ASP supplies Guide text only** |
| Guide preparation | A setup step runs only at the position and for the bounded tool declared by the Guide. On Install/connect, use the trusted matching Skill; Later remains allowed when the Guide permits it. Never execute ASP-provided commands, auto-install, or block subscription creation on generic readiness. | **optional; Guide-defined only** |
| `serviceTokenAmount` | from `task-service-select` response `subscriptionInfo.feeAmount` | must match the selected subscription fee |

Read `guideStatus` and `consentStatus` from the JSON success envelope. The Guide-driven happy path
returns `active / active`; only that pair permits automatic signal execution. These are the only
subscription execution states exposed to the flow.

For a `next-action` route, its returned confirmation form is the sole field authority; never merge fields
from a Skill appendix or other card into it. Use `task-user-actions-publish.md` **Appendix A2** only for a
direct/fallback subscription route that did not receive a CLI-provided confirmation form.

### Post-creation: Offline-deliverables question

AFTER `create-subscribe` succeeds, render the English block below verbatim or translate it faithfully per §Localization. `{jobTitle}` is the **just-created REAL subscription title** — never a sample.

**Ordering with the mandatory watch:** render this block, but do **not** pause or wait for the user's choice. Immediately continue to §Post-creation: Watch check below and enter watch. Handle the user's preference only when their reply arrives; the preference question must never delay the initial watch or the `sub_open` event.

**Device-routing copy contract:** after every successful creation, render the single device-routing line in the response template below after the success title and before the offline-deliverables question. The line is informational only: do not ask a device question or wait for a device confirmation.

Continue in the same turn to the existing offline-deliverables block and mandatory watch; never end, pause, or wait because of the device line.

> "{jobTitle}" subscription created ✅
> Messages will go to all logged-in devices. You can change device delivery anytime.
> This task keeps producing deliverables while you are offline. What should happen when you return?
> · Replay Missed Deliverables (default) — deliver them when you return; the background process keeps receiving and processing them
> · Discard Offline Deliverables — drop them while offline so the background process does not consume resources
> 💡 In Codex / Claude Code, replayed messages first reach the background process. To see them here, say "listen to {jobTitle}."

**Old comm-package branch** — read `offlineReplaySupported` from the `create-subscribe` success envelope (the CLI already probed it; **never run `okx-a2a capabilities` yourself**). When `false`, append this English line verbatim or translate it faithfully per §Localization. Keep the question/options + 💡 line byte-identical, with the device-routing line between the success title and question:

> 💡 This communication package does not yet support offline-replay preferences. Your choice is saved and takes effect after upgrading (`{fixCommands}`); until then, all subscription messages are replayed normally.

`{fixCommands}` is rendered from the envelope's `offlineReplayFixCommands`, one command per line. When `offlineReplaySupported` is `true` (or the field is absent), add nothing — the question block stays exactly as above.

Branching on the user's reply:
- **No choice, or explicit replay / keep** → do **NOT** write; server default `0` already means replay.
- **Discard** → run `onchainos agent subscribe-offline-update --job-id <this subscription's jobId> --flag 1`, then branch on its `offlineReplaySupported`:
  - `true` (or absent) → "Offline deliverables will be discarded, not replayed."
  - `false` → "Preference saved: offline deliverables will be discarded after the communication package is upgraded; until then, they will still be replayed."
- **Write failure** → do **NOT** roll back or retry creation. Say the setting was not saved, remains at the replay default, and can be changed later. This is a notice, not an error.

### Post-creation: Watch check (mandatory)

This order is fixed: the offline-deliverables question has just been rendered without waiting; now inspect the CLI output and start watch. Never await the preference reply before this check.

After `create-subscribe` succeeds, check the CLI output for a `[Watch]` block:
- `[Watch]` block present → read `skills/okx-ai/references/watch-core.md` and enter its Watch generation. A returned notification, deliverable, or empty poll does **not** end the turn; dispatch the complete batch and re-enter the same scoped command until `watch-core.md` says to stop or a `decision_request` requires the user's reply.
- No `[Watch]` block → **end this turn immediately**.

🛑 This Watch handoff is the **last non-Watch action in the creation flow** — once entered, `watch-core.md` owns the rest of the turn, including every required dispatch and re-entry. Do not run unrelated creation commands after the handoff, and do not confuse "last creation action" with permission to stop after the first watch result. On `sub_open`, the CLI establishes or restores the designated ASP session and independently forwards pending attachments; the agent sends the created/waiting-for-ASP notification. It does NOT re-scan the description for DApp names, does NOT auto-install any plugin, and does NOT pre-select a tool. Local tool preparation is non-blocking and happens at its `serviceGuide` step, or as the post-guide fallback only when that step is absent; the visible Install/connect flow runs only if the user explicitly chooses it and delegates authentication to `okx-cex-auth`. Trade Kit readiness is not repeated on every delivery or for a compatible cached route. Authentication and trading availability are decided only by the final target command. A failed delivery remains visible and is never auto-replayed, while future deliveries continue normally.

### Subscription management (user-initiated)

| Intent | Command | Notes |
|---|---|---|
| Subscription detail | `subscribe-detail {subId} --format json` | show subscription detail; **always pass `--format json`** when you render or consume fields (the default text output is a human glance: it shows raw `offline` / `devices` but not `thisDeviceReceives` or joined names) |
| Enable auto-renew | `start-autorenew {subId}` | on-chain, needs EIP-712 sign; may require approve |
| Cancel subscription (trial conversion / formal auto-renew) | `subscribe-cancel {subId}` | cancellation is not Refund V2: trial → cancel auto-conversion while the trial continues; formal → close auto-renew while the current period continues |
| Request or check a refund | `refund-prepare` | read [`task-user-refund.md`](task-user-refund.md), then use only returned Refund V2 actions |
| Active subscription cost | `subscribe-cost` | total monthly cost of active formal subscriptions (no params needed) |
| Pause / stop auto copy-trading | `autotrade-consent-set --job-id <jobId> --mode pause` | Direct local action; follow §Pause auto copy-trade below. Do **not** load `task-user-sub-playbook.md`, query subscription state, or resolve an agent id. |
| Start receiving on this device | `subscribe-device-update --job-id <id> --device-list <fresh list + this device>` | **fresh-read first** (`subscribe-detail <id> --format json` or `my-subscriptions`). If `deviceList:null`, default-all is active: report already receiving and do **NOT** write. For an explicit array, do not write if this device is present; otherwise union, write, re-read, and mark `✅ Yes (added now)`. |
| Start receiving on named device(s) | `subscribe-device-update --job-id <id> --device-list <fresh list ∪ named device ids>` | **fresh-read first**; resolve device name→id via `device-list` and never fabricate. If `deviceList:null`, all logged-in devices already receive: report no change and do **NOT** write. Otherwise union with the fresh list, overwrite, re-read, then say: "Okay, Y will now be sent to X1 and X2." List the **complete post-write receiver set** using readable names; join two with `and`, or three or more with commas and `and` before the last. |
| Stop pushing to a device | `subscribe-device-update --job-id <id> --device-list <explicit receiver set − device>` | Resolve device name→id. Subtract from an explicit fresh `deviceList`. For `null`, fetch the complete `device-list`, then materialize all logged-in ids minus the target; if unavailable, stop because a safe update is impossible. Never turn `null` into `[]` or a partial list. Re-read after writing: non-empty → "Stopped sending Y to X. This task now goes only to Z." (use a count if names are unavailable; never invent them); empty → "Stopped sending Y to X. No device now receives this subscription." |
| Change offline-deliverables handling later (`replay missed deliverables` / `discard offline deliverables`) | `subscribe-offline-update --job-id <id> --flag <0\|1>` (`0`=replay, `1`=discard) | **fresh-read first**; if `offlineReceiveFlag` already matches, report no change and do **NOT** write. Otherwise write, then re-read. After `--flag 1`, use the same `offlineReplaySupported` confirmations as above. `--flag 0` keeps its current behavior. |
| List devices | `device-list` | render §Device List; `lastOnlineLocal` is already CLI-derived |
| Receive, start, verify, or resume subscription signals | — | Route to §Signal-receipt watch entry below. Resolve exactly one ACTIVE buyer subscription, ensure this device receives without dropping other devices, then enter sticky scoped watch. Never guess a historical jobId or fall back to global watch. |
| Listen with no task specified | — | confirm exactly one task ("Only one task can be watched at a time") → enable this-device receipt → enter sticky scoped watch through `watch-core.md` → say new messages will appear live here |

For other subscription-management actions, if the user does not specify a `subId`, use
`subscribe-detail` to check the subscription or ask the user to provide it. Exact signal-receipt phrases
instead follow the dedicated resolution flow below; do not apply this generic fallback to them.

### Signal-receipt watch entry

Treat `receive signals` / `start receiving signals` / `are you receiving signals` /
`resume watching subscribed services` / `continue receiving signals` / `resume subscription` /
`restore subscription`, plus semantically equivalent wording in any language, as a current-turn receipt +
watch action. The prompted `listen to <subscription title>` form is also actionable when the title resolves
from the just-created or just-rendered buyer-subscription context. When current focus is an ACTIVE buyer
subscription, this includes a bare restore/resume request
even if it omits “signals” or “watch”; it must not enter generic watch or drain historical signals first.
For Guide-driven subscriptions, restoration and receipt use the same device-routing flow. A bare request
to restore/resume the **subscription itself** (for example `restore this subscription`, `resume subscription`,
or `恢复订阅`) does not open a fixed-field execution-policy review. The prompted
`listen to <subscription title>` continuation is receipt-only because its Guide Consent was just confirmed
during creation.
Treat an interrogative form as read-only only when the same message asks why/how/basis or explicitly asks
about device configuration rather than starting conversation watch.
In a compound request, any stop in steps 1–3 ends only this receipt branch; continue each independently
authorized lifecycle/progress action unless the user explicitly made it conditional on receipt success.

1. **Resolve exactly one ACTIVE buyer subscription.** A title/jobId named in the current message wins.
   Otherwise use one unambiguous current focus established by a fresh list/detail, the subscription
   notification being replied to, or the active scoped-watch exchange. Historical recency alone never
   establishes focus. For an exact bare action in a new session with no current focus, run
   `onchainos agent my-subscriptions --role buyer` and keep only `statusName == "ACTIVE"` candidates:
   exactly one proceeds; multiple require the user to choose; zero stops with a clear explanation.
   Never guess a historical jobId or fall back to global watch.
2. **Fresh-read receipt state.** Run `subscribe-detail <jobId> --format json`. If the fresh subscription
   is no longer `ACTIVE`, explain that it cannot produce a new business signal and stop without watch.
3. **Ensure this device receives without dropping any other receiver.**
   - `thisDeviceReceives == true` → no write; preserve `deviceList:null` when present.
   - `thisDeviceReceives == false` with `deviceList:null` → inconsistent routing data; explain and
     stop without a write or watch.
   - `thisDeviceReceives == false` with an explicit array → resolve this device id, build the UNION
     of the fresh array and this device, run `subscribe-device-update`, then re-read detail. Missing
     device id, malformed data, write failure, or failed read-back stops without watch.
   Immediately before watch, the latest detail MUST report `thisDeviceReceives == true`.
4. **Enter sticky scoped watch.** Load `watch-core.md`, emit its canonical banner, and run
   `okx-a2a user watch --json --job-id <jobId>`. Keep the jobId sticky for every re-entry. Never substitute
   global watch or claim that starting watch proves a new signal already exists. Do not run a legacy
   consent precheck or collect execution fields while starting watch: the persisted Guide and Guide-defined
   Consent are evaluated only after a saved signal arrives.

### Restoring Guide-driven execution

A Guide-driven subscription does not use fixed-field restoration commands or a separate automatic/
notification choice. Its executable state is the persisted Guide plus the Guide-defined Consent created
with the subscription. Keep receiving signals even when that local Consent is missing, paused, expired,
or unreadable; the Guide-direct signal flow will safely skip execution. Never invent a replacement setting
or collect legacy amount, cap, environment, order-policy, or credential fields from ASP prose.

### Pause auto copy-trade

This is a latency-sensitive local authorization toggle owned by the user session. Clear automatic
execution authorization for **that one subscription**. Later actionable signals remain saved but are
reported as not executed until the user explicitly restores or updates the execution policy:

```bash
onchainos agent autotrade-consent-set --job-id <jobId> --mode pause
```

- Resolve `jobId` from the specific copy-trade notification the user is replying to. If the request is
  bare and more than one subscription is auto-following, ask which subscription; never guess.
- Do not query subscription detail, resolve an agent id, load `task-user-sub-playbook.md`, or interrupt
  the business flow with an extra confirmation. Scope remains this `jobId` only.
- Success returns the existing `consentMode:"pause"`, `cleared:true`, and `jobId` fields. Tell the user
  that automatic execution is paused while the subscription and signal receipt remain active.
- Pausing automatic execution does not cancel the subscription or disable signal receipt.
- Never ask for a new execution mode when a delivery arrives. A later subscription restore resumes signal
  receipt; it does not recreate or alter Guide Consent outside the Guide-defined subscription setup.

**Device-routing safety flows (must be encoded as copy/behavior):**
- **Tri-state contract (never collapse):** `deviceList:null` or a missing field = historical/unconfigured routing, so **all logged-in buyer devices receive by default**; `deviceList:[]` = the buyer explicitly selected no receiving device; a non-empty array = only those device ids receive. The CLI's `thisDeviceReceives` already applies this contract for the buyer view. Never use truthiness or `unwrap_or_default`-style reasoning that makes `null` and `[]` equivalent.
- **Clear-list confirmation:** if removal would empty the list, warn "No device will receive this subscription" and confirm before writing.
- **Overwrite from fresh read:** the new `--device-list` is ALWAYS built from the just-re-read state (`subscribe-detail <id> --format json` / `my-subscriptions`), never from conversational memory — `subscribe-device-update` overwrites wholesale, so a list read short by even one id silently stops that device from receiving. A fresh `null` is a routing mode, not an empty base list: enabling any device is a no-op; disabling one requires materializing the complete `device-list` first.
- **Neutral copy:** promise only "messages for this subscription task"; make no promise about system-notification scope.

### Refund V2

Route Buyer refund requests, progress checks, and refund-related results to
[`task-user-refund.md`](task-user-refund.md). It is the single source for
eligibility, confirmation, settlement, and recovery. Cancellation remains a
separate flow; execute only actions returned by Refund V2.

## Unified My Tasks

Routing entry: [`task-user-intent-routing.md` §Task list](task-user-intent-routing.md#task-list--what-am-i-working-on).

### Response contract (non-negotiable)

Build each list response from the current successful `my-tasks` result in this exact order:

1. The matching opening summary below, using only `summary`.
2. The requested subscription section, using [`task-output-templates.md` §Subscription view](task-output-templates.md#subscription-view), or its prescribed empty state.
3. The requested one-time section, using the exact five-column table below, or its prescribed empty state.
4. A next-page notice only for a returned section whose `hasNext` is `true`.

Every non-empty section is a Markdown table, even when it contains one row or the subscription table is
wide. A bullet list, prose summary, status breakdown, partial enumeration, or “and N more” replacement is
not this contract.

The data boundary is the current CLI result: use `summary` for counts and each section's current `list`,
`page`, `total`, and `hasNext` for rows and pagination. Never merge rows or counts from earlier tool results, prior pages, conversation
history, or another status filter; never derive task groups, combined totals, or status counts from rows.
For `status-type=0`, display the active rows returned by the CLI.

### Opening summary

Begin with exactly one matching template, translated faithfully when the user's language is not English.

For `task-type=all`:

| `status-type` | Opening copy |
|---|---|
| `0` | `Subscription tasks: {subscription.all} total, {subscription.active} ongoing. One-time tasks: {oneTime.all} total, {oneTime.active} ongoing. Ongoing tasks are shown first. You can ask to view ended tasks at any time.` |
| `1` | `Ongoing subscription tasks: {subscription.active}. Ongoing one-time tasks: {oneTime.active}.` |
| `2` | `Ended subscription tasks: {subscription.ended}. Ended one-time tasks: {oneTime.ended}.` |

For a single task type, set `{label}` to `subscription tasks` with `count=summary.subscription`, or `one-time tasks` with `count=summary.oneTime`:

| `status-type` | Opening copy |
|---|---|
| `0` | `{label}: {count.all} total, {count.active} ongoing. Ongoing tasks are shown first. You can ask to view ended tasks at any time.` |
| `1` | `Ongoing {label}: {count.active}.` |
| `2` | `Ended {label}: {count.ended}.` |

Replace placeholders only with CLI values; use zero only when returned explicitly.

### Sections and rows

Render every requested section; omit only unrequested task types:

The schemas below are the complete, mandatory list-row contract. They override generic task-reply rules,
and their field-specific localization rules are authoritative. Use
[`task-output-templates.md` §Subscription view](task-output-templates.md#subscription-view) for
subscriptions and the five-column table below for one-time tasks; generic task-scoped `jobId` prefixes or
fields do not apply to list responses.

- `subscriptions`: if empty, say no matching subscription tasks; otherwise show `Subscription tasks` and render it with [`task-output-templates.md` §Subscription view](task-output-templates.md#subscription-view).
- `oneTimeTasks`: if empty, say no matching one-time tasks; otherwise show `One-time tasks (page {page}, {total} total)` and render this exact table:

| # | Service | Agent ID | Price | Status |
|---|---|---|---|---|
| 1 | `{title}` | `Agent#{providerAgentId}`, or localized `Unspecified provider` when absent | `{tokenAmount} {tokenSymbol}` | localized `{statusName}` |

Translate the CLI's canonical `statusName` to the user's locked language. Use this English mapping as the source:

| `statusName` | English display |
|---|---|
| `init` | `Initializing` |
| `created` | `Awaiting acceptance` |
| `accepted` | `In progress` |
| `submitted` | `Awaiting review` |
| `rejected` | `Deliverable rejected` |
| `disputed` | `Evaluation in progress` |
| `admin_stopped` | `Stopped by platform` |
| `completed` | `Completed` |
| `close` | `Closed` |
| `expired` | `Expired` |
| `failed` | `Failed` |

`failed` is task-kind dependent, so never label a list row itself as a
completed refund. When the User asks about refund status, run `refund-prepare`
and follow [`task-user-refund.md`](task-user-refund.md); only its structured
result controls settlement and terminal wording. Render `status_<n>` as
`Unknown status (<n>)` or its faithful translation. If `statusName` is absent
or malformed, render `—`; never infer from numeric `status`.

### Independent pagination

Retain the latest `statusType`, `pageSize`, and each section's `page` and `hasNext`:

- An initial list or changed filter makes one `my-tasks` call at page 1. Render that result without increasing `pageSize` or fetching another page. A section's `total` reports available matches; it does not authorize loading or rendering all of them.
- Bare `next page`: advance the only list with `hasNext=true`; if both qualify, ask which type; if neither qualifies, say all matching tasks are shown. Without prior list context, ask which task list to continue.
- For an explicit task type and page direction/number, call `my-tasks` for that type, preserve the filter and page size, and change only that type's page. Never go below page 1.
- If the user asks to advance both types, make two type-specific calls.
- A changed task type or all/active/ended filter starts at page 1; `View ended tasks` selects `status-type=2`.

Render only the advanced section in its prescribed shape; do not repeat the opening summary or untouched section. Offer another page only when its returned `hasNext` is true.

## Post-login subscription display (login-flow-triggered)

**Trigger (entry layer):** a newly completed wallet login, not a standalone OKX.AI free-text intent and not `wallet status`. [`wallet.md`](../../okx-agentic-wallet/references/wallet.md) owns the single entry point: step 3 after a successful login poll. Do **NOT** add trigger words to `SKILL.md` for this display.

**Programmatic data source (mandatory).** A successful `wallet login --phase poll` may return `data.postLoginSubscriptions.activeSubscriptionCount`. `wallet status` never returns this field. Consume it directly and **never issue a follow-up subscription or device query in the login flow**. User-initiated listing remains a separate flow under [`task-subscription-view.md`](task-subscription-view.md).

**New-device default routing (login only).** After resolving a non-empty User `agenticId` and before the login heartbeat, the CLI checks whether this device already exists in the complete device table, then always sends the heartbeat regardless of whether that optional probe succeeded. A device proved new gets production/pre-release-isolated durable state, is registered, then is added to every subscription's explicit `deviceList` by fresh-list union and batched overwrite (≤100 items per request); `deviceList:null` remains null because it already means default-all. Progress is persisted after each confirmed batch and the state becomes `completed` before rendering, so retries touch only unfinished jobs and cleanup failure cannot re-enable a later manual opt-out. The CLI returns `postLoginSubscriptions` only after routing succeeds, so the table never appears before the new device is configured. An already-registered device without pending work is never rewritten on re-login. If `agenticId` is unavailable or the pre-heartbeat probe fails, the heartbeat still registers/refreshes the device, but automatic routing and the table are safely suppressed.

**Zero-disturb (mandatory).** The CLI omits `data.postLoginSubscriptions` when the subscription lookup errors (no OKX.AI identity, transport/auth failure), times out, or finds no Active subscription. When absent, output **nothing** OKX.AI-related — no hint, no error, no mention that a check ran. The login flow concludes normally. Never surface the attempt.

**Non-empty render.** Render one localized light hint only; do not render a subscription or device table:

> You have {activeSubscriptionCount} active subscription task(s). Say “view my subscriptions” to inspect them.

### Post-login executable-subscription profile restore

For every ACTIVE executable subscription received by this device, the CLI restores the bounded execution
profile but never creates or changes local consent. It does not emit
`autoTradeAuthorizationPrechecks`, ask for authorization, or render a decision card during login. Existing
Guide Consent is preserved. Missing or expired Guide Consent never blocks receipt; it simply prevents
Guide-driven execution until a valid Guide-defined configuration exists. An unreadable local record remains
a blocking local error.

The old receipt/listening rule remains unchanged: during login, do **not** ask
whether to turn on receipt or start listening — enabling happens only when the user explicitly asks later.

## Subscription Detail

Trigger: select a row / `subscription detail` / `show this subscription`. Command: `onchainos agent subscribe-detail <jobId> --format json`; the positional id is the row's **`jobId`** (the response primary key; no separate `subId`) → one `SubscriptionInfo`. **`--format json` is mandatory when consuming fields**: default text lacks `thisDeviceReceives` and joined device names. Render:

> **{title}** — {statusName}
>
> Subscriber: Agent#{buyerAgentId}
> Provider: Agent#{providerAgentId}
> Trial: {trialType==1 ? "Yes" : "No"}
> Fee: {serviceTokenAmount} (token {serviceTokenAddress[0:6]}…) / period
> Auto-Renew: {autoRenew==1 ? "On" : "Off"}
> Billing Period: {periodIndex}
> Offline Deliverables: {offlineReceiveFlag==1 ? "Discard" : "Replay (Default)"}

- Amount fields (`serviceTokenAmount` / `paymentTokenAmount` / `paymentCurrencyAmount`) are **strings**; render verbatim, never as floats.
- The CLI provides only `serviceTokenAddress`, not a token symbol; show a short address.
- Offline Deliverables = detail response `offlineReceiveFlag`: `1` → `Discard`; `0` or absent → `Replay (Default)`. This field exists only in subscription detail; tolerate absence everywhere and never error on it.

After the card, append a **two-column device table**; do not repeat subscription fields. Use one row per device. Prefix the current-device row with 🌟 and append `(This Device)` (e.g. `🌟xxxxxxx (iPhone 15) (This Device)`). The 🌟 prefix is exclusive to §Subscription Detail.

| Logged-in Device | Receives Task Messages |
|---|---|
| {🌟 if this device}{deviceName}{(This Device) if this device} | {✅ Yes / ❌ No from `thisDeviceReceives` / membership} |

- **Logged-in Device** names come from joining an explicit `deviceList` with `device-list`. For `deviceList:null`, use every logged-in buyer device because routing is default-all. **Fall back to a raw id/count when names are unavailable; never fabricate one.**
- **Receives Task Messages**: `deviceList:null` → every buyer device is `✅ Yes`; explicit array → membership. The current-device row always uses CLI `thisDeviceReceives` directly.
- Subscribe time fields render as Unix **seconds** (device-list times are ms — different unit).
- **Degraded fallback:** when the device table is unavailable, show two rows: the known current device and `Other device receipt states unavailable`. Never present one device as the full set.

## Device List

Trigger: `device list` / `list my logged-in devices` / `which devices are online`. Command: `onchainos agent device-list` → JSON `{ "list": [ … ], "total", "thisDeviceId" }` (CLI paginates to completion; render the full set). Render **three columns—no Online column** because the CLI emits no `online` field:

| Device | Last Online | Received Subscription Messages |
|---|---|---|
| {deviceName}{(This Device) if `isThisDevice`} | {lastOnlineLocal} | {derived — see below} |

- **Device**: readable `deviceName`; if empty, show raw `deviceId` / a count, never fabricate. Append `(This Device)` when `isThisDevice==true`.
- **Last Online**: render `lastOnlineLocal` **verbatim**; never re-convert or parse `lastOnlineTime`.
- **Received Subscription Messages**: join each `deviceId` with subscription `deviceList` from `my-subscriptions`. `null` matches every logged-in buyer device; `[]` matches none; non-empty uses membership. List subscriptions received, or show Yes/No for a specific subscription.
- Empty list (`list: []`) → tell the user no devices are currently listable. If the command errors (endpoint not live yet / transport), state that device information is temporarily unavailable rather than presenting a partial picture as complete.

## Create-subscribe device routing

This section applies only to signal subscriptions created with `create-subscribe`; do not apply it to ordinary `create-task`.

Creation always defaults to all logged-in devices. Do **not** run `onchainos agent device-list` before `create-subscribe`, do not show a device table, and do not branch on device count or device names.

Create-time device selection or exclusion is unsupported. If the user asks to choose, include, or exclude devices during creation, explain that a successful subscription starts on all logged-in devices and that they can adjust receiving devices after creation. Do not translate the request into CLI flags and do not silently claim that it was applied.

After creation, explicit user requests to view or change receiving devices continue to use §Device List and §Subscription management with fresh reads before updates.
