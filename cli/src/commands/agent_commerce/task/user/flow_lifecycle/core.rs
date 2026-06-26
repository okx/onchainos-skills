//! Core happy-path lifecycle prompt generators.

use super::super::flow::FlowContext;

// ── A2A deliver content parser ──────────────────────────────────────────

/// Parsed deliverable from the A2A message `content` field.
enum DeliverPayload {
    File {
        file_key: String,
        digest: String,
        salt: String,
        nonce: String,
        secret: String,
        filename: Option<String>,
    },
    Text(String),
}

/// Parse the `content` field of an `[intent:deliver]` A2A message.
///
/// File format:
/// ```text
/// jobId: 0x...
/// deliverableType: file
/// fileKey: ...
/// digest: ...
/// salt: ...
/// nonce: ...
/// secret: ...
/// filename: ...
/// [intent:deliver]
/// ```
///
/// Text format:
/// ```text
/// jobId: 0x...
/// deliverableType: text
/// - - -
/// <content>
/// - - -
/// [intent:deliver]
/// ```
fn parse_deliver_content(content: &str) -> Option<DeliverPayload> {
    if !content.contains("[intent:deliver]") {
        return None;
    }

    let kv = |key: &str| -> Option<String> {
        content.lines()
            .find(|line| {
                let trimmed = line.trim();
                trimmed.starts_with(key) && trimmed[key.len()..].starts_with(':')
            })
            .map(|line| line.trim()[key.len() + 1..].trim().to_string())
    };

    let dtype = kv("deliverableType")?;

    match dtype.as_str() {
        "file" => {
            let file_key = kv("fileKey").filter(|s| !s.is_empty())?;
            let digest = kv("digest").filter(|s| !s.is_empty())?;
            let salt = kv("salt").filter(|s| !s.is_empty())?;
            let nonce = kv("nonce").filter(|s| !s.is_empty())?;
            let secret = kv("secret").filter(|s| !s.is_empty())?;
            let filename = kv("filename").filter(|s| !s.is_empty());
            Some(DeliverPayload::File { file_key, digest, salt, nonce, secret, filename })
        }
        "text" => {
            let start = content.find("- - -")?;
            let after = start + 5;
            let body = if let Some(rel_end) = content[after..].rfind("- - -") {
                &content[after..after + rel_end]
            } else {
                &content[after..]
            };
            let trimmed = body.trim();
            if trimmed.is_empty() { return None; }
            Some(DeliverPayload::Text(trimmed.to_string()))
        }
        _ => None,
    }
}

fn is_safe_temp_path(fp: &std::path::Path) -> bool {
    let tmp_dir = std::env::temp_dir();
    if fp.starts_with(&tmp_dir) {
        return true;
    }
    #[cfg(unix)]
    {
        if fp.starts_with("/tmp/") {
            return true;
        }
    }
    if let (Ok(c_fp), Ok(c_tmp)) = (fp.canonicalize(), tmp_dir.canonicalize()) {
        return c_fp.starts_with(&c_tmp);
    }
    false
}

