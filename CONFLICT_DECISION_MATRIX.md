# Conflict decision matrix

Merge direction: `origin/codex/a2a-skill-cli-contract` into `origin/codex/a2a-skill-cli-contract-elvis`

- **Elvis / ours / HEAD:** `e69c6fd05c83ac1090d8f4c9d4e883666ef3eda8`
- **Contract / theirs:** `c7e071bb58b906a51498be105163f739e387d6df`
- **Merge base:** `6ff8fc6b093f04cfe80c66248f7cf8f3bb7c39e9`
- **Textual conflicts:** 13 files, 58 hunks

## Final resolution status (2026-09-04)

All 58 textual conflict hunks are resolved and there are no unmerged files. No merge commit has been created and nothing has been pushed. The C01–C58 entries below remain as the pre-resolution logic audit.

- Keep Elvis fixed-price v2 `create-task` / `create-subscribe`, extended with Contract `serviceGuide`, optional hash, and `guideConsentJson`.
- Keep subscription provider required.
- Guide + Consent is the sole execution authority; creation no longer accepts `copyTrade`, service-description authorization, or fixed-field `autotrade-*` settings.
- After v2 obtains the real `jobId`, persist Guide and prepared Consent before broadcast; activate after success and abort prepared Consent on failure or unknown receipt.
- Keep the structured result envelope with `guideStatus`, `consentStatus`, and `executionProfileSaved`.
- There is no `sub_open`: User handles session/attachments on `sub_created`; ASP starts directly on `sub_asp_selected`, with no second acceptance.
- Keep both Guide eligibility and trade-record/direct-claim/direct-finalize replay protection.

## How to review this matrix

Each item is classified as:

- **Primary:** an actual product/protocol decision is required.
- **Combined:** both branches contain independent useful behavior and should normally be integrated.
- **Mechanical:** no independent decision; it must follow another primary decision exactly.
- **Test/docs:** must be rebuilt from the chosen runtime contract rather than selected blindly.

Suggested response format: `C01 combined: keep v2 fields + Guide bundle`, `C19 Contract`, and so on. Several mechanical items can be approved as a group once their parent decision is made.

## A. Public command surface — `cli/src/commands/agent_commerce/mod.rs`

### C01 — lines 121–145 — `create-task` tail fields — Primary

**Elvis logic**

- `service_token_amount` is required `String`, preserving the exact decimal rather than parsing through floating point.
- Adds `category_code`, `min_credit_score`, semantic `visibility`, and X Layer `chain_id`.
- This is part of the v2 fixed-price create-and-fund contract. Adjacent fields already use `provider_agent_id`, `payment_token_symbol`, and `payment_token_amount`.

**Contract logic**

- Makes `service_token_amount` optional.
- Adds `service_guide`, optional Guide hash, and explicit Guide Consent JSON.
- Accepts a hidden compatibility `agentId`/`agent-id` argument but ignores it because the user identity is resolved automatically.

**Effect of choosing Elvis only**

- Keeps the v2 transaction fields but exposes no Guide bundle, while auto-merged Guide methods elsewhere still expect those fields. It will not compile without removing all Guide logic.

**Effect of choosing Contract only**

- Produces a hybrid API: adjacent fields remain Elvis v2 fields, while the tail switches to optional legacy service values. It does not recreate the complete Contract API and is internally inconsistent.

**Real decision**

- Decide whether fixed-price v2 is canonical and whether Guide + Consent is added to it.
- Suggested combined shape: retain all Elvis v2 fields, add the three optional Guide fields, and omit the ignored compatibility `agentId` unless external callers require it.

**Dependencies:** C03, C04, C09–C17, C42, C44, C45, C50, C51, C55.

### C02 — lines 171–191 — `create-subscribe` provider and execution fields — Primary

**Elvis logic**

- Requires a designated `provider_agent_id`.
- Keeps the explicit `copy_trade` marker.
- Stores `service_description` only as bounded routing/tool hints; raw description is not executable authorization.

**Contract logic**

- Provider is optional and may be inferred from the service.
- Removes `copy_trade` and `service_description` authorization/routing inputs.
- Uses the exact provider Guide, hash, and user-confirmed Guide Consent as the execution contract.

**Decision consequences**

- Elvis selects a platform-defined autotrade configuration model.
- Contract selects a provider-defined Guide model.
- Keeping both as independent authorization sources is dangerous: they can disagree over whether a trade is permitted.

**Suggested resolution**

- Select one authorization source. If Guide is canonical, retain `service_description` only for display/search metadata, never authorization; decide separately whether provider designation must remain mandatory for the v2 backend.

