//! A2A one-time-task lifecycle projection and its scoped local-history fallback.
//!
//! Fresh task detail remains authoritative for the current phase. Historical
//! system events only fill timestamps and exception context. The SQLite access
//! in this module is deliberately private, read-only, bound to one fixed
//! command-store location, and reachable only from `agent lifecycle`.

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use super::state_machine::Status;
use super::{network::task_api_client::TaskApiClient, AGENT_ROLE_USER};

const MAX_LOCAL_ROWS: i64 = 512;
const MAX_COMMAND_JSON_BYTES: i64 = 128 * 1_024;

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
    AspSelected,
    Created,
    Accepted,
    Submitted,
    Completed,
    Rejected,
    DisputeRequested,
    Disputed,
    DisputeResolved,
    ReviewExpired,
    RejectExpired,
    ProviderRejected,
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
    pub name: String,
    pub kind: LifecycleEventKind,
    pub occurred_at: Option<String>,
    pub deadline_at: Option<String>,
    pub authoritative_status: Option<String>,
    pub job_type: Option<i32>,
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
    pub dispute_requested_at: Option<String>,
    pub disputed_at: Option<String>,
    pub dispute_resolved_at: Option<String>,
    pub review_expired_at: Option<String>,
    pub reject_expired_at: Option<String>,
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
pub(crate) struct LifecycleDisplayNode {
    pub marker: String,
    pub key: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleDisplay {
    pub progress_step: u8,
    pub progress_total: u8,
    pub deliverable_available: bool,
    pub review_ready: bool,
    pub timeline: Vec<LifecycleDisplayNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub follow_up: Vec<LifecycleDisplayNode>,
    pub current_summary: String,
    pub handled_by: String,
    pub next: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
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
    pub history_read_succeeded: bool,
    pub history_event_count: usize,
    pub asp_agent_id: Option<String>,
    pub review_deadline_at: Option<String>,
    pub milestones: Milestones,
    pub events: Vec<LifecycleEvent>,
    pub display: LifecycleDisplay,
    pub synced_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskDetailProjection {
    job_type: Option<i32>,
    status: Option<Status>,
    provider_agent_id: Option<String>,
    token_amount: Option<String>,
    token_symbol: Option<String>,
}

#[derive(Default)]
struct LocalHistoryRead {
    messages: Vec<HistoryMessage>,
    read_succeeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReconciledCurrent {
    job_type: Option<i32>,
    status: Option<Status>,
    status_from_local: bool,
}

/// CLI handler for the read-only User-side lifecycle query.
pub async fn handle_lifecycle(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> anyhow::Result<()> {
    let wallet_agents = super::fetch_my_agents_by_role("user").await;
    let Some(resolved_agent_id) = select_current_wallet_user_agent(&wallet_agents, agent_id) else {
        crate::output::success(unavailable_snapshot(
            job_id,
            "Current wallet identity could not be confirmed, so local task history was not read.",
        ));
        return Ok(());
    };

    let detail = match super::query::fetch_task_detail(client, job_id, &resolved_agent_id).await {
        Ok(value) => value,
        Err(_) => {
            let local = read_scoped_local_history(job_id, &resolved_agent_id);
            let mut snapshot = snapshot_from_local_fallback(job_id, &local);
            snapshot.display.notice = Some(if snapshot.history_available {
                "Latest task details are unavailable; showing the latest verified local task record."
                    .to_string()
            } else {
                "Task details and verified local history are unavailable. Try again later."
                    .to_string()
            });
            crate::output::success(snapshot);
            return Ok(());
        }
    };

    let projected = project_task_detail(&detail);
    let local = read_scoped_local_history(job_id, &resolved_agent_id);
    let mut messages = local.messages;
    let mut history_read_succeeded = local.read_succeeded;
    if let Some(provider) = projected.provider_agent_id.as_deref() {
        if let Ok(raw) = super::okx_a2a::session_history(job_id, provider) {
            if let Ok(mut session_messages) = parse_history(&raw) {
                history_read_succeeded = true;
                messages.append(&mut session_messages);
            }
        }
    }
    let events = events_from_history(job_id, &messages);
    let reconciled = reconcile_current(&projected, &events);

    if reconciled.job_type != Some(0) {
        let task_type = match reconciled.job_type {
            Some(1) => "subscription",
            Some(_) => "unsupported",
            None => "unknown",
        };
        let mut snapshot = unavailable_snapshot(
            job_id,
            "The task type could not be confirmed as a one-time task.",
        );
        snapshot.task_type = task_type.to_string();
        snapshot.asp_agent_id = projected.provider_agent_id;
        crate::output::success(snapshot);
        return Ok(());
    }

    let Some(status) = reconciled.status.as_ref() else {
        let mut snapshot = unavailable_snapshot(
            job_id,
            "The latest task details do not include a usable current status.",
        );
        snapshot.task_type = "one_time".to_string();
        snapshot.asp_agent_id = projected.provider_agent_id;
        crate::output::success(snapshot);
        return Ok(());
    };

    let mut snapshot =
        build_snapshot_with_history_state(job_id, status, &messages, history_read_succeeded);
    if reconciled.status_from_local {
        snapshot.status_source = "local_official_event".to_string();
        snapshot.confidence = LifecycleConfidence::Partial;
    }
    snapshot.asp_agent_id = projected.provider_agent_id;
    merge_detail_milestones(&mut snapshot.milestones, &detail);
    snapshot.review_deadline_at = review_deadline(&detail, &snapshot.events, &snapshot.milestones)
        .map(|value| value.to_string());
    let deliverable_available = local_user_deliverable_exists(job_id);
    snapshot.display = build_display(
        snapshot.phase,
        &snapshot.milestones,
        snapshot.review_deadline_at.as_deref(),
        projected.token_amount.as_deref(),
        projected.token_symbol.as_deref(),
        &snapshot.events,
        deliverable_available,
        if snapshot.history_available {
            None
        } else {
            Some("Some historical times are unavailable; the current stage comes from the latest task details.")
        },
    );
    crate::output::success(snapshot);
    Ok(())
}

fn project_task_detail(detail: &Value) -> TaskDetailProjection {
    TaskDetailProjection {
        job_type: detail.get("jobType").and_then(parse_job_type),
        status: detail
            .get("status")
            .and_then(scalar_i32)
            .map(Status::from_int),
        provider_agent_id: detail
            .get("providerAgentId")
            .and_then(scalar_string)
            .or_else(|| detail.get("aspAgentId").and_then(scalar_string)),
        token_amount: ["refundAmount", "paymentTokenAmount", "tokenAmount"]
            .iter()
            .find_map(|key| detail.get(*key).and_then(scalar_string)),
        token_symbol: ["refundTokenSymbol", "paymentTokenSymbol", "tokenSymbol"]
            .iter()
            .find_map(|key| detail.get(*key).and_then(scalar_string)),
    }
}

fn select_current_wallet_user_agent(agents: &[Value], requested: &str) -> Option<String> {
    let requested = requested.trim();
    if !requested.is_empty() {
        return wallet_has_user_agent(agents, requested).then(|| requested.to_string());
    }
    agents.iter().find_map(|agent| {
        let id = agent.get("agentId").and_then(scalar_string)?;
        wallet_has_user_agent(std::slice::from_ref(agent), &id).then_some(id)
    })
}

fn wallet_has_user_agent(agents: &[Value], expected: &str) -> bool {
    agents.iter().any(|agent| {
        let id_matches = agent.get("agentId").and_then(scalar_string).as_deref() == Some(expected);
        let role_matches = agent.get("role").is_some_and(|role| {
            scalar_i64(role).is_some_and(|role| role == AGENT_ROLE_USER)
                || role
                    .as_str()
                    .is_some_and(|role| role.eq_ignore_ascii_case("user"))
        });
        id_matches && role_matches
    })
}

fn parse_job_type(value: &Value) -> Option<i32> {
    match value {
        Value::String(value) if value.eq_ignore_ascii_case("one_time") => Some(0),
        Value::String(value) if value.eq_ignore_ascii_case("subscription") => Some(1),
        _ => scalar_i32(value),
    }
}

fn reconcile_current(
    projected: &TaskDetailProjection,
    events: &[LifecycleEvent],
) -> ReconciledCurrent {
    let local_job_type = events.iter().rev().find_map(|event| event.job_type);
    let local_status = events.iter().rev().find_map(status_from_event);
    let status_from_local = projected.status.is_none() && local_status.is_some();
    ReconciledCurrent {
        job_type: projected.job_type.or(local_job_type),
        status: projected.status.clone().or(local_status),
        status_from_local,
    }
}

fn scalar_i32(value: &Value) -> Option<i32> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| value.as_str()?.trim().parse::<i32>().ok())
}

fn scalar_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

/// Tolerantly parse `okx-a2a session history --json` output.
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

/// Recognize only structured official events bound to the exact requested Job ID.
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
        if envelope.get("jobId").and_then(scalar_string).as_deref() != Some(job_id) {
            continue;
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
        let deadline_at = ["reviewDeadlineAt", "reviewWindowEndsAt", "expireTime"]
            .iter()
            .find_map(|key| envelope.get(*key).and_then(scalar_string));
        let event_id = envelope.get("eventId").and_then(scalar_string);
        let event_name = event_name.trim().to_ascii_lowercase();
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
            name: event_name,
            kind,
            occurred_at,
            deadline_at,
            authoritative_status: ["jobStatus", "taskStatus", "status"]
                .iter()
                .find_map(|key| envelope.get(*key).and_then(scalar_string)),
            job_type: envelope.get("jobType").and_then(parse_job_type),
            sender_inbox_id: message.sender_inbox_id.clone(),
        });
    }
    events.sort_by(|left, right| {
        timestamp_sort_key(left.occurred_at.as_deref())
            .cmp(&timestamp_sort_key(right.occurred_at.as_deref()))
            .then_with(|| left.message_id.cmp(&right.message_id))
    });
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
    envelope
        .get("source")
        .and_then(Value::as_str)
        .is_some_and(|source| source.eq_ignore_ascii_case("system"))
        .then(|| envelope.clone())
}

