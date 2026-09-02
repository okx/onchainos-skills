//! Buyer create-and-fund protocol for a one-time A2A task.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay;

const CREATE_AND_FUND_BIZ_TYPE: i64 = 201;

pub(in super::super) struct CreateAndFundInput<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub description_summary: Option<&'a str>,
    pub token_symbol: &'a str,
    pub amount: &'a str,
    pub provider_agent_id: &'a str,
    pub service_id: &'a str,
    pub service_params: &'a str,
    pub service_token_address: &'a str,
    pub service_token_amount: &'a str,
    pub category_code: Option<&'a str>,
    pub min_credit_score: Option<f64>,
    pub visibility: i64,
    pub chain_id: u64,
    pub attachments: &'a [String],
}

pub(in super::super) struct CreationReceipt {
    pub job_id: String,
    pub broadcast: Value,
    pub attachments: Vec<Value>,
}

struct ConfirmContext {
    job_id: String,
    task_salt: String,
    provider: String,
    receiver: String,
    evaluator: String,
    currency: String,
    recipient: String,
    amount: String,
    submit_window: u64,
    dispute_window: u64,
    evaluate_window: u64,
    completed_window: u64,
    hook: String,
    hook_data: String,
    salt: String,
    expired_at: String,
}

impl ConfirmContext {
    /// The deployed escrow flow currently derives the nonce with the hook
    /// address in the provider slot. Keep this compatibility mapping local to
    /// create-and-fund instead of changing the generic signing primitive.
    fn provider_for_escrow_nonce(&self) -> &str {
        &self.hook
    }
}

fn build_confirm_body(input: &CreateAndFundInput<'_>) -> Value {
    json!({
        "providerAgentId": input.provider_agent_id,
        "tokenSymbol": input.token_symbol,
        "amount": input.amount,
        "chainId": input.chain_id,
        "serviceId": input.service_id,
    })
}

fn required_string(value: &Value, name: &str) -> Result<String> {
    value[name]
        .as_str()
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("createAndFundConfirmStatus response missing {name}"))
}

fn required_u64(value: &Value, name: &str) -> Result<u64> {
    value[name]
        .as_u64()
        .or_else(|| value[name].as_str().and_then(|field| field.parse().ok()))
        .ok_or_else(|| {
            anyhow::anyhow!("createAndFundConfirmStatus response missing or invalid {name}")
        })
}

fn normalize_expired_at(value: &Value) -> Result<String> {
    let timestamp = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()));
    if let Some(timestamp) = timestamp {
        return chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|value| value.to_rfc3339())
            .ok_or_else(|| anyhow::anyhow!("invalid expiredAt: {timestamp}"));
    }
    value
        .as_str()
        .filter(|text| chrono::DateTime::parse_from_rfc3339(text).is_ok())
        .map(str::to_string)
        .ok_or_else(|| {
            anyhow::anyhow!("createAndFundConfirmStatus response missing or invalid expiredAt")
        })
}

fn parse_confirmation(value: &Value) -> Result<ConfirmContext> {
    Ok(ConfirmContext {
        job_id: required_string(value, "jobId")?,
        task_salt: required_string(value, "taskSalt")?,
        provider: required_string(value, "provider")?,
        receiver: required_string(value, "receiver")?,
        evaluator: required_string(value, "evaluator")?,
        currency: required_string(value, "currency")?,
        recipient: required_string(value, "recipient")?,
        amount: required_string(value, "amount")?,
        submit_window: required_u64(value, "submitWindow")?,
        dispute_window: required_u64(value, "disputeWindow")?,
        evaluate_window: required_u64(value, "evaluateWindow")?,
        completed_window: required_u64(value, "completedWindow")?,
        hook: required_string(value, "hook")?,
        hook_data: required_string(value, "hookData")?,
        salt: required_string(value, "salt")?,
        expired_at: normalize_expired_at(&value["expiredAt"])?,
    })
}

