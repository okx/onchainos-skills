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

Collect Description (+ optional Provider) → ASP match → determine branch → load branch-specific playbook.

================================================
Step 1 -- Field collection (common fields only)
================================================

Description: MUST come from user's explicit input — no guessing/auto-fill. Title: agent-generated. Currency, Budget, Max budget: do NOT collect here — they are branch-dependent and will be collected after asp-match determines subscription vs regular.

| Field | CLI flag | Constraint | How to collect |
|---|---|---|---|
| Description | --description | 20-2000 chars | Consolidate user's words. If <20 → ask to expand |
| Title | --title | <=30 chars | Agent-generated; count chars, shorten if >30 |
| Designated provider | --provider | Optional; provider agentId | Extract the provider the user names. If not given, leave blank — Step 3 will auto-discover |

**Draft-description confirmation gate**: This gate applies only to publish/create-task `asp-match`. It does not apply to identity discovery or `onchainos agent search`. If the Description is a consolidation, summary, or reuse of the user's prior/current message rather than an exact explicitly-confirmed task description, show the draft Description and ask whether to use it for ASP matching. **End this turn. Do NOT run `asp-match` until the user explicitly confirms.** If the user edits it, use the edited text and then proceed.

================================================
Step 2 -- Basic validation
================================================

1. Description < 20 chars → ask to expand
2. Draft Description not explicitly confirmed → ask for confirmation (per gate above), then end turn

================================================
Step 3 -- ASP matching
================================================

**Path A — Provider specified** (`--provider` is set). Match its registered services:

```bash
onchainos agent asp-match --task-desc \"<description>\" --provider-agent-id <agentId> --format json
```

- Empty → \"This ASP has no matching services. Ask the user to designate another provider or adjust the description.\"
- Non-empty → extract top service (see field list below).

**Path B — No provider specified**. Auto-discover matching ASPs by description:

```bash
onchainos agent asp-match --task-desc \"<description>\" --format json
```

- Empty → no matching ASP found. Ask the user to: (a) specify a provider agentId manually, or (b) adjust the description, then re-run asp-match. Loop until a match is found or the user gives up.
- Non-empty → auto-select the top-ranked recommendation's provider and service. Proceed as if the user had designated that provider.

**Field extraction** (both paths): from the selected recommendation/service retain `providerAgentId`, `serviceId`, `serviceName`, `serviceDescription`, `serviceType`, `feeAmount` (non-subscription service fee only), `feeToken`→serviceTokenAddress, `feeTokenSymbol`, `endpoint` (if A2MCP), **`supportSubscription`** (branch flag), **`subscriptionInfo`** (billing interval, subscription fee from `subscription[].fee`, and normalized trial snapshot: `supportTrial` + `freeTrial`), and the complete **`autoTradePreflight` object verbatim**. Do not retain only the compact text summary: the subscription branch needs the structured `reminders[]`, `tools[]`, and `pluginId` values.
- If the same ASP returns both subscription and non-subscription services, display each with `[Subscription]` / `[One-time]` label and let the user choose. The chosen service determines the branch.

================================================
Step 3.5 -- Load branch playbook
================================================

Check the selected service's `supportSubscription` field and load the branch-specific playbook:

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

For trading-signal subscriptions, NEVER invent service parameters for an account, wallet, balance/collateral, per-trade amount, authorization limit/cap, venue/tool choice, plugin installation, API credentials, signal fields, or runtime consent. Those belong to tool readiness and the first-signal runtime decision flow, not `serviceParams`.

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
1. `[Watch]` block present → follow its instructions: read `skills/okx-ai/references/watch-core.md`, execute watch, then **end this turn**.
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
- **Currency** = `feeTokenSymbol` from asp-match (auto-filled)
- **Budget** = `subscriptionInfo.feeAmount` from asp-match (auto-filled fixed subscription price)

Collect/infer:

1. **serviceParams inference** (same logic as §serviceParams inference below).

2. **useTrial**: if `subscriptionInfo.supportTrial == true` from asp-match → automatically set to `true` (do NOT ask the user). Otherwise `false`. Display trial hours from `subscriptionInfo.freeTrial` in the confirmation form.

3. **autoRenew**: ask the user explicitly (0=off, 1=on). Do NOT pre-fill a default — collect the answer before Step 5.

