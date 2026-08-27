//! Task creation, attachment forwarding, and term-change event prompt generators.

// --- User-action: create task ------------------------------------------

pub(crate) fn create_task(message: Option<&serde_json::Value>) -> String {
    let branch = message
        .and_then(|m| m.get("branch"))
        .and_then(|v| v.as_str());
    match branch {
        Some("subscription") => create_task_subscription(),
        Some("regular") => create_task_regular(),
        _ => create_task_common(),
    }
}

fn create_task_common() -> String {
    "\
[Current Operation] Publish task (create_task)
[Role] User Agent
[Session Type] user session (talking directly to the user)

Collect Description → parse search intent → task-service-select → confirm service → load branch-specific playbook.

================================================
Step 1 -- Field collection (common fields only)
================================================

Description: MUST come from user's explicit input — no guessing/auto-fill. Title: agent-generated. Currency is branch-dependent. Budget and Max budget are never collected initially; after service selection they default to the selected service fee.

| Field | CLI flag | Constraint | How to collect |
|---|---|---|---|
| Description | --description | 20-2000 chars | Consolidate user's words. If <20 → ask to expand |
| Title | --title | <=30 chars | Agent-generated; count chars, shorten if >30 |

================================================
Step 2 -- Basic validation
================================================

1. Description < 20 chars → ask to expand

================================================
Step 3 -- Search-intent parsing and service selection
================================================

For the initial search, pass the user's original utterance verbatim to [`intent-keyword-extraction.md`], then use its output unchanged as `<args>` in:

```bash
onchainos agent task-service-select <args> --agentic-id <buyerAgentId> --sid <sid> --limit 1 --format json
```

Serialize `keywords` exactly like `service-match`: emit `--keywords` once, followed by all extracted
keyword values in order. For `--sid`, prefer the extracted value; otherwise use the user-selected `sid`
retained in context, not that Service's `serviceId`. Omit it when neither exists, and never infer it. Do not
otherwise preprocess or enrich the input or output.

- `matchStatus=no_match` → if `asp-agent-id` was supplied, say that the specified ASP has no matching service; otherwise say that no matching service was found. Ask the user to adjust the description or specify/change the provider.
- `matchStatus=no_online_service` → matches exist, but none is eligible (offline non-x402 services remain ineligible). Ask whether to view alternatives or adjust the description/provider.
- `matchStatus=matched` → render the service confirmation card from `data.services[0]`. The CLI keeps original ranking while filtering candidates to online services plus offline A2MCP services with a non-empty endpoint.

**Subscription duplicate gate — before the normal service confirmation card:**
- For a selected service with `supportSubscription == true`, require `subscriptionCheck.status == \"checked\"` and inspect `services[0].existingSubscription`. The CLI has already compared the exact `serviceId` against this buyer's subscriptions. A missing check is a hard stop: report that existing subscriptions could not be verified and do not confirm or create.
- `existingSubscription == null` → no non-terminal subscription exists for this service; continue normally. COMPLETED / CLOSED / FAILED historical subscriptions do not block a new one.
- `existingSubscription != null` → require top-level `duplicateSubscription`. A missing object is a hard stop. Do **not** call `service-list`, render the normal confirmation card, or continue to Steps 3.5–6. Do not query, list, or suggest the ASP's other services.
  - Render only `duplicateSubscription.userFacingPrompt`, translated faithfully to the user's language. Preserve the selected service name and `jobId` exactly. The duplicate result intentionally omits fee, trial, description, and readiness so these details cannot leak into the reply.
  - Offer only the actions in `nextAfterUserChoice`. ACTIVE includes only **Restore listening**; INIT / REJECTED / DISPUTED / unknown non-terminal ends after the duplicate warning with no follow-up action.
  - If the user chooses **Restore listening**, keep `<jobId>` as the explicit current subscription and route to `skills/okx-ai/references/task-user-playbook.md` §Signal-receipt watch entry as an explicit restore request; its first authorization gate uses `--review-existing`.

**Service confirmation gate**:
- Show Provider, Service, Type, Online, Price, Subscription/Trial summary, and Description.
- Render `serviceType` verbatim (for example, `A2A` or `A2MCP`); never translate or localize it.
- For a non-subscription Service, render `feeAmount` with `feeTokenSymbol`. If `feeAmount` is zero (number or numeric string), render localized `Free` instead of `0 <symbol>`.
- An offline A2MCP service with a non-empty endpoint is eligible; do not reject it for being offline. Offline non-x402 services remain ineligible.
- Ask the user to confirm using this service. Offer \"show 3 alternatives\" only when `hasMore == true` and `searchAfter` is a non-empty string; otherwise state that no more alternatives are available.
- If the user chooses alternatives, call:
  ```bash
  onchainos agent task-service-select --search-after \"<searchAfter>\" --limit 3 --agentic-id <buyerAgentId> --format json
  ```
  Do not include first-search conditions with `--search-after`. Render returned services and let the user choose one.

Retain the complete `task-service-select` JSON stdout. The CLI has already normalized the selected service fields, preserved each service's `online` status, and preserved the structured `autoTradePreflight` object for subscription preparation. Do not parse raw service-match fields yourself.

================================================
Step 3.5 -- Load branch playbook
================================================

After the user confirms a service, check the selected service's `supportSubscription` and load the branch-specific playbook. Retain the selected `task-service-select` JSON for later field extraction, but `next-action` branch routing still uses the explicit `branch` field.

- `supportSubscription == true` → call:
  ```bash
  onchainos agent next-action --role user --agentId <agentId> --message '{\"event\":\"create_task\",\"branch\":\"subscription\"}'
  ```
- otherwise → call:
  ```bash
  onchainos agent next-action --role user --agentId <agentId> --message '{\"event\":\"create_task\",\"branch\":\"regular\"}'
  ```

Then follow the returned playbook from Step 4 onward. **Do not proceed without loading the branch playbook.**\n"
        .to_string()
}