**Dependencies:** C05, C06, C19–C37, C43, C46–C48, C52, C53, C56.

### C03 — lines 1449–1459 — public `CreateTask` destructuring — Mechanical

**Elvis logic:** extracts `category_code`, `min_credit_score`, `visibility`, `chain_id`.

**Contract logic:** extracts Guide fields and discards hidden `_agent_id`.

**Required outcome:** destructure the exact final field set chosen in C01. If C01 is combined, extract both groups. There is no separate business choice here.

### C04 — lines 1474–1483 — forwarding into internal `TaskCommand::Create` — Mechanical

**Elvis logic:** forwards v2 category/credit/visibility/chain.

**Contract logic:** forwards Guide/hash/Consent.

**Required outcome:** must match C01, C03, C42 and C44 exactly. A missing forwarded field silently makes the public flag ineffective or causes compilation failure.

### C05 — lines 1501–1508 — public `CreateSubscribe` destructuring — Mechanical

**Elvis logic:** extracts `copy_trade` and `service_description`.

**Contract logic:** extracts Guide/hash/Consent.

**Required outcome:** follow C02. If `service_description` remains metadata and Guide is authorization, both groups may be forwarded, but `copy_trade` must not independently enable execution.

### C06 — lines 1524–1531 — forwarding into internal subscription command — Mechanical

**Elvis logic:** forwards `copy_trade` and `service_description`.

**Contract logic:** forwards Guide/hash/Consent.

**Required outcome:** mirror C02, C05, C43 and C46. This is the second copy of the same command contract.

## B. Shared service lookup — `cli/src/commands/agent_commerce/task/common/mod.rs`

### C07 — lines 548–576 — service-list subprocess arguments — Combined

**Elvis logic**

- Introduces `spawn_service_list_filtered(agent_id, service_id)`.
- Adds backend-side `--service-id` filtering when supplied.
- Leaves pagination unspecified, so behavior depends on service-list defaults.

**Contract logic**

- Always requests `--page 1 --page-size 100`.
- Has no optional service-ID filter in this block.

**Suggested resolution**

- Keep Elvis's helper and append Contract's page/page-size arguments before the optional service-ID argument.
- This preserves exact lookup and avoids missing a service outside a small default page.

**Risk to confirm:** if more than 100 services are allowed and `--service-id` filtering is unavailable on an older backend, this still needs pagination rather than a single page.

## C. One-time task creation — `task/user/create.rs`

### C08 — lines 3–9 — imports — Combined/Mechanical

**Elvis logic:** needs only `bail` and `Result` for the v2 path.

**Contract logic:** additionally needs `Context`, `BTreeMap`, and `Write` for Guide JSON validation and progress output.

**Required outcome:** retain imports actually used by the final implementation. If Guide + Consent is retained, `Context` and `BTreeMap` remain necessary. `Write` is only needed if the old inline stderr path remains.

### C09 — lines 36–51 — `CreateTaskParams` data model — Primary

**Elvis logic**

- `service_params`, token address, and token amount are required strings.
- Exact strings are passed into `v2::CreateAndFundInput`.
- Includes category, minimum credit score, visibility, and chain ID.

**Contract logic**

- Service fields are optional.
- Adds Guide/hash/Consent fields.
- Assumes the legacy body builder, which can omit service price/address.

**Decision consequences**

- This determines whether service selection is already authoritative before creation.
- Optional price/address permits a broader legacy task, while the Elvis flow is specifically a confirmed fixed-price service purchase.

**Suggested resolution:** keep required v2 service fields and add optional Guide fields. If generic non-service task creation must remain supported, expose it as a distinct mode/command rather than ambiguous optionality in the funded-service path.

### C10 — lines 56–67 — validated state representation — Combined

**Elvis logic:** stores normalized token symbol and numeric visibility.

**Contract logic:** stores parsed `GuideConsentInput`, whose Guide draft and Consent values have already been validated.

**Suggested resolution:** `ValidatedParams` should contain all three: sanitized title, normalized token symbol/visibility, and optional validated Guide Consent. These values serve independent purposes.

### C11 — lines 135–166 — `CreateTaskParams::validate` result — Combined

**Elvis logic**

- Validates payment and service decimal strings without floating-point ambiguity.
- Restricts chain to X Layer.
- Checks credit score range, visibility mapping, and attachment sources.
- Sanitizes title for the v2 call.

**Contract logic**

- Returns normalized legacy currency/title plus parsed Guide Consent.
- Relies on earlier legacy budget/payment-mode validation in the Contract version.

