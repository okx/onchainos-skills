use super::super::content;

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

fn is_subscription(message: Option<&serde_json::Value>) -> bool {
    display_field(message, "jobType").as_deref() == Some("1")
}

fn is_paid(amount: &str) -> bool {
    amount
        .trim()
        .parse::<f64>()
        .is_ok_and(|value| value.is_finite() && value > 0.0)
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

pub(crate) fn job_asp_accept_expire(job_id: &str, message: Option<&serde_json::Value>) -> String {
    let job_name = job_name(message);
    let amount = token_amount(message);
    let token_symbol = token_symbol(message);
    let notification = if is_subscription(message) {
        content::subscription_job_asp_accept_expire_asp_notify(
            &job_name,
            job_id,
            &amount,
            &token_symbol,
        )
    } else {
        content::regular_job_asp_accept_expire_asp_notify(
            &job_name,
            job_id,
            &amount,
            &token_symbol,
            is_paid(&amount),
        )
    };
    notification_result(job_id, "job_asp_accept_expire", notification)
}

pub(crate) fn job_asp_reject_closed(job_id: &str, message: Option<&serde_json::Value>) -> String {
    let job_name = job_name(message);
    let reason = display_field(message, "aspRejectReason")
        .or_else(|| display_field(message, "reason"))
        .unwrap_or_else(|| "No reason provided".to_string());
    let notification = if is_subscription(message) {
        content::subscription_job_asp_reject_closed_asp_notify(&job_name, job_id, &reason)
    } else {
        content::regular_job_asp_reject_closed_asp_notify(&job_name, job_id, &reason)
    };
    notification_result(job_id, "job_asp_reject_closed", notification)
}

pub(crate) fn job_asp_reject_expire(job_id: &str, message: Option<&serde_json::Value>) -> String {
    let job_name = job_name(message);
    let amount = token_amount(message);
    let token_symbol = token_symbol(message);
    let notification = if is_subscription(message) {
        content::subscription_job_asp_reject_expire_asp_notify(
            &job_name,
            job_id,
            &amount,
            &token_symbol,
        )
    } else {
        content::regular_job_asp_reject_expire_asp_notify(
            &job_name,
            job_id,
            &amount,
            &token_symbol,
            is_paid(&amount),
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

    #[test]
    fn regular_paid_accept_expiry_returns_notify_action() {
        let output = parse(job_asp_accept_expire(
            "job-1",
            Some(&json!({
                "jobTitle": "Audit",
                "jobType": 0,
                "tokenAmount": "5",
                "tokenSymbol": "USDT",
            })),
        ));

        assert_eq!(output["phase"], "notification");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "notify_user");
        assert_eq!(output["payload"]["role"], "asp");
        assert_eq!(output["payload"]["notification"]["localize"], true);
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("escrowed amount of 5 USDT"));
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
