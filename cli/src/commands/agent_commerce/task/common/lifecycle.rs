//! Pure A2A one-time-task lifecycle projection.
//!
//! This module deliberately performs no I/O. Callers fetch authoritative task
//! detail and XMTP history, then pass both inputs here. XMTP history is useful
//! for timestamps; [`Status`] remains the source of truth for the current phase.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use super::state_machine::Status;
use super::{network::task_api_client::TaskApiClient, AGENT_ROLE_USER};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryMessage {
    pub id: String,
    pub sender_inbox_id: Option<String>,
    pub content: Value,
    pub sent_at: Option<String>,
    pub delivery_status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleEventKind {
    Created,
    Accepted,
    Submitted,
    Completed,
    Rejected,
    Disputed,
    Closed,
    Expired,
    Refunded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleEvent {
    pub message_id: String,
    pub event_id: Option<String>,
    pub kind: LifecycleEventKind,
    pub occurred_at: Option<String>,
    pub sender_inbox_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Milestones {
    pub created_at: Option<String>,
    pub accepted_at: Option<String>,
    pub submitted_at: Option<String>,
    pub completed_at: Option<String>,
    pub rejected_at: Option<String>,
    pub disputed_at: Option<String>,
    pub closed_at: Option<String>,
    pub expired_at: Option<String>,
    pub refunded_at: Option<String>,
    pub failed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecyclePhase {
    Initializing,
    WaitingForAsp,
    AspExecuting,
    WaitingForUserReview,
    Rejected,
    Disputed,
    Completed,
    Closed,
    Expired,
    Refunded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleConfidence {
    Confirmed,
    Partial,
    Conflict,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleSnapshot {
    pub job_id: String,
    pub task_type: String,
    pub phase: LifecyclePhase,
    pub status_label: String,
    pub responsible_party: String,
    pub next_action: String,
    pub confidence: LifecycleConfidence,
    pub authoritative_status: String,
    pub status_source: String,
    pub history_available: bool,
    pub asp_agent_id: Option<String>,
    pub milestones: Milestones,
    pub events: Vec<LifecycleEvent>,
    pub synced_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskDetailProjection {
    status: Status,
    provider_agent_id: Option<String>,
}

/// CLI handler for the read-only User-side lifecycle query.
pub async fn handle_lifecycle(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> anyhow::Result<()> {
    let resolved_agent_id =
        super::query::resolve_agent_id_or_error(agent_id, AGENT_ROLE_USER).await?;
    let detail = super::query::fetch_task_detail(client, job_id, &resolved_agent_id).await?;
    let projected = project_one_time_detail(&detail)?;

    // A task may legitimately have no ASP yet. History is also an enrichment:
    // daemon/session failures must never hide the authoritative task status.
    let (messages, history_available) = match projected.provider_agent_id.as_deref() {
        Some(provider) => match super::okx_a2a::session_history(job_id, provider) {
            Ok(raw) => match parse_history(&raw) {
                Ok(messages) => (messages, true),
                Err(error) => {
                    eprintln!("[lifecycle] XMTP history JSON could not be parsed: {error}");
                    (Vec::new(), false)
                }
            },
            Err(error) => {
                eprintln!("[lifecycle] XMTP history unavailable: {error:#}");
                (Vec::new(), false)
            }
        },
        None => (Vec::new(), false),
    };
    let mut snapshot =
        build_snapshot_with_history_state(job_id, &projected.status, &messages, history_available);
    snapshot.asp_agent_id = projected.provider_agent_id;
    merge_detail_milestones(&mut snapshot.milestones, &detail);
    crate::output::success(snapshot);
    Ok(())
}

fn project_one_time_detail(detail: &Value) -> anyhow::Result<TaskDetailProjection> {
    let job_type = detail.get("jobType").and_then(parse_job_type);
    if job_type != Some(0) {
        anyhow::bail!(
            "agent lifecycle currently supports one-time tasks only; authoritative task detail returned jobType={}",
            detail.get("jobType").map(Value::to_string).unwrap_or_else(|| "missing".into())
        );
    }
    let status_code = detail
        .get("status")
        .and_then(scalar_i32)
        .ok_or_else(|| anyhow::anyhow!("authoritative task detail is missing a valid status"))?;
    let provider_agent_id = detail
        .get("providerAgentId")
        .and_then(scalar_string)
        .or_else(|| detail.get("aspAgentId").and_then(scalar_string));
    Ok(TaskDetailProjection {
        status: Status::from_int(status_code),
        provider_agent_id,
    })
}

fn parse_job_type(value: &Value) -> Option<i32> {
    match value {
        Value::String(value) if value.eq_ignore_ascii_case("one_time") => Some(0),
        _ => scalar_i32(value),
    }
}

fn scalar_i32(value: &Value) -> Option<i32> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| value.as_str()?.trim().parse::<i32>().ok())
}

/// Tolerantly parse the raw array emitted by `okx-a2a session history --json`.
/// Non-object rows and rows without an id are ignored. Missing optional fields
/// are preserved as `None`; content may be any JSON value.
pub(crate) fn parse_history(raw: &str) -> Result<Vec<HistoryMessage>, serde_json::Error> {
    let value: Value = serde_json::from_str(raw)?;
    let Some(rows) = value.as_array() else {
        return Ok(Vec::new());
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            let object = row.as_object()?;
            let id = scalar_string(object.get("id")?)?;
            Some(HistoryMessage {
                id,
                sender_inbox_id: object.get("senderInboxId").and_then(scalar_string),
                content: object.get("content").cloned().unwrap_or(Value::Null),
                sent_at: object.get("sentAt").and_then(scalar_string),
                delivery_status: object.get("deliveryStatus").and_then(scalar_string),
            })
        })
        .collect())
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

/// Recognize only structured official event envelopes. Natural-language text
/// containing words such as `job_submitted` is intentionally not classified.
pub(crate) fn events_from_history(
    job_id: &str,
    messages: &[HistoryMessage],
) -> Vec<LifecycleEvent> {
    let mut seen_messages = HashSet::new();
    let mut seen_events = HashSet::new();
    let mut events = Vec::new();
    for message in messages {
        if !seen_messages.insert(message.id.clone()) {
            continue;
        }
        let Some(envelope) = system_event_envelope(&message.content) else {
            continue;
        };
        if let Some(envelope_job) = envelope.get("jobId").and_then(scalar_string) {
            if envelope_job != job_id {
                continue;
            }
        }
        let Some(event_name) = envelope.get("event").and_then(Value::as_str) else {
            continue;
        };
        let Some(kind) = event_kind(event_name) else {
            continue;
        };
        let occurred_at = ["occurredAt", "eventTime", "timestamp", "createdAt"]
            .iter()
            .find_map(|key| envelope.get(*key).and_then(scalar_string))
            .or_else(|| message.sent_at.clone());
        let event_id = envelope.get("eventId").and_then(scalar_string);
        let logical_key = event_id.clone().unwrap_or_else(|| {
            format!(
                "{job_id}:{event_name}:{}",
                occurred_at.as_deref().unwrap_or("")
            )
        });
        if !seen_events.insert(logical_key) {
            continue;
        }
        events.push(LifecycleEvent {
            message_id: message.id.clone(),
            event_id,
            kind,
            occurred_at,
            sender_inbox_id: message.sender_inbox_id.clone(),
        });
    }
    events.sort_by(|left, right| left.occurred_at.cmp(&right.occurred_at));
    events
}

fn system_event_envelope(content: &Value) -> Option<Value> {
    let decoded = match content {
        Value::Object(_) => Some(content.clone()),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object),
        _ => None,
    }?;
    let envelope = decoded
        .get("message")
        .filter(|value| value.is_object())
        .unwrap_or(&decoded);
    (envelope.get("source").and_then(Value::as_str) == Some("system")).then(|| envelope.clone())
}

fn event_kind(name: &str) -> Option<LifecycleEventKind> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "job_created" => LifecycleEventKind::Created,
        "job_accepted" => LifecycleEventKind::Accepted,
        "job_submitted" => LifecycleEventKind::Submitted,
        "job_completed" => LifecycleEventKind::Completed,
        "job_rejected" => LifecycleEventKind::Rejected,
        "job_disputed" => LifecycleEventKind::Disputed,
        "job_closed" => LifecycleEventKind::Closed,
        "job_expired" | "job_asp_accept_expire" | "job_asp_reject_expire" => {
            LifecycleEventKind::Expired
        }
        "job_asp_reject_closed" => LifecycleEventKind::Closed,
        "job_refunded" => LifecycleEventKind::Refunded,
        "job_failed" => LifecycleEventKind::Failed,
        _ => return None,
    })
}

pub(crate) fn build_snapshot(
    job_id: &str,
    authoritative_status: &Status,
    messages: &[HistoryMessage],
) -> LifecycleSnapshot {
    build_snapshot_with_history_state(job_id, authoritative_status, messages, true)
}

pub(crate) fn build_snapshot_with_history_state(
    job_id: &str,
    authoritative_status: &Status,
    messages: &[HistoryMessage],
    history_available: bool,
) -> LifecycleSnapshot {
    let events = events_from_history(job_id, messages);
    let milestones = fold_milestones(&events);
    let phase = phase_from_status(authoritative_status);
    let confidence = if matches!(authoritative_status, Status::Other(_)) {
        LifecycleConfidence::Unknown
    } else if events_conflict_with_status(authoritative_status, &events) {
        LifecycleConfidence::Conflict
    } else if !history_available {
        LifecycleConfidence::Partial
    } else {
        LifecycleConfidence::Confirmed
    };
    LifecycleSnapshot {
        job_id: job_id.to_string(),
        task_type: "one_time".to_string(),
        phase,
        status_label: status_label(authoritative_status).to_string(),
        responsible_party: responsible_party(phase).to_string(),
        next_action: next_action(phase).to_string(),
        confidence,
        authoritative_status: authoritative_status.as_str().to_string(),
        status_source: "task_api".to_string(),
        history_available,
        asp_agent_id: None,
        milestones,
        events,
        synced_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Init => "Task initializing",
        Status::Created => "Waiting for ASP acceptance",
        Status::Accepted => "ASP executing",
        Status::Submitted => "Waiting for user review",
        Status::Rejected => "Deliverable rejected",
        Status::Disputed => "Evaluation in progress",
        Status::AdminStopped => "Stopped by platform",
        Status::Completed => "Task completed",
        Status::Close => "Task closed",
        Status::Expired => "Task expired",
        Status::Failed => "Refund completed",
        Status::Other(_) => "Status unavailable",
    }
}

fn responsible_party(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::Initializing => "official",
        LifecyclePhase::WaitingForAsp | LifecyclePhase::AspExecuting => "asp",
        LifecyclePhase::WaitingForUserReview | LifecyclePhase::Rejected => "user",
        LifecyclePhase::Disputed | LifecyclePhase::Unknown => "official",
        LifecyclePhase::Completed
        | LifecyclePhase::Closed
        | LifecyclePhase::Expired
        | LifecyclePhase::Refunded
        | LifecyclePhase::Failed => "none",
    }
}

