//! Read verified-sender XMTP subscription jobs for a User agent.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

const LIST_JOBS_BY_VERIFIED_SENDER_PATH: &str =
    "/priapi/v1/aieco/im/xmtp/subscription/list-jobs-by-verified-sender";

fn required_identifier(value: &str, argument: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--{argument} must not be blank");
    }
    Ok(value.to_owned())
}

pub(crate) fn list_jobs_path(group_id: &str, sender_inbox_id: &str) -> Result<String> {
    let group_id = required_identifier(group_id, "group-id")?;
    let sender_inbox_id = required_identifier(sender_inbox_id, "sender-inbox-id")?;
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("groupId", &group_id);
    query.append_pair("senderInboxId", &sender_inbox_id);
    Ok(format!("{LIST_JOBS_BY_VERIFIED_SENDER_PATH}?{}", query.finish()))
}

fn required_job_identifier(job: &Value, field: &str, index: usize) -> Result<()> {
    let value = job
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if value.is_none() {
        bail!("jobs[{index}].{field} must be a nonblank string");
    }
    Ok(())
}

pub(crate) fn validate_jobs(data: Value) -> Result<Value> {
    let jobs = data
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("list jobs response is missing jobs array"))?;
    for (index, job) in jobs.iter().enumerate() {
        if !job.is_object() {
            bail!("jobs[{index}] must be an object");
        }
        required_job_identifier(job, "jobId", index)?;
        required_job_identifier(job, "userAgentId", index)?;
        required_job_identifier(job, "aspAgentId", index)?;
    }
    Ok(data)
}

/// Isolated CLI integration tests can supply a raw backend payload or an
/// error without mutating process-global transport configuration. This branch
/// is omitted from release binaries; ordinary execution always uses the
/// authenticated request below.
fn debug_test_response_override() -> Result<Option<Value>> {
    if !cfg!(debug_assertions) {
        return Ok(None);
    }
    if let Ok(error) = std::env::var("ONCHAINOS_TEST_MOCK_LIST_JOBS_BY_VERIFIED_SENDER_ERROR") {
        bail!("{error}");
    }
    let Ok(raw_response) =
        std::env::var("ONCHAINOS_TEST_MOCK_LIST_JOBS_BY_VERIFIED_SENDER_JSON")
    else {
        return Ok(None);
    };
    serde_json::from_str(&raw_response)
        .map(Some)
        .map_err(|error| anyhow!("invalid list-jobs-by-verified-sender test response: {error}"))
}

/// Fetch the verified-sender jobs through the JWT + `agenticId` path only.
/// The returned data is intentionally not printed here so the command router
/// can apply the standard CLI output envelope exactly once.
pub async fn handle_list_jobs_by_verified_sender(
    client: &mut TaskApiClient,
    group_id: &str,
    sender_inbox_id: &str,
    agentic_id: &str,
) -> Result<Value> {
    let path = list_jobs_path(group_id, sender_inbox_id)?;
    let agentic_id = required_identifier(agentic_id, "agentic-id")?;
    let data = match debug_test_response_override()? {
        Some(data) => data,
        None => client.get_with_agent_id(&path, &agentic_id).await?,
    };
    validate_jobs(data)
}

#[cfg(test)]
mod tests {
    use super::{list_jobs_path, required_identifier, validate_jobs};
    use serde_json::json;

    #[test]
    fn blank_identifiers_fail_before_a_request_path_is_created() {
        assert!(list_jobs_path("   ", "sender").is_err());
        assert!(list_jobs_path("group", "\t").is_err());
        assert!(required_identifier("\n", "agentic-id").is_err());
    }

    #[test]
    fn query_values_are_trimmed_and_percent_encoded() {
        let path = list_jobs_path(" group +&?中文 ", " sender +&?中文 ").unwrap();

        assert_eq!(
            path,
            "/priapi/v1/aieco/im/xmtp/subscription/list-jobs-by-verified-sender?groupId=group+%2B%26%3F%E4%B8%AD%E6%96%87&senderInboxId=sender+%2B%26%3F%E4%B8%AD%E6%96%87"
        );
    }

    #[test]
    fn complete_and_empty_jobs_are_accepted() {
        let complete = json!({
            "jobs": [{
                "jobId": "job-1",
                "userAgentId": "user-1",
                "aspAgentId": "asp-1"
            }]
        });
        assert_eq!(validate_jobs(complete.clone()).unwrap(), complete);

        let empty = json!({"jobs": []});
        assert_eq!(validate_jobs(empty.clone()).unwrap(), empty);
    }

    #[test]
    fn missing_or_invalid_jobs_fail_closed() {
        for response in [
            json!({}),
            json!({"jobs": {}}),
            json!({"jobs": [null]}),
            json!({"jobs": [{"userAgentId": "user", "aspAgentId": "asp"}]}),
            json!({"jobs": [{"jobId": "job", "userAgentId": " ", "aspAgentId": "asp"}]}),
            json!({"jobs": [{"jobId": "job", "userAgentId": "user", "aspAgentId": 1}]}),
        ] {
            assert!(validate_jobs(response).is_err());
        }
    }
}
