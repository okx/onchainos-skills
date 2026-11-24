//! Fetch one current marketplace Service for task creation.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

const SERVICE_DETAIL_PATH: &str = "/priapi/v1/aieco/task/asp/service/search";

fn scalar_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

pub async fn handle_service_detail(
    client: &mut TaskApiClient,
    sid: &str,
    agentic_id: &str,
) -> Result<()> {
    let sid = sid.trim();
    if sid.is_empty() {
        bail!("--sid must not be blank");
    }
    let agentic_id = agentic_id.trim();
    if agentic_id.is_empty() {
        bail!("--agentic-id must not be blank");
    }

    let body = serde_json::to_vec(&json!({"sid": sid, "limit": 1}))?;
    let data = client
        .raw_post_with_identity(SERVICE_DETAIL_PATH, body, "application/json", agentic_id)
        .await?;
    let services = data
        .get("services")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("service detail response is missing services"))?;
    let service = services
        .iter()
        .find(|service| scalar_string(service.get("sid")).as_deref() == Some(sid))
        .cloned()
        .ok_or_else(|| anyhow!("service detail returned no Service matching sid `{sid}`"))?;

    crate::output::success(service);
    Ok(())
}
