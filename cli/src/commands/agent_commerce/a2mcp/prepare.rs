//! Marketplace preparation for explicitly selected A2MCP services.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

const SERVICE_PATH: &str = "/priapi/v5/wallet/agentic/agent/services";

fn select_snapshot(data: &Value, sid: &str) -> Result<Value> {
    let service = data
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("list").and_then(Value::as_array))
        .flatten()
        .find(|service| {
            service.get("id").is_some_and(|id| match id {
                Value::String(value) => value.trim() == sid,
                Value::Number(value) => value.to_string() == sid,
                _ => false,
            })
        })
        .ok_or_else(|| anyhow!("A2MCP preparation returned no Service matching sid `{sid}`"))?;
    if !service
        .get("serviceType")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("A2MCP"))
    {
        bail!("selected Service is not A2MCP");
    }
    Ok(service.clone())
}

fn ready_result(snapshot: Value) -> Value {
    json!({
        "phase": "service_routing",
        "decision": "ready",
        "reason": "a2mcp_service_confirmed",
        "nextAction": [{"id": "invoke_a2mcp", "recommend": true}],
        "payload": {"schemaVersion": 1, "serviceSnapshot": snapshot},
    })
}

pub(crate) async fn prepare_service(
    client: &mut TaskApiClient,
    sid: &str,
    asp_agent_id: &str,
) -> Result<()> {
    if common::current_account_xlayer_address().is_none()
        || ensure_tokens_refreshed().await.is_err()
    {
        crate::output::success(json!({
            "phase": "login_validation",
            "decision": "blocked",
            "reason": "login_required",
            "nextAction": [{"id": "login", "recommend": true}],
            "payload": {},
        }));
        return Ok(());
    }

    let sid = sid.trim();
    let asp_agent_id = asp_agent_id.trim();
    if sid.is_empty() {
        bail!("--sid must not be blank");
    }
    if asp_agent_id.is_empty() {
        bail!("--asp-agent-id must not be blank for A2MCP");
    }
    let data = client
        .get_authed_query(SERVICE_PATH, &[("agentId", asp_agent_id), ("id", sid)])
        .await?;
    crate::output::success(ready_result(select_snapshot(&data, sid)?));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_selected_snapshot_without_a2a_fee_conversion() {
        let service = json!({
            "id": 38241,
            "serviceType": "A2MCP",
            "fee": "0.1",
            "reqType": "MCP",
            "toolName": "wallet_insights_report",
            "inputSchema": {"type": "object"},
            "extra": {"preserve": true},
        });
        let response = json!([{"list": [{"id": 1}, service.clone()]}]);
        let selected = select_snapshot(&response, "38241").unwrap();
        assert_eq!(selected, service);
        let result = ready_result(selected);
        assert_eq!(result["payload"]["serviceSnapshot"], service);
        assert_eq!(result["nextAction"][0]["id"], "invoke_a2mcp");
    }

    #[test]
    fn rejects_a2a_or_wrong_service_ids() {
        let a2a = json!([{"list": [{"id": "39218", "serviceType": "A2A", "fee": "0.1"}]}]);
        assert!(select_snapshot(&a2a, "39218")
            .unwrap_err()
            .to_string()
            .contains("not A2MCP"));
        assert!(select_snapshot(&a2a, "38241").is_err());
        assert!(select_snapshot(&json!([{"list": [{"serviceId": "38241"}]}]), "38241").is_err());
    }
}
