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

fn is_paid(amount: &str) -> bool {
    amount
        .trim()
        .parse::<f64>()
        .is_ok_and(|value| value.is_finite() && value > 0.0)
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

pub(crate) fn job_asp_accept_expire(job_id: &str, task: &PreFetchedTaskContext) -> String {
    let Some(job_name) = authoritative_field(&task.title) else {
        return authoritative_context_required(job_id, "job_asp_accept_expire", &["title"]);
    };
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, "job_asp_accept_expire", &["jobType"]);
    };
    let Some(amount) = authoritative_field(&task.token_amount) else {
        return authoritative_context_required(job_id, "job_asp_accept_expire", &["tokenAmount"]);
    };
    let paid = is_paid(amount);
    let token_symbol = authoritative_field(&task.token_symbol).unwrap_or("");
    if (is_subscription || paid) && token_symbol.is_empty() {
        return authoritative_context_required(job_id, "job_asp_accept_expire", &["tokenSymbol"]);
    }
    let notification = if is_subscription {
        content::subscription_job_asp_accept_expire_asp_notify(
            job_name,
            job_id,
            amount,
            token_symbol,
        )
    } else {
        content::regular_job_asp_accept_expire_asp_notify(
            job_name,
            job_id,
            amount,
            token_symbol,
            paid,
        )
    };
    notification_result(job_id, "job_asp_accept_expire", notification)
}

pub(crate) fn job_asp_reject_closed(
    job_id: &str,
    task: &PreFetchedTaskContext,
    message: Option<&serde_json::Value>,
) -> String {
    let Some(job_name) = authoritative_field(&task.title) else {
        return authoritative_context_required(job_id, "job_asp_reject_closed", &["title"]);
    };
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, "job_asp_reject_closed", &["jobType"]);
    };
    let reason = display_field(message, "aspRejectReason")
        .or_else(|| display_field(message, "reason"))
        .unwrap_or_else(|| "No reason provided".to_string());
    let notification = if is_subscription {
        content::subscription_job_asp_reject_closed_asp_notify(job_name, job_id, &reason)
    } else {
        content::regular_job_asp_reject_closed_asp_notify(job_name, job_id, &reason)
    };
    notification_result(job_id, "job_asp_reject_closed", notification)
}

pub(crate) fn job_asp_reject_expire(job_id: &str, task: &PreFetchedTaskContext) -> String {
    let Some(job_name) = authoritative_field(&task.title) else {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["title"]);
    };
    let Some(is_subscription) = authoritative_task_kind(task) else {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["jobType"]);
    };
    let Some(amount) = authoritative_field(&task.token_amount) else {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["tokenAmount"]);
    };
    let paid = is_paid(amount);
    let token_symbol = authoritative_field(&task.token_symbol).unwrap_or("");
    if (is_subscription || paid) && token_symbol.is_empty() {
        return authoritative_context_required(job_id, "job_asp_reject_expire", &["tokenSymbol"]);
    }
    let notification = if is_subscription {
        content::subscription_job_asp_reject_expire_asp_notify(
            job_name,
            job_id,
            amount,
            token_symbol,
        )
    } else {
        content::regular_job_asp_reject_expire_asp_notify(
            job_name,
            job_id,
            amount,
            token_symbol,
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
        PreFetchedTaskContext::from_api_response(&json!({
            "title": title,
            "jobType": job_type,
            "paymentTokenAmount": token_amount,
            "tokenSymbol": token_symbol,
            "status": status,
        }))
    }

    #[test]
    fn regular_paid_accept_expiry_returns_notify_action() {
        let task = task_context("Audit", 0, "5", "USDT", 8);
        let output = parse(job_asp_accept_expire("job-1", &task));

        assert_eq!(output["phase"], "notification");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "notify_user");
        assert_eq!(output["payload"]["role"], "asp");
        assert_eq!(output["payload"]["notification"]["localize"], true);
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("Escrowed amount: 5 USDT"));
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("Buyer refund settlement remains pending"));
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
        assert!(content.contains("Authoritative audit"));
        assert!(content.contains("Reason: capacity unavailable"));
        assert!(!content.contains("Forged title"));
    }

    #[test]
    fn reject_expire_uses_fresh_authoritative_fields_and_pending_copy() {
        let task = task_context("Authoritative audit", 1, "5", "USDT", 8);
        let output = parse(job_asp_reject_expire("job-1", &task));
        let content = output["payload"]["notification"]["content"]
            .as_str()
            .unwrap();

        assert!(content.contains("Authoritative audit"));
        assert!(content.contains("5 USDT"));
        assert!(content.contains("Automatic refund settlement"));
        assert!(content.contains("is pending"));
        assert!(content.contains("Job status: Expired (8)"));
        assert!(content.contains("No further service delivery is required."));
        assert!(!content.contains("Job status: Closed"));
        assert!(!content.contains("Job status: Failed"));
        assert!(!content.contains("will be returned"));
    }

    #[test]
    fn unsupported_authoritative_job_type_blocks_notification() {
        let task = task_context("Audit", 2, "5", "USDT", 8);
        let output = parse(job_asp_reject_expire("job-1", &task));

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "authoritative_task_context_required");
        assert_eq!(output["payload"]["error"]["missingFields"][0], "jobType");
        assert!(output["nextAction"].as_array().unwrap().is_empty());
        assert!(output["payload"].get("notification").is_none());
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
