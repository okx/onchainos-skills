//! Core happy-path lifecycle prompt generators.

use super::super::flow::FlowContext;

// --- Execution stage ----------------------------------------------------

pub(crate) async fn provider_applied(ctx: &FlowContext<'_>, over_most_budget: bool, visibility: i64) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    // visibility: 0 = public, 1 = private. The "make public" option only makes sense
    // when the task is currently private; otherwise drop the option and renumber close.
    let is_private = visibility == 1;
    let option_count_num = if is_private { "4" } else { "3" };
    let close_label = if is_private { "D" } else { "C" };
    let option_public_line = if is_private {
        "C. Make the task public so any qualified ASP can apply\n         "
    } else {
        ""
    };

    let mut client = TaskApiClient::new();

    if over_most_budget {
        // ── Over-budget branch: reject the apply, mirror job_provider_reject's playbook ──
        if let Err(e) = super::super::reject_apply::handle_reject_apply(&mut client, job_id, Some(agent_id)).await {
            return format!(
                "[provider_applied/over_budget] ❌ reject-apply failed in-process: {e}\n\n\
                 Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
            );
        }

        let short_id = ctx.short_id;
        let session_hint = super::super::flow::SESSION_STATUS_HINT;
        let l10n_prompt = super::super::flow::L10N_PROMPT;
        let follow_playbook = super::super::flow::FOLLOW_PLAYBOOK;
        let route_hint = super::super::flow::ROUTE_VIA_ENVELOPE;
        let cmd = super::super::flow::pending_cmd(job_id, agent_id, None, &format!("[Over budget {short_id}] next-step decision"), "apply_over_budget");

        return format!(
        "[provider_applied/over_budget] ✅ reject-apply completed in-process.\n\n\
         🛑 Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         {session_hint}\n\
         ```bash\n\
         {cmd}\n\
         ```\n\
         {l10n_prompt}\n\
         **`--user-content` template** (canonical English — translate to user's language; keep the {option_count_num} lettered options):\n\
         ```\n\
         [Job {short_id} — you are the User Agent] The ASP's quote exceeded the maximum budget for this task. The apply has been rejected automatically.\n\n\
         What would you like to do next?\n\
         A. Browse the ASP list\n\
         B. Designate a specific ASP by agentId\n\
         {option_public_line}{close_label}. Close the task\n\
         ```\n\n\
         {follow_playbook}\n\
         {route_hint}\n"
        );
    }

    // ── Within-budget branch: confirm-accept on-chain (escrow funded; status → accepted) ──
    if let Err(e) = super::super::accept::handle_confirm_accept(&mut client, job_id).await {
        return format!(
            "[provider_applied/confirm_accept] ❌ confirm-accept failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        );
    }

    "[Current state] provider_applied (within max budget; confirm-accept completed in-process)\n\
     [Role] User (User Agent)\n\n\
     ✓ In-process: confirm-accept — escrow funded, on-chain accept submitted (see txHash printed above). Status is now `accepted`.\n\
     → **End this turn** and wait for the `job_accepted` system notification.\n".to_string()
}

pub(crate) fn job_accepted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;

    let accepted_escrow_notify = super::super::content::job_accepted_escrow_user_notify(job_id, title_display);
    let accepted_x402_fail = super::super::content::job_accepted_x402_replay_fail_user_notify(job_id);
    let complete_failed = super::super::content::complete_failed_user_notify(job_id);

    let pm = ctx.payment_mode;
    let pm_extract = if pm.is_some() { "" } else { "paymentMode (int: 1=escrow, 3=x402), " };
    let branch_header = if pm.is_none() { "**Step 2 -- Branch by payment mode:**\n\n" } else { "" };

    let escrow_section = if pm == Some(3) { String::new() } else { format!("\
     --------- Branch A: escrow ---------\n\n\
     Notify the user that accept succeeded via `okx-a2a user notify`:\n\
     🌐 **Localize first** — translate the canonical English content below.\n\
     ```bash\n\
     okx-a2a user notify --content '<your translated content>'\n\
     ```\n\n\
     Canonical English content:\n\
     {accepted_escrow_notify}\n\n\
     [Follow-up events]\n\
     - job_submitted → review the deliverable\n\n") };

    let x402_section = if pm == Some(1) { String::new() } else { format!("\
     --------- Branch B: x402 ---------\n\n\
     In x402 mode, accept has already been settled on-chain (funds paid); task-402-pay was executed in the previous turn (job_payment_mode_changed).\n\n\
     **B-Step 1 -- Determine replaySuccess from the previous turn's task-402-pay:**\n\
     Look up the task-402-pay output in this sub session context.\n\
     ⚠️ If it cannot be found (e.g. lost due to context compaction), **default to replaySuccess=true** --\n\
     x402 funds are paid during accept, the user was already notified of the delivery result (success or failure) in the previous turn,\n\
     and skipping complete would leave the task stuck in accepted forever.\n\n\
     **B-Branch 1: replaySuccess=true (or default when context is missing)**\n\n\
     **B-Step 2 -- Run complete (single sign):**\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     (Internally: POST /priapi/v1/aieco/task/{job_id}/direct/complete → get calldata → sign uopHash → broadcast on-chain.)\n\n\
     🛑 **broadcast ≠ on-chain confirmed**: `complete` CLI success = transaction broadcast submitted, not on-chain confirmed.\n\
     ❌ Do NOT call `okx-a2a user notify` here — the final completion summary is owned by the `job_completed` event (fired after on-chain confirmation).\n\
     ❌ Do NOT say \"task complete\" / \"funds settled\" / \"任务完成\" — factually wrong at this point.\n\n\
     ⚠️ **complete failure fallback**: if `onchainos agent complete` returns an error (CLI output contains `\"ok\": false` or stderr error),\n\
     notify the user via `okx-a2a user notify` and provide a retry command:\n\
     🌐 **Localize first** — translate the canonical English content below.\n\
     ```bash\n\
     okx-a2a user notify --content '<your translated content>'\n\
     ```\n\
     Canonical English content: {complete_failed}\n\
     → **End this turn** and wait for user retry or a wakeup_notify event.\n\n\
     **B-Branch 2: replaySuccess=false (only take this branch when replaySuccess=false is explicitly found in context)**\n\n\
     ⚠️ **Do not run complete** -- the user did not receive the deliverable.\n\n\
     **B-Step 2 — Check whether a `x402_replay_input` pending decision was already pushed:**\n\
     Look in this sub session's context for a `pending-decisions-v2 request ... --source-event x402_replay_input` call from the previous `job_payment_mode_changed` turn.\n\n\
     \x20\x20▸ **If yes** (a `x402_replay_input` decision was already sent to the user session):\n\
     \x20\x20\x20\x20**Skip notification.** The user will reply to the pending decision with the required parameters.\n\
     \x20\x20\x20\x20The `user_decision_x402_replay_input` handler will re-run `task-402-pay --body` and call `complete` on success.\n\
     \x20\x20\x20\x20→ **End this turn.** Do nothing further.\n\n\
     \x20\x20▸ **If no** (generic replay failure, no pending decision):\n\
     \x20\x20\x20\x20Notify the user of replay failure via `okx-a2a user notify`:\n\
     \x20\x20\x20\x20🌐 **Localize first** — translate the canonical English content below.\n\
     \x20\x20\x20\x20```bash\n\
     \x20\x20\x20\x20okx-a2a user notify --content '<your translated content>'\n\
     \x20\x20\x20\x20```\n\
     \x20\x20\x20\x20Canonical English content:\n\
     \x20\x20\x20\x20{accepted_x402_fail}\n\n\
     [Follow-up events]\n\
     - replaySuccess=true / default: job_completed → final confirmation\n\
     - replaySuccess=false + x402_replay_input pending: wait for user reply to provide body → re-replay → complete\n\
     - replaySuccess=false + no pending: wait for user instructions (retry or close task)\n\n\
     🛑🛑🛑 **job_completed MANDATORY rule**:\n\
     After complete is settled on-chain, a `job_completed` system event will arrive.\n\
     Upon receiving `job_completed`, you **MUST** call:\n\
     ```bash\n\
     onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_completed\",\"jobId\":\"{job_id}\"}}'\n\
     ```\n\
     Follow the returned playbook (it will guide you to notify the user that the job is complete).\n\
     ❌ **NEVER** ignore the `job_completed` event -- ignoring it = user never learns the job is done.\n\
     ❌ **NEVER** skip `next-action` and compose the completion notice yourself -- the playbook contains the full summary.\n") };

    let step1 = if ctx.prefetched.is_some() {
        format!("\
     **Step 1 -- Use pre-fetched task context above:**\n\
     Read {title_in_extract}description, providerAgentId, {pm_extract}tokenAmount, tokenSymbol from the `[Pre-fetched task context]` block.\n\
     ⚠️ If any field is missing, fall back to `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}`.\n\
     [Failure fallback] If all sources fail, degrade to `[Job Accepted] Job `{job_id}` has been accepted; execution begins.` — the user MUST still receive a notification.\n\n")
    } else {
        format!("\
     **Step 1 -- Fetch full task info:**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     Extract: {title_in_extract}description, providerAgentId, {pm_extract}tokenAmount, tokenSymbol.\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade to `[Job Accepted] Job `{job_id}` has been accepted; execution begins.` — the user MUST still receive a notification.\n\n")
    };

    format!(
    "[Current Status] job_accepted (user has confirmed accept; task enters execution stage)\n\
     [Role] User (User Agent)\n\n\
     🛑 **You MUST notify the user; do not produce a plain text reply inside the sub session** (see Hard Rule 9).\n\n\
     [Your next actions (strict order)]\n\n\
     {step1}\
     {branch_header}\
     {escrow_section}\
     {x402_section}"
    )
}

