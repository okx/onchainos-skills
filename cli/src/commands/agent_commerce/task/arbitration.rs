//! Unified ASP arbitration domain: rejection decisions plus list/detail queries.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::common::network::task_api_client::TaskApiClient;
use super::evaluator::dispute_status::DisputeStatusResponse;

pub const JOB_REJECTED: &str = "job_rejected";
pub const SUB_USER_REJECT: &str = "sub_user_reject";

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
    UnsupportedAction,
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
    let missing = [
        ("name", name.as_deref()),
        ("amount", amount.as_deref()),
        ("tokenSymbol", token_symbol.as_deref()),
    ]
    .into_iter()
    .filter_map(|(key, value)| {
        value
            .filter(|v| !v.trim().is_empty())
            .is_none()
            .then_some(key)
    })
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
    progression(
        "arbitration_decision",
        "requires_user_input",
        "delivery_rejected",
        next_actions,
        json!({
            "jobId": job_id,
            "decisionId": decision_id(source_event, job_id, message),
            "taskType": if source_event == SUB_USER_REJECT { "subscription" } else { "one_time" },
            "name": name,
            "amount": amount,
            "tokenSymbol": token_symbol,
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
            "arbitration choices must map A/B to the source event's allowed actions".to_string(),
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
        if let Some(reason) = arbitration_reason(normalized) {
            params.insert("reason".to_string(), Value::String(reason));
        }
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
        || reply.starts_with("同意退款")
        || reply.starts_with("同意全额退款")
        || reply.starts_with("确认退款")
        || reply.starts_with("确认退还")
    {
        return Some("A");
    }
    if starts_b
        || lowered.starts_with("file dispute")
        || lowered.starts_with("raise dispute")
        || lowered.starts_with("file arbitration")
        || lowered.starts_with("raise arbitration")
        || reply.starts_with("发起仲裁")
        || reply.starts_with("提起仲裁")
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
        && chars.next().is_none_or(|next| {
            next.is_whitespace() || matches!(next, '.' | ':' | '：' | ',' | '，')
        })
}

fn arbitration_reason(reply: &str) -> Option<String> {
    let trimmed = reply.trim();
    let after_choice = if starts_with_choice(&trimmed.to_ascii_lowercase(), 'b') {
        trimmed.get(1..).unwrap_or("")
    } else if let Some(rest) = trimmed.strip_prefix("发起仲裁") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("提起仲裁") {
        rest
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
        } else {
            ""
        }
    };
    let reason = after_choice
        .trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, '.' | ':' | '：' | ',' | '，')
        })
        .strip_prefix("理由")
        .unwrap_or(after_choice.trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, '.' | ':' | '：' | ',' | '，')
        }))
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '：'))
        .trim();
    (!reason.is_empty()).then(|| reason.to_string())
}

/// Unified read-only CLI entry for ASP arbitration cases.
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
    let result = build_list_result(page, total, &items);
    super::common::network::api_trace::record_contract(
        "arbitration-list",
        &json!({
            "agentId": agent_id,
            "page": page,
            "pageSize": page_size,
            "backendResponse": response,
        }),
        Some(&result),
        None,
    );
    crate::output::success(result);
    Ok(())
}