4. **Signal execution setup and capability preflight**:
   - Never ask the old standalone binary execution opt-in question and never infer financial authorization from `serviceDescription`, ASP text, candidate tools, or the backend delivery marker.
   - If the user's own request explicitly asks for automatic signal execution, collect/infer exactly four bounded fields from user-authored context: mode=`auto`, fixed per-signal quote amount, per-signal cap, and quote currency (`USDT` or `USDC`). If any field is missing, ask only for those missing fields in one localized natural-language question, then **END THIS TURN**. Do not use A/B/C or numbered choices. Amount must be positive and no greater than cap.
   - If the user did not explicitly request automatic execution, collect no execution settings and continue normally. The first actionable delivery may ask for the missing configuration then; a service description alone never opts the user in.
   - Use only the retained schema-v2 `autoTradePreflight` object for the separate optional preparation gate below. Never block the subscription on a missing/unconfigured tool, never infer a venue from prose, and never run installation or configuration unless the user explicitly chooses that action.

   The preflight is advisory only and does not control delivery routing. Do NOT parse `serviceDescription` yourself to reconstruct missing preflight data. If `services[].autoTradePreflight` is absent, invalid or unavailable, omit the preparation card and continue creating the subscription. Do not retry `asp-match` solely to obtain preflight data. There is no standalone binary execution field to collect and no `--copy-trade` argument to pass.

   **Deterministic Trade Kit probe decision:** inspect `autoTradePreflight.tradeKitProbe.mode` after the service has been selected:
   - `probe_before_confirmation` → build one command from every token in `tradeKitProbe.assetClasses`, preserving the array order:
     ```text
     onchainos agent trade-kit-readiness --asset-class <class> [--asset-class <class> ...]
     ```
     Run it now. Do not persist its result. Continue without a card only when `ok:true` and `data.readiness == \"ready\"`.
   - `deferred_until_venue_selection` → Do not run a Trade Kit probe for `deferred_until_venue_selection`; the user has not selected that venue. Keep Trade Kit as `verification_unknown/authorization_not_checked` and continue the non-blocking subscription flow.
   - `not_applicable` → do not run the command.

   **Pre-confirmation gate (mandatory for a non-ready probe result or actionable local reminder):** render one separate, localized **Tool preparation (optional)** choice card:
   - `needs_configuration` → offer exactly OAuth — run `okx auth login --manual`; API key — run `okx config init`; and Later — continue subscribing.
   - `verification_unknown` → exactly `Retry` and `Later — continue subscribing`. Never present an unknown/network/timeout result as logged out.
   - `missing` or `incompatible` → one fixed install/upgrade action from `data.remediation`, plus `Later — continue subscribing`.
   - Local plugin `install_plugin` reminders remain one de-duplicated action per exact `pluginId`, plus `Later — continue subscribing`.

   The card footer must say that Later does not affect subscription creation, delivery visibility/storage, or the user's ability to ask their agent later to execute the delivered signal manually with any available tool. Do not invent actions from `serviceDescription`, DApp names, `choose_at_first_signal`, or `readiness_advisory`.

   After rendering that card, **END THIS TURN**. Do NOT render Step 5, do NOT call `create-subscribe`, and do NOT treat an install action as subscription confirmation in the same turn. This is preparation only: do NOT call it venue selection, do NOT set a default, and do NOT persist a venue preference or consent.

   On the user's next reply:
   - **Later / skip** → proceed to Step 5 using the retained selected-service fields. Later never upgrades readiness to ready.
   - **OAuth / API key / Retry / Trade Kit install or upgrade** → run only that explicit choice, then re-run the readiness command with the same retained `tradeKitProbe.assetClasses` and repeat this gate. Never continue from stale readiness.
   - **One named plugin preparation action** → run only that user-approved plugin install. Then re-run the original `asp-match` command with the same task description, provider and user agent, locate the **same `serviceId`**, replace the retained `autoTradePreflight` with the fresh object, and repeat this gate. If the same service is no longer returned, stop and ask the user to choose a service again. Never silently switch to the new top service.
   - **Ambiguous / multiple actions** → re-render the same bounded card; do not install anything.

   If there is no actionable reminder and no required Trade Kit probe, skip this gate and proceed to Step 5. Missing/invalid preflight also skips the gate and remains non-blocking. None of these states may block creation of the subscription.

**Max budget is NOT collected** for subscription tasks — the price is fixed at `subscriptionInfo.feeAmount`.

→ Proceed to **Step 5** (subscription confirmation form).

{service_params}\
================================================
Step 5 -- Subscription confirmation form
================================================

