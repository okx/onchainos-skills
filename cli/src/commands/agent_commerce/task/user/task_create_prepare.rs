//! Deterministic checks between Service confirmation and task field collection.

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Map, Value};

use crate::commands::agent_commerce::task::common::{
    self, autotrade::tooling, network::task_api_client::TaskApiClient,
};
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

use super::{asp_ops, create};

const PHASE_LOGIN_VALIDATION: &str = "login_validation";
const PHASE_IDENTITY_VALIDATION: &str = "identity_validation";
const PHASE_SERVICE_VALIDATION: &str = "service_validation";
const PHASE_SERVICE_ROUTING: &str = "service_routing";
const PHASE_PAYMENT_VALIDATION: &str = "payment_validation";
const PHASE_SUBSCRIPTION_VALIDATION: &str = "subscription_validation";
const PHASE_CREATION: &str = "creation";

fn build_decision(
    phase: &str,
    decision: &str,
    reason: &str,
    next_action: Value,
) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("phase".to_string(), Value::String(phase.to_string()));
    out.insert("decision".to_string(), Value::String(decision.to_string()));
    out.insert("reason".to_string(), Value::String(reason.to_string()));
    out.insert("nextAction".to_string(), next_action);
    out
}

fn emit(phase: &str, decision: &str, reason: &str, next_action: Value, payload: Value) {
    crate::output::success(decision_with_payload(
        phase,
        decision,
        reason,
        next_action,
        payload,
    ));
}

fn decision_with_payload(
    phase: &str,
    decision: &str,
    reason: &str,
    next_action: Value,
    payload: Value,
) -> Value {
    let mut out = build_decision(phase, decision, reason, next_action);
    out.insert("payload".to_string(), payload);
    Value::Object(out)
}

fn next_action(id: &str, recommend: bool) -> Value {
    json!([{"id": id, "recommend": recommend}])
}

fn a2mcp_service_routing_decision(service_snapshot: Value) -> Value {
    decision_with_payload(
        PHASE_SERVICE_ROUTING,
        "ready",
        "a2mcp_service_confirmed",
        next_action("invoke_a2mcp", true),
        json!({
            "schemaVersion": 1,
            "serviceSnapshot": service_snapshot,
        }),
    )
}

struct DuplicateSubscriptionContext {
    job_id: String,
    title: String,
    status: i64,
    active: bool,
}

fn duplicate_next_actions(existing: &DuplicateSubscriptionContext) -> Value {
    if existing.active {
        json!([
            {"id": "restore_subscription", "recommend": true},
            {"id": "stop", "recommend": false}
        ])
    } else {
        next_action("stop", true)
    }
}

fn duplicate_payload(existing: &DuplicateSubscriptionContext) -> Value {
    json!({
        "jobId": existing.job_id,
        "title": existing.title,
        "status": existing.status,
        "active": existing.active,
    })
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

fn duplicate_subscription_context(
    existing: &super::subscription_ops::ExistingSubscriptionSummary,
) -> Result<DuplicateSubscriptionContext> {
    let job_id = existing.job_id.trim();
    if job_id.is_empty() {
        bail!("blocking buyer subscription is missing required field `jobId`");
    }
    let title = existing.title.trim();
    if title.is_empty() {
        bail!("blocking buyer subscription is missing required field `title`");
    }
    Ok(DuplicateSubscriptionContext {
        job_id: job_id.to_string(),
        title: title.to_string(),
        status: existing.status,
        active: existing.restore_listening_available,
    })
}

fn duplicate_subscription_for_service(
    service: &Value,
    existing_subscriptions: &[super::subscription_ops::ExistingSubscriptionSummary],
) -> Result<Option<DuplicateSubscriptionContext>> {
    if service
        .get("supportSubscription")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Ok(None);
    }
    let service_id = required_service_string(service, "serviceId")?;
    super::subscription_ops::existing_subscription_for_service(
        existing_subscriptions,
        &service_id,
    )
    .map(duplicate_subscription_context)
    .transpose()
}

