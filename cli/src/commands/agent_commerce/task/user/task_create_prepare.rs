//! Deterministic checks between Service confirmation and task field collection.

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Map, Value};

use crate::commands::agent_commerce::task::common::{
    self, autotrade::tooling, network::task_api_client::TaskApiClient,
};
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

use super::{asp_ops, create, subscription_ops};

const PHASE_LOGIN_CHECK: &str = "login-check";
const PHASE_USER_IDENTITY_CHECK: &str = "user-identity-check";
const PHASE_A2MCP_CHECK: &str = "a2mcp-check";
const PHASE_SERVICE_TYPE_CHECK: &str = "service-type-check";
const PHASE_BALANCE_CHECK: &str = "balance-check";
const PHASE_SUBSCRIPTION_CHECK: &str = "subscription-check";
const PHASE_READY_CHECK: &str = "ready-check";

const LOGIN_ACTION: &str = "Load the okx-agentic-wallet skill and complete wallet login. After login succeeds, rerun task-create-prepare with the same sid. Do not rerun service-match. If login cannot be completed, stop.";
const USER_IDENTITY_ACTION: &str = "Load references/identity-register.md and register a User Agent with --role user. Then rerun task-create-prepare with the same sid.";
const A2MCP_ACTION: &str = "Load okx-agent-payments-protocol skill with data.payload. Do not call create-task or create-subscribe.";
const UNKNOWN_SERVICE_TYPE_ACTION: &str = "Inform the user that data.payload.serviceType is unsupported for task creation. Do not call create-task or create-subscribe; stop.";

fn decision(phase: &str, action: impl Into<String>) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("phase".to_string(), Value::String(phase.to_string()));
    out.insert("action".to_string(), Value::String(action.into()));
    out
}

fn emit(phase: &str, action: impl Into<String>, payload: Value) {
    let mut out = decision(phase, action);
    out.insert("payload".to_string(), payload);
    crate::output::success(Value::Object(out));
}

fn ready_action() -> &'static str {
    "Open references/task-user-actions-create.md at Prepared service entry with data.payload and the original user utterance. Do not rerun service-match or task-create-prepare."
}

fn duplicate_action(existing: &subscription_ops::ExistingSubscriptionSummary) -> String {
    if existing.restore_listening_available {
        format!(
            "Do not create a duplicate subscription. Ask to restore listening for jobId={}. After explicit confirmation, open references/task-user-playbook.md at Signal-receipt watch with that jobId; otherwise stop.",
            existing.job_id
        )
    } else {
        format!(
            "Do not create a duplicate subscription. Say jobId={} blocks creation and listening cannot be restored; stop.",
            existing.job_id
        )
    }
}

fn balance_action(warning: &Value) -> String {
    format!(
        "Show this warning exactly, then stop: {warning}. After funding, rerun task-create-prepare with the same sid."
    )
}

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

fn required_service_string(service: &Value, key: &str) -> Result<String> {
    scalar_string(service.get(key))
        .ok_or_else(|| anyhow!("selected Service is missing required field `{key}`"))
}

fn selected_provider_agent_id(service: &Value) -> Result<String> {
    scalar_string(service.get("asp").and_then(|asp| asp.get("aspAgentId")))
        .ok_or_else(|| anyhow!("matched Service is missing required field `asp.aspAgentId`"))
}

fn selected_sid(service: &Value) -> Option<String> {
    scalar_string(service.get("sid"))
}

fn service_id(value: &Value) -> Option<String> {
    scalar_string(value.get("serviceId"))
        .or_else(|| scalar_string(value.get("ServiceId")))
        .or_else(|| scalar_string(value.get("id")))
}

fn find_service(value: &Value, expected_service_id: &str) -> Option<Value> {
    if service_id(value).as_deref() == Some(expected_service_id) {
        return Some(value.clone());
    }
    match value {
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_service(item, expected_service_id)),
        Value::Object(map) => {
            ["list", "services", "data"]
                .into_iter()
                .filter_map(|key| map.get(key))
                .find_map(|item| find_service(item, expected_service_id))
        }
        _ => None,
    }
}