fn next_action(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::Initializing => "Wait for task initialization",
        LifecyclePhase::WaitingForAsp => "Wait for the ASP to accept the task",
        LifecyclePhase::AspExecuting => "Wait for the ASP to submit the deliverable",
        LifecyclePhase::WaitingForUserReview => "Review the ASP deliverable",
        LifecyclePhase::Rejected => "Follow the refund or evaluation flow",
        LifecyclePhase::Disputed => "Wait for the evaluation result",
        LifecyclePhase::Completed
        | LifecyclePhase::Closed
        | LifecyclePhase::Expired
        | LifecyclePhase::Refunded
        | LifecyclePhase::Failed => "No further task action",
        LifecyclePhase::Unknown => "Refresh the authoritative task status",
    }
}

fn merge_detail_milestones(milestones: &mut Milestones, detail: &Value) {
    fill_from_detail(
        &mut milestones.created_at,
        detail,
        &["createdAt", "createTime"],
    );
    fill_from_detail(
        &mut milestones.accepted_at,
        detail,
        &["acceptedAt", "acceptTime"],
    );
    fill_from_detail(
        &mut milestones.submitted_at,
        detail,
        &["submittedAt", "submitTime"],
    );
    fill_from_detail(
        &mut milestones.completed_at,
        detail,
        &["completedAt", "completeTime"],
    );
}