fn build_create_and_fund_body(
    input: &CreateAndFundInput<'_>,
    confirmation: &ConfirmContext,
    signature: &str,
    valid_after: &str,
    valid_before: &str,
) -> Result<Value> {
    let mut body = json!({
        "visibility": input.visibility,
        "jobId": confirmation.job_id,
        "taskSalt": confirmation.task_salt,
        "signature": signature,
        "validAfter": valid_after.parse::<u64>().context("invalid signed validAfter")?,
        "validBefore": valid_before.parse::<u64>().context("invalid signed validBefore")?,
        "title": input.title,
        "description": input.description,
        "paymentTokenSymbol": input.token_symbol,
        "paymentTokenAmount": input.amount,
        "chainId": input.chain_id,
        "providerAgentId": input.provider_agent_id,
        "serviceId": input.service_id,
        "serviceParams": input.service_params,
        "serviceTokenAddress": input.service_token_address,
        "serviceTokenAmount": input.service_token_amount,
    });
    if let Some(value) = input.description_summary {
        body["descriptionSummary"] = json!(value);
    }
    if let Some(value) = input.category_code {
        body["categoryCode"] = json!(value);
    }
    if let Some(value) = input.min_credit_score {
        body["minCreditScore"] = json!(value);
    }
    Ok(body)
}

fn validate_create_response(expected_job_id: &str, value: &Value) -> Result<(String, i64)> {
    let job_id = value["jobId"]
        .as_str()
        .filter(|field| !field.is_empty())
        .ok_or_else(|| anyhow::anyhow!("createAndFund response missing jobId"))?;
    if job_id != expected_job_id {
        bail!("createAndFund returned jobId {job_id}, expected {expected_job_id}");
    }
    if value.get("uopData").is_none() || value["uopData"].is_null() {
        bail!("createAndFund response missing uopData");
    }
    let biz_type = signing::extract_biz_type(value);
    if biz_type != CREATE_AND_FUND_BIZ_TYPE {
        bail!("unexpected bizType {biz_type}; expected {CREATE_AND_FUND_BIZ_TYPE}");
    }
    Ok((job_id.to_string(), biz_type))
}

