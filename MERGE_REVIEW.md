# `codex/a2a-skill-cli-contract` → `codex/a2a-skill-cli-contract-elvis` merge review

Regenerated after remote refresh: 2026-09-04 (Asia/Hong_Kong)

> **Superseded lifecycle note (2026-09-04):** the backend broadcast contract is authoritative. bizType 204 emits `sub_open` to Buyer and ASP; bizType 205 emits `sub_created` to Buyer and `sub_asp_selected` to ASP. Any lifecycle resolution below that removes `sub_open` or assigns the initial provider decision to `sub_created` is retained only as historical merge context.

## Executive result

> Resolution update (2026-09-04): the original 58 textual conflict hunks and the 2 additional hunks from the latest Contract refresh are resolved. The first merge is commit `966e03a21`; the refreshed merge is ready to commit but has not yet been pushed. Creation keeps the Elvis v2 transaction path, adds Contract Guide/Consent persistence around the real `jobId`, removes `copyTrade` and fixed-field creation authorization, and retains structured output. `sub_open` and ASP second acceptance are removed; `sub_created` owns User session/attachment setup and `sub_asp_selected` starts ASP service directly.

- Merge direction tested: merge `origin/codex/a2a-skill-cli-contract` into `origin/codex/a2a-skill-cli-contract-elvis`.
- Elvis tip (`HEAD` / ours): `e69c6fd05c83ac1090d8f4c9d4e883666ef3eda8`.
- Contract tip (theirs): `6bb02d665c7d` (latest refresh; the original review used `c7e071bb58b906a51498be105163f739e387d6df`).
- Merge base: `6ff8fc6b093f04cfe80c66248f7cf8f3bb7c39e9`.
- Divergence: contract has 30 unique commits (22 non-merge); Elvis has 23 unique commits (21 non-merge).
- Git result: automatic merge stopped with 13 conflicted files and 58 conflict hunks: 48 Rust hunks and 10 skill-document hunks.
- The refreshed delta added 2 conflicts: keep the v2 `provider_assignment_playbook` for `job_asp_selected` while retaining Contract's title-template injection hardening; combine attachment source validation/manifest output with Contract's jobId path-component guard.

This is not a safe ours/theirs merge. The branches independently replaced the same task-creation and subscription contracts. Git also auto-merged incompatible code outside the visible markers. Simulations confirmed:

- selecting Elvis for every marked hunk still produces 3 compiler errors;
- selecting contract for every marked hunk produces 15 compiler errors.

The resolution therefore needs an explicit target contract, followed by a field-by-field integration.

## Delta from the previous review

Both remote branches advanced normally; both previous tips are ancestors of the new tips, so there was no force-push.

### New Elvis commit: `e69c6fd0 fix(a2a): keep recovered tasks out of subscription routing`

The commit changes 4 files with 76 insertions and 21 deletions:

- stops `job_submitted` recovery from calling `route_subscription_delivery_to_skill`; recovered delivery from this path is explicitly treated as a one-time task;
- removes `transport_identity` from `RecoveredDeliverable`, because the recovered one-time path no longer needs subscription delivery identity;
- removes the public re-export of `route_subscription_delivery_to_skill` from `flow_lifecycle/mod.rs` and `task/user/mod.rs`;
- adds `retire_processed_spool_file`, deleting a successfully persisted direct-delivery spool or renaming it to `.consumed`;
- adds a regression test that spool retirement is successful and idempotent.

The routing change is directionally correct: a successful `/task/{jobId}` prefetch should own one-time-task recovery, while the subscription lookup should only run when no one-time task detail exists.

Two residual replay risks remain and should be reviewed during resolution:

1. In direct intake, failure of both spool deletion and `.consumed` rename is only audited; processing continues. The `.json` spool can therefore remain eligible for a later `job_submitted` recovery and be persisted again.
2. `process_recovered_file` still ignores `remove_file(temp_path)` failure and returns a successful recovered delivery. Because `job_submitted` prefers spool over manifest, a repeated event can select and save the same spool again.

