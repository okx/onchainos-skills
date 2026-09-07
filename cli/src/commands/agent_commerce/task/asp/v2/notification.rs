use super::super::content;
use crate::commands::agent_commerce::task::common::PreFetchedTaskContext;

fn display_field(message: Option<&serde_json::Value>, key: &str) -> Option<String> {
    message
        .and_then(|value| value.get(key))
        .and_then(|value| match value {
            serde_json::Value::String(value) if !value.is_empty() => Some(value.clone()),
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

fn display_i64(message: Option<&serde_json::Value>, key: &str) -> Option<i64> {
    message.and_then(|value| value.get(key)).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn job_name(message: Option<&serde_json::Value>) -> String {
    ["jobTitle", "jobName"]
        .into_iter()
        .find_map(|key| display_field(message, key))
        .unwrap_or_else(|| "job".to_string())
}

fn token_amount(message: Option<&serde_json::Value>) -> String {
    display_field(message, "tokenAmount").unwrap_or_else(|| "0".to_string())
}

fn token_symbol(message: Option<&serde_json::Value>) -> String {
    display_field(message, "tokenSymbol").unwrap_or_default()
}

fn payment_is_paid(amount: &str) -> Option<bool> {
    let amount = amount.trim();
    let mut parts = amount.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    let valid = parts.next().is_none()
        && !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction
            .is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    valid.then(|| amount.bytes().any(|byte| byte != b'0' && byte != b'.'))
}

fn authoritative_field(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty() && value != "?").then_some(value)
}

fn authoritative_task_kind(task: &PreFetchedTaskContext) -> Option<bool> {
    match task.job_type {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

fn service_name(task: &PreFetchedTaskContext, message: Option<&serde_json::Value>) -> String {
    task.service_name
        .as_deref()
        .and_then(authoritative_field)
        .map(ToOwned::to_owned)
        .or_else(|| display_field(message, "serviceName"))
        .or_else(|| authoritative_field(&task.title).map(ToOwned::to_owned))
        .unwrap_or_else(|| "Service unavailable".to_string())
}

fn notification_result(job_id: &str, event: &str, notification: String) -> String {
    serde_json::json!({
        "phase": "notification",
        "decision": "ready",
        "reason": "notification_required",
        "nextAction": [{
            "id": "notify_user",
            "recommend": true,
            "params": {
                "jobId": job_id,
                "event": event,
            },
        }],
        "payload": {
            "role": "asp",
            "event": event,
            "jobId": job_id,
            "notification": {
                "content": notification,
                "localize": true,
            },
            "rating": { "required": false },
        },
    })
    .to_string()
}

/// Reuse the established ASP terminal-notification action contract. The action
/// ID is legacy-named for subscriptions, but its payload is job-scoped and the
/// consumer performs the required notify + session-cleanup sequence.
fn terminal_notification_result(job_id: &str, event: &str, notification: String) -> String {
    serde_json::json!({
        "phase": "notification",
        "decision": "ready",
        "reason": "notification_required",
        "nextAction": [{
            "id": "notify_and_cleanup_subscription",
            "recommend": true,
            "params": { "jobId": job_id },
        }],
        "payload": {
            "role": "asp",
            "event": event,
            "jobId": job_id,
            "notification": {
                "content": notification,
                "localize": true,
            },
            "rating": { "required": false },
            "cleanup": { "jobId": job_id },
        },
    })
    .to_string()
}

pub(crate) fn authoritative_context_required(
    job_id: &str,
    event: &str,
    missing_fields: &[&str],
) -> String {
    serde_json::json!({
        "phase": "notification",
        "decision": "blocked",
        "reason": "authoritative_task_context_required",
        "nextAction": [],
        "payload": {
            "role": "asp",
            "event": event,
            "jobId": job_id,
            "error": {
                "code": "authoritative_task_context_required",
                "missingFields": missing_fields,
            },
            "rating": { "required": false },
        },
    })
    .to_string()
}

pub(crate) fn job_asp_accept_expire(
    job_id: &str,
    task: &PreFetchedTaskContext,
    message: Option<&serde_json::Value>,
) -> String {
    if task.status != Some(8) {
        return authoritative_context_required(job_id, "job_asp_accept_expire", &["status"]);
    }
    let service_name = service_name(task, message);
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, "job_asp_accept_expire", &["jobType"]);
    };
    let is_trial = if is_subscription {
        match task.trial_type {
            Some(0) => false,
            Some(1) => true,
            _ => {
                return authoritative_context_required(
                    job_id,
                    "job_asp_accept_expire",
                    &["trialType"],
                );
            }
        }
    } else {
        false
    };
    let amount = authoritative_field(&task.token_amount).unwrap_or("");
    let paid = if is_trial {
        false
    } else {
        let Some(paid) = payment_is_paid(amount) else {
            return authoritative_context_required(
                job_id,
                "job_asp_accept_expire",
                &["tokenAmount"],
            );
        };
        paid
    };
    let token_symbol = authoritative_field(&task.token_symbol).unwrap_or("payment token");
    let notification = if is_subscription {
        content::subscription_job_asp_accept_expire_asp_notify(
            &service_name,
            job_id,
            amount,
            token_symbol,
            is_trial,
            paid,
        )
    } else {
        content::regular_job_asp_accept_expire_asp_notify(&service_name, job_id)
    };
    terminal_notification_result(job_id, "job_asp_accept_expire", notification)
}

pub(crate) fn job_delivery_expired(
    job_id: &str,
    task: &PreFetchedTaskContext,
    event: &str,
) -> String {
    if task.status != Some(8) {
        return authoritative_context_required(job_id, event, &["status"]);
    }
    let job_name = authoritative_field(&task.title).unwrap_or("Task title unavailable");
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, event, &["jobType"]);
    };
    let (task_type, is_trial) = if is_subscription {
        match task.trial_type {
            Some(0) => ("Subscription (1)", false),
            Some(1) => ("Subscription (1)", true),
            _ => return authoritative_context_required(job_id, event, &["trialType"]),
        }
    } else {
        ("One-time task (0)", false)
    };
    let amount = authoritative_field(&task.token_amount).unwrap_or("");
    let paid = if is_trial {
        false
    } else {
        let Some(paid) = payment_is_paid(amount) else {
            return authoritative_context_required(job_id, event, &["tokenAmount"]);
        };
        paid
    };
    let token_symbol = authoritative_field(&task.token_symbol).unwrap_or("payment token");
    let notification = content::job_delivery_expire_asp_notify(
        job_name,
        job_id,
        task_type,
        amount,
        token_symbol,
        is_trial,
        paid,
    );
    terminal_notification_result(job_id, event, notification)
}

pub(crate) fn job_asp_reject_closed(
    job_id: &str,
    task: &PreFetchedTaskContext,
    message: Option<&serde_json::Value>,
) -> String {
    let service_name = service_name(task, message);
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, "job_asp_reject_closed", &["jobType"]);
    };
    let reason = display_field(message, "aspRejectReason")
        .or_else(|| display_field(message, "reason"))
        .unwrap_or_else(|| "No reason provided".to_string());
    let notification = if is_subscription {
        content::subscription_job_asp_reject_closed_asp_notify(&service_name, job_id, &reason)
    } else {
        content::regular_job_asp_reject_closed_asp_notify(&service_name, job_id, &reason)
    };
    notification_result(job_id, "job_asp_reject_closed", notification)
}