fn fill_from_detail(slot: &mut Option<String>, detail: &Value, keys: &[&str]) {
    if slot.is_none() {
        *slot = keys
            .iter()
            .find_map(|key| detail.get(*key).and_then(scalar_string));
    }
}

fn fold_milestones(events: &[LifecycleEvent]) -> Milestones {
    let mut result = Milestones::default();
    for event in events {
        let slot = match event.kind {
            LifecycleEventKind::Created => &mut result.created_at,
            LifecycleEventKind::Accepted => &mut result.accepted_at,
            LifecycleEventKind::Submitted => &mut result.submitted_at,
            LifecycleEventKind::Completed => &mut result.completed_at,
            LifecycleEventKind::Rejected => &mut result.rejected_at,
            LifecycleEventKind::Disputed => &mut result.disputed_at,
            LifecycleEventKind::Closed => &mut result.closed_at,
            LifecycleEventKind::Expired => &mut result.expired_at,
            LifecycleEventKind::Refunded => &mut result.refunded_at,
            LifecycleEventKind::Failed => &mut result.failed_at,
        };
        if slot.is_none() {
            *slot = event.occurred_at.clone();
        }
    }
    result
}

fn phase_from_status(status: &Status) -> LifecyclePhase {
    match status {
        Status::Init => LifecyclePhase::Initializing,
        Status::Created => LifecyclePhase::WaitingForAsp,
        Status::Accepted => LifecyclePhase::AspExecuting,
        Status::Submitted => LifecyclePhase::WaitingForUserReview,
        Status::Rejected => LifecyclePhase::Rejected,
        Status::Disputed => LifecyclePhase::Disputed,
        Status::Completed => LifecyclePhase::Completed,
        Status::Close | Status::AdminStopped => LifecyclePhase::Closed,
        Status::Expired => LifecyclePhase::Expired,
        Status::Failed => LifecyclePhase::Refunded,
        Status::Other(_) => LifecyclePhase::Unknown,
    }
}