Recommended fix: use one retirement helper in both direct and recovery paths. Do not report successful intake until the file is deleted or renamed out of the `*.json` scan set; alternatively persist a durable consumed delivery identity and check it before saving.

### New contract commits: `bf3c8e38` plus merge `c7e071bb`

The non-merge commit removes `skills/okx-ai/references/identity-errors.md` and removes its callers/tests: 5 files changed, 6 insertions and 75 deletions.

There are no dangling `identity-errors.md` references at the new contract tip, and the contract test build passes. However, the behavior was deleted rather than relocated: the `okx-ai` skill no longer contains the prior identity-specific redaction rule, business-error retry limits, whitelist handling, region-error suppression, or blocked/pending-settlement mappings. Before accepting this deletion, confirm that these rules are now intentionally owned by the CLI or a different canonical reference. The passing tests do not prove behavioral parity because the corresponding assertions were removed in the same commit.

## Pre-merge branch health

The checks below were run on clean detached copies of each fetched remote tip.

| Branch | `cargo check` | `cargo check --tests` | Interpretation |
|---|---:|---:|---|
| contract `c7e071bb` | pass | pass | Tip compiles with its tests. |
| Elvis `e69c6fd0` | pass | fail | CLI compiles; test compilation has a pre-existing missing include: `skills/okx-ai/references/task-user-actions-publish.md`, referenced by `cli/tests/confirmation_form_contract.rs`. |

The Elvis test failure predates this merge and must not be counted as a merge regression.

## Change-set comparison

From the merge base:

| Side | Files | Insertions | Deletions | Dominant intent |
|---|---:|---:|---:|---|
| contract | 93 | 3,094 | 4,150 | Guide-driven execution, centralized buyer routing, subscription views, identity/service-list refinements, keyring hardening, new `okx-ai-v2` routing skill, identity-error reference removal. |
| Elvis | 59 | 4,883 | 3,805 | A2A v2 create/fund and subscription execution, EIP-3009 nonce hook, XMTP delivery, provider decision and parameter clarification, strict persisted-artifact intake, structured next-action output, recovered-task routing fix. |

File overlap is relatively small but sits exactly on the critical path:

- 71 files changed only on contract.
- 37 files changed only on Elvis.
- 22 files changed on both sides.
- 13 of those 22 overlap files have textual conflicts; the remaining 9 auto-merged but still require semantic review.

### Contract-only changes that should normally be retained

- New `skills/okx-ai-v2/` tree and routing skeleton.
- Identity CLI pagination/display updates and identity-reference cleanup.
- Forced encrypted-file keyring support.
- Guide-driven copy-trading implementation (`autotrade/guide.rs`, consent/tooling/executor changes).
- Guide-driven copy-trading docs and tests.
- Centralized buyer routing and subscription-view documentation.

### Elvis-only changes that should normally be retained

- `task/user/v2/create_and_fund.rs` and `create_subscription.rs` transaction paths.
- EIP-3009 create-and-fund example and signing-hook changes.
- Provider decision flow and `service-param-update`.
- XMTP/A2A session delivery and provider runtime binding changes.
- Strict deliverable-envelope validation and persisted-artifact-before-review rule.
- Subscription-open handling and authoritative buyer-acceptance flow.
- Structured `phase` / `decision` / `reason` / `nextAction` / `payload` output contract.

## Primary architectural decisions required

### D1 — Public creation API

Elvis uses a fixed-price v2 API: required `provider-agent-id`, exact decimal-string payment/service amounts, `description-summary`, `category-code`, `min-credit-score`, `visibility`, and `chain-id`.

Contract uses the older budget API plus a new Guide bundle: `budget`, `max-budget`, `currency`, `provider`, `payment-mode`, and optional `service-guide` / hash / consent.

Recommendation: keep the Elvis fixed-price v2 surface as the transaction contract, then add the Guide bundle as optional fields. Do not restore float budgets or the legacy `payment-mode` path unless backward compatibility is explicitly required.