fn service_params_inference() -> &'static str {
    "\
================================================
§serviceParams inference
================================================

Using the selected service's `serviceDescription` + `serviceName` + the user's task `description`, infer a `serviceParams` plain-text string.

**Identify required user input** from `serviceDescription` (strict / fail closed):
Create a service parameter ONLY when the listing explicitly addresses the subscriber and says a concrete value is required, for example \"you must provide ...\", \"please input ...\", \"required parameter: ...\", or an explicit subscriber-fillable placeholder. A capability description, output schema, signal example, risk disclosure, execution precondition, or phrase such as \"check X before execution\" is NOT a request for subscriber input.

For trading-signal subscriptions, keep account, wallet, balance/collateral, per-trade amount, authorization limit/cap, venue/tool choice, plugin installation, API credentials, signal fields, execution mode, Trade Kit environment, margin mode, and order policy out of `serviceParams`. Parse user-authored execution settings into the separate `--autotrade-*` fields described below.

If explicit subscriber-input language is absent or ambiguous → `serviceParams` MUST be empty. Do not create `<to be provided>` rows from inference alone.

**Match against user's task description**:
- Provided → extract the concrete value
- Not provided → mark as `<to be provided>` with a hint

**Format**: natural-language `key：value` pairs separated by `；` or `\\n`. Do NOT use JSON.

**Confidence routing**:
- All filled → use directly in confirmation form
- Some `<to be provided>` → show in form with marks; user can edit
- No input required → serviceParams is empty

Do NOT ask the user for serviceParams separately — always show in the confirmation form. The user can correct it there.\n"
}

fn attachments_and_stop() -> String {
    use crate::commands::agent_commerce::task::common::config::is_cli_mode;

    let watch_section = if is_cli_mode() {
        "\
**After create-task/create-subscribe + task-attach (if any), check CLI output for a `[Watch]` block:**
0. If balanceWarning exists, stop here; do not Watch.
1. `[Watch]` block present → follow its instructions: read `skills/okx-ai/references/watch-core.md` and enter its Watch generation. A returned notification, deliverable, or empty poll does **not** end the turn; dispatch and re-enter until `watch-core.md` says to stop or a decision requires the user's reply.
2. No `[Watch]` block → **end this turn immediately**."
    } else {
        "**End this turn immediately.** Do NOT mention or ask about monitoring/watching task progress."
    };

    format!("\
================================================
Step 6.5 -- Save attachments
================================================

If the user included file(s)/image(s) as task material → for each: `onchainos agent task-attach --file \"<path>\" <jobId>`. Download to local path first if needed. Failure → skip (do not block). No files → skip this step.

================================================

After success:

- `blockedReason=insufficient-balance`: save the exact `create-task` command + `balanceWarning`; if `fundingNoticeCommand` exists, run it. `terminal-unicode`: show `terminalQr` + full notice. `image-notify`: localize `contentCanonical`, run `notifyCommandArgs`, put `markdownImage` under option 1 in final. If missing, show `balanceWarning`. END TURN; do not create again or Watch.
- No `balanceWarning`: tell the user directly: \"{create_designated}\"
- Legacy submitted `balanceWarning`: save `jobId` + warning, render `funding-notice`; on Codex/Claude Code repeat the full notice in final. END TURN; do not Watch.

{watch_section}

Do not say \"published\"/\"succeeded\" (only submitted). No other commands after the step above; no describing subsequent flow.\n",
        create_designated = super::super::content::create_task_designated_user_notify(),
    )
}