fn event_kind(name: &str) -> Option<LifecycleEventKind> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "job_asp_selected" => LifecycleEventKind::AspSelected,
        "job_created" => LifecycleEventKind::Created,
        "job_accepted" => LifecycleEventKind::Accepted,
        "job_submitted" => LifecycleEventKind::Submitted,
        "job_completed" | "job_auto_completed" => LifecycleEventKind::Completed,
        "job_rejected" => LifecycleEventKind::Rejected,
        "dispute_approved" => LifecycleEventKind::DisputeRequested,
        "job_disputed" => LifecycleEventKind::Disputed,
        "dispute_resolved" => LifecycleEventKind::DisputeResolved,
        "review_expired" => LifecycleEventKind::ReviewExpired,
        "reject_expired" => LifecycleEventKind::RejectExpired,
        "job_provider_reject" => LifecycleEventKind::ProviderRejected,
        "job_closed" | "job_asp_reject_closed" => LifecycleEventKind::Closed,
        "job_expired" | "job_asp_accept_expire" | "submit_expired" => LifecycleEventKind::Expired,
        "job_refunded" | "job_auto_refunded" | "job_asp_reject_expire" => {
            LifecycleEventKind::Refunded
        }
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
    history_read_succeeded: bool,
) -> LifecycleSnapshot {
    let events = events_from_history(job_id, messages);
    let milestones = fold_milestones(&events);
    let phase = phase_from_status(authoritative_status);
    let history_available = !events.is_empty();
    let confidence = if matches!(authoritative_status, Status::Other(_)) {
        LifecycleConfidence::Unknown
    } else if events_conflict_with_status(authoritative_status, &events) {
        LifecycleConfidence::Conflict
    } else if !history_available {
        LifecycleConfidence::Partial
    } else {
        LifecycleConfidence::Confirmed
    };
    let display = build_display(
        phase,
        &milestones,
        None,
        None,
        None,
        &events,
        false,
        (!history_available).then_some(
            "Some historical times are unavailable; the current stage comes from the latest task details.",
        ),
    );
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
        history_read_succeeded,
        history_event_count: events.len(),
        asp_agent_id: None,
        review_deadline_at: None,
        milestones,
        events,
        display,
        synced_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn unavailable_snapshot(job_id: &str, notice: &str) -> LifecycleSnapshot {
    let status = Status::Other("unavailable".to_string());
    let mut snapshot = build_snapshot_with_history_state(job_id, &status, &[], false);
    snapshot.task_type = "unknown".to_string();
    snapshot.status_source = "unavailable".to_string();
    snapshot.display.notice = Some(notice.to_string());
    snapshot
}

fn snapshot_from_local_fallback(job_id: &str, local: &LocalHistoryRead) -> LifecycleSnapshot {
    let events = events_from_history(job_id, &local.messages);
    let task_type = events.iter().rev().find_map(|event| event.job_type);
    let status = events.iter().rev().find_map(status_from_event);
    if task_type != Some(0) || status.is_none() {
        let mut snapshot =
            unavailable_snapshot(job_id, "Verified local task history is incomplete.");
        snapshot.history_read_succeeded = local.read_succeeded;
        snapshot.history_available = !events.is_empty();
        snapshot.history_event_count = events.len();
        snapshot.events = events;
        return snapshot;
    }

    let Some(status) = status else {
        return unavailable_snapshot(job_id, "Verified local task history is incomplete.");
    };
    let mut snapshot =
        build_snapshot_with_history_state(job_id, &status, &local.messages, local.read_succeeded);
    snapshot.status_source = "local_official_event".to_string();
    snapshot.confidence = LifecycleConfidence::Partial;
    snapshot.review_deadline_at =
        review_deadline(&Value::Null, &snapshot.events, &snapshot.milestones)
            .map(|value| value.to_string());
    snapshot.display = build_display(
        snapshot.phase,
        &snapshot.milestones,
        snapshot.review_deadline_at.as_deref(),
        None,
        None,
        &snapshot.events,
        local_user_deliverable_exists(job_id),
        Some("Latest task details are unavailable; showing the latest verified local task record."),
    );
    snapshot
}

fn status_from_event(event: &LifecycleEvent) -> Option<Status> {
    if let Some(raw) = event.authoritative_status.as_deref() {
        if let Ok(code) = raw.parse::<i32>() {
            return Some(Status::from_int(code));
        }
        let parsed = Status::parse(&raw.trim().to_ascii_lowercase());
        if !matches!(parsed, Status::Other(_)) {
            return Some(parsed);
        }
    }
    Some(match event.kind {
        LifecycleEventKind::AspSelected
        | LifecycleEventKind::Created
        | LifecycleEventKind::ProviderRejected => Status::Created,
        LifecycleEventKind::Accepted => Status::Accepted,
        LifecycleEventKind::Submitted | LifecycleEventKind::ReviewExpired => Status::Submitted,
        LifecycleEventKind::Rejected
        | LifecycleEventKind::DisputeRequested
        | LifecycleEventKind::RejectExpired => Status::Rejected,
        LifecycleEventKind::Disputed => Status::Disputed,
        LifecycleEventKind::Completed => Status::Completed,
        LifecycleEventKind::Closed => Status::Close,
        LifecycleEventKind::Expired => Status::Expired,
        LifecycleEventKind::Refunded | LifecycleEventKind::Failed => Status::Failed,
        LifecycleEventKind::DisputeResolved => return None,
    })
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Init => "Task initializing",
        Status::Created => "Waiting for ASP acceptance",
        Status::Accepted => "ASP executing",
        Status::Submitted => "Waiting for user review",
        Status::Rejected => "Deliverable rejected",
        Status::Disputed => "Platform review in progress",
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
        LifecyclePhase::Rejected => "Wait for the ASP response or platform review",
        LifecyclePhase::Disputed => "Wait for the platform review result",
        LifecyclePhase::Completed
        | LifecyclePhase::Closed
        | LifecyclePhase::Expired
        | LifecyclePhase::Refunded
        | LifecyclePhase::Failed => "No further task action",
        LifecyclePhase::Unknown => "Try the task query again later",
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
    fill_from_detail(
        &mut milestones.rejected_at,
        detail,
        &["rejectedAt", "rejectTime"],
    );
    fill_from_detail(
        &mut milestones.disputed_at,
        detail,
        &["disputedAt", "disputeTime"],
    );
    fill_from_detail(
        &mut milestones.dispute_resolved_at,
        detail,
        &["disputeResolvedAt", "arbitrationCompletedAt"],
    );
    fill_from_detail(
        &mut milestones.closed_at,
        detail,
        &["closedAt", "closeTime"],
    );
    fill_from_detail(&mut milestones.expired_at, detail, &["expiredAt"]);
    fill_from_detail(
        &mut milestones.refunded_at,
        detail,
        &["refundedAt", "refundTime"],
    );
    fill_from_detail(
        &mut milestones.failed_at,
        detail,
        &["failedAt", "failureTime"],
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
            LifecycleEventKind::AspSelected | LifecycleEventKind::ProviderRejected => continue,
            LifecycleEventKind::Created => &mut result.created_at,
            LifecycleEventKind::Accepted => &mut result.accepted_at,
            LifecycleEventKind::Submitted => &mut result.submitted_at,
            LifecycleEventKind::Completed => &mut result.completed_at,
            LifecycleEventKind::Rejected => &mut result.rejected_at,
            LifecycleEventKind::DisputeRequested => &mut result.dispute_requested_at,
            LifecycleEventKind::Disputed => &mut result.disputed_at,
            LifecycleEventKind::DisputeResolved => &mut result.dispute_resolved_at,
            LifecycleEventKind::ReviewExpired => &mut result.review_expired_at,
            LifecycleEventKind::RejectExpired => &mut result.reject_expired_at,
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
    let terminal = events.iter().rev().find_map(|event| match event.kind {
        LifecycleEventKind::Completed => Some(Status::Completed),
        LifecycleEventKind::Closed => Some(Status::Close),
        LifecycleEventKind::Expired => Some(Status::Expired),
        LifecycleEventKind::Refunded | LifecycleEventKind::Failed => Some(Status::Failed),
        LifecycleEventKind::DisputeResolved => status_from_event(event),
        _ => None,
    });
    match terminal {
        None => false,
        Some(_) if !status.is_terminal() => true,
        Some(local) => {
            local != *status
                && !(matches!(status, Status::AdminStopped) && matches!(local, Status::Close))
        }
    }
}

fn review_deadline(
    detail: &Value,
    events: &[LifecycleEvent],
    milestones: &Milestones,
) -> Option<i64> {
    let exact = super::deadline::first_timestamp(
        detail,
        &["reviewDeadlineAt", "reviewWindowEndsAt", "expireTime"],
    );
    if exact.is_some() {
        return exact;
    }
    let event_deadline = events
        .iter()
        .rev()
        .find(|event| event.kind == LifecycleEventKind::Submitted)
        .and_then(|event| event.deadline_at.as_deref())
        .and_then(super::deadline::parse_timestamp_seconds);
    if event_deadline.is_some() {
        return event_deadline;
    }
    milestones
        .submitted_at
        .as_deref()
        .and_then(super::deadline::parse_timestamp_seconds)
        .and_then(|submitted| {
            submitted.checked_add(super::deadline::REVIEW_WINDOW_SECONDS)
        })
}

fn timestamp_sort_key(value: Option<&str>) -> (bool, i64, &str) {
    match value.and_then(super::deadline::parse_timestamp_seconds) {
        Some(timestamp) => (false, timestamp, ""),
        None => (true, i64::MAX, value.unwrap_or("")),
    }
}

fn format_timestamp(value: Option<&str>) -> Option<String> {
    value
        .and_then(super::deadline::parse_timestamp_seconds)
        .and_then(super::deadline::format_local_timestamp_with_offset)
}

fn time_or_unavailable(value: Option<&str>) -> String {
    format_timestamp(value).unwrap_or_else(|| "Time unavailable".to_string())
}

fn time_range(start: Option<&str>, end: Option<&str>) -> String {
    match (format_timestamp(start), format_timestamp(end)) {
        (Some(start), Some(end)) => format!("{start} - {end}"),
        (Some(start), None) => format!("{start} started; end time unavailable"),
        (None, Some(end)) => format!("Completed at {end}; start time unavailable"),
        (None, None) => "Time unavailable".to_string(),
    }
}

fn node(marker: &str, key: &str, title: &str, detail: Option<String>) -> LifecycleDisplayNode {
    LifecycleDisplayNode {
        marker: marker.to_string(),
        key: key.to_string(),
        title: title.to_string(),
        detail,
    }
}

fn pending_node(key: &str, title: &str) -> LifecycleDisplayNode {
    let detail = if key == "completed" {
        "Not completed"
    } else {
        "Not started"
    };
    node("○", key, title, Some(detail.to_string()))
}

fn pending_review_node(deliverable_available: bool) -> LifecycleDisplayNode {
    if deliverable_available {
        node(
            "○",
            "user_review",
            "Deliverable review",
            Some("Deliverable received; waiting for the task status update".to_string()),
        )
    } else {
        pending_node("user_review", "Deliverable review")
    }
}

fn local_user_deliverable_exists(job_id: &str) -> bool {
    if !safe_lookup_key(job_id) {
        return false;
    }
    let Ok(Some(manifest)) = super::deliverables::read_manifest("user", job_id) else {
        return false;
    };
    if manifest.job_id != job_id || manifest.role != "user" {
        return false;
    }
    let Some(entry) = manifest.entries.last() else {
        return false;
    };
    let mut components = Path::new(&entry.filename).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return false;
    }
    let Ok(directory) = super::deliverables::deliverables_dir("user", job_id) else {
        return false;
    };
    fs::symlink_metadata(directory.join(&entry.filename))
        .is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Build the display-ready five-stage timeline returned by `create-task`.
///
/// At this point the creation transaction has been submitted, but the
/// authoritative `job_created` event may not have arrived yet. Keep the first
/// stage active and describe only that confirmed local fact; later lifecycle
/// queries replace it with backend and scoped-history timestamps.
pub(crate) fn initial_creation_display() -> LifecycleDisplay {
    let milestones = Milestones::default();
    let mut display = build_display(
        LifecyclePhase::Initializing,
        &milestones,
        None,
        None,
        None,
        &[],
        false,
        None,
    );
    if let Some(first) = display.timeline.first_mut() {
        first.detail = Some("Creation submitted; waiting for task confirmation".to_string());
    }
    display
}

#[allow(clippy::too_many_arguments)]
fn build_display(
    phase: LifecyclePhase,
    milestones: &Milestones,
    review_deadline_at: Option<&str>,
    token_amount: Option<&str>,
    token_symbol: Option<&str>,
    events: &[LifecycleEvent],
    deliverable_available: bool,
    notice: Option<&str>,
) -> LifecycleDisplay {
    let created = || {
        node(
            "✓",
            "created",
            "Task created",
            Some(time_or_unavailable(milestones.created_at.as_deref())),
        )
    };
    let accepted = || {
        node(
            "✓",
            "accepted",
            "ASP accepted",
            Some(time_or_unavailable(milestones.accepted_at.as_deref())),
        )
    };
    let executed = || {
        node(
            "✓",
            "asp_execution",
            "ASP executed",
            Some(time_range(
                milestones.accepted_at.as_deref(),
                milestones.submitted_at.as_deref(),
            )),
        )
    };
    let has_rejection = milestones.rejected_at.is_some()
        || events
            .iter()
            .any(|event| event.kind == LifecycleEventKind::Rejected);
    let evaluation_resolved = milestones.dispute_resolved_at.is_some();

    let (progress_step, timeline, follow_up, current_summary, handled_by, next) = match phase {
        LifecyclePhase::Initializing => (
            1,
            vec![
                node(
                    "▶",
                    "created",
                    "Creating task",
                    Some("Start time unavailable".to_string()),
                ),
                pending_node("accepted", "ASP acceptance"),
                pending_node("asp_execution", "ASP execution"),
                pending_review_node(deliverable_available),
                pending_node("completed", "Task completion"),
            ],
            Vec::new(),
            "Creating task",
            "Platform",
            "Wait for task creation",
        ),
        LifecyclePhase::WaitingForAsp => (
            2,
            vec![
                created(),
                node(
                    "▶",
                    "accepted",
                    "Waiting for ASP acceptance",
                    Some(
                        format_timestamp(milestones.created_at.as_deref())
                            .map(|time| format!("{time} started"))
                            .unwrap_or_else(|| "Start time unavailable".to_string()),
                    ),
                ),
                pending_node("asp_execution", "ASP execution"),
                pending_review_node(deliverable_available),
                pending_node("completed", "Task completion"),
            ],
            Vec::new(),
            "Waiting for ASP acceptance",
            "ASP",
            "Wait for the ASP to accept",
        ),
        LifecyclePhase::AspExecuting => (
            3,
            vec![
                created(),
                accepted(),
                node(
                    "▶",
                    "asp_execution",
                    "ASP executing",
                    Some(
                        format_timestamp(milestones.accepted_at.as_deref())
                            .map(|time| format!("{time} started"))
                            .unwrap_or_else(|| "Start time unavailable".to_string()),
                    ),
                ),
                pending_review_node(deliverable_available),
                pending_node("completed", "Task completion"),
            ],
            Vec::new(),
            "ASP executing",
            "ASP",
            "Wait for the ASP to submit",
        ),
        LifecyclePhase::WaitingForUserReview => {
            let review_expired = milestones.review_expired_at.is_some();
            let review_ready = !review_expired && deliverable_available;
            let review_detail = if review_expired {
                time_or_unavailable(milestones.review_expired_at.as_deref())
            } else if !deliverable_available {
                format_timestamp(milestones.submitted_at.as_deref())
                    .map(|time| format!("{time} started; waiting for the deliverable"))
                    .unwrap_or_else(|| "Waiting for the deliverable".to_string())
            } else {
                match (
                    format_timestamp(milestones.submitted_at.as_deref()),
                    format_timestamp(review_deadline_at),
                ) {
                    (Some(start), Some(deadline)) => {
                        format!("{start} started; accept or reject by {deadline}")
                    }
                    (Some(start), None) => {
                        format!("{start} started; accept or reject; deadline unavailable")
                    }
                    (None, Some(deadline)) => format!("Accept or reject by {deadline}"),
                    (None, None) => {
                        "Accept or reject the deliverable; timing unavailable".to_string()
                    }
                }
            };
            (
                4,
                vec![
                    created(),
                    accepted(),
                    executed(),
                    node(
                        if review_expired { "—" } else { "▶" },
                        "user_review",
                        if review_expired {
                            "Deliverable review period ended"
                        } else {
                            "Waiting for you to review the deliverable"
                        },
                        Some(review_detail),
                    ),
                    pending_node("completed", "Task completion"),
                ],
                Vec::new(),
                if review_expired {
                    "Waiting for task completion"
                } else if !review_ready {
                    "Waiting for the deliverable"
                } else {
                    "Waiting for you to review the deliverable"
                },
                if review_expired || !review_ready {
                    "ASP"
                } else {
                    "You"
                },
                if review_expired {
                    "Wait for the final result"
                } else if !review_ready {
                    "Wait for the deliverable to arrive"
                } else {
                    "Accept or reject the deliverable"
                },
            )
        }
        LifecyclePhase::Rejected | LifecyclePhase::Disputed => {
            let rejected = node(
                "✓",
                "user_review",
                "Deliverable rejected",
                Some(time_or_unavailable(milestones.rejected_at.as_deref())),
            );
            let follow = if evaluation_resolved {
                node(
                    "✓",
                    "platform_review",
                    "Platform review completed",
                    Some(time_range(
                        milestones
                            .disputed_at
                            .as_deref()
                            .or(milestones.dispute_requested_at.as_deref()),
                        milestones.dispute_resolved_at.as_deref(),
                    )),
                )
            } else if phase == LifecyclePhase::Disputed {
                node(
                    "▶",
                    "platform_review",
                    "Platform review in progress",
                    Some(
                        format_timestamp(milestones.disputed_at.as_deref())
                            .map(|time| format!("{time} started"))
                            .unwrap_or_else(|| "Start time unavailable".to_string()),
                    ),
                )
            } else if milestones.dispute_requested_at.is_some() {
                node(
                    "▶",
                    "platform_review",
                    "Waiting for platform review",
                    Some(
                        format_timestamp(milestones.dispute_requested_at.as_deref())
                            .map(|time| format!("{time} requested"))
                            .unwrap_or_else(|| "Request time unavailable".to_string()),
                    ),
                )
            } else if milestones.reject_expired_at.is_some() {
                node(
                    "▶",
                    "refund_pending",
                    "Waiting for refund",
                    Some(time_or_unavailable(milestones.reject_expired_at.as_deref())),
                )
            } else {
                node(
                    "▶",
                    "asp_response",
                    "Waiting for ASP response",
                    Some(
                        format_timestamp(milestones.rejected_at.as_deref())
                            .map(|time| format!("{time} started"))
                            .unwrap_or_else(|| "Start time unavailable".to_string()),
                    ),
                )
            };
            (
                4,
                vec![
                    created(),
                    accepted(),
                    executed(),
                    rejected,
                    pending_node("completed", "Task completion"),
                ],
                vec![follow],
                if evaluation_resolved {
                    "Platform review completed"
                } else if phase == LifecyclePhase::Disputed {
                    "Platform review in progress"
                } else {
                    "Waiting for the next response"
                },
                if evaluation_resolved || phase == LifecyclePhase::Disputed {
                    "Platform"
                } else {
                    "ASP"
                },
                if evaluation_resolved {
                    "Wait for the final task result"
                } else {
                    "Wait for the result"
                },
            )
        }
        LifecyclePhase::Completed => {
            let review_node = if has_rejection {
                node(
                    "✓",
                    "user_review",
                    "Deliverable rejected",
                    Some(time_or_unavailable(milestones.rejected_at.as_deref())),
                )
            } else {
                node(
                    "✓",
                    "user_review",
                    "Deliverable confirmed",
                    Some(time_range(
                        milestones.submitted_at.as_deref(),
                        milestones.completed_at.as_deref(),
                    )),
                )
            };
            let follow = evaluation_resolved.then(|| {
                node(
                    "✓",
                    "platform_review",
                    "Platform review completed",
                    Some(format!(
                        "{}; result supports the ASP",
                        time_range(
                            milestones
                                .disputed_at
                                .as_deref()
                                .or(milestones.dispute_requested_at.as_deref()),
                            milestones.dispute_resolved_at.as_deref()
                        )
                    )),
                )
            });
            (
                5,
                vec![
                    created(),
                    accepted(),
                    executed(),
                    review_node,
                    node(
                        "✓",
                        "completed",
                        "Task completed",
                        Some(time_or_unavailable(milestones.completed_at.as_deref())),
                    ),
                ],
                follow.into_iter().collect(),
                "Task completed",
                "No action needed",
                "No further action",
            )
        }
        LifecyclePhase::Refunded | LifecyclePhase::Failed => {
            let review_node = if has_rejection {
                node(
                    "✓",
                    "user_review",
                    "Deliverable rejected",
                    Some(time_or_unavailable(milestones.rejected_at.as_deref())),
                )
            } else if milestones.submitted_at.is_some() {
                node(
                    "—",
                    "user_review",
                    "Deliverable review ended",
                    Some("The task ended before deliverable review".to_string()),
                )
            } else {
                pending_node("user_review", "Deliverable review")
            };
            let mut follow = Vec::new();
            if evaluation_resolved {
                follow.push(node(
                    "✓",
                    "platform_review",
                    "Platform review completed",
                    Some(format!(
                        "{}; result supports you",
                        time_range(
                            milestones
                                .disputed_at
                                .as_deref()
                                .or(milestones.dispute_requested_at.as_deref()),
                            milestones.dispute_resolved_at.as_deref()
                        )
                    )),
                ));
            }
            let refund_time = milestones
                .refunded_at
                .as_deref()
                .or(milestones.failed_at.as_deref());
            let amount = match (token_amount, token_symbol) {
                (Some(amount), Some(symbol)) if !amount.is_empty() && !symbol.is_empty() => {
                    Some(format!("{amount} {symbol} returned"))
                }
                (Some(amount), _) if !amount.is_empty() => Some(format!("{amount} returned")),
                _ => None,
            };
            let refund_detail = match (format_timestamp(refund_time), amount) {
                (Some(time), Some(amount)) => Some(format!("{time}; {amount}")),
                (Some(time), None) => Some(time),
                (None, Some(amount)) => Some(amount),
                (None, None) => None,
            };
            follow.push(node("✓", "refund", "Refund completed", refund_detail));
            let accepted_node = if milestones.accepted_at.is_some() {
                accepted()
            } else {
                pending_node("accepted", "ASP acceptance")
            };
            let execution_node = if milestones.submitted_at.is_some() {
                executed()
            } else if milestones.accepted_at.is_some() {
                node(
                    "—",
                    "asp_execution",
                    "ASP execution ended",
                    Some(time_range(
                        milestones.accepted_at.as_deref(),
                        refund_time,
                    )),
                )
            } else {
                pending_node("asp_execution", "ASP execution")
            };
            let progress_step = if has_rejection || milestones.submitted_at.is_some() {
                4
            } else if milestones.accepted_at.is_some() {
                3
            } else {
                2
            };
            (
                progress_step,
                vec![
                    created(),
                    accepted_node,
                    execution_node,
                    review_node,
                    node(
                        "—",
                        "completed",
                        "Task not completed",
                        Some("Ended before normal completion".to_string()),
                    ),
                ],
                follow,
                "Refund completed",
                "No action needed",
                "No further action",
            )
        }
        LifecyclePhase::Expired | LifecyclePhase::Closed => {
            let last_event = events.last();
            let has_submission = milestones.submitted_at.is_some();
            let execution_started = last_event.is_some_and(|event| event.name == "submit_expired")
                || milestones.accepted_at.is_some();
            let stage_three = if has_submission {
                executed()
            } else if execution_started {
                node(
                    "—",
                    "asp_execution",
                    if phase == LifecyclePhase::Expired {
                        "ASP submission timed out"
                    } else {
                        "ASP execution ended"
                    },
                    Some(time_or_unavailable(
                        milestones
                            .expired_at
                            .as_deref()
                            .or(milestones.closed_at.as_deref()),
                    )),
                )
            } else {
                pending_node("asp_execution", "ASP execution")
            };
            let stage_two = if milestones.accepted_at.is_some() {
                accepted()
            } else if execution_started {
                accepted()
            } else if phase == LifecyclePhase::Expired {
                node(
                    "—",
                    "accepted",
                    "ASP acceptance timed out",
                    Some(time_or_unavailable(milestones.expired_at.as_deref())),
                )
            } else {
                pending_node("accepted", "ASP acceptance")
            };
            let event_time = if phase == LifecyclePhase::Expired {
                milestones.expired_at.as_deref()
            } else {
                milestones.closed_at.as_deref()
            };
            let stage_four = if has_submission {
                node(
                    "—",
                    "user_review",
                    "Deliverable review period ended",
                    Some(time_or_unavailable(event_time)),
                )
            } else {
                pending_node("user_review", "Deliverable review")
            };
            let follow = vec![node(
                "✓",
                if phase == LifecyclePhase::Expired {
                    "expired"
                } else {
                    "closed"
                },
                if phase == LifecyclePhase::Expired {
                    "Task expired"
                } else {
                    "Task closed"
                },
                format_timestamp(event_time),
            )];
            (
                if has_submission {
                    4
                } else if execution_started {
                    3
                } else {
                    2
                },
                vec![
                    created(),
                    stage_two,
                    stage_three,
                    stage_four,
                    pending_node("completed", "Task completion"),
                ],
                follow,
                if phase == LifecyclePhase::Expired {
                    "Task expired"
                } else {
                    "Task closed"
                },
                "No action needed",
                "No further action",
            )
        }
        LifecyclePhase::Unknown => (
            1,
            vec![
                pending_node("created", "Task creation"),
                pending_node("accepted", "ASP acceptance"),
                pending_node("asp_execution", "ASP execution"),
                pending_node("user_review", "Deliverable review"),
                pending_node("completed", "Task completion"),
            ],
            Vec::new(),
            "Task status unavailable",
            "Unknown",
            "Try again later",
        ),
    };

    LifecycleDisplay {
        progress_step,
        progress_total: 5,
        deliverable_available,
        review_ready: phase == LifecyclePhase::WaitingForUserReview
            && deliverable_available
            && milestones.review_expired_at.is_none(),
        timeline,
        follow_up,
        current_summary: current_summary.to_string(),
        handled_by: handled_by.to_string(),
        next: next.to_string(),
        notice: notice.map(ToOwned::to_owned),
    }
}

/// Return the only production SQLite path this command is allowed to inspect.
fn fixed_command_store_path() -> Option<(PathBuf, PathBuf)> {
    let root = dirs::home_dir()?.join(".okx-agent-task");
    let database = root.join("sqlite").join("command-store.sqlite");
    Some((root, database))
}

fn read_scoped_local_history(job_id: &str, user_agent_id: &str) -> LocalHistoryRead {
    if !safe_lookup_key(job_id) || !safe_lookup_key(user_agent_id) {
        return LocalHistoryRead::default();
    }
    let Some((root, database)) = fixed_command_store_path() else {
        return LocalHistoryRead::default();
    };
    read_scoped_local_history_at(&root, &database, job_id, user_agent_id)
}

fn safe_lookup_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
}

fn read_scoped_local_history_at(
    allowed_root: &Path,
    database: &Path,
    job_id: &str,
    user_agent_id: &str,
) -> LocalHistoryRead {
    let Some(database) = validated_database_path(allowed_root, database) else {
        return LocalHistoryRead::default();
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let Ok(connection) = Connection::open_with_flags(database, flags) else {
        return LocalHistoryRead::default();
    };
    let _ = connection.busy_timeout(Duration::from_millis(100));
    let Ok(mut statement) = connection.prepare(
        "SELECT id, command_json, created_at_ms \
         FROM command_queue \
         WHERE type = 'ai-dispatch' \
           AND length(command_json) <= ?3 \
           AND instr(command_json, ?1) > 0 \
         ORDER BY created_at_ms ASC LIMIT ?2",
    ) else {
        return LocalHistoryRead::default();
    };
    let Ok(rows) = statement.query_map((job_id, MAX_LOCAL_ROWS, MAX_COMMAND_JSON_BYTES), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    }) else {
        return LocalHistoryRead::default();
    };

    let mut messages = Vec::new();
    for row in rows.flatten() {
        let (row_id, command_json, created_at_ms) = row;
        let Ok(command) = serde_json::from_str::<Value>(&command_json) else {
            continue;
        };
        let content = command.get("content").cloned().unwrap_or(Value::Null);
        let decoded_content = decode_object(&content);
        if !local_row_belongs_to_user(&command, decoded_content.as_ref(), user_agent_id) {
            continue;
        }
        let Some(envelope) = decoded_content
            .as_ref()
            .and_then(system_event_envelope)
            .or_else(|| system_event_envelope(&content))
        else {
            continue;
        };
        if envelope.get("jobId").and_then(scalar_string).as_deref() != Some(job_id) {
            continue;
        }
        let id = command
            .get("messageId")
            .and_then(scalar_string)
            .unwrap_or(row_id);
        let sent_at = command
            .get("createdAt")
            .and_then(scalar_string)
            .or_else(|| created_at_ms.map(|value| value.to_string()));
        messages.push(HistoryMessage {
            id,
            sender_inbox_id: None,
            content,
            sent_at,
            delivery_status: Some("local".to_string()),
        });
    }
    LocalHistoryRead {
        messages,
        read_succeeded: true,
    }
}

fn validated_database_path(allowed_root: &Path, database: &Path) -> Option<PathBuf> {
    let sqlite_dir = allowed_root.join("sqlite");
    for path in [allowed_root, sqlite_dir.as_path(), database] {
        let metadata = fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
    }
    if !fs::metadata(database).ok()?.is_file() {
        return None;
    }
    let canonical_root = fs::canonicalize(allowed_root).ok()?;
    let canonical_database = fs::canonicalize(database).ok()?;
    canonical_database
        .starts_with(&canonical_root)
        .then_some(canonical_database)
}

fn decode_object(value: &Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value.clone()),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object),
        _ => None,
    }
}