### D2 — Subscription execution policy

Elvis persists platform-defined flags (`copy-trade`, `service-description`, `autotrade-*`, required fields and settings JSON). Contract replaces these with the provider's exact Guide plus a separately confirmed Guide Consent object.

Recommendation: treat the contract Guide + Consent model as the new authorization model, but port it into the Elvis v2 subscription executor. If compatibility is required, old `autotrade-*` inputs should be a clearly deprecated adapter that produces Guide Consent—not a second independent policy source.

### D3 — Create flow implementation

Elvis delegates to v2 helpers that return a receipt containing the backend job ID, attachment persistence, broadcast metadata and runtime binding. Contract implements the older create / sign / broadcast sequence inline, with duplicate checks, funding blocks, Guide prepare/activate/rollback, and provider prebinding.

Recommendation: retain a single v2 executor. Move the contract-side safety stages around that executor:

1. validate Guide + Consent before remote write;
2. keep the duplicate-subscription race check if the backend does not guarantee idempotency;
3. prepare Guide storage after the real job ID exists but before broadcast;
4. rollback prepared Guide and provider prebind if broadcast fails;
5. activate Guide Consent only after broadcast succeeds.

This likely requires extending the v2 helper callback/hook boundary; pasting the inline legacy flow beside it would duplicate remote creation.

### D4 — Output contract

Elvis returns a structured state-machine envelope and a `watch_task` next action. Contract emits a flatter success object with `guidance`, optional text mode, and a `[Watch]` handoff.

Recommendation: preserve the Elvis structured envelope as canonical JSON. Add `guideStatus`, `consentStatus`, `offlineReplaySupported`, and fix commands inside `payload`. If human text output remains supported, derive it from the same receipt instead of maintaining a second control-flow contract.

### D5 — `sub_created` ownership — resolved

The reviewed lifecycle has no `sub_open` event and no second subscription ASP acceptance. `sub_created` is the User-side subscription-start event; `sub_asp_selected` is the ASP-side service-start event.

Resolution: move Elvis's session establishment/restoration and best-effort attachment forwarding into `sub_created`, then use authoritative subscription detail for the fixed trial/non-trial success copy. Keep the contract prohibition on DApp rescanning or implicit installation. Guide/Consent applies only to later Buyer signal intake.

### D6 — Delivery idempotency and trading authorization

Elvis requires `tradeRecordsV1`, exact `(jobId, deliveryId)` replay checks, terminal record persistence, active `mode=auto`, and direct claim/finalize. Contract requires active Guide + matching Consent + saved Signal, fail-closed Guide evaluation, and direct claim/finalize.

Recommendation: combine both safety layers. Guide + Consent decides authorization and parameters; trade-record query plus direct claim/finalize provides idempotency. Remove the legacy wrapper prompt, but do not drop Elvis's persisted replay protection.

## Textual conflict inventory

Line numbers below refer to this unresolved review worktree and will shift after edits. `HEAD`/ours is Elvis; `origin/codex/a2a-skill-cli-contract`/theirs is contract.

### 1. `cli/src/commands/agent_commerce/mod.rs` — 6 hunks

- `121-145`: `create-task` tail fields. Elvis exact service values/category/visibility/chain versus contract Guide bundle and optional service amounts.
- `171-191`: `create-subscribe` fields. Elvis required provider/copy-trade/service-description versus contract optional provider/Guide/hash/consent.
- `1449-1459`, `1474-1483`: destructuring and forwarding of `create-task` fields; must mirror the final `TaskCommand::Create` exactly.
- `1501-1508`, `1524-1531`: destructuring and forwarding of subscription fields; must mirror final `TaskCommand::CreateSubscribe` exactly.

Resolution note: this file duplicates the public command surface that also exists in `task/user/mod.rs`; resolve both together or Clap and internal dispatch will diverge.

### 2. `cli/src/commands/agent_commerce/task/common/mod.rs` — 1 hunk

