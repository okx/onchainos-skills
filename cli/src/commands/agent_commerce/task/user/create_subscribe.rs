//! Create a subscription task.
//!
//! Flow: providerConfirmStatus → EIP-712 sign terms → createSubscription → local readiness → broadcast(bizType=204)

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::autotrade::{
    amount::Decimal,
    consent::{self, DynamicConsentSettings, MarginMode, OrderPolicy, TradeKitAuthMode},
    grants,
    trade_kit::TradeEnvironment,
};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
use crate::commands::agent_commerce::task::common::subscription_identity::{
    select_subscription_agent_id,
};
use crate::commands::agent_commerce::task::common::{self, DEBUG_LOG};
use crate::commands::agent_commerce::task::signing;

pub(crate) const SUBSCRIBE_API_PREFIX: &str = "/priapi/v1/aieco/task/subscribe";

pub struct CreateSubscribeParams {
    pub service_id: String,
    pub use_trial: bool,
    pub service_params: String,
    pub service_token_amount: String,
    pub service_token_address: String,
    pub auto_renew: i32,
    pub copy_trade: i32,
    pub title: String,
    pub description: String,
    pub attachments: Option<Vec<String>>,
    pub provider_agent_id: String,
    pub service_description: String,
    pub service_interval: String,
    pub autotrade_mode: Option<String>,
    pub autotrade_amount: Option<String>,
    pub autotrade_cap: Option<String>,
    pub autotrade_quote: Option<String>,
    pub autotrade_environment: Option<String>,
    pub autotrade_margin_mode: Option<String>,
    pub autotrade_order_policy: Option<String>,
    pub autotrade_auth_mode: Option<String>,
    pub autotrade_settings_json: Option<String>,
    pub autotrade_required_fields: Vec<String>,
    pub format: String,
}

const MAX_TITLE_CHARS: usize = 30;
const MAX_DESCRIPTION_CHARS: usize = 4096;
const TRADE_AMOUNT_REQUIRED_FIELD: &str = "tradeAmount";
const TRADE_AMOUNT_INTERNAL_ALIAS: &str = "tradeAmountU";

#[derive(Clone, Debug, PartialEq, Eq)]
struct SubscriptionAutoTradeConfig {
    mode: consent::ConsentMode,
    amount: Option<String>,
    cap: Option<String>,
    quote: String,
    environment: Option<TradeEnvironment>,
    margin_mode: Option<MarginMode>,
    order_policy: Option<OrderPolicy>,
    auth_mode: Option<TradeKitAuthMode>,
    dynamic_settings: DynamicConsentSettings,
}

impl CreateSubscribeParams {
    fn autotrade_requested(&self) -> bool {
        self.autotrade_mode.is_some()
            || self.autotrade_amount.is_some()
            || self.autotrade_cap.is_some()
            || self.autotrade_quote.is_some()
            || self.autotrade_environment.is_some()
            || self.autotrade_margin_mode.is_some()
            || self.autotrade_order_policy.is_some()
            || self.autotrade_auth_mode.is_some()
            || self.autotrade_settings_json.is_some()
            || !self.autotrade_required_fields.is_empty()
    }

    fn validate(&self) -> Result<()> {
        if self.service_id.is_empty() {
            bail!("--service-id is required");
        }
        if self.service_token_amount.is_empty() {
            bail!("--service-token-amount is required");
        }
        if self.service_token_address.is_empty() {
            bail!("--service-token-address is required");
        }
        if self.auto_renew != 0 && self.auto_renew != 1 {
            bail!("--auto-renew must be 0 (off) or 1 (on), got {}", self.auto_renew);
        }
        if self.copy_trade != 0 && self.copy_trade != 1 {
            bail!("--copy-trade must be 0 (off) or 1 (on), got {}", self.copy_trade);
        }
        if self.provider_agent_id.trim().is_empty() {
            bail!("--provider-agent-id is required; use the confirmed Service result unchanged");
        }
        if self.title.is_empty() {
            bail!("--title is required");
        }
        if self.title.chars().count() > MAX_TITLE_CHARS {
            bail!("--title exceeds {MAX_TITLE_CHARS} characters");
        }
        if self.description.is_empty() {
            bail!("--description is required");
        }
        if self.description.chars().count() > MAX_DESCRIPTION_CHARS {
            bail!("--description exceeds {MAX_DESCRIPTION_CHARS} characters");
        }
        super::attachments::validate_attachment_sources(
            self.attachments.as_deref().unwrap_or(&[]),
        )?;
        if self.autotrade_requested() && self.autotrade_mode.is_none() {
            bail!("--autotrade-mode is required when configuring signal execution; choose auto or notify_only");
        }
        let autotrade_config = self.autotrade_config()?;
        if autotrade_config.mode == consent::ConsentMode::Decline
            && (self.autotrade_amount.is_some()
                || self.autotrade_cap.is_some()
                || self.autotrade_quote.is_some()
                || self.autotrade_environment.is_some()
                || self.autotrade_margin_mode.is_some()
                || self.autotrade_order_policy.is_some()
                || self.autotrade_auth_mode.is_some()
                || self.autotrade_settings_json.is_some()
                || self
                    .autotrade_required_fields
                    .iter()
                    .any(|field| field != "mode"))
        {
            bail!("notify_only does not accept automatic execution settings");
        }
        self.validate_required_autotrade_fields(&autotrade_config)?;
        Ok(())
    }