**Suggested resolution:** preserve Elvis validation and add `validated_guide_consent()`. Do not restore float-budget validation unless C01 explicitly chooses the legacy creation contract.

### C12 — lines 171–216 — decimal validator versus Guide persistence helper — Combined

This conflict is caused by both branches inserting different functions at the same location.

**Elvis logic:** `validate_decimal_amount` rejects signs, exponent notation, empty fractions, non-digits, and more than six decimals.

**Contract logic:** `prepare_guide_consent` writes the exact Guide and a prepared Consent record under the real job ID before broadcast.

**Required outcome:** keep both functions if C01 combines v2 and Guide. They do not contradict each other.

### C13 — lines 289–328 — actual remote creation pipeline — Primary/Critical

**Elvis logic**

- Calls `v2::execute` once with `CreateAndFundInput`.
- The v2 helper owns create-and-fund, attachment handling, runtime binding, signing/broadcast, and returns a receipt.
- Uses exact payment strings and the v2 business type.

**Contract logic**

- Builds and posts the legacy `/task/create` request inline.
- Copies attachments after obtaining job ID.
- Saves designated provider and prepares Guide Consent before broadcast.
- Prebinds provider runtime and directly calls `sign_uop_and_broadcast`.

**Unsafe resolution:** keeping both paths would create the same task twice.

**Real decision:** choose a single owner for remote creation. Suggested: retain `v2::execute`, then extend its hook/callback boundary so Guide preparation occurs after job ID allocation but before broadcast.

### C14 — lines 351–382 — broadcast success/failure lifecycle — Primary/Critical

**Elvis logic:** awaits the v2 receipt and reads `receipt.broadcast.txHash`, defaulting to `pending`.

**Contract logic**

- On broadcast error, aborts prepared Guide Consent and rolls back provider prebind.
- After success, activates prepared Guide Consent.
- Activation failure is reported but leaves the remote task created and execution unavailable.

**Suggested resolution:** retain receipt extraction but add Contract rollback/activation semantics inside or immediately around the v2 executor. Confirm whether missing `txHash` is acceptable as `pending` or should be a structured unknown state.

### C15 — lines 391–418 — audit schema — Combined/Testable

**Elvis logic:** audits exact payment symbol/amount, designated provider, and `bizType=201`.

**Contract logic:** audits legacy currency/budget/maxBudget/paymentMode and Guide/Consent activation state.

**Required outcome:** audit only fields that exist in the final public contract. For v2 + Guide, keep Elvis payment/biz fields and add Guide/Consent state; do not fabricate legacy budget/payment-mode values.

### C16 — lines 424–486 — success/output contract — Primary

**Elvis logic**

- Emits structured `phase=creation`, `decision=ready`, `reason=broadcast_submitted`.
- Returns `nextAction=[watch_task]`.
- Puts job ID, types, provider, payment, runtime binding, attachments, and complete broadcast receipt under `payload`.

**Contract logic**

- Builds human guidance text and a flatter JSON object with job ID, tx hash, Guide and Consent status.
- Adds a scoped `[Watch]` handoff in CLI mode.
- Explicitly warns not to call `set-payment-mode`.

**Decision consequences:** agents consuming `nextAction` and agents consuming `data.guidance` follow different orchestration contracts.

**Suggested resolution:** keep one structured envelope; add Guide status and any human guidance under `payload` or a clearly non-authoritative display field. Keep `watch_task` as the machine action.

### C17 — lines 594–609 — unit-test fixture — Test/Mechanical

**Elvis logic:** fixture exercises fixed-price strings, category, score, visibility and chain.

**Contract logic:** fixture exercises optional service values and absent Guide bundle.

**Required outcome:** rebuild fixture from C09. If Guide is optional, add separate tests for no Guide, valid Guide, hash mismatch and missing Consent rather than overloading one fixture.

## D. Subscription creation — `task/user/create_subscribe.rs`

### C18 — lines 11–21 — imports — Combined/Mechanical

**Elvis logic:** imports A2A capability module, subscription identity selector and debug flag.

**Contract logic:** additionally imports `OfflineReplayCapability` and the whole `common` namespace used by prebinding.

**Required outcome:** retain all imports needed by the chosen combined flow; formatting differences are irrelevant.

### C19 — lines 38–49 — subscription parameter model — Primary

**Elvis logic:** provider is mandatory `String`; `service_description` is stored as bounded routing hints.

**Contract logic:** provider is optional; Guide/hash/Consent are execution inputs.

**Decision to make**

