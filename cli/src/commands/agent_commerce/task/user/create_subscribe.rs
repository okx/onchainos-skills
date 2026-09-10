//! Create a subscription task.
//!
//! Flow: providerConfirmStatus → EIP-712 sign terms → createSubscription → local readiness → broadcast(bizType=204)

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a;
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
    pub attachments: Option<Vec<String>>,
    pub provider_agent_id: String,
    /// Provider service Guide retained locally before the subscription broadcast.
    pub service_guide: Option<String>,
    /// Optional provider-supplied SHA-256 of `service_guide`.
    pub service_guide_hash: Option<String>,
    /// User-confirmed values for the matching Guide.
    pub guide_consent_json: Option<String>,
    pub service_interval: String,
    pub format: String,
}

const MAX_TITLE_CHARS: usize = 30;
const MAX_DESCRIPTION_CHARS: usize = 4096;

/// The validated, ephemeral Guide + Consent input for one subscription create.
///
/// This is deliberately not a subscription state. A created subscription only
/// exposes whether its persisted Guide and Consent are both active.
#[derive(Debug)]
struct GuideConsentInput {
    draft: super::super::common::autotrade::guide::GuideDraft,
    consent_values: BTreeMap<String, serde_json::Value>,
}

impl CreateSubscribeParams {
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
        let guide_draft = self.guide_draft()?;
        let Some(draft) = guide_draft else {
            if self.service_guide_hash.is_some() || self.guide_consent_json.is_some() {
                bail!("guide-driven signal execution requires --service-guide");
            }
            return Ok(None);
        };
        if self.guide_consent_json.is_none() {
            bail!("guide-driven signal execution requires --guide-consent-json, including {{}} when the Guide declares no consent fields");
        }
        let consent_values = self.guide_consent_values()?;
        super::super::common::autotrade::guide::validate_consent_values(&consent_values)?;
        Ok(Some(GuideConsentInput {
            draft,
            consent_values,
        }))
    }

    fn validate(&self) -> Result<Option<GuideConsentInput>> {
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
        self.validated_guide_consent()
    }
}

fn prepare_guide_consent(
    job_id: &str,
    params: &CreateSubscribeParams,
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
    )?;
    Ok(())
}

