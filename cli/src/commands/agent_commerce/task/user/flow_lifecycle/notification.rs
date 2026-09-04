//! Display-only job notifications shared by ordinary and subscription tasks.

use super::super::content;
use super::super::flow::{notify_and_end, FlowContext};

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
    display_field(message, "jobTitle")
        .or_else(|| display_field(message, "jobName"))
        .unwrap_or_else(|| "job".to_string())
}

fn token_amount(message: Option<&serde_json::Value>) -> String {
    display_field(message, "tokenAmount").unwrap_or_else(|| "0".to_string())
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

fn provider_name(message: Option<&serde_json::Value>) -> String {
    display_field(message, "providerName").unwrap_or_else(|| "ASP".to_string())
}

fn provider_agent_id(message: Option<&serde_json::Value>) -> String {
    display_field(message, "providerAgentId").unwrap_or_else(|| "unknown".to_string())
}

fn reason(message: Option<&serde_json::Value>) -> String {
    display_field(message, "aspRejectReason")
        .or_else(|| display_field(message, "reason"))
        .unwrap_or_else(|| "No reason provided".to_string())
}

pub(crate) fn job_asp_accept_expire(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let job_name = job_name(message);
    let amount = token_amount(message);
    let token_symbol = display_field(message, "tokenSymbol").unwrap_or_default();
    let provider_name = provider_name(message);
    let provider_agent_id = provider_agent_id(message);
    let rendered = if is_subscription(message) {
        content::subscription_job_asp_accept_expire_user_notify(
            &job_name,
            ctx.job_id,
            &amount,
            &token_symbol,
            &provider_name,
            &provider_agent_id,
        )
    } else {
        content::regular_job_asp_accept_expire_user_notify(
            &job_name,
            ctx.job_id,
            &amount,
            &token_symbol,
            &provider_name,
            &provider_agent_id,
            is_paid(&amount),
        )
    };
    notify_and_end(&rendered)
}

pub(crate) fn job_asp_reject_closed(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let job_name = job_name(message);
    let amount = token_amount(message);
    let token_symbol = display_field(message, "tokenSymbol").unwrap_or_default();
    let provider_name = provider_name(message);
    let provider_agent_id = provider_agent_id(message);
    let reason = reason(message);
    let rendered = if is_subscription(message) {
        content::subscription_job_asp_reject_closed_user_notify(
            &job_name,
            ctx.job_id,
            &amount,
            &token_symbol,
            &provider_name,
            &provider_agent_id,
            &reason,
        )
    } else {
        content::regular_job_asp_reject_closed_user_notify(
            &job_name,
            ctx.job_id,
            &amount,
            &token_symbol,
            &provider_name,
            &provider_agent_id,
            &reason,
            is_paid(&amount),
        )
    };
    notify_and_end(&rendered)
}

pub(crate) fn job_asp_reject_expire(
    ctx: &FlowContext<'_>,
    message: Option<&serde_json::Value>,
) -> String {
    let job_name = job_name(message);
    let amount = token_amount(message);
    let token_symbol = display_field(message, "tokenSymbol").unwrap_or_default();
    let rendered = if is_subscription(message) {
        content::subscription_job_asp_reject_expire_user_notify(
            &job_name,
            ctx.job_id,
            &amount,
            &token_symbol,
        )
    } else {
        content::regular_job_asp_reject_expire_user_notify(
            &job_name,
            ctx.job_id,
            &amount,
            &token_symbol,
            is_paid(&amount),
        )
    };
    notify_and_end(&rendered)
}
