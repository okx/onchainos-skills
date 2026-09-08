//! Unified ASP arbitration domain: rejection decisions plus list/detail queries.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::common::network::task_api_client::TaskApiClient;
use super::evaluator::dispute_status::DisputeStatusResponse;

pub const JOB_REJECTED: &str = "job_rejected";
pub const SUB_USER_REJECT: &str = "sub_user_reject";

fn encode_decision_text(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

fn refund_request_card(
    service_name: &str,
    job_id: &str,
    task_type: &str,
    current_period: &str,
    refund_amount: &str,
    refund_reason: &str,
    response_deadline: &str,
) -> (String, String) {
    let table = if task_type == "Subscription" {
        format!(
            "| Service Name | Job ID | Task Type | Current Period | Requested Refund | Buyer’s Reason | Response Deadline |\n\
             |---|---|---|---|---|---|---|\n\
             | {service_name} | {job_id} | {task_type} | {current_period} | {refund_amount} | {refund_reason} | {response_deadline} |"
        )
    } else {
        format!(
            "| Service Name | Job ID | Task Type | Requested Refund | Buyer’s Reason | Response Deadline |\n\
             |---|---|---|---|---|---|\n\
             | {service_name} | {job_id} | {task_type} | {refund_amount} | {refund_reason} | {response_deadline} |"
        )
    };
    let card = format!(
        "### Buyer Refund Request\n\n{table}\n\n\
         Please respond by the deadline. Otherwise, a full refund will be issued automatically.\n\n\
         To refund the buyer, reply “Approve refund.” To dispute the request, reply “Request evaluation” and provide your reason."
    );
    let label = format!("{service_name} — {refund_amount}");
    (encode_decision_text(&card), encode_decision_text(&label))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefundDisplayMetadata {
    pub service_name: String,
    pub task_type: String,
    pub amount: String,
    pub token_symbol: String,
    pub response_deadline: i64,
}

impl RefundDisplayMetadata {
    fn new(
        source_event: &str,
        service_name: Option<&str>,
        amount: Option<&str>,
        token_symbol: Option<&str>,
        response_deadline: Option<i64>,
    ) -> Option<Self> {
        let service_name = service_name
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let amount = amount.map(str::trim).filter(|value| !value.is_empty())?;
        if !super::user::refund_v2::validate_decimal(amount) {
            return None;
        }
        let token_symbol = if super::user::refund_v2::is_zero_decimal(amount) {
            token_symbol.map(str::trim).unwrap_or("")
        } else {
            token_symbol
                .map(str::trim)
                .filter(|value| !value.is_empty())?
        };
        let response_deadline = response_deadline.filter(|value| *value > 0)?;
        super::common::deadline::format_utc_timestamp(response_deadline)?;
        let task_type = match source_event {
            JOB_REJECTED => "One-time",
            SUB_USER_REJECT => "Subscription",
            _ => return None,
        };
        Some(Self {
            service_name: service_name.to_string(),
            task_type: task_type.to_string(),
            amount: amount.to_string(),
            token_symbol: token_symbol.to_string(),
            response_deadline,
        })
    }

    pub fn encode(&self) -> Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self)?))
    }

    pub fn decode(raw: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(raw)
            .context("invalid refund display metadata encoding")?;
        let metadata: Self =
            serde_json::from_slice(&bytes).context("invalid refund display metadata payload")?;
        let expected_task_type = match metadata.task_type.as_str() {
            "One-time" | "Subscription" => metadata.task_type.clone(),
            _ => bail!("invalid refund display task type"),
        };
        Self::new(
            if expected_task_type == "Subscription" {
                SUB_USER_REJECT
            } else {
                JOB_REJECTED
            },
            Some(&metadata.service_name),
            Some(&metadata.amount),
            Some(&metadata.token_symbol),
            Some(metadata.response_deadline),
        )
        .ok_or_else(|| anyhow::anyhow!("refund display metadata is incomplete"))
    }

    pub fn refund_amount_label(&self) -> String {
        if super::user::refund_v2::is_zero_decimal(&self.amount) {
            "No refund required".to_string()
        } else {
            format!("{} {}", self.amount, self.token_symbol)
        }
    }

    pub fn response_deadline_label(&self) -> Option<String> {
        super::common::deadline::format_utc_timestamp(self.response_deadline)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionChoice {
    pub key: String,
    pub action_id: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub params: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedChoice {
    pub action_id: String,
    pub params: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceError {
    Ambiguous,
    MissingReason,
    UnsupportedAction,
}

impl ChoiceError {
    pub fn reason_code(self) -> &'static str {
        match self {
            Self::Ambiguous => "ambiguous_choice",
            Self::MissingReason => "arbitration_reason_required",
            Self::UnsupportedAction => "unsupported_action",
        }
    }
}

pub fn is_decision_source(source_event: &str) -> bool {
    matches!(source_event, JOB_REJECTED | SUB_USER_REJECT)
}

pub fn default_choices(source_event: &str, job_id: &str) -> Vec<DecisionChoice> {
    let ids = match source_event {
        JOB_REJECTED => Some(("agree_refund", "raise_arbitration")),
        SUB_USER_REJECT => Some(("sub_agree_refund", "raise_subscription_arbitration")),
        _ => None,
    };
    let Some((refund, arbitration)) = ids else {
        return Vec::new();
    };
    let params = Map::from_iter([("jobId".to_string(), Value::String(job_id.to_string()))]);
    vec![
        DecisionChoice {
            key: "A".to_string(),
            action_id: refund.to_string(),
            params: params.clone(),
        },
        DecisionChoice {
            key: "B".to_string(),
            action_id: arbitration.to_string(),
            params,
        },
    ]
}

pub fn decision_id(source_event: &str, job_id: &str, message: Option<&Value>) -> String {
    let instance = message
        .and_then(|m| {
            ["eventId", "messageId", "periodIndex", "subStartTime"]
                .into_iter()
                .find_map(|key| scalar_string(m.get(key)))
        })
        .unwrap_or_else(|| "current".to_string());
    format!("{job_id}:{source_event}:{instance}")
}

pub fn build_decision_result(
    source_event: &str,
    job_id: &str,
    name: Option<String>,
    amount: Option<String>,
    token_symbol: Option<String>,
    message: Option<&Value>,
) -> Value {
    let is_subscription = source_event == SUB_USER_REJECT;
    let service_name = message
        .and_then(|value| scalar_string(value.get("serviceName")))
        .or_else(|| name.clone());
    let refund_reason = message.and_then(|value| {
        let keys: &[&str] = if is_subscription {
            &["rejectReason", "refundReason", "userReason"]
        } else {
            &["refundReason", "rejectReason", "userReason", "reason"]
        };
        keys.iter()
            .find_map(|key| exact_nonempty_string(value.get(*key)))
    });
    let period_start = is_subscription
        .then(|| {
            message.and_then(|value| integer_from_keys(value, &["subStartTime", "periodStartTime"]))
        })
        .flatten();
    let period_end = is_subscription
        .then(|| {
            message.and_then(|value| integer_from_keys(value, &["subEndTime", "periodEndTime"]))
        })
        .flatten();
    let current_period = display_period(period_start, period_end);
    let current_period_label = current_period.as_str().map(ToOwned::to_owned);
    let response_deadline_timestamp = message.and_then(|value| {
        let keys: &[&str] = if is_subscription {
            &["rejectWindowEndsAt"]
        } else {
            &["rejectWindowEndsAt", "responseDeadline", "expireTime"]
        };
        integer_from_keys(value, keys)
    });
    let response_deadline = format_timestamp_value(response_deadline_timestamp);
    let response_deadline_label =
        response_deadline_timestamp.and_then(super::common::deadline::format_utc_timestamp);
    let requested_refund = display_amount(amount.as_deref(), token_symbol.as_deref());
    let refund_amount_label = requested_refund.as_str().map(ToOwned::to_owned);

    let mut required = vec![
        ("serviceName", service_name.is_some()),
        ("amount", refund_amount_label.is_some()),
        ("refundReason", refund_reason.is_some()),
        ("responseDeadline", response_deadline_label.is_some()),
    ];
    if is_subscription {
        required.push(("periodStart", period_start.is_some()));
        required.push(("periodEnd", period_end.is_some()));
    }
    let missing = required
        .into_iter()
        .filter_map(|(key, present)| (!present).then_some(key))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        return progression(
            "arbitration_decision",
            "blocked",
            "missing_required_facts",
            Vec::new(),
            json!({"jobId": job_id, "missingFields": missing}),
        );
    }

    let mut choices = default_choices(source_event, job_id);
    if source_event == SUB_USER_REJECT {
        if let Some((key, value)) = subscription_period_binding(message) {
            for choice in &mut choices {
                choice.params.insert(
                    "decisionBindingKey".to_string(),
                    Value::String(key.to_string()),
                );
                choice.params.insert(
                    "decisionBindingValue".to_string(),
                    Value::String(value.clone()),
                );
            }
        }
    }
    let next_actions = choices
        .iter()
        .map(|choice| {
            json!({
                "key": choice.key,
                "id": choice.action_id,
                "recommend": false,
                "params": choice.params,
            })
        })
        .collect::<Vec<_>>();
    let extra_fields = optional_fields(message);
    let buyer_reason = refund_reason
        .as_ref()
        .map(|value| Value::String(value.clone()))
        .unwrap_or(Value::Null);
    let task_type = if is_subscription {
        "Subscription"
    } else {
        "One-time"
    };
    let encoded_card = service_name
        .as_deref()
        .zip(refund_amount_label.as_deref())
        .zip(refund_reason.as_deref())
        .zip(response_deadline_label.as_deref())
        .map(
            |(((service_name, refund_amount), refund_reason), deadline)| {
                refund_request_card(
                    service_name,
                    job_id,
                    task_type,
                    current_period_label.as_deref().unwrap_or(""),
                    refund_amount,
                    refund_reason,
                    deadline,
                )
            },
        );
    let (user_content_b64, list_label_b64) = encoded_card
        .map(|(content, label)| (Some(content), Some(label)))
        .unwrap_or((None, None));
    let refund_display_b64 = RefundDisplayMetadata::new(
        source_event,
        service_name.as_deref(),
        amount.as_deref(),
        token_symbol.as_deref(),
        response_deadline_timestamp,
    )
    .and_then(|metadata| metadata.encode().ok());
    progression(
        "arbitration_decision",
        "requires_user_input",
        "delivery_rejected",
        next_actions,
        json!({
            "jobId": job_id,
            "decisionId": decision_id(source_event, job_id, message),
            "taskType": task_type,
            "name": name,
            "serviceName": service_name,
            "amount": amount,
            "tokenSymbol": token_symbol,
            "currentPeriod": current_period,
            "requestedRefund": requested_refund,
            "buyerReason": buyer_reason.clone(),
            "refundReason": refund_reason,
            "responseDeadline": response_deadline,
            "responseDeadlineLabel": response_deadline_label,
            "responseDeadlineTimestamp": response_deadline_timestamp,
            "userContentB64": user_content_b64,
            "listLabelB64": list_label_b64,
            "refundDisplayB64": refund_display_b64,
            "extraFields": extra_fields,
        }),
    )
}

pub fn build_selected_result(job_id: &str, resolved: &ResolvedChoice) -> Value {
    progression(
        "arbitration_decision",
        "ready",
        "user_choice_resolved",
        vec![json!({
            "id": resolved.action_id,
            "recommend": true,
            "params": resolved.params,
        })],
        json!({"jobId": job_id}),
    )
}

pub fn blocked_result(reason: &str, job_id: &str, details: Value) -> String {
    serde_json::to_string(&progression(
        "arbitration_decision",
        "blocked",
        reason,
        Vec::new(),
        json!({"jobId": job_id, "details": details}),
    ))
    .unwrap_or_else(|_| "{}".to_string())
}

pub fn parse_choices(
    raw: Option<&str>,
    source_event: &str,
    job_id: &str,
) -> Result<Vec<DecisionChoice>, String> {
    let mut choices = match raw {
        Some(raw) => serde_json::from_str::<Vec<DecisionChoice>>(raw)
            .map_err(|e| format!("invalid --choices-json: {e}"))?,
        None => default_choices(source_event, job_id),
    };
    for choice in &mut choices {
        choice.action_id = canonical_action_id(&choice.action_id).to_string();
    }
    validate_choices(&choices, source_event, job_id)?;
    Ok(choices)
}

pub fn validate_choices(
    choices: &[DecisionChoice],
    source_event: &str,
    job_id: &str,
) -> Result<(), String> {
    if !is_decision_source(source_event) {
        return Ok(());
    }
    let expected = default_choices(source_event, job_id);
    if choices.len() != 2
        || choices[0].key != "A"
        || choices[1].key != "B"
        || choices[0].action_id != expected[0].action_id
        || choices[1].action_id != expected[1].action_id
        || !valid_choice_params(choices, job_id)
    {
        return Err(
            "evaluation choices must map A/B to the source event's allowed actions".to_string(),
        );
    }
    Ok(())
}

pub fn resolve_choice(
    source_event: &str,
    choices: &[DecisionChoice],
    user_reply: &str,
) -> Result<ResolvedChoice, ChoiceError> {
    if !is_decision_source(source_event) {
        return Err(ChoiceError::UnsupportedAction);
    }
    let normalized = user_reply.trim();
    let key = deterministic_choice_key(normalized).ok_or(ChoiceError::Ambiguous)?;
    let choice = choices
        .iter()
        .find(|choice| choice.key.eq_ignore_ascii_case(key))
        .ok_or(ChoiceError::UnsupportedAction)?;
    let action_id = canonical_action_id(&choice.action_id);
    if !allowed_action(source_event, action_id) {
        return Err(ChoiceError::UnsupportedAction);
    }
    let mut params = choice.params.clone();
    if key == "B" {
        let reason = arbitration_reason(normalized).ok_or(ChoiceError::MissingReason)?;
        params.insert("reason".to_string(), Value::String(reason));
    }
    Ok(ResolvedChoice {
        action_id: action_id.to_string(),
        params,
    })
}

pub fn resolved_action(
    source_event: &str,
    action_id: &str,
    job_id: &str,
    params: Option<&Value>,
) -> Result<ResolvedChoice, ChoiceError> {
    let action_id = canonical_action_id(action_id);
    if !allowed_action(source_event, action_id) {
        return Err(ChoiceError::UnsupportedAction);
    }
    let mut resolved_params = params
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if resolved_params
        .get("jobId")
        .and_then(Value::as_str)
        .is_some_and(|value| value != job_id)
    {
        return Err(ChoiceError::UnsupportedAction);
    }
    resolved_params.insert("jobId".to_string(), Value::String(job_id.to_string()));
    if matches!(
        action_id,
        "raise_arbitration" | "raise_subscription_arbitration"
    ) && resolved_params
        .get("reason")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(ChoiceError::MissingReason);
    }
    Ok(ResolvedChoice {
        action_id: action_id.to_string(),
        params: resolved_params,
    })
}