- Is designated provider required by the v2 subscription backend?
- Is Guide the sole trading authorization?
- Is service description retained only for display/tool discovery?

**Suggested combined model:** keep provider required if v2 requires it; keep description as non-authorizing metadata; add optional Guide bundle as the only automatic-execution policy.

### C20 — lines 75–109 — validation return type and Guide parsing — Primary

**Elvis logic:** `validate()` returns `Result<()>`; no Guide object is produced.

**Contract logic**

- Parses Consent JSON into a map.
- Requires Guide whenever hash or Consent is supplied.
- Requires explicit Consent JSON, including `{}` for a Guide with no fields.
- Rejects credential-like Consent fields via `validate_consent_values`.
- Returns `Result<Option<GuideConsentInput>>`.
- Rejects create-time `exclude_device` while preserving the flag for a specific compatibility error.

**Suggested resolution:** use the Contract return type and Guide validation if Guide is canonical, while retaining Elvis's other validation checks. Decide whether `exclude-device` must remain parseable for backward-compatible error messaging.

### C21 — lines 143–178 — old autotrade validation versus Guide validation — Primary

**Elvis logic**

- Validates attachments through the shared hardened helper.
- Requires explicit mode when execution fields exist.
- Rejects automatic settings with notify-only mode.
- Validates every declared required autotrade field.

**Contract logic**

- Only checks attachment paths exist directly.
- Returns parsed/validated Guide Consent.
- Removes the entire platform-defined `autotrade-*` policy model.

**Decision consequences:** this chooses the authorization schema, not merely validation style.

**Suggested resolution:** use shared attachment validation plus Guide Consent validation. Retain old autotrade validation only if old flags remain as an explicit compatibility adapter.

### C22 — lines 203–295 — Contract helper suite — Combined, subject to executor choice

**Elvis logic:** no code at this location because the v2 executor owns much of the work.

**Contract logic adds:**

- Guide Consent activation helper.
- Legacy subscription create-body builder with `deviceList: null`.
- Success-envelope builder with Guide/Consent and offline replay fields.
- Duplicate-subscription error payload, offering restore-listening only when valid.

**Required judgment**

- Keep Guide activation and duplicate-block construction.
- Preserve offline-replay capability fields, but merge them into the selected structured output.
- Do not keep the legacy create-body helper if v2 owns remote creation; instead ensure v2 sends the same authoritative terms/device routing.

### C23 — lines 302–316 — login/session preflight — Combined

**Elvis logic:** refreshes tokens, then requires non-empty `sessionCert` because subsequent A2A/v2 operations need it.

**Contract logic:** derives `json_mode`, then refreshes tokens; it does not enforce `sessionCert` here.

**Suggested resolution:** initialize output mode and retain the session certificate gate if any post-create A2A operation requires it. Session creation now belongs to `sub_created`; verify whether create itself truly needs the certificate before keeping this as a hard block.

### C24 — lines 324–468 — subscription remote-write pipeline — Primary/Critical

**Elvis logic:** resolves wallet and delegates once to `v2::execute_create_subscription`.

**Contract logic**

- Rechecks duplicate subscription immediately before write to close confirmation/create races.
- Performs funding sufficiency check and returns a structured funding block.
- Fetches authoritative provider renewal terms.
- Signs `typedData` with EIP-712.
- Uses backend-returned `useTrial` as authoritative.
- Posts create, copies attachments, prepares Guide Consent and provider prebind, then signs/broadcasts.

**Important history:** Elvis commit `81a2de7c` intentionally removed duplicate/balance checks from this function and kept them in prepare. Contract intentionally reintroduced the duplicate check to close a race.

**Decision required:** whether prepare-time checking is enough. Suggested safety position: retain a final duplicate/idempotency check before write, but integrate it into the v2 executor so the remote create happens only once.

### C25 — lines 502–536 — post-broadcast processing — Combined/Critical

**Elvis logic:** reads offline-replay capability and tx hash from the v2 receipt.

**Contract logic:** rolls back Guide/prebind on failed broadcast and activates Guide Consent only after success.

**Suggested resolution:** both. Capability probing must remain copy-only and run after successful create; Guide rollback/activation must be tied to the actual v2 broadcast result.

### C26 — lines 549–557 — subscription audit fields — Primary/Combined

**Elvis logic:** records `copyTrade`, whether autotrade config was requested/configured, and `bizType=204`.

**Contract logic:** records Guide and Consent status.

