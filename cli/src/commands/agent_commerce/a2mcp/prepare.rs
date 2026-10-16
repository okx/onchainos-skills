//! Marketplace preparation for explicitly selected A2MCP services.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

const SERVICE_PATH: &str = "/priapi/v5/wallet/agentic/agent/services";

fn service_id_matches(service: &Value, sid: &str) -> bool {
    service.get("id").is_some_and(|id| match id {
        Value::String(value) => value.trim() == sid,
        Value::Number(value) => value.to_string() == sid,
        _ => false,
    })
}

/// The response group (`{agentInfo, list}`) whose `list` holds the Service.
fn selected_group<'a>(data: &'a Value, sid: &str) -> Result<&'a Value> {
    data.as_array()
        .into_iter()
        .flatten()
        .find(|group| {
            group
                .get("list")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|service| service_id_matches(service, sid))
        })
        .ok_or_else(|| anyhow!("A2MCP preparation returned no Service matching sid `{sid}`"))
}

fn select_snapshot(group: &Value, sid: &str) -> Result<Value> {
    let service = group
        .get("list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|service| service_id_matches(service, sid))
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

fn select_agent_info(group: &Value, sid: &str, asp_agent_id: &str) -> Result<Value> {
    let agent_info = group
        .get("agentInfo")
        .filter(|value| value.is_object())
        .ok_or_else(|| {
            anyhow!("A2MCP preparation response is missing agentInfo for sid `{sid}`")
        })?;
    let response_agent_id = agent_info
        .get("agentId")
        .and_then(|value| match value {
            Value::String(value) => Some(value.trim().to_string()),
            Value::Number(value) if value.is_u64() => Some(value.to_string()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("A2MCP preparation response is missing agentInfo.agentId"))?;
    if response_agent_id != asp_agent_id {
        bail!(
            "A2MCP preparation returned Agent ID `{response_agent_id}`, expected `{asp_agent_id}`"
        );
    }
    Ok(json!({"agentId": response_agent_id}))
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
    let group = selected_group(&data, sid)?;
    let snapshot = select_snapshot(group, sid)?;
    let agent_info = select_agent_info(group, sid, asp_agent_id)?;
    let mut result = ready_result(snapshot);
    result["payload"]["agentInfo"] = agent_info;
    crate::output::success(result);
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
        let group = selected_group(&response, "38241").unwrap();
        let selected = select_snapshot(group, "38241").unwrap();
        assert_eq!(selected, service);
        let result = ready_result(selected);
        assert_eq!(result["payload"]["serviceSnapshot"], service);
        assert_eq!(result["nextAction"][0]["id"], "invoke_a2mcp");
    }

    #[test]
    fn rejects_a2a_or_wrong_service_ids() {
        let a2a = json!([{"list": [{"id": "39218", "serviceType": "A2A", "fee": "0.1"}]}]);
        let group = selected_group(&a2a, "39218").unwrap();
        assert!(select_snapshot(group, "39218")
            .unwrap_err()
            .to_string()
            .contains("not A2MCP"));
        assert!(selected_group(&a2a, "38241").is_err());
        assert!(selected_group(&json!([{"list": [{"serviceId": "38241"}]}]), "38241").is_err());
    }

    #[test]
    fn accepts_string_and_integer_provider_ids() {
        for id in [json!(" 7514 "), json!(7514)] {
            let response = json!([{
                "agentInfo": {"agentId": id},
                "list": [{"id": 38241, "serviceType": "A2MCP"}],
            }]);
            let group = selected_group(&response, "38241").unwrap();
            assert_eq!(
                select_agent_info(group, "38241", "7514").unwrap(),
                json!({"agentId": "7514"}),
            );
            assert!(select_agent_info(group, "38241", "7515").is_err());
        }
    }

    #[test]
    fn rejects_missing_or_invalid_provider_ids() {
        for id in [
            Value::Null,
            json!(" "),
            json!(true),
            json!(7514.5),
            json!(-1),
        ] {
            let response = json!([{
                "agentInfo": {"agentId": id},
                "list": [{"id": 38241, "serviceType": "A2MCP"}],
            }]);
            let group = selected_group(&response, "38241").unwrap();
            assert!(select_agent_info(group, "38241", "7514").is_err());
        }
    }

    #[test]
    fn rejects_group_without_agent_info() {
        let response = json!([{"list": [{"id": 38241, "serviceType": "A2MCP"}]}]);
        let group = selected_group(&response, "38241").unwrap();
        assert!(select_agent_info(group, "38241", "7514")
            .unwrap_err()
            .to_string()
            .contains("missing agentInfo"));
    }
}