pub fn scalar_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn exact_nonempty_string(value: Option<&Value>) -> Option<String> {
    value?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn progression(
    phase: &str,
    decision: &str,
    reason: &str,
    next_action: Vec<Value>,
    payload: Value,
) -> Value {
    json!({
        "phase": phase,
        "decision": decision,
        "reason": reason,
        "nextAction": next_action,
        "payload": payload,
    })
}

fn optional_fields(message: Option<&Value>) -> Map<String, Value> {
    let mut fields = Map::new();
    let Some(message) = message else {
        return fields;
    };
    for key in [
        "periodIndex",
        "subStartTime",
        "subEndTime",
        "rejectWindowEndsAt",
        "expireTime",
    ] {
        if let Some(value) = message.get(key).filter(|value| !value.is_null()) {
            fields.insert(key.to_string(), value.clone());
        }
    }
    fields
}

fn subscription_period_binding(message: Option<&Value>) -> Option<(&'static str, String)> {
    let message = message?;
    ["periodIndex", "subStartTime", "subEndTime"]
        .into_iter()
        .find_map(|key| scalar_string(message.get(key)).map(|value| (key, value)))
}

fn valid_choice_params(choices: &[DecisionChoice], job_id: &str) -> bool {
    if choices.len() != 2 || choices[0].params != choices[1].params {
        return false;
    }
    let params = &choices[0].params;
    if params.get("jobId").and_then(Value::as_str) != Some(job_id)
        || params.keys().any(|key| {
            !matches!(
                key.as_str(),
                "jobId" | "decisionBindingKey" | "decisionBindingValue"
            )
        })
    {
        return false;
    }
    match (
        params.get("decisionBindingKey").and_then(Value::as_str),
        params.get("decisionBindingValue").and_then(Value::as_str),
    ) {
        (None, None) => true,
        (Some(key), Some(value)) => {
            matches!(key, "periodIndex" | "subStartTime" | "subEndTime") && !value.trim().is_empty()
        }
        _ => false,
    }
}

fn allowed_action(source_event: &str, action_id: &str) -> bool {
    matches!(
        (source_event, action_id),
        (JOB_REJECTED, "raise_arbitration")
            | (JOB_REJECTED, "agree_refund")
            | (SUB_USER_REJECT, "raise_subscription_arbitration")
            | (SUB_USER_REJECT, "sub_agree_refund")
    )
}

fn canonical_action_id(action_id: &str) -> &str {
    match action_id {
        // Local pending cards issued before the terminology migration may
        // still carry these IDs. Accept them only at the boundary and always
        // emit the canonical arbitration vocabulary downstream.
        "dispute_raise" => "raise_arbitration",
        "sub_dispute" => "raise_subscription_arbitration",
        "view_dispute" => "view_arbitration",
        value => value,
    }
}

fn deterministic_choice_key(reply: &str) -> Option<&'static str> {
    let lowered = reply.to_ascii_lowercase();
    let starts_a = starts_with_choice(&lowered, 'a');
    let starts_b = starts_with_choice(&lowered, 'b');
    if (starts_a && contains_choice_marker(&lowered, 'b'))
        || (starts_b && contains_choice_marker(&lowered, 'a'))
    {
        return None;
    }
    if starts_a
        || lowered.starts_with("agree refund")
        || lowered.starts_with("agree to refund")
        || lowered.starts_with("accept refund")
        || lowered.starts_with("accept full refund")
        || lowered.starts_with("approve refund")
    {
        return Some("A");
    }
    if starts_b
        || lowered.starts_with("file dispute")
        || lowered.starts_with("raise dispute")
        || lowered.starts_with("file arbitration")
        || lowered.starts_with("raise arbitration")
        || lowered.starts_with("request evaluation")
        || lowered.starts_with("start evaluation")
        || lowered.starts_with("file for evaluation")
    {
        return Some("B");
    }
    None
}