    fn validate_required_autotrade_fields(
        &self,
        config: &SubscriptionAutoTradeConfig,
    ) -> Result<()> {
        let mut missing = Vec::new();
        let mut used_internal_trade_amount_alias = false;
        for declared_field in &self.autotrade_required_fields {
            let field = canonical_required_autotrade_field(declared_field);
            used_internal_trade_amount_alias |= declared_field == TRADE_AMOUNT_INTERNAL_ALIAS;
            let present = match field {
                "mode" => self.autotrade_mode.is_some(),
                TRADE_AMOUNT_REQUIRED_FIELD => config.amount.is_some(),
                "cap" => config.cap.is_some(),
                "quote" => true,
                "environment" => config.environment.is_some(),
                "marginMode" => config.margin_mode.is_some(),
                "orderPolicy" => config.order_policy.is_some(),
                "authMode" => config.auth_mode.is_some(),
                other => consent::dynamic_setting_present(&config.dynamic_settings, other),
            };
            if !present && !missing.iter().any(|missing_field| missing_field == field) {
                missing.push(field.to_string());
            }
        }
        if !missing.is_empty() {
            let alias_hint = if used_internal_trade_amount_alias
                && missing
                    .iter()
                    .any(|field| field == TRADE_AMOUNT_REQUIRED_FIELD)
            {
                " (tradeAmountU is an internal consent field; use tradeAmount with --autotrade-required-field)"
            } else {
                ""
            };
            bail!(
                "missing required automatic execution fields: {}{}",
                missing.join(", "),
                alias_hint
            );
        }
        Ok(())
    }

    fn autotrade_config(&self) -> Result<SubscriptionAutoTradeConfig> {
        let mode = match self.autotrade_mode.as_deref() {
            None => consent::ConsentMode::Decline,
            Some(mode) if mode.eq_ignore_ascii_case("auto") => consent::ConsentMode::Auto,
            Some(mode)
                if mode.eq_ignore_ascii_case("notify_only")
                    || mode.eq_ignore_ascii_case("notify-only")
                    || mode.eq_ignore_ascii_case("manual") =>
            {
                consent::ConsentMode::Decline
            }
            Some(_) => bail!("--autotrade-mode must be one of: auto | notify_only"),
        };
        let amount = parse_optional_positive_decimal(
            self.autotrade_amount.as_deref(),
            "--autotrade-amount",
        )?;
        let cap =
            parse_optional_positive_decimal(self.autotrade_cap.as_deref(), "--autotrade-cap")?;
        let quote = self
            .autotrade_quote
            .as_deref()
            .unwrap_or(consent::DEFAULT_QUOTE)
            .to_ascii_lowercase();
        if !consent::QUOTE_WHITELIST.contains(&quote.as_str()) {
            bail!("--autotrade-quote must be one of: usdt | usdc");
        }
        let environment = match self.autotrade_environment.as_deref() {
            None => None,
            Some(value) if value.eq_ignore_ascii_case("live") => Some(TradeEnvironment::Live),
            Some(value) if value.eq_ignore_ascii_case("demo") => Some(TradeEnvironment::Demo),
            Some(_) => bail!("--autotrade-environment must be one of: live | demo"),
        };
        let margin_mode = self
            .autotrade_margin_mode
            .as_deref()
            .map(MarginMode::parse)
            .transpose()?;
        let order_policy = self
            .autotrade_order_policy
            .as_deref()
            .map(OrderPolicy::parse)
            .transpose()?;
        let auth_mode = self
            .autotrade_auth_mode
            .as_deref()
            .map(TradeKitAuthMode::parse)
            .transpose()?;
        let mut dynamic_settings = consent::parse_dynamic_settings_json(
            self.autotrade_settings_json.as_deref(),
            "--autotrade-settings-json",
        )?;
        if !self.autotrade_required_fields.is_empty() {
            let mut required_fields = Vec::new();
            for field in &self.autotrade_required_fields {
                let field = canonical_required_autotrade_field(field).to_string();
                consent::validate_required_field_name(&field)?;
                if !required_fields.contains(&field) {
                    required_fields.push(field);
                }
            }
            dynamic_settings.insert(
                "requiredFields".to_string(),
                serde_json::to_value(required_fields)?,
            );
            consent::validate_dynamic_settings(&dynamic_settings)?;
        }
        consent::validate_amount_policy(amount.as_deref(), &dynamic_settings)?;

        Ok(SubscriptionAutoTradeConfig {
            mode,
            amount,
            cap,
            quote,
            environment,
            margin_mode,
            order_policy,
            auth_mode,
            dynamic_settings,
        })
    }
}

fn canonical_required_autotrade_field(field: &str) -> &str {
    match field {
        TRADE_AMOUNT_INTERNAL_ALIAS => TRADE_AMOUNT_REQUIRED_FIELD,
        other => other,
    }
}

fn parse_optional_positive_decimal(value: Option<&str>, flag: &str) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed =
        Decimal::parse(value).map_err(|_| anyhow::anyhow!("{flag} must be a positive decimal"))?;
    if parsed.is_zero() {
        bail!("{flag} must be greater than 0");
    }
    Ok(Some(parsed.to_plain_string()))
}

