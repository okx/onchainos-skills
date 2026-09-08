//! Terminal states, timeouts, auto-completion, and fallback prompt generators.

use super::super::flow::{notify_and_end, notify_and_end_terminal, FlowContext};

fn display_or_unavailable(value: Option<&str>) -> &str {
    value.unwrap_or("unavailable")
}

fn authoritative_title<'a>(ctx: &'a FlowContext<'_>) -> &'a str {
    ctx.prefetched
        .map(|value| value.title.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("Task title unavailable")
}

fn message_text(message: Option<&serde_json::Value>, key: &str) -> Option<String> {
    message
        .and_then(|value| value.get(key))
        .and_then(|value| match value {
            serde_json::Value::String(value) if !value.trim().is_empty() => {
                Some(value.trim().to_string())
            }
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

fn message_i64(message: Option<&serde_json::Value>, key: &str) -> Option<i64> {
    message.and_then(|value| value.get(key)).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn service_name(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    ctx.prefetched
        .and_then(|value| value.service_name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| message_text(message, "serviceName"))
        .unwrap_or_else(|| authoritative_title(ctx).to_string())
}

fn final_refund_notice(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
    automatic: bool,
    expected_status: i64,
) -> (String, bool) {
    let verified = super::super::refund::verify_final_refund_event(
        message,
        ctx.prefetched,
        expected_status,
        ctx.agent_id,
    );
    let provider_id = ctx
        .prefetched
        .and_then(|value| value.provider_agent_id.as_deref());
    let provider_name = ctx
        .prefetched
        .and_then(|value| value.provider_name.as_deref());
    let mut provider = match (provider_name, provider_id) {
        (Some(name), Some(id)) => format!("{name} ({id})"),
        (Some(name), None) => name.to_string(),
        (None, Some(id)) => format!("name unavailable ({id})"),
        (None, None) => "not provided by the final event".to_string(),
    };
    let mut service = ctx
        .prefetched
        .and_then(|value| value.service_name.as_deref())
        .or_else(|| ctx.prefetched.and_then(|value| value.service_id.as_deref()))
        .unwrap_or("unverified")
        .to_string();
    let amount = ctx
        .prefetched
        .map(|value| value.token_amount.as_str())
        .filter(|value| !value.is_empty());
    let symbol = ctx
        .prefetched
        .map(|value| value.token_symbol.as_str())
        .filter(|value| !value.is_empty() && *value != "?");
    let mut amount_display = match (amount, symbol) {
        (Some(amount), Some(symbol)) => format!("{amount} {symbol}"),
        (Some(amount), None) => format!("{amount} (token symbol unavailable)"),
        _ => "not provided by the final event".to_string(),
    };
    let mut tx_hash = None;
    if let Ok(evidence) = &verified {
        provider = format!(
            "{} ({})",
            evidence.provider_name, evidence.provider_agent_id
        );
        service = evidence.service_name.clone();
        amount_display = format!("{} {}", evidence.amount, evidence.token_symbol);
        tx_hash = evidence.tx_hash.clone();
    }
    let complete = verified.is_ok();
    let heading = if complete {
        if automatic {
            "[Auto-Refund Settled]"
        } else {
            "[Refund Settled]"
        }
    } else {
        "[Refund Settlement Detail Incomplete]"
    };
    let status = if complete {
        if tx_hash.is_some() {
            "Refund confirmed by the backend's on-chain lifecycle result and fresh task detail; funds returned to your wallet. The verified Tx Hash is shown above. This job is complete.".to_string()
        } else {
            "Refund confirmed by the backend's on-chain lifecycle result and fresh task detail; funds returned to your wallet. The backend did not expose the Tx Hash in the current response. This job is complete.".to_string()
        }
    } else {
        format!(
            "Refund completion cannot be verified: {}. Do not claim completion from this message; refresh Refund status.",
            verified.unwrap_err()
        )
    };
    (
        format!(
            "{heading} {} (`{}`)\n\
             - Refund ASP: {provider}\n\
             - Service: {service}\n\
             - Refund amount: {amount_display}\n\
             - Tx Hash: {}\n\
             {status}",
            authoritative_title(ctx),
            ctx.job_id,
            display_or_unavailable(tx_hash.as_deref()),
        ),
        complete,
    )
}

fn notify_refund_result(ctx: &FlowContext<'_>, content: &str, complete: bool) -> String {
    if complete {
        notify_and_end_terminal(content, &ctx.terminal_session_hint)
    } else {
        notify_and_end(content)
    }
}

pub(crate) fn job_refunded(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    let (content, complete) = final_refund_notice(ctx, message, false, 9);
    notify_refund_result(ctx, &content, complete)
}

pub(crate) fn job_auto_refunded(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let (content, complete) = final_refund_notice(ctx, message, true, 9);
    notify_refund_result(ctx, &content, complete)
}

fn expired_terminal_result(ctx: &FlowContext<'_>, cause: &str) -> String {
    let Some(detail) = ctx.prefetched else {
        let content = format!(
            "[Expired Task Detail Incomplete] {} (`{}`): fresh authoritative Expired(8) task detail is unavailable. Do not infer refund receipt or clean up from caller-supplied event data.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end(&content);
    };
    if detail.status != Some(8) || detail.user_agent_id.as_deref() != Some(ctx.agent_id) {
        let content = format!(
            "[Expired Task Detail Incomplete] {} (`{}`): fresh task detail does not prove buyer-owned Expired(8). Do not infer refund receipt or clean up from caller-supplied event data.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end(&content);
    }

    let trial = match detail.job_type {
        Some(0) => false,
        Some(1) => match detail.trial_type {
            Some(0) => false,
            Some(1) => true,
            _ => {
                let content = format!(
                    "[Expired Task Detail Incomplete] {} (`{}`): fresh subscription detail is missing a supported trialType. Do not infer that funds moved.",
                    authoritative_title(ctx), ctx.job_id
                );
                return notify_and_end(&content);
            }
        },
        _ => {
            let content = format!(
                "[Expired Task Detail Incomplete] {} (`{}`): fresh task detail is missing a supported jobType. Do not infer that funds moved.",
                authoritative_title(ctx), ctx.job_id
            );
            return notify_and_end(&content);
        }
    };
    let zero_amount = super::super::refund::is_zero_decimal(detail.token_amount.trim());
    if trial || zero_amount {
        let no_funds = if trial {
            "This was a trial subscription, so no refundable escrow payment was collected."
        } else {
            "The task had a zero payment amount, so no funds needed to be returned."
        };
        let content = format!(
            "[Job Expired] {} (`{}`): {cause}. {no_funds} The task is complete and no buyer-side refund action is required.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end_terminal(&content, &ctx.terminal_session_hint);
    }

    let (content, complete) = final_refund_notice(ctx, None, true, 8);
    notify_refund_result(
        ctx,
        &format!("{content}\n- Timeout result: {cause}"),
        complete,
    )
}

pub(crate) fn job_expired(ctx: &FlowContext<'_>) -> String {
    expired_terminal_result(ctx, "A task deadline elapsed")
}

pub(crate) fn job_asp_accept_expire(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let Some(detail) = ctx.prefetched else {
        let content = format!(
            "[ASP Acceptance Timeout Detail Incomplete] {} (`{}`): fresh authoritative task detail does not prove buyer-owned Expired(8). Do not report refund settlement and do not initiate any buyer-side refund claim or finalization.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end(&content);
    };
    if detail.status != Some(8) || detail.user_agent_id.as_deref() != Some(ctx.agent_id) {
        let content = format!(
            "[ASP Acceptance Timeout Detail Incomplete] {} (`{}`): fresh authoritative task detail does not prove buyer-owned Expired(8). Do not report refund settlement and do not initiate any buyer-side refund claim or finalization.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end(&content);
    }

    let (is_subscription, is_trial) = match detail.job_type {
        Some(0) => (false, false),
        Some(1) => match detail.trial_type {
            Some(0) => (true, false),
            Some(1) => (true, true),
            _ => {
                let content = format!(
                    "[ASP Acceptance Timeout Detail Incomplete] {} (`{}`): fresh authoritative subscription detail is missing a supported trialType. Do not claim that refundable escrow was collected or returned.",
                    authoritative_title(ctx), ctx.job_id
                );
                return notify_and_end(&content);
            }
        },
        _ => {
            let content = format!(
                "[ASP Acceptance Timeout Detail Incomplete] {} (`{}`): fresh authoritative task detail is missing a supported jobType. Do not render caller-provided display fields or report refund settlement.",
                authoritative_title(ctx), ctx.job_id
            );
            return notify_and_end(&content);
        }
    };
    let service_name = service_name(ctx, message);
    let provider_name = detail
        .provider_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("name unavailable");
    let provider_agent_id = detail
        .provider_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unavailable");
    let amount = detail.token_amount.trim();
    let token_symbol = detail.token_symbol.trim();
    let paid_refund_confirmed =
        super::super::refund::authoritative_refund_settlement_confirmed(detail, 8);
    let zero_amount = super::super::refund::is_zero_decimal(amount);
    if !is_trial && !zero_amount && !paid_refund_confirmed {
        let content = format!(
            "[ASP Acceptance Timeout Detail Incomplete] Job `{}` has fresh buyer-owned Expired(8), but its original payment amount is invalid. Do not substitute caller-provided fields or report a refund amount.",
            ctx.job_id
        );
        return notify_and_end(&content);
    }

    let content = super::super::content::job_asp_accept_expire_user_notify(
        ctx.job_id,
        &service_name,
        is_subscription,
        provider_name,
        provider_agent_id,
        if amount.is_empty() {
            "unavailable"
        } else {
            amount
        },
        if token_symbol.is_empty() || token_symbol == "?" {
            "token symbol unavailable"
        } else {
            token_symbol
        },
        !zero_amount,
        is_trial,
    );
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn job_asp_reject_expire(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let authoritative_refund_complete = ctx.prefetched.is_some_and(|detail| {
        detail.status == Some(9)
            && detail.user_agent_id.as_deref() == Some(ctx.agent_id)
            && detail.refund_request_provenance
    });
    if !authoritative_refund_complete {
        let content = format!(
            "[Automatic Refund Detail Incomplete] {} (`{}`): fresh authoritative detail does not combine buyer-owned Failed(9) with the durable local request-refund provenance for this task. Do not report refund completion or clean up the session. Run `onchainos agent refund-prepare {}` to reconcile.",
            authoritative_title(ctx), ctx.job_id, ctx.job_id
        );
        return notify_and_end(&content);
    }
    let detail = ctx.prefetched.expect("checked above");
    let is_subscription = match detail.job_type {
        Some(0) => false,
        Some(1) => true,
        _ => {
            let content = format!(
                "[Automatic Refund Detail Incomplete] {} (`{}`): fresh task detail is missing a supported jobType.",
                authoritative_title(ctx), ctx.job_id
            );
            return notify_and_end(&content);
        }
    };
    let amount = detail.token_amount.trim();
    let token_symbol = detail.token_symbol.trim();
    let content = super::super::content::job_asp_reject_expire_user_notify(
        ctx.job_id,
        &service_name(ctx, message),
        amount,
        token_symbol,
        message_i64(message, "rejectWindowEndsAt"),
        is_subscription,
        !super::super::refund::is_zero_decimal(amount),
    );
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn job_asp_reject_closed(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let Some(detail) = ctx.prefetched else {
        let content = format!(
            "[Job Close Detail Incomplete] {} (`{}`): fresh authoritative detail is unavailable.",
            authoritative_title(ctx),
            ctx.job_id
        );
        return notify_and_end(&content);
    };
    if detail.status != Some(7) || detail.user_agent_id.as_deref() != Some(ctx.agent_id) {
        let content = format!(
            "[Job Close Detail Incomplete] {} (`{}`): fresh authoritative detail does not prove Closed(7) ownership by the current User Agent.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end(&content);
    }

    let service_name = service_name(ctx, message);
    let amount = detail.token_amount.trim();
    let token_symbol = detail.token_symbol.trim();
    let provider_name = detail
        .provider_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ASP");
    let provider_agent_id = detail
        .provider_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unavailable");
    let reason = message_text(message, "aspRejectReason")
        .or_else(|| message_text(message, "reason"))
        .unwrap_or_else(|| "No reason provided".to_string());

    let content = match detail.job_type {
        Some(0) => super::super::content::regular_job_asp_reject_closed_user_notify(
            &service_name,
            ctx.job_id,
            amount,
            token_symbol,
            provider_name,
            provider_agent_id,
            &reason,
            !super::super::refund::is_zero_decimal(amount),
        ),
        Some(1) => {
            let trial_type = message_i64(message, "trialType").or(detail.trial_type);
            let is_trial = match trial_type {
                Some(0) => false,
                Some(1) => true,
                _ => {
                    let content = format!(
                        "[Job Close Detail Incomplete] {} (`{}`): subscription notification is missing a supported trialType.",
                        authoritative_title(ctx), ctx.job_id
                    );
                    return notify_and_end(&content);
                }
            };
            super::super::content::subscription_job_asp_reject_closed_user_notify(
                &service_name,
                ctx.job_id,
                amount,
                token_symbol,
                provider_name,
                provider_agent_id,
                &reason,
                is_trial,
            )
        }
        _ => {
            let content = format!(
                "[Job Close Detail Incomplete] {} (`{}`): fresh task detail is missing a supported jobType.",
                authoritative_title(ctx), ctx.job_id
            );
            return notify_and_end(&content);
        }
    };
    notify_and_end_terminal(&content, &ctx.terminal_session_hint)
}

pub(crate) fn job_closed(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    closed_notice(ctx, message)
}

fn closed_notice(ctx: &FlowContext<'_>, message: Option<&serde_json::Value>) -> String {
    let authoritative_close_owned_by_user = ctx.prefetched.is_some_and(|value| {
        value.status == Some(7) && value.user_agent_id.as_deref() == Some(ctx.agent_id)
    });
    if !authoritative_close_owned_by_user {
        let content = format!(
            "[Job Close Detail Incomplete] {} (`{}`): fresh authoritative detail does not prove Closed(7) ownership by the current User Agent. Do not report closure or refund completion; run `onchainos agent refund-prepare {}`.",
            authoritative_title(ctx), ctx.job_id, ctx.job_id
        );
        return notify_and_end(&content);
    }
    let amount = ctx
        .prefetched
        .map(|value| value.token_amount.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let zero_price = ctx
        .prefetched
        .is_some_and(|value| value.job_type == Some(0))
        && super::super::refund::is_zero_decimal(amount);
    if zero_price {
        let content = format!(
            "[Job Closed] {} (`{}`) has been closed. The task price was 0, so no refund was required.",
            authoritative_title(ctx), ctx.job_id
        );
        return notify_and_end_terminal(&content, &ctx.terminal_session_hint);
    }

    let (content, complete) = final_refund_notice(ctx, message, false, 7);
    notify_refund_result(ctx, &content, complete)
}

// --- Timeouts / auto-completion ---------------------------------------

pub(crate) async fn submit_expired(ctx: &FlowContext<'_>) -> String {
    expired_terminal_result(
        ctx,
        "The ASP did not submit the deliverable before the deadline",
    )
}

pub(crate) fn reject_expired(ctx: &FlowContext<'_>) -> String {
    // Refund makes the ASP-response timeout settlement a backend
    // responsibility. Wait for its backend transaction-result projection.
    let content = super::super::content::reject_expired_user_notify(ctx.job_id);
    notify_and_end(&content)
}

pub(crate) fn review_deadline_warn(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_display = ctx.title_display;
    let review_deadline_prompt =
        super::super::content::review_deadline_warn_user_prompt(job_id, short_id);
    let request_block =
        crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
            job_id,
            "user",
            agent_id,
            ctx.prefetched.and_then(|p| p.provider_agent_id.as_deref()),
            &review_deadline_prompt,
            &format!("[Decision {short_id}] {title_display} acceptance decision (deadline soon)"),
            "review_deadline_warn",
        );
    format!(
    "[System Notification] review_deadline_warn (review deadline approaching)\n\
     [Role] User Agent\n\n\
     **CRITICAL — this event MUST push the review decision to the user via `pending-decisions-v2 request` (NOT a plain text reply, NOT just `onchainos agent user-notify`).**\n\
     Review deadline = user funds safety red line — if the user is not notified, funds auto-release to the ASP on timeout, irreversibly.\n\
     Do not substitute a plain text reply for the `pending-decisions-v2 request` call.\n\
     Do not substitute `onchainos agent user-notify` for the `pending-decisions-v2 request` (the user must make a review decision; a one-way notify cannot relay).\n\n\
     **Push the review decision to the user (5-substep protocol; read ALL 5 before running any command)**:\n\n\
     {request_block}",
    )
}

// --- User-action pseudo events ----------------------------------------

pub(crate) async fn close_task(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let content = format!(
        "[Close Requires V2 Confirmation] Job `{job_id}` was not changed. A local `close` event is not authoritative permission for an irreversible lifecycle/funds write. Run `onchainos agent refund-prepare {job_id}` and execute only the action returned from fresh state after explicit confirmation."
    );
    notify_and_end(&content)
}

// --- Other events ------------------------------------------------------

pub(crate) fn reward_claimed(ctx: &FlowContext<'_>) -> String {
    let content = super::super::content::reward_claimed_user_notify(ctx.job_id, ctx.title_display);
    notify_and_end(&content)
}

pub(crate) fn wakeup_notify(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let wakeup_resume = super::super::content::wakeup_resume_user_notify(job_id);
    format!(
    "[System Notification] wakeup_notify (task wake-up after network / machine restart)\n\
     [Role] User Agent\n\n\
     This is a wake-up heartbeat event, **not** a business-driven event. The real business status lives in envelope.message.jobStatus.\n\
     You should not run a playbook with `wakeup_notify` as --event -- this playbook is only a guide.\n\n\
     [Your next actions (strict order)]\n\n\
     **Step 1 — Read the real status from the envelope**:\n\
     From the wakeup_notify envelope that triggered this turn, read `message.jobStatus` (e.g. `accepted` / `submitted` / `rejected` / `disputed` / `completed` / `failed` and other real status strings).\n\n\
     **Step 2 — Re-call next-action with the real status to fetch the current playbook**:\n\
     ```bash\n\
     onchainos agent next-action --role user --agentId {agent_id} --message '{{\"event\":\"<value of message.jobStatus>\",\"jobId\":\"{job_id}\"}}'\n\
     ```\n\
     Follow the returned playbook for what to do at the current status.\n\n\
     **Step 3 — Idempotency self-check (avoid re-prompting the user)**:\n\
     If the playbook from Step 2 would push a decision to the user — i.e. it contains `onchainos agent pending-decisions-v2 request` — **first** call:\n\
     ```bash\n\
     onchainos agent pending-decisions-v2 list --format json\n\
     ```\n\
     - The returned `entries` already contains an entry with `job_id={job_id}` for this role (the prompt was queued before disconnection) → **skip the script's push step**; instead translate the resume notification below into the user's language and send via `onchainos agent user-notify --content \"<localized content>\"`, then end the turn. Resume notification: {wakeup_resume}\n\
     - No matching entry → run the Step 2 playbook normally; the `pending-decisions-v2 request` call handles the prompt.\n\n\
     **Do not** send the ASP \"I'm back online\" or similar small talk — they do not care about your connection state.\n\
     If the Step 2 playbook is passive (e.g. status=accepted waiting for ASP delivery), just emit a \"task resumed\" notification and end the turn; do not proactively run business actions.\n"
    )
}

// --- Fallback ----------------------------------------------------------

pub(crate) fn staked_and_unknown(event_str: &str, job_id: &str) -> String {
    format!(
        "[Unknown Status] {event_str}\n\
     [Advice]\n\
     1. Call `onchainos agent common context {job_id} --role user` to view full context\n\
     2. If this status is not part of the expected flow, wait for user instructions\n\
     3. Do not predict / assume other notifications\n"
    )
}