async fn fetch_service_detail(user_agent_id: &str, sid: &str) -> Result<Value> {
    let output = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "agent",
            "service-detail",
            "--sid",
            sid,
            "--agentic-id",
            user_agent_id,
        ])
        .output()
        .await
        .context("failed to invoke service-detail")?;
    if !output.status.success() {
        bail!(
            "service-detail failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let response: Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse service-detail JSON output")?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("service-detail returned a non-success response");
    }
    let service = response
        .get("data")
        .cloned()
        .ok_or_else(|| anyhow!("service-detail response is missing data"))?;
    if !service.is_object() {
        bail!("service-detail response data must be a Service object");
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
    let supports_trial = service
        .get("subscriptionInfo")
        .and_then(|info| info.get("supportTrial"))
        .and_then(Value::as_bool)
        == Some(true);
    let has_positive_trial = decimal(
        service
            .get("subscriptionInfo")
            .and_then(|info| info.get("freeTrial")),
        "subscriptionInfo.freeTrial",
    )
    .ok()
    .flatten()
    .is_some_and(|value| value > 0.0);
    supports_trial && has_positive_trial
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
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] a2a subscription flow start");
    }

    if common::current_account_xlayer_address().is_none()
        || ensure_tokens_refreshed().await.is_err()
    {
        emit(
            PHASE_LOGIN_VALIDATION,
            "blocked",
            "login_required",
            next_action("login", true),
            json!({}),
        );
        return Ok(());
    }

    let user_agent_id = match create::resolve_user_agent().await {
        Ok((agent_id, _)) => agent_id,
        Err(_) => {
            emit(
                PHASE_IDENTITY_VALIDATION,
                "blocked",
                "user_identity_required",
                next_action("register_user_agent", true),
                json!({}),
            );
            return Ok(());
        }
    };

    let selected_sid = sid.trim();
    if selected_sid.is_empty() {
        bail!("--sid must not be blank");
    }
    let service = fetch_service_detail(&user_agent_id, selected_sid).await?;
    required_service_string(&service, "serviceId")?;
    let service_type = required_service_string(&service, "serviceType")?;
    if service_type.eq_ignore_ascii_case("A2MCP") {
        // Service discovery and confirmation stay in the OKX.AI creation
        // entry. The selected authoritative Service object becomes the
        // immutable direct-invocation snapshot; no Task is created. `success`
        // adds the standard `{ok:true,data:...}` envelope around this decision.
        crate::output::success(a2mcp_service_routing_decision(service));
        return Ok(());
    }
    if !service_type.eq_ignore_ascii_case("A2A") {
        let service = normalize_service(service);
        emit(
            PHASE_SERVICE_VALIDATION,
            "blocked",
            "unsupported_service_type",
            next_action("stop", true),
            service,
        );
        return Ok(());
    }

    // Match create-subscribe's write-boundary source and status policy. The
    // Service detail isSubscribing flag may omit settlement-pending EXPIRED(8)
    // rows that still block duplicate creation.
    let duplicate_subscription =
        if service.get("supportSubscription").and_then(Value::as_bool) == Some(true) {
            let existing_subscriptions =
                super::subscription_ops::fetch_non_terminal_buyer_subscriptions_for_agent(
                    client,
                    &user_agent_id,
                )
                .await?;
            duplicate_subscription_for_service(&service, &existing_subscriptions)?
        } else {
            None
        };
    let service = normalize_service(service);
    if let Some(existing) = duplicate_subscription.as_ref() {
        emit(
            PHASE_SUBSCRIPTION_VALIDATION,
            "blocked",
            "duplicate_subscription",
            duplicate_next_actions(existing),
            duplicate_payload(existing),
        );
        return Ok(());
    }

    let required = effective_fee(&service)?;
    if trial_available(&service) || required == 0.0 {
        emit(
            PHASE_CREATION,
            "ready",
            "all_checks_passed",
            next_action("open_create_playbook", true),
            service,
        );
        return Ok(());
    }

    let currency = required_service_string(&service, "feeTokenSymbol")?;
    match common::ensure_sufficient_balance(required, &currency).await {
        Ok(()) => {
            emit(
                PHASE_CREATION,
                "ready",
                "all_checks_passed",
                next_action("open_create_playbook", true),
                service,
            );
            Ok(())
        }
        Err(error) => {
            let Some(insufficient) = error
                .downcast_ref::<common::deposit_qr::InsufficientBalanceError>()
                .cloned()
            else {
                return Err(error).context("failed to check the selected Service balance");
            };
            let deposit = common::deposit_qr::resolve_current_deposit_info(&user_agent_id)
                .await
                .ok_or_else(|| anyhow!("failed to resolve the funding address"))?;
            let fee_token = required_service_string(&service, "feeToken")?;
            let result = crate::funding::build_funding_bundle_for_address(
                "",
                &deposit.chain_index,
                &deposit.address,
                crate::funding::FundingBlockedInput {
                    asset: &insufficient.currency,
                    token_address: &fee_token,
                    required: &insufficient.required,
                    balance: Some(&insufficient.available),
                    operation: Some(crate::funding::FUNDING_OPERATION_TASK_CREATION),
                    error_code: None,
                    error_message: None,
                },
            )?;
            crate::output::success(result);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscription_service(support_trial: bool, free_trial: Value) -> Value {
        json!({
            "supportSubscription": true,
            "subscriptionInfo": {
                "supportTrial": support_trial,
                "freeTrial": free_trial
            }
        })
    }

    #[test]
    fn trial_requires_support_and_positive_free_trial() {
        assert!(trial_available(&subscription_service(true, json!(7))));
        assert!(trial_available(&subscription_service(true, json!("7"))));
        assert!(!trial_available(&subscription_service(true, json!(0))));
        assert!(!trial_available(&subscription_service(true, Value::Null)));
        assert!(!trial_available(&subscription_service(false, json!(7))));
    }

    #[test]
    fn zero_fee_subscription_is_ready_without_trial() {
        let service = json!({
            "supportSubscription": true,
            "subscriptionInfo": {
                "supportTrial": false,
                "freeTrial": 0,
                "feeAmount": 0
            }
        });

        assert!(!trial_available(&service));
        assert_eq!(effective_fee(&service).unwrap(), 0.0);
    }

    #[test]
    fn duplicate_payload_uses_authoritative_buyer_subscription() {
        let summary = super::super::subscription_ops::ExistingSubscriptionSummary {
            job_id: "job-42".to_string(),
            service_id: "svc-42".to_string(),
            provider_agent_id: "asp-42".to_string(),
            status_name: "ACTIVE".to_string(),
            restore_listening_available: true,
            title: "Signal Subscription".to_string(),
            status: 1,
        };
        let existing = duplicate_subscription_context(&summary).expect("valid duplicate metadata");

        assert_eq!(
            duplicate_payload(&existing),
            json!({
                "jobId": "job-42",
                "title": "Signal Subscription",
                "status": 1,
                "active": true
            })
        );
        assert_eq!(
            duplicate_next_actions(&existing),
            json!([
                {"id": "restore_subscription", "recommend": true},
                {"id": "stop", "recommend": false}
            ])
        );
    }

    #[test]
    fn inactive_duplicate_only_allows_stop() {
        let summary = super::super::subscription_ops::ExistingSubscriptionSummary {
            job_id: "job-43".to_string(),
            service_id: "svc-43".to_string(),
            provider_agent_id: "asp-43".to_string(),
            status_name: "EXPIRED".to_string(),
            restore_listening_available: false,
            title: "Paused Signals".to_string(),
            status: 8,
        };
        let existing = duplicate_subscription_context(&summary).expect("valid duplicate metadata");

        assert_eq!(existing.status, 8);
        assert_eq!(existing.title, "Paused Signals");
        assert_eq!(duplicate_next_actions(&existing), next_action("stop", true));
    }

    #[test]
    fn expired_buyer_subscription_blocks_when_service_detail_says_not_subscribing() {
        let service = json!({
            "serviceId": "svc-expired",
            "supportSubscription": true,
            "isSubscribing": false
        });
        let summaries = vec![super::super::subscription_ops::ExistingSubscriptionSummary {
            job_id: "job-expired".to_string(),
            service_id: "svc-expired".to_string(),
            provider_agent_id: "asp-expired".to_string(),
            status_name: "EXPIRED".to_string(),
            restore_listening_available: false,
            title: "Expired Signals".to_string(),
            status: 8,
        }];

        let existing = duplicate_subscription_for_service(&service, &summaries)
            .expect("duplicate lookup")
            .expect("expired subscription must block");

        assert_eq!(existing.job_id, "job-expired");
        assert_eq!(existing.status, 8);
        assert!(!existing.active);
    }

    #[test]
    fn confirmed_a2mcp_service_routes_to_direct_invocation_with_verbatim_snapshot() {
        let service = json!({
            "asp": {
                "aspAgentId": "5421",
                "aspName": "PixelBrief",
                "feedbackRate": 96.92,
                "onlineStatus": 1,
                "rating": "★ 4.86",
                "securityRate": 4.86,
                "soldCount": 21721
            },
            "endpoint": "https://pixelbrief.tech/v1/logo",
            "feeAmount": 0.05,
            "feeToken": "0x779ded0c9e1022225f8e0630b35a9b54be713736",
            "feeTokenSymbol": "USDT",
            "freeTrial": null,
            "isSubscribing": false,
            "serviceDescription": "Returns logo SVG and palette for a brand name and mood.\n1. brand name 2. mood 3. optional style",
            "serviceId": "9a5041d8-e03d-461d-b5cd-d2ffdd6111f3",
            "serviceName": "Logo SVG only",
            "serviceType": "A2MCP",
            "sid": 33803,
            "sortOrder": null,
            "subscription": [],
            "supportTrial": false
        });

        let envelope = json!({
            "ok": true,
            "data": a2mcp_service_routing_decision(service.clone()),
        });

        assert_eq!(
            envelope,
            json!({
                "ok": true,
                "data": {
                    "phase": "service_routing",
                    "decision": "ready",
                    "reason": "a2mcp_service_confirmed",
                    "nextAction": [{"id": "invoke_a2mcp", "recommend": true}],
                    "payload": {
                        "schemaVersion": 1,
                        "serviceSnapshot": service,
                    }
                }
            })
        );
    }

    #[test]
    fn login_required_decision_omits_action() {
        let output = build_decision(
            PHASE_LOGIN_VALIDATION,
            "blocked",
            "login_required",
            next_action("login", true),
        );

        assert_eq!(output["phase"], "login_validation");
        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "login_required");
        assert_eq!(output["nextAction"][0]["id"], "login");
        assert_eq!(output["nextAction"][0]["recommend"], true);
        assert!(output.get("action").is_none());
    }

    #[test]
    fn unknown_service_type_decision_omits_action() {
        let output = build_decision(
            PHASE_SERVICE_VALIDATION,
            "blocked",
            "unsupported_service_type",
            next_action("stop", true),
        );

        assert_eq!(output["phase"], "service_validation");
        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "unsupported_service_type");
        assert_eq!(output["nextAction"][0]["id"], "stop");
        assert!(output.get("action").is_none());
    }

    #[test]
    fn emit_shape_uses_phase_next_action_and_payload_without_action() {
        let mut output = build_decision(
            PHASE_CREATION,
            "ready",
            "all_checks_passed",
            next_action("open_create_playbook", true),
        );
        output.insert("payload".to_string(), json!({"serviceId": "svc-1"}));

        assert_eq!(output["phase"], "creation");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["reason"], "all_checks_passed");
        assert_eq!(output["nextAction"][0]["id"], "open_create_playbook");
        assert!(output.get("action").is_none());
        assert_eq!(output["payload"]["serviceId"], "svc-1");
        assert!(output.get("status").is_none());
        assert!(output.get("playbook").is_none());
        assert!(output.get("serviceData").is_none());
    }
}