async fn fetch_matched_service(user_agent_id: &str, sid: &str) -> Result<Value> {
    let output = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "agent",
            "service-match",
            "--sid",
            sid,
            "--agentic-id",
            user_agent_id,
            "--limit",
            "1",
        ])
        .output()
        .await
        .context("failed to invoke service-match")?;
    if !output.status.success() {
        bail!(
            "service-match failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let response: Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse service-match JSON output")?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("service-match returned a non-success response");
    }
    let services = response
        .get("data")
        .and_then(|data| data.get("services"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("service-match response is missing data.services"))?;
    services
        .iter()
        .find(|service| selected_sid(service).as_deref() == Some(sid))
        .cloned()
        .ok_or_else(|| anyhow!("service-match returned no Service matching sid `{sid}`"))
}

async fn fetch_service_detail(provider_agent_id: &str, service_id: &str) -> Result<Value> {
    let output = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "agent",
            "service-list",
            "--agent-id",
            provider_agent_id,
            "--service-id",
            service_id,
        ])
        .output()
        .await
        .context("failed to invoke service-list")?;
    if !output.status.success() {
        bail!(
            "service-list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let response: Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse service-list JSON output")?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("service-list returned a non-success response");
    }
    let data = response.get("data").cloned().unwrap_or(Value::Null);

    let service = find_service(&data, service_id).ok_or_else(|| {
        anyhow!(
            "service-list returned no Service matching serviceId `{service_id}` for Provider Agent `{provider_agent_id}`"
        )
    })?;
    Ok(service)
}

fn merge_service_guide(mut service: Value, detail: &Value) -> Result<Value> {
    let service_object = service
        .as_object_mut()
        .ok_or_else(|| anyhow!("service-match returned a non-object Service"))?;
    let guide = detail
        .get("serviceGuide")
        .or_else(|| detail.get("ServiceGuide"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match guide {
        Some(value) => {
            service_object.insert("serviceGuide".to_string(), Value::String(value.to_string()));
        }
        None => {
            service_object.remove("serviceGuide");
        }
    }
    Ok(service)
}

fn decimal(value: Option<&Value>, field: &str) -> Result<Option<f64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = value
        .as_f64()
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.trim().parse::<f64>().ok())
        })
        .ok_or_else(|| anyhow!("selected Service field `{field}` must be a number"))?;
    if !parsed.is_finite() || parsed < 0.0 {
        bail!("selected Service field `{field}` must be a non-negative number");
    }
    Ok(Some(parsed))
}

fn effective_fee(service: &Value) -> Result<f64> {
    if service.get("supportSubscription").and_then(Value::as_bool) == Some(true) {
        return decimal(
            service
                .get("subscriptionInfo")
                .and_then(|info| info.get("feeAmount")),
            "subscriptionInfo.feeAmount",
        )?
        .ok_or_else(|| anyhow!("selected subscription Service has no subscription fee"));
    }
    decimal(service.get("feeAmount"), "feeAmount")?
        .ok_or_else(|| anyhow!("selected Service has no feeAmount"))
}