fn contains_choice_marker(value: &str, expected: char) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| token.len() == 1 && token.starts_with(expected))
}

fn starts_with_choice(value: &str, expected: char) -> bool {
    let mut chars = value.chars();
    chars.next() == Some(expected)
        && chars
            .next()
            .is_none_or(|next| next.is_whitespace() || matches!(next, '.' | ':' | ','))
}

fn arbitration_reason(reply: &str) -> Option<String> {
    let trimmed = reply.trim();
    let after_choice = if starts_with_choice(&trimmed.to_ascii_lowercase(), 'b') {
        trimmed.get(1..).unwrap_or("")
    } else {
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("file dispute") {
            trimmed.get("file dispute".len()..).unwrap_or("")
        } else if lower.starts_with("raise dispute") {
            trimmed.get("raise dispute".len()..).unwrap_or("")
        } else if lower.starts_with("file arbitration") {
            trimmed.get("file arbitration".len()..).unwrap_or("")
        } else if lower.starts_with("raise arbitration") {
            trimmed.get("raise arbitration".len()..).unwrap_or("")
        } else if lower.starts_with("request evaluation") {
            trimmed.get("request evaluation".len()..).unwrap_or("")
        } else if lower.starts_with("start evaluation") {
            trimmed.get("start evaluation".len()..).unwrap_or("")
        } else if lower.starts_with("file for evaluation") {
            trimmed.get("file for evaluation".len()..).unwrap_or("")
        } else {
            ""
        }
    };
    let reason = after_choice
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ':' | ','));
    let reason = if let Some(prefix) = reason.get(.."reason".len()) {
        let rest = reason.get("reason".len()..).unwrap_or("");
        if prefix.eq_ignore_ascii_case("reason")
            && rest
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || matches!(c, '.' | ':' | ','))
        {
            rest
        } else {
            reason
        }
    } else {
        reason
    };
    let reason = reason
        .trim_start_matches(|c: char| c.is_whitespace() || c == ':')
        .trim();
    (!reason.is_empty()).then(|| reason.to_string())
}

