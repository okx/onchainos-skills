//! Create a subscription task.
//!
//! Flow: providerConfirmStatus → EIP-712 sign terms → create → sign uopData → broadcast(bizType=101)

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a::{self, OfflineReplayCapability};
use crate::commands::agent_commerce::task::common::subscription_identity::select_subscription_agent_id;
use crate::commands::agent_commerce::task::common::{self, DEBUG_LOG};
use crate::commands::agent_commerce::task::signing;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

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
    /// Provider service Guide retained locally before the subscription broadcast.
    pub service_guide: Option<String>,
    /// Optional provider-supplied SHA-256 of `service_guide`.
    pub service_guide_hash: Option<String>,
    /// Declarative Guide contract for consent/signal fields and bounded tool bindings.
    pub autotrade_guide_semantics_json: Option<String>,
    /// User-confirmed values keyed exclusively by the matching Guide's
    /// `consentFields` declaration.
    pub guide_consent_json: Option<String>,
    pub service_interval: String,
    pub format: String,
    pub exclude_device: Option<Vec<String>>,
}

const MAX_TITLE_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 4096;

impl CreateSubscribeParams {
    fn guide_execution_requested(&self) -> bool {
        self.service_guide.is_some()
            || self.service_guide_hash.is_some()
            || self.autotrade_guide_semantics_json.is_some()
            || self.guide_consent_json.is_some()
    }

    fn guide_draft(&self) -> Result<Option<super::super::common::autotrade::guide::GuideDraft>> {
        super::super::common::autotrade::guide::parse_draft(
            self.service_guide.as_deref(),
            self.service_guide_hash.as_deref(),
            self.autotrade_guide_semantics_json.as_deref(),
        )
    }

    fn guide_consent_values(&self) -> Result<BTreeMap<String, serde_json::Value>> {
        let Some(raw) = self.guide_consent_json.as_deref() else {
            return Ok(BTreeMap::new());
        };
        serde_json::from_str(raw)
            .context("--guide-consent-json must be a JSON object keyed by Guide consentFields")
    }

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
            bail!(
                "--auto-renew must be 0 (off) or 1 (on), got {}",
                self.auto_renew
            );
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
        let guide_draft = self.guide_draft()?;
        if self.guide_execution_requested() && guide_draft.is_none() {
            bail!("guide-driven signal execution requires --service-guide and --autotrade-guide-semantics-json");
        }
        if let Some(guide_draft) = guide_draft {
            super::super::common::autotrade::guide::validate_consent_values(
                &guide_draft.semantics,
                &self.guide_consent_values()?,
            )?;
        }
        Ok(())
    }

}

fn prepare_guide_execution(job_id: &str, params: &CreateSubscribeParams) -> Result<()> {
    let guide_draft = params
        .guide_draft()?
        .ok_or_else(|| anyhow::anyhow!("service Guide is required for signal execution"))?;
    let guide_file = guide_draft.clone().into_file(
        job_id,
        &params.service_id,
        params.provider_agent_id.as_deref(),
    );
    super::super::common::autotrade::guide::write_guide(&guide_file, &guide_draft.source)?;
    super::super::common::autotrade::guide::write_prepared_consent(
        job_id,
        &guide_file,
        params.guide_consent_values()?,
        super::super::common::autotrade::DEFAULT_AUTOTRADE_TTL_SEC,
    )?;
    Ok(())
}