- `548-576`: Elvis adds optional backend `--service-id` filtering; contract forces `--page 1 --page-size 100` for service listing.

Recommended resolution: additive. Build one command containing agent ID, pagination, and the optional service-ID filter. This is low risk, but add a test for a service beyond the first backend default page.

### 3. `cli/src/commands/agent_commerce/task/user/create.rs` — 10 hunks

- `3-9`: imports needed by Guide parsing and stderr writing.
- `36-51`: parameter model: exact required v2 service fields versus optional legacy service fields plus Guide bundle.
- `56-67`: validated exact token/visibility versus parsed Guide Consent state.
- `135-166`: v2 decimal/chain/credit/attachment validation versus legacy currency/title/Guide validation.
- `171-216`: Elvis exact decimal validator versus contract Guide prepare/persist helper. Both functions are needed; this is a positional conflict, not an either/or choice.
- `289-328`: mutually exclusive create implementations: Elvis `v2::execute` versus legacy inline backend create + attachment + provider + Guide preparation + broadcast.
- `351-382`: v2 receipt extraction versus Guide/prebind rollback and post-broadcast activation.
- `391-418`: audit schema: exact v2 payment/biz type versus legacy budget/payment mode plus Guide state.
- `424-486`: structured next-action receipt versus flat guidance/text-oriented response.
- `594-609`: test fixture fields reflect the two incompatible public APIs.

Hidden conflict: methods for `service_guide`, `provider`, `currency`, `budget`, and other contract fields were auto-merged outside markers, while the merged struct begins with Elvis fields. Choosing one side's marked blocks alone cannot type-check.

### 4. `cli/src/commands/agent_commerce/task/user/create_subscribe.rs` — 20 hunks

- `11-21`: imports; contract needs `OfflineReplayCapability`, `common`, `Context`, Guide collections and I/O support.
- `38-49`: required provider + service description versus optional provider + Guide bundle.
- `75-109`: Elvis validation signature versus contract Guide parsing/Consent validation and hidden create-time device guard.
- `143-178`: Elvis attachment/autotrade-policy validation versus contract attachment/Guide validation.
- `203-295`: contract-only helpers for Guide activation, canonical create body, success payload, and duplicate-subscription block.
- `302-316`: Elvis requires non-empty `sessionCert`; contract initializes JSON mode and refreshes tokens. Both checks/initialization are needed for the v2 path.
- `324-468`: Elvis v2 subscription executor versus contract duplicate race check, funding check, EIP-712 terms, backend create, Guide preparation, provider prebind and broadcast.
- `502-536`: v2 receipt/offline capability versus Guide rollback/activation.
- `549-557`: audit fields: Elvis copy-trade/autotrade/biz type versus Guide/Consent state.
- `585-640`: structured v2 output versus JSON/text output and Watch handoff.
- `663-681`, `694-701`, `714-723`: Clap fixture, field destructuring and assertions for the two public APIs.
- `770-821`: contract duplicate-subscription restoration tests versus no corresponding Elvis block.
- `857-881`, `912-961`: incompatible CLI parsing tests, especially provider requirement and removed options.
- `1031-1039`: fixture uses `String` provider/service-description versus `Option<String>` provider/Guide fields.
- `1064-1214`: Elvis copy-trade acceptance test versus contract routing, offline replay, funding block, removed-copy-trade and Guide tests.
- `1222-1263`: old `autotrade-*` CLI fixture versus Guide + Consent fixture.
- `1291-1523`: 229 lines of Elvis dynamic autotrade-policy tests collide with the contract Guide activation test.

Hidden conflict: Git auto-combined Guide methods with Elvis fields and also retained `copy_trade` while contract tests expect it removed. Neither full-side choice compiles. This file should be reconstructed around the chosen v2 + Guide lifecycle instead of resolved hunk-by-hunk.

### 5. `cli/src/commands/agent_commerce/task/user/flow_lifecycle/core.rs` — 2 hunks