fn integer_value(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
}

fn integer_from_keys(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| integer_value(value.get(*key)))
}

fn format_timestamp(timestamp: Option<i64>) -> Option<String> {
    super::common::deadline::format_local_timestamp_with_offset(timestamp?)
}

fn format_timestamp_value(timestamp: Option<i64>) -> Value {
    format_timestamp(timestamp)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

fn display_period(start: Option<i64>, end: Option<i64>) -> Value {
    match (format_timestamp(start), format_timestamp(end)) {
        (Some(start), Some(end)) => Value::String(format!("{start}–{end}")),
        _ => Value::Null,
    }
}

fn is_zero_amount(amount: &str) -> bool {
    let value = amount.trim().trim_start_matches('+');
    !value.is_empty()
        && value
            .chars()
            .all(|character| matches!(character, '0' | '.'))
}

fn display_amount(amount: Option<&str>, token_symbol: Option<&str>) -> Value {
    let Some(amount) = amount.map(str::trim).filter(|value| !value.is_empty()) else {
        return Value::Null;
    };
    if is_zero_amount(amount) {
        return Value::String("No refund required".to_string());
    }
    let Some(token_symbol) = token_symbol
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Value::Null;
    };
    Value::String(format!("{amount} {token_symbol}"))
}

fn evaluation_status_at(
    task_status: Option<i64>,
    prepare_end_time: Option<i64>,
    now_seconds: i64,
    now_millis: i64,
) -> &'static str {
    if matches!(task_status, Some(6 | 9)) {
        return "Decided";
    }
    if let Some(deadline) = prepare_end_time {
        let now = if deadline.unsigned_abs() >= 100_000_000_000 {
            now_millis
        } else {
            now_seconds
        };
        if now <= deadline {
            return "Evidence preparation";
        }
    }
    "Evaluating"
}

fn evaluation_status(task_status: Option<i64>, prepare_end_time: Option<i64>) -> &'static str {
    let now = chrono::Utc::now();
    evaluation_status_at(
        task_status,
        prepare_end_time,
        now.timestamp(),
        now.timestamp_millis(),
    )
}

/// Unified read-only CLI entry for ASP evaluation cases.
pub async fn handle_arbitration_list(
    client: &mut TaskApiClient,
    agent_id: &str,
    page: u32,
    page_size: u32,
) -> Result<()> {
    let agent_id = agent_id.trim();
    if agent_id.is_empty() {
        bail!("--agent-id must not be empty");
    }
    if page == 0 {
        bail!("--page must be greater than 0");
    }
    if page_size == 0 {
        bail!("--page-size must be greater than 0");
    }

    // The backend retains its historical dispute path. This module normalizes
    // that protocol into the public arbitration progression contract.
    let path = client.dispute_list_path(page, page_size);
    let response = client.get_with_agent_id(&path, agent_id).await?;
    let items = response["list"].as_array().cloned().unwrap_or_default();
    let total = response["total"].as_u64().unwrap_or(0);
    let mut enriched = Vec::with_capacity(items.len());
    for item in items {
        let status = match item["jobId"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(job_id) => {
                let path = client.endpoint(job_id, "dispute/status");
                match client.get_with_agent_id(&path, agent_id).await {
                    Ok(response) => serde_json::from_value::<DisputeStatusResponse>(response).ok(),
                    Err(_) => None,
                }
            }
            None => None,
        };
        enriched.push((item, status));
    }
    let result = build_list_result(page, total, &enriched);
    crate::output::success(result);
    Ok(())
}

