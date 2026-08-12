//! Subscription lifecycle event handlers (user side).

use super::super::flow::{notify_and_end, notify_and_end_terminal, FlowContext};
use crate::commands::agent_commerce::task::common::okx_a2a;

fn extract_str<'a>(message: Option<&'a serde_json::Value>, key: &str) -> Option<&'a str> {
    message
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

fn extract_i64(message: Option<&serde_json::Value>, key: &str) -> Option<i64> {
    message.and_then(|m| m.get(key)).and_then(|v| v.as_i64())
}

fn service_name<'a>(message: Option<&'a serde_json::Value>, ctx: &'a FlowContext<'_>) -> &'a str {
    extract_str(message, "jobTitle")
        .or_else(|| extract_str(message, "title"))
        .or_else(|| ctx.prefetched.map(|p| p.description.as_str()))
        .unwrap_or("subscription")
}

pub(crate) fn sub_created(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    // Subscribe-success has two copy variants keyed on trialType: 1 → trial start
    // (charge-free; the real first charge is announced by sub_trial_into_active),
    // anything else / absent → paid subscription with immediate first charge.
    // Defaulting the absent case to the paid variant matches the copy doc, which
    // defines that entry as the no-trial direct-subscribe notice.
    let content = if extract_i64(message, "trialType") == Some(1) {
        super::super::content::sub_created_trial_user_notify(
            extract_str(message, "tokenAmount"),
            extract_str(message, "tokenSymbol"),
            // Wire has not finished the trail*→trial* field rename; keep the
            // legacy spelling as a read fallback until it does.
            extract_i64(message, "trialStartTime")
                .or_else(|| extract_i64(message, "trailStartTime")),
            extract_i64(message, "trialEndTime").or_else(|| extract_i64(message, "trailEndTime")),
        )
    } else {
        let auto_renew = message
            .and_then(|m| m.get("autoRenew"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            == 1;
        super::super::content::sub_created_user_notify(
            ctx.job_id,
            service_name(message, ctx),
            extract_str(message, "tokenAmount"),
            extract_str(message, "tokenSymbol"),
            extract_i64(message, "subStartTime"),
            extract_i64(message, "subEndTime"),
            auto_renew,
        )
    };

    let provider_id = extract_str(message, "providerAgentId")
        .or_else(|| ctx.prefetched.and_then(|p| p.provider_agent_id.as_deref()));

    let session_block = match provider_id {
        Some(pid) => create_sub_session(ctx.job_id, ctx.agent_id, pid),
        None => String::new(),
    };

    let mut out = format!(
        "**Localize first** — rewrite the content below in the user's language before sending. \
         Do NOT pass the English template verbatim to a non-English user.\n\
         ```bash\n\
         onchainos agent user-notify --content \"<localized content shown below>\"\n\
         ```\n\
         Content: {content}\n\n"
    );

    if !session_block.is_empty() {
        out.push_str(&session_block);
        out.push('\n');
    }

    // FR-7 / AC-8: no post-`sub_created` DApp text re-scan or install. Tool readiness
    // was already surfaced at `asp-match` time via `autoTradePreflight`; the runtime
    // re-checks tool status when the first real signal arrives.
    out.push_str("**End this turn** after the notification is sent.\n");

    out
}

/// Create sub session + SKILL_PREFETCH + upload attachments, mirroring
/// `branch_a2a_cli` in the regular task flow.
fn create_sub_session(job_id: &str, agent_id: &str, provider_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;

    match okx_a2a::session_query_exists(job_id, agent_id, provider_id) {
        Ok(true) => return String::new(),
        Ok(false) => {}
        Err(e) => {
            return format!("[sub_created] session query failed: {e}\n");
        }
    }

    if let Err(e) = okx_a2a::session_create(job_id, agent_id, provider_id) {
        return format!("[sub_created] session create failed: {e}\n");
    }

    let prefetch = "[SKILL_PREFETCH] Read the okx-ai skill. Pre-load user role context. \
        This prefetch message itself requires no action — but when the NEXT inbound message arrives \
        (same turn or later turn), you MUST process it normally via task-user-sub-playbook.md \
        §Peer Message Routing (#1–#6). Do NOT carry over \"no action\" to business messages.";
    if let Err(e) = okx_a2a::session_send(job_id, Some(provider_id), prefetch) {
        return format!("[sub_created] session send (SKILL_PREFETCH) failed: {e}\n");
    }

    super::upload_and_forward_all_attachments(job_id, agent_id, provider_id);

    String::new()
}

pub(crate) fn sub_cancel(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    let cancel_result = extract_str(message, "cancelResult");
    let fail_reason =
        extract_str(message, "failReason").or_else(|| extract_str(message, "failReasopn"));
    let trial_type = extract_i64(message, "trialType");
    let svc = service_name(message, ctx);
    let trial_ends_at =
        extract_i64(message, "trialEndTime").or_else(|| extract_i64(message, "trailEndTime"));
    let sub_end = extract_i64(message, "subEndTime");
    let content = super::super::content::sub_cancel_user_notify(
        cancel_result,
        fail_reason,
        trial_type,
        svc,
        ctx.job_id,
        trial_ends_at,
        sub_end,
    );
    // Terminal only when the trial's auto-conversion was actually cancelled. A FAILED cancel
    // leaves the subscription alive (the trial will still convert), so the session stays open.
    let cancelled = cancel_result.is_none_or(|r| !r.eq_ignore_ascii_case("fail"));
    if cancelled && trial_type == Some(1) {
        notify_and_end_terminal(&content, &ctx.terminal_session_hint)
    } else {
        notify_and_end(&content)
    }
}

pub(crate) fn sub_user_reject(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_user_reject_user_notify(
        svc,
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
        extract_i64(message, "rejectWindowEndsAt"),
        extract_str(message, "tokenAmount"),
        extract_str(message, "tokenSymbol"),
    );
    notify_and_end(&content)
}

pub(crate) fn sub_asp_agree(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_asp_agree_user_notify(
        svc,
        extract_str(message, "tokenAmount"),
        extract_str(message, "tokenSymbol"),
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
    );
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn sub_asp_dispute(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let svc = service_name(message, ctx);
    let title_query_hint = ctx.title_query_hint;

    let provider_id = match ctx
        .prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s,
        None => {
            return format!(
                "[sub_asp_dispute] prefetched.provider_agent_id missing for job {job_id}; \
             cannot fetch chat history for dispute evidence.\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
            )
        }
    };
    let chat_block = match okx_a2a::session_history(job_id, provider_id) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "[]" {
                "(no chat history available)".to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(e) => {
            return format!(
                "[sub_asp_dispute] `okx-a2a session history` failed: {e}\n\n\
             See _shared/exception-escalation.md §2 — push `cli_failed` decision.\n"
            )
        }
    };

    let notify_content = super::super::content::sub_asp_dispute_user_notify(
        svc,
        job_id,
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
    );

    format!(
    "[Current Status] sub_asp_dispute (subscription evaluation opened; CLI auto-submits evidence on this event)\n\
     [Role] User Agent\n\n\
     **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
     The agent does NOT ask the user for evidence; it formats the chat history, calls `dispute upload`\n\
     (which also auto-attaches the most recent 20 saved deliverables from `~/.onchainos/deliverables/user/{job_id}/`),\n\
     and then notifies the user via `onchainos agent user-notify`. **Do NOT** use `pending-decisions-v2 request`\n\
     for this event. **Do NOT** send any message to the ASP — both sides see the evaluation via on-chain events.\n\n\
     [Your next actions (strict order)]\n\n\
     {title_query_hint}\
     **Step 1 — Chat history (pre-fetched and inlined below; do NOT call `okx-a2a session history` again):**\n\n\
     ```\n\
     ==== Negotiation / delivery chat history ====\n\
     {chat_block}\n\
     ```\n\n\
     **Step 2 — Extract a `--text` body from the chat history above** (≤16 KB):\n\
     Keep ONLY the key checkpoints — subscription scope discussion / deliverable messages + both sides' key dispute points. Prepend `(key checkpoints extracted)` so the evaluator knows it was trimmed. If history is genuinely empty, pass a minimal placeholder like `(no chat history available)`.\n\n\
     **Step 3 — Upload (off-chain multipart):**\n\
     ```bash\n\
     onchainos agent dispute upload {job_id} --role user --agent-id {agent_id} --max-files 20 --text \"<chat history block from Step 2>\"\n\
     ```\n\
     The CLI auto-attaches the most recent 20 entries under `~/.onchainos/deliverables/user/{job_id}/manifest.json` as multipart `files[]` parts — **do NOT pass `--file`**; the manifest covers all locally-saved deliverables. If the upload fails, retry up to 3 times; if it keeps failing, still proceed to Step 4 — the on-chain dispute will continue without off-chain evidence and the evaluator rules on what is available.\n\n\
     **Step 4 — Notify the user via `onchainos agent user-notify` (after upload returns):**\n\
     **Localize first** — translate the content below into the user's language before sending.\n\
     ```bash\n\
     onchainos agent user-notify --content \"<localized content>\"\n\
     ```\n\
     Content:\n\
     \x20\x20\x20\x20{notify_content}\n\n\
     **Step 5 — End this turn.** Do NOT send any message to the ASP.\n\n\
"
    )
}

pub(crate) fn sub_trial_into_active(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_trial_into_active_user_notify(
        ctx.job_id,
        svc,
        extract_str(message, "tokenAmount"),
        extract_str(message, "tokenSymbol"),
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
    );
    notify_and_end(&content)
}

pub(crate) async fn sub_renew(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    let renew_result = extract_str(message, "renewResult");
    let fail_reason =
        extract_str(message, "failReason").or_else(|| extract_str(message, "failReasopn"));
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_renew_user_notify(
        renew_result,
        fail_reason,
        svc,
        ctx.job_id,
        extract_str(message, "tokenAmount"),
        extract_str(message, "tokenSymbol"),
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
        extract_i64(message, "subBufferEndTime"),
    );
    // Never terminal — a failed renewal only enters the grace period (service continues,
    // backend keeps retrying); the subscription ends later via sub_close_notify /
    // sub_failed_notify, and those events own the session-cleanup hint.
    if renew_result == Some("fail") {
        if let (Some(symbol), Some(amount_str)) = (extract_str(message, "tokenSymbol"), extract_str(message, "tokenAmount")) {
            if let Ok(required) = amount_str.parse::<f64>() {
                if required > 0.0 {
                    if let Ok((_account_id, address)) = crate::commands::agent_commerce::task::signing::resolve_wallet_by_agent_id(ctx.agent_id).await {
                        if !address.is_empty() {
                            let balance_low = crate::commands::agent_commerce::task::common::query_xlayer_balance(&address, symbol)
                                .await
                                .map_or(true, |b| b < required);
                            if balance_low {
                                return super::super::flow::notify_and_end_with_deposit(&content, &address);
                            }
                        }
                    }
                }
            }
        }
    }
    notify_and_end(&content)
}

/// FR-9: select the `sub_expire_warn` copy by the subscription's `autoRenew`.
/// `Some(0)` (explicitly off) -> the new "ending soon" template with the current
/// period range; `Some(nonzero)` (on) or `None` (missing/legacy → treat as on)
/// -> the existing template, byte-for-byte unchanged. Pure/testable.
pub(crate) fn select_sub_expire_warn_content(
    auto_renew: Option<i64>,
    job_id: &str,
    period_start: &str,
    period_end: &str,
) -> String {
    match auto_renew {
        Some(0) => super::super::content::sub_expire_warn_no_autorenew_notify(
            job_id,
            period_start,
            period_end,
        ),
        _ => super::super::content::sub_expire_warn_user_notify(job_id),
    }
}

/// FR-9: keep the follow-up hint in the same state model as the selected
/// content. Only an explicit `autoRenew=0` tells the user how to enable
/// auto-renew; missing/legacy data follows the existing no-action path.
pub(crate) fn sub_expire_warn_renewal_hint(auto_renew: Option<i64>, job_id: &str) -> String {
    match auto_renew {
        Some(0) => format!(
            "Auto-renewal is **off**. To enable it before expiry:\n\
             ```bash\n\
             onchainos agent start-autorenew {job_id}\n\
             ```"
        ),
        _ => "No action needed unless you want to cancel.".to_string(),
    }
}

/// Extract an epoch-seconds field tolerating both number and string JSON forms.
fn as_epoch_secs(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| {
        x.as_i64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

pub(crate) async fn sub_expire_warn(ctx: &FlowContext<'_>) -> String {
    use super::super::create_subscribe::SUBSCRIBE_API_PREFIX;
    use super::super::subscription_ops::select_subscription_agent_id;
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let mut client = TaskApiClient::new();
    let detail = match select_subscription_agent_id(agent_id, "") {
        Ok(agent_id) => {
            client
                .get_with_identity(&format!("{SUBSCRIBE_API_PREFIX}/{job_id}"), &agent_id)
                .await
        }
        Err(error) => Err(error),
    };

    // FR-9: distinguish an explicit `autoRenew=0` (false → new template) from
    // missing/legacy (None → treated as true → existing template).
    let auto_renew_opt: Option<i64> = detail.as_ref().ok().and_then(|r| r["autoRenew"].as_i64());

    // Current-period range for the autoRenew=false template (unused otherwise).
    let period_start = detail
        .as_ref()
        .ok()
        .and_then(|r| as_epoch_secs(r, "subStartTime"));
    let period_end = detail
        .as_ref()
        .ok()
        .and_then(|r| as_epoch_secs(r, "subEndTime"));
    let period_start_str = super::super::content::fmt_epoch(period_start).unwrap_or_default();
    let period_end_str = super::super::content::fmt_epoch(period_end).unwrap_or_default();

    let content =
        select_sub_expire_warn_content(auto_renew_opt, job_id, &period_start_str, &period_end_str);

    let renewal_hint = sub_expire_warn_renewal_hint(auto_renew_opt, job_id);

    format!(
        "**Localize first** — rewrite the content below in the user's language before sending.\n\
         ```bash\n\
         onchainos agent user-notify --content \"<localized content shown below>\"\n\
         ```\n\
         Content: {content}\n\n\
         {renewal_hint}\n\n\
         End turn after notification.\n"
    )
}

pub(crate) fn sub_complete_notify(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_complete_notify_user_notify(
        svc,
        ctx.job_id,
        extract_i64(message, "subEndTime"),
    );
    let rating_block = build_auto_rating_block(ctx);
    format!(
        "**Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
         ```bash\n\
         onchainos agent user-notify --content \"<localized content shown below>\"\n\
         ```\n\
         Content: {content}\n\n\
         {rating_block}\
         {}\n",
        ctx.terminal_session_hint,
    )
}

pub(crate) fn sub_close_notify(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_close_notify_user_notify(
        svc,
        ctx.job_id,
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
    );
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn sub_reject_refund_notify(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    // Auto-refund is executed by the backend (Sub-4-6, product-confirmed 2026-07-24): the ASP
    // missed the response window and the system has already issued the full refund. This is a
    // display-only terminal notice — the client neither prompts a decision nor calls
    // claim-auto-refund; RefundSettled moves the subscription to Failed.
    let svc = service_name(message, ctx);
    let content = super::super::content::sub_reject_refund_notify_user(
        svc,
        extract_i64(message, "subStartTime"),
        extract_i64(message, "subEndTime"),
        extract_i64(message, "rejectWindowEndsAt"),
        extract_str(message, "tokenAmount"),
        extract_str(message, "tokenSymbol"),
    );
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn sub_failed_notify(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let svc = service_name(message, ctx);
    let reason = extract_str(message, "failReason").or_else(|| extract_str(message, "failReasopn"));
    let content = super::super::content::sub_failed_notify_user_notify(
        svc,
        extract_i64(message, "trialType"),
        reason,
        ctx.job_id,
        extract_i64(message, "subBufferEndTime"),
    );
    let rating_block = build_auto_rating_block(ctx);
    format!(
        "**Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
         ```bash\n\
         onchainos agent user-notify --content \"<localized content shown below>\"\n\
         ```\n\
         Content: {content}\n\n\
         {rating_block}\
         {}\n",
        ctx.terminal_session_hint,
    )
}

/// Check whether the user has already rated this task; if not, gather
/// deliverables + task description and return an auto-rating instruction
/// block for the LLM to evaluate and call `feedback-submit`.
fn build_auto_rating_block(ctx: &FlowContext<'_>) -> String {
    use crate::commands::agent_commerce::task::common::onchainos_self;

    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let provider_id = match ctx
        .prefetched
        .and_then(|p| p.provider_agent_id.as_deref())
        .filter(|s| !s.is_empty())
    {
        Some(s) => s,
        None => return String::new(),
    };

    match onchainos_self::task_feedback_exists(agent_id, job_id) {
        Ok(true) => return String::new(),
        Ok(false) => {}
        Err(_) => {}
    }

    let description = ctx.prefetched.map(|p| p.description.as_str()).unwrap_or("");

    let deliverable_context = build_deliverable_sample(job_id);

    format!(
        "\n**Auto-rate ASP** — compare deliverables against task description, score 0.00–5.00, comment ≤100 chars.\n\n\
         Task description:\n\
         ```\n\
         {description}\n\
         ```\n\n\
         {deliverable_context}\n\
         Execute after evaluation:\n\
         ```bash\n\
         onchainos agent feedback-submit --agent-id {provider_id} --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment>\"\n\
         ```\n"
    )
}

const MAX_SAMPLE_DELIVERABLES: usize = 5;
const MAX_TEXT_PREVIEW_CHARS: usize = 500;

/// Read the deliverable manifest, randomly pick up to 5 entries,
/// and format their content as context for LLM evaluation.
fn build_deliverable_sample(job_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::deliverables;

    let manifest = match deliverables::read_manifest("user", job_id) {
        Ok(Some(m)) if !m.entries.is_empty() => m,
        _ => return "Deliverables: none found\n".to_string(),
    };

    let dir = match deliverables::deliverables_dir("user", job_id) {
        Ok(d) => d,
        Err(_) => return "Deliverables: directory not found\n".to_string(),
    };

    let entries = &manifest.entries;
    let indices = pick_sample_indices(entries.len(), MAX_SAMPLE_DELIVERABLES, job_id);

    let mut parts = Vec::with_capacity(indices.len());
    for (seq, &idx) in indices.iter().enumerate() {
        let entry = &entries[idx];
        let mut part = format!(
            "  {}. {} (type: {}, saved: {})",
            seq + 1,
            entry.original_name,
            entry.deliverable_type,
            entry.saved_at,
        );
        if entry.deliverable_type == "text" {
            let file_path = dir.join(&entry.filename);
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                let preview: String = content.chars().take(MAX_TEXT_PREVIEW_CHARS).collect();
                let was_truncated = content.chars().nth(MAX_TEXT_PREVIEW_CHARS).is_some();
                let display = if was_truncated {
                    format!("{preview}...(truncated)")
                } else {
                    preview
                };
                part.push_str(&format!("\n     Content: {display}"));
            }
        }
        parts.push(part);
    }

    format!(
        "Deliverables ({} total, showing {}):\n{}\n",
        entries.len(),
        indices.len(),
        parts.join("\n"),
    )
}

/// Select up to `max_pick` indices from `[0, total)` via partial
/// Fisher-Yates shuffle seeded by `job_id` (deterministic per job).
fn pick_sample_indices(total: usize, max_pick: usize, job_id: &str) -> Vec<usize> {
    if total <= max_pick {
        return (0..total).collect();
    }
    let mut rng = fnv1a_seed(job_id);
    let mut pool: Vec<usize> = (0..total).collect();
    for i in 0..max_pick {
        rng = rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let j = i + ((rng >> 33) as usize) % (total - i);
        pool.swap(i, j);
    }
    let mut result = pool[..max_pick].to_vec();
    result.sort_unstable();
    result
}

fn fnv1a_seed(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_helpers() {
        let msg = serde_json::json!({
            "event": "sub_created",
            "jobId": "0xabc123",
            "trialType": 1,
            "autoRenew": 0,
            "cancelResult": "success",
            "renewResult": "fail",
            "failReason": "insufficient balance"
        });
        assert_eq!(extract_str(Some(&msg), "event"), Some("sub_created"));
        assert_eq!(extract_str(Some(&msg), "missing"), None);
        assert_eq!(extract_i64(Some(&msg), "trialType"), Some(1));
        assert_eq!(extract_i64(Some(&msg), "missing"), None);
    }

    #[test]
    fn sub_created_has_no_dapp_rescan() {
        // FR-7 / AC-8: the post-`sub_created` DApp text re-scan is removed.
        let ctx = ctx_with_hint();
        let out = sub_created(&ctx, None);
        assert!(
            !out.contains("DApp plugin pre-install"),
            "sub_created must not re-scan for DApps: {out}"
        );
        assert!(
            !out.contains("okx-dapp-discovery"),
            "sub_created must not route to dapp-discovery: {out}"
        );
    }

    // FR-9: sub_expire_warn template selection across all three autoRenew values.
    #[test]
    fn select_sub_expire_warn_content_by_autorenew() {
        let s_true = select_sub_expire_warn_content(Some(1), "JOB-1", "2026-07-01", "2026-07-31");
        let s_none = select_sub_expire_warn_content(None, "JOB-1", "2026-07-01", "2026-07-31");
        let s_false = select_sub_expire_warn_content(Some(0), "JOB-1", "2026-07-01", "2026-07-31");
        // autoRenew=true and missing/legacy both use the existing template (unchanged).
        assert_eq!(s_true, s_none);
        assert!(
            s_true.contains("[Renewal Reminder]"),
            "true → existing: {s_true}"
        );
        // explicit autoRenew=false uses the new "ending soon" template + period range.
        assert_ne!(s_false, s_true);
        assert!(
            s_false.contains("[Subscription Ending Soon]"),
            "false → new: {s_false}"
        );
        assert!(
            s_false.contains("2026-07-01") && s_false.contains("2026-07-31"),
            "period range substituted: {s_false}"
        );
    }

    #[test]
    fn sub_expire_warn_renewal_hint_matches_autorenew_state() {
        let h_true = sub_expire_warn_renewal_hint(Some(1), "JOB-1");
        let h_none = sub_expire_warn_renewal_hint(None, "JOB-1");
        let h_false = sub_expire_warn_renewal_hint(Some(0), "JOB-1");

        assert_eq!(
            h_true, h_none,
            "missing/legacy autoRenew must keep the legacy no-action hint"
        );
        assert!(
            h_true.contains("No action needed"),
            "autoRenew=true hint: {h_true}"
        );
        assert!(
            h_false.contains("start-autorenew JOB-1"),
            "autoRenew=false hint offers enable command: {h_false}"
        );
    }

    const HINT_MARKER: &str = "SESSION_CLEANUP_HINT_MARKER";
    fn ctx_with_hint() -> FlowContext<'static> {
        FlowContext {
            job_id: "job1",
            agent_id: "agent1",
            short_id: "s1",
            title_display: "My Sub",
            title_query_hint: "",
            title_in_extract: "",
            terminal_session_hint: HINT_MARKER.to_string(),
            payment_mode: None,
            prefetched: None,
            data: None,
        }
    }

    #[test]
    fn sub_cancel_trial_success_is_terminal() {
        let ctx = ctx_with_hint();
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "cancelResult": "success", "trialType": 1
        });
        let out = sub_cancel(&ctx, Some(&msg));
        assert!(
            out.contains("Auto-conversion for the \"My Sub\" free trial has been cancelled"),
            "trial cancel shows trial-unaffected copy: {out}"
        );
        assert!(
            out.contains(HINT_MARKER),
            "successful trial cancel is terminal → session-cleanup hint appended: {out}"
        );
    }

    #[test]
    fn sub_cancel_formal_success_is_non_terminal() {
        let ctx = ctx_with_hint();
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "cancelResult": "success", "trialType": 0,
            "subEndTime": 1_700_600_000i64
        });
        let out = sub_cancel(&ctx, Some(&msg));
        assert!(
            out.contains("Auto-renew for \"My Sub\" has been cancelled"),
            "auto-renew-cancelled copy: {out}"
        );
        assert!(
            out.contains("Current service continues until"),
            "renders subEndTime: {out}"
        );
        assert!(
            !out.contains(HINT_MARKER),
            "formal-period cancel is non-terminal → NO session-cleanup hint: {out}"
        );
    }

    #[test]
    fn sub_cancel_fail_is_never_terminal_even_for_trial() {
        // A failed cancel leaves the subscription alive (the trial still converts),
        // so even trialType==1 must NOT get the session-cleanup hint.
        let ctx = ctx_with_hint();
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "cancelResult": "fail", "trialType": 1,
            "failReasopn": "insufficient balance"
        });
        let out = sub_cancel(&ctx, Some(&msg));
        assert!(
            out.contains("insufficient balance"),
            "failReasopn typo fallback plumbed through the handler: {out}"
        );
        assert!(
            !out.contains(HINT_MARKER),
            "failed trial cancel stays non-terminal → NO session-cleanup hint: {out}"
        );
    }

    #[test]
    fn sub_cancel_fail_formal_shows_reason_verbatim() {
        let ctx = ctx_with_hint();
        let reason = "\u{7f51}\u{7edc}\u{9519}\u{8bef}"; // "network error"
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "cancelResult": "fail", "trialType": 0,
            "failReason": reason
        });
        let out = sub_cancel(&ctx, Some(&msg));
        assert!(out.contains(reason), "failReason echoed verbatim: {out}");
        assert!(
            !out.contains(HINT_MARKER),
            "formal-period fail stays non-terminal: {out}"
        );
    }

    #[test]
    fn sub_cancel_trial_end_time_new_name_wins() {
        let ctx = ctx_with_hint();
        let ts = 1_700_000_000i64;
        let only_new = serde_json::json!({ "trialType": 1, "trialEndTime": ts });
        let out = sub_cancel(&ctx, Some(&only_new));
        assert!(
            out.contains("until "),
            "trialEndTime read into the trial-window clause: {out}"
        );
        let both = serde_json::json!({ "trialType": 1, "trialEndTime": ts, "trailEndTime": 1_600_000_000i64 });
        let only_legacy_other =
            serde_json::json!({ "trialType": 1, "trailEndTime": 1_600_000_000i64 });
        assert_eq!(
            sub_cancel(&ctx, Some(&both)),
            out,
            "trialEndTime takes precedence over trailEndTime when both present"
        );
        assert_ne!(
            sub_cancel(&ctx, Some(&both)),
            sub_cancel(&ctx, Some(&only_legacy_other)),
            "the legacy value is not used when the new name is present"
        );
    }

    #[test]
    fn sub_cancel_trail_end_time_legacy_fallback() {
        let ctx = ctx_with_hint();
        let ts = 1_700_000_000i64;
        let only_new = serde_json::json!({ "trialType": 1, "trialEndTime": ts });
        let only_legacy = serde_json::json!({ "trialType": 1, "trailEndTime": ts });
        let out_new = sub_cancel(&ctx, Some(&only_new));
        let out_legacy = sub_cancel(&ctx, Some(&only_legacy));
        assert!(
            out_legacy.contains("until "),
            "legacy trailEndTime fallback still read: {out_legacy}"
        );
        assert_eq!(
            out_new, out_legacy,
            "legacy fallback renders identically to the canonical spelling"
        );
    }

    #[tokio::test]
    async fn sub_renew_fail_is_never_terminal_and_plumbs_reason() {
        // Failed renewal enters the grace period — the subscription is still alive, so the
        // handler must NOT append the session-cleanup hint (close/failed events own that).
        let ctx = ctx_with_hint();
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "renewResult": "fail",
            "failReasopn": "approve\u{4e0d}\u{8db3}", "subBufferEndTime": 1_700_700_000i64
        });
        let out = sub_renew(&ctx, Some(&msg)).await;
        assert!(
            out.contains("[⚠️ Renewal Failed]"),
            "fail branch copy: {out}"
        );
        assert!(
            out.contains("approve\u{4e0d}\u{8db3}"),
            "failReasopn typo fallback plumbed through the handler: {out}"
        );
        assert!(
            !out.contains(HINT_MARKER),
            "failed renewal is non-terminal → NO session-cleanup hint: {out}"
        );
    }

    #[tokio::test]
    async fn sub_renew_success_is_non_terminal() {
        let ctx = ctx_with_hint();
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "renewResult": "success",
            "tokenAmount": "5", "tokenSymbol": "USDT"
        });
        let out = sub_renew(&ctx, Some(&msg)).await;
        assert!(out.contains("[Renewed]"), "success branch copy: {out}");
        assert!(
            !out.contains(HINT_MARKER),
            "successful renewal is non-terminal: {out}"
        );
    }

    #[test]
    fn sub_failed_notify_plumbs_reason_and_is_terminal() {
        let ctx = ctx_with_hint();
        let msg = serde_json::json!({
            "jobTitle": "My Sub", "trialType": 1, "failReason": "\u{4f59}\u{989d}\u{4e0d}\u{8db3}"
        });
        let out = sub_failed_notify(&ctx, Some(&msg));
        assert!(out.contains("[Trial Ended]"), "trial branch label: {out}");
        assert!(
            out.contains("\u{4f59}\u{989d}\u{4e0d}\u{8db3}"),
            "failReason plumbed through the handler: {out}"
        );
        assert!(
            out.contains(HINT_MARKER),
            "sub_failed_notify is terminal by design: {out}"
        );
    }

    // ── fnv1a_seed tests ─────────────────────────────────────────────

    #[test]
    fn fnv1a_empty_string_returns_offset_basis() {
        assert_eq!(fnv1a_seed(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn fnv1a_deterministic() {
        let a = fnv1a_seed("job-42");
        let b = fnv1a_seed("job-42");
        assert_eq!(a, b);
    }

    #[test]
    fn fnv1a_different_inputs_differ() {
        assert_ne!(fnv1a_seed("job-1"), fnv1a_seed("job-2"));
        assert_ne!(fnv1a_seed("abc"), fnv1a_seed("cba"));
    }

    #[test]
    fn fnv1a_single_byte() {
        let h = (0xcbf2_9ce4_8422_2325_u64 ^ b'a' as u64).wrapping_mul(0x0100_0000_01b3);
        assert_eq!(fnv1a_seed("a"), h);
    }

    // ── pick_sample_indices tests ────────────────────────────────────

    #[test]
    fn pick_total_le_max_returns_all_in_order() {
        assert_eq!(pick_sample_indices(0, 5, "x"), Vec::<usize>::new());
        assert_eq!(pick_sample_indices(3, 5, "x"), vec![0, 1, 2]);
        assert_eq!(pick_sample_indices(5, 5, "x"), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn pick_returns_exactly_max_pick_elements() {
        let result = pick_sample_indices(20, 5, "job-100");
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn pick_indices_within_range() {
        let result = pick_sample_indices(10, 3, "job-abc");
        for &idx in &result {
            assert!(idx < 10, "index {idx} out of range [0, 10)");
        }
    }

    #[test]
    fn pick_no_duplicates() {
        let result = pick_sample_indices(100, 5, "job-xyz");
        let mut deduped = result.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(result.len(), deduped.len(), "duplicates found: {result:?}");
    }

    #[test]
    fn pick_result_is_sorted() {
        let result = pick_sample_indices(50, 5, "job-sort");
        let mut sorted = result.clone();
        sorted.sort_unstable();
        assert_eq!(result, sorted);
    }

    #[test]
    fn pick_deterministic_same_job_id() {
        let a = pick_sample_indices(30, 5, "job-deterministic");
        let b = pick_sample_indices(30, 5, "job-deterministic");
        assert_eq!(a, b);
    }

    #[test]
    fn pick_different_job_ids_likely_differ() {
        let a = pick_sample_indices(100, 5, "job-alpha");
        let b = pick_sample_indices(100, 5, "job-beta");
        assert_ne!(
            a, b,
            "different seeds should (almost certainly) produce different samples"
        );
    }
}