fn persist_subscription_autotrade(
    job_id: &str,
    config: &SubscriptionAutoTradeConfig,
) -> Result<()> {
    // The default quote is an automatic-execution convenience, not a value the
    // user authorized for a new notify-only policy.
    let quote = (config.mode == consent::ConsentMode::Auto).then_some(config.quote.as_str());
    consent::write_consent_policy_with_dynamic_settings(
        job_id,
        config.mode,
        config.cap.as_deref(),
        config.amount.as_deref(),
        quote,
        config.environment,
        config.margin_mode,
        config.order_policy,
        config.auth_mode,
        Some(&config.dynamic_settings),
        super::super::common::autotrade::DEFAULT_AUTOTRADE_TTL_SEC,
    )?;
    let grant_result = match config.mode {
        consent::ConsentMode::Auto => grants::write_auto_grant(
            job_id,
            super::super::common::autotrade::DEFAULT_AUTOTRADE_TTL_SEC,
        ),
        consent::ConsentMode::Manual | consent::ConsentMode::Decline => {
            grants::clear_grant(job_id);
            Ok(())
        }
    };
    if let Err(err) = grant_result {
        consent::clear_consent(job_id);
        grants::clear_grant(job_id);
        return Err(err);
    }
    Ok(())
}

fn build_duplicate_subscription_block(
    service_id: &str,
    existing: &super::subscription_ops::ExistingSubscriptionSummary,
) -> serde_json::Value {
    let base = format!(
        "Service {service_id} already has a subscription task, jobId: {}. It cannot be created again.",
        existing.job_id
    );
    let prompt = if existing.restore_listening_available {
        format!("{base} Would you like to restore listening?")
    } else {
        base
    };
    let mut block = serde_json::json!({
        "blockedReason": "duplicate-subscription",
        "userFacingPrompt": prompt,
        "existingSubscription": existing,
    });
    if existing.restore_listening_available {
        block["nextAfterUserChoice"] = serde_json::json!(["restore-listening"]);
    }
    block
}

