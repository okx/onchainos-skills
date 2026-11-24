//! V2 buyer-side serviceParams update used by the provider clarification loop.

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

const MAX_SUCCESSFUL_ROUNDS: u8 = 3;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuccessfulUpdate {
    request_id: String,
    round: u8,
    service_params: Value,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RoundState {
    successful_updates: Vec<SuccessfulUpdate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ServiceParamTaskType {
    Single,
}

impl ServiceParamTaskType {
    fn path(self, client: &TaskApiClient, job_id: &str) -> String {
        match self {
            Self::Single => client.endpoint(job_id, "serviceParam"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
        }
    }
}

fn validate_inputs(
    job_id: &str,
    agent_id: &str,
    request_id: &str,
    round: u8,
    service_params: &str,
) -> Result<Value> {
    if job_id.trim().is_empty() {
        bail!("jobId is required");
    }
    if agent_id.trim().is_empty() {
        bail!("--agent-id is required");
    }
    if request_id.trim().is_empty() {
        bail!("--request-id is required");
    }
    if !(1..=MAX_SUCCESSFUL_ROUNDS).contains(&round) {
        bail!("--round must be between 1 and {MAX_SUCCESSFUL_ROUNDS}");
    }
    if service_params.trim().is_empty() {
        bail!("--service-params must contain the complete updated parameters");
    }
    serde_json::from_str(service_params).context("--service-params must be one complete JSON value")
}

fn state_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("OKX_AGENT_TASK_HOME") {
        return Ok(PathBuf::from(root).join("task-params"));
    }
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    Ok(home.join(".okx-agent-task").join("task-params"))
}

fn state_path(root: &Path, job_id: &str) -> PathBuf {
    let encoded = job_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    root.join(format!("{encoded}.json"))
}

fn read_state(path: &Path) -> Result<RoundState> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("parse task-params state {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RoundState::default()),
        Err(error) => {
            Err(error).with_context(|| format!("read task-params state {}", path.display()))
        }
    }
}

fn write_state(path: &Path, state: &RoundState) -> Result<()> {
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("write task-params state {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("commit task-params state {}", path.display()))
}

fn validate_round<'a>(
    state: &'a RoundState,
    request_id: &str,
    round: u8,
    service_params: &Value,
) -> Result<Option<&'a SuccessfulUpdate>> {
    if let Some(existing) = state
        .successful_updates
        .iter()
        .find(|entry| entry.request_id == request_id)
    {
        if existing.round != round || existing.service_params != *service_params {
            bail!("requestId was already used with different round or serviceParams");
        }
        return Ok(Some(existing));
    }
    let successful = u8::try_from(state.successful_updates.len()).unwrap_or(u8::MAX);
    if successful >= MAX_SUCCESSFUL_ROUNDS {
        bail!("three successful task-parameter updates already completed; provider must accept or decline");
    }
    let expected = successful + 1;
    if round != expected {
        bail!("--round must be the next successful round ({expected})");
    }
    Ok(None)
}

pub async fn handle(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    task_type: ServiceParamTaskType,
    request_id: &str,
    round: u8,
    service_params: &str,
) -> Result<()> {
    let parsed = validate_inputs(job_id, agent_id, request_id, round, service_params)?;
    let request_id = request_id.trim();
    let root = state_root()?;
    fs::create_dir_all(&root)
        .with_context(|| format!("create task-params state directory {}", root.display()))?;
    let state_path = state_path(&root, job_id);
    let lock_path = state_path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("open task-params lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .context("lock task-params round state")?;
    let mut state = read_state(&state_path)?;
    if validate_round(&state, request_id, round, &parsed)?.is_some() {
        crate::output::success(json!({
            "phase": "task_params_update",
            "decision": "ready",
            "reason": "duplicate_request_already_confirmed",
            "nextAction": [{
                "id": "send_task_params_response",
                "recommend": true,
                "params": {
                    "jobId": job_id,
                    "taskType": task_type.as_str(),
                    "requestId": request_id,
                    "round": round,
                    "serviceParams": parsed,
                }
            }],
            "payload": {
                "jobId": job_id,
                "requestId": request_id,
                "round": round,
                "successfulRounds": state.successful_updates.len(),
                "backendUpdated": true,
                "duplicate": true,
            }
        }));
        return Ok(());
    }
    let path = task_type.path(client, job_id);
    let response = client
        .post_mutation_with_identity(
            &path,
            &json!({"serviceParams": serde_json::to_string(&parsed)?}),
            agent_id,
        )
        .await
        .context(
            "serviceParam update failed or returned an unknown network result; do not send task_params_response",
        )?;
    if !response.is_null() {
        bail!(
            "serviceParam update returned unexpected data; do not send task_params_response: {response}"
        );
    }
    state.successful_updates.push(SuccessfulUpdate {
        request_id: request_id.trim().to_string(),
        round,
        service_params: parsed.clone(),
    });
    write_state(&state_path, &state)?;

    crate::output::success(json!({
        "phase": "task_params_update",
        "decision": "ready",
        "reason": "backend_update_confirmed",
        "nextAction": [{
            "id": "send_task_params_response",
            "recommend": true,
            "params": {
                "jobId": job_id,
                "taskType": task_type.as_str(),
                "requestId": request_id,
                "round": round,
                "serviceParams": parsed,
            }
        }],
        "payload": {
            "jobId": job_id,
            "taskType": task_type.as_str(),
            "requestId": request_id,
            "round": round,
            "successfulRounds": state.successful_updates.len(),
            "serviceParams": parsed,
            "backendUpdated": true,
            "duplicate": false,
        }
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_complete_json() {
        assert!(validate_inputs("job-1", "buyer-1", "req-1", 1, "").is_err());
        assert!(validate_inputs("job-1", "buyer-1", "req-1", 1, "plain text").is_err());
        assert_eq!(
            validate_inputs("job-1", "buyer-1", "req-1", 1, r#"{"symbol":"SOL"}"#).unwrap(),
            json!({"symbol":"SOL"})
        );
    }

    #[test]
    fn selects_only_the_single_task_endpoint() {
        let client = TaskApiClient::new();
        assert_eq!(
            ServiceParamTaskType::Single.path(&client, "job-1"),
            "/priapi/v1/aieco/task/job-1/serviceParam"
        );
        assert_eq!(
            ServiceParamTaskType::value_variants(),
            &[ServiceParamTaskType::Single]
        );
    }

    #[test]
    fn successful_rounds_are_sequential_deduplicated_and_capped() {
        let params = json!({"symbol":"SOL"});
        let mut state = RoundState::default();
        assert!(validate_round(&state, "req-1", 1, &params)
            .unwrap()
            .is_none());
        state.successful_updates.push(SuccessfulUpdate {
            request_id: "req-1".into(),
            round: 1,
            service_params: params.clone(),
        });
        assert!(validate_round(&state, "req-1", 1, &params)
            .unwrap()
            .is_some());
        assert!(validate_round(&state, "req-2", 3, &params).is_err());
        for round in 2..=3 {
            state.successful_updates.push(SuccessfulUpdate {
                request_id: format!("req-{round}"),
                round,
                service_params: params.clone(),
            });
        }
        assert!(validate_round(&state, "req-4", 3, &params).is_err());
    }
}
