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
                 See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
            );
        }

        let short_id = ctx.short_id;
        let user_content = format!(
            "[Job {short_id} — you are the User Agent] The ASP's quote exceeded the maximum budget for this task. The apply has been rejected automatically.\n\n\
             What would you like to do next?\n\
             A. Browse the ASP list\n\
             B. Designate a specific ASP by agentId\n\
             {option_public_line}{close_label}. Close the task"
        );
        let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id, "buyer", agent_id, None,
            &user_content,
            &format!("[Over budget {short_id}] next-step decision"),
            "apply_over_budget",
        );

        return format!(
        "🛑 Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         {request_block}\n"
        );
    }

    // ── Within-budget branch: confirm-accept on-chain (escrow funded; status → accepted) ──
    if let Err(e) = super::super::accept::handle_confirm_accept(&mut client, job_id, ctx.prefetched).await {
        return format!(
            "[provider_applied/confirm_accept] ❌ confirm-accept failed in-process: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        );
    }

    "**End this turn** and wait for the `job_accepted` system notification.".to_string()
}

pub(crate) fn job_accepted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let pm = ctx.payment_mode;

    // ── Escrow: CLI fills all values, LLM just localizes + sends ──
    if pm != Some(3) {
        let (title, desc, provider_id, amount, symbol) = match ctx.prefetched {
            Some(p) => (
                p.title.as_str(),
                if p.description.is_empty() { "<description>" } else { p.description.as_str() },
                p.provider_agent_id.as_deref().unwrap_or("<providerAgentId>"),
                p.token_amount.as_str(),
                p.token_symbol.as_str(),
            ),
            None => ("<title>", "<description>", "<providerAgentId>", "<tokenAmount>", "<tokenSymbol>"),
        };

        let l10n = super::super::flow::LOCALIZATION_PREFIX;
        return format!(
            "{l10n}\
             ✓ job_accepted (escrow). Notify the user:\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>'\n\
             ```\n\
             Template (translate to user's language, keep structure):\n\
             \x20\x20[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
             \x20\x20Title: {title}\n\
             \x20\x20Description: {desc}\n\
             \x20\x20ASP agentId: {provider_id}\n\
             \x20\x20Payment: escrow\n\
             \x20\x20Amount: {amount} {symbol}\n\n\
             End turn after notifying.\n"
        );
    }

    // ── x402: LLM needs to determine replaySuccess + run complete ──
    let l10n = super::super::flow::LOCALIZATION_PREFIX;
    let accepted_x402_fail = super::super::content::job_accepted_x402_replay_fail_user_notify(job_id);
    let complete_failed = super::super::content::complete_failed_user_notify(job_id);

    format!(
    "{l10n}\
     [Current Status] job_accepted (x402 — funds already paid)\n\n\
     **Step 1 -- Determine replaySuccess from the previous turn's task-402-pay:**\n\
     Look up the task-402-pay output in this sub session context.\n\
     ⚠️ If not found (e.g. context compaction), **default to replaySuccess=true** —\n\
     skipping complete would leave the task stuck in accepted forever.\n\n\
     **Branch 1: replaySuccess=true (or default)**\n\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     🛑 broadcast ≠ on-chain confirmed. Do NOT notify user or say \"task complete\" here.\n\
     ⚠️ On error → notify user:\n\
     ```bash\n\
     okx-a2a user notify --content '<localized content>'\n\
     ```\n\
     Content: {complete_failed}\n\
     → End turn, wait for retry or wakeup_notify.\n\n\
     **Branch 2: replaySuccess=false (explicitly found in context)**\n\n\
     Do not run complete.\n\
     Check whether a `x402_replay_input` pending decision was already pushed in the previous turn:\n\
     ▸ Yes → end turn (user will reply to the pending decision).\n\
     ▸ No → notify user:\n\
     ```bash\n\
     okx-a2a user notify --content '<localized content>'\n\
     ```\n\
     Content: {accepted_x402_fail}\n\
     → Wait for `job_completed` system event.\n"
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

    // When the review marker exists, job_submitted already arrived. After manual
    // download + save succeeds, the LLM should re-trigger job_submitted (which will
    // now find the manifest) instead of waiting for an event that already came.
    let marker_exists = crate::commands::agent_commerce::task::common::deliverables::has_review_marker(job_id);
    let step4 = if marker_exists {
        format!(
            "**Step 4 — Re-trigger review** (job_submitted already arrived before this deliverable):\n\
             ```bash\n\
             onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'\n\
             ```\n"
        )
    } else {
        format!(
            "**Step 4 — End turn**. Wait for `job_submitted` → `onchainos agent next-action --role buyer --agentId {agent_id} --message '{{\"event\":\"job_submitted\",\"jobId\":\"{job_id}\"}}'`.\n"
        )
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
     **Step 3 — Notify user**\n\
     ```bash\n\
     okx-a2a user notify --content '<localized content>'\n\
     ```\n\
     Content:\n\
     \x20\x20[Deliverable Received] {title_field} (`{short_id}`)\n\
     \x20\x20Provider: {provider_field}\n\
     \x20\x20Type: <file|text>\n\
     \x20\x20Saved at: <savedPath>\n\
     \x20\x20Awaiting on-chain submission confirmation; review will follow.\n\n\
     {step4}"
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
    use crate::audit;
    use crate::commands::agent_commerce::task::common::{deliverables, okx_a2a};
    use std::time::Duration;

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let base_tags = vec![format!("jobId={job_id}"), format!("agentId={agent_id}")];

    let dtype = message
        .and_then(|m| m.get("deliverableType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if dtype.is_empty() {
        audit::log("cli", "buyer/deliverable_received_no_type", false, Duration::default(),
            Some(base_tags.clone()), Some("deliverableType missing, fallback to LLM path"));
        return deliverable_received(ctx);
    }

    audit::log("cli", "buyer/deliverable_received", true, Duration::default(),
        Some([base_tags.clone(), vec![format!("type={dtype}")]].concat()), None);

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

    let (saved_path, deliverable_type) = match dtype {
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
                audit::log("cli", "buyer/deliverable_file_missing_metadata", false, Duration::default(),
                    Some(base_tags.clone()), Some("encryption metadata incomplete, fallback to LLM path"));
                return deliverable_received(ctx);
            }

            audit::log("cli", "buyer/deliverable_file_download", true, Duration::default(),
                Some([base_tags.clone(), vec![format!("fileKey={file_key}")]].concat()), None);

            let local_path = match okx_a2a::file_download(
                file_key, agent_id, digest, salt, nonce, secret, filename,
            ) {
                Ok(p) => {
                    audit::log("cli", "buyer/deliverable_file_downloaded", true, Duration::default(),
                        Some([base_tags.clone(), vec![format!("localPath={p}")]].concat()), None);
                    p
                }
                Err(e) => {
                    audit::log("cli", "buyer/deliverable_file_download_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec![format!("fileKey={file_key}")]].concat()), Some(&e.to_string()));
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
                Ok(r) => {
                    audit::log("cli", "buyer/deliverable_saved", true, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into(), format!("path={}", r.path)]].concat()), None);
                    (r.path, "file".to_string())
                }
                Err(e) => {
                    audit::log("cli", "buyer/deliverable_save_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into()]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
        "text" => {
            let file_path = msg_str("filePath");
            if file_path.is_empty() {
                audit::log("cli", "buyer/deliverable_text_no_filepath", false, Duration::default(),
                    Some(base_tags.clone()), Some("filePath missing, fallback to LLM path"));
                return deliverable_received(ctx);
            }
            let fp = std::path::Path::new(file_path);
            let tmp_dir = std::env::temp_dir();
            if !fp.starts_with(&tmp_dir) {
                audit::log("cli", "buyer/deliverable_text_path_rejected", false, Duration::default(),
                    Some(base_tags.clone()), Some("filePath not under temp dir"));
                eprintln!("[deliverable_received_cli] filePath must be under temp dir: {file_path}");
                return deliverable_received(ctx);
            }
            let raw = match std::fs::read_to_string(fp) {
                Ok(content) => content,
                Err(e) => {
                    audit::log("cli", "buyer/deliverable_text_read_failed", false, Duration::default(),
                        Some(base_tags.clone()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] read filePath failed: {e}");
                    return deliverable_received(ctx);
                }
            };

            // Extract text between first and last `- - -`. Use rfind for the
            // closing separator so deliverable text containing `- - -` is preserved.
            let text = if let Some(start) = raw.find("- - -") {
                let after = start + 5;
                let body = if let Some(rel_end) = raw[after..].rfind("- - -") {
                    &raw[after..after + rel_end]
                } else {
                    &raw[after..]
                };
                body.trim().to_string()
            } else {
                raw.trim().to_string()
            };

            audit::log("cli", "buyer/deliverable_text_parsed", true, Duration::default(),
                Some([base_tags.clone(), vec![format!("charCount={}", text.chars().count())]].concat()), None);

            let tmp_path = tmp_dir.join(format!("deliverable-text-{job_id}.txt"));
            if let Err(e) = std::fs::write(&tmp_path, &text) {
                audit::log("cli", "buyer/deliverable_text_write_failed", false, Duration::default(),
                    Some(base_tags.clone()), Some(&e.to_string()));
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

            match save_result {
                Ok(r) => {
                    audit::log("cli", "buyer/deliverable_saved", true, Duration::default(),
                        Some([base_tags.clone(), vec!["type=text".into(), format!("path={}", r.path)]].concat()), None);
                    (r.path, "text".to_string())
                }
                Err(e) => {
                    audit::log("cli", "buyer/deliverable_save_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=text".into()]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
        _ => {
            audit::log("cli", "buyer/deliverable_received_unknown_type", false, Duration::default(),
                Some([base_tags.clone(), vec![format!("type={dtype}")]].concat()), Some("unknown deliverableType, fallback to LLM path"));
            return deliverable_received(ctx);
        }
    };

    // Out-of-order handling: if the review marker exists, job_submitted already arrived
    // before this deliverable. Delete marker → directly output the review prompt so the
    // sub doesn't wait for a job_submitted that already came.
    if deliverables::has_review_marker(job_id) {
        deliverables::delete_review_marker(job_id);
        audit::log("cli", "buyer/deliverable_received_marker_found", true, Duration::default(),
            Some(base_tags.clone()), Some("job_submitted arrived first; merging into review flow"));

        // Reconstruct prefetched with the just-saved deliverable so job_submitted_escrow
        // sees it in Step 2 ("Deliverable already saved").
        let mut patched = ctx.prefetched.cloned().unwrap_or_else(|| {
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext {
                title: title.to_string(),
                description: String::new(),
                token_symbol: sym.to_string(),
                token_amount: amt.to_string(),
                payment_mode: ctx.payment_mode,
                max_budget: None,
                provider_agent_id: if provider_id.is_empty() { None } else { Some(provider_id.to_string()) },
                buyer_agent_id: None,
                visibility: None,
                status: None,
                deliverable: None,
                service_id: None,
                service_token_address: None,
                service_token_amount: None,
                service_params: None,
                buyer_agent_address: None,
                token_address: None,
            }
        });
        patched.deliverable = Some(crate::commands::agent_commerce::task::common::PreFetchedDeliverable {
            path: saved_path.clone(),
            deliverable_type: deliverable_type.clone(),
            original_name: String::new(),
        });

        let merged_ctx = super::super::flow::FlowContext {
            job_id: ctx.job_id,
            agent_id: ctx.agent_id,
            short_id: ctx.short_id,
            title_display: ctx.title_display,
            title_query_hint: ctx.title_query_hint,
            title_in_extract: ctx.title_in_extract,
            terminal_session_hint: ctx.terminal_session_hint.clone(),
            payment_mode: ctx.payment_mode,
            prefetched: Some(&patched),
            data: ctx.data,
        };
        return job_submitted_escrow(&merged_ctx);
    }

    let l10n = super::super::flow::LOCALIZATION_PREFIX;
    format!(
        "{l10n}\
         ✓ {deliverable_type} deliverable saved.\n\
         savedPath: {saved_path}\n\
         title: {title} | shortId: {short_id} | provider: {provider_id}\n\n\
         Notify the user:\n\
         ```bash\n\
         okx-a2a user notify --content '<localized content>'\n\
         ```\n\
         Template (translate to user's language, keep structure; path must be full absolute — never abbreviate):\n\
         \x20\x20[Deliverable Received] {title} (`{short_id}`)\n\
         \x20\x20Provider: {provider_id}\n\
         \x20\x20Type: {deliverable_type}\n\
         \x20\x20Saved at: {saved_path}\n\
         \x20\x20Awaiting on-chain submission confirmation; acceptance review will follow.\n\n\
         End turn after notifying.\n"
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
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_escrow] ❌ prefetched task context has no providerAgentId for job {job_id}; cannot run the review flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    // Out-of-order handling: if the review marker exists, deliverable_received hasn't
    // arrived yet (this is the first job_submitted with no deliverable). Return a
    // lightweight "wait" prompt — deliverable_received_cli will detect the marker and
    // output the review flow when it arrives.
    if p.deliverable.is_none() && crate::commands::agent_commerce::task::common::deliverables::has_review_marker(job_id) {
        return format!(
            "[System] job_submitted received but deliverable has not arrived yet (XMTP [intent:deliver] pending).\n\
             The review flow will auto-trigger when the deliverable is received.\n\
             No action required — end this turn and wait.\n"
        );
    }

    // Inline-from-prefetched values used in Step 2b's task-deliverable-save commands.
    let title = p.title.as_str();
    let token_symbol = p.token_symbol.as_str();
    let token_amount = p.token_amount.as_str();

    let step2 = if let Some(d) = p.deliverable.as_ref() {
        format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: {dtype}\n\
     \x20\x20- For text deliverables, read the file content at localPath to get `deliverableText`\n\n",
            path = d.path, dtype = d.deliverable_type,
        )
    } else {
        format!("\
     **Step 2a — Check saved deliverable:**\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role buyer\n\
     ```\n\
     Non-empty `deliverables` → use first entry's `path` as localPath, `deliverableType`; skip Step 2b.\n\
     Empty → fall through to Step 2b.\n\n\
     **Step 2b — Fallback: fetch from chat history:**\n\
     ```bash\n\
     okx-a2a session history --job-id {job_id} --to-agent-id {provider_field} --json\n\
     ```\n\
     Find the ASP message with `[intent:deliver]` suffix (newest first).\n\n\
     ▸ Case A (file — message has fileKey/digest/salt/nonce/secret):\n\
     ```bash\n\
     okx-a2a file download --file-key <fileKey> --agent-id {agent_id} --digest <digest> --salt <salt> --nonce <nonce> --secret <secret> [--filename <filename>]\n\
     ```\n\
     stdout = localPath (must be full absolute path). Then persist:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<localPath>\" --deliverable-type file --title \"{title}\" \\\n\
       --short-id {short_id} --file-key \"<fileKey>\" \\\n\
       --counterparty-agent-id \"{provider_field}\" --counterparty-name \"<providerName>\" \\\n\
       --token-symbol \"{token_symbol}\" --token-amount \"{token_amount}\"\n\
     ```\n\n\
     ▸ Case B (text — body between `- - -` separators):\n\
     Extract full text → write to temp .txt → persist:\n\
     ```bash\n\
     onchainos agent task-deliverable-save --job-id {job_id} --role buyer \\\n\
       --file \"<temp .txt path>\" --deliverable-type text --title \"{title}\" \\\n\
       --short-id {short_id} --counterparty-agent-id \"{provider_field}\" \\\n\
       --counterparty-name \"<providerName>\" --token-symbol \"{token_symbol}\" --token-amount \"{token_amount}\"\n\
     ```\n\
     After save, update localPath from save command output.\n\n")
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
    "🛑 MUST use `pending-decisions-v2 request` — NOT `okx-a2a user notify` (one-way = no relay = deadlock). Auto-approval forbidden.\n\
     🛑 Even if deliverable was already downloaded this turn, execute ALL steps below.\n\n\
     [Your next actions (strict order)]\n\n\
     {step2}\
     **Step 3 — Compose `--user-content` and push decision card:**\n\n\
     Compose `--user-content` from Step 2's deliverable variables (fill placeholders from runtime values):\n\n\
     ⚠️ `<localPath>` must be the full absolute path (e.g. /Users/xxx/…). Never abbreviate or shorten.\n\n\
     ▸ deliverableType=file:\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (file).\n\
     File path: <localPath>\n\
     <if deliverableText non-empty: ASP note: <deliverableText>>\n\
     Payment: escrow\n\
     A. Approve → reply 'A'\n\
     B. Reject (state reason; used as evidence if disputed) → reply 'B reason: …'\n\
     ```\n\n\
     ▸ deliverableType=text (localPath available):\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (text).\n\
     Saved at: <localPath>\n\
     Payment: escrow\n\
     A. Approve → reply 'A'\n\
     B. Reject (state reason; used as evidence if disputed) → reply 'B reason: …'\n\
     ```\n\n\
     ▸ deliverableType=text (localPath unavailable — inline full text):\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (text).\n\
     ---Deliverable---\n\
     <deliverableText — full content, no truncation>\n\
     ---End of deliverable---\n\
     Payment: escrow\n\
     A. Approve → reply 'A'\n\
     B. Reject (state reason; used as evidence if disputed) → reply 'B reason: …'\n\
     ```\n\n\
     Push to user (localize `--user-content` and `--list-label` to user's language first):\n\n\
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
    let title_display = ctx.title_display;
    let terminal_session_hint = &ctx.terminal_session_hint;
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id, title_display);

    // Prefetched task context + providerAgentId are required — without them we
    // cannot resolve deliverable / rating recipient.
    let p = match ctx.prefetched {
        Some(p) => p,
        None => return format!(
            "[job_submitted_x402] ❌ no prefetched task context for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_x402] ❌ prefetched task context has no providerAgentId for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };

    let step2 = if let Some(d) = p.deliverable.as_ref() {
        format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: {dtype}\n\
     \x20\x20- For text deliverables, read the file content at localPath to get `deliverableText`\n\n",
            path = d.path, dtype = d.deliverable_type,
        )
    } else {
        format!("\
     **Step 2a — Check saved deliverable:**\n\
     ```bash\n\
     onchainos agent task-deliverable-list --job-id {job_id} --role buyer\n\
     ```\n\
     Non-empty `deliverables` → use first entry's `path`/`deliverableType`; skip Step 2b.\n\
     Empty → fall through to Step 2b.\n\n\
     **Step 2b — Recover from earlier task-402-pay output:**\n\
     The deliverable was the `replayBody` from `task-402-pay` (auto-saved by CLI).\n\
     Look for `replayBodyDisplay` in this sub session's context.\n\
     Set: deliverableType=text, deliverableText=<replayBodyDisplay>, localPath=<path from Step 2a if available>.\n\n")
    };

    format!(
    "⚠️ x402: funds already paid; user cannot reject — notify + auto-rate only.\n\n\
     [Your next actions (strict order)]\n\n\
     {step2}\
     **Step 3 — Auto-rate ASP, then notify user:**\n\n\
     **3a — Rate the ASP (mandatory, before notify):**\n\
     Score 0.00–5.00 based on deliverable vs description. Comment ≤100 chars.\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id {provider_field} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment>\"\n\
     ```\n\
     `--agent-id` = ASP being rated; `--creator-id` = buyer's agent id.\n\n\
     **3b — Notify user (deliverable + rating in one message):**\n\
     ```bash\n\
     okx-a2a user notify --content '<localized content>'\n\
     ```\n\
     Compose from two halves (concatenate with two blank lines):\n\
     \x20\x20▸ Deliverable (always; pick template):\n\
     \x20\x20\x20\x20file: `[Deliverable Received] Job {job_id} — x402, payment settled. File: <localPath>`\n\
     \x20\x20\x20\x20text+path: `[Deliverable Received] Job {job_id} — x402, payment settled. Saved at: <localPath>`\n\
     \x20\x20\x20\x20text-no-path: `[Deliverable Received] Job {job_id} — x402, payment settled.` + full deliverableText inline\n\
     \x20\x20▸ Rating (include ONLY if feedback-submit succeeded; if it failed or errored, **omit this entire half**):\n\
     \x20\x20\x20\x20{rating_notify}\n\
     \x20\x20\x20\x20(fill `<score>` with the X.XX value used in 3a, `<description>` with the comment from 3a)\n\n\
     **3c — Terminal wrap-up:**\n\
     {terminal_session_hint}\n"
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
        Ok(()) => "**End this turn** and wait for the `job_completed` system notification.".to_string(),
        Err(e) => format!(
            "[approve_review] ❌ `onchainos agent complete {job_id}` failed in-process: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
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
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    }
}

// --- Terminal states ---------------------------------------------------

/// Primary `job_completed` playbook — on-chain confirmation notification.
///
/// This event fires when the blockchain confirms the `complete` transaction.
/// It is the ONLY place where "funds released" is factually true.
/// `approve_review` only broadcasts; this event confirms.
pub(crate) fn job_completed(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let terminal_session_hint = &ctx.terminal_session_hint;

    let provider_id = ctx.prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("<providerAgentId>");

    let (token_amount, token_symbol) = ctx.prefetched
        .map(|p| (p.token_amount.as_str(), p.token_symbol.as_str()))
        .unwrap_or(("<tokenAmount>", "<tokenSymbol>"));

    let tx_hash = message
        .and_then(|m| m.get("txHash"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("<txHash from the system event message>");

    let pm = ctx.payment_mode;

    let completed_notify = if pm == Some(3) {
        super::super::content::job_completed_x402_user_notify(job_id, title_display)
    } else {
        super::super::content::job_completed_escrow_user_notify(job_id, title_display, token_amount, token_symbol, tx_hash)
    };
    let rating_notify = super::super::content::rating_submitted_user_notify(job_id, title_display);

    let l10n = super::super::flow::LOCALIZATION_PREFIX;
    format!(
        "{l10n}\
         ✓ job_completed — on-chain confirmed. Rate ASP, then notify user in one message.\n\n\
         **Step 1 — Rate ASP** (0.00–5.00, comment ≤100 chars):\n\
         ```bash\n\
         onchainos agent feedback-submit --agent-id {provider_id} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment>\"\n\
         ```\n\n\
         **Step 2 — Notify user** (completion + rating):\n\
         ```bash\n\
         okx-a2a user notify --content '<localized content>'\n\
         ```\n\
         Template:\n\
         \x20\x20{completed_notify}\n\
         \x20\x20{rating_notify}  ← omit if Step 1 failed\n\n\
         **Step 3 — Wrap-up:**\n\
         {terminal_session_hint}\n"
    )
}