**Required outcome:** audit final truth. If Guide replaces old autotrade flags, retain `bizType=204`, drop misleading copyTrade/config fields, and add Guide/Consent status. If a compatibility adapter exists, audit both input mode and normalized Guide state distinctly.

### C27 — lines 585–640 — subscription success/output behavior — Primary

**Elvis logic:** always emits the structured creation state with `watch_task` and a rich v2 `payload`.

**Contract logic**

- JSON mode returns flat subscription success fields and offline-replay capability.
- Text mode prints human status.
- CLI mode emits scoped Watch handoff.

**Suggested resolution:** same policy as C16. Prefer one structured result containing v2 receipt, Guide status and offline replay information, with a machine-readable `watch_task`. Text rendering may wrap that data but should not define a second state machine.

### C28 — lines 663–681 — default CLI parse fixture input — Test/Primary dependency

**Elvis logic:** supplies required provider ID.

**Contract logic:** omits provider to verify it is optional.

**Decision:** this test follows C19. If provider remains required, keep Elvis input and add a rejection test for omission. If optional, preserve the Contract test and verify provider inference separately.

### C29 — lines 694–701 — parsed field destructuring in test — Test/Mechanical

**Elvis logic:** reads `copy_trade` and `service_description`.

**Contract logic:** reads Guide/hash/Consent.

**Required outcome:** mirror the final command fields from C19/C21.

### C30 — lines 714–723 — default-value assertions — Test/Mechanical

**Elvis logic:** expects required provider, default copyTrade 0 and empty service description.

**Contract logic:** expects optional provider and empty Guide bundle.

**Required outcome:** rebuild assertions from C19. If description remains metadata and Guide is optional, both corresponding defaults can be asserted; provider optionality remains a product decision.

### C31 — lines 770–821 — duplicate-subscription response tests — Combined/Safety

**Elvis logic:** no test here because create-time duplicate handling was removed.

**Contract logic:** verifies duplicate response hides raw status, names existing job, and offers restore-listening only for an active/restorable subscription.

**Suggested resolution:** retain these tests if any final pre-write duplicate check exists. They encode safe user-visible behavior independently of whether the check lives in v2 or this module.

### C32 — lines 857–881 — boolean parsing fixture — Test/Primary dependency

**Elvis logic:** supplies required provider ID while testing `--auto-renew true`.

**Contract logic:** omits provider.

**Required outcome:** follows C19; the boolean behavior itself is the same.

### C33 — lines 912–961 — `--exclude-device` compatibility behavior — Primary/Compatibility

**Elvis logic:** the option is removed from Clap, so parsing fails immediately.

**Contract logic:** keeps the legacy flag hidden/parseable, then returns a specific local error explaining that device selection must happen after creation.

**Decision consequences:** both reject the behavior, but differ in user experience and compatibility.

**Suggested resolution:** keep Contract's parse-then-specific-error if old clients still send this flag; otherwise remove it completely and update docs/tests.

### C34 — lines 1031–1039 — test fixture provider/Guide fields — Test/Mechanical

**Elvis logic:** converts absent provider to empty required string and initializes empty service description.

**Contract logic:** keeps provider as `Option` and initializes absent Guide bundle.

**Required outcome:** mirror C19. Avoid representing “missing required provider” as an empty valid value unless validation explicitly rejects it.

### C35 — lines 1064–1214 — old copy-trade test versus Contract helper tests — Test suite composition

**Elvis logic:** verifies `--copy-trade` is accepted.

**Contract logic verifies:**

- default all-device routing;
- optional provider serialization;
- offline replay fields and upgrade commands;
- funding-block protocol;
- `--copy-trade` is rejected;
- retired delivery marker is omitted.

**Required outcome:** do not choose this as one block. Preserve Contract's routing/offline/funding tests if those behaviors survive. The copy-trade acceptance/rejection assertion follows C02/C21.

### C36 — lines 1222–1263 — automatic-execution CLI fixture — Primary/Test

**Elvis logic:** supplies explicit platform fields: mode, amount, cap, quote, environment, margin mode, order policy, auth mode and required-field declarations.

**Contract logic:** supplies exact service Guide plus user-confirmed Consent JSON.

**Decision:** this is the clearest executable test of the two competing authorization models. Select Guide if it is canonical. If backward compatibility is required, add a separate adapter test showing old fields normalize into the new model; never allow both sources to disagree silently.

### C37 — lines 1291–1523 — old autotrade validation tests versus Guide activation test — Test reconstruction

**Elvis logic:** contains a large suite for required dynamic fields, aliases, fixed amount basis, cap metadata, environment normalization, manual→notify-only mapping and persistence of autotrade grants.