- `308-325`: Elvis legacy subscription-signal prompt versus contract deletion. Remove the legacy prompt if Guide-driven execution is authoritative.
- `332-341`: Elvis replay/consentSnapshot/direct-tool rules versus contract Guide + Consent + Signal rules.

Recommended resolution: keep Guide evaluation and Elvis trade-record replay protection together, followed by one direct claim, one money-moving call, and one finalize. Preserve fail-closed behavior when any persisted artifact is missing.

### 6. `cli/src/commands/agent_commerce/task/user/flow_lifecycle/manage.rs` — 1 hunk

- `292-313`: generated create-subscribe command uses Elvis service/autotrade flags versus contract Guide bundle.

Resolve only after D2 and the final Clap shape are approved.

### 7. `cli/src/commands/agent_commerce/task/user/flow_lifecycle/subscription.rs` — 1 hunk

- `120-132`: Elvis comment assumes readiness was surfaced at `asp-match`; contract appends `session_block` and states Guide controls later handling.

Resolved: `sub_created` owns the required A2A session and attachment work. `sub_open` was removed from the state machine, dispatch, command surface, tests, and docs.

### 8. `cli/src/commands/agent_commerce/task/user/mod.rs` — 7 hunks

- `110-131`: internal `TaskCommand::Create` fields mirror the public create conflict.
- `163-229`: internal subscription fields; Elvis has provider/copy-trade/service-description plus detailed `autotrade-*`, while contract uses optional provider/Guide/Consent and retains device compatibility fields.
- `1922-1931`, `1947-1956`: create destructuring and `CreateTaskParams` construction.
- `1971-1978`, `1981-1985`, `2006-2012`: subscription destructuring and parameter construction, including contract `exclude_device`/attachments positions.

Resolution note: resolve this in lockstep with `agent_commerce/mod.rs`, `create.rs`, and `create_subscribe.rs`. Add compile-time/CLI tests that assert both command enums expose the same field contract.

### 9. `skills/okx-ai/references/task-action-routing.md` — 1 hunk

- `13-19`: Elvis adds `send_task_params_response` and `watch_task`; contract reduces `open_create_playbook` responsibility.

Recommended resolution: keep all three routes, but update whether `open_create_playbook` is a confirmation-only or post-confirmation step to match the final creation state machine.

### 10. `skills/okx-ai/references/task-cli-reference.md` — 4 hunks

- `168-186`, `198-213`: incompatible `create-task` syntax and field tables.
- `627-642`, `658-678`: incompatible `create-subscribe` syntax and authorization-field tables.

Do not resolve documentation before code. The final reference must describe exactly one canonical public interface and explicitly document any compatibility adapter.

### 11. `skills/okx-ai/references/task-output-templates.md` — 1 hunk

- `128-169`: Elvis adds state-machine meanings for `task_create_prepare`, create broadcast receipts, A2MCP routing, and phase mapping; contract deletes the section while centralizing routing elsewhere.

Recommendation: retain the semantics but move duplicated routing detail to the new canonical routing reference. Tests should ensure links/actions remain valid.

### 12. `skills/okx-ai/references/task-user-actions-create.md` — 2 hunks

- `183-210`: fixed-price v2 create + structured watch versus legacy budget/payment-mode create + Guide bundle + guidance Watch.
- `240-253`: Elvis v2 subscription payload/autotrade status interpretation versus contract Guide-bundle rule.

Recommendation: combine the v2 receipt/watch contract with the Guide-bundle rule after the CLI surface is finalized.

### 13. `skills/okx-ai/references/task-user-playbook.md` — 2 hunks

- `17-66`: Elvis strict deliverable intake, out-of-order `job_submitted`, and user-intent routing section versus contract deletion caused by centralized routing.
- `130-134`: resolved by assigning session/attachment behavior to `sub_created` and retaining no-rescan/no-install behavior.

Recommendation: keep the strict deliverable security contract. De-duplicate the intent-routing table if the central router is canonical, but retain a link and the fail-closed intake rules. Resolve the event ownership explicitly per D5.