/// Unified read-only CLI entry for one ASP arbitration case.
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
        .context("failed to parse arbitration detail response")?;
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
    let result = build_detail_result(job_id, &supplement, Some(&arbitration));
    super::common::network::api_trace::record_contract(
        "arbitration-detail",
        &json!({
            "agentId": agent_id,
            "jobId": job_id,
            "backendResponse": backend_response,
            "supplement": supplement,
        }),
        Some(&result),
        None,
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

pub(crate) fn build_list_result(page: u32, total: u64, items: &[Value]) -> Value {
    let items = items
        .iter()
        .filter_map(|item| {
            let job_id = item["jobId"].as_str()?.trim();
            if job_id.is_empty() {
                return None;
            }
            let task_status = item["status"].as_i64();
            Some(json!({
                "jobId": job_id,
                "description": value_from_keys(item, &["title"]),
                "occurredAt": value_from_keys(item, &["createTime"]),
                "taskStatus": task_status.map(backend_task_status_name),
                "taskStatusCode": value_from_keys(item, &["status"]),
                "arbitrationPhase": arbitration_phase(task_status, None),
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
                    "confirmationRequired": true,
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
) -> Value {
    let task_status = arbitration
        .map(|value| i64::from(value.task_status))
        .or_else(|| supplement["status"].as_i64());
    let prepare_end_time = arbitration.and_then(|value| value.prepare_end_time);
    let phase = arbitration_phase(task_status, prepare_end_time);
    let explicit_verdict = value_from_keys(supplement, &["verdict", "disputeResult"]);
    let verdict = if explicit_verdict.is_null() {
        arbitration_verdict(task_status)
    } else {
        explicit_verdict
    };
    progression(
        "arbitration_detail",
        "ready",
        "arbitration_found",
        Vec::new(),
        json!({
            "jobId": job_id,
            "description": value_from_keys(supplement, &["title", "serviceName"]),
            "occurredAt": value_from_keys(supplement, &["disputeTime", "updatedAt", "updateTime", "createdAt", "createTime"]),
            "jobType": arbitration.and_then(|value| value.job_type),
            "amount": arbitration
                .and_then(|value| value.token_amount.clone())
                .map(Value::String)
                .unwrap_or_else(|| value_from_keys(supplement, &["tokenAmount", "serviceTokenAmount"])),
            "tokenSymbol": arbitration
                .and_then(|value| value.token_symbol.clone())
                .map(Value::String)
                .unwrap_or_else(|| value_from_keys(supplement, &["tokenSymbol", "paymentTokenSymbol"])),
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
            "txHash": value_from_keys(supplement, &["disputeTxHash", "refundTxHash", "txHash"]),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_result_uses_one_shared_shape_for_both_task_types() {
        for source in [JOB_REJECTED, SUB_USER_REJECT] {
            let result = build_decision_result(
                source,
                "job-1",
                Some("Service".to_string()),
                Some("1.25".to_string()),
                Some("USDT".to_string()),
                Some(&json!({"periodIndex": 2})),
            );
            assert_eq!(result["phase"], "arbitration_decision");
            assert_eq!(result["decision"], "requires_user_input");
            assert_eq!(result["nextAction"].as_array().map(Vec::len), Some(2));
            assert_eq!(result["payload"]["name"], "Service");
        }
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
    fn deterministic_reply_maps_choice_and_optional_reason() {
        let choices = default_choices(JOB_REJECTED, "job-1");
        let selected = resolve_choice(JOB_REJECTED, &choices, "B，理由：按要求完成").unwrap();
        assert_eq!(selected.action_id, "raise_arbitration");
        assert_eq!(selected.params["reason"], "按要求完成");
        assert_eq!(
            resolve_choice(JOB_REJECTED, &choices, "同意退款")
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
            resolve_choice(JOB_REJECTED, &choices, "发起仲裁")
                .unwrap()
                .action_id,
            "raise_arbitration"
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
            Some(&json!({"periodIndex": 2})),
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
                Some(&json!({"jobId": "job-1"}))
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
            &[json!({
                "jobId": "job-1",
                "title": "Research",
                "status": 4,
                "createTime": 123,
            })],
        );
        assert_eq!(result["phase"], "arbitration_list");
        assert_eq!(result["payload"]["items"][0]["jobId"], "job-1");
        assert_eq!(result["payload"]["items"][0]["description"], "Research");
        assert_eq!(result["payload"]["items"][0]["occurredAt"], 123);
        assert_eq!(result["payload"]["items"][0]["taskStatus"], "disputed");
        assert!(result["payload"]["items"][0]["verdict"].is_null());
        assert_eq!(result["nextAction"][0]["id"], "view_arbitration");
        assert_eq!(
            result["nextAction"][0]["params"]["allowedJobIds"],
            json!(["job-1"])
        );
        assert_eq!(
            result["nextAction"][0]["params"]["confirmationRequired"],
            true
        );
    }

    #[test]
    fn arbitration_detail_keeps_unknown_backend_fields_null() {
        let result = build_detail_result(
            "job-1",
            &json!({"status": 4, "title": "Research", "disputeTime": 123}),
            None,
        );
        assert_eq!(result["phase"], "arbitration_detail");
        assert_eq!(result["payload"]["jobId"], "job-1");
        assert_eq!(result["payload"]["occurredAt"], 123);
        assert_eq!(result["payload"]["arbitrationPhase"], "unknown");
        assert!(result["payload"]["verdict"].is_null());
        assert!(result["payload"]["txHash"].is_null());
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
            let result = build_detail_result("job-1", &json!({}), Some(&arbitration));
            assert_eq!(result["payload"]["arbitrationPhase"], "resolved");
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
}
