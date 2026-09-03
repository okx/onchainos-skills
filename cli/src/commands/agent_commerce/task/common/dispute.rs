//! Structured contracts for ASP dispute interactions.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

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
        JOB_REJECTED => Some(("agree_refund", "dispute_raise")),
        SUB_USER_REJECT => Some(("sub_agree_refund", "sub_dispute")),
        _ => None,
    };
    let Some((refund, dispute)) = ids else {
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
            action_id: dispute.to_string(),
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
            "dispute_decision",
            "blocked",
            "missing_required_facts",
            Vec::new(),
            json!({"jobId": job_id, "missingFields": missing}),
        );
    }

    let choices = default_choices(source_event, job_id);
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
        "dispute_decision",
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
        "dispute_decision",
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
        "dispute_decision",
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
    let choices = match raw {
        Some(raw) => serde_json::from_str::<Vec<DecisionChoice>>(raw)
            .map_err(|e| format!("invalid --choices-json: {e}"))?,
        None => default_choices(source_event, job_id),
    };
    if !is_decision_source(source_event) {
        return Ok(choices);
    }
    let expected = default_choices(source_event, job_id);
    if choices.len() != 2
        || choices[0].key != "A"
        || choices[1].key != "B"
        || choices[0].action_id != expected[0].action_id
        || choices[1].action_id != expected[1].action_id
    {
        return Err(
            "dispute choices must map A/B to the source event's allowed actions".to_string(),
        );
    }
    Ok(choices)
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
    if !allowed_action(source_event, &choice.action_id) {
        return Err(ChoiceError::UnsupportedAction);
    }
    let mut params = choice.params.clone();
    if key == "B" {
        if let Some(reason) = dispute_reason(normalized) {
            params.insert("reason".to_string(), Value::String(reason));
        }
    }
    Ok(ResolvedChoice {
        action_id: choice.action_id.clone(),
        params,
    })
}

pub fn resolved_action(
    source_event: &str,
    action_id: &str,
    job_id: &str,
    params: Option<&Value>,
) -> Result<ResolvedChoice, ChoiceError> {
    if !allowed_action(source_event, action_id) {
        return Err(ChoiceError::UnsupportedAction);
    }
    let mut resolved_params = params
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    resolved_params
        .entry("jobId".to_string())
        .or_insert_with(|| Value::String(job_id.to_string()));
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

fn allowed_action(source_event: &str, action_id: &str) -> bool {
    matches!(
        (source_event, action_id),
        (JOB_REJECTED, "dispute_raise")
            | (JOB_REJECTED, "agree_refund")
            | (SUB_USER_REJECT, "sub_dispute")
            | (SUB_USER_REJECT, "sub_agree_refund")
    )
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

fn dispute_reason(reply: &str) -> Option<String> {
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
            assert_eq!(result["phase"], "dispute_decision");
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
        assert_eq!(selected.action_id, "dispute_raise");
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
            "dispute_raise"
        );
    }

    #[test]
    fn reversed_dispute_choices_are_rejected() {
        let reversed = serde_json::to_string(&vec![
            DecisionChoice {
                key: "A".to_string(),
                action_id: "dispute_raise".to_string(),
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
}