| Field | Value |
|---|---|
| Title | <short title, <=30 chars> |
| Description | <full content> (if <=200 chars in table; if >200 write `see below` and render below) |
| Provider | Agent <providerAgentId>(<providerAgentName>) — degrade to Agent <providerAgentId> when name empty/absent |
| Service params | <serviceParams readable display, or \"None\"> |
| Service price | <subscriptionInfo.feeAmount> <feeTokenSymbol> / month |
| Trial | Yes (<subscriptionInfo.freeTrial> hours free) / No (based on `subscriptionInfo.supportTrial`) |
| Auto-renew | On / Off |
| Signal execution | Automatic (only when explicitly requested; otherwise omit this row) |
| Per-signal amount | <amount> <USDT/USDC> (only with Signal execution) |
| Per-signal cap | <cap> <USDT/USDC> (only with Signal execution) |

> Confirm? Once confirmed, the subscription will be created on-chain.

→ **End this turn**; wait for the user's reply.

================================================
Step 5.5 -- Route by user decision (separate turn)
================================================

- Confirm / publish → Step 6
- Edit description → update → **re-run asp-match** (may switch branch; if branch changes, load the other branch playbook via `next-action`) → Step 4 → Step 5
- Edit serviceParams → update → Step 5
- Change ASP → update `--provider` to the new agentId → **re-run asp-match** (may switch branch) → Step 4 → Step 5
- Edit autoRenew → update → Step 5
- Edit automatic signal execution / amount / cap / quote currency → update; ask only for any missing bounded field → Step 5
- Prepare Trade Kit / Retry now → run only the explicit action, re-run the batch readiness command with the retained `tradeKitProbe.assetClasses`, and repeat the Step 4 gate; do not re-run `asp-match` or save a venue preference
- Prepare a plugin now → run only the explicit plugin installation, re-run the original `asp-match`, re-select the same `serviceId`, and repeat the Step 4 gate; do not save a venue preference
- Later / skip tool preparation → Step 5 without blocking subscription

================================================
Step 6 -- Publish subscription (create-subscribe)
================================================

```bash
onchainos agent create-subscribe \\
  --service-id <sid> \\
  --use-trial <true|false> \\
  --service-token-amount \"<subscriptionInfo.feeAmount>\" \\
  --service-token-address \"<feeToken>\" \\
  --auto-renew <0|1> \\
  --title \"<title>\" \\
  --description \"<description>\" \\
  --service-description \"<serviceDescription>\" \\
  --provider-agent-id <agentId> \\
  [--autotrade-mode auto \\
   --autotrade-amount \"<decimal-number>\" \\
   --autotrade-cap \"<decimal-number>\" \\
   --autotrade-quote <usdt|usdc>]
```
- Pass the four `--autotrade-*` flags together only when the final subscription confirmation contained the user's explicit automatic-execution setup. Never synthesize them from service or deliverable content.
- `--autotrade-amount` and `--autotrade-cap` are human-readable quote amounts selected by `--autotrade-quote`: pass a decimal number only (for example `10` or `20.5`), never minimal units and never a `USDT`/`USDC` suffix.
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

For regular tasks, collect Currency, Budget, and Max budget from the user:

1. **Payment token** (--currency): Only USDT / USDG. Fuzzy input (\"U\"/\"USD\") → ask \"USDT or USDG?\".
   - Validate: must match `feeTokenSymbol` from asp-match. Mismatch → ask user to change token or designate another provider.
2. **Budget** (--budget): ask user explicitly. number; <=5 decimals; max 10M.
   - Budget < 0 → reject (zero is legal)
   - Budget > 10M or > 5 decimal places → reject
3. **Max budget** (--max-budget): ask user explicitly. Constraint: >= budget; <=5 decimal places; max 10M.
   - max_budget < budget → reject
   - max_budget > 10M or > 5 decimal places → reject

4. **serviceParams inference** (same logic as §serviceParams inference below).

→ Proceed to **Step 5** (regular confirmation form).

{service_params}\
================================================
Step 5 -- Regular confirmation form
================================================

| Field | Value |
|---|---|
| Title | <short title, <=30 chars> |
| Description | <full content> (if <=200 chars in table; if >200 write `see below` and render below) |
| ASP | Agent <providerAgentId>(<providerAgentName>) — degrade to Agent <providerAgentId> when name empty/absent |
| Service params | <serviceParams readable display, or \"None\"> |
| Service price | <feeAmount> <feeTokenSymbol> (only show this row if feeAmount has a value) |
| Budget | <number> |
| Max budget | <number> (negotiation price cap) |
| Payment token | <USDT or USDG> |

Payment mode: A2A → `escrow`, A2MCP → `x402` (from serviceType; do not ask user, do not show as a card row).

> Confirm and publish?

→ **End this turn**; wait for the user's reply.

================================================
Step 5.5 -- Route by user decision (separate turn)
================================================

- Confirm / publish → Step 6
- Edit description → update → **re-run asp-match** (may switch branch; if branch changes, load the other branch playbook via `next-action`) → Step 4 → Step 5
- Edit budget/max-budget/currency → update → re-validate → Step 5
- Edit serviceParams → update → Step 5
- Change ASP → update `--provider` to the new agentId → **re-run asp-match** (may switch branch) → Step 4 → Step 5

================================================
Step 6 -- Publish regular (create-task)
================================================

```bash
onchainos agent create-task \\
  --description \"<description>\" --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> --currency <USDT|USDG> \\
  --provider <agentId> --service-id <sid> --payment-mode <escrow|x402> \\
  [--service-params \"<params>\"] [--service-token-address <addr>] [--service-token-amount <amt>]
```
- `--provider`, `--service-id`, `--payment-mode` required. Payment mode: A2A→escrow, A2MCP→x402.
- CLI error → relay to user, do NOT auto-modify → return to Step 5.

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
    fn common_create_task_requires_draft_description_confirmation_before_matching() {
        let out = create_task_common();
        assert!(
            out.contains("Draft-description confirmation gate"),
            "common create_task playbook must include the confirmation gate: {out}"
        );
        assert!(
            out.contains("Do NOT run `asp-match` until the user explicitly confirms"),
            "draft descriptions must be confirmed before ASP matching: {out}"
        );
        assert!(
            out.contains("This gate applies only to publish/create-task `asp-match`"),
            "confirmation gate must be scoped away from identity search: {out}"
        );
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
        // Explicit user-authored automatic execution is shown as bounded fields;
        // these rows are omitted entirely when the user did not opt in.
        assert!(out.contains("| Signal execution | Automatic"));
        assert!(out.contains("| Per-signal amount |"));
        assert!(out.contains("| Per-signal cap |"));
        assert!(out.contains("otherwise omit this row"));
        assert!(out.contains("Do not use A/B/C or numbered choices"));
        // Preflight readiness belongs only to the separate optional preparation card.
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
    fn subscription_playbook_treats_preflight_as_optional_advisory() {
        let out = create_task_subscription();
        let common = create_task_common();
        assert!(
            out.contains("advisory only and does not control delivery routing"),
            "playbook must keep preflight advisory: {out}"
        );
        assert!(
            out.contains("omit the preparation card and continue"),
            "missing preflight must not block creation: {out}"
        );
        assert!(
            !out.contains("  --copy-trade"),
            "removed copy-trade argument must not appear: {out}"
        );
        assert!(
            !out.contains("re-run `asp-match` exactly once"),
            "preflight absence must not force an extra match: {out}"
        );
        assert!(
            out.contains("Do NOT parse `serviceDescription` yourself"),
            "playbook must forbid agent-side description parsing: {out}"
        );
        assert!(
            out.contains("Tool preparation (optional)"),
            "unready tools must expose an optional preparation action: {out}"
        );
        assert!(
            out.contains("do NOT persist a venue preference or consent"),
            "preparation must not become venue selection: {out}"
        );
        assert!(
            common.contains("complete **`autoTradePreflight` object verbatim**"),
            "common match step must retain structured preflight data: {common}"
        );
        assert!(
            out.contains("**END THIS TURN**"),
            "tool preparation must be a separate user turn: {out}"
        );
        assert!(
            out.contains("locate the **same `serviceId`**"),
            "readiness refresh must not silently switch services: {out}"
        );
        assert!(
            out.contains("Never continue from stale readiness"),
            "installation result must be refreshed deterministically: {out}"
        );
        assert!(
            out.contains("execute the delivered signal manually with any available tool"),
            "skipping preparation must preserve the independent manual path: {out}"
        );
        assert!(out.contains("tradeKitProbe.mode"));
        assert!(out.contains("probe_before_confirmation"));
        assert!(out.contains("deferred_until_venue_selection"));
        assert!(out.contains(
            "onchainos agent trade-kit-readiness --asset-class <class> [--asset-class <class> ...]"
        ));
        assert!(out.contains("OAuth — run `okx auth login --manual`"));
        assert!(out.contains("API key — run `okx config init`"));
        assert!(out.contains("Retry"));
        assert!(out.contains("Later — continue subscribing"));
        assert!(out.contains("Do not run a Trade Kit probe for `deferred_until_venue_selection`"));
        assert!(out.contains("re-run the readiness command"));
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
}
