//! Create a subscription task.
//!
//! Flow: providerConfirmStatus → EIP-712 sign terms → create → sign uopData → broadcast(bizType=101)

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::autotrade::{
    amount::Decimal,
    consent::{self, MarginMode, OrderPolicy},
    grants,
    trade_kit::TradeEnvironment,
};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a::{self, OfflineReplayCapability};
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
    pub title: String,
    pub description: String,
    pub provider_agent_id: Option<String>,
    pub service_description: String,
    pub service_interval: String,
    pub autotrade_mode: Option<String>,
    pub autotrade_amount: Option<String>,
    pub autotrade_cap: Option<String>,
    pub autotrade_quote: Option<String>,
    pub autotrade_environment: Option<String>,
    pub autotrade_margin_mode: Option<String>,
    pub autotrade_order_policy: Option<String>,
    pub autotrade_required_fields: Vec<String>,
    pub format: String,
    pub exclude_device: Option<Vec<String>>,
}

const MAX_TITLE_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq)]
struct SubscriptionAutoTradeConfig {
    mode: consent::ConsentMode,
    amount: Option<String>,
    cap: Option<String>,
    quote: String,
    environment: Option<TradeEnvironment>,
    margin_mode: Option<MarginMode>,
    order_policy: Option<OrderPolicy>,
}

impl CreateSubscribeParams {
    fn validate(&self) -> Result<()> {
        if self.exclude_device.is_some() {
            bail!("create-time device selection is unsupported; create the subscription for all logged-in devices, then adjust receiving devices with subscribe-device-update");
        }
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
        let autotrade_config = self.autotrade_config()?;
        self.validate_required_autotrade_fields(&autotrade_config)?;
        Ok(())
    }

    fn validate_required_autotrade_fields(
        &self,
        config: &SubscriptionAutoTradeConfig,
    ) -> Result<()> {
        let mut missing = Vec::new();
        for field in &self.autotrade_required_fields {
            let present = match field.as_str() {
                "mode" => self.autotrade_mode.is_some(),
                "tradeAmount" => config.amount.is_some(),
                "cap" => config.cap.is_some(),
                "quote" => true,
                "environment" => config.environment.is_some(),
                "marginMode" => config.margin_mode.is_some(),
                "orderPolicy" => config.order_policy.is_some(),
                _ => bail!(
                    "--autotrade-required-field must be one of: mode | tradeAmount | cap | quote | environment | marginMode | orderPolicy"
                ),
            };
            if !present && !missing.contains(field) {
                missing.push(field.clone());
            }
        }
        if !missing.is_empty() {
            bail!(
                "missing required automatic execution fields: {}",
                missing.join(", ")
            );
        }
        Ok(())
    }