**Contract logic:** names a test asserting prepared Guide Consent becomes active only after broadcast.

**Merge artifact warning:** the body following this marker is the Contract Guide test. Selecting Elvis here would attach that body to the Elvis test name and leave a logically invalid test file.

**Required outcome:** reconstruct tests manually. Retain Guide prepare/activate and explicit-Consent tests. Preserve only old-field tests that correspond to an intentionally supported compatibility adapter.

## E. Runtime lifecycle prompts

### C38 — `flow_lifecycle/core.rs` lines 308–325 — legacy execution prompt — Primary/Cleanup

**Elvis logic:** retains a large legacy signal-routing prompt based on `task-subscription-signal.md`, consent snapshots and the old `autotrade-execute` gateway.

**Contract logic:** deletes it because Guide-driven direct execution replaces the legacy wrapper.

**Suggested resolution:** delete the legacy prompt if the corresponding wrapper and old policy model are retired. Keeping an unreachable prompt still creates maintenance/security drift; keeping it reachable contradicts the Contract guide model.

### C39 — `flow_lifecycle/core.rs` lines 332–341 — direct execution safety rules — Combined/Critical

**Elvis logic**

- Requires `tradeRecordsV1` capability.
- Queries exact `(jobId, deliveryId)` before moving money.
- Persists terminal result and never retries if persistence fails.
- Requires active `mode=auto` consent and maps typed dynamic settings.
- Uses direct claim/finalize around one tool call.

**Contract logic**

- Requires active matching Guide + Consent + saved Signal.
- Applies Guide conditions fail-closed.
- Treats Guide/Signal as data, never executable shell/script/URL/tool authorization.
- Uses direct claim with Guide-derived amount and one finalize.

**Suggested resolution:** combine Guide authorization with Elvis replay protection. Guide decides eligibility/parameters; trade-record query and claim/finalize enforce idempotency. Remove references to retired legacy fields.

### C40 — `flow_lifecycle/manage.rs` lines 292–313 — generated publish command — Mechanical after authorization decision

**Elvis logic:** generates `create-subscribe` using service params/description/provider plus all `autotrade-*` values and JSON output.

**Contract logic:** generates command using exact Guide/hash/provider/Consent.

**Required outcome:** match C19–C21 and C36 exactly. Also retain required non-conflicting arguments such as service params, service interval and `--format json` if the final command still supports them.

### C41 — `flow_lifecycle/subscription.rs` lines 120–132 — `sub_created` session block — Resolved

**Resolution:** there is no `sub_open`. `sub_created` establishes or restores the A2A session with the designated ASP and forwards every pending attachment on a best-effort basis; one attachment failure does not block the rest or the later notification.

**Additional rule retained:** no DApp rescan/tool inference/install occurs at `sub_created`; persisted Guide + Consent controls only later Buyer signal intake.

## F. Internal user command surface — `task/user/mod.rs`

### C42 — lines 110–131 — internal `TaskCommand::Create` fields — Mechanical mirror of C01

**Elvis logic:** required service amount plus category/score/visibility/chain.

**Contract logic:** optional service amount plus Guide/hash/Consent.

**Required outcome:** exactly mirror the public command and `CreateTaskParams`. Suggested combined choice follows C01/C09.

### C43 — lines 163–229 — internal subscription field model — Primary mirror of C02

**Elvis logic:** mandatory provider, copy-trade marker, service-description hints, service interval, full `autotrade-*` policy, and always-structured output comment.

**Contract logic:** optional provider, Guide/hash/Consent, service interval and text/JSON output distinction; old autotrade fields are removed.

**Required outcome:** decide the authorization model once, then make public enum, internal enum and params identical. Avoid leaving old fields in only one enum.

### C44 — lines 1922–1931 — internal create destructuring — Mechanical

**Elvis logic:** extracts category/score/visibility/chain.

**Contract logic:** extracts Guide/hash/Consent.

**Required outcome:** mirror C42.

### C45 — lines 1947–1956 — construction of `CreateTaskParams` — Mechanical

**Elvis logic:** supplies v2 metadata.

**Contract logic:** supplies Guide bundle.

**Required outcome:** mirror C09/C42. Every required struct field must be supplied exactly once.

### C46 — lines 1971–1978 — subscription destructuring authorization fields — Mechanical

**Elvis logic:** extracts copyTrade and service description.

**Contract logic:** extracts Guide bundle.

**Required outcome:** mirror C43 and C19.

### C47 — lines 1981–1985 — device/attachment destructuring placement — Mechanical/Compatibility