fn activate_guide_consent(job_id: &str) -> Result<()> {
    super::super::common::autotrade::guide::activate_prepared_consent(job_id)
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
    let guide_consent = params.validate()?;

    ensure_tokens_refreshed().await.map_err(|e| {
        anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}")
    })?;
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

    let execution_mode = super::super::common::autotrade::subscription_config::execution_mode(
        &user_agent_id,
        &params.service_id,
    )?
    .ok_or_else(|| {
        anyhow::anyhow!(
            "subscription automatic-copy preference is required; collect the user's Guide answers and save the preference before create-subscribe"
        )
    })?;
    if execution_mode
        == super::super::common::autotrade::subscription_config::ExecutionMode::GuideDirect
        && guide_consent.is_none()
    {
        bail!(
            "automatic copy-trading requires --service-guide and --guide-consent-json before create-subscribe"
        );
    }

    // Repeat the selection-time duplicate and balance checks immediately
    // before the V2 subscription write boundary.
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

    if let Some(insufficient) =
        subscribe_balance_shortfall(&params.service_token_amount, &params.service_token_address)
            .await?
    {
        let deposit = common::deposit_qr::resolve_current_deposit_info(&user_agent_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("failed to resolve the funding address"))?;
        // Keep subscription creation on the same funding contract as the
        // one-time task create-and-fund flow. In particular, funding is a
        // successful structured response, not a CLI error envelope.
        crate::output::success(super::create::build_task_creation_funding_result(
            &insufficient,
            &deposit,
            &params.service_token_address,
        )?);
        return Ok(());
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
            if let Some(ref guide_consent) = guide_consent {
                prepare_guide_consent(job_id, &params, guide_consent)?;
            }
            Ok(())
        },
    )
    .await?;

    let offline_replay = okx_a2a::probe_offline_replay_capability();
    let tx_hash = receipt.broadcast["txHash"].as_str().unwrap_or("pending");
    let guide_and_consent_active = if guide_consent.is_some() {
        match activate_guide_consent(&receipt.job_id) {
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
            format!("jobId={}", receipt.job_id),
            format!("agentId={user_agent_id}"),
            format!("serviceId={}", params.service_id),
            format!("useTrial={}", receipt.effective_use_trial),
            format!("autoRenew={}", params.auto_renew),
            "bizType=204".to_string(),
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

    let mut payload = serde_json::json!({
        "jobId": receipt.job_id,
        "type": 204,
        "bizType": 204,
        "status": "broadcast_submitted",
        "providerAgentId": params.provider_agent_id,
        "serviceId": params.service_id,
        "useTrial": receipt.effective_use_trial,
        "autoRenew": params.auto_renew,
        "runtimeBound": true,
        "attachments": receipt.attachments,
        "guideStatus": if guide_and_consent_active { "active" } else { "none" },
        "consentStatus": if guide_and_consent_active { "active" } else { "none" },
        "executionProfileSaved": guide_and_consent_active,
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

async fn subscribe_balance_shortfall(
    service_token_amount: &str,
    service_token_address: &str,
) -> Result<Option<common::deposit_qr::InsufficientBalanceError>> {
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
        Ok(symbol) => symbol,
        Err(error) => {
            if DEBUG_LOG {
                eprintln!(
                    "[create-subscribe] token symbol resolution failed; skipping balance pre-check: {error}"
                );
            }
            return Ok(None);
        }
    };

    match common::ensure_sufficient_balance(required, &symbol).await {
        Ok(()) => Ok(None),
        Err(error) => match error.downcast_ref::<common::deposit_qr::InsufficientBalanceError>() {
            Some(insufficient) => Ok(Some(insufficient.clone())),
            None => Err(error),
        },
    }
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
            "--provider-agent-id",
            "asp-1",
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
                service_guide,
                service_guide_hash,
                guide_consent_json,
                service_params,
                service_interval,
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
                assert!(service_guide.is_none());
                assert!(service_guide_hash.is_none());
                assert!(guide_consent_json.is_none());
                assert_eq!(service_params, "");
                assert_eq!(service_interval, "month");
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
                assert_eq!(provider_agent_id, "agent-99");
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
            "--provider-agent-id",
            "asp-1",
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
        assert!(TestCli::try_parse_from([
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
            "--provider-agent-id",
            "asp-1",
            "--exclude-device",
            "device-2",
        ])
        .is_err());
    }

    // The backend create response is the first point where the subscription has
    // a jobId. Try to bind that job to the current runtime before broadcasting,
    // but do not block the broadcast when the local binding is unavailable.
    #[test]
    fn create_subscribe_treats_job_provider_binding_as_best_effort() {
        let source = include_str!("v2/create_subscription.rs");
        let job_id = source
            .find("validate_create_response(&response)")
            .expect("v2 create must validate jobId from the create response");
        let readiness = source
            .find("establish_local_readiness(&job_id)")
            .expect("v2 create must establish local execution readiness");
        let bind = source
            .find("bind_job_provider_to_current_runtime(&job_id)")
            .expect("v2 create must attempt runtime binding without requiring it");
        let broadcast = source
            .find("signing::sign_uop_and_broadcast_full(")
            .expect("v2 create must broadcast the subscription transaction");
        let rollback = source
            .find("if let Some(prebind) = prebind.as_ref()")
            .expect("handler must roll back a newly-created binding when broadcast fails");

        assert!(
            !source.contains("bind_job_provider_to_current_runtime_required(&job_id)"),
            "subscription creation must not block on runtime binding"
        );
        assert!(
            job_id < readiness,
            "jobId must be resolved before local readiness"
        );
        assert!(
            readiness < bind,
            "local readiness must precede runtime binding"
        );
        assert!(
            bind < broadcast,
            "best-effort runtime binding must be attempted before broadcast"
        );
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
            attachments: None,
            provider_agent_id: provider.unwrap_or_default().to_string(),
            service_guide: None,
            service_guide_hash: None,
            guide_consent_json: None,
            service_interval: "month".to_string(),
            format: "json".to_string(),
        }
    }

    fn attach_minimal_guide(params: &mut super::CreateSubscribeParams) {
        params.service_guide =
            Some("Follow the saved Signal using only the confirmed Consent.".to_string());
        params.guide_consent_json = Some("{}".to_string());
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
        assert!(error
            .to_string()
            .contains("attachment file is not readable"));
    }

    #[test]
    fn subscription_funding_block_uses_common_funding_contract() {
        let insufficient = common::deposit_qr::InsufficientBalanceError::new(
            "insufficient".to_string(),
            "USDT",
            0.0001,
            0.0,
        );
        let deposit = common::deposit_qr::deposit_info_for_address(
            "0x1234567890abcdef1234567890abcdef12345678",
        );
        let output = super::super::create::build_task_creation_funding_result(
            &insufficient,
            &deposit,
            "0x779ded0c9e1022225f8e0630b35a9b54be713736",
        )
        .expect("common Funding result");
        assert_eq!(output["phase"], "funding_required");
        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "insufficient_balance");
        assert_eq!(output["nextAction"], serde_json::json!([]));
        assert_eq!(output["payload"]["operation"], "task_creation");
        assert_eq!(output["payload"]["fundingNeed"]["required"], "0.0001");
        assert!(output["payload"]["qr"].is_object());
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
            "--provider-agent-id",
            "asp-1",
        ])
        .is_err());
    }

    #[test]
    fn create_without_guide_has_no_execution_profile() {
        let params = CreateSubscribeParams {
            service_id: "svc_report_only".to_string(),
            use_trial: false,
            service_params: String::new(),
            service_token_amount: "10".to_string(),
            service_token_address: "0x6776".to_string(),
            auto_renew: 0,
            title: "Analytics report".to_string(),
            description: "Read-only market report without trading signals".to_string(),
            attachments: None,
            provider_agent_id: "agent-99".to_string(),
            service_guide: None,
            service_guide_hash: None,
            guide_consent_json: None,
            service_interval: "month".to_string(),
            format: "json".to_string(),
        };
        assert!(params.validate().is_ok());
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
            "--provider-agent-id",
            "asp-1",
            "--service-guide",
            "guide body",
            "--guide-consent-json",
            r#"{"strategyArmed":true}"#,
        ]);
        let super::super::TaskCommand::CreateSubscribe {
            guide_consent_json, ..
        } = cli.cmd
        else {
            panic!("expected CreateSubscribe");
        };
        assert_eq!(
            guide_consent_json.as_deref(),
            Some(r#"{"strategyArmed":true}"#)
        );
    }

    #[test]
    fn cli_create_subscribe_help_documents_required_field_contract() {
        let help = match TestCli::try_parse_from(["test", "create-subscribe", "--help"]) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("--help must exit through clap"),
        };

        for expected in ["--guide-consent-json", "matching Guide"] {
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

        let mut params = params_fixture(Some("asp-1"));
        params.service_guide =
            Some("Place only as directed by this Guide and the saved Signal.".to_string());
        params.guide_consent_json = Some(r#"{"strategyArmed":true}"#.to_string());
        let consent = params.validate().unwrap().expect("Guide Consent input");
        prepare_guide_consent("job-subscribe-guide", &params, &consent).unwrap();
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
            crate::commands::agent_commerce::task::common::autotrade::guide::consent_snapshot(
                "job-subscribe-guide"
            )
            .status,
            "unavailable"
        );
        activate_guide_consent("job-subscribe-guide").unwrap();
        assert_eq!(
            crate::commands::agent_commerce::task::common::autotrade::guide::consent_snapshot(
                "job-subscribe-guide"
            )
            .status,
            "active"
        );

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn guide_consent_requires_explicit_json_even_when_empty() {
        let mut params = params_fixture(Some("asp-1"));
        attach_minimal_guide(&mut params);
        params.guide_consent_json = None;

        let error = params
            .validate()
            .expect_err("Guide bundle needs explicit Consent");
        assert!(
            error.to_string().contains("--guide-consent-json"),
            "unexpected error: {error}"
        );
    }
}
