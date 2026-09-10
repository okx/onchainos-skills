//! Buyer create-and-broadcast protocol for a subscription task.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

const SUBSCRIBE_API_PREFIX: &str = "/priapi/v1/aieco/task/subscribe";
const CREATE_SUBSCRIPTION_BIZ_TYPE: i64 = 204;

pub(in super::super) struct CreateSubscriptionInput<'a> {
    pub service_id: &'a str,
    pub use_trial: bool,
    pub service_params: &'a str,
    pub service_token_amount: &'a str,
    pub service_token_address: &'a str,
    pub auto_renew: i32,
    pub title: &'a str,
    pub description: &'a str,
    pub provider_agent_id: &'a str,
    pub service_interval: &'a str,
    pub attachments: &'a [String],
}

pub(in super::super) struct SubscriptionCreationReceipt {
    pub job_id: String,
    pub effective_use_trial: bool,
    pub broadcast: Value,
    pub attachments: Vec<Value>,
}

fn build_confirm_body(input: &CreateSubscriptionInput<'_>) -> Value {
    json!({
        "serviceId": input.service_id,
        "autoRenew": input.auto_renew,
        "useTrial": input.use_trial,
        "subId": 0,
        "providerAgentId": input.provider_agent_id,
    })
}

fn parse_confirmation(value: &Value, requested_use_trial: bool) -> Result<(Value, Value, bool)> {
    if value.as_object().map_or(true, |object| object.is_empty()) {
        bail!(
            "providerConfirmStatus returned empty terms; the service may not support subscription"
        );
    }
    let typed_data = value
        .get("typedData")
        .filter(|typed_data| {
            typed_data
                .as_object()
                .is_some_and(|object| !object.is_empty())
        })
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("providerConfirmStatus response missing typedData"))?;
    let mut terms = value.clone();
    terms
        .as_object_mut()
        .expect("non-empty confirmation must be an object")
        .remove("typedData");
    let effective_use_trial = value["useTrial"].as_bool().unwrap_or(requested_use_trial);
    Ok((terms, typed_data, effective_use_trial))
}

fn build_create_body(
    input: &CreateSubscriptionInput<'_>,
    effective_use_trial: bool,
    terms: Value,
    terms_sig: &str,
) -> Value {
    json!({
        "serviceId": input.service_id,
        "useTrial": effective_use_trial,
        "providerAgentId": input.provider_agent_id,
        "serviceInterval": input.service_interval,
        "serviceParams": input.service_params,
        "deviceList": Value::Null,
        "serviceTokenAmount": input.service_token_amount,
        "serviceTokenAddress": input.service_token_address,
        "autoRenew": input.auto_renew,
        "title": input.title,
        "description": input.description,
        "terms": terms,
        "termsSig": terms_sig,
    })
}

fn validate_create_response(value: &Value) -> Result<(String, i64)> {
    let job_id = value["jobId"]
        .as_str()
        .map(str::trim)
        .filter(|job_id| !job_id.is_empty())
        .ok_or_else(|| anyhow::anyhow!("createSubscription response missing jobId"))?
        .to_string();
    if value.get("uopData").is_none() || value["uopData"].is_null() {
        bail!("createSubscription response missing uopData");
    }
    let biz_type = signing::extract_biz_type(value);
    if biz_type != CREATE_SUBSCRIPTION_BIZ_TYPE {
        bail!("unexpected bizType {biz_type}; expected {CREATE_SUBSCRIPTION_BIZ_TYPE}");
    }
    Ok((job_id, biz_type))
}

pub(in super::super) async fn execute<F>(
    client: &mut TaskApiClient,
    input: CreateSubscriptionInput<'_>,
    account_id: &str,
    address: &str,
    user_agent_id: &str,
    establish_local_readiness: F,
) -> Result<SubscriptionCreationReceipt>
where
    F: FnOnce(&str) -> Result<()>,
{
    let confirmation = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/providerConfirmStatus"),
            &build_confirm_body(&input),
            user_agent_id,
        )
        .await
        .context("providerConfirmStatus failed")?;
    let (terms, typed_data, effective_use_trial) =
        parse_confirmation(&confirmation, input.use_trial)?;
    let terms_sig = signing::sign_typed_data(&typed_data, address)
        .await
        .context("EIP-712 subscription terms signing failed")?;
    let body = build_create_body(&input, effective_use_trial, terms, &terms_sig);
    let response = client
        .post_mutation_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/createSubscription"),
            &body,
            user_agent_id,
        )
        .await
        .context("createSubscription failed or returned an unknown network result")?;
    let (job_id, biz_type) = validate_create_response(&response)?;

    let attachments = super::super::attachments::copy_attachments_to_job_with_manifest(
        &job_id,
        input.attachments,
    )?;
    establish_local_readiness(&job_id)
        .context("subscription local execution configuration could not be persisted")?;
    // Provider routing is best-effort. A local okx-a2a readiness or binding
    // failure must not prevent an otherwise valid subscription broadcast.
    let prebind = common::a2a_binding::bind_job_provider_to_current_runtime(&job_id).await;

    let broadcast = match signing::sign_uop_and_broadcast_full(
        client,
        &response["uopData"],
        account_id,
        address,
        &job_id,
        biz_type,
        user_agent_id,
        None,
    )
    .await
    {
        Ok(value) if !value.is_null() => value,
        Ok(_) => {
            if let Some(prebind) = prebind.as_ref() {
                prebind.rollback_if_created().await;
            }
            common::autotrade::guide::abort_prepared_consent(&job_id);
            bail!("broadcast returned no receipt for jobId={job_id}");
        }
        Err(error) => {
            if let Some(prebind) = prebind.as_ref() {
                prebind.rollback_if_created().await;
            }
            common::autotrade::guide::abort_prepared_consent(&job_id);
            return Err(error).with_context(|| {
                format!("broadcast failed or returned an unknown result for jobId={job_id}")
            });
        }
    };

    Ok(SubscriptionCreationReceipt {
        job_id,
        effective_use_trial,
        broadcast,
        attachments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CreateSubscriptionInput<'static> {
        CreateSubscriptionInput {
            service_id: "svc-1",
            use_trial: true,
            service_params: "{\"symbol\":\"BTC\"}",
            service_token_amount: "10",
            service_token_address: "0xtoken",
            auto_renew: 1,
            title: "Signals",
            description: "Execute selected signals",
            provider_agent_id: "asp-1",
            service_interval: "month",
            attachments: &[],
        }
    }

    #[test]
    fn create_body_matches_subscription_backend_contract() {
        let body = build_create_body(&input(), true, json!({"subId": 0}), "0xsig");
        assert_eq!(body["providerAgentId"], "asp-1");
        assert!(body.get("copyTrade").is_none());
        assert_eq!(body["deviceList"], Value::Null);
        assert_eq!(body["termsSig"], "0xsig");
        assert!(body.get("descriptionSummary").is_none());
    }

    #[test]
    fn response_requires_job_uop_and_biz_type_204() {
        let valid = json!({"jobId": "job-1", "type": 204, "uopData": {"callData": "0x"}});
        assert_eq!(
            validate_create_response(&valid).unwrap(),
            ("job-1".to_string(), 204)
        );
        assert!(validate_create_response(&json!({"type": 204, "uopData": {}})).is_err());
        assert!(validate_create_response(&json!({"jobId": "job-1", "type": 204})).is_err());
        assert!(
            validate_create_response(&json!({"jobId": "job-1", "type": 101, "uopData": {}}))
                .is_err()
        );
    }
}