pub(in super::super) async fn execute(
    client: &mut TaskApiClient,
    input: CreateAndFundInput<'_>,
    account_id: &str,
    address: &str,
    user_agent_id: &str,
) -> Result<CreationReceipt> {
    let confirmation_value = client
        .post_with_identity(
            "/priapi/v1/aieco/task/createAndFundConfirmStatus",
            &build_confirm_body(&input),
            user_agent_id,
        )
        .await
        .context("createAndFundConfirmStatus failed")?;
    let confirmation = parse_confirmation(&confirmation_value)?;
    audit::log(
        "cli",
        "user/task_create_and_fund_confirmed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={}", confirmation.job_id),
            format!("agentId={user_agent_id}"),
            format!("providerAgentId={}", input.provider_agent_id),
            format!("serviceId={}", input.service_id),
        ]),
        None,
    );

    // The authorization is derived only from the backend-confirmed escrow fields.
    let authorization = a2a_pay::sign_escrow(a2a_pay::SignEscrowParams {
        chain_id: input.chain_id,
        provider: confirmation.provider_for_escrow_nonce().to_string(),
        receiver: confirmation.receiver.clone(),
        arbitrator: confirmation.evaluator.clone(),
        currency: confirmation.currency.clone(),
        escrow_contract: confirmation.recipient.clone(),
        amount: confirmation.amount.clone(),
        submit_window: confirmation.submit_window,
        dispute_window: confirmation.dispute_window,
        arbitration_window: confirmation.evaluate_window,
        termination_window: confirmation.completed_window,
        hook: confirmation.hook.clone(),
        hook_data: confirmation.hook_data.clone(),
        salt: confirmation.salt.clone(),
        expired_at: confirmation.expired_at.clone(),
    })
    .await
    .context("EIP-3009 create-and-fund signing failed")?;

    let body = build_create_and_fund_body(
        &input,
        &confirmation,
        &authorization.signature,
        &authorization.authorization.valid_after,
        &authorization.authorization.valid_before,
    )?;
    let response = client
        .post_mutation_with_identity("/priapi/v1/aieco/task/createAndFund", &body, user_agent_id)
        .await
        .with_context(|| {
            format!(
                "createAndFund failed or returned an unknown network result for jobId={}",
                confirmation.job_id
            )
        })?;
    let (job_id, biz_type) = validate_create_response(&confirmation.job_id, &response)?;

    // Local readiness is established before broadcast so job_created cannot race it.
    let attachments = super::super::attachments::copy_attachments_to_job_with_manifest(
        &job_id,
        input.attachments,
    )?;
    let prebind = common::a2a_binding::bind_job_provider_to_current_runtime_required(&job_id)
        .await
        .context("cannot bind task to the current AI runtime; creation was not broadcast")?;

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
        Ok(value) => value,
        Err(error) => {
            prebind.rollback_if_created().await;
            return Err(error).with_context(|| {
                format!("broadcast failed or returned an unknown result for jobId={job_id}")
            });
        }
    };
    if broadcast.is_null() {
        prebind.rollback_if_created().await;
        bail!("broadcast returned no receipt");
    }

    Ok(CreationReceipt {
        job_id,
        broadcast,
        attachments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> CreateAndFundInput<'static> {
        CreateAndFundInput {
            title: "Write a report",
            description: "Summarize the market",
            description_summary: Some("Market summary"),
            token_symbol: "USDT",
            amount: "1.25",
            provider_agent_id: "6508",
            service_id: "42",
            service_params: "{}",
            service_token_address: "0xtoken",
            service_token_amount: "1.25",
            category_code: Some("FINANCE"),
            min_credit_score: Some(0.5),
            visibility: 1,
            chain_id: 196,
            attachments: &[],
        }
    }

    #[test]
    fn confirm_body_uses_fixed_price_contract() {
        let body = build_confirm_body(&input());
        assert_eq!(body["providerAgentId"], "6508");
        assert_eq!(body["tokenSymbol"], "USDT");
        assert_eq!(body["amount"], "1.25");
        assert_eq!(body["chainId"], 196);
        assert_eq!(body["serviceId"], "42");
        assert!(body.get("maxBudget").is_none());
        assert!(body.get("paymentMode").is_none());
    }

    fn confirmation() -> ConfirmContext {
        parse_confirmation(&json!({
            "jobId": "0xjob", "taskSalt": "0xtask", "provider": "0xprovider",
            "receiver": "0xreceiver", "evaluator": "0xevaluator",
            "currency": "0xcurrency", "recipient": "0xescrow", "amount": "1250000",
            "submitWindow": 10, "disputeWindow": "20", "evaluateWindow": 30,
            "completedWindow": 40, "hook": "0xhook", "hookData": "0xdata",
            "salt": "0xsalt", "expiredAt": 1_785_775_968i64,
        }))
        .unwrap()
    }

    #[test]
    fn create_body_uses_only_new_contract_fields() {
        let body =
            build_create_and_fund_body(&input(), &confirmation(), "0xsig", "0", "123").unwrap();
        assert_eq!(body["jobId"], "0xjob");
        assert_eq!(body["type"], Value::Null);
        assert_eq!(body["paymentTokenAmount"], "1.25");
        assert_eq!(body["descriptionSummary"], "Market summary");
        assert_eq!(body["categoryCode"], "FINANCE");
        assert!(body.get("paymentMostTokenAmount").is_none());
        assert!(body.get("paymentMode").is_none());
    }

    #[test]
    fn escrow_nonce_uses_hook_in_provider_slot_for_legacy_contract_compatibility() {
        let confirmation = confirmation();
        assert_eq!(confirmation.provider, "0xprovider");
        assert_eq!(confirmation.provider_for_escrow_nonce(), "0xhook");
    }

    #[test]
    fn response_requires_same_job_and_biz_type_201() {
        let valid = json!({"jobId": "0xjob", "type": 201, "uopData": {"callData": "0x"}});
        assert_eq!(validate_create_response("0xjob", &valid).unwrap().1, 201);
        assert!(validate_create_response(
            "0xjob",
            &json!({"jobId": "other", "type": 201, "uopData": {}})
        )
        .is_err());
        assert!(validate_create_response(
            "0xjob",
            &json!({"jobId": "0xjob", "type": 1, "uopData": {}})
        )
        .is_err());
    }
}