fn local_row_belongs_to_user(command: &Value, content: Option<&Value>, expected: &str) -> bool {
    let client_id = command
        .get("clientAgentId")
        .and_then(scalar_string)
        .or_else(|| content?.get("clientAgentId").and_then(scalar_string));
    if let Some(client_id) = client_id {
        return client_id == expected;
    }
    command
        .get("agentId")
        .and_then(scalar_string)
        .or_else(|| content?.get("agentId").and_then(scalar_string))
        .is_some_and(|agent_id| agent_id == expected)
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

    fn test_tempdir() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp");
        fs::create_dir_all(&root).unwrap();
        tempfile::Builder::new()
            .prefix("lifecycle-")
            .tempdir_in(root)
            .unwrap()
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
    fn structured_events_require_official_source_and_exact_job_id() {
        let rows = vec![
            message(
                "1",
                json!({"source":"system","event":"job_created","jobId":"j"}),
                "1",
            ),
            message("2", json!({"source":"system","event":"job_accepted"}), "2"),
            message(
                "3",
                json!({"source":"peer","event":"job_accepted","jobId":"j"}),
                "3",
            ),
            message("4", json!("ASP says job_submitted"), "4"),
        ];
        let events = events_from_history("j", &rows);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, LifecycleEventKind::Created);
    }

    #[test]
    fn deduplicates_and_sorts_numeric_timestamps_before_folding() {
        let rows = vec![
            message(
                "submitted",
                json!({"source":"system","event":"job_submitted","jobId":"j","timestamp":30}),
                "30",
            ),
            message(
                "accepted",
                json!({"source":"system","event":"job_accepted","jobId":"j","timestamp":9}),
                "9",
            ),
            message(
                "accepted",
                json!({"source":"system","event":"job_accepted","jobId":"j","timestamp":10}),
                "10",
            ),
        ];
        let snapshot = build_snapshot("j", &Status::Submitted, &rows);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.milestones.accepted_at.as_deref(), Some("9"));
        assert_eq!(snapshot.milestones.submitted_at.as_deref(), Some("30"));
    }

    #[test]
    fn empty_history_is_not_reported_as_available() {
        let snapshot = build_snapshot("j", &Status::Accepted, &[]);
        assert_eq!(snapshot.phase, LifecyclePhase::AspExecuting);
        assert_eq!(snapshot.confidence, LifecycleConfidence::Partial);
        assert!(!snapshot.history_available);
        assert!(snapshot.history_read_succeeded);
        assert_eq!(snapshot.history_event_count, 0);
    }

    #[test]
    fn creation_result_has_a_display_ready_initial_five_stage_timeline() {
        let display = initial_creation_display();
        assert_eq!(display.progress_step, 1);
        assert_eq!(display.progress_total, 5);
        assert_eq!(display.timeline.len(), 5);
        assert_eq!(display.timeline[0].marker, "▶");
        assert_eq!(display.timeline[0].title, "Creating task");
        assert_eq!(
            display.timeline[0].detail.as_deref(),
            Some("Creation submitted; waiting for task confirmation")
        );
        assert_eq!(display.timeline[1].detail.as_deref(), Some("Not started"));
        assert_eq!(display.timeline[3].title, "Deliverable review");
        assert_eq!(display.timeline[4].detail.as_deref(), Some("Not completed"));
        assert!(!display.deliverable_available);
        assert!(!display.review_ready);
        assert!(display.notice.is_none());
    }

    #[test]
    fn reject_expiry_is_refund_not_generic_expiry() {
        let rows = vec![message(
            "refund",
            json!({"source":"system","event":"job_asp_reject_expire","jobId":"j","jobStatus":"failed"}),
            "40",
        )];
        let snapshot = build_snapshot("j", &Status::Failed, &rows);
        assert_eq!(snapshot.events[0].kind, LifecycleEventKind::Refunded);
        assert_eq!(snapshot.milestones.refunded_at.as_deref(), Some("40"));
        assert_eq!(snapshot.display.follow_up[0].title, "Refund completed");
    }

    #[test]
    fn review_expiry_is_plain_language_and_does_not_fake_submission_time() {
        let rows = vec![message(
            "review-expired",
            json!({"source":"system","event":"review_expired","jobId":"j","timestamp":40}),
            "40",
        )];
        let snapshot = build_snapshot("j", &Status::Submitted, &rows);
        assert_eq!(snapshot.milestones.submitted_at, None);
        assert_eq!(snapshot.milestones.review_expired_at.as_deref(), Some("40"));
        assert_eq!(
            snapshot.display.timeline[3].title,
            "Deliverable review period ended"
        );
        assert_eq!(snapshot.display.timeline[3].marker, "—");
    }

    #[test]
    fn reject_expiry_waits_for_refund_without_claiming_it_completed() {
        let rows = vec![
            message(
                "rejected",
                json!({"source":"system","event":"job_rejected","jobId":"j","timestamp":30}),
                "30",
            ),
            message(
                "reject-expired",
                json!({"source":"system","event":"reject_expired","jobId":"j","timestamp":40}),
                "40",
            ),
        ];
        let snapshot = build_snapshot("j", &Status::Rejected, &rows);
        assert_eq!(snapshot.milestones.rejected_at.as_deref(), Some("30"));
        assert_eq!(snapshot.milestones.reject_expired_at.as_deref(), Some("40"));
        assert_eq!(snapshot.display.follow_up[0].title, "Waiting for refund");
        assert!(!snapshot
            .display
            .follow_up
            .iter()
            .any(|node| node.title == "Refund completed"));
    }

    #[test]
    fn exact_deadline_wins_then_event_then_submitted_plus_three_days() {
        let submitted = 1_700_000_000_i64;
        let events = events_from_history(
            "j",
            &[message(
                "submitted",
                json!({"source":"system","event":"job_submitted","jobId":"j","timestamp":submitted,"expireTime":submitted + 123}),
                &submitted.to_string(),
            )],
        );
        let milestones = fold_milestones(&events);
        assert_eq!(
            review_deadline(
                &json!({"expireTime": submitted + 456}),
                &events,
                &milestones
            ),
            Some(submitted + 456)
        );
        assert_eq!(
            review_deadline(&Value::Null, &events, &milestones),
            Some(submitted + 123)
        );

        let no_event_deadline = events_from_history(
            "j",
            &[message(
                "submitted",
                json!({"source":"system","event":"job_submitted","jobId":"j","timestamp":submitted}),
                &submitted.to_string(),
            )],
        );
        let milestones = fold_milestones(&no_event_deadline);
        assert_eq!(
            review_deadline(&Value::Null, &no_event_deadline, &milestones),
            Some(submitted + super::super::deadline::REVIEW_WINDOW_SECONDS)
        );
    }

    #[test]
    fn current_wallet_user_agent_is_selected_without_task_detail_binding() {
        let agents = vec![
            json!({"agentId":"asp-1","role":2}),
            json!({"agentId":"user-1","role":1}),
        ];
        assert_eq!(
            select_current_wallet_user_agent(&agents, ""),
            Some("user-1".to_string())
        );
        assert_eq!(
            select_current_wallet_user_agent(&agents, "user-1"),
            Some("user-1".to_string())
        );
        assert_eq!(select_current_wallet_user_agent(&agents, "asp-1"), None);
    }

    #[test]
    fn local_events_fill_missing_api_type_and_status() {
        let projected = project_task_detail(&json!({
            "buyerAgentId": "user-1",
            "providerAgentId": "asp-1"
        }));
        let events = events_from_history(
            "j",
            &[message(
                "accepted",
                json!({
                    "source":"system",
                    "event":"job_accepted",
                    "jobId":"j",
                    "jobType":0,
                    "jobStatus":"accepted",
                    "timestamp":20
                }),
                "20",
            )],
        );
        let reconciled = reconcile_current(&projected, &events);
        assert_eq!(reconciled.job_type, Some(0));
        assert_eq!(reconciled.status, Some(Status::Accepted));
        assert!(reconciled.status_from_local);
    }

    #[test]
    fn evaluation_is_completed_only_with_a_resolved_event() {
        let started = build_snapshot(
            "j",
            &Status::Completed,
            &[message(
                "requested",
                json!({"source":"system","event":"dispute_approved","jobId":"j","timestamp":30}),
                "30",
            )],
        );
        assert!(started.display.follow_up.is_empty());

        let resolved = build_snapshot(
            "j",
            &Status::Disputed,
            &[
                message(
                    "disputed",
                    json!({"source":"system","event":"job_disputed","jobId":"j","timestamp":30}),
                    "30",
                ),
                message(
                    "resolved",
                    json!({"source":"system","event":"dispute_resolved","jobId":"j","timestamp":40}),
                    "40",
                ),
            ],
        );
        assert_eq!(resolved.display.follow_up[0].title, "Platform review completed");
        assert_eq!(resolved.display.follow_up[0].marker, "✓");
    }

    #[test]
    fn early_refund_keeps_unreached_normal_stages_pending() {
        let snapshot = build_snapshot(
            "j",
            &Status::Failed,
            &[message(
                "refund",
                json!({"source":"system","event":"job_auto_refunded","jobId":"j","timestamp":40}),
                "40",
            )],
        );
        assert_eq!(snapshot.display.timeline[1].detail.as_deref(), Some("Not started"));
        assert_eq!(snapshot.display.timeline[2].detail.as_deref(), Some("Not started"));
        assert_eq!(snapshot.display.timeline[3].detail.as_deref(), Some("Not started"));
    }

    #[test]
    fn expiration_after_submission_keeps_execution_completed() {
        let snapshot = build_snapshot(
            "j",
            &Status::Expired,
            &[
                message(
                    "accepted",
                    json!({"source":"system","event":"job_accepted","jobId":"j","timestamp":20}),
                    "20",
                ),
                message(
                    "submitted",
                    json!({"source":"system","event":"job_submitted","jobId":"j","timestamp":30}),
                    "30",
                ),
                message(
                    "expired",
                    json!({"source":"system","event":"job_expired","jobId":"j","timestamp":40}),
                    "40",
                ),
            ],
        );
        assert_eq!(snapshot.display.timeline[2].title, "ASP executed");
        assert_eq!(snapshot.display.timeline[2].marker, "✓");
        assert_eq!(
            snapshot.display.timeline[3].title,
            "Deliverable review period ended"
        );
    }

    #[test]
    fn missing_submitted_time_never_uses_now_for_deadline() {
        assert_eq!(
            review_deadline(
                &json!({"expireConfig":{"reviewDeadline":259200}}),
                &[],
                &Milestones::default()
            ),
            None
        );
    }

    #[test]
    fn display_always_has_five_nodes_and_one_detail_per_node() {
        for status in [
            Status::Init,
            Status::Created,
            Status::Accepted,
            Status::Submitted,
            Status::Rejected,
            Status::Disputed,
            Status::Completed,
            Status::Close,
            Status::Expired,
            Status::Failed,
            Status::Other("future".to_string()),
        ] {
            let snapshot = build_snapshot("j", &status, &[]);
            assert_eq!(snapshot.display.timeline.len(), 5);
            assert!(snapshot.display.timeline.iter().all(|node| node
                .detail
                .as_deref()
                .is_none_or(|line| !line.contains('\n'))));
        }
    }

    #[test]
    fn incomplete_final_node_differs_from_other_future_nodes() {
        let snapshot = build_snapshot("j", &Status::Accepted, &[]);
        assert_eq!(snapshot.display.timeline[3].detail.as_deref(), Some("Not started"));
        assert_eq!(snapshot.display.timeline[4].detail.as_deref(), Some("Not completed"));
    }

    #[test]
    fn deliverable_before_submitted_status_waits_for_status_update() {
        let display = build_display(
            LifecyclePhase::AspExecuting,
            &Milestones::default(),
            None,
            None,
            None,
            &[],
            true,
            None,
        );
        assert_eq!(display.timeline[3].marker, "○");
        assert_eq!(display.timeline[3].title, "Deliverable review");
        assert_eq!(
            display.timeline[3].detail.as_deref(),
            Some("Deliverable received; waiting for the task status update")
        );
        assert_eq!(display.handled_by, "ASP");
        assert!(!display.next.contains("Accept or reject"));
        assert!(display.deliverable_available);
        assert!(!display.review_ready);
    }

    #[test]
    fn submitted_status_requires_deliverable_before_review_guidance() {
        let milestones = Milestones {
            submitted_at: Some("1700000000".to_string()),
            ..Milestones::default()
        };
        let waiting = build_display(
            LifecyclePhase::WaitingForUserReview,
            &milestones,
            Some("1700259200"),
            None,
            None,
            &[],
            false,
            None,
        );
        let ready = build_display(
            LifecyclePhase::WaitingForUserReview,
            &milestones,
            Some("1700259200"),
            None,
            None,
            &[],
            true,
            None,
        );

        assert_eq!(waiting.timeline[3].marker, ready.timeline[3].marker);
        assert_eq!(waiting.timeline[3].title, ready.timeline[3].title);
        assert!(waiting.timeline[3]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("waiting for the deliverable")));
        assert_eq!(waiting.handled_by, "ASP");
        assert_eq!(waiting.next, "Wait for the deliverable to arrive");
        assert!(!waiting.deliverable_available);
        assert!(!waiting.review_ready);
        assert!(ready.timeline[3]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("accept or reject by")));
        assert_eq!(ready.handled_by, "You");
        assert_eq!(ready.next, "Accept or reject the deliverable");
        assert!(ready.deliverable_available);
        assert!(ready.review_ready);
    }

    #[test]
    fn refund_omits_unavailable_detail_fields() {
        let snapshot = build_snapshot("j", &Status::Failed, &[]);
        let refund = snapshot
            .display
            .follow_up
            .iter()
            .find(|node| node.key == "refund")
            .unwrap();
        assert_eq!(refund.detail, None);
        let json = serde_json::to_value(refund).unwrap();
        assert!(json.get("detail").is_none());
    }

    #[test]
    fn local_lookup_keys_are_bounded_and_allowlisted() {
        assert!(safe_lookup_key("0xabc-123_test"));
        assert!(!safe_lookup_key("job' OR 1=1 --"));
        assert!(!safe_lookup_key("../../other.sqlite"));
        assert!(!safe_lookup_key(&"a".repeat(257)));
    }

    #[test]
    fn local_reader_is_read_only_scoped_and_filters_other_users() {
        let temp = test_tempdir();
        let root = temp.path().join(".okx-agent-task");
        let sqlite = root.join("sqlite");
        fs::create_dir_all(&sqlite).unwrap();
        let database = sqlite.join("command-store.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE command_queue (
                    id TEXT PRIMARY KEY,
                    type TEXT,
                    command_json TEXT,
                    created_at_ms INTEGER
                );",
            )
            .unwrap();
        let own = json!({
            "type":"ai-dispatch",
            "agentId":"asp-1",
            "clientAgentId":"user-1",
            "messageId":"m-own",
            "content": json!({
                "clientAgentId":"user-1",
                "message":{"source":"system","event":"job_created","jobId":"job-1","jobType":0,"jobStatus":"created","timestamp":1}
            }).to_string()
        });
        let other = json!({
            "type":"ai-dispatch",
            "clientAgentId":"user-2",
            "content": json!({
                "clientAgentId":"user-2",
                "message":{"source":"system","event":"job_accepted","jobId":"job-1","jobType":0,"timestamp":2}
            }).to_string()
        });
        connection
            .execute(
                "INSERT INTO command_queue VALUES (?1, 'ai-dispatch', ?2, 1)",
                ("row-own", own.to_string()),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO command_queue VALUES (?1, 'ai-dispatch', ?2, 2)",
                ("row-other", other.to_string()),
            )
            .unwrap();
        drop(connection);

        let read = read_scoped_local_history_at(&root, &database, "job-1", "user-1");
        assert!(read.read_succeeded);
        assert_eq!(read.messages.len(), 1);
        assert_eq!(events_from_history("job-1", &read.messages).len(), 1);

        let read_only =
            Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert!(read_only
            .execute("INSERT INTO command_queue VALUES ('x','x','x',0)", [])
            .is_err());
    }

    #[test]
    fn local_fallback_requires_an_owned_user_identity_not_an_asp_identity() {
        let agents = vec![
            json!({"agentId":"user-1","role":1}),
            json!({"agentId":"asp-1","role":2}),
            json!({"agentId":"user-2","role":"user"}),
        ];
        assert!(wallet_has_user_agent(&agents, "user-1"));
        assert!(wallet_has_user_agent(&agents, "user-2"));
        assert!(!wallet_has_user_agent(&agents, "asp-1"));
        assert!(!wallet_has_user_agent(&agents, "unknown"));
    }

    #[test]
    fn missing_directory_database_or_table_degrades_without_error() {
        let temp = test_tempdir();
        let missing = temp.path().join("missing.sqlite");
        assert!(
            !read_scoped_local_history_at(temp.path(), &missing, "job-1", "user-1").read_succeeded
        );

        let sqlite = temp.path().join("sqlite");
        fs::create_dir_all(&sqlite).unwrap();
        let database = sqlite.join("command-store.sqlite");
        drop(Connection::open(&database).unwrap());
        let read = read_scoped_local_history_at(temp.path(), &database, "job-1", "user-1");
        assert!(!read.read_succeeded);
        assert!(read.messages.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_database_is_rejected() {
        use std::os::unix::fs::symlink;

        let temp = test_tempdir();
        let root = temp.path().join("root");
        let sqlite = root.join("sqlite");
        fs::create_dir_all(&sqlite).unwrap();
        let actual = temp.path().join("actual.sqlite");
        drop(Connection::open(&actual).unwrap());
        let database = sqlite.join("command-store.sqlite");
        symlink(&actual, &database).unwrap();
        assert!(!read_scoped_local_history_at(&root, &database, "job-1", "user-1").read_succeeded);
    }
}