pub(crate) fn deliverable_received(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let (title_field, sym_field, amt_field, provider_field) = match ctx.prefetched {
        Some(p) => (
            p.title.clone(),
            p.token_symbol.clone(),
            p.token_amount.clone(),
            p.provider_agent_id.clone().unwrap_or_else(|| "<providerAgentId>".to_string()),
        ),
        None => (
            "<title>".to_string(),
            "<tokenSymbol>".to_string(),
            "<tokenAmount>".to_string(),
            "<providerAgentId>".to_string(),
        ),
    };
    format!(
    "[Current action] deliverable_received — download → save → notify\n\
     [Role] Buyer\n\n\
     Determine `deliverableType` from the ASP's message, then execute all steps in one turn.\n\n\
     **Step 1 — Download / extract**\n\
     • **file** (message has fileKey/digest/salt/nonce/secret): `okx-a2a file download --file-key <fileKey> --agent-id {agent_id} --digest <digest> --salt <salt> --nonce <nonce> --secret <secret> [--filename <filename>]` → record localPath.\n\
     • **text** (content between `- - -` separators): extract full text, write to a temp .txt file → record localPath.\n\n\
     **Step 2 — Save**\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<localPath>\" --deliverable-type <file|text> --title \"{title_field}\" \\\n\
       --short-id {short_id} \\\n\
       --counterparty-agent-id \"{provider_field}\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"{sym_field}\" --token-amount \"{amt_field}\"\n\
     ```\n\
     For file type only, add `--file-key \"<fileKey>\"`. Record savedPath from output.\n\n\
     **Step 3 — Notify user** (🌐 translate to user's language; keep data values verbatim)\n\
     ```bash\n\
     okx-a2a user notify --content '<translated content>'\n\
     ```\n\
     Canonical content:\n\
     \x20\x20[Deliverable Received] {title_field} (`{short_id}`)\n\
     \x20\x20Provider: {provider_field}\n\
     \x20\x20Type: <file|text>\n\
     \x20\x20Saved at: <savedPath>\n\
     \x20\x20(text only: show first 200 chars as preview)\n\
     \x20\x20Awaiting on-chain submission confirmation; review will follow.\n\n\
     **Step 4 — End turn**. Wait for `job_submitted` → `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'`.\n"
    )
}