pub async fn handle_create_subscribe(
    client: &mut TaskApiClient,
    params: CreateSubscribeParams,
) -> Result<()> {
    params.validate()?;
    let autotrade_requested = params.autotrade_requested();
    let autotrade_config = params.autotrade_config()?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;
    let has_session_cert = crate::wallet_store::load_session()?
        .is_some_and(|session| !session.session_cert.trim().is_empty());
    if !has_session_cert {
        bail!("current login has no sessionCert; run `onchainos wallet login` again before create-subscribe");
    }

    let (user_agent_id, _) = super::create::resolve_user_agent().await?;
    let user_agent_id = select_subscription_agent_id(&user_agent_id, "")?;
    if DEBUG_LOG {
        eprintln!("[create-subscribe] user identity check passed (agentId: {user_agent_id})");
    }

    // Repeat the selection-time check immediately before the write path to
    // close the confirmation-to-create race. Read or parse failures propagate,
    // so no balance, signing, confirmation, create, or broadcast call follows.
    let existing_subscriptions =
        super::subscription_ops::fetch_non_terminal_buyer_subscriptions_for_agent(
            client,
            &user_agent_id,
        ).await?;
    if let Some(existing) = super::subscription_ops::existing_subscription_for_service(
        &existing_subscriptions,
        &params.service_id,
    ) {
        return Err(crate::output::CliDuplicateSubscription {
            data: build_duplicate_subscription_block(&params.service_id, existing),
        }.into());
    }

    if let Some(warning) = subscribe_balance_warning(
        &params.service_token_amount,
        &params.service_token_address,
        &user_agent_id,
    )
    .await?
    {
        return Err(crate::output::CliFundingBlocked {
            data: build_subscription_funding_block(&warning),
        }
        .into());
    }

    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;
    let receipt = super::v2::execute_create_subscription(
        client,
        super::v2::CreateSubscriptionInput {
            service_id: &params.service_id,
            use_trial: params.use_trial,
            service_params: &params.service_params,
            service_token_amount: &params.service_token_amount,
            service_token_address: &params.service_token_address,
            auto_renew: params.auto_renew,
            copy_trade: params.copy_trade,
            title: &params.title,
            description: &params.description,
            provider_agent_id: &params.provider_agent_id,
            service_interval: &params.service_interval,
            attachments: params.attachments.as_deref().unwrap_or(&[]),
        },
        &account_id,
        &address,
        &user_agent_id,
        |job_id| {
            if autotrade_requested {
                persist_subscription_autotrade(job_id, &autotrade_config)?;
            }
            if !params.service_description.trim().is_empty() {
                crate::commands::agent_commerce::task::common::autotrade::profile::save_from_description(
                    job_id,
                    &params.service_id,
                    Some(&params.provider_agent_id),
                    &params.service_description,
                )?;
            }
            Ok(())
        },
    )
    .await?;

    let offline_replay = okx_a2a::probe_offline_replay_capability();
    let tx_hash = receipt.broadcast["txHash"].as_str().unwrap_or("pending");

    audit::log(
        "cli",
        "user/create_subscribe",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={}", receipt.job_id),
            format!("agentId={user_agent_id}"),
            format!("serviceId={}", params.service_id),
            format!("useTrial={}", receipt.effective_use_trial),
            format!("autoRenew={}", params.auto_renew),
            format!("copyTrade={}", params.copy_trade),
            format!("autoTradeConfigRequested={autotrade_requested}"),
            format!("autoTradeConfigured={autotrade_requested}"),
            "bizType=204".to_string(),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    let mut payload = serde_json::json!({
        "jobId": receipt.job_id,
        "type": 204,
        "bizType": 204,
        "status": "broadcast_submitted",
        "providerAgentId": params.provider_agent_id,
        "serviceId": params.service_id,
        "useTrial": receipt.effective_use_trial,
        "autoRenew": params.auto_renew,
        "copyTrade": params.copy_trade,
        "runtimeBound": true,
        "attachments": receipt.attachments,
        "autoTradeConfigRequested": autotrade_requested,
        "autoTradeConfigured": autotrade_requested,
        "executionProfileSaved": !params.service_description.trim().is_empty(),
        "offlineReplaySupported": offline_replay.supported,
        "broadcast": receipt.broadcast,
    });
    if !offline_replay.supported {
        payload["offlineReplayFixCommands"] =
            serde_json::json!(offline_replay.fix_commands_or_default());
    }
    crate::output::success(serde_json::json!({
        "phase": "creation",
        "decision": "ready",
        "reason": "broadcast_submitted",
        "nextAction": [{
            "id": "watch_task",
            "recommend": true,
            "params": {"jobId": receipt.job_id}
        }],
        "payload": payload,
    }));
    Ok(())
}

async fn subscribe_balance_warning(
    service_token_amount: &str,
    service_token_address: &str,
    user_agent_id: &str,
) -> Result<Option<serde_json::Value>> {
    let required: f64 = service_token_amount.parse().unwrap_or(0.0);
    if required <= 0.0 {
        return Ok(None);
    }

    let symbol = match common::util::resolve_token_symbol_by_address(
        common::XLAYER_CHAIN_INDEX,
        service_token_address,
    )
    .await
    {
        Ok(sym) => sym,
        Err(e) => {
            if DEBUG_LOG {
                eprintln!(
                    "[create-subscribe] ⚠ token symbol resolution failed \
                     (skipping balance pre-check): {e}"
                );
            }
            return Ok(None);
        }
    };

    match common::ensure_sufficient_balance(required, &symbol).await {
        Ok(()) => Ok(None),
        Err(e) => match e.downcast_ref::<common::deposit_qr::InsufficientBalanceError>() {
            Some(ib) => {
                let ib_owned = ib.clone();
                let (warning, _) =
                    common::deposit_qr::balance_warning_json(&ib_owned, user_agent_id).await;
                Ok(Some(warning))
            }
            None => Err(e),
        },
    }
}

fn build_subscription_funding_block(warning: &serde_json::Value) -> serde_json::Value {
    common::funding_notice::funding_blocked_envelope(warning, "subscription", "Subscription")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::super::TaskCommand,
    }

    #[test]
    fn cli_create_subscribe_all_required() {
        let cli = TestCli::parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_001",
            "--use-trial",
            "--service-token-amount", "10",
            "--service-token-address", "0x6776",
            "--auto-renew", "1",
            "--title", "Signal Subscription",
            "--description", "On-chain signal subscription service",
            "--provider-agent-id", "asp-1",
        ]);
        match cli.cmd {
            super::super::TaskCommand::CreateSubscribe {
                service_id,
                use_trial,
                service_token_amount,
                service_token_address,
                auto_renew,
                title,
                description,
                attachments,
                provider_agent_id,
                copy_trade,
                service_description,
                service_params,
                service_interval,
                autotrade_mode,
                autotrade_amount,
                autotrade_cap,
                autotrade_quote,
                autotrade_environment,
                autotrade_margin_mode,
                autotrade_order_policy,
                autotrade_auth_mode,
                autotrade_settings_json,
                autotrade_required_fields,
                format,
            } => {
                assert_eq!(service_id, "svc_001");
                assert!(use_trial);
                assert_eq!(service_token_amount, "10");
                assert_eq!(service_token_address, "0x6776");
                assert_eq!(auto_renew, "1");
                assert_eq!(title, "Signal Subscription");
                assert_eq!(description, "On-chain signal subscription service");
                assert!(attachments.is_none());
                assert_eq!(provider_agent_id, "asp-1");
                assert_eq!(copy_trade, 0);
                assert_eq!(service_description, "");
                assert_eq!(service_params, "");
                assert_eq!(service_interval, "month");
                assert!(autotrade_mode.is_none());
                assert!(autotrade_settings_json.is_none());
                assert!(autotrade_amount.is_none());
                assert!(autotrade_cap.is_none());
                assert!(autotrade_quote.is_none());
                assert!(autotrade_environment.is_none());
                assert!(autotrade_margin_mode.is_none());
                assert!(autotrade_order_policy.is_none());
                assert!(autotrade_auth_mode.is_none());
                assert!(autotrade_required_fields.is_empty());
                assert_eq!(format, "");
            }
            _ => panic!("expected CreateSubscribe"),
        }
    }

    #[test]
    fn cli_create_subscribe_accepts_repeated_files() {
        let cli = TestCli::parse_from([
            "test",
            "create-subscribe",
            "--service-id",
            "svc_attachments",
            "--service-token-amount",
            "10",
            "--service-token-address",
            "0x6776",
            "--auto-renew",
            "1",
            "--title",
            "Subscription with files",
            "--description",
            "Subscription request with two supporting files",
            "--provider-agent-id",
            "asp-1",
            "--file",
            "/tmp/brief.pdf",
            "--file",
            "/tmp/data.csv",
        ]);

        let super::super::TaskCommand::CreateSubscribe { attachments, .. } = cli.cmd else {
            panic!("expected CreateSubscribe");
        };
        assert_eq!(
            attachments,
            Some(vec![
                "/tmp/brief.pdf".to_string(),
                "/tmp/data.csv".to_string(),
            ])
        );
    }

    #[test]
    fn duplicate_block_only_offers_restore_for_active_subscription() {
        let active = super::super::subscription_ops::ExistingSubscriptionSummary {
            job_id: "job-active".to_string(),
            service_id: "svc-1".to_string(),
            provider_agent_id: "asp-1".to_string(),
            status_name: "ACTIVE".to_string(),
            restore_listening_available: true,
        };
        let active = super::build_duplicate_subscription_block("svc-1", &active);
        assert_eq!(active["blockedReason"], "duplicate-subscription");
        assert_eq!(active["existingSubscription"]["jobId"], "job-active");
        assert!(active["userFacingPrompt"].as_str().unwrap().contains("jobId: job-active"));
        assert!(active["userFacingPrompt"].as_str().unwrap().contains("cannot be created again"));
        assert!(!active["userFacingPrompt"].as_str().unwrap().contains("ACTIVE"));
        assert_eq!(
            active["nextAfterUserChoice"],
            serde_json::json!(["restore-listening"])
        );

        let rejected = super::super::subscription_ops::ExistingSubscriptionSummary {
            job_id: "job-rejected".to_string(),
            service_id: "svc-1".to_string(),
            provider_agent_id: "asp-1".to_string(),
            status_name: "REJECTED".to_string(),
            restore_listening_available: false,
        };
        let rejected = super::build_duplicate_subscription_block("svc-1", &rejected);
        assert!(rejected.get("nextAfterUserChoice").is_none());
        assert!(!rejected["userFacingPrompt"].as_str().unwrap().contains("Restore listening"));
        assert!(!rejected["userFacingPrompt"].as_str().unwrap().contains("REJECTED"));
        assert!(rejected.get("serviceId").is_none());
    }

    #[test]
    fn cli_create_subscribe_with_provider() {
        let cli = TestCli::parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_002",
            "--service-token-amount", "5",
            "--service-token-address", "0xAddr",
            "--auto-renew", "0",
            "--title", "Copy Trade",
            "--description", "Auto copy trade subscription",
            "--provider-agent-id", "agent-99",
        ]);
        match cli.cmd {
            super::super::TaskCommand::CreateSubscribe {
                provider_agent_id, use_trial, ..
            } => {
                assert_eq!(provider_agent_id, "agent-99");
                assert!(!use_trial);
            }
            _ => panic!("expected CreateSubscribe"),
        }
    }

    #[test]
    fn cli_create_subscribe_bool_strings() {
        let cli = TestCli::parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_003",
            "--service-token-amount", "1",
            "--service-token-address", "0xA",
            "--auto-renew", "true",
            "--title", "t",
            "--description", "d for test bool strings ok",
            "--provider-agent-id", "asp-1",
        ]);
        match cli.cmd {
            super::super::TaskCommand::CreateSubscribe { auto_renew, .. } => {
                assert_eq!(auto_renew, "true");
            }
            _ => panic!("expected CreateSubscribe"),
        }
    }

    #[test]
    fn cli_create_subscribe_missing_service_id_fails() {
        assert!(TestCli::try_parse_from([
            "test", "create-subscribe",
            "--service-token-amount", "10",
            "--service-token-address", "0xAddr",
            "--auto-renew", "1",
            "--title", "t",
            "--description", "d",
        ]).is_err());
    }

    #[test]
    fn cli_create_subscribe_rejects_create_time_device_selection() {
        assert!(TestCli::try_parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_001",
            "--service-token-amount", "10",
            "--service-token-address", "0xAddr",
            "--auto-renew", "1",
            "--title", "t",
            "--description", "d",
            "--provider-agent-id", "asp-1",
            "--exclude-device", "device-2",
        ]).is_err());
    }

    // The backend create response is the first point where the subscription has
    // a jobId. Bind that job to the current runtime before broadcasting so the
    // on-chain creation event cannot race ahead of local provider routing.
    #[test]
    fn create_subscribe_binds_job_provider_before_broadcast() {
        let source = include_str!("v2/create_subscription.rs");
        let job_id = source
            .find("validate_create_response(&response)")
            .expect("v2 create must validate jobId from the create response");
        let readiness = source
            .find("establish_local_readiness(&job_id)")
            .expect("v2 create must establish local execution readiness");
        let bind = source
            .find("bind_job_provider_to_current_runtime_required(&job_id)")
            .expect("v2 create must require runtime binding");
        let broadcast = source
            .find("signing::sign_uop_and_broadcast_full(")
            .expect("v2 create must broadcast the subscription transaction");
        let rollback = source
            .find("prebind.rollback_if_created().await")
            .expect("handler must roll back a newly-created binding when broadcast fails");

        assert!(job_id < readiness, "jobId must be resolved before local readiness");
        assert!(readiness < bind, "local readiness must precede runtime binding");
        assert!(bind < broadcast, "bind-current must run before broadcast");
        assert!(
            broadcast < rollback,
            "broadcast failure handling must be able to roll back the pre-bind"
        );
    }

    #[test]
    fn create_subscribe_validates_required_fields_before_remote_work() {
        let source = include_str!("create_subscribe.rs");
        let handler = source
            .split_once("pub async fn handle_create_subscribe")
            .expect("create-subscribe handler must exist")
            .1
            .split_once("#[cfg(test)]")
            .expect("handler must precede its tests")
            .0;

        let local_validation = handler
            .find("params.validate()?;")
            .expect("required-field validation must be part of the create handler");
        let first_remote_boundary = handler
            .find("ensure_tokens_refreshed().await")
            .expect("create handler must refresh the remote session");

        assert!(
            local_validation < first_remote_boundary,
            "required-field failures must return before authentication, remote create, signing, or broadcast"
        );
    }

    fn params_fixture(provider: Option<&str>) -> super::CreateSubscribeParams {
        super::CreateSubscribeParams {
            service_id: "svc".to_string(),
            use_trial: false,
            service_params: String::new(),
            service_token_amount: "10".to_string(),
            service_token_address: "0xtok".to_string(),
            auto_renew: 1,
            copy_trade: 0,
            title: "t".to_string(),
            description: "d".to_string(),
            attachments: None,
            provider_agent_id: provider.unwrap_or_default().to_string(),
            service_description: String::new(),
            service_interval: "month".to_string(),
            autotrade_mode: None,
            autotrade_amount: None,
            autotrade_cap: None,
            autotrade_quote: None,
            autotrade_environment: None,
            autotrade_margin_mode: None,
            autotrade_order_policy: None,
            autotrade_auth_mode: None,
            autotrade_settings_json: None,
            autotrade_required_fields: Vec::new(),
            format: "json".to_string(),
        }
    }

    #[test]
    fn create_subscribe_rejects_missing_attachment_before_creation() {
        let mut params = params_fixture(Some("asp-1"));
        params.attachments = Some(vec![
            "/path/that/does/not/exist/subscription-attachment.pdf".to_string(),
        ]);

        let error = params
            .validate()
            .expect_err("a missing attachment must stop subscription creation");
        assert!(error.to_string().contains("attachment file is not readable"));
    }

    #[test]
    fn subscription_funding_block_uses_funding_notice_protocol() {
        let warning = serde_json::json!({
            "sufficient": false,
            "chain": "XLayer",
            "currency": "USDT",
            "available": "0",
            "required": "0.0001",
            "shortfall": "0.0001",
            "depositAddress": "0x1234567890abcdef1234567890abcdef12345678",
            "depositChain": "XLayer"
        });

        let output = build_subscription_funding_block(&warning);
        assert_eq!(output["blocked"], serde_json::json!(true));
        assert_eq!(output["submitted"], serde_json::json!(false));
        assert_eq!(output["mustRunFundingNotice"], serde_json::json!(true));
        assert_eq!(
            output["fundingNoticeCommand"],
            "onchainos agent funding-notice --chain XLayer --currency USDT --shortfall 0.0001 --deposit-address 0x1234567890abcdef1234567890abcdef12345678 --available 0 --required 0.0001 --deposit-chain XLayer --reason subscription --format json"
        );
    }

    #[test]
    fn cli_create_subscribe_accepts_copy_trade_argument() {
        let cli = TestCli::parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_001",
            "--service-token-amount", "10",
            "--service-token-address", "0x6776",
            "--auto-renew", "1",
            "--copy-trade", "0",
            "--title", "Signal Subscription",
            "--description", "On-chain signal subscription service",
            "--provider-agent-id", "asp-1",
        ]);
        let super::super::TaskCommand::CreateSubscribe { copy_trade, .. } = cli.cmd else {
            panic!("expected CreateSubscribe");
        };
        assert_eq!(copy_trade, 0);
    }

    #[test]
    fn cli_create_subscribe_accepts_complete_autotrade_configuration() {
        let cli = TestCli::parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_auto",
            "--service-token-amount", "1",
            "--service-token-address", "0xA",
            "--auto-renew", "1",
            "--title", "Signals",
            "--description", "Execute the delivered signals",
            "--provider-agent-id", "asp-1",
            "--autotrade-mode", "auto",
            "--copy-trade", "1",
            "--autotrade-amount", "20.00",
            "--autotrade-cap", "50",
            "--autotrade-quote", "USDT",
            "--autotrade-environment", "demo",
            "--autotrade-margin-mode", "cross",
            "--autotrade-order-policy", "market",
            "--autotrade-auth-mode", "oauth",
            "--autotrade-required-field", "environment",
            "--autotrade-required-field", "orderPolicy",
            "--autotrade-required-field", "marginMode",
            "--autotrade-required-field", "authMode",
        ]);
        let super::super::TaskCommand::CreateSubscribe {
            autotrade_mode,
            autotrade_amount,
            autotrade_cap,
            autotrade_quote,
            autotrade_environment,
            autotrade_margin_mode,
            autotrade_order_policy,
            autotrade_auth_mode,
            autotrade_required_fields,
            ..
        } = cli.cmd else {
            panic!("expected CreateSubscribe");
        };
        assert_eq!(autotrade_mode.as_deref(), Some("auto"));
        assert_eq!(autotrade_amount.as_deref(), Some("20.00"));
        assert_eq!(autotrade_cap.as_deref(), Some("50"));
        assert_eq!(autotrade_quote.as_deref(), Some("USDT"));
        assert_eq!(autotrade_environment.as_deref(), Some("demo"));
        assert_eq!(autotrade_margin_mode.as_deref(), Some("cross"));
        assert_eq!(autotrade_order_policy.as_deref(), Some("market"));
        assert_eq!(autotrade_auth_mode.as_deref(), Some("oauth"));
        assert_eq!(
            autotrade_required_fields,
            ["environment", "orderPolicy", "marginMode", "authMode"]
        );
    }

    #[test]
    fn cli_create_subscribe_help_documents_required_field_contract() {
        let help = match TestCli::try_parse_from(["test", "create-subscribe", "--help"]) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("--help must exit through clap"),
        };

        for expected in [
            "mode (--autotrade-mode)",
            "tradeAmount (--autotrade-amount)",
            "cap (--autotrade-cap)",
            "quote (--autotrade-quote)",
            "environment",
            "--autotrade-environment",
            "marginMode (--autotrade-margin-mode)",
            "orderPolicy (--autotrade-order-policy)",
            "authMode",
            "--autotrade-auth-mode",
            "tradeAmountU",
            "deprecated alias",
        ] {
            assert!(
                help.contains(expected),
                "help must contain {expected:?}: {help}"
            );
        }
    }

    #[test]
    fn create_validation_rejects_missing_declared_autotrade_fields() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_environment = Some("demo".to_string());
        params.autotrade_required_fields = vec![
            "environment".to_string(),
            "orderPolicy".to_string(),
            "marginMode".to_string(),
        ];

        let error = params
            .validate()
            .expect_err("missing declared execution settings must block creation");
        assert_eq!(
            error.to_string(),
            "missing required automatic execution fields: orderPolicy, marginMode"
        );
    }

    #[test]
    fn create_validation_accepts_user_confirmed_dynamic_required_field() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_required_fields = vec![
            "leverageMode".to_string(),
            "leverage".to_string(),
            "extra.maxConcurrentPositions".to_string(),
        ];

        assert_eq!(
            params.validate().unwrap_err().to_string(),
            "missing required automatic execution fields: leverageMode, leverage, extra.maxConcurrentPositions"
        );
        params.autotrade_settings_json = Some(
            r#"{"leverageMode":"fixed","leverage":"2","extra":{"maxConcurrentPositions":{"label":"Maximum concurrent positions","type":"integer","value":3}}}"#
                .to_string(),
        );
        assert!(params.validate().is_ok());
        let settings = params.autotrade_config().unwrap().dynamic_settings;
        assert_eq!(settings["leverage"], serde_json::json!("2"));
        assert_eq!(
            settings["extra"]["maxConcurrentPositions"]["value"],
            serde_json::json!(3)
        );
        assert_eq!(
            settings["requiredFields"],
            serde_json::json!([
                "leverageMode",
                "leverage",
                "extra.maxConcurrentPositions"
            ])
        );
    }

    #[test]
    fn create_validation_persists_fixed_margin_amount_basis() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_amount = Some("10".to_string());
        params.autotrade_required_fields = vec![
            "tradeAmount".to_string(),
            "tradeAmountMode".to_string(),
            "tradeAmountBasis".to_string(),
        ];
        params.autotrade_settings_json = Some(
            r#"{"tradeAmountMode":"fixed_amount","tradeAmountBasis":"margin"}"#.to_string(),
        );

        assert!(params.validate().is_ok());
        let config = params.autotrade_config().unwrap();
        assert_eq!(config.amount.as_deref(), Some("10"));
        assert_eq!(
            config.dynamic_settings["tradeAmountMode"],
            serde_json::json!("fixed_amount")
        );
        assert_eq!(
            config.dynamic_settings["tradeAmountBasis"],
            serde_json::json!("margin")
        );
        assert_eq!(
            config.dynamic_settings["requiredFields"],
            serde_json::json!(["tradeAmount", "tradeAmountMode", "tradeAmountBasis"])
        );
    }

    #[test]
    fn create_validation_enforces_asp_required_amount_and_cap() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_required_fields = vec![
            "tradeAmount".to_string(),
            "cap".to_string(),
            "tradeAmount".to_string(),
        ];
        assert_eq!(
            params.validate().unwrap_err().to_string(),
            "missing required automatic execution fields: tradeAmount, cap"
        );

        params.autotrade_amount = Some("10".to_string());
        params.autotrade_cap = Some("100".to_string());
        assert!(params.validate().is_ok());
    }

    #[test]
    fn create_validation_accepts_public_trade_amount_required_field() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_amount = Some("100".to_string());
        params.autotrade_required_fields = vec![TRADE_AMOUNT_REQUIRED_FIELD.to_string()];

        assert!(params.validate().is_ok());
    }

    #[test]
    fn create_validation_normalizes_internal_trade_amount_alias() {
        assert_eq!(
            canonical_required_autotrade_field(TRADE_AMOUNT_INTERNAL_ALIAS),
            TRADE_AMOUNT_REQUIRED_FIELD
        );

        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_amount = Some("100".to_string());
        params.autotrade_required_fields = vec![TRADE_AMOUNT_INTERNAL_ALIAS.to_string()];

        assert!(params.validate().is_ok());
    }

    #[test]
    fn create_validation_reports_public_trade_amount_name_when_missing() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_required_fields = vec![TRADE_AMOUNT_REQUIRED_FIELD.to_string()];

        assert_eq!(
            params.validate().unwrap_err().to_string(),
            "missing required automatic execution fields: tradeAmount"
        );
    }

    #[test]
    fn create_validation_deduplicates_alias_and_explains_internal_name() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_required_fields = vec![
            TRADE_AMOUNT_INTERNAL_ALIAS.to_string(),
            TRADE_AMOUNT_REQUIRED_FIELD.to_string(),
        ];

        assert_eq!(
            params.validate().unwrap_err().to_string(),
            "missing required automatic execution fields: tradeAmount (tradeAmountU is an internal consent field; use tradeAmount with --autotrade-required-field)"
        );
    }

    #[test]
    fn autotrade_config_requires_explicit_mode_for_execution_fields() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_amount = Some("20".to_string());
        assert_eq!(
            params.validate().unwrap_err().to_string(),
            "--autotrade-mode is required when configuring signal execution; choose auto or notify_only"
        );
    }

    #[test]
    fn autotrade_config_without_execution_fields_is_notify_only_and_not_requested() {
        let params = params_fixture(Some("asp-1"));
        let config = params.autotrade_config().unwrap();
        assert_eq!(config.mode, consent::ConsentMode::Decline);
        assert!(!params.autotrade_requested());
    }

    #[test]
    fn autotrade_config_rejects_non_explicit_trade_environment() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_environment = Some("configured".to_string());
        assert!(params
            .validate()
            .unwrap_err()
            .to_string()
            .contains("--autotrade-environment must be one of: live | demo"));
    }

    #[test]
    fn autotrade_config_normalizes_and_does_not_enforce_cap() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_amount = Some("20.00".to_string());
        params.autotrade_cap = Some("50.0".to_string());
        params.autotrade_quote = Some("USDT".to_string());
        let config = params.autotrade_config().unwrap();
        assert_eq!(config.amount.as_deref(), Some("20"));
        assert_eq!(config.cap.as_deref(), Some("50"));
        assert_eq!(config.quote, "usdt");

        params.autotrade_amount = Some("51".to_string());
        assert!(params.validate().is_ok());
        assert_eq!(
            params.autotrade_config().unwrap().amount.as_deref(),
            Some("51")
        );
    }

    #[test]
    fn autotrade_config_maps_legacy_manual_to_notify_only() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("manual".to_string());

        let config = params.autotrade_config().unwrap();
        assert_eq!(config.mode, consent::ConsentMode::Decline);
        assert!(params.validate().is_ok());
    }

    #[test]
    fn notify_only_rejects_automatic_execution_settings() {
        let mut params = params_fixture(Some("asp-1"));
        params.autotrade_mode = Some("notify_only".to_string());
        params.autotrade_amount = Some("25".to_string());
        assert_eq!(
            params.validate().unwrap_err().to_string(),
            "notify_only does not accept automatic execution settings"
        );
    }

    #[test]
    fn persist_subscription_autotrade_writes_consent_and_enforceable_grants() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("create_subscribe_autotrade");
        if home.exists() {
            std::fs::remove_dir_all(&home).ok();
        }
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &home);

        let config = SubscriptionAutoTradeConfig {
            mode: consent::ConsentMode::Auto,
            amount: Some("20".to_string()),
            cap: Some("50".to_string()),
            quote: "usdt".to_string(),
            environment: Some(TradeEnvironment::Demo),
            margin_mode: Some(MarginMode::Cross),
            order_policy: Some(OrderPolicy::Market),
            auth_mode: Some(TradeKitAuthMode::OAuth),
            dynamic_settings: DynamicConsentSettings::new(),
        };
        persist_subscription_autotrade("job-subscribe-auto", &config).unwrap();

        let stored = consent::load_consent("job-subscribe-auto")
            .unwrap()
            .expect("consent must exist");
        assert_eq!(stored.mode, consent::ConsentMode::Auto);
        assert_eq!(stored.trade_amount_u.as_deref(), Some("20"));
        assert_eq!(stored.cap_u.as_deref(), Some("50"));
        assert_eq!(stored.quote_token.as_deref(), Some("usdt"));
        assert_eq!(stored.trade_environment, config.environment);
        assert_eq!(stored.auth_mode, config.auth_mode);
        assert_eq!(stored.margin_mode, config.margin_mode);
        assert_eq!(stored.order_policy, config.order_policy);
        assert!(grants::check_grant("job-subscribe-auto", "dex", "buy", "50").is_ok());
        assert!(grants::check_grant("job-subscribe-auto", "trade_kit", "sell", "50").is_ok());
        assert!(grants::check_grant("job-subscribe-auto", "trade_kit", "sell", "51").is_ok());

        let notify_only = SubscriptionAutoTradeConfig {
            mode: consent::ConsentMode::Decline,
            amount: None,
            cap: None,
            quote: consent::DEFAULT_QUOTE.to_string(),
            environment: None,
            margin_mode: None,
            order_policy: None,
            auth_mode: None,
            dynamic_settings: DynamicConsentSettings::new(),
        };
        persist_subscription_autotrade("job-subscribe-notify", &notify_only).unwrap();
        let stored = consent::load_consent("job-subscribe-notify")
            .unwrap()
            .expect("notify-only consent must exist");
        assert_eq!(stored.mode, consent::ConsentMode::Decline);
        assert_eq!(stored.quote_token, None);
        assert!(grants::check_grant("job-subscribe-notify", "trade_kit", "buy", "1").is_err());

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }
}
