//! Buyer create-and-fund entry point for a one-time A2A task.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    self, fetch_my_agents_by_role_strict, network::task_api_client::TaskApiClient, AGENT_ROLE_USER,
    DEBUG_LOG, XLAYER_CHAIN_ID,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

pub const MAX_BUDGET: f64 = 10_000_000.0;
pub const MIN_DESCRIPTION_CHARS: usize = 20;
pub const MAX_DESCRIPTION_CHARS: usize = 2000;
pub const MAX_DESCRIPTION_SUMMARY_CHARS: usize = 200;
pub const MAX_BUDGET_DECIMALS: usize = 6;
pub const MAX_TITLE_CHARS: usize = 30;

pub struct CreateTaskParams {
    pub title: String,
    pub description: String,
    pub description_summary: Option<String>,
    pub provider_agent_id: String,
    pub payment_token_symbol: String,
    pub payment_token_amount: String,
    pub attachments: Option<Vec<String>>,
    pub service_id: String,
    pub service_params: String,
    pub service_token_address: String,
    pub service_token_amount: String,
    pub category_code: Option<String>,
    pub min_credit_score: Option<f64>,
    pub visibility: String,
    pub chain_id: u64,
    pub service_guide: Option<String>,
    pub service_guide_hash: Option<String>,
    pub guide_consent_json: Option<String>,
}

struct ValidatedParams {
    title: String,
    token_symbol: String,
    visibility: i64,
    guide_consent: Option<GuideConsentInput>,
}

#[derive(Debug)]
struct GuideConsentInput {
    draft: super::super::common::autotrade::guide::GuideDraft,
    consent_values: BTreeMap<String, serde_json::Value>,
}

impl CreateTaskParams {
    fn guide_draft(&self) -> Result<Option<super::super::common::autotrade::guide::GuideDraft>> {
        super::super::common::autotrade::guide::parse_draft(
            self.service_guide.as_deref(),
            self.service_guide_hash.as_deref(),
        )
    }

    fn guide_consent_values(&self) -> Result<BTreeMap<String, serde_json::Value>> {
        let Some(raw) = self.guide_consent_json.as_deref() else {
            return Ok(BTreeMap::new());
        };
        serde_json::from_str(raw).context("--guide-consent-json must be a JSON object")
    }

    fn validated_guide_consent(&self) -> Result<Option<GuideConsentInput>> {
        let Some(draft) = self.guide_draft()? else {
            if self.service_guide_hash.is_some() || self.guide_consent_json.is_some() {
                bail!("Guide Consent requires --service-guide");
            }
            return Ok(None);
        };
        if self.guide_consent_json.is_none() {
            bail!("--guide-consent-json is required with --service-guide, including {{}} when the Guide declares no consent fields");
        }
        let consent_values = self.guide_consent_values()?;
        super::super::common::autotrade::guide::validate_consent_values(&consent_values)?;
        Ok(Some(GuideConsentInput {
            draft,
            consent_values,
        }))
    }

    fn validate(&self) -> Result<ValidatedParams> {
        validate_title(&self.title)?;
        let description_len = self.description.chars().count();
        if self.description.trim().is_empty() {
            bail!("--description must not be empty");
        }
        if description_len > MAX_DESCRIPTION_CHARS {
            bail!(
                "--description may not exceed {MAX_DESCRIPTION_CHARS} characters (currently {description_len})"
            );
        }
        if self
            .description_summary
            .as_deref()
            .is_some_and(|value| value.chars().count() > MAX_DESCRIPTION_SUMMARY_CHARS)
        {
            bail!(
                "--description-summary may not exceed {MAX_DESCRIPTION_SUMMARY_CHARS} characters"
            );
        }
        if self.provider_agent_id.trim().is_empty() {
            bail!("--provider-agent-id is required; use the confirmed Service result unchanged");
        }
        if self.service_id.trim().is_empty() {
            bail!("--service-id is required; use the confirmed Service result unchanged");
        }
        if self.service_token_address.trim().is_empty() {
            bail!("--service-token-address must not be empty");
        }
        serde_json::from_str::<serde_json::Value>(&self.service_params)
            .map_err(|e| anyhow::anyhow!("--service-params must be valid JSON: {e}"))?;

        let token_symbol = normalize_currency(&self.payment_token_symbol)?;
        validate_decimal_amount(&self.payment_token_amount, "payment-token-amount")?;
        validate_decimal_amount(&self.service_token_amount, "service-token-amount")?;
        if self.chain_id != XLAYER_CHAIN_ID as u64 {
            bail!("--chain-id currently supports X Layer ({XLAYER_CHAIN_ID}) only");
        }
        if self
            .min_credit_score
            .is_some_and(|score| !(0.0..=1.0).contains(&score))
        {
            bail!("--min-credit-score must be between 0 and 1");
        }
        let visibility = match self.visibility.as_str() {
            "private" => 1,
            "public" => 0,
            _ => bail!("--visibility must be private or public"),
        };
        super::attachments::validate_attachment_sources(
            self.attachments.as_deref().unwrap_or(&[]),
        )?;

        Ok(ValidatedParams {
            title: common::util::sanitize_title_for_shell(&self.title),
            token_symbol,
            visibility,
            guide_consent: self.validated_guide_consent()?,
        })
    }
}