pub(crate) fn job_asp_reject_expire(
    job_id: &str,
    task: &PreFetchedTaskContext,
    message: Option<&serde_json::Value>,
) -> String {
    if task.status != Some(9) {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["status"]);
    }
    let service_name = service_name(task, message);
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["jobType"]);
    };
    let Some(amount) = authoritative_field(&task.token_amount) else {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["tokenAmount"]);
    };
    let paid = payment_is_paid(amount).unwrap_or(false);
    let token_symbol = authoritative_field(&task.token_symbol).unwrap_or("");
    if (is_subscription || paid) && token_symbol.is_empty() {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["tokenSymbol"]);
    }
    let notification = if is_subscription {
        content::subscription_job_asp_reject_expire_asp_notify(
            &service_name,
            job_id,
            amount,
            token_symbol,
            display_i64(message, "rejectWindowEndsAt"),
        )
    } else {
        content::regular_job_asp_reject_expire_asp_notify(
            &service_name,
            job_id,
            amount,
            token_symbol,
            display_i64(message, "rejectWindowEndsAt"),
            paid,
        )
    };
    notification_result(job_id, "job_asp_reject_expire", notification)
}

pub(crate) fn sub_asp_claim_notify(job_id: &str, message: Option<&serde_json::Value>) -> String {
    let job_name = job_name(message);
    let amount = token_amount(message);
    let token_symbol = token_symbol(message);
    let tx_hash = display_field(message, "txHash").unwrap_or_else(|| "Not provided".to_string());
    let notification = content::sub_asp_claim_notify_asp_notify(
        &job_name,
        job_id,
        &amount,
        &token_symbol,
        &tx_hash,
    );
    notification_result(job_id, "sub_asp_claim_notify", notification)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(output: String) -> serde_json::Value {
        serde_json::from_str(&output).unwrap()
    }

    fn task_context(
        title: &str,
        job_type: i64,
        token_amount: &str,
        token_symbol: &str,
        status: i64,
    ) -> PreFetchedTaskContext {
        let mut context = PreFetchedTaskContext::from_api_response(&json!({
            "title": title,
            "serviceName": format!("{title} service"),
            "jobType": job_type,
            "paymentTokenAmount": token_amount,
            "tokenSymbol": token_symbol,
            "status": status,
        }));
        if job_type == 1 {
            context.trial_type = Some(0);
        }
        context
    }

    #[test]
    fn regular_paid_accept_expiry_matches_spec_and_uses_terminal_cleanup_action() {
        let task = task_context("Audit", 0, "5", "USDT", 8);
        let output = parse(job_asp_accept_expire("job-1", &task, None));

        assert_eq!(output["phase"], "notification");
        assert_eq!(output["decision"], "ready");
        assert_eq!(
            output["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(output["payload"]["role"], "asp");
        assert_eq!(output["payload"]["notification"]["localize"], true);
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("You did not process Audit service within 3 hours"));
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("Job status: Expired"));
        assert_eq!(output["payload"]["cleanup"]["jobId"], "job-1");
    }

    #[test]
    fn reject_closed_uses_authoritative_task_fields_but_keeps_event_reason() {
        let task = task_context("Authoritative audit", 1, "5", "USDT", 7);
        let caller_message = json!({
            "jobTitle": "Forged title",
            "jobType": 0,
            "tokenAmount": "999",
            "tokenSymbol": "FAKE",
            "aspRejectReason": "capacity unavailable",
        });

        let output = parse(job_asp_reject_closed("job-1", &task, Some(&caller_message)));
        let content = output["payload"]["notification"]["content"]
            .as_str()
            .unwrap();

        assert!(content.contains("[Task Declined]"));
        assert!(content.contains("Authoritative audit service"));
        assert!(content.contains("Reason: capacity unavailable"));
        assert!(!content.contains("Forged title"));
    }

    #[test]
    fn reject_expire_uses_fresh_failed_authoritative_fields() {
        let task = task_context("Authoritative audit", 1, "5", "USDT", 9);
        let event = json!({ "rejectWindowEndsAt": 1_700_000_000 });
        let output = parse(job_asp_reject_expire("job-1", &task, Some(&event)));
        let content = output["payload"]["notification"]["content"]
            .as_str()
            .unwrap();

        assert!(content.contains("Authoritative audit service"));
        assert!(content.contains("5 USDT"));
        assert!(content.contains("Automatic Refund Processing"));
        assert!(content.contains("will be returned to the User Agent’s wallet"));
        assert!(content.contains("Response deadline: 2023-11-14 22:13 UTC"));
        assert!(content.contains("Job status: Failed"));
        assert!(!content.contains("Job status: Closed"));
        assert!(!content.contains("Job status: Expired"));
        assert!(!content.contains("is pending"));
    }

    #[test]
    fn unsupported_authoritative_job_type_blocks_notification() {
        let task = task_context("Audit", 2, "5", "USDT", 9);
        let output = parse(job_asp_reject_expire("job-1", &task, None));

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "authoritative_task_context_required");
        assert_eq!(output["payload"]["error"]["missingFields"][0], "jobType");
        assert!(output["nextAction"].as_array().unwrap().is_empty());
        assert!(output["payload"].get("notification").is_none());
    }

    #[test]
    fn delivery_expiry_reports_authoritative_refund_and_terminal_cleanup() {
        let task = task_context("Analysis", 0, "5", "USDT", 8);
        for event in ["job_expired", "submit_expired"] {
            let output = parse(job_delivery_expired("job-1", &task, event));
            assert_eq!(output["payload"]["event"], event);
            assert_eq!(
                output["nextAction"][0]["id"],
                "notify_and_cleanup_subscription"
            );
            assert_eq!(output["payload"]["cleanup"]["jobId"], "job-1");
            let content = output["payload"]["notification"]["content"]
                .as_str()
                .unwrap();
            assert!(content.contains("[Delivery Expired]"));
            assert!(content.contains("funds have reached the Buyer"));
        }
    }

    #[test]
    fn trial_and_zero_subscription_expiry_are_terminal_without_refund_claim() {
        let mut trial = task_context("Trial signals", 1, "12.34", "USDT", 8);
        trial.trial_type = Some(1);
        let trial = parse(job_asp_accept_expire("job-1", &trial, None));
        let content = trial["payload"]["notification"]["content"]
            .as_str()
            .unwrap();
        assert!(content.contains("Neither the subscription nor the free trial began"));
        assert!(!content.contains("funds have reached the Buyer"));
        assert_eq!(
            trial["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );

        let zero = task_context("Free signals", 1, "0.000", "USDT", 8);
        let zero = parse(job_asp_accept_expire("job-2", &zero, None));
        let content = zero["payload"]["notification"]["content"].as_str().unwrap();
        assert!(content.contains("The subscription did not begin"));
        assert!(!content.contains("funds have reached the Buyer"));
    }

    #[test]
    fn subscription_claim_uses_tx_hash() {
        let output = parse(sub_asp_claim_notify(
            "job-2",
            Some(&json!({
                "jobTitle": "Signals",
                "tokenAmount": "0.003",
                "tokenSymbol": "USDT",
                "txHash": "0xclaim",
            })),
        ));

        assert_eq!(output["payload"]["event"], "sub_asp_claim_notify");
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("Transaction: 0xclaim"));
    }
}