fn activate_guide_execution(job_id: &str) -> Result<()> {
    super::super::common::autotrade::guide::activate_prepared_consent(job_id)
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
    guide_execution_requested: bool,
    guide_execution_configured: bool,
) -> serde_json::Value {
    let mut envelope = serde_json::json!({
        "subId": sub_id,
        "txHash": tx_hash,
        "deviceRoutingDegraded": false,
        "offlineReplaySupported": offline_replay.supported,
        "guideExecutionRequested": guide_execution_requested,
        "guideExecutionConfigured": guide_execution_configured,
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
    let guide_execution_requested = params.guide_execution_requested();

    let json_mode = params.format.eq_ignore_ascii_case("json");

    ensure_tokens_refreshed().await.map_err(|e| {
        anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}")
    })?;

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
        )
        .await?;
    if let Some(existing) = super::subscription_ops::existing_subscription_for_service(
        &existing_subscriptions,
        &params.service_id,
    ) {
        return Err(crate::output::CliDuplicateSubscription {
            data: build_duplicate_subscription_block(&params.service_id, existing),
        }
        .into());
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
        bail!(
            "providerConfirmStatus returned empty terms; the service may not support subscription"
        );
    }

    let typed_data = &confirm_resp["typedData"];
    if typed_data.is_null() || typed_data.as_object().map_or(true, |o| o.is_empty()) {
        bail!("providerConfirmStatus response missing typedData");
    }

    // Step 2: EIP-712 sign terms (sign the typedData sub-object, not the full response)
    let terms_sig = signing::sign_typed_data(typed_data, &address)
        .await
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
    let effective_use_trial = confirm_resp["useTrial"]
        .as_bool()
        .unwrap_or(params.use_trial);
    if DEBUG_LOG && effective_use_trial != params.use_trial {
        eprintln!(
            "[create-subscribe] useTrial overridden by backend: requested={}, effective={}",
            params.use_trial, effective_use_trial
        );
    }

    let create_body = build_create_body(&params, effective_use_trial, terms_for_create, &terms_sig);

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

    // Create time is the only safe point to bind the provider Guide and the
    // subscriber's consent to the real backend `jobId`, while still allowing a
    // local persistence failure to stop before signing/broadcasting.
    let prepared_guide_execution = if guide_execution_requested {
        prepare_guide_execution(&sub_id, &params)?;
        true
    } else {
        false
    };

    // Bind the subscription job to the current AI runtime before broadcast.
    // The on-chain creation event can be consumed while broadcast is still
    // returning, so the provider mapping must already exist at that point.
    let provider_prebind = common::a2a_binding::bind_job_provider_to_current_runtime(&sub_id).await;

    // Step 4 + 5: sign uopData → broadcast (reuses the standard task broadcast endpoint)
    let tx_hash = match signing::sign_uop_and_broadcast(
        client,
        uop_data,
        &account_id,
        &address,
        &sub_id,
        biz_type,
        &user_agent_id,
        None,
    )
    .await
    {
        Ok(tx_hash) => tx_hash,
        Err(err) => {
            if prepared_guide_execution {
                super::super::common::autotrade::guide::abort_prepared_consent(&sub_id);
            }
            if let Some(prebind) = &provider_prebind {
                prebind.rollback_if_created().await;
            }
            return Err(err);
        }
    };

    // A prepared Guide Consent becomes executable only after the subscription has been
    // broadcast. Activation failure leaves it prepared, so delivery handling
    // remains fail-closed even though the remote subscription now exists.
    let guide_execution_configured = if prepared_guide_execution {
        match activate_guide_execution(&sub_id) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("[guide-execution] subscription created, but Guide Consent could not be activated: {err}");
                false
            }
        }
    } else {
        false
    };

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
            format!("guideExecutionRequested={guide_execution_requested}"),
            format!("guideExecutionConfigured={guide_execution_configured}"),
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
            guide_execution_requested,
            guide_execution_configured,
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
    if guide_execution_requested {
        println!(
            "  Signal execution policy: {}",
            if guide_execution_configured {
                "configured"
            } else {
                "configuration pending"
            }
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
            "test",
            "create-subscribe",
            "--service-id",
            "svc_001",
            "--use-trial",
            "--service-token-amount",
            "10",
            "--service-token-address",
            "0x6776",
            "--auto-renew",
            "1",
            "--title",
            "Signal Subscription",
            "--description",
            "On-chain signal subscription service",
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
                service_guide,
                service_guide_hash,
                autotrade_guide_semantics_json,
                guide_consent_json,
                service_params,
                service_interval,
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
                assert!(service_guide.is_none());
                assert!(service_guide_hash.is_none());
                assert!(autotrade_guide_semantics_json.is_none());
                assert!(guide_consent_json.is_none());
                assert_eq!(service_params, "");
                assert_eq!(service_interval, "month");
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
        assert!(active["userFacingPrompt"]
            .as_str()
            .unwrap()
            .contains("jobId: job-active"));
        assert!(active["userFacingPrompt"]
            .as_str()
            .unwrap()
            .contains("cannot be created again"));
        assert!(!active["userFacingPrompt"]
            .as_str()
            .unwrap()
            .contains("ACTIVE"));
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
        assert!(!rejected["userFacingPrompt"]
            .as_str()
            .unwrap()
            .contains("Restore listening"));
        assert!(!rejected["userFacingPrompt"]
            .as_str()
            .unwrap()
            .contains("REJECTED"));
        assert!(rejected.get("serviceId").is_none());
    }

    #[test]
    fn cli_create_subscribe_with_provider() {
        let cli = TestCli::parse_from([
            "test",
            "create-subscribe",
            "--service-id",
            "svc_002",
            "--service-token-amount",
            "5",
            "--service-token-address",
            "0xAddr",
            "--auto-renew",
            "0",
            "--title",
            "Copy Trade",
            "--description",
            "Auto copy trade subscription",
            "--provider-agent-id",
            "agent-99",
        ]);
        match cli.cmd {
            super::super::TaskCommand::CreateSubscribe {
                provider_agent_id,
                use_trial,
                ..
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
            "test",
            "create-subscribe",
            "--service-id",
            "svc_003",
            "--service-token-amount",
            "1",
            "--service-token-address",
            "0xA",
            "--auto-renew",
            "true",
            "--title",
            "t",
            "--description",
            "d for test bool strings ok",
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
            "test",
            "create-subscribe",
            "--service-token-amount",
            "10",
            "--service-token-address",
            "0xAddr",
            "--auto-renew",
            "1",
            "--title",
            "t",
            "--description",
            "d",
        ])
        .is_err());
    }

    #[test]
    fn cli_create_subscribe_rejects_create_time_device_selection() {
        let cli = TestCli::try_parse_from([
            "test",
            "create-subscribe",
            "--service-id",
            "svc_001",
            "--service-token-amount",
            "10",
            "--service-token-address",
            "0xAddr",
            "--auto-renew",
            "1",
            "--title",
            "t",
            "--description",
            "d",
            "--exclude-device",
            "device-2",
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

    // The backend create response is the first point where the subscription has
    // a jobId. Bind that job to the current runtime before broadcasting so the
    // on-chain creation event cannot race ahead of local provider routing.
    #[test]
    fn create_subscribe_binds_job_provider_before_broadcast() {
        let source = include_str!("create_subscribe.rs");
        let handler = source
            .split_once("pub async fn handle_create_subscribe")
            .expect("create-subscribe handler must exist")
            .1
            .split_once("#[cfg(test)]")
            .expect("handler must precede its tests")
            .0;

        let job_id = handler
            .find("let sub_id = create_resp[\"jobId\"]")
            .expect("handler must read jobId from the create response");
        let bind = handler
            .find("bind_job_provider_to_current_runtime(&sub_id)")
            .expect("handler must bind the subscription job to the current runtime");
        let broadcast = handler
            .find("signing::sign_uop_and_broadcast(")
            .expect("handler must broadcast the subscription transaction");
        let rollback = handler
            .find("prebind.rollback_if_created().await")
            .expect("handler must roll back a newly-created binding when broadcast fails");

        assert!(job_id < bind, "jobId must be resolved before bind-current");
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
            title: "t".to_string(),
            description: "d".to_string(),
            provider_agent_id: provider.map(str::to_string),
            service_description: String::new(),
            service_guide: None,
            service_guide_hash: None,
            autotrade_guide_semantics_json: None,
            guide_consent_json: None,
            service_interval: "month".to_string(),
            format: "json".to_string(),
            exclude_device: None,
        }
    }

    fn attach_minimal_guide(params: &mut super::CreateSubscribeParams) {
        params.service_guide = Some("Use only the declared operation.".to_string());
        params.autotrade_guide_semantics_json =
            Some(r#"{"execution":{"toolId":"onchainos","operation":"swap"}}"#.to_string());
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
        assert_eq!(success["guideExecutionRequested"], serde_json::json!(true));
        assert_eq!(success["guideExecutionConfigured"], serde_json::json!(true));
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
        let env2 =
            super::build_create_success("0xjob", "0xhash", &unsupported_default, false, false);
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
            "test",
            "create-subscribe",
            "--service-id",
            "svc_001",
            "--service-token-amount",
            "10",
            "--service-token-address",
            "0x6776",
            "--auto-renew",
            "1",
            "--copy-trade",
            "0",
            "--title",
            "Signal Subscription",
            "--description",
            "On-chain signal subscription service",
        ])
        .is_err());
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
            service_guide: None,
            service_guide_hash: None,
            autotrade_guide_semantics_json: None,
            guide_consent_json: None,
            service_interval: "month".to_string(),
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
    fn cli_create_subscribe_accepts_guide_defined_consent_values() {
        let cli = TestCli::parse_from([
            "test",
            "create-subscribe",
            "--service-id",
            "svc_auto",
            "--service-token-amount",
            "1",
            "--service-token-address",
            "0xA",
            "--auto-renew",
            "1",
            "--title",
            "Signals",
            "--description",
            "Execute the delivered signals",
            "--service-guide",
            "guide body",
            "--autotrade-guide-semantics-json",
            r#"{"consentFields":[{"key":"strategyArmed","required":true,"type":"boolean"}],"signalFields":[{"key":"units","required":true,"type":"decimal"}],"execution":{"toolId":"onchainos","operation":"swap","authorizationParameter":"size","bindings":[{"parameter":"size","source":"signal.units"}]}}"#,
            "--guide-consent-json",
            r#"{"strategyArmed":true}"#,
        ]);
        let super::super::TaskCommand::CreateSubscribe {
            guide_consent_json,
            ..
        } = cli.cmd
        else {
            panic!("expected CreateSubscribe");
        };
        assert_eq!(guide_consent_json.as_deref(), Some(r#"{"strategyArmed":true}"#));
    }

    #[test]
    fn cli_create_subscribe_help_documents_required_field_contract() {
        let help = match TestCli::try_parse_from(["test", "create-subscribe", "--help"]) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("--help must exit through clap"),
        };

        for expected in ["--guide-consent-json", "matching Guide's", "declared consent fields"] {
            assert!(
                help.contains(expected),
                "help must contain {expected:?}: {help}"
            );
        }
    }

    #[test]
    fn prepared_guide_consent_activates_only_after_broadcast() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("create_subscribe_guide");
        if home.exists() {
            std::fs::remove_dir_all(&home).ok();
        }
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &home);

        let mut params = params_fixture(None);
        params.service_guide = Some("Place the declared operation only.".to_string());
        params.autotrade_guide_semantics_json =
            Some(r#"{"consentFields":[{"key":"strategyArmed","required":true,"type":"boolean"}],"signalFields":[{"key":"pairCode","required":true,"type":"string"},{"key":"units","required":true,"type":"decimal"}],"execution":{"toolId":"onchainos","operation":"swap","authorizationParameter":"size","conditions":[{"source":"consent.strategyArmed","equals":true}],"bindings":[{"parameter":"instrument","source":"signal.pairCode"},{"parameter":"size","source":"signal.units"}]}}"#.to_string());
        params.guide_consent_json = Some(r#"{"strategyArmed":true}"#.to_string());
        prepare_guide_execution("job-subscribe-guide", &params).unwrap();
        assert!(home
            .join("autotrade")
            .join("guide")
            .join("job-subscribe-guide.md")
            .is_file());
        assert!(home
            .join("autotrade")
            .join("consent")
            .join("job-subscribe-guide.md")
            .is_file());
        assert_eq!(
            crate::commands::agent_commerce::task::common::autotrade::guide::consent_snapshot("job-subscribe-guide").status,
            "unavailable"
        );
        activate_guide_execution("job-subscribe-guide").unwrap();
        assert_eq!(
            crate::commands::agent_commerce::task::common::autotrade::guide::consent_snapshot("job-subscribe-guide").status,
            "active"
        );

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }
}