fn validate_decimal_amount(value: &str, flag: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() || value.starts_with(['-', '+']) {
        bail!("--{flag} must be a non-negative decimal string");
    }
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|part| {
            part.is_empty()
                || part.len() > MAX_BUDGET_DECIMALS
                || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        bail!(
            "--{flag} must be a decimal string with at most {MAX_BUDGET_DECIMALS} decimal places"
        );
    }
    Ok(())
}

fn prepare_guide_consent(
    job_id: &str,
    params: &CreateTaskParams,
    execution: &GuideConsentInput,
) -> Result<()> {
    let guide_file = execution.draft.clone().into_file(
        job_id,
        &params.service_id,
        Some(&params.provider_agent_id),
    );
    super::super::common::autotrade::guide::write_guide(&guide_file, &execution.draft.source)?;
    super::super::common::autotrade::guide::write_prepared_consent(
        job_id,
        &guide_file,
        execution.consent_values.clone(),
        super::super::common::autotrade::DEFAULT_AUTOTRADE_TTL_SEC,
    )
}

// ─── Validation helpers ─────────────────────────────────────────────────

pub fn normalize_currency(currency: &str) -> Result<String> {
    let normalized: String = currency
        .chars()
        .map(|character| if character == '₮' { 'T' } else { character })
        .collect::<String>()
        .to_uppercase();
    match normalized.as_str() {
        "USDT" | "USDT0" => Ok("USDT".to_string()),
        "USDG" => Ok("USDG".to_string()),
        _ => bail!("unsupported token: {currency}; only USDT (USD₮0) and USDG are supported"),
    }
}

pub fn validate_budget(budget: f64) -> Result<()> {
    if budget < 0.0 {
        bail!("budget must be a non-negative amount");
    }
    if budget > MAX_BUDGET {
        bail!(
            "per-task budget may not exceed {} USDT/USDG",
            MAX_BUDGET as u64
        );
    }
    Ok(())
}

pub fn validate_budget_decimals(budget: f64) -> Result<()> {
    let value = format!("{budget}");
    if let Some(dot) = value.find('.') {
        let fraction = value[dot + 1..].trim_end_matches('0');
        if fraction.len() > MAX_BUDGET_DECIMALS {
            bail!(
                "budget precision is limited to {MAX_BUDGET_DECIMALS} decimal places, currently {}",
                fraction.len()
            );
        }
    }
    Ok(())
}