    fn autotrade_config(&self) -> Result<SubscriptionAutoTradeConfig> {
        let mode = match self.autotrade_mode.as_deref() {
            None => consent::ConsentMode::Auto,
            Some(mode) if mode.eq_ignore_ascii_case("auto") => consent::ConsentMode::Auto,
            Some(mode) if mode.eq_ignore_ascii_case("manual") => consent::ConsentMode::Manual,
            Some(_) => bail!("--autotrade-mode must be one of: auto | manual"),
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

        Ok(SubscriptionAutoTradeConfig {
            mode,
            amount,
            cap,
            quote,
            environment,
            margin_mode,
            order_policy,
        })
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
    consent::write_consent_policy_with_settings(
        job_id,
        config.mode,
        config.cap.as_deref(),
        config.amount.as_deref(),
        Some(&config.quote),
        config.environment,
        config.margin_mode,
        config.order_policy,
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

/// Assemble the `create` request body. `device_list` is ALWAYS embedded (even
/// empty) so the created record never relies on server-default routing;
/// `providerAgentId` is only present when a designated provider was requested.
fn build_create_body(
    params: &CreateSubscribeParams,
    effective_use_trial: bool,
    terms_for_create: serde_json::Value,
    terms_sig: &str,
) -> serde_json::Value {
    let mut create_body = serde_json::json!({
        "serviceId": params.service_id,
        "useTrial": effective_use_trial,
        "serviceParams": params.service_params,
        "serviceTokenAmount": params.service_token_amount,
        "serviceTokenAddress": params.service_token_address,
        "autoRenew": params.auto_renew,
        "title": params.title,
        "description": params.description,
        "serviceInterval": params.service_interval,
        "terms": terms_for_create,
        "termsSig": terms_sig,
        "deviceList": serde_json::Value::Null,
    });
    if let Some(ref pid) = params.provider_agent_id {
        create_body["providerAgentId"] = serde_json::json!(pid);
    }
    create_body
}

/// The json-mode success envelope data — retains `deviceRoutingDegraded: false`
/// for compatibility, plus the `offlineReplaySupported` capability flag (always
/// present). When the comm package cannot honor an offline-replay preference, it
/// also carries
/// `offlineReplayFixCommands` (the upgrade commands) so the skill can prompt an
/// upgrade. These offline-replay fields are copy-only — they never change whether
/// or how the subscription was created.
fn build_create_success(
    sub_id: &str,
    tx_hash: &str,
    offline_replay: &OfflineReplayCapability,
    autotrade_requested: bool,
    autotrade_configured: bool,
) -> serde_json::Value {
    let mut envelope = serde_json::json!({
        "subId": sub_id,
        "txHash": tx_hash,
        "deviceRoutingDegraded": false,
        "offlineReplaySupported": offline_replay.supported,
        "autoTradeConfigRequested": autotrade_requested,
        "autoTradeConfigured": autotrade_configured,
    });
    if !offline_replay.supported {
        envelope["offlineReplayFixCommands"] =
            serde_json::json!(offline_replay.fix_commands_or_default());
    }
    envelope
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
    let autotrade_requested = params.autotrade_mode.is_some()
        || params.autotrade_amount.is_some()
        || params.autotrade_cap.is_some()
        || params.autotrade_quote.is_some()
        || params.autotrade_environment.is_some()
        || params.autotrade_margin_mode.is_some()
        || params.autotrade_order_policy.is_some()
        || !params.autotrade_required_fields.is_empty();
    let autotrade_config = params.autotrade_config()?;

    let json_mode = params.format.eq_ignore_ascii_case("json");

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

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

    // Step 1: providerConfirmStatus → terms object
    let mut confirm_body = serde_json::json!({
        "serviceId": params.service_id,
        "autoRenew": params.auto_renew,
        "useTrial": params.use_trial,
        "subId": 0,
    });
    if let Some(ref pid) = params.provider_agent_id {
        confirm_body["providerAgentId"] = serde_json::json!(pid);
    }

    let confirm_resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/providerConfirmStatus"),
            &confirm_body,
            &user_agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("providerConfirmStatus failed: {e}"))?;

    if DEBUG_LOG {
        eprintln!("[create-subscribe] providerConfirmStatus response: {confirm_resp}");
    }

    if confirm_resp.is_null() || confirm_resp.as_object().map_or(true, |o| o.is_empty()) {
        bail!("providerConfirmStatus returned empty terms; the service may not support subscription");
    }

    let typed_data = &confirm_resp["typedData"];
    if typed_data.is_null() || typed_data.as_object().map_or(true, |o| o.is_empty()) {
        bail!("providerConfirmStatus response missing typedData");
    }

    // Step 2: EIP-712 sign terms (sign the typedData sub-object, not the full response)
    let terms_sig = signing::sign_typed_data(typed_data, &address).await
        .map_err(|e| anyhow::anyhow!("EIP-712 terms signing failed: {e}"))?;

    if DEBUG_LOG {
        eprintln!("[create-subscribe] termsSig: {terms_sig}");
    }

    // Step 3: POST create
    // terms = providerConfirmStatus response minus the nested typedData
    // (backend expects flat RenewalTerms fields only: asp, aspAgentId, token, subId, user, etc.)
    let mut terms_for_create = confirm_resp.clone();
    if let Some(obj) = terms_for_create.as_object_mut() {
        obj.remove("typedData");
    }

    // useTrial must come from the backend response (the authoritative source), not the
    // user-supplied flag — the backend may override it (e.g. trial already used).
    let effective_use_trial = confirm_resp["useTrial"].as_bool().unwrap_or(params.use_trial);
    if DEBUG_LOG && effective_use_trial != params.use_trial {
        eprintln!(
            "[create-subscribe] useTrial overridden by backend: requested={}, effective={}",
            params.use_trial, effective_use_trial
        );
    }

    let create_body = build_create_body(
        &params,
        effective_use_trial,
        terms_for_create,
        &terms_sig,
    );

    let create_resp = client
        .post_with_identity(
            &format!("{SUBSCRIBE_API_PREFIX}/create"),
            &create_body,
            &user_agent_id,
        )
        .await
        .map_err(|e| anyhow::anyhow!("create-subscribe failed: {e}"))?;

    let sub_id = create_resp["jobId"].as_str().unwrap_or("?").to_string();
    let biz_type = signing::extract_biz_type(&create_resp);
    let uop_data = &create_resp["uopData"];

    if DEBUG_LOG {
        eprintln!("[create-subscribe] subId={sub_id}, bizType={biz_type}");
    }

    // Step 4 + 5: sign uopData → broadcast (reuses the standard task broadcast endpoint)
    let tx_hash = signing::sign_uop_and_broadcast(
        client, uop_data, &account_id, &address, &sub_id, biz_type, &user_agent_id, None,
    ).await?;

    // The jobId exists only after create + broadcast. Persist the MVP default
    // policy together with any final user-authored setup into the same local
    // files used by runtime execution. A write failure cannot roll back the
    // subscription.
    let autotrade_configured = match persist_subscription_autotrade(&sub_id, &autotrade_config) {
        Ok(()) => true,
        Err(err) => {
            eprintln!("[autotrade] subscription created, but execution configuration could not be persisted: {err}");
            false
        }
    };

    // Persist only bounded classifier output. Failure is advisory: the
    // subscription already exists and runtime still has safe shape defaults.
    if !params.service_description.trim().is_empty() {
        if let Err(err) =
            crate::commands::agent_commerce::task::common::autotrade::profile::save_from_description(
                &sub_id,
                &params.service_id,
                params.provider_agent_id.as_deref(),
                &params.service_description,
            )
        {
            eprintln!("[autotrade] could not persist subscription execution hints: {err}");
        }
    }

    audit::log(
        "cli",
        "user/create_subscribe",
        true,
        Duration::default(),
        Some(vec![
            format!("subId={sub_id}"),
            format!("agentId={user_agent_id}"),
            format!("serviceId={}", params.service_id),
            format!("useTrial={effective_use_trial}"),
            format!("autoRenew={}", params.auto_renew),
            format!("autoTradeConfigRequested={autotrade_requested}"),
            format!("autoTradeConfigured={autotrade_configured}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    // Establish the provider XMTP session now (before the first delivery) so
    // subscription deliverables are not held at consent=0. Delivery transport
    // must not depend on the backend compatibility marker.
    if let Some(pid) = params.provider_agent_id.as_deref() {
        super::subscription_ops::ensure_subscription_session(&sub_id, &user_agent_id, pid);
    }

    if json_mode {
        // Copy-only capability probe: read AFTER the create has fully succeeded so
        // its result can never influence whether the write was sent or judged.
        let offline_replay = okx_a2a::probe_offline_replay_capability();
        crate::output::success(build_create_success(
            &sub_id,
            &tx_hash,
            &offline_replay,
            autotrade_requested,
            autotrade_configured,
        ));
        // Balance is verified before create/broadcast; insufficiency exits earlier
        // via the blocked funding-notice envelope.
        if super::content::is_cli_mode() {
            println!();
            println!("{}", super::content::scoped_watch_handoff(&sub_id));
        }
        return Ok(());
    }

    println!("✓ Subscription submitted (transaction broadcast, awaiting on-chain confirmation)");
    println!("  jobId:  {sub_id}");
    println!("  txHash: {tx_hash}");
    if autotrade_requested {
        println!(
            "  Signal execution policy: {}",
            if autotrade_configured { "configured" } else { "configuration pending" }
        );
    }
    if let Some(ref pid) = params.provider_agent_id {
        println!("  Designated provider: {pid}");
    }
    if !super::content::is_cli_mode() {
        println!("Next: wait for the on-chain confirmation; the designated provider will be contacted automatically.");
    }
    if super::content::is_cli_mode() {
        println!();
        println!("{}", super::content::scoped_watch_handoff(&sub_id));
    }

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
                provider_agent_id,
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
                autotrade_required_fields,
                format,
                exclude_device,
            } => {
                assert_eq!(service_id, "svc_001");
                assert!(use_trial);
                assert_eq!(service_token_amount, "10");
                assert_eq!(service_token_address, "0x6776");
                assert_eq!(auto_renew, "1");
                assert_eq!(title, "Signal Subscription");
                assert_eq!(description, "On-chain signal subscription service");
                assert!(provider_agent_id.is_none());
                assert_eq!(service_description, "");
                assert_eq!(service_params, "");
                assert_eq!(service_interval, "month");
                assert!(autotrade_mode.is_none());
                assert!(autotrade_amount.is_none());
                assert!(autotrade_cap.is_none());
                assert!(autotrade_quote.is_none());
                assert!(autotrade_environment.is_none());
                assert!(autotrade_margin_mode.is_none());
                assert!(autotrade_order_policy.is_none());
                assert!(autotrade_required_fields.is_empty());
                assert_eq!(format, "");
                assert!(exclude_device.is_none());
            }
            _ => panic!("expected CreateSubscribe"),
        }
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
                assert_eq!(provider_agent_id.as_deref(), Some("agent-99"));
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
        let cli = TestCli::try_parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_001",
            "--service-token-amount", "10",
            "--service-token-address", "0xAddr",
            "--auto-renew", "1",
            "--title", "t",
            "--description", "d",
            "--exclude-device", "device-2",
        ])
        .expect("legacy flag remains parseable so the command can return a specific error");

        let exclude_device = match cli.cmd {
            super::super::TaskCommand::CreateSubscribe { exclude_device, .. } => exclude_device,
            _ => panic!("expected CreateSubscribe"),
        };
        let mut params = params_fixture(None);
        params.exclude_device = exclude_device;
        let error = params
            .validate()
            .expect_err("create-time device selection must be rejected locally");

        assert!(
            error
                .to_string()
                .contains("create-time device selection is unsupported"),
            "unexpected error: {error}"
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
            title: "t".to_string(),
            description: "d".to_string(),
            provider_agent_id: provider.map(str::to_string),
            service_description: String::new(),
            service_interval: "month".to_string(),
            autotrade_mode: None,
            autotrade_amount: None,
            autotrade_cap: None,
            autotrade_quote: None,
            autotrade_environment: None,
            autotrade_margin_mode: None,
            autotrade_order_policy: None,
            autotrade_required_fields: Vec::new(),
            format: "json".to_string(),
            exclude_device: None,
        }
    }

    #[test]
    fn create_body_defaults_subscription_routing_to_all_devices() {
        // A successful subscription create always uses the backend's default-all
        // routing mode; create-time device selection is not supported.
        let p = params_fixture(None);
        let body = super::build_create_body(&p, false, serde_json::json!({ "asp": "x" }), "0xsig");
        assert_eq!(body["deviceList"], serde_json::Value::Null);
        assert!(body.get("deviceList").is_some());
        assert_eq!(body["termsSig"], serde_json::json!("0xsig"));
        assert!(body.get("providerAgentId").is_none());
    }

    #[test]
    fn create_body_carries_default_all_routing_and_provider_when_present() {
        let p = params_fixture(Some("agent-7"));
        let body = super::build_create_body(&p, true, serde_json::json!({}), "0xsig");
        assert_eq!(body["deviceList"], serde_json::Value::Null);
        assert_eq!(body["providerAgentId"], serde_json::json!("agent-7"));
        assert_eq!(body["useTrial"], serde_json::json!(true));
    }

    #[test]
    fn create_success_envelope_never_reports_device_routing_degradation() {
        use crate::commands::agent_commerce::task::common::okx_a2a::OfflineReplayCapability;
        // Supported comm package ⇒ offlineReplaySupported:true and NO fix-commands field.
        let supported = OfflineReplayCapability {
            supported: true,
            fix_commands: Vec::new(),
        };
        let success = super::build_create_success("0xjob", "0xhash", &supported, true, true);
        assert_eq!(success["deviceRoutingDegraded"], serde_json::json!(false));
        assert_eq!(success["subId"], serde_json::json!("0xjob"));
        assert_eq!(success["txHash"], serde_json::json!("0xhash"));
        assert_eq!(success["offlineReplaySupported"], serde_json::json!(true));
        assert_eq!(success["autoTradeConfigRequested"], serde_json::json!(true));
        assert_eq!(success["autoTradeConfigured"], serde_json::json!(true));
        assert!(success.get("offlineReplayFixCommands").is_none());
        let ok = super::build_create_success("0xjob", "0xhash", &supported, false, false);
        assert_eq!(ok["deviceRoutingDegraded"], serde_json::json!(false));
    }

    #[test]
    fn create_success_envelope_carries_offline_replay_fix_commands_when_unsupported() {
        use crate::commands::agent_commerce::task::common::okx_a2a::OfflineReplayCapability;
        // Unsupported + probe supplied its own fixCommands → passed through verbatim.
        let unsupported = OfflineReplayCapability {
            supported: false,
            fix_commands: vec!["npm i -g @okxweb3/a2a-node@1.2.3".to_string()],
        };
        let env = super::build_create_success("0xjob", "0xhash", &unsupported, false, false);
        assert_eq!(env["offlineReplaySupported"], serde_json::json!(false));
        assert_eq!(
            env["offlineReplayFixCommands"],
            serde_json::json!(["npm i -g @okxweb3/a2a-node@1.2.3"])
        );
        // Unsupported + no probe fixCommands → packaged default.
        let unsupported_default = OfflineReplayCapability {
            supported: false,
            fix_commands: Vec::new(),
        };
        let env2 = super::build_create_success("0xjob", "0xhash", &unsupported_default, false, false);
        assert_eq!(
            env2["offlineReplayFixCommands"],
            serde_json::json!(["npm install -g @okxweb3/a2a-node@latest"])
        );
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
    fn cli_create_subscribe_rejects_removed_copy_trade_argument() {
        assert!(TestCli::try_parse_from([
            "test", "create-subscribe",
            "--service-id", "svc_001",
            "--service-token-amount", "10",
            "--service-token-address", "0x6776",
            "--auto-renew", "1",
            "--copy-trade", "0",
            "--title", "Signal Subscription",
            "--description", "On-chain signal subscription service",
        ]).is_err());
    }

    #[test]
    fn create_body_omits_retired_delivery_marker() {
        let params = CreateSubscribeParams {
            service_id: "svc_report_only".to_string(),
            use_trial: false,
            service_params: String::new(),
            service_token_amount: "10".to_string(),
            service_token_address: "0x6776".to_string(),
            auto_renew: 0,
            title: "Analytics report".to_string(),
            description: "Read-only market report without trading signals".to_string(),
            provider_agent_id: Some("agent-99".to_string()),
            service_description: String::new(),
            service_interval: "month".to_string(),
            autotrade_mode: None,
            autotrade_amount: None,
            autotrade_cap: None,
            autotrade_quote: None,
            autotrade_environment: None,
            autotrade_margin_mode: None,
            autotrade_order_policy: None,
            autotrade_required_fields: Vec::new(),
            format: "json".to_string(),
            exclude_device: None,
        };

        let body = build_create_body(
            &params,
            false,
            serde_json::json!({"subId": 0}),
            "0xsignature",
        );

        assert!(body.get("copyTrade").is_none());
        assert_eq!(body["providerAgentId"], serde_json::json!("agent-99"));
        assert_eq!(body["description"], params.description);
        assert!(body.get("descriptionSummary").is_none());
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
            "--autotrade-mode", "auto",
            "--autotrade-amount", "20.00",
            "--autotrade-cap", "50",
            "--autotrade-quote", "USDT",
            "--autotrade-environment", "demo",
            "--autotrade-margin-mode", "cross",
            "--autotrade-order-policy", "market",
            "--autotrade-required-field", "environment",
            "--autotrade-required-field", "orderPolicy",
            "--autotrade-required-field", "marginMode",
        ]);
        let super::super::TaskCommand::CreateSubscribe {
            autotrade_mode,
            autotrade_amount,
            autotrade_cap,
            autotrade_quote,
            autotrade_environment,
            autotrade_margin_mode,
            autotrade_order_policy,
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
        assert_eq!(
            autotrade_required_fields,
            ["environment", "orderPolicy", "marginMode"]
        );
    }

    #[test]
    fn create_validation_rejects_missing_declared_autotrade_fields() {
        let mut params = params_fixture(None);
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
    fn create_validation_rejects_unknown_required_autotrade_field() {
        let mut params = params_fixture(None);
        params.autotrade_required_fields = vec!["leverage".to_string()];

        assert!(params
            .validate()
            .unwrap_err()
            .to_string()
            .contains("--autotrade-required-field must be one of"));
    }

    #[test]
    fn create_validation_enforces_asp_required_amount_and_cap() {
        let mut params = params_fixture(None);
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
    fn autotrade_config_accepts_partial_fields_and_defaults_auto_usdt() {
        let mut params = params_fixture(None);
        params.autotrade_amount = Some("20".to_string());
        let config = params.autotrade_config().unwrap();
        assert_eq!(config.mode, consent::ConsentMode::Auto);
        assert_eq!(config.amount.as_deref(), Some("20"));
        assert_eq!(config.cap, None);
        assert_eq!(config.quote, "usdt");
        assert_eq!(config.environment, None);
    }

    #[test]
    fn autotrade_config_rejects_non_explicit_trade_environment() {
        let mut params = params_fixture(None);
        params.autotrade_environment = Some("configured".to_string());
        assert!(params
            .validate()
            .unwrap_err()
            .to_string()
            .contains("--autotrade-environment must be one of: live | demo"));
    }

    #[test]
    fn autotrade_config_normalizes_and_does_not_enforce_cap() {
        let mut params = params_fixture(None);
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
    fn autotrade_config_preserves_explicit_manual_and_user_values() {
        let mut params = params_fixture(None);
        params.autotrade_mode = Some("manual".to_string());
        params.autotrade_amount = Some("25.00".to_string());
        params.autotrade_cap = Some("10".to_string());
        params.autotrade_quote = Some("USDC".to_string());

        let config = params.autotrade_config().unwrap();
        assert_eq!(config.mode, consent::ConsentMode::Manual);
        assert_eq!(config.amount.as_deref(), Some("25"));
        assert_eq!(config.cap.as_deref(), Some("10"));
        assert_eq!(config.quote, "usdc");
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
        assert_eq!(stored.margin_mode, config.margin_mode);
        assert_eq!(stored.order_policy, config.order_policy);
        assert!(grants::check_grant("job-subscribe-auto", "dex", "buy", "50").is_ok());
        assert!(grants::check_grant("job-subscribe-auto", "trade_kit", "sell", "50").is_ok());
        assert!(grants::check_grant("job-subscribe-auto", "trade_kit", "sell", "51").is_ok());

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }
}