**Elvis logic:** empty side because Elvis already destructures attachments in its own command ordering and removed `exclude_device`.

**Contract logic:** destructures compatibility `exclude_device` and attachments here.

**Required outcome:** attachments must always survive. Include `exclude_device` only if C33 chooses parse-then-error compatibility. Do not duplicate the attachments binding.

### C48 — lines 2006–2012 — construction of `CreateSubscribeParams` tail — Mechanical

**Elvis logic:** supplies `autotrade_auth_mode` and settings JSON; other old autotrade fields appear immediately before this hunk.

**Contract logic:** supplies `exclude_device` and attachments; Guide fields appear earlier.

**Required outcome:** reconstruct the entire initializer from C19–C21/C33/C43. This hunk cannot be safely selected independently because the auto-merged surrounding initializer already contains fields from both models.

## G. Skill documentation conflicts

### C49 — `task-action-routing.md` lines 13–19 — action routes — Combined

**Elvis logic:**

- `open_create_playbook` is a Step-3/post-confirmation action.
- Adds `send_task_params_response` for provider clarification through A2A session.
- Adds `watch_task` as a scoped, continuing Watch action.

**Contract logic:** `open_create_playbook` simply delegates to the playbook and does not mark a step.

**Suggested resolution:** retain all Elvis-only actions because provider clarification and structured Watch depend on them. Rewrite `open_create_playbook` ownership to match the final centralized buyer router and avoid two components both performing confirmation.

### C50 — `task-cli-reference.md` lines 168–186 — documented `create-task` syntax — Primary/Docs

**Elvis logic:** documents fixed-price v2 fields, exact decimal strings, required provider/service price, optional category/score/visibility/chain.

**Contract logic:** documents legacy budget/max-budget/currency/provider/payment-mode plus optional service values and Guide bundle.

**Required outcome:** follows C01/C09. Do not publish a hybrid command example that the CLI cannot parse.

### C51 — `task-cli-reference.md` lines 198–213 — `create-task` field semantics — Primary/Docs

**Elvis logic:** service params default `{}`, service token address/amount required, and v2 metadata documented.

**Contract logic:** service values optional/natural-language and Guide bundle conditions documented.

**Required outcome:** document the final v2 requirements and add the Guide bundle rules if retained. Explicitly state whether service params is JSON or natural language; the branches currently disagree.

### C52 — `task-cli-reference.md` lines 627–642 — documented `create-subscribe` syntax — Primary/Docs

**Elvis logic:** required provider plus service description, optional old autotrade fields, interval, files.

**Contract logic:** optional provider plus mandatory Guide/Consent for guide-driven execution.

**Required outcome:** follows C19–C21 and C36. Separate “ordinary notification subscription” from “automatic Guide-driven subscription” if Guide is only conditionally required.

### C53 — `task-cli-reference.md` lines 658–678 — subscription field semantics — Primary/Docs

**Elvis logic:** documents every old policy field, typed `extra` structure, required-field mapping and compatibility alias.

**Contract logic:** documents provider inference and exact Guide storage/Consent rules.

**Required outcome:** retain only semantics supported by final code. If old flags are removed, delete their lengthy field contract rather than leaving dead instructions. If adapted, document normalization and precedence.

### C54 — `task-output-templates.md` lines 128–169 — structured phase mapping — Primary/Docs

**Elvis logic:** documents meanings of prepare/create responses, broadcast-submitted state, `watch_task`, A2MCP routing and the full phase/decision/action table.

**Contract logic:** deletes this block as part of centralized routing simplification.

**Decision consequences:** removing it without relocation loses the executable meaning of Elvis's structured responses.

**Suggested resolution:** keep one canonical phase mapping, possibly moved to the centralized router. Preserve action semantics and test links rather than duplicating prose across files.

### C55 — `task-user-actions-create.md` lines 183–210 — regular creation execution — Primary/Docs

**Elvis logic:** calls fixed-price v2 create, passes confirmed service context unchanged, avoids rechecking/confirming, then follows structured `watch_task` until `job_created`.

**Contract logic:** calls legacy payment-mode command, includes Guide bundle, follows CLI `data.guidance`, and enters Watch when a `[Watch]` block appears.

**Required outcome:** follows C13/C16/C50. Keep Guide bundle only if runtime accepts it; choose one authoritative watch mechanism.

### C56 — `task-user-actions-create.md` lines 240–253 — subscription post-create semantics versus Guide bundle rule — Combined

**Elvis logic:** explains structured v2 payload, requires types 204, defines configured/unconfigured autotrade flags, then executes `watch_task`.