/// Unified read-only CLI entry for one ASP evaluation case.
pub async fn handle_arbitration_detail(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    let job_id = job_id.trim();
    let agent_id = agent_id.trim();
    if job_id.is_empty() {
        bail!("jobId must not be empty");
    }
    if agent_id.is_empty() {
        bail!("--agent-id must not be empty");
    }

    // Keep Elvis's public query contract: JWT + agenticId only. Evaluator and
    // legacy generic-status flows may use their own identity/session contract.
    let path = client.endpoint(job_id, "dispute/status");
    let backend_response = client.get_with_agent_id(&path, agent_id).await?;
    let arbitration = serde_json::from_value::<DisputeStatusResponse>(backend_response.clone())
        .context("failed to parse evaluation detail response")?;
    let supplement = if arbitration.job_type == Some(1) {
        client
            .fetch_subscription(job_id, agent_id)
            .await
            .unwrap_or_else(|_| json!({}))
    } else {
        client
            .get_with_identity(&client.task_path(job_id), agent_id)
            .await
            .unwrap_or_else(|_| json!({}))
    };
    let evidence_path = client.endpoint(job_id, "evidence");
    let evidence = client
        .get_with_agent_id(&evidence_path, agent_id)
        .await
        .ok();
    let result = build_detail_result(
        job_id,
        &supplement,
        Some(&arbitration),
        Some(&backend_response),
        evidence.as_ref(),
    );
    crate::output::success(result);
    Ok(())
}

fn value_from_keys(value: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| value.get(*key).filter(|value| !value.is_null()).cloned())
        .unwrap_or(Value::Null)
}

fn backend_task_status_name(code: i64) -> &'static str {
    match code {
        0 => "created",
        1 => "accepted",
        2 => "submitted",
        3 => "rejected",
        4 => "disputed",
        5 => "admin_stopped",
        6 => "complete",
        7 => "close",
        8 => "expired",
        9 => "failed",
        _ => "unknown",
    }
}

/// Normalize the ASP-facing arbitration phase from the backend task status and
/// evidence deadline. Backend fields such as `disputeRoundStatus` remain in the
/// payload unchanged and are not the merchant-facing phase authority.
fn arbitration_phase_at(
    task_status: Option<i64>,
    prepare_end_time: Option<i64>,
    now_seconds: i64,
    now_millis: i64,
) -> &'static str {
    match task_status {
        Some(6 | 9) => "resolved",
        Some(4) => match prepare_end_time {
            Some(deadline) => {
                let now = if deadline >= 100_000_000_000 {
                    now_millis
                } else {
                    now_seconds
                };
                if now <= deadline {
                    "evidence_preparation"
                } else {
                    "in_progress"
                }
            }
            None => "unknown",
        },
        _ => "unknown",
    }
}

fn arbitration_phase(task_status: Option<i64>, prepare_end_time: Option<i64>) -> &'static str {
    let now = chrono::Utc::now();
    arbitration_phase_at(
        task_status,
        prepare_end_time,
        now.timestamp(),
        now.timestamp_millis(),
    )
}

fn arbitration_verdict(task_status: Option<i64>) -> Value {
    match task_status {
        Some(6) => Value::String("asp_won".to_string()),
        Some(9) => Value::String("asp_lost_auto_refund".to_string()),
        _ => Value::Null,
    }
}

pub(crate) fn build_list_result(
    page: u32,
    total: u64,
    items: &[(Value, Option<DisputeStatusResponse>)],
) -> Value {
    let items = items
        .iter()
        .filter_map(|(item, arbitration)| {
            let job_id = item["jobId"].as_str()?.trim();
            if job_id.is_empty() {
                return None;
            }
            let task_status = arbitration
                .as_ref()
                .map(|value| i64::from(value.task_status))
                .or_else(|| item["status"].as_i64());
            let prepare_end_time = arbitration.as_ref().and_then(|value| value.prepare_end_time);
            let status = evaluation_status(task_status, prepare_end_time);
            let key_time = match status {
                "Evidence preparation" => format_timestamp_value(prepare_end_time),
                "Evaluating" => format_timestamp_value(
                    arbitration.as_ref().and_then(|value| value.round_end_time),
                ),
                "Decided" => format_timestamp_value(integer_from_keys(
                    item,
                    &["resolvedAt", "decisionTime", "updatedAt", "updateTime"],
                )),
                _ => Value::Null,
            };
            Some(json!({
                "jobId": job_id,
                "serviceName": value_from_keys(item, &["serviceName", "title", "jobTitle"]),
                "status": status,
                "evaluationStarted": format_timestamp_value(integer_from_keys(item, &["disputeTime", "evaluationStartedAt", "createdAt", "createTime"])),
                "keyTime": key_time,
                "description": value_from_keys(item, &["title"]),
                "occurredAt": value_from_keys(item, &["createTime"]),
                "taskStatus": task_status.map(backend_task_status_name),
                "taskStatusCode": task_status,
                "arbitrationPhase": arbitration_phase(task_status, prepare_end_time),
                "verdict": arbitration_verdict(task_status),
            }))
        })
        .collect::<Vec<_>>();
    let allowed_job_ids = items
        .iter()
        .filter_map(|item| item["jobId"].as_str())
        .collect::<Vec<_>>();
    progression(
        "arbitration_list",
        "ready",
        if items.is_empty() {
            "no_arbitrations"
        } else {
            "arbitrations_found"
        },
        if allowed_job_ids.is_empty() {
            Vec::new()
        } else {
            vec![json!({
                "id": "view_arbitration",
                "recommend": false,
                "params": {
                    "allowedJobIds": allowed_job_ids,
                    "confirmationRequired": false,
                }
            })]
        },
        json!({"total": total, "page": page, "items": items}),
    )
}