pub(crate) async fn resolve_user_agent() -> Result<(String, String)> {
    let agents = fetch_my_agents_by_role_strict("user").await?;
    let user = agents
        .iter()
        .find(|agent| agent["role"].as_i64() == Some(AGENT_ROLE_USER))
        .ok_or_else(|| anyhow::anyhow!("the current account has no user identity; run `onchainos agent create --role user` first"))?;
    let agent_id = user["agentId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("agent is missing the agentId field"))?
        .to_string();
    Ok((
        agent_id,
        user["ownerAddress"].as_str().unwrap_or("").to_string(),
    ))
}

pub async fn handle_create(client: &mut TaskApiClient, params: CreateTaskParams) -> Result<()> {
    let validated = params.validate()?;

    crate::home::ensure_task_state_writable().context(
        "task state storage is not writable; set ONCHAINOS_HOME to a writable directory",
    )?;

    ensure_tokens_refreshed().await.map_err(|e| {
        anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}")
    })?;
    let has_session_cert = crate::wallet_store::load_session()?
        .is_some_and(|session| !session.session_cert.trim().is_empty());
    if !has_session_cert {
        bail!("current login has no sessionCert; run `onchainos wallet login` again before create-task");
    }
    let (user_agent_id, _) = resolve_user_agent().await?;
    if DEBUG_LOG {
        eprintln!("[task-create] user identity check passed (agentId: {user_agent_id})");
    }

    // Repeat the prepare-time balance check immediately before the V2
    // create-and-fund write boundary. A typed shortfall returns the shared
    // Funding contract without creating or broadcasting the task.
    let required = params
        .payment_token_amount
        .parse::<f64>()
        .context("--payment-token-amount is outside the supported numeric range")?;
    if let Err(error) = common::ensure_sufficient_balance(required, &validated.token_symbol).await {
        if let Some(insufficient) = error
            .downcast_ref::<common::deposit_qr::InsufficientBalanceError>()
        {
            let deposit = common::deposit_qr::resolve_current_deposit_info(&user_agent_id)
                .await
                .ok_or_else(|| anyhow::anyhow!("failed to resolve the funding address"))?;
            crate::output::success(build_task_creation_funding_result(
                insufficient,
                &deposit,
                &params.service_token_address,
            )?);
            return Ok(());
        }
    }

    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;
    let receipt = super::v2::execute(
        client,
        super::v2::CreateAndFundInput {
            title: &validated.title,
            description: &params.description,
            description_summary: params.description_summary.as_deref(),
            token_symbol: &validated.token_symbol,
            amount: &params.payment_token_amount,
            provider_agent_id: &params.provider_agent_id,
            service_id: &params.service_id,
            service_params: &params.service_params,
            service_token_address: &params.service_token_address,
            service_token_amount: &params.service_token_amount,
            category_code: params.category_code.as_deref(),
            min_credit_score: params.min_credit_score,
            visibility: validated.visibility,
            chain_id: params.chain_id,
            attachments: params.attachments.as_deref().unwrap_or(&[]),
        },
        &account_id,
        &address,
        &user_agent_id,
        |job_id| {
            if let Some(ref guide_consent) = validated.guide_consent {
                prepare_guide_consent(job_id, &params, guide_consent)?;
            }
            Ok(())
        },
    )
    .await?;

    let tx_hash = receipt.broadcast["txHash"].as_str().unwrap_or("pending");
    let guide_and_consent_active = if validated.guide_consent.is_some() {
        match super::super::common::autotrade::guide::activate_prepared_consent(&receipt.job_id) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("[guide-execution] task created, but Guide Consent could not be activated: {err}");
                false
            }
        }
    } else {
        false
    };
    audit::log(
        "cli",
        "user/task_create_and_fund_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={}", receipt.job_id),
            format!("agentId={user_agent_id}"),
            format!("paymentTokenSymbol={}", validated.token_symbol),
            format!("paymentTokenAmount={}", params.payment_token_amount),
            format!("designatedProvider={}", params.provider_agent_id),
            "bizType=201".to_string(),
            format!(
                "guideStatus={}",
                if guide_and_consent_active {
                    "active"
                } else {
                    "none"
                }
            ),
            format!(
                "consentStatus={}",
                if guide_and_consent_active {
                    "active"
                } else {
                    "none"
                }
            ),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    crate::output::success(serde_json::json!({
        "phase": "creation",
        "decision": "ready",
        "reason": "broadcast_submitted",
        "nextAction": [{
            "id": "watch_task",
            "recommend": true,
            "params": {"jobId": receipt.job_id}
        }],
        "payload": {
            "jobId": receipt.job_id,
            "type": 201,
            "bizType": 201,
            "status": "broadcast_submitted",
            "providerAgentId": params.provider_agent_id,
            "paymentTokenSymbol": validated.token_symbol,
            "paymentTokenAmount": params.payment_token_amount,
            "runtimeBound": true,
            "guideStatus": if guide_and_consent_active { "active" } else { "none" },
            "consentStatus": if guide_and_consent_active { "active" } else { "none" },
            "attachments": receipt.attachments,
            "broadcast": receipt.broadcast
        }
    }));
    Ok(())
}

pub(super) fn build_task_creation_funding_result(
    insufficient: &common::deposit_qr::InsufficientBalanceError,
    deposit: &common::deposit_qr::DepositInfo,
    token_address: &str,
) -> Result<serde_json::Value> {
    crate::funding::build_funding_bundle_for_address(
        "",
        &deposit.chain_index,
        &deposit.address,
        crate::funding::FundingBlockedInput {
            asset: &insufficient.currency,
            token_address,
            required: &insufficient.required,
            balance: Some(&insufficient.available),
            operation: Some(crate::funding::FUNDING_OPERATION_TASK_CREATION),
            error_code: None,
            error_message: None,
        },
    )
}

fn validate_title(title: &str) -> Result<()> {
    if title.trim().is_empty() {
        bail!("title must not be empty");
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        bail!(
            "title may not exceed {MAX_TITLE_CHARS} characters (currently {})",
            title.chars().count()
        );
    }
    Ok(())
}

fn validate_description_body(description: &str) -> Result<()> {
    let length = description.chars().count();
    if length < MIN_DESCRIPTION_CHARS {
        bail!(
            "description is too short (minimum {MIN_DESCRIPTION_CHARS} chars, currently {length})"
        );
    }
    if length > MAX_DESCRIPTION_CHARS {
        bail!("description may not exceed {MAX_DESCRIPTION_CHARS} chars (currently {length})");
    }
    Ok(())
}