**Contract logic:** says Guide/hash/Consent must be passed together only when serviceGuide is non-blank.

**Suggested resolution:** both concepts are useful after normalization: retain structured receipt/watch semantics and the atomic Guide-bundle rule. Replace old autotradeConfigured meanings with Guide/Consent status if old flags are retired.

### C57 — `task-user-playbook.md` lines 17–66 — deliverable intake and intent routing — Primary/Security

**Elvis logic:** adds strict raw A2A envelope intake, regular-file/0600/temp-path requirements, matching job/agent checks, mandatory persistence before review, out-of-order `job_submitted` handling, subscription admission, trade-record replay checks, and a user-intent routing table.

**Contract logic:** deletes the entire block while centralizing buyer routing elsewhere.

**Decision consequences:** intent-table duplication can be removed, but deleting the strict intake rules removes security and ordering invariants unless they are relocated.

**Suggested resolution:** retain or relocate every intake/security invariant. De-duplicate only the routing table and link to its canonical owner.

### C58 — `task-user-playbook.md` lines 130–134 — Watch handoff/event ownership — Resolved

**Resolution:** remove `sub_open`; move Elvis's session/attachment behavior into `sub_created`. `sub_created` validates authoritative Active state and renders the fixed trial/non-trial creation-success copy. It is not an ASP acceptance notice and performs no tool scan, installation, or Guide fallback.

**Dependencies:** C39, C41, C52, C53, C56.

## Cross-cutting decisions to approve before editing

### X1 — Creation API

- [ ] Elvis fixed-price v2
- [ ] Contract legacy budget/payment-mode
- [ ] Combined: v2 plus optional Guide bundle

Controls: C01, C03, C04, C09–C17, C42, C44, C45, C50, C51, C55.

### X2 — Subscription provider requirement

- [ ] Required designated provider
- [ ] Optional/inferred provider

Controls: C02, C19, C28, C30, C32, C34, C43, C46.

### X3 — Automatic-execution authorization

- [ ] Elvis platform `autotrade-*` settings
- [ ] Contract exact Guide + Consent
- [ ] Guide + Consent canonical, with old flags only as a deprecated adapter

Controls: C02, C19–C22, C26, C29, C30, C35–C40, C43, C46, C48, C52, C53, C56.

### X4 — Subscription remote-write owner

- [ ] Elvis v2 executor
- [ ] Contract inline legacy flow
- [ ] v2 executor extended with duplicate/funding/Guide lifecycle hooks

Controls: C22–C25, C27, C31.

### X5 — Machine output and Watch

- [ ] Elvis structured `nextAction`
- [ ] Contract flat data + guidance/Watch text
- [ ] Structured envelope canonical, optional derived display guidance

Controls: C16, C22, C27, C49, C54–C56.

### X6 — Subscription lifecycle event ownership — Resolved

- [x] There is no `sub_open`
- [x] `sub_created` establishes/restores the session, forwards attachments, uses authoritative detail, and renders fixed copy
- [x] `sub_asp_selected` starts ASP service directly, without a second acceptance

Controls: C41, C58.

### X7 — Replay and delivery safety

- [ ] Guide/Consent eligibility only
- [ ] Elvis trade-record checks only
- [ ] Both: Guide authorization plus trade-record/claim/finalize idempotency

Controls: C39, C57 and Elvis's new spool-retirement code outside textual conflicts.

## Non-textual conflicts that must be handled after C01–C58

These do not have `<<<<<<<` markers but the extreme-choice compilation checks proved they are incompatible:

1. `config::subscription_trade_path` is removed by Contract while Elvis direct prompt still calls it.
2. `ConsentSnapshot` gained required `guide_hash`; Elvis test constructors do not supply it.
3. Audit matching loses arms for Elvis-retained legacy route/executor commands.
4. Selecting Contract command hunks leaves Elvis `version_notice` deletion versus a surviving module declaration.
5. Resolved: obsolete Elvis `sub_open` dispatch and its implementation were removed.
6. Elvis's latest removal of `route_subscription_delivery_to_skill` re-export conflicts with Contract-side imports when Contract hunks are selected.
7. Provider delivery dispatch fields differ (`message`, `autotrade`).
8. Elvis-only provider decision/service-param/subscription decision commands remain referenced by audit and callers if Contract enum blocks are selected.
9. `asp_ops.rs` tests construct the Elvis v2 create command and will not accept Contract's legacy field model.

Resolve these after the primary choices; otherwise fixing them first will encode an accidental architecture.