fn events_conflict_with_status(status: &Status, events: &[LifecycleEvent]) -> bool {
    let terminal = events.iter().rev().find(|event| {
        matches!(
            event.kind,
            LifecycleEventKind::Completed
                | LifecycleEventKind::Closed
                | LifecycleEventKind::Expired
                | LifecycleEventKind::Refunded
                | LifecycleEventKind::Failed
        )
    });
    match (status, terminal.map(|event| event.kind)) {
        (Status::Completed, Some(LifecycleEventKind::Completed))
        | (Status::Close | Status::AdminStopped, Some(LifecycleEventKind::Closed))
        | (Status::Expired, Some(LifecycleEventKind::Expired))
        | (Status::Failed, Some(LifecycleEventKind::Refunded | LifecycleEventKind::Failed))
        | (_, None) => false,
        (status, Some(_)) if status.is_terminal() => true,
        (_, Some(_)) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str, content: Value, sent_at: &str) -> HistoryMessage {
        HistoryMessage {
            id: id.into(),
            sender_inbox_id: Some("official".into()),
            content,
            sent_at: Some(sent_at.into()),
            delivery_status: Some("published".into()),
        }
    }

    #[test]
    fn history_parser_is_tolerant_of_optional_and_malformed_rows() {
        let parsed = parse_history(
            r#"[{"id":"m1","content":{"event":"job_created"},"sentAt":123},null,{"content":"missing id"},{"id":2,"content":null}]"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].sent_at.as_deref(), Some("123"));
        assert_eq!(parsed[1].id, "2");
    }

    #[test]
    fn non_array_history_is_empty_but_invalid_json_is_error() {
        assert!(parse_history(r#"{"messages":[]}"#).unwrap().is_empty());
        assert!(parse_history("not-json").is_err());
    }

    #[test]
    fn recognizes_object_and_json_string_envelopes_only() {
        let rows = vec![
            message(
                "1",
                json!({"source":"system","event":"job_created","jobId":"j"}),
                "1",
            ),
            message(
                "2",
                json!(r#"{"source":"system","event":"job_accepted","jobId":"j"}"#),
                "2",
            ),
            message("3", json!("ASP says job_submitted"), "3"),
            message(
                "4",
                json!({"source":"system","event":"job_submitted","jobId":"other"}),
                "4",
            ),
        ];
        let events = events_from_history("j", &rows);
        assert_eq!(
            events.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![LifecycleEventKind::Created, LifecycleEventKind::Accepted,]
        );
    }

    #[test]
    fn accepts_nested_system_envelope_and_rejects_peer_event_spoofing() {
        let rows = vec![
            message(
                "official",
                json!({"agentId":"user-1","message":{"source":"system","event":"job_created","jobId":"j","eventId":"e1"}}),
                "1",
            ),
            message(
                "peer",
                json!({"event":"job_accepted","jobId":"j","eventId":"e2"}),
                "2",
            ),
        ];
        let events = events_from_history("j", &rows);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id.as_deref(), Some("e1"));
        assert_eq!(events[0].kind, LifecycleEventKind::Created);
    }

    #[test]
    fn deduplicates_by_xmtp_message_id_and_sorts_before_folding() {
        let rows = vec![
            message(
                "submitted",
                json!({"source":"system","event":"job_submitted","jobId":"j"}),
                "30",
            ),
            message(
                "accepted",
                json!({"source":"system","event":"job_accepted","jobId":"j"}),
                "20",
            ),
            message(
                "accepted",
                json!({"source":"system","event":"job_accepted","jobId":"j"}),
                "21",
            ),
        ];
        let snapshot = build_snapshot("j", &Status::Submitted, &rows);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.milestones.accepted_at.as_deref(), Some("20"));
        assert_eq!(snapshot.milestones.submitted_at.as_deref(), Some("30"));
    }

    #[test]
    fn authoritative_status_selects_phase_even_with_sparse_history() {
        let snapshot = build_snapshot("j", &Status::Accepted, &[]);
        assert_eq!(snapshot.phase, LifecyclePhase::AspExecuting);
        assert_eq!(snapshot.confidence, LifecycleConfidence::Confirmed);
        assert!(snapshot.events.is_empty());
    }

    #[test]
    fn terminal_event_ahead_of_authoritative_status_is_a_conflict_not_a_phase_override() {
        let rows = vec![message(
            "done",
            json!({"source":"system","event":"job_completed","jobId":"j"}),
            "2026-09-09T10:00:00Z",
        )];
        let snapshot = build_snapshot("j", &Status::Accepted, &rows);
        assert_eq!(snapshot.phase, LifecyclePhase::AspExecuting);
        assert_eq!(snapshot.confidence, LifecycleConfidence::Conflict);
        assert_eq!(
            snapshot.milestones.completed_at.as_deref(),
            Some("2026-09-09T10:00:00Z")
        );
    }

    #[test]
    fn matching_terminal_status_and_event_are_confirmed() {
        let rows = vec![message(
            "done",
            json!({"source":"system","event":"job_refunded"}),
            "40",
        )];
        let snapshot = build_snapshot("j", &Status::Failed, &rows);
        assert_eq!(snapshot.phase, LifecyclePhase::Refunded);
        assert_eq!(snapshot.confidence, LifecycleConfidence::Confirmed);
        assert_eq!(snapshot.milestones.refunded_at.as_deref(), Some("40"));
    }

    #[test]
    fn unknown_authoritative_status_does_not_guess_from_messages() {
        let rows = vec![message(
            "accepted",
            json!({"source":"system","event":"job_accepted"}),
            "20",
        )];
        let snapshot = build_snapshot("j", &Status::Other("future".into()), &rows);
        assert_eq!(snapshot.phase, LifecyclePhase::Unknown);
        assert_eq!(snapshot.confidence, LifecycleConfidence::Unknown);
    }

    #[test]
    fn projects_numeric_and_named_one_time_detail() {
        for job_type in [json!(0), json!("0"), json!("one_time")] {
            let detail = json!({
                "jobType": job_type,
                "status": "1",
                "providerAgentId": 8415
            });
            let projected = project_one_time_detail(&detail).unwrap();
            assert_eq!(projected.status, Status::Accepted);
            assert_eq!(projected.provider_agent_id.as_deref(), Some("8415"));
        }
    }

    #[test]
    fn detail_projection_rejects_subscription_missing_type_and_missing_status() {
        assert!(project_one_time_detail(&json!({"jobType": 1, "status": 1})).is_err());
        assert!(project_one_time_detail(&json!({"status": 1})).is_err());
        assert!(project_one_time_detail(&json!({"jobType": 0})).is_err());
    }

    #[test]
    fn unavailable_history_keeps_authoritative_phase_but_marks_partial() {
        let snapshot = build_snapshot_with_history_state("j", &Status::Submitted, &[], false);
        assert_eq!(snapshot.phase, LifecyclePhase::WaitingForUserReview);
        assert_eq!(snapshot.confidence, LifecycleConfidence::Partial);
        assert_eq!(snapshot.status_source, "task_api");
        assert!(!snapshot.history_available);
    }
}