/// Legacy prepare seam. Creation itself consumes the confirmed fixed-price
/// strings and does not repeat these business checks.
pub(crate) fn validate_draft_fields(
    description: Option<&str>,
    title: Option<&str>,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
) -> serde_json::Value {
    let mut checks = Vec::new();
    let mut errors = Vec::new();
    macro_rules! check {
        ($field:expr, $result:expr) => {
            match $result {
                Ok(()) => checks.push(serde_json::json!({"field": $field, "ok": true})),
                Err(error) => {
                    let message = error.to_string();
                    checks.push(serde_json::json!({"field": $field, "ok": false, "error": message}));
                    errors.push(message);
                }
            }
        };
    }
    if let Some(value) = description {
        check!("description", validate_description_body(value));
    }
    if let Some(value) = title {
        check!("title", validate_title(value));
    }
    if let Some(value) = currency {
        match normalize_currency(value) {
            Ok(normalized) => checks.push(
                serde_json::json!({"field": "currency", "ok": true, "normalized": normalized}),
            ),
            Err(error) => {
                let message = error.to_string();
                checks
                    .push(serde_json::json!({"field": "currency", "ok": false, "error": message}));
                errors.push(message);
            }
        }
    }
    if let Some(value) = budget {
        check!(
            "budget",
            validate_budget(value).and_then(|_| validate_budget_decimals(value))
        );
    }
    if let Some(value) = max_budget {
        check!(
            "max_budget",
            validate_budget(value).and_then(|_| validate_budget_decimals(value))
        );
    }
    if let (Some(budget), Some(max_budget)) = (budget, max_budget) {
        if max_budget < budget {
            errors.push(format!(
                "max_budget ({max_budget}) must be >= budget ({budget})"
            ));
        }
    }
    serde_json::json!({"ok": errors.is_empty(), "checks": checks, "errors": errors})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> CreateTaskParams {
        CreateTaskParams {
            title: "Market report".to_string(),
            description: "Summarize the confirmed market inputs".to_string(),
            description_summary: Some("Market summary".to_string()),
            provider_agent_id: "6508".to_string(),
            payment_token_symbol: "USDT".to_string(),
            payment_token_amount: "10.25".to_string(),
            attachments: None,
            service_id: "svc-1".to_string(),
            service_params: "{}".to_string(),
            service_token_address: "0xtoken".to_string(),
            service_token_amount: "10.25".to_string(),
            category_code: Some("FINANCE".to_string()),
            min_credit_score: Some(0.5),
            visibility: "private".to_string(),
            chain_id: 196,
            service_guide: None,
            service_guide_hash: None,
            guide_consent_json: None,
        }
    }

    #[test]
    fn fixed_price_params_validate_without_budget_or_payment_mode() {
        let validated = params().validate().unwrap();
        assert_eq!(validated.token_symbol, "USDT");
        assert_eq!(validated.visibility, 1);
    }

    #[test]
    fn exact_decimal_validation_rejects_float_ambiguity() {
        assert!(validate_decimal_amount("0.000001", "amount").is_ok());
        assert!(validate_decimal_amount("0.0000001", "amount").is_err());
        assert!(validate_decimal_amount("1e3", "amount").is_err());
        assert!(validate_decimal_amount("-1", "amount").is_err());
    }

    #[test]
    fn unicode_limits_use_character_count() {
        let mut value = params();
        value.title = "任".repeat(MAX_TITLE_CHARS + 1);
        assert!(value.validate().is_err());
        value.title = "任".repeat(MAX_TITLE_CHARS);
        assert!(value.validate().is_ok());
    }

    #[test]
    fn task_create_funding_block_uses_common_funding_contract() {
        let insufficient = common::deposit_qr::InsufficientBalanceError::new(
            "insufficient".to_string(),
            "USDT",
            0.01,
            0.0,
        );
        let deposit = common::deposit_qr::deposit_info_for_address(
            "0x1234567890abcdef1234567890abcdef12345678",
        );
        let result = build_task_creation_funding_result(
            &insufficient,
            &deposit,
            "0x779ded0c9e1022225f8e0630b35a9b54be713736",
        )
        .expect("common Funding result");
        assert_eq!(result["phase"], "funding_required");
        assert_eq!(result["decision"], "blocked");
        assert_eq!(result["reason"], "insufficient_balance");
        assert_eq!(result["nextAction"], serde_json::json!([]));
        assert_eq!(result["payload"]["operation"], "task_creation");
        assert_eq!(result["payload"]["fundingNeed"]["required"], "0.01");
        assert!(result["payload"]["qr"].is_object());
    }
}