/// Read A2A JSON from a temp file and extract the deliver payload from `content`.
fn parse_a2a_file(path: &str) -> Option<DeliverPayload> {
    let fp = std::path::Path::new(path);
    if !is_safe_temp_path(fp) {
        return None;
    }
    let raw = std::fs::read_to_string(fp).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let content = json.get("content").and_then(|v| v.as_str())?;
    parse_deliver_content(content)
}

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
                "[provider_applied/over_budget] reject-apply failed in-process: {e}\n\n\
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
        "Push the next-step decision card via `pending-decisions-v2 request`, then end turn.\n\n\
         {request_block}\n"
        );
    }

    // ── Within-budget branch: confirm-accept on-chain (escrow funded; status → accepted) ──
    if let Err(e) = super::super::accept::handle_confirm_accept(&mut client, job_id, ctx.prefetched).await {
        return format!(
            "[provider_applied/confirm_accept] confirm-accept failed in-process: {e}\n\n\
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
             onchainos agent user-notify --content '<localized content>'\n\
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
     If not found (e.g. context compaction), **default to replaySuccess=true** —\n\
     skipping complete would leave the task stuck in accepted forever.\n\n\
     **Branch 1: replaySuccess=true (or default)**\n\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     broadcast ≠ on-chain confirmed. Do NOT notify user or say \"task complete\" here.\n\
     On error → notify user:\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Content: {complete_failed}\n\
     → End turn, wait for retry or wakeup_notify.\n\n\
     **Branch 2: replaySuccess=false (explicitly found in context)**\n\n\
     Do not run complete.\n\
     Check whether a `x402_replay_input` pending decision was already pushed in the previous turn:\n\
     ▸ Yes → end turn (user will reply to the pending decision).\n\
     ▸ No → notify user:\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
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
     [Role] User\n\n\
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
     onchainos agent user-notify --content '<localized content>'\n\
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
/// The sub-session LLM saves the raw A2A JSON to a temp file and passes
/// `a2aFile` in `--message`. This handler reads the file, parses the
/// `content` field to determine file vs text, does the download/save
/// entirely in Rust, then returns a minimal notify-only prompt.
///
/// Legacy `--message` fields (deliverableType/fileKey/text/filePath) are
/// still accepted as fallback for backward compatibility.
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

    let msg_str = |key: &str| {
        message
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };

    // ── Resolve DeliverPayload: a2aFile → legacy fields → fallback ──
    let a2a_file = msg_str("a2aFile");
    let payload = if !a2a_file.is_empty() {
        match parse_a2a_file(a2a_file) {
            Some(p) => {
                audit::log("cli", "buyer/deliverable_from_a2a_file", true, Duration::default(),
                    Some([base_tags.clone(), vec![format!("path={a2a_file}")]].concat()), None);
                p
            }
            None => {
                audit::log("cli", "buyer/deliverable_a2a_file_parse_failed", false, Duration::default(),
                    Some([base_tags.clone(), vec![format!("path={a2a_file}")]].concat()),
                    Some("failed to parse A2A file or extract deliver content"));
                return deliverable_received(ctx);
            }
        }
    } else {
        // Legacy: LLM passed fields directly in --message JSON
        let dtype = msg_str("deliverableType");
        if dtype.is_empty() {
            audit::log("cli", "buyer/deliverable_received_no_type", false, Duration::default(),
                Some(base_tags.clone()), Some("no a2aFile and no deliverableType, fallback to LLM path"));
            return deliverable_received(ctx);
        }
        match dtype {
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
                DeliverPayload::File {
                    file_key: file_key.to_string(),
                    digest: digest.to_string(),
                    salt: salt.to_string(),
                    nonce: nonce.to_string(),
                    secret: secret.to_string(),
                    filename: filename.map(|s| s.to_string()),
                }
            }
            "text" => {
                let inline_text = msg_str("text");
                let file_path = msg_str("filePath");
                if !inline_text.is_empty() {
                    DeliverPayload::Text(inline_text.to_string())
                } else if !file_path.is_empty() {
                    let fp = std::path::Path::new(file_path);
                    if !is_safe_temp_path(fp) {
                        audit::log("cli", "buyer/deliverable_text_path_rejected", false, Duration::default(),
                            Some(base_tags.clone()), Some("filePath not under temp dir"));
                        return deliverable_received(ctx);
                    }
                    match std::fs::read_to_string(fp) {
                        Ok(raw) => {
                            match parse_deliver_content(&raw) {
                                Some(DeliverPayload::Text(t)) => DeliverPayload::Text(t),
                                _ => {
                                    // File contains raw text without protocol framing
                                    DeliverPayload::Text(raw.trim().to_string())
                                }
                            }
                        }
                        Err(e) => {
                            audit::log("cli", "buyer/deliverable_text_read_failed", false, Duration::default(),
                                Some(base_tags.clone()), Some(&e.to_string()));
                            return deliverable_received(ctx);
                        }
                    }
                } else {
                    audit::log("cli", "buyer/deliverable_text_no_content", false, Duration::default(),
                        Some(base_tags.clone()), Some("neither a2aFile, text, nor filePath provided"));
                    return deliverable_received(ctx);
                }
            }
            _ => {
                audit::log("cli", "buyer/deliverable_received_unknown_type", false, Duration::default(),
                    Some([base_tags.clone(), vec![format!("type={dtype}")]].concat()), None);
                return deliverable_received(ctx);
            }
        }
    };

    let dtype_str = match &payload { DeliverPayload::File { .. } => "file", DeliverPayload::Text(_) => "text" };
    audit::log("cli", "buyer/deliverable_received", true, Duration::default(),
        Some([base_tags.clone(), vec![format!("type={dtype_str}")]].concat()), None);

    let (title, sym, amt, provider_id) = match ctx.prefetched {
        Some(p) => (
            p.title.as_str(),
            p.token_symbol.as_str(),
            p.token_amount.as_str(),
            p.provider_agent_id.as_deref().unwrap_or(""),
        ),
        None => ("<title>", "<tokenSymbol>", "<tokenAmount>", ""),
    };

    // ── Execute: download (file) or write tmp (text) → handle_save ──
    let (saved_path, deliverable_type, text_content) = match payload {
        DeliverPayload::File { ref file_key, ref digest, ref salt, ref nonce, ref secret, ref filename } => {
            audit::log("cli", "buyer/deliverable_file_download", true, Duration::default(),
                Some([base_tags.clone(), vec![format!("fileKey={file_key}")]].concat()), None);

            let local_path = match okx_a2a::file_download(
                file_key, agent_id, digest, salt, nonce, secret, filename.as_deref(),
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
                    (r.path, "file".to_string(), None)
                }
                Err(e) => {
                    audit::log("cli", "buyer/deliverable_save_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=file".into()]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
        DeliverPayload::Text(text) => {
            audit::log("cli", "buyer/deliverable_text_parsed", true, Duration::default(),
                Some([base_tags.clone(), vec![format!("charCount={}", text.chars().count())]].concat()), None);

            let tmp_dir = std::env::temp_dir();
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
                    (r.path, "text".to_string(), Some(text))
                }
                Err(e) => {
                    audit::log("cli", "buyer/deliverable_save_failed", false, Duration::default(),
                        Some([base_tags.clone(), vec!["type=text".into()]].concat()), Some(&e.to_string()));
                    eprintln!("[deliverable_received_cli] save failed: {e}");
                    return deliverable_received(ctx);
                }
            }
        }
    };

    // Pre-decide the ASP rating + pre-translate the rating_submitted notify
    // + pre-translate the JobCompleted notify on the backup session (escrow
    // only). The future `job_completed` event then dispatches
    // `feedback-submit` + `user-notify` in-process with zero LLM decisions.
    //
    // All three artifacts are bundled into one backup turn because they share
    // the same trigger (rating decided from this deliverable) and the same
    // downstream consumer (the job_completed fast path).
    if ctx.payment_mode != Some(3) {
        // Description is the basis for the sub LLM's rating decision — if it's
        // missing (no prefetched / empty), skip the prefetch entirely and let
        // the LLM playbook handle job_completed with full context at event time.
        let task_description = ctx.prefetched
            .map(|p| p.description.as_str())
            .filter(|s| !s.is_empty());
        if let Some(task_description) = task_description {
            let rating_title = ctx.prefetched
                .map(|p| p.title.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(ctx.title_display);
            let deliverable_summary = match (deliverable_type.as_str(), text_content.as_deref()) {
                ("text", Some(t)) => format!("type: text\ncontent:\n{t}"),
                ("file", _) => format!("type: file\nsaved path: {saved_path}"),
                _ => format!("type: {deliverable_type}\nsaved path: {saved_path}"),
            };
            // JobCompleted notify — jobId + title prefilled; `<tokenAmount>` /
            // `<tokenSymbol>` kept as placeholders, filled by the `job_completed`
            // fast path with the on-chain locked values from `ctx.prefetched`.
            let canonical_job_completed = super::super::content::job_completed_escrow_user_notify(
                job_id, rating_title, "<tokenAmount>", "<tokenSymbol>",
            );
            let prefetch_batch = format!(
                "[PREFETCH — internal cache only, NOT a user-facing flow]\n\
             Pre-decide the ASP rating, then pre-translate two notifications for job `{job_id}`. \
             Execute all steps in one turn.\n\
             ⚠️ The triple-backtick fence markers are NOT part of the content — do not include them.\n\
             ⚠️ Keep EVERY angle-bracket placeholder (e.g. `<tokenAmount>`, `<tokenSymbol>`) verbatim in your translation — CLI will fill them at dispatch time.\n\
             🛑 **Output discipline (strict):** the THREE `cache-*` commands below are the ONLY commands you may run in this turn.\n\
             Task description:\n\
             ```\n\
             {task_description}\n\
             ```\n\n\
             Deliverable:\n\
             ```\n\
             {deliverable_summary}\n\
             ```\n\n\
             [Step 1] Decide score (`X.XX`, 0.00–5.00) + comment (≤100 chars). Then run:\n\
             \x20\x20onchainos agent cache-rating --job-id {job_id} --score <X.XX> --comment '<your comment>'\n\n\
             [Step 2] Fill `<score>` and `<description>` in the template below with the values you just decided, translate the filled result into the user's chat language, then run:\n\
             \x20\x20onchainos agent cache-notify --job-id {job_id} --event-key rating_submitted --content '<your translation>'\n\
             Template:\n\
             ```\n\
             [📝 Rating Submitted] {rating_title} (`{job_id}`) — rated.\n\
             Score: <score> / 5.00\n\
             💬 Comment: <description>\n\
             ```\n\n\
             [Step 3] Translate the JobCompleted template below into the user's chat language (placeholders preserved verbatim), then run:\n\
             \x20\x20onchainos agent cache-notify --job-id {job_id} --event-key job_completed_escrow --content '<your translation>'\n\
             Template:\n\
             ```\n\
             {canonical_job_completed}\n\
             ```"
            );
            let _ = okx_a2a::session_send(
                job_id, None, &prefetch_batch,
            );
        }
    }

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
                user_agent_id: None,
                visibility: None,
                status: None,
                deliverable: None,
                service_id: None,
                service_token_address: None,
                service_token_amount: None,
                service_params: None,
                user_agent_address: None,
                token_address: None,
            }
        });
        patched.deliverable = Some(crate::commands::agent_commerce::task::common::PreFetchedDeliverable {
            path: saved_path.clone(),
            deliverable_type: deliverable_type.clone(),
            original_name: String::new(),
            text_content: text_content.clone(),
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
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\
         Template (translate to user's language, keep structure; path must be full absolute — never abbreviate):\n\
         \x20\x20[Deliverable Received] {title} (`{short_id}`)\n\
         \x20\x20Provider: {provider_id}\n\
         \x20\x20Type: {deliverable_type}\n\
         \x20\x20Saved at: [{saved_path}]({saved_path})\n\
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
            "paymentMode could not be pre-fetched. Run `onchainos agent status {job}` first to determine paymentMode (1=escrow, 3=x402), then follow the matching branch below.\n\n\
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
            "[job_submitted_escrow] no prefetched task context for job {job_id}; cannot run the review flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_escrow] prefetched task context has no providerAgentId for job {job_id}; cannot run the review flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    // Out-of-order handling: prefetch doesn't include local deliverable info.
    // Check the local manifest to decide: saved → populate & proceed, else → marker + wait.
    if p.deliverable.is_none() {
        use crate::commands::agent_commerce::task::common::deliverables;
        if let Ok(Some(manifest)) = deliverables::read_manifest("buyer", job_id) {
            if let Some(entry) = manifest.entries.last() {
                let saved_path = deliverables::deliverables_dir("buyer", job_id)
                    .map(|d| d.join(&entry.filename))
                    .unwrap_or_default();
                let text_content = if entry.deliverable_type == "text" {
                    std::fs::read_to_string(&saved_path).ok()
                } else {
                    None
                };
                let mut patched = p.clone();
                patched.deliverable = Some(crate::commands::agent_commerce::task::common::PreFetchedDeliverable {
                    path: saved_path.display().to_string(),
                    deliverable_type: entry.deliverable_type.clone(),
                    original_name: entry.original_name.clone(),
                    text_content,
                });
                let patched_ctx = super::super::flow::FlowContext {
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
                return job_submitted_escrow(&patched_ctx);
            }
        }
        if !deliverables::has_review_marker(job_id) {
            let _ = deliverables::write_review_marker(job_id);
        }
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
        if d.deliverable_type == "text" {
            let content = d.text_content.as_deref().unwrap_or("<content unavailable>");
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: text\n\
     \x20\x20- deliverableText:\n\
     ```\n\
     {content}\n\
     ```\n\n",
                path = d.path,
            )
        } else {
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: file\n\n",
                path = d.path,
            )
        }
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
    "MUST use `pending-decisions-v2 request` — NOT `onchainos agent user-notify` (one-way = no relay = deadlock). Auto-approval forbidden.\n\n\
     [Your next actions (strict order)]\n\n\
     {step2}\
     **Step 3 — Compose `--user-content` and push decision card:**\n\n\
     Compose `--user-content` from Step 2's deliverable variables (fill placeholders from runtime values):\n\n\
     `<localPath>` must be the full absolute path (e.g. /Users/xxx/…). Never abbreviate or shorten.\n\n\
     ▸ deliverableType=file:\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (file).\n\
     File path: [<localPath>](<localPath>)\n\
     Payment: escrow\n\
     A. Approve → reply 'A'\n\
     B. Reject (state reason; used as evidence if disputed) → reply 'B reason: …'\n\
     ```\n\n\
     ▸ deliverableType=text:\n\
     ```\n\
     [Job {short_id}] The ASP has submitted the deliverable (text).\n\
     Saved at: [<localPath>](<localPath>)\n\
     ---Deliverable---\n\
     <deliverableText from Step 2 — full content, no truncation>\n\
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
            "[job_submitted_x402] no prefetched task context for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };
    let provider_field: &str = match p.provider_agent_id.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => return format!(
            "[job_submitted_x402] prefetched task context has no providerAgentId for job {job_id}; cannot run the x402 notify+rate flow.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
        ),
    };

    let step2 = if let Some(d) = p.deliverable.as_ref() {
        if d.deliverable_type == "text" {
            let content = d.text_content.as_deref().unwrap_or("<content unavailable>");
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: text\n\
     \x20\x20- deliverableText:\n\
     ```\n\
     {content}\n\
     ```\n\n",
                path = d.path,
            )
        } else {
            format!("\
     **Step 2 — Deliverable already saved**:\n\
     \x20\x20- localPath: {path}\n\
     \x20\x20- deliverableType: file\n\n",
                path = d.path,
            )
        }
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
    "x402: funds already paid; user cannot reject — notify + auto-rate only.\n\n\
     [Your next actions (strict order)]\n\n\
     {step2}\
     **Step 3 — Auto-rate ASP, then notify user:**\n\n\
     **3a — Rate the ASP (mandatory, before notify):**\n\
     Score 0.00–5.00 based on deliverable vs description. Comment ≤100 chars.\n\
     ```bash\n\
     onchainos agent feedback-submit --agent-id {provider_field} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment>\"\n\
     ```\n\
     `--agent-id` = ASP being rated; `--creator-id` = user's agent id.\n\n\
     **3b — Notify user (deliverable + rating in one message):**\n\
     ```bash\n\
     onchainos agent user-notify --content '<localized content>'\n\
     ```\n\
     Compose from two halves (concatenate with two blank lines):\n\
     \x20\x20▸ Deliverable (always; pick template):\n\
     \x20\x20\x20\x20file: `[Deliverable Received] Job {job_id} — x402, payment settled. File: [<localPath>](<localPath>)`\n\
     \x20\x20\x20\x20text (localPath available): `[Deliverable Received] Job {job_id} — x402, payment settled. Saved at: [<localPath>](<localPath>)` + deliverableText from Step 2\n\
     \x20\x20\x20\x20text (no localPath): `[Deliverable Received] Job {job_id} — x402, payment settled.` + deliverableText from Step 2 inline\n\
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
/// user via onchainos agent user-notify / don't auto-rate / don't say funds released
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
            "[approve_review] `onchainos agent complete {job_id}` failed in-process: {e}\n\n\
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
            "[reject_review] [OK]`onchainos agent reject {job_id} --reason \"{reason}\"` broadcast in-process. End the turn now.\n\n\
             broadcast ≠ on-chain confirmed. The `job_rejected` system event will fire after on-chain confirmation; the ASP then decides whether to dispute (arbitration) or agree to a refund. The user cannot initiate arbitration.\n\
             Do NOT send any message to the ASP about the rejection — they learn via on-chain events.\n"
        ),
        Err(e) => format!(
            "[reject_review] `onchainos agent reject {job_id} --reason \"{reason}\"` failed in-process: {e}\n\n\
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
pub(crate) fn job_completed(ctx: &FlowContext<'_>, _message: Option<&serde_json::Value>) -> String {
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

    let pm = ctx.payment_mode;

    // Fast path (escrow only): rating + both notify templates pre-cached at
    // deliverable_received time. Run feedback-submit and user-notify entirely
    // in-process; zero LLM decisions.
    //
    // The `job_completed_escrow` template is cached at deliverable_received
    // with `<tokenAmount>` / `<tokenSymbol>` placeholders — filled here with
    // the on-chain locked values from `ctx.prefetched`.
    let provider_id_opt = ctx.prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty());
    if pm != Some(3) {
        if let Some(real_provider_id) = provider_id_opt {
            use crate::commands::agent_commerce::task::common::{
                okx_a2a, onchainos_self, prefilled_notify, prefilled_rating, session_cleanup,
            };
            let cached_completed = prefilled_notify::get(job_id, "job_completed_escrow").ok().flatten();
            let cached_rating_notify = prefilled_notify::get(job_id, "rating_submitted").ok().flatten();
            let cached_rating = prefilled_rating::get(job_id).ok().flatten();
            let amount_ok = !token_amount.is_empty() && !token_amount.starts_with('<');
            let symbol_ok = !token_symbol.is_empty() && !token_symbol.starts_with('<');
            if let (Some(completed_tpl), Some(rating_text), Some(rating)) =
                (cached_completed, cached_rating_notify, cached_rating)
            {
                let placeholders_present = completed_tpl.contains("<tokenAmount>")
                    && completed_tpl.contains("<tokenSymbol>");
                if amount_ok && symbol_ok && placeholders_present {
                    let completed = completed_tpl
                        .replace("<tokenAmount>", token_amount)
                        .replace("<tokenSymbol>", token_symbol);
                    let feedback_ok = onchainos_self::feedback_submit(
                        real_provider_id, agent_id, &rating.score, job_id, &rating.comment,
                    ).is_ok();
                    let combined = if feedback_ok {
                        format!("{completed}\n\n{rating_text}")
                    } else {
                        completed
                    };
                    let _ = okx_a2a::user_notify(&combined, false);
                    let _ = session_cleanup::handle_session_cleanup(job_id, false);

                    return "Task is at a terminal state. User has been notified by the CLI. Do NOT run any further command.".to_string();
                }
                // Placeholder missing or amount/symbol unknown → fall through to LLM playbook.
            }
        }
    }

    let completed_notify = if pm == Some(3) {
        super::super::content::job_completed_x402_user_notify(job_id, title_display)
    } else {
        super::super::content::job_completed_escrow_user_notify(job_id, title_display, token_amount, token_symbol)
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
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\
         Template:\n\
         \x20\x20{completed_notify}\n\n\
         \x20\x20{rating_notify}  ← omit if Step 1 failed\n\n\
         **Step 3 — Wrap-up:**\n\
         {terminal_session_hint}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_deliver_content ────────────────────────────────────────

    #[test]
    fn parse_file_deliver() {
        let content = "\
jobId: 0x5ea81a18be490d59f88cb2258b4d902d76a1b9848f9e4b452c1266ee40d34721
deliverableType: file
fileKey: 0x5ea81a18be490d59f88cb2258b4d902d76a1b9848f9e4b452c1266ee40d34721/0x5ea81a18be490d59f88cb2258b4d902d76a1b9848f9e4b452c1266ee40d34721-54333239-3175-43b5-b455-015eb8aa0ad5
digest: 93f2c0186b237f10629873167217dfa173c3cbf5eebf4da71715871b16b31e0e
salt: 4CyqL4avwltYQoBg8rZ/luUpISvDwVq9H2AGs2i5JOQ=
nonce: 3qEw/DyUDt32EeA1
secret: 6Y350QXsL+lsk3AyPVMl3UguwaLj+Dc7yAYU8FUpb6k=
filename: argentina-wc-prediction.md
[intent:deliver]";

        let payload = parse_deliver_content(content).expect("should parse file deliver");
        match payload {
            DeliverPayload::File { file_key, digest, salt, nonce, secret, filename } => {
                assert!(file_key.starts_with("0x5ea81a18"), "fileKey: {file_key}");
                assert!(file_key.ends_with("015eb8aa0ad5"), "fileKey: {file_key}");
                assert_eq!(digest, "93f2c0186b237f10629873167217dfa173c3cbf5eebf4da71715871b16b31e0e");
                assert_eq!(salt, "4CyqL4avwltYQoBg8rZ/luUpISvDwVq9H2AGs2i5JOQ=");
                assert_eq!(nonce, "3qEw/DyUDt32EeA1");
                assert_eq!(secret, "6Y350QXsL+lsk3AyPVMl3UguwaLj+Dc7yAYU8FUpb6k=");
                assert_eq!(filename.as_deref(), Some("argentina-wc-prediction.md"));
            }
            DeliverPayload::Text(_) => panic!("expected File, got Text"),
        }
    }

    #[test]
    fn parse_text_deliver() {
        let content = "\
jobId: 0x8bad8245e68c40b0199dd49918e88b79dc21c6cfc68f69f2819570552412e185
deliverableType: text
- - -
onchain-arb 套利扫描报告
===========================
扫描时间: 2026-06-24 22:47 GMT+8
📊 各代币价差全景
LINK 🎯 | ETH | BTC
- - -
[intent:deliver]";

        let payload = parse_deliver_content(content).expect("should parse text deliver");
        match payload {
            DeliverPayload::Text(text) => {
                assert!(text.starts_with("onchain-arb"), "text starts with: {}", &text[..30]);
                assert!(text.contains("LINK 🎯"), "should preserve emoji");
                assert!(text.contains("📊"), "should preserve Unicode");
                assert!(!text.contains("[intent:deliver]"), "should not include suffix");
                assert!(!text.contains("- - -"), "should not include separators");
                assert!(!text.contains("deliverableType"), "should not include header");
            }
            DeliverPayload::File { .. } => panic!("expected Text, got File"),
        }
    }

    #[test]
    fn parse_a2a_json_file_type() {
        let a2a_json = r#"{
  "msgType": "a2a-agent-chat",
  "content": "jobId: 0x5ea8\ndeliverableType: file\nfileKey: abc123\ndigest: d1g\nsalt: s4lt\nnonce: n0nc\nsecret: s3cr\nfilename: report.md\n[intent:deliver]",
  "sender": {"agentId": "1891"}
}"#;
        let json: serde_json::Value = serde_json::from_str(a2a_json).unwrap();
        let content = json.get("content").unwrap().as_str().unwrap();
        let payload = parse_deliver_content(content).expect("should parse from A2A JSON");
        match payload {
            DeliverPayload::File { file_key, digest, salt, nonce, secret, filename } => {
                assert_eq!(file_key, "abc123");
                assert_eq!(digest, "d1g");
                assert_eq!(salt, "s4lt");
                assert_eq!(nonce, "n0nc");
                assert_eq!(secret, "s3cr");
                assert_eq!(filename.as_deref(), Some("report.md"));
            }
            DeliverPayload::Text(_) => panic!("expected File"),
        }
    }

    #[test]
    fn parse_a2a_json_text_type() {
        let a2a_json = r#"{
  "content": "jobId: 0x8bad\ndeliverableType: text\n- - -\nHello World 🌍\nLine 2\n- - -\n[intent:deliver]"
}"#;
        let json: serde_json::Value = serde_json::from_str(a2a_json).unwrap();
        let content = json.get("content").unwrap().as_str().unwrap();
        let payload = parse_deliver_content(content).expect("should parse text from A2A JSON");
        match payload {
            DeliverPayload::Text(text) => {
                assert_eq!(text, "Hello World 🌍\nLine 2");
            }
            DeliverPayload::File { .. } => panic!("expected Text"),
        }
    }

    #[test]
    fn parse_no_intent_deliver_returns_none() {
        let content = "jobId: 0xabc\ndeliverableType: text\n- - -\nsome text\n- - -\n";
        assert!(parse_deliver_content(content).is_none());
    }

    #[test]
    fn parse_missing_fields_returns_none() {
        let content = "jobId: 0xabc\ndeliverableType: file\nfileKey: k\n[intent:deliver]";
        assert!(parse_deliver_content(content).is_none(), "missing digest/salt/nonce/secret");
    }

    #[test]
    fn parse_text_with_internal_separator() {
        let content = "\
deliverableType: text
- - -
Part A
- - -
Part B continues
- - -
[intent:deliver]";
        let payload = parse_deliver_content(content).expect("should handle internal separator");
        match payload {
            DeliverPayload::Text(text) => {
                assert!(text.contains("Part A"), "should include Part A");
                assert!(text.contains("- - -"), "internal separator preserved");
                assert!(text.contains("Part B"), "should include Part B");
            }
            _ => panic!("expected Text"),
        }
    }
}