fn create_task_subscription() -> String {
    format!("\
[Current Operation] Publish task — subscription branch
[Role] User Agent

================================================
Step 4 -- Subscription field collection
================================================

For subscription tasks, Currency and Budget are derived from the service — do NOT ask the user:
- **Currency** = `feeTokenSymbol` from task-service-select (auto-filled)
- **Budget** = `subscriptionInfo.feeAmount` from task-service-select (auto-filled fixed subscription price)

Collect/infer:

1. **serviceParams inference** (same logic as §serviceParams inference below).

2. **useTrial**: if `subscriptionInfo.supportTrial == true` from task-service-select → automatically set to `true` (do NOT ask the user). Otherwise `false`. Display trial hours from `subscriptionInfo.freeTrial` in the confirmation form.

3. **autoRenew**: ask the user explicitly (0=off, 1=on). Do NOT pre-fill a default — collect the answer before Step 5.

4. **Signal execution setup and capability preflight**:
   - Automatic signal execution is the MVP default. Set mode=`auto` unless the user explicitly says not to execute automatically; an explicit opt-out sets mode=`manual`.
   - Inspect `serviceDescription` only to identify which execution settings the ASP asks the subscriber to provide and any values presented as suggestions. ASP text is not the user's answer and must never be persisted by itself.
   - Parse mode, fixed per-signal quote amount, per-signal cap, quote currency (`USDT` or `USDC`), margin mode (`cross` or `isolated`), and order policy (`market` or `signal_price_limit`) only from user-authored context. ASP prose determines only which supported fields to ask for; it never supplies a value. When a required field has not been answered, ask for only the missing fields in one localized natural-language question, then **END THIS TURN**. Never use A/B/C, numbered choices, or a decision card.
   - Amount and cap are optional unless the ASP explicitly asks the user to configure them. Each supplied value must be a positive decimal. Do not compare amount with cap. Quote defaults to `USDT`.
   - Retain `autoTradePreflight` only as advisory local runtime information. Never block subscription creation on a missing/incompatible/unknown tool. Installation or upgrade may run only after the user explicitly chooses the optional Trade Kit preparation action below; choosing Later must continue the subscription unchanged.
   - When `autoTradePreflight.tradeKitProbe.mode=probe_before_confirmation`, collect exactly one user-authored Trade Kit environment (`live` or `demo`) and one order policy (`market` or `signal_price_limit`). When `tradeKitProbe.assetClasses` contains `perp`, also collect one margin mode (`cross` or `isolated`). Never infer any value from ASP text, Trade Kit defaults, or readiness output. Ask for all missing settings together, then **END THIS TURN**. Retain them as `tradeEnvironment`, `marginMode`, and `orderPolicy`.

   The preflight is advisory only and does not control delivery routing. Do NOT parse `serviceDescription` yourself to reconstruct missing preflight data. If `services[].autoTradePreflight` is absent, invalid or unavailable, omit the advisory notice and continue creating the subscription. Do not retry `task-service-select` solely to obtain preflight data. There is no standalone binary execution field to collect and no `--copy-trade` argument to pass.
   **Deterministic Trade Kit probe decision:** inspect `autoTradePreflight.tradeKitProbe.mode` after the service has been selected:
   - `probe_before_confirmation` → build one command from every token in `tradeKitProbe.assetClasses`, preserving the array order:
     ```text
     onchainos agent trade-kit-readiness --asset-class <class> [--asset-class <class> ...] --environment <live|demo>
     ```
     Run it now. This schema-v3 command checks local CLI startup, version, and public capabilities only; it never checks authentication, account permissions, network availability, or trading availability. Do not persist its result. A non-ready result is an advisory notice only, and `verification_unknown` is non-blocking.
   - `deferred_until_venue_selection` → Do not auto-run a Trade Kit probe because the user has not selected that venue. If the user later selects Trade Kit, local compatibility may be checked during route preparation. Preparing Trade Kit does not select it as the venue. Do not show an authentication/configuration warning from this deferred state.
   - `not_applicable` → do not run the command.

   **Trade Kit preparation and connection gate (optional; separate turn):** whenever mode is `probe_before_confirmation`, render one localized card after the local probe with exactly these two choices. This is an optional setup action, not a claim that the user is logged out:
   1. **Install/connect Trade Kit**
   2. **Later — continue subscribing**

   State that preparation is optional, Later does not affect subscription creation or delivery storage, and preparing Trade Kit does not select it as the execution venue. Then **END THIS TURN**. This is a tool-preparation choice, so the no-numbered-choices rule for collecting execution values does not apply.

   On the user's next reply:
   - **Later** → proceed to Step 5 with the retained selected-service and user-authored fields. Never upgrade readiness to ready.
   - **Install/connect Trade Kit** → first resolve `okx-cex-auth` from the currently installed skills. If available, load it directly. Only when unavailable, run the required skill security scan scoped to `okx/agent-skills`; after a passing scan, run exactly `npx skills add okx/agent-skills --yes --global`, then load `okx-cex-auth`. Follow that skill for CLI installation, site selection, OAuth/API-key setup, and authentication recovery; never duplicate those steps here. If the earlier local readiness result was `missing` or `incompatible`, re-run the same local readiness command once after installation/upgrade solely to verify CLI compatibility. Never re-run readiness to verify OAuth, never convert login success into readiness `ready`, and never persist an authentication conclusion. Once the auth skill completes, proceed to Step 5 without another connection card.
   - **Ambiguous reply** → re-render the same two choices without installing or configuring anything.

   Other non-Trade-Kit preparation reminders remain concise advisory notices without choices and continue to Step 5. Never auto-install a tool, persist a readiness/authentication result, or treat preparation as venue selection.

**Max budget is NOT collected** for subscription tasks — the price is fixed at `subscriptionInfo.feeAmount`.

→ Proceed to **Step 5** (subscription confirmation form).

{service_params}\
================================================
Step 5 -- Subscription confirmation form
================================================

The confirmation form has exactly the seven product-facing rows below. Execution mode, per-signal amount, per-signal cap, quote currency, Trade Kit environment, margin mode, order policy, and any other execution setting are internal execution configuration. Never append, merge, or render them as rows in this or any other confirmation form, even when they appear in the user request, service description, retained context, or service usage guide. Continue retaining the user-authored values for the Step 6 `--autotrade-*` arguments.

| Field | Value |
|---|---|
| Title | <short title, <=30 chars> |
| Description | <full content> (if <=200 chars in table; if >200 write `see below` and render below) |
| Provider | Agent <providerAgentId>(<providerAgentName>) — degrade to Agent <providerAgentId> when name empty/absent |
| Service params | <serviceParams readable display, or \"None\"> |
| Service price | <subscriptionInfo.feeAmount> <feeTokenSymbol> / month |
| Trial | Yes (<subscriptionInfo.freeTrial> hours free) / No (based on `subscriptionInfo.supportTrial`) |
| Auto-renew | On / Off |

> Confirm? Once confirmed, the subscription will be created on-chain.

→ **End this turn**; wait for the user's reply.

================================================
Step 5.5 -- Route by user decision (separate turn)
================================================

- Confirm / publish → Step 6
- Edit description → update search intent → **re-run task-service-select** (may switch branch; if branch changes, load the other branch playbook via `next-action`) → Step 4 → Step 5
- Edit serviceParams → update → Step 5
- Change ASP → update `--asp-agent-id` to the new agentId → **re-run task-service-select** (may switch branch) → Step 4 → Step 5
- Edit autoRenew → update → Step 5
- Edit automatic signal execution / amount / cap / quote currency / Trade Kit environment / margin mode / order policy → update the user-authored value; cap remains informational → Step 5

================================================
Step 6 -- Publish subscription (create-subscribe)
================================================

```bash
onchainos agent create-subscribe \\
  --service-id <serviceId> \\
  --use-trial <true|false> \\
  --service-token-amount \"<subscriptionInfo.feeAmount>\" \\
  --service-token-address \"<feeToken>\" \\
  --auto-renew <0|1> \\
  --title \"<title>\" \\
  --description \"<description>\" \\
  --service-description \"<serviceDescription>\" \\
  --provider-agent-id <agentId> \\
  --autotrade-mode <auto|manual> \\
  [--autotrade-amount \"<decimal-number>\"] \\
  [--autotrade-cap \"<decimal-number>\"] \\
  [--autotrade-quote <usdt|usdc>] \
  [--autotrade-environment <live|demo>] \
  [--autotrade-margin-mode <cross|isolated>] \
  [--autotrade-order-policy <market|signal_price_limit>] \
  [--autotrade-required-field <mode|tradeAmount|cap|quote|environment|marginMode|orderPolicy>]...
```
- Always pass the confirmed mode; it defaults to `auto`. Pass amount, cap, quote, Trade Kit environment, margin mode, and order policy only from user-authored context. For a confirmed Trade Kit route, environment and order policy are required; margin mode is additionally required for `perp`. ASP suggestions alone are never values.
- Pass one `--autotrade-required-field` for every execution field that this flow required the user to confirm. Include fields explicitly required by the ASP description. For a confirmed Trade Kit route, always include `environment` and `orderPolicy`, plus `marginMode` for `perp`. Do not include tool installation, OAuth/API-key readiness, or ASP-suggested values. The CLI validates this declaration before any remote create request.
- `--autotrade-amount` and `--autotrade-cap` are human-readable quote amounts selected by `--autotrade-quote`: pass a decimal number only (for example `10` or `20.5`), never minimal units and never a `USDT`/`USDC` suffix.
- Do not compare `--autotrade-amount` with `--autotrade-cap`. A stored cap is informational in this MVP.
- CLI error → relay to user, do NOT auto-modify → return to Step 5.

{attachments_stop}",
        service_params = service_params_inference(),
        attachments_stop = attachments_and_stop(),
    )
}