/// CLI-mode fast path: download + save in-process, return a notify-only prompt.
///
/// The sub-session LLM passes deliverable metadata (deliverableType, fileKey, etc.)
/// in the `--message` JSON. This handler does the file download and deliverable save
/// entirely in Rust, then returns a minimal playbook that only asks the LLM to
/// translate and dispatch a user notification.
pub(crate) fn deliverable_received_cli(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    use crate::commands::agent_commerce::task::common::{deliverables, okx_a2a};

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let dtype = message
        .and_then(|m| m.get("deliverableType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if dtype.is_empty() {
        return deliverable_received(ctx);
    }

    let (title, sym, amt, provider_id) = match ctx.prefetched {
        Some(p) => (
            p.title.as_str(),
            p.token_symbol.as_str(),
            p.token_amount.as_str(),
            p.provider_agent_id.as_deref().unwrap_or(""),
        ),
        None => ("<title>", "<tokenSymbol>", "<tokenAmount>", ""),
    };

    let msg_str = |key: &str| {
        message
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };

    let (saved_path, original_name, deliverable_type, text_preview) = match dtype {
        "file" => {
            let file_key = msg_str("fileKey");
            let digest = msg_str("digest");
            let salt = msg_str("salt");
            let nonce = msg_str("nonce");
            let secret = msg_str("secret");
            let filename = message.and_then(|m| m.get("filename")).and_then(|v| v.as_str());

            if file_key.is_empty() || digest.is_empty() || salt.is_empty()
                || nonce.is_empty() || secret.is_empty()
            {
                return deliverable_received(ctx);
            }

            let local_path = match okx_a2a::file_download(
                file_key, agent_id, digest, salt, nonce, secret, filename,
            ) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[deliverable_received_cli] file download failed: {e}");
                    return deliverable_received(ctx);
                }
            };

            let save_result = deliverables::handle_save(&deliverables::SaveParams {
                job_id,
                role: "buyer",
                file_path: &local_path,
                deliverable_type: "file",
                title,
                short_id,
                file_key: Some(file_key),
                token_symbol: Some(sym),
                token_amount: Some(amt),
                counterparty_agent_id: if provider_id.is_empty() { None } else { Some(provider_id) },
                counterparty_name: None,
            });

            match save_result {
                Ok(r) => (
                    r.path,
                    filename.unwrap_or("deliverable").to_string(),
                    "file".to_string(),
                    String::new(),
                ),
                Err(e) => {
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
        "text" => {
            let text = msg_str("text");
            if text.is_empty() {
                return deliverable_received(ctx);
            }

            let tmp_dir = std::env::temp_dir();
            let tmp_path = tmp_dir.join(format!("deliverable-text-{job_id}.txt"));
            if let Err(e) = std::fs::write(&tmp_path, text) {
                eprintln!("[deliverable_received_cli] write temp file failed: {e}");
                return deliverable_received(ctx);
            }

            let save_result = deliverables::handle_save(&deliverables::SaveParams {
                job_id,
                role: "buyer",
                file_path: &tmp_path.display().to_string(),
                deliverable_type: "text",
                title,
                short_id,
                file_key: None,
                token_symbol: Some(sym),
                token_amount: Some(amt),
                counterparty_agent_id: if provider_id.is_empty() { None } else { Some(provider_id) },
                counterparty_name: None,
            });

            let preview = if text.len() <= 200 {
                text.to_string()
            } else {
                format!("{}…", &text[..text.char_indices().nth(200).map(|(i, _)| i).unwrap_or(text.len())])
            };

            match save_result {
                Ok(r) => (r.path, "deliverable.txt".to_string(), "text".to_string(), preview),
                Err(e) => {
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
        _ => return deliverable_received(ctx),
    };

    let preview_block = if deliverable_type == "text" && !text_preview.is_empty() {
        format!(
            "\n  ---Preview---\n  {text_preview}\n  ---End of preview---"
        )
    } else {
        String::new()
    };

    format!(
        "[deliverable_received_cli] ✓ {deliverable_type} deliverable downloaded and saved in-process.\n\
         savedPath: {saved_path}\n\
         originalName: {original_name}\n\n\
         [Your next action] Translate the canonical English notification below to the user's language, then dispatch it. End the turn after notifying.\n\n\
         Canonical content:\n\
         \x20\x20[Deliverable Received] {title} (`{short_id}`)\n\
         \x20\x20Provider: {provider_id}\n\
         \x20\x20Type: {deliverable_type}\n\
         \x20\x20Saved at: {saved_path}{preview_block}\n\
         \x20\x20\n\
         \x20\x20Awaiting on-chain submission confirmation; acceptance review will follow.\n\n\
         ```bash\n\
         okx-a2a user notify --content '<your translated content>'\n\
         ```\n\n\
         **End this turn** after notifying. Wait for `job_submitted`.\n\
         When it arrives, call `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'`.\n"
    )
}

/// Top-level dispatcher — picks the path-specific playbook based on `ctx.payment_mode`.
/// The two payment modes have completely different post-submit semantics:
///   - escrow (1): user must review (approve / reject) via a pending-decision card.
///   - x402   (3): funds already paid; just notify + auto-rate; flow ends here.
/// When `payment_mode` is `None` (rare; prefetch failure) we emit both branches with
/// a "verify paymentMode first" header so the LLM can disambiguate.
pub(crate) fn job_submitted(ctx: &FlowContext<'_>) -> String {
    match ctx.payment_mode {
        Some(1) => job_submitted_escrow(ctx),
        Some(3) => job_submitted_x402(ctx),
        _ => format!(
            "⚠️ paymentMode could not be pre-fetched. Run `onchainos agent status {job}` first to determine paymentMode (1=escrow, 3=x402), then follow the matching branch below.\n\n\
             ━━━━━━━━━ paymentMode=1 (escrow) ━━━━━━━━━\n\n\
             {escrow}\n\n\
             ━━━━━━━━━ paymentMode=3 (x402) ━━━━━━━━━\n\n\
             {x402}",
            job = ctx.job_id,
            escrow = job_submitted_escrow(ctx),
            x402 = job_submitted_x402(ctx),
        ),
    }
}

/// Escrow path (paymentMode=1):
///   Step 1 (task ctx) → Step 2a (saved check) → Step 2b (download / extract + save)
///   → Step 3 (compose review user_content) → push pending-decisions-v2 review card.
/// User must reply A (approve) / B (reject). Auto-approve is strictly forbidden.
pub(crate) fn job_submitted_escrow(ctx: &FlowContext<'_>) -> String {
    let l10n_prompt_bold = super::super::flow::L10N_PROMPT_BOLD;
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;

    // Prefetched task context + providerAgentId are required — without them we
    // cannot resolve deliverable / chat-history target / rating recipient.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[job_submitted_escrow] ❌ no prefetched task context for job {job_id}; cannot run the review flow.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_escrow] ❌ prefetched task context has no providerAgentId for job {job_id}; cannot run the review flow.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    };
    // Inline-from-prefetched values used in Step 2b's task-deliverable-save commands.
    let title = p.title.as_str();
    let token_symbol = p.token_symbol.as_str();
    let token_amount = p.token_amount.as_str();

    // Step 2 — one block, branched on whether the deliverable was already
    // persisted. Some(d) → Step 2a only (saved); None → Step 2a + Step 2b
    // (need to query + escrow download/save fallback).
    let step2 = if let Some(d) = p.deliverable.as_ref() {
        format!("\
     **Step 2a — Deliverable already saved** (detected by CLI pre-fetch; no need to call `task-deliverable-list`):\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: {dtype}\n\
     \x20\x20- For text deliverables, read the file content at localPath to get `deliverableText`\n\
     \x20\x20- Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line\n\
     \x20\x20- **Skip Step 2b entirely**\n\
     \x20\x20- Go to Step 3\n\n",
            path = d.path, dtype = d.deliverable_type,
        )
    } else {
        format!("\
     **Step 2a — Check if deliverable was already saved** (by the earlier `deliverable_received` playbook):\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role buyer\n\
     ```\n\
     If `deliverables` array is non-empty → the deliverable has already been downloaded and saved:\n\
     \x20\x20- Use the `path` from the first entry as `localPath`\n\
     \x20\x20- Use the `deliverableType` from the first entry\n\
     \x20\x20- For text deliverables, read the file content at `path` to get `deliverableText`\n\
     \x20\x20- **Skip Step 2b entirely** (no need to re-download or re-save)\n\
     \x20\x20- Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line\n\
     \x20\x20- Go to Step 3\n\n\
     If `deliverables` array is empty → the `deliverable_received` playbook did not fire or failed; fall through to Step 2b.\n\n\
     **Step 2b — Fallback: fetch from chat history and save:**\n\
     Run `okx-a2a session history` to fetch the chat history with the provider, then find the ASP message **carrying the `[intent:deliver]` suffix tag** (scan newest to oldest; first match is the deliverable):\n\
     ```bash\n\
     okx-a2a session history --job-id {job_id} --to-agent-id {provider_field} --json\n\
     ```\n\
     Then branch on `deliverableType`:\n\n\
     --- Case A: deliverableType=file (message contains fileKey / digest / salt / nonce / secret decryption fields) ---\n\n\
     Run `okx-a2a file download` to download + decrypt:\n\
     ```bash\n\
     okx-a2a file download \\\n\
     \x20\x20--file-key <fileKey> \\\n\
     \x20\x20--agent-id {agent_id} \\\n\
     \x20\x20--digest <digest> \\\n\
     \x20\x20--salt <salt> \\\n\
     \x20\x20--nonce <nonce> \\\n\
     \x20\x20--secret <secret> \\\n\
     \x20\x20[--filename <filename>]\n\
     ```\n\
     Fill `<fileKey> / <digest> / <salt> / <nonce> / <secret>` from the ASP's message; `--filename` is optional.\n\
     ⚠️ Before calling, print: `[buyer] file download: fileKey=<fileKey>, agentId={agent_id}`\n\
     ⚠️ After calling, print: `[buyer] file download result: localPath=<stdout path>`\n\n\
     stdout is the local saved path (either a plain path or a JSON `{{path: ...}}` wrapper); **it MUST be a full absolute path** (e.g. /Users/xxx/Downloads/task-staging.png).\n\
     ⚠️ **Never show only the filename** -- the user cannot locate the file. Any later content shown to the user MUST include the full path.\n\
     If download fails → note in the display: \"file download failed, please ask the ASP to resend\".\n\
     ⚠️ If the ASP message contains text alongside the file, capture it into deliverableText as well.\n\n\
     🛑 **IMMEDIATELY after download succeeds**, persist the deliverable (REQUIRED):\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<localPath>\" --deliverable-type file --title \"{title}\" \\\n\
       --short-id {short_id} --file-key \"<fileKey>\" \\\n\
       --counterparty-agent-id \"{provider_field}\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"{token_symbol}\" --token-amount \"{token_amount}\"\n\
     ```\n\
     After save, update localPath to the path printed by the save command. If save fails, log but do NOT block the review flow.\n\n\
     Deliverable display variables: deliverableType=file, localPath=<full path>, deliverableText=<note text, empty if none>\n\n\
     --- Case B: deliverableType=text (body content between `- - -` separators) ---\n\n\
     Extract the text between `- - -` separators in the `[intent:deliver]` message; **keep the original wording in full**.\n\n\
     🛑 **IMMEDIATELY after extraction**, persist the text deliverable (REQUIRED):\n\
     Write deliverableText to a temp .txt file, then:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text \\\n\
       --title \"{title}\" --short-id {short_id} \\\n\
       --counterparty-agent-id \"{provider_field}\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"{token_symbol}\" --token-amount \"{token_amount}\"\n\
     ```\n\
     After save, record the path printed by the save command as localPath.\n\n\
     Deliverable display variables: deliverableType=text, deliverableText=<full original text>, localPath=<path from save command output>\n\n")
    };

    // Step 3 — compose review card user_content + push via pending-decisions-v2.
    let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
        job_id,
        "buyer",
        agent_id,
        ctx.prefetched.and_then(|p| p.provider_agent_id.as_deref()),
        "<composed in Step 3a from the deliverableType template above — paste the localized result here verbatim, including the A. and B. option lines>",
        &format!("[Decision {short_id}] {title_display} acceptance decision"),
        "job_submitted",
    );

    format!(
    "[Current Status] job_submitted (ASP has submitted the deliverable) — paymentMode=escrow\n\
     [Role] User (User Agent)\n\n\
     🛑🛑🛑 **ABSOLUTE REQUIREMENT — you MUST push the review decision to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT `okx-a2a user notify`)**.\n\
     `okx-a2a user notify` is a pure one-way notification: user replies cannot be relayed back to the sub session → the review flow deadlocks. The correct flow handles this via `pending-decisions-v2 request` (which queues a decision card via the okx-a2a decision-request channel) so the user session can relay the review decision back.\n\
     🔴 Real incident: a model received job_submitted, sent a plain notification with \"the ASP has submitted; awaiting your review\" — the user never saw the deliverable, could not relay a decision, and the task was stuck.\n\n\
     🛑🛑🛑 **Even if you already processed the ASP's a2a-agent-chat deliverable message earlier in this turn (e.g. ran `okx-a2a file download`), upon receiving job_submitted you MUST still execute every Step below in full**.\n\
     Handling a2a-agent-chat (file download) != the review flow — the review must be driven by the job_submitted playbook, and the deliverable content (file path / text) MUST be placed into the `--user-content` of `pending-decisions-v2 request` for the user to see.\n\n\
     🛑 **Auto-approval is strictly forbidden**: wait for the user's relayed decision; the agent must not decide on behalf of the user, regardless of deliverable quality or how close to deadline.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 — Task context (pre-fetched; no CLI call needed):**\n\
     All task fields (paymentMode, tokenSymbol, providerAgentId, etc.) are in the `[Pre-fetched task context]` block above.\n\
     qualityStandards: extract from the description field above (task creation time value is authoritative).\n\n\
     **Step 2 — Obtain the deliverable content (check saved first, then fallback to chat history):**\n\n\
     ⚠️ The deliverable content MUST appear in Step 3's userContent — the user has not seen the body yet. **Do not omit, summarize, or just write \"already sent to you\".**\n\n\
     {step2}\
     --------- Step 3: escrow review — push the decision card to the user ---------\n\n\
     **Step 3a — Compose `--user-content` from Step 2's deliverable variables** (English source — fill `<placeholder>` from runtime values, THEN localize per [Localization] rules):\n\n\
     {l10n_prompt_bold}\n\n\
     ▸ deliverableType=file:\n\
     ```\n\
     [Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (file); downloaded locally.\n\
     Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     <if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     Deliverable URL: <deliverableUrl>\n\
     Payment: escrow\n\
     \n\
     Choose:\n\
     A. Approve the deliverable → reply 'A'\n\
     B. Reject the deliverable (please state your reason; if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator) → reply 'B reason: …'\n\
     ```\n\n\
     ▸ deliverableType=text — branch by localPath availability:\n\n\
     \x20\x20✅ localPath is available (save succeeded):\n\
     \x20\x20```\n\
     \x20\x20[Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (text).\n\
     \x20\x20Deliverable saved at: <localPath> (full absolute path)\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20Payment: escrow\n\
     \x20\x20\n\
     \x20\x20Choose:\n\
     \x20\x20A. Approve the deliverable → reply 'A'\n\
     \x20\x20B. Reject the deliverable (please state your reason; if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator) → reply 'B reason: …'\n\
     \x20\x20```\n\n\
     \x20\x20⚠️ localPath is unavailable (save failed — fallback to inline full text):\n\
     \x20\x20```\n\
     \x20\x20[Job {short_id} — you are the User Agent] The ASP has submitted the deliverable (text).\n\
     \x20\x20---Deliverable---\n\
     \x20\x20<deliverableText — full content, no truncation, no summarization>\n\
     \x20\x20---End of deliverable---\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20Payment: escrow\n\
     \x20\x20\n\
     \x20\x20Choose:\n\
     \x20\x20A. Approve the deliverable → reply 'A'\n\
     \x20\x20B. Reject the deliverable (please state your reason; if the ASP files a dispute, your rejection reason will be automatically submitted as evidence to the arbitrator) → reply 'B reason: …'\n\
     \x20\x20```\n\n\
     **Step 3b — Push to user via the 3-substep protocol** (use the localized `--user-content` from Step 3a; read ALL 3 sub-steps before running any command):\n\n\
     {request_block}\n"
    )
}

/// x402 path (paymentMode=3):
///   Step 1 (task ctx) → Step 2a (saved check) → Step 2b (recover deliverable from
///   task-402-pay's replayBody if not already saved) → B-1 (notify user, NO review)
///   → B-2 (auto-rate ASP, mandatory) → B-2.5 (notify rating) → B-3 (sub session
///   wrap-up). Funds were paid at job_accepted; user cannot reject.
pub(crate) fn job_submitted_x402(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let terminal_session_hint = &ctx.terminal_session_hint;
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);

    // Prefetched task context + providerAgentId are required — without them we
    // cannot resolve deliverable / rating recipient.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[job_submitted_x402] ❌ no prefetched task context for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_x402] ❌ prefetched task context has no providerAgentId for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    };

    // Step 2 (Step 2a + Step 2b combined) — branches on whether the deliverable
    // was already persisted. Some(d) → Step 2a only ("already saved"); None →
    // Step 2a ("need to query") + Step 2b (x402 replayBody recovery).
    let step2 = if let Some(d) = p.deliverable.as_ref() {
        format!("\
     **Step 2a — Deliverable already saved** (detected by CLI pre-fetch; no need to call `task-deliverable-list`):\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: {dtype}\n\
     \x20\x20- For text deliverables, read the file content at localPath to get `deliverableText`\n\
     \x20\x20- Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line\n\
     \x20\x20- **Skip Step 2b entirely**\n\
     \x20\x20- Go to Step 3\n\n",
            path = d.path, dtype = d.deliverable_type,
        )
    } else {
        format!("\
     **Step 2a — Check if deliverable was already saved** (by the earlier `deliverable_received` playbook):\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role buyer\n\
     ```\n\
     If `deliverables` array is non-empty → the deliverable has already been downloaded and saved:\n\
     \x20\x20- Use the `path` from the first entry as `localPath`\n\
     \x20\x20- Use the `deliverableType` from the first entry\n\
     \x20\x20- For text deliverables, read the file content at `path` to get `deliverableText`\n\
     \x20\x20- **Skip Step 2b entirely** (no need to re-download or re-save)\n\
     \x20\x20- Extract `qualityStandards` from the `[Pre-fetched task context]` description above; if empty, skip that line\n\
     \x20\x20- Go to Step 3\n\n\
     If `deliverables` array is empty → the `deliverable_received` playbook did not fire or failed; fall through to Step 2b.\n\n\
     **Step 2b — Recover x402 deliverable from earlier task-402-pay output:**\n\
     In x402, the deliverable was the `replayBody` returned by `task-402-pay` in the earlier `job_payment_mode_changed` turn.\n\
     ✅ The CLI auto-saved the deliverable to disk during `task-402-pay` (no manual `task-deliverable-save` needed).\n\
     Look for the `replayBodyDisplay` value in this sub session's context (it was printed when the CLI output was processed).\n\
     Set deliverable display variables: deliverableType=text, deliverableText=<replayBodyDisplay content>, localPath=<path from Step 2a task-deliverable-list if available>.\n\n")
    };

    format!(
    "[Current Status] job_submitted (ASP has submitted the deliverable) — paymentMode=x402\n\
     [Role] User (User Agent)\n\n\
     ⚠️ In x402 funds are already paid at job_accepted; the user **cannot reject the deliverable**, just notify + auto-rate.\n\
     🛑 **Even if you already processed the ASP's a2a-agent-chat deliverable message earlier in this turn**, upon receiving job_submitted you MUST still execute every Step below in full.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 — Task context (pre-fetched; no CLI call needed):**\n\
     All task fields (paymentMode, tokenSymbol, providerAgentId, etc.) are in the `[Pre-fetched task context]` block above.\n\
     qualityStandards: extract from the description field above (task creation time value is authoritative).\n\n\
     **Step 2 — Obtain the deliverable content:**\n\n\
     {step2}\
     --------- Step 3: x402 — auto-rate FIRST, then single consolidated notify ---------\n\n\
     🛑 Auto-rate the ASP FIRST, then send ONE consolidated `okx-a2a user notify` that combines the deliverable notice and the rating info.\n\n\
     **B-Step 1 — 🛑 Auto-rate the ASP FIRST (MANDATORY; must complete before B-Step 2):**\n\
     Based on the deliverable content vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id {provider_field} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId); `--creator-id` is the buyer's own agent id ({agent_id}).\n\
     Record whether feedback-submit succeeded (output contains `txHash`) or failed; the result decides whether the rating half is included in B-Step 2.\n\n\
     **B-Step 2 — Notify the user with a SINGLE consolidated message via `okx-a2a user notify`:**\n\
     🌐 **Localize first** — translate the canonical English content below into the user's language (preserve every data value verbatim — jobId hex, paths, URLs, score, comment).\n\
     ```bash\n\
     okx-a2a user notify --content '<your translated content>'\n\
     ```\n\n\
     Canonical English content — compose by merging the two halves below (concatenate with a blank line between them):\n\n\
     ▸ Deliverable received notice (always included; pick the sub-template that matches `deliverableType`):\n\n\
     \x20\x20deliverableType=file:\n\
     \x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20Deliverable file path: <localPath> (full absolute path, e.g. /Users/xxx/Downloads/task.png)\n\
     \x20\x20<if deliverableText is non-empty, append: ASP note: <deliverableText>>\n\
     \x20\x20Deliverable URL: <deliverableUrl>\n\n\
     \x20\x20deliverableType=text — branch by localPath availability:\n\n\
     \x20\x20\x20\x20✅ localPath is available (save succeeded):\n\
     \x20\x20\x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20\x20\x20Deliverable saved at: <localPath> (full absolute path)\n\
     \x20\x20\x20\x20Deliverable URL: <deliverableUrl>\n\
     \x20\x20\x20\x20---Deliverable (preview)---\n\
     \x20\x20\x20\x20<first 200 characters of deliverableText; if total length ≤ 200, show full text and use ---Deliverable--- / ---End of deliverable--- headers instead>\n\
     \x20\x20\x20\x20---End of preview---\n\
     \x20\x20\x20\x20<if deliverableText was truncated, append: (… truncated; full content saved locally)>\n\n\
     \x20\x20\x20\x20⚠️ localPath is unavailable (save failed — fallback to inline full text):\n\
     \x20\x20\x20\x20[Deliverable Received] Job `{job_id}` — the ASP has submitted the deliverable (x402 mode; payment already settled).\n\
     \x20\x20\x20\x20---Deliverable---\n\
     \x20\x20\x20\x20<deliverableText — full content, no truncation, no summarization>\n\
     \x20\x20\x20\x20---End of deliverable---\n\
     \x20\x20\x20\x20Deliverable URL: <deliverableUrl>\n\n\
     ▸ Rating info (include ONLY if B-Step 1's feedback-submit succeeded; if it failed, omit this entire half):\n\
     \x20\x20{rating_notify}\n\
     \x20\x20(fill `<score>` with the X.XX value used in B-Step 1, `<description>` with the comment from B-Step 1, `<title>` from task context)\n\n\
     **B-Step 3 — Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n"
    )
}

/// Directly runs `onchainos agent complete` in-process. The single-arg bash
/// command provides no LLM decision-making value — Rust just broadcasts and
/// returns. Iron rules from the previous LLM-driven version ("don't notify
/// user via okx-a2a user notify / don't auto-rate / don't say funds released
/// before job_completed") all become moot — Rust cannot misbehave.
///
/// Failure path: the playbook emitted on error directs the LLM into the
/// standard cli_failed 5-substep protocol (push a decision to the user).
pub(crate) async fn approve_review(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;
    let mut client = TaskApiClient::new();
    match super::super::complete::handle_complete(&mut client, job_id).await {
        Ok(()) => format!(
            "[approve_review] ✅ `onchainos agent complete {job_id}` broadcast by Rust in-process. End the turn now.\n\n\
             ⚠️ broadcast ≠ on-chain confirmed. The `job_completed` system event will fire after on-chain confirmation — handle it via `next-action` with `event=job_completed` in --message to notify the user.\n"
        ),
        Err(e) => format!(
            "[approve_review] ❌ `onchainos agent complete {job_id}` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    }
}

/// Directly runs `onchainos agent reject` in-process. The rejection reason
/// is expected on `ctx.data` (forwarded from `next-action --data` by the
/// `user_decision_job_submitted` router after the LLM extracts it from
/// the relayed user reply); falls back to "did not meet acceptance
/// criteria" when absent. Iron rules from the previous LLM-driven version
/// ("don't send a message to the ASP about the rejection") become moot —
/// Rust just broadcasts and returns.
///
/// Failure path: standard cli_failed instruction (push decision to user).
pub(crate) async fn reject_review(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
    let job_id = ctx.job_id;

    let reason = ctx
        .data
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("did not meet acceptance criteria");

    let mut client = TaskApiClient::new();
    match super::super::reject::handle_reject(&mut client, job_id, reason).await {
        Ok(()) => format!(
            "[reject_review] ✅ `onchainos agent reject {job_id} --reason \"{reason}\"` broadcast in-process. End the turn now.\n\n\
             ⚠️ broadcast ≠ on-chain confirmed. The `job_rejected` system event will fire after on-chain confirmation; the ASP then decides whether to dispute (arbitration) or agree to a refund. The buyer cannot initiate arbitration.\n\
             ❌ Do NOT send any message to the ASP about the rejection — they learn via on-chain events.\n"
        ),
        Err(e) => format!(
            "[reject_review] ❌ `onchainos agent reject {job_id} --reason \"{reason}\"` failed in-process: {e}\n\n\
             Push a `cli_failed` decision to the user via `pending-decisions-v2 request` (see SKILL.md §Exception Escalation 5-substep protocol). Do NOT retry blindly.\n"
        ),
    }
}

// --- Terminal states ---------------------------------------------------

/// Primary `job_completed` playbook — on-chain confirmation notification.
///
/// This event fires when the blockchain confirms the `complete` transaction.
/// It is the ONLY place where "funds released" is factually true.
/// `approve_review` only broadcasts; this event confirms.
pub(crate) fn job_completed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let completed_escrow_notify = super::super::content::job_completed_escrow_user_notify(job_id, title_display);
    let completed_x402_notify = super::super::content::job_completed_x402_user_notify(job_id, title_display);
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id);

    let pm = ctx.payment_mode;
    let pm_extract = if pm.is_some() { "" } else { ", paymentMode (int: 1=escrow, 3=x402)" };
    let branch_header = if pm.is_none() { "**Step 2 -- Branch by payment mode:**\n\n" } else { "" };

    let escrow_section = if pm == Some(3) { String::new() } else { format!("\
     --------- Branch A: escrow -- flow ends ---------\n\n\
     In escrow mode, job_completed means the ASP has delivered and the user has approved; funds are released from contract to the ASP.\n\n\
     **A-Step 1 — 🛑 Auto-rate the ASP FIRST (MANDATORY; must complete before A-Step 2):**\n\
     Based on the deliverable that was reviewed vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\
     Record whether feedback-submit succeeded (output contains `txHash`) or failed; the result decides whether the rating half is included in A-Step 2.\n\n\
     **A-Step 2 — Notify the user with a SINGLE consolidated message via `okx-a2a user notify`:**\n\
     🛑🛑🛑 You are in a **sub session (backup)**. Any text you output here is invisible to the user. The ONLY way to reach the user is `okx-a2a user notify`.\n\
     ⚠️ txHash: find the txHash (format 0x...) from the earlier `onchainos agent complete` CLI output in this sub session context. If not in context (e.g. auto-complete scenarios), omit the on-chain receipt line.\n\
     🌐 **Localize first** — translate the canonical English content below into the user's language (preserve txHash / score / amounts / title verbatim).\n\
     ```bash\n\
     okx-a2a user notify --content '<your translated content>'\n\
     ```\n\n\
     Canonical English content — compose by merging the two halves below (concatenate with a blank line between them):\n\n\
     ▸ Completion notice (always included):\n\
     \x20\x20{completed_escrow_notify}\n\n\
     ▸ Rating info (include ONLY if A-Step 1's feedback-submit succeeded; if it failed, omit this entire half):\n\
     \x20\x20{rating_notify}\n\
     \x20\x20(fill `<score>` with the X.XX value used in A-Step 1, `<description>` with the comment from A-Step 1, `<title>` from task context)\n\n\
     **A-Step 3 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n") };

    let x402_section = if pm == Some(1) { String::new() } else { format!("\
     --------- Branch B: x402 -- final summary + auto-rate ---------\n\n\
     ⚠️ In x402, job_completed means the payment pipeline (accept + complete) is settled on-chain.\n\
     The deliverable was already sent to the user during task-402-pay; this step rates the ASP and emits the final summary.\n\n\
     **B-Step 1 — 🛑 Auto-rate the ASP FIRST (MANDATORY; must complete before B-Step 2):**\n\
     Based on the deliverable (the `replayBody` from task-402-pay in this sub session context) vs the task description and quality standards, generate:\n\
     \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: 5.00 = exceeds expectations, 4.00 = fully meets, 3.00 = acceptable with minor gaps, 2.00 = partially meets, 1.00 = mostly inadequate, 0.00 = did not deliver.\n\
     \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
     Then execute:\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
     ```\n\
     ⚠️ `--agent-id` is the ASP being rated (providerAgentId from Step 1 context); `--creator-id` is the buyer's own agent id ({agent_id}).\n\
     Record whether feedback-submit succeeded (output contains `txHash`) or failed; the result decides whether the rating half is included in B-Step 2.\n\n\
     **B-Step 2 — Notify the user with a SINGLE consolidated message via `okx-a2a user notify`:**\n\
     🛑🛑🛑 You are in a **sub session (backup)**. Any text you output here is invisible to the user. The ONLY way to reach the user is `okx-a2a user notify`.\n\
     🌐 **Localize first** — translate the canonical English content below into the user's language (preserve score / amounts / title verbatim).\n\
     ```bash\n\
     okx-a2a user notify --content '<your translated content>'\n\
     ```\n\n\
     Canonical English content — compose by merging the two halves below (concatenate with a blank line between them):\n\n\
     ▸ Completion notice (always included):\n\
     \x20\x20{completed_x402_notify}\n\n\
     ▸ Rating info (include ONLY if B-Step 1's feedback-submit succeeded; if it failed, omit this entire half):\n\
     \x20\x20{rating_notify}\n\
     \x20\x20(fill `<score>` with the X.XX value used in B-Step 1, `<description>` with the comment from B-Step 1, `<title>` from task context)\n\n\
     **B-Step 3 -- Terminal wrap-up (keep the sub session):**\n\
     {terminal_session_hint}\n\
     Task fully complete.\n\n") };

    format!(
    "🚨🚨🚨 **NEW SYSTEM EVENT — ON-CHAIN CONFIRMATION** 🚨🚨🚨\n\
     This is `job_completed` — the blockchain has **confirmed** the complete transaction.\n\
     ⚠️ You may have called `onchainos agent complete` earlier — that was only the **broadcast**.\n\
     This event is NOT a duplicate or confirmation of your previous action — it is a **new mandatory event** carrying the on-chain result.\n\
     🔴 **The user has NOT received the completion summary yet.** If you skip this playbook, the user will never know the task is done.\n\n\
     [Current Status] job_completed (on-chain confirmed — task settled)\n\
     [Role] User (User Agent)\n\n\
     🛑 You are inside a sub/backup session. Execute the steps below verbatim, in order — auto-rate FIRST, then send a single consolidated `okx-a2a user notify`. Do NOT add steps, do NOT skip.\n\n\
     **Step 1 -- Fetch task context (if needed):**\n\
     Extract {title_in_extract}providerAgentId, tokenAmount, tokenSymbol{pm_extract} from this sub session's context.\n\
     If not available, run:\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     [common context failure fallback] If the command fails or fields are missing, drop dynamic fields and degrade to `[Job Completed] Job `{job_id}` — completed; funds settled.` — the user MUST still receive a notification.\n\n\
     {branch_header}\
     {escrow_section}\
     {x402_section}\
     🛑 Forbidden: `sessions_spawn`, `sessions_yield`, sending any message to provider, plain text replies inside the sub session.\n\n\
     [OUTPUT_TEMPLATE]\n\
     Your entire response for this event MUST include the following tool calls, in order:\n\
     1. One `onchainos agent feedback-submit` call — auto-rate the ASP (A/B-Step 1)\n\
     2. One `okx-a2a user notify` bash call — consolidated completion + rating notification (A/B-Step 2)\n\
     Skipping the rating or sending the notification before rating is a **critical failure** — the user will never see their rating.\n"
    )
}