## Hidden semantic conflicts found by whole-side simulation

### If all marked hunks select Elvis (`--ours`)

Compilation still fails with 3 errors:

1. `flow_lifecycle/core.rs`: calls `config::subscription_trade_path`, but contract's auto-merged config no longer provides it.
2. `task/user/mod.rs`: Elvis test constructs `ConsentSnapshot` without contract's new required `guide_hash`.
3. `audit.rs`: match is non-exhaustive for Elvis-retained `AutotradeExecute`, `SubscriptionRouteSet`, and `SubscriptionRouteClear` variants because contract removed their audit arms.

These indicate the old route/wrapper command family must be explicitly retained or fully removed across config, command enum, audit, tests, and docs.

### If all marked hunks select contract (`--theirs`)

Compilation fails with 15 errors, including:

1. `common/mod.rs` declares `version_notice`, but Elvis deleted the source file.
2. New after `e69c6fd0`: `task/user/mod.rs` re-exports `route_subscription_delivery_to_skill`, but Elvis's auto-merged `flow_lifecycle/mod.rs` no longer exports it.
3. Resolved: `sub_open` dispatch and lifecycle references were removed.
4. `agent_commerce/mod.rs` still references removed `TASK_MIN_VERSION`.
5. Provider delivery dispatch still supplies Elvis `message` and `autotrade` fields that the selected command variant lacks.
6. `audit.rs` references Elvis-only `ServiceParamUpdate`, provider accept/decline and subscription accept/decline variants that the selected enum drops.
7. `asp_ops.rs` tests expect Elvis v2 create fields, but selected `TaskCommand::Create` exposes the legacy optional service fields.

These are evidence that Elvis's provider lifecycle and v2 API cannot be discarded locally; their callers and tests span non-conflicted files.

## Auto-merged overlap files requiring manual semantic review

The following overlap files did not receive textual markers but sit on changed contracts:

- `cli/src/audit.rs`: command variants and event names; already implicated by whole-side compile failures.
- `task/common/autotrade/mod.rs`: legacy wrapper versus direct Guide execution exports.
- `task/common/config.rs`: route selection and minimum-version constants; already implicated by compile failures.
- `task/user/asp_ops.rs`: builds/parses create commands and has v2 field assertions.
- `task/user/flow.rs`: reviewed; obsolete `sub_open` dispatch removed.
- `skills/okx-ai/references/task-user-actions.md`.
- `skills/okx-ai/references/task-user-intent-routing.md`.
- `skills/okx-ai/references/task-user-sub-playbook.md`.
- `skills/okx-ai/references/watch-core.md`.

Also review Elvis-only files against the final API, particularly `v2/create_and_fund.rs`, `v2/create_subscription.rs`, `provider_decision.rs`, `service_param_update.rs`, `signing.rs`, and the XMTP/A2A binding files.

## Recommended resolution order

1. Approve D1–D6; especially the canonical public fields and Guide authorization model.
2. Rebuild `CreateTaskParams` and `CreateSubscribeParams` first, without touching docs.
3. Extend the v2 executor boundary for Guide prepare/rollback/activate and duplicate/funding checks.
4. Align both command enums and dispatch arms.
5. Reconcile lifecycle events (`sub_created`, `sub_asp_selected`, delivery intake, replay protection). (`sub_open` has been removed.)
6. Reconcile audit mappings and remove or retain old autotrade/route variants consistently.
7. Rebuild tests around the chosen contract; preserve tests from both sides where behavior remains applicable.
8. Fix the pre-existing missing Elvis test reference.
9. Update skill docs from the final CLI, then run the repository's CLI and skills refresh/verification commands.

## Current review state

- The primary review worktree is detached at Elvis tip and has the attempted merge in progress.
- Conflict markers are intentionally preserved for review.
- No source conflict has been resolved, staged as resolved, committed, or pushed.
- The user's original working tree and its uncommitted changes were not modified.