fn create_task_regular() -> String {
    format!("\
[Current Operation] Publish task — regular branch
[Role] User Agent

================================================
Step 4 -- Regular field collection
================================================

For regular tasks, collect Currency internally but do not show it in the confirmation form. Derive Budget and Max budget from the selected service:

1. **Payment token** (--currency): Only USDT / USDG. Fuzzy input (\"U\"/\"USD\") → ask \"USDT or USDG?\".
   - Validate: must match `feeTokenSymbol` from task-service-select. Mismatch → ask user to change token or designate another provider.
2. Read `feeAmount` from the exact selected service. Missing/non-numeric → stop before confirmation.
   - `budget = feeAmount`
   - `max_budget = feeAmount`
   - Apply the existing create-task amount rules (non-negative, <=6 decimals, max 10M). Do not ask the user for either value.

3. **serviceParams inference** (same logic as §serviceParams inference below).

→ Proceed to **Step 5** (regular confirmation form).

{service_params}\
================================================
Step 5 -- Regular confirmation form
================================================

Never add execution mode, per-signal amount, per-signal cap, quote currency, Trade Kit environment, margin mode, order policy, or any other execution setting to this or any other confirmation form.

| Field | Value |
|---|---|
| Title | <short title, <=30 chars> |
| Description | <full content> (if <=200 chars in table; if >200 write `see below` and render below) |
| ASP | Agent <providerAgentId>(<providerAgentName>) — degrade to Agent <providerAgentId> when name empty/absent |
| Service params | <serviceParams readable display, or \"None\"> |
| Service price | <localized Free when feeAmount is zero; otherwise feeAmount + feeTokenSymbol> (only show this row if feeAmount has a value) |

Payment mode: A2A → `escrow`, A2MCP → `x402` (from serviceType; do not ask user, do not show as a card row).

> Confirm and publish?

→ **End this turn**; wait for the user's reply.

================================================
Step 5.5 -- Route by user decision (separate turn)
================================================

- Confirm / publish → Step 6
- Edit description → update search intent → **re-run task-service-select** (may switch branch; if branch changes, load the other branch playbook via `next-action`) → Step 4 → Step 5
- Edit budget/max-budget → validate the proposed value(s) with the existing rules, including `max_budget >= budget`; keep an omitted field unchanged and do not auto-adjust the other field. Invalid → explain and keep the current values. Valid → show the proposed value(s) separately, ask for one explicit confirmation, and end the turn. After confirmation, update the existing field(s) and return to Step 5; the confirmation form still omits both budget rows.
- Edit currency → update → re-validate → Step 5
- Edit serviceParams → update → Step 5
- Change ASP → update `--asp-agent-id` to the new agentId → **re-run task-service-select** (may switch branch) → reset budget/max_budget from the newly selected service fee → Step 4 → Step 5

================================================
Step 6 -- Publish regular (create-task)
================================================

```bash
onchainos agent create-task \\
  --description \"<description>\" --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> --currency <USDT|USDG> \\
  --provider <agentId> --service-id <serviceId> --payment-mode <escrow|x402> \\
  [--service-params \"<params>\"] [--service-token-address <addr>] [--service-token-amount <amt>]
```
- `--provider`, `--service-id`, `--payment-mode` required. Payment mode: A2A→escrow, A2MCP→x402.
- CLI error → relay to user, do NOT auto-modify → return to Step 5.
- After `create-task` succeeds, budget and max budget are locked; never offer a direct edit.

{attachments_stop}",
        service_params = service_params_inference(),
        attachments_stop = attachments_and_stop(),
    )
}

// --- Attachment forwarding ---------------------------------------------

/// Upload + forward a single attachment file in Rust. Returns Ok(()) on
/// success or Err with a human message on failure.
fn upload_and_forward_one(
    file_path: &str,
    agent_id: &str,
    job_id: &str,
    to_agent_id: &str,
) -> Result<(), String> {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    let upload = okx_a2a::file_upload(file_path, agent_id, job_id, None, None)
        .map_err(|e| format!("file upload failed for {file_path}: {e}"))?;

    let msg = format!(
        "jobId: {job_id}\n\
         attachmentType: file\n\
         fileKey: {file_key}\n\
         digest: {digest}\n\
         salt: {salt}\n\
         nonce: {nonce}\n\
         secret: {secret}\n\
         filename: {filename}\n\
         description: This is an attachment/reference material for the task. The ASP should download it for task execution.\n\
         [intent:attachment]",
        file_key = upload.file_key,
        digest = upload.digest,
        salt = upload.salt,
        nonce = upload.nonce,
        secret = upload.secret,
        filename = upload.filename,
    );

    okx_a2a::xmtp_send(job_id, to_agent_id, &msg)
        .map_err(|e| format!("xmtp-send failed for {file_path}: {e}"))
}

/// Upload + forward ALL pending attachments for a job. Best-effort: failures
/// are logged but do not block the caller. Returns the count of successfully
/// forwarded files.
pub(crate) fn upload_and_forward_all_attachments(
    job_id: &str,
    agent_id: &str,
    to_agent_id: &str,
) -> usize {
    use crate::commands::agent_commerce::task::common::DEBUG_LOG;

    let files = super::super::attachments::list_attachment_paths(job_id);
    if files.is_empty() {
        return 0;
    }
    let mut ok_count = 0usize;
    for fp in &files {
        match upload_and_forward_one(fp, agent_id, job_id, to_agent_id) {
            Ok(()) => {
                ok_count += 1;
                if DEBUG_LOG {
                    eprintln!("[attachment_cli] ✓ forwarded: {fp}");
                }
            }
            Err(e) => {
                eprintln!("[attachment_cli] ⚠ skipped: {e}");
            }
        }
    }
    ok_count
}

/// Rust fast-path for `attachment_added`: upload + xmtp-send in-process,
/// then return a notify-only prompt for the LLM.
pub(crate) fn attachment_added_cli(
    ctx: &super::super::flow::FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let file_path = message
        .and_then(|m| m.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if file_path.is_empty() {
        return format!(
            "[attachment_added_cli] ERROR: filePath missing in --message JSON.\n\n\
             [Your next action] Notify the user:\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized: Attachment forwarding failed — file path was not provided. Please retry via task-attach.>\"\n\
             ```\n"
        );
    }

    let to_agent_id = ctx
        .prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .unwrap_or("");
    if to_agent_id.is_empty() {
        return format!(
            "[attachment_added_cli] ERROR: provider not assigned — cannot forward attachment.\n\n\
             [Your next action] Notify the user:\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized: [Job {short_id}] Attachment saved locally but no provider assigned yet. It will be forwarded automatically once a provider accepts the task.>\"\n\
             ```\n"
        );
    }

    match upload_and_forward_one(file_path, agent_id, job_id, to_agent_id) {
        Ok(()) => {
            let att_sent = super::super::content::attachment_sent_user_notify()
                .replace("<short_jobId>", short_id);
            format!(
                "[attachment_added_cli] ✓ Attachment uploaded and forwarded to provider in-process.\n\n\
                 [Your next action] Notify the user and end turn.\n\n\
                 **Localize first** — translate the content below into the user's language before sending.\n\
                 Content:\n\
                 \x20\x20{att_sent}\n\n\
                 ```bash\n\
                 onchainos agent user-notify --content \"<localized content>\"\n\
                 ```\n\
                 **End this turn.**\n"
            )
        }
        Err(e) => {
            eprintln!("[attachment_added_cli] upload/forward failed: {e}");
            format!(
                "[attachment_added_cli] ERROR: upload/forward failed: {e}\n\n\
                 [Your next action] Notify the user that the attachment could not be sent.\n\n\
                 ```bash\n\
                 onchainos agent user-notify --content \"<translate: [Job {short_id}] Attachment forwarding failed. Please retry later.>\"\n\
                 ```\n\n\
                 **End this turn.**\n"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_create_task_confirms_selected_service_after_matching() {
        let out = create_task_common();
        assert!(
            !out.contains("Draft-description confirmation gate"),
            "common create_task playbook must not confirm the description before matching: {out}"
        );
        assert!(
            out.contains("Ask the user to confirm using this service"),
            "common create_task playbook must confirm the selected service: {out}"
        );
    }

    #[test]
    fn common_create_task_blocks_duplicate_subscription_before_confirmation() {
        let out = create_task_common();
        let duplicate_gate = out
            .find("Subscription duplicate gate")
            .expect("duplicate gate must exist");
        let confirmation_gate = out
            .find("Service confirmation gate")
            .expect("confirmation gate must exist");
        assert!(duplicate_gate < confirmation_gate);
        assert!(out.contains("services[0].existingSubscription"));
        assert!(out.contains("COMPLETED / CLOSED / FAILED historical subscriptions do not block"));
        assert!(out.contains("require top-level `duplicateSubscription`"));
        assert!(out.contains("duplicateSubscription.userFacingPrompt"));
        assert!(out.contains("intentionally omits fee, trial, description, and readiness"));
        assert!(out.contains("Do **not** call `service-list`"));
        assert!(out.contains("Do not query, list, or suggest the ASP's other services"));
        assert!(out.contains("ACTIVE includes only **Restore listening**"));
        assert!(out.contains("§Signal-receipt watch entry as an explicit restore request"));
        assert!(out.contains("first authorization gate uses `--review-existing`"));
    }

    #[test]
    fn subscription_playbook_reads_preflight_not_prompt() {
        let out = create_task_subscription();
        // FR-7 / AC-8: the old copy-trade prompt is gone …
        assert!(
            !out.contains("Enable auto copy-trade?"),
            "old EN copy-trade prompt must be removed: {out}"
        );
        // … including the Chinese "\u{81ea}\u{52a8}\u{8ddf}\u{5355}" (auto copy-trade) question.
        assert!(
            !out.contains("\u{81ea}\u{52a8}\u{8ddf}\u{5355}"),
            "CN copy-trade prompt must be absent"
        );
        // The pre-confirmation preparation gate still consumes bounded preflight information.
        assert!(
            out.contains("autoTradePreflight"),
            "subscription playbook must reference autoTradePreflight: {out}"
        );
        // The confirmation form must NOT render the old binary On/Off row.
        assert!(
            !out.contains("| Auto copy-trade |"),
            "confirmation form must not contain an Auto copy-trade row: {out}"
        );
        // Execution configuration is retained for persistence but never exposed as form rows.
        assert!(!out.contains("| Signal execution |"));
        assert!(!out.contains("| Per-signal amount |"));
        assert!(!out.contains("| Per-signal cap |"));
        assert!(!out.contains("| Trade Kit environment |"));
        assert!(out.contains("exactly the seven product-facing rows below"));
        for expected_row in [
            "| Title |",
            "| Description |",
            "| Provider |",
            "| Service params |",
            "| Service price |",
            "| Trial |",
            "| Auto-renew |",
        ] {
            assert!(out.contains(expected_row), "missing confirmation row {expected_row}");
        }
        let form = out
            .split("Step 5 -- Subscription confirmation form")
            .nth(1)
            .expect("subscription confirmation section")
            .split("> Confirm?")
            .next()
            .expect("subscription confirmation table");
        assert_eq!(
            form.lines().filter(|line| line.starts_with("| ")).count(),
            8,
            "confirmation table must contain one header plus exactly seven product rows"
        );
        assert!(out.contains(
            "Continue retaining the user-authored values for the Step 6 `--autotrade-*` arguments"
        ));
        assert!(out.contains("--autotrade-environment <live|demo>"));
        assert!(out.contains("--autotrade-required-field"));
        assert!(out.contains(
            "The CLI validates this declaration before any remote create request"
        ));
        assert!(out.contains("Do not compare amount with cap"));
        assert!(out.contains("Never use A/B/C, numbered choices, or a decision card"));
        // Preflight readiness stays advisory and never becomes confirmation fields.
        assert!(
            !out.contains("| Signal types |"),
            "confirmation form must not add a Signal types row: {out}"
        );
        assert!(
            !out.contains("| Candidate tools |"),
            "confirmation form must not add a Candidate tools row: {out}"
        );
        assert!(
            !out.contains("| Advisory |"),
            "confirmation form must not add an Advisory row: {out}"
        );
    }

    #[test]
    fn subscription_playbook_offers_optional_trade_kit_preparation() {
        let out = create_task_subscription();
        let common = create_task_common();
        assert!(
            out.contains("advisory only and does not control delivery routing"),
            "playbook must keep preflight advisory: {out}"
        );
        assert!(out.contains("continue creating the subscription"));
        assert!(
            !out.contains("  --copy-trade"),
            "removed copy-trade argument must not appear: {out}"
        );
        assert!(
            !out.contains("re-run `task-service-select` exactly once"),
            "preflight absence must not force an extra match: {out}"
        );
        assert!(out.contains("ASP text is not the user's answer"));
        assert!(out.contains("Trade Kit preparation and connection gate (optional; separate turn)"));
        assert!(out.contains("Install/connect Trade Kit"));
        assert!(out.contains("Later — continue subscribing"));
        assert!(out.contains("checks local CLI startup, version, and public capabilities only"));
        assert!(out.contains("never checks authentication"));
        assert!(out.contains("resolve `okx-cex-auth`"));
        assert!(out.contains("required skill security scan"));
        assert!(out.contains("npx skills add okx/agent-skills --yes --global"));
        assert!(out.contains("Never re-run readiness to verify OAuth"));
        assert!(out.contains("never convert login success into readiness `ready`"));
        assert!(out.contains("Then **END THIS TURN**"));
        assert!(
            common.contains("structured `autoTradePreflight` object"),
            "common match step must retain structured preflight data: {common}"
        );
        assert!(out.contains("tradeKitProbe.mode"));
        assert!(out.contains("probe_before_confirmation"));
        assert!(out.contains("deferred_until_venue_selection"));
        assert!(out.contains(
            "onchainos agent trade-kit-readiness --asset-class <class> [--asset-class <class> ...] --environment <live|demo>"
        ));
        assert!(out.contains("Do not auto-run a Trade Kit probe"));
        assert!(out.contains("does not select it as the venue"));
        assert!(out.contains("re-run the same local readiness command once"));
    }

    #[test]
    fn regular_confirmation_form_never_exposes_execution_configuration() {
        let out = create_task_regular();
        assert!(!out.contains("| Signal execution |"));
        assert!(!out.contains("| Per-signal amount |"));
        assert!(!out.contains("| Per-signal cap |"));
        assert!(out.contains(
            "Never add execution mode, per-signal amount, per-signal cap, quote currency, Trade Kit environment, margin mode, order policy, or any other execution setting to this or any other confirmation form"
        ));
    }

    #[test]
    fn service_params_are_explicit_only_and_exclude_runtime_trade_settings() {
        let out = service_params_inference();
        assert!(out.contains("strict / fail closed"));
        assert!(out.contains("balance/collateral"));
        assert!(out.contains("per-trade amount"));
        assert!(out.contains("authorization limit/cap"));
        assert!(out.contains("venue/tool choice"));
        assert!(out.contains("serviceParams` MUST be empty"));
    }

    #[test]
    fn regular_create_task_requires_full_balance_notice_before_watch() {
        let out = create_task_regular();
        assert!(out.contains("balanceWarning"));
        assert!(out.contains("blockedReason=insufficient-balance"));
        assert!(out.contains("save the exact `create-task` command + `balanceWarning`"));
        assert!(out.contains("if `fundingNoticeCommand` exists, run it"));
        assert!(out.contains("`terminal-unicode`"));
        assert!(out.contains("show `terminalQr` + full notice"));
        assert!(out.contains("`image-notify`"));
        assert!(out.contains("run `notifyCommandArgs`"));
        assert!(out.contains("If missing, show `balanceWarning`"));
        assert!(out.contains("END TURN"));
        assert!(out.contains("do not create again or Watch"));
        assert!(out.contains("Legacy submitted `balanceWarning`"));
        assert!(out.contains("do not Watch"));
    }

    #[test]
    fn regular_create_task_defaults_budget_to_service_fee_without_showing_it() {
        let out = create_task_regular();

        assert!(out.contains("budget = feeAmount"));
        assert!(out.contains("max_budget = feeAmount"));
        assert!(!out.contains("ask user explicitly"));
        assert!(!out.contains("| Budget |"));
        assert!(!out.contains("| Max budget |"));
        assert!(!out.contains("| Payment token |"));
        assert!(out.contains("non-negative"));
    }

    #[test]
    fn regular_create_task_keeps_pre_create_budget_edits_with_separate_confirmation() {
        let out = create_task_regular();

        assert!(out.contains("Edit budget/max-budget"));
        assert!(out.contains("show the proposed value(s) separately"));
        assert!(out.contains("do not auto-adjust the other field"));
        assert!(out.contains("After `create-task` succeeds, budget and max budget are locked"));
    }
}