fn trial_available(service: &Value) -> bool {
    service
        .get("subscriptionInfo")
        .and_then(|info| info.get("supportTrial"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn normalize_service(mut service: Value) -> Value {
    let description = service
        .get("serviceDescription")
        .and_then(Value::as_str)
        .unwrap_or("");
    let inventory = tooling::ToolInventory::detect();
    let preflight = tooling::build_preflight(description, &inventory);
    service["autoTradePreflight"] = serde_json::to_value(preflight).unwrap_or_else(|_| {
        serde_json::to_value(tooling::degraded_preflight()).unwrap_or_else(|_| json!({}))
    });
    asp_ops::compact_task_service_for_ai(&service)
}

pub(crate) async fn handle_task_create_prepare(
    client: &mut TaskApiClient,
    sid: &str,
) -> Result<()> {
    if common::current_account_xlayer_address().is_none()
        || ensure_tokens_refreshed().await.is_err()
    {
        emit(PHASE_LOGIN_CHECK, LOGIN_ACTION, json!({}));
        return Ok(());
    }

    let user_agent_id = match create::resolve_user_agent().await {
        Ok((agent_id, _)) => agent_id,
        Err(_) => {
            emit(PHASE_USER_IDENTITY_CHECK, USER_IDENTITY_ACTION, json!({}));
            return Ok(());
        }
    };

    let selected_sid = sid.trim();
    if selected_sid.is_empty() {
        bail!("--sid must not be blank");
    }
    let service = fetch_matched_service(&user_agent_id, selected_sid).await?;
    let provider_agent_id = selected_provider_agent_id(&service)?;
    let selected_service_id = required_service_string(&service, "serviceId")?;
    let detail = fetch_service_detail(&provider_agent_id, &selected_service_id).await?;
    let service = merge_service_guide(service, &detail)?;
    let service = normalize_service(service);

    let service_type = required_service_string(&service, "serviceType")?;
    if service_type.eq_ignore_ascii_case("A2MCP") {
        emit(PHASE_A2MCP_CHECK, A2MCP_ACTION, service);
        return Ok(());
    }
    if !service_type.eq_ignore_ascii_case("A2A") {
        emit(
            PHASE_SERVICE_TYPE_CHECK,
            UNKNOWN_SERVICE_TYPE_ACTION,
            service,
        );
        return Ok(());
    }

    let support_subscription = service
        .get("supportSubscription")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if support_subscription {
        let existing = subscription_ops::fetch_non_terminal_buyer_subscriptions_for_agent(
            client,
            &user_agent_id,
        )
        .await?;
        if let Some(existing) =
            subscription_ops::existing_subscription_for_service(&existing, &selected_service_id)
        {
            let action = duplicate_action(existing);
            emit(PHASE_SUBSCRIPTION_CHECK, action, service);
            return Ok(());
        }
    }

    let required = effective_fee(&service)?;
    if trial_available(&service) || required == 0.0 {
        emit(PHASE_READY_CHECK, ready_action(), service);
        return Ok(());
    }

    let currency = required_service_string(&service, "feeTokenSymbol")?;
    match common::ensure_sufficient_balance(required, &currency).await {
        Ok(()) => {
            emit(PHASE_READY_CHECK, ready_action(), service);
            Ok(())
        }
        Err(error) => {
            let insufficient = error
                .downcast_ref::<common::deposit_qr::InsufficientBalanceError>()
                .cloned();
            let Some(insufficient) = insufficient else {
                return Err(error).context("failed to check the selected Service balance");
            };
            let (warning, _) =
                common::deposit_qr::balance_warning_json(&insufficient, &user_agent_id).await;
            let action = balance_action(&warning);
            emit(PHASE_BALANCE_CHECK, action, service);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_required_action_resumes_preparation_with_the_same_service() {
        let output = decision(PHASE_LOGIN_CHECK, LOGIN_ACTION);
        let action = output["action"]
            .as_str()
            .expect("login-check output must include an action");

        assert_eq!(output["phase"], "login-check");
        assert!(output.get("status").is_none());
        assert!(output.get("playbook").is_none());
        assert!(action.contains("okx-agentic-wallet skill"));
        assert!(action.contains("same sid"));
        assert!(action.contains("Do not rerun service-match"));
        assert!(!action.contains("ask the user to retry"));
    }

    #[test]
    fn unknown_service_type_action_identifies_the_field_and_blocks_creation() {
        let output = decision(PHASE_SERVICE_TYPE_CHECK, UNKNOWN_SERVICE_TYPE_ACTION);
        let action = output["action"]
            .as_str()
            .expect("service-type-check output must include an action");

        assert_eq!(output["phase"], "service-type-check");
        assert!(action.contains("data.payload.serviceType"));
        assert!(action.contains("unsupported for task creation"));
        assert!(action.contains("Do not call create-task or create-subscribe"));
        assert!(!action.starts_with("Say "));
    }

    #[test]
    fn emit_shape_uses_action_phase_and_payload_terms() {
        let mut output = decision(PHASE_READY_CHECK, ready_action());
        output.insert("payload".to_string(), json!({"serviceId": "svc-1"}));

        assert_eq!(output["phase"], "ready-check");
        assert!(output["action"].as_str().unwrap().contains("data.payload"));
        assert!(!output["action"].as_str().unwrap().contains("branch="));
        assert_eq!(output["payload"]["serviceId"], "svc-1");
        assert!(output.get("status").is_none());
        assert!(output.get("playbook").is_none());
        assert!(output.get("serviceData").is_none());
    }
}
