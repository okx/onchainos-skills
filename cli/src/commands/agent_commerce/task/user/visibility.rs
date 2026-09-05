//! Change a marketplace task's visibility.
//!
//! The update endpoint uses its own wire values: public=1 and private=0. This
//! differs from the task-creation contract and must not be shared with it.

use anyhow::{anyhow, bail, Result};
use clap::ValueEnum;
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

use super::create::resolve_user_agent;

const SET_VISIBILITY_ACTION: &str = "setVisibility";
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lower")]
pub enum TaskVisibility {
    Public,
    Private,
}

impl TaskVisibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }

    fn update_api_value(self) -> i32 {
        match self {
            Self::Public => 1,
            Self::Private => 0,
        }
    }
}

fn build_request(visibility: TaskVisibility) -> Value {
    json!({ "visibility": visibility.update_api_value() })
}

fn is_success(data: &Value) -> bool {
    data.is_null() || *data == Value::Bool(true)
}

pub async fn handle_task_visibility_update(
    client: &mut TaskApiClient,
    job_id: &str,
    visibility: TaskVisibility,
) -> Result<()> {
    if job_id.trim().is_empty() {
        bail!("--job-id must not be empty");
    }

    let (user_agent_id, _) = resolve_user_agent().await?;
    let path = client.endpoint(job_id, SET_VISIBILITY_ACTION);
    let response = client
        .post_mutation_with_identity(&path, &build_request(visibility), &user_agent_id)
        .await
        .map_err(|error| anyhow!("task-visibility-update failed: {error}"))?;

    if !is_success(&response) {
        bail!(
            "task-visibility-update failed: backend did not confirm the update: {}",
            serde_json::to_string(&response).unwrap_or_else(|_| response.to_string())
        );
    }

    crate::output::success(json!({
        "updated": true,
        "payload": {
            "jobId": job_id,
            "targetVisibility": visibility.as_str(),
            "targetVisibilityValue": visibility.update_api_value(),
        },
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TaskVisibility;

    #[test]
    fn visibility_uses_the_update_api_wire_values() {
        assert_eq!(TaskVisibility::Public.update_api_value(), 1);
        assert_eq!(TaskVisibility::Private.update_api_value(), 0);
    }

    #[test]
    fn accepts_the_documented_null_success_response() {
        assert!(super::is_success(&serde_json::Value::Null));
        assert!(!super::is_success(&serde_json::json!(false)));
    }
}
