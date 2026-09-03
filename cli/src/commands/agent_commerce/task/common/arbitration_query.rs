//! User/ASP arbitration list and detail queries.

use anyhow::{bail, Result};
use serde_json::Value;

use super::network::task_api_client::TaskApiClient;
use super::state_machine::{DisputeRoundStatus, Status};

const ARBITRATION_LIST_PATH: &str = "/priapi/v1/aieco/task/dispute/my";

fn status_name(status: i64) -> String {
    i32::try_from(status)
        .map(Status::from_int)
        .map(|status| status.as_str().to_string())
        .unwrap_or_else(|_| format!("status_{status}"))
}

fn round_status_name(status: i64) -> String {
    i32::try_from(status)
        .map(DisputeRoundStatus::from_int)
        .map(|status| status.as_str().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn arbitration_phase(task_status: Option<i64>, round_status: Option<i64>) -> &'static str {
    match round_status {
        Some(0) => "evidence_preparation",
        Some(1 | 2) => "arbitrating",
        Some(3) => "resolved",
        Some(4) => "rejected",
        Some(5) => "invalidated",
        Some(_) => "unknown",
        None if matches!(task_status, Some(6 | 9)) => "resolved",
        None => "unknown",
    }
}

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

    let path = format!("{ARBITRATION_LIST_PATH}?page={page}&pageSize={page_size}");
    let mut data = client.get_with_agent_id(&path, agent_id).await?;
    if let Some(list) = data.get_mut("list").and_then(Value::as_array_mut) {
        for item in list {
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            let Some(status) = object.get("status").and_then(Value::as_i64) else {
                continue;
            };
            object.insert("statusName".to_string(), Value::String(status_name(status)));
        }
    }

    crate::output::success(data);
    Ok(())
}

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

    let path = client.endpoint(job_id, "dispute/status");
    let mut data = client.get_with_agent_id(&path, agent_id).await?;
    if let Some(object) = data.as_object_mut() {
        let task_status = object.get("taskStatus").and_then(Value::as_i64);
        let round_status = object
            .get("disputeRoundStatus")
            .or_else(|| object.get("disputeStatus"))
            .and_then(Value::as_i64);

        if let Some(status) = task_status {
            object.insert(
                "taskStatusName".to_string(),
                Value::String(status_name(status)),
            );
        }
        if let Some(status) = round_status {
            object.insert(
                "disputeRoundStatusName".to_string(),
                Value::String(round_status_name(status)),
            );
        }
        object.insert(
            "phase".to_string(),
            Value::String(arbitration_phase(task_status, round_status).to_string()),
        );
    }

    crate::output::success(data);
    Ok(())
}