pub(crate) fn build_detail_result(
    job_id: &str,
    supplement: &Value,
    arbitration: Option<&DisputeStatusResponse>,
    status_payload: Option<&Value>,
    evidence: Option<&Value>,
) -> Value {
    let task_status = arbitration
        .map(|value| i64::from(value.task_status))
        .or_else(|| supplement["status"].as_i64());
    let prepare_end_time = arbitration.and_then(|value| value.prepare_end_time);
    let phase = arbitration_phase(task_status, prepare_end_time);
    let status = evaluation_status(task_status, prepare_end_time);
    let explicit_verdict = value_from_keys(supplement, &["verdict", "disputeResult"]);
    let verdict = if explicit_verdict.is_null() {
        arbitration_verdict(task_status)
    } else {
        explicit_verdict
    };
    let service_name = value_from_keys(supplement, &["serviceName", "title", "jobTitle"]);
    let amount = arbitration
        .and_then(|value| value.token_amount.clone())
        .or_else(|| {
            scalar_string(Some(&value_from_keys(
                supplement,
                &["tokenAmount", "serviceTokenAmount"],
            )))
        });
    let token_symbol = arbitration
        .and_then(|value| value.token_symbol.clone())
        .or_else(|| {
            scalar_string(Some(&value_from_keys(
                supplement,
                &["tokenSymbol", "paymentTokenSymbol"],
            )))
        });
    let buyer_reason = evidence
        .map(|value| value_from_keys(&value["client"], &["reason"]))
        .filter(|value| !value.is_null())
        .unwrap_or(Value::Null);
    let evaluation_started = status_payload
        .and_then(|value| integer_from_keys(value, &["disputeTime", "createdAt", "createTime"]))
        .or_else(|| integer_from_keys(supplement, &["disputeTime"]));
    progression(
        "arbitration_detail",
        "ready",
        "arbitration_found",
        Vec::new(),
        json!({
            "jobId": job_id,
            "serviceName": service_name,
            "requestedRefund": display_amount(amount.as_deref(), token_symbol.as_deref()),
            "buyerReason": buyer_reason,
            "status": status,
            "evaluationStarted": format_timestamp_value(evaluation_started),
            "description": value_from_keys(supplement, &["title", "serviceName"]),
            "occurredAt": value_from_keys(supplement, &["disputeTime", "updatedAt", "updateTime", "createdAt", "createTime"]),
            "jobType": arbitration.and_then(|value| value.job_type),
            "amount": amount,
            "tokenSymbol": token_symbol,
            "taskStatus": task_status.map(backend_task_status_name),
            "taskStatusCode": task_status,
            "arbitrationPhase": phase,
            "currentRound": arbitration.and_then(|value| value.current_round),
            "disputeRoundStatus": arbitration.and_then(|value| value.dispute_round_status),
            "prepareEndTime": prepare_end_time,
            "roundEndTime": arbitration.and_then(|value| value.round_end_time),
            "deadline": match phase {
                "evidence_preparation" => prepare_end_time,
                "in_progress" => arbitration.and_then(|value| value.round_end_time),
                _ => None,
            },
            "verdict": verdict,
            "fundDestination": value_from_keys(supplement, &["fundDestination", "fundsTo"]),
            "refundAmount": value_from_keys(supplement, &["refundAmount"]),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_result_uses_one_shared_shape_for_both_task_types() {
        for source in [JOB_REJECTED, SUB_USER_REJECT] {
            let message = if source == SUB_USER_REJECT {
                json!({
                    "periodIndex": 2,
                    "serviceName": "Signal Service",
                    "rejectReason": "Signals were not delivered",
                    "subStartTime": 1_700_000_000,
                    "subEndTime": 1_700_500_000,
                    "rejectWindowEndsAt": 1_700_600_000,
                })
            } else {
                json!({
                    "periodIndex": 2,
                    "refundReason": "The result was incomplete",
                    "expireTime": 1_700_600_000,
                })
            };
            let result = build_decision_result(
                source,
                "job-1",
                Some("Service".to_string()),
                Some("1.25".to_string()),
                Some("USDT".to_string()),
                Some(&message),
            );
            assert_eq!(result["phase"], "arbitration_decision");
            assert_eq!(result["decision"], "requires_user_input");
            assert_eq!(result["nextAction"].as_array().map(Vec::len), Some(2));
            assert_eq!(result["payload"]["name"], "Service");
        }
    }

    #[test]
    fn decision_result_exposes_refund_card_fields_when_event_supplies_them() {
        let result = build_decision_result(
            SUB_USER_REJECT,
            "job-1",
            Some("Task title".to_string()),
            Some("1.25".to_string()),
            Some("USDT".to_string()),
            Some(&json!({
                "serviceName": "Signal Service",
                "rejectReason": " Signals were not delivered. 详情保持原样 ",
                "subStartTime": 1_700_000_000,
                "subEndTime": 1_700_500_000,
                "rejectWindowEndsAt": 1_700_600_000,
            })),
        );

        assert_eq!(result["payload"]["serviceName"], "Signal Service");
        assert_eq!(
            result["payload"]["refundReason"],
            " Signals were not delivered. 详情保持原样 "
        );
        assert!(result["payload"]["responseDeadline"].is_string());
        assert_eq!(
            result["payload"]["responseDeadlineTimestamp"],
            1_700_600_000i64
        );
        assert_eq!(
            result["payload"]["responseDeadlineLabel"],
            "2023-11-21 20:53 (UTC+00:00)"
        );
        let encoded = result["payload"]["refundDisplayB64"].as_str().unwrap();
        let metadata = RefundDisplayMetadata::decode(encoded).unwrap();
        assert_eq!(metadata.service_name, "Signal Service");
        assert_eq!(metadata.task_type, "Subscription");
        assert_eq!(metadata.refund_amount_label(), "1.25 USDT");
    }

    #[test]
    fn subscription_decision_requires_documented_notice_fields() {
        let result = build_decision_result(
            SUB_USER_REJECT,
            "job-1",
            Some("Task title".to_string()),
            Some("1".to_string()),
            Some("USDT".to_string()),
            Some(&json!({
                "serviceName": "Signal Service",
                "subStartTime": 1_700_000_000,
                "subEndTime": 1_700_500_000,
                "expireTime": 1_700_600_000,
            })),
        );

        assert_eq!(result["decision"], "blocked");
        assert_eq!(result["reason"], "missing_required_facts");
        assert_eq!(
            result["payload"]["missingFields"],
            json!(["refundReason", "responseDeadline"])
        );
        assert_eq!(result["nextAction"], json!([]));
    }

    #[test]
    fn subscription_refund_card_is_encoded_without_rewriting_buyer_reason() {
        let reason = "Oli's result `must` stay.\nSecond line | exact";
        let result = build_decision_result(
            SUB_USER_REJECT,
            "0xfull-job-id",
            Some("Task title".to_string()),
            Some("1.25".to_string()),
            Some("USDT".to_string()),
            Some(&json!({
                "serviceName": "Signal Service",
                "rejectReason": reason,
                "subStartTime": 1_700_000_000,
                "subEndTime": 1_700_500_000,
                "rejectWindowEndsAt": 1_700_600_000,
            })),
        );

        let content = URL_SAFE_NO_PAD
            .decode(result["payload"]["userContentB64"].as_str().unwrap())
            .unwrap();
        let content = String::from_utf8(content).unwrap();
        assert!(content.contains("### Buyer Refund Request"));
        assert!(content.contains("0xfull-job-id"));
        assert!(content.contains(reason));
        assert!(content.contains("Approve refund"));
        assert!(content.contains("Request evaluation"));
    }

    #[test]
    fn subscription_zero_refund_uses_authoritative_label() {
        let result = build_decision_result(
            SUB_USER_REJECT,
            "job-1",
            Some("Task title".to_string()),
            Some("0".to_string()),
            None,
            Some(&json!({
                "serviceName": "Signal Service",
                "rejectReason": "No service was delivered",
                "subStartTime": 1_700_000_000,
                "subEndTime": 1_700_500_000,
                "rejectWindowEndsAt": 1_700_600_000,
            })),
        );

        assert_eq!(result["payload"]["requestedRefund"], "No refund required");
        assert!(result["payload"]["userContentB64"].is_string());
    }

    #[test]
    fn missing_card_facts_block_actions() {
        let result = build_decision_result(
            JOB_REJECTED,
            "job-1",
            None,
            Some("1".to_string()),
            Some("USDT".to_string()),
            None,
        );
        assert_eq!(result["decision"], "blocked");
        assert_eq!(result["reason"], "missing_required_facts");
        assert_eq!(result["nextAction"], json!([]));
    }

    #[test]
    fn deterministic_reply_maps_choice_and_requires_evaluation_reason() {
        let choices = default_choices(JOB_REJECTED, "job-1");
        let approved = resolve_choice(JOB_REJECTED, &choices, "Approve refund").unwrap();
        assert_eq!(approved.action_id, "agree_refund");
        assert_eq!(
            resolve_choice(JOB_REJECTED, &choices, "File for evaluation"),
            Err(ChoiceError::MissingReason)
        );
        let evaluation = resolve_choice(
            JOB_REJECTED,
            &choices,
            "File for evaluation: delivery was incomplete",
        )
        .unwrap();
        assert_eq!(evaluation.action_id, "raise_arbitration");
        assert_eq!(evaluation.params["reason"], "delivery was incomplete");
        let selected = resolve_choice(
            JOB_REJECTED,
            &choices,
            "B reason: delivery below expectation",
        )
        .unwrap();
        assert_eq!(selected.params["reason"], "delivery below expectation");
        assert_eq!(
            resolve_choice(JOB_REJECTED, &choices, "Approve refund")
                .unwrap()
                .action_id,
            "agree_refund"
        );
        assert_eq!(
            resolve_choice(JOB_REJECTED, &choices, "A")
                .unwrap()
                .action_id,
            "agree_refund"
        );
        assert_eq!(
            resolve_choice(JOB_REJECTED, &choices, "Request evaluation"),
            Err(ChoiceError::MissingReason)
        );
        let selected = resolve_choice(
            JOB_REJECTED,
            &choices,
            "Request evaluation: the delivery met the agreed requirements",
        )
        .unwrap();
        assert_eq!(selected.action_id, "raise_arbitration");
        assert_eq!(
            selected.params["reason"],
            "the delivery met the agreed requirements"
        );
        let selected = resolve_choice(
            JOB_REJECTED,
            &choices,
            "request evaluation reason: delivery meets the specification",
        )
        .unwrap();
        assert_eq!(
            selected.params["reason"],
            "delivery meets the specification"
        );
        assert_eq!(
            resolve_choice(JOB_REJECTED, &choices, "B"),
            Err(ChoiceError::MissingReason)
        );
    }

    #[test]
    fn reversed_arbitration_choices_are_rejected() {
        let reversed = serde_json::to_string(&vec![
            DecisionChoice {
                key: "A".to_string(),
                action_id: "raise_arbitration".to_string(),
                params: Map::new(),
            },
            DecisionChoice {
                key: "B".to_string(),
                action_id: "agree_refund".to_string(),
                params: Map::new(),
            },
        ])
        .unwrap();
        assert!(parse_choices(Some(&reversed), JOB_REJECTED, "job-1").is_err());
    }

    #[test]
    fn arbitration_choices_and_resolved_actions_are_bound_to_the_envelope_job() {
        let mut choices = default_choices(JOB_REJECTED, "job-2");
        choices[0]
            .params
            .insert("jobId".to_string(), Value::String("job-1".to_string()));
        assert!(parse_choices(
            Some(&serde_json::to_string(&choices).unwrap()),
            JOB_REJECTED,
            "job-2"
        )
        .is_err());

        assert_eq!(
            resolved_action(
                JOB_REJECTED,
                "agree_refund",
                "job-2",
                Some(&json!({"jobId": "job-1"}))
            ),
            Err(ChoiceError::UnsupportedAction)
        );
        assert_eq!(
            resolved_action(
                JOB_REJECTED,
                "raise_arbitration",
                "job-2",
                Some(&json!({"jobId": "job-2"}))
            ),
            Err(ChoiceError::MissingReason)
        );
    }

    #[test]
    fn ambiguous_reply_never_maps_to_write_action() {
        let choices = default_choices(SUB_USER_REJECT, "job-1");
        assert_eq!(
            resolve_choice(SUB_USER_REJECT, &choices, "OK"),
            Err(ChoiceError::Ambiguous)
        );
        assert_eq!(
            resolve_choice(SUB_USER_REJECT, &choices, "A or B"),
            Err(ChoiceError::Ambiguous)
        );
    }

    #[test]
    fn subscription_decision_id_is_period_scoped() {
        let first = decision_id(SUB_USER_REJECT, "job-1", Some(&json!({"periodIndex": 2})));
        let second = decision_id(SUB_USER_REJECT, "job-1", Some(&json!({"periodIndex": 3})));
        assert_ne!(first, second);
    }

    #[test]
    fn subscription_choices_carry_period_binding_into_the_relay() {
        let result = build_decision_result(
            SUB_USER_REJECT,
            "job-1",
            Some("Service".to_string()),
            Some("1".to_string()),
            Some("USDT".to_string()),
            Some(&json!({
                "periodIndex": 2,
                "serviceName": "Signal Service",
                "rejectReason": "Signals were not delivered",
                "subStartTime": 1_700_000_000,
                "subEndTime": 1_700_500_000,
                "rejectWindowEndsAt": 1_700_600_000,
            })),
        );
        for action in result["nextAction"].as_array().unwrap() {
            assert_eq!(action["params"]["decisionBindingKey"], "periodIndex");
            assert_eq!(action["params"]["decisionBindingValue"], "2");
        }
    }

    #[test]
    fn legacy_pending_action_ids_normalize_to_arbitration_ids() {
        let legacy = json!([
            {"key": "A", "actionId": "agree_refund", "params": {"jobId": "job-1"}},
            {"key": "B", "actionId": "dispute_raise", "params": {"jobId": "job-1"}}
        ]);
        let choices = parse_choices(Some(&legacy.to_string()), JOB_REJECTED, "job-1").unwrap();
        assert_eq!(choices[1].action_id, "raise_arbitration");
        assert_eq!(
            resolved_action(
                JOB_REJECTED,
                "dispute_raise",
                "job-1",
                Some(&json!({"jobId": "job-1", "reason": "completed as agreed"}))
            )
            .unwrap()
            .action_id,
            "raise_arbitration"
        );
    }

    #[test]
    fn arbitration_list_exposes_stable_selection_ids() {
        let result = build_list_result(
            1,
            1,
            &[(
                json!({
                    "jobId": "job-1",
                    "title": "Research",
                    "status": 4,
                    "createTime": 1_700_000_000,
                }),
                None,
            )],
        );
        assert_eq!(result["phase"], "arbitration_list");
        assert_eq!(result["payload"]["items"][0]["jobId"], "job-1");
        assert_eq!(result["payload"]["items"][0]["description"], "Research");
        assert_eq!(result["payload"]["items"][0]["occurredAt"], 1_700_000_000);
        assert_eq!(result["payload"]["items"][0]["taskStatus"], "disputed");
        assert_eq!(result["payload"]["items"][0]["status"], "Evaluating");
        assert!(result["payload"]["items"][0]["verdict"].is_null());
        assert_eq!(result["nextAction"][0]["id"], "view_arbitration");
        assert_eq!(
            result["nextAction"][0]["params"]["allowedJobIds"],
            json!(["job-1"])
        );
        assert_eq!(
            result["nextAction"][0]["params"]["confirmationRequired"],
            false
        );
    }

    #[test]
    fn arbitration_detail_keeps_unknown_backend_fields_null() {
        let result = build_detail_result(
            "job-1",
            &json!({"status": 4, "title": "Research", "disputeTime": 123}),
            None,
            None,
            None,
        );
        assert_eq!(result["phase"], "arbitration_detail");
        assert_eq!(result["payload"]["jobId"], "job-1");
        assert_eq!(result["payload"]["occurredAt"], 123);
        assert_eq!(result["payload"]["arbitrationPhase"], "unknown");
        assert!(result["payload"]["verdict"].is_null());
        assert_eq!(result["payload"]["status"], "Evaluating");
    }

    #[test]
    fn arbitration_detail_uses_backend_dispute_status_fields() {
        let arbitration: DisputeStatusResponse = serde_json::from_value(json!({
            "jobId": "job-1",
            "jobType": 1,
            "currentRound": 2,
            "selectedVoter": null,
            "taskStatus": 4,
            "disputeRoundStatus": 1,
            "prepareEndTime": 100,
            "roundEndTime": 200,
            "tokenAmount": "3",
            "tokenSymbol": "USDT"
        }))
        .unwrap();
        let result = build_detail_result(
            "job-1",
            &json!({
                "status": 6,
                "title": "Research",
                "createTime": 50,
                "tokenAmount": "stale"
            }),
            Some(&arbitration),
            None,
            Some(&json!({"client": {"reason": "The output missed the requested scope"}})),
        );

        assert_eq!(result["payload"]["jobType"], 1);
        assert_eq!(result["payload"]["taskStatusCode"], 4);
        assert_eq!(result["payload"]["taskStatus"], "disputed");
        assert_eq!(result["payload"]["arbitrationPhase"], "in_progress");
        assert_eq!(result["payload"]["currentRound"], 2);
        assert_eq!(result["payload"]["prepareEndTime"], 100);
        assert_eq!(result["payload"]["roundEndTime"], 200);
        assert_eq!(result["payload"]["deadline"], 200);
        assert_eq!(result["payload"]["amount"], "3");
        assert_eq!(result["payload"]["tokenSymbol"], "USDT");
        assert_eq!(result["payload"]["status"], "Evaluating");
        assert_eq!(
            result["payload"]["buyerReason"],
            "The output missed the requested scope"
        );
    }

    #[test]
    fn terminal_task_status_maps_to_asp_verdict() {
        for (task_status, expected) in [(6, "asp_won"), (9, "asp_lost_auto_refund")] {
            let arbitration: DisputeStatusResponse = serde_json::from_value(json!({
                "jobId": "job-1",
                "taskStatus": task_status,
                "disputeRoundStatus": 3
            }))
            .unwrap();
            let result = build_detail_result("job-1", &json!({}), Some(&arbitration), None, None);
            assert_eq!(result["payload"]["arbitrationPhase"], "resolved");
            assert_eq!(result["payload"]["status"], "Decided");
            assert_eq!(result["payload"]["verdict"], expected);
        }
    }

    #[test]
    fn arbitration_uses_prepare_deadline_with_inclusive_boundary() {
        assert_eq!(
            arbitration_phase_at(Some(4), Some(2_000), 2_000, 2_000_000),
            "evidence_preparation"
        );
        assert_eq!(
            arbitration_phase_at(Some(4), Some(1_999), 2_000, 2_000_000),
            "in_progress"
        );
        assert_eq!(
            arbitration_phase_at(
                Some(4),
                Some(2_000_000_000_000),
                2_000_000_000,
                2_000_000_000_000,
            ),
            "evidence_preparation"
        );
    }

    #[test]
    fn evaluation_status_uses_only_the_three_product_states() {
        assert_eq!(
            evaluation_status_at(Some(4), Some(2_000), 2_000, 2_000_000),
            "Evidence preparation"
        );
        assert_eq!(
            evaluation_status_at(Some(4), Some(1_999), 2_000, 2_000_000),
            "Evaluating"
        );
        assert_eq!(
            evaluation_status_at(Some(6), None, 2_000, 2_000_000),
            "Decided"
        );
    }
}
