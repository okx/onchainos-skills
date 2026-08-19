//! Create a subscription task.
//!
//! Flow: providerConfirmStatus → EIP-712 sign terms → create → sign uopData → broadcast(bizType=101)

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::autotrade::{
    amount::Decimal, consent, grants,
};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a::{self, OfflineReplayCapability};
use crate::commands::agent_commerce::task::common::{self, DEBUG_LOG};
use crate::commands::agent_commerce::task::signing;

pub(crate) const SUBSCRIBE_API_PREFIX: &str = "/priapi/v1/aieco/task/subscribe";
/// Compatibility marker required by the current subscription API. It enables
/// delivery routing only; runtime parsing, consent, cap and tool checks remain
/// authoritative for whether a delivery can execute.
const SUBSCRIPTION_DELIVERY_ENABLED: i32 = 1;

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
    pub format: String,
    /// Device ids to omit from the default all-devices routing set (repeatable).
    pub exclude_device: Option<Vec<String>>,
}

const MAX_TITLE_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 4096;
const SUBSCRIPTION_AUTOTRADE_TTL_SEC: u64 = 31_536_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SubscriptionAutoTradeConfig {
    amount: String,
    cap: String,
    quote: String,
}

impl CreateSubscribeParams {
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
        self.autotrade_config()?;
        Ok(())
    }

    fn autotrade_config(&self) -> Result<Option<SubscriptionAutoTradeConfig>> {
        let has_policy_field = self.autotrade_amount.is_some()
            || self.autotrade_cap.is_some()
            || self.autotrade_quote.is_some();
        let Some(mode) = self.autotrade_mode.as_deref() else {
            if has_policy_field {
                bail!("--autotrade-mode auto is required when automatic execution fields are supplied");
            }
            return Ok(None);
        };
        if !mode.eq_ignore_ascii_case("auto") {
            bail!("--autotrade-mode currently supports only: auto");
        }

        let mut missing = Vec::new();
        if self.autotrade_amount.as_deref().map_or(true, str::is_empty) {
            missing.push("--autotrade-amount");
        }
        if self.autotrade_cap.as_deref().map_or(true, str::is_empty) {
            missing.push("--autotrade-cap");
        }
        if self.autotrade_quote.as_deref().map_or(true, str::is_empty) {
            missing.push("--autotrade-quote");
        }
        if !missing.is_empty() {
            bail!(
                "automatic signal execution is missing required fields: {}",
                missing.join(", ")
            );
        }

        let amount = self.autotrade_amount.as_deref().unwrap_or_default();
        let cap = self.autotrade_cap.as_deref().unwrap_or_default();
        let parsed_amount = Decimal::parse(amount)
            .map_err(|_| anyhow::anyhow!("--autotrade-amount must be a positive decimal"))?;
        let parsed_cap = Decimal::parse(cap)
            .map_err(|_| anyhow::anyhow!("--autotrade-cap must be a positive decimal"))?;
        if parsed_amount.is_zero() {
            bail!("--autotrade-amount must be greater than 0");
        }
        if parsed_cap.is_zero() {
            bail!("--autotrade-cap must be greater than 0");
        }
        if !parsed_amount.le(&parsed_cap) {
            bail!("--autotrade-amount must not exceed --autotrade-cap");
        }
        let quote = self
            .autotrade_quote
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !consent::QUOTE_WHITELIST.contains(&quote.as_str()) {
            bail!("--autotrade-quote must be one of: usdt | usdc");
        }

        Ok(Some(SubscriptionAutoTradeConfig {
            amount: parsed_amount.to_plain_string(),
            cap: parsed_cap.to_plain_string(),
            quote,
        }))
    }
}

fn persist_subscription_autotrade(
    job_id: &str,
    config: &SubscriptionAutoTradeConfig,
) -> Result<()> {
    consent::write_consent_with_trade_amount(
        job_id,
        consent::ConsentMode::Auto,
        Some(&config.cap),
        Some(&config.amount),
        Some(&config.quote),
        SUBSCRIPTION_AUTOTRADE_TTL_SEC,
    )?;
    if let Err(err) = grants::write_cap_grant(job_id, &config.cap, SUBSCRIPTION_AUTOTRADE_TTL_SEC) {
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
    device_list: &[String],
) -> serde_json::Value {
    let mut create_body = serde_json::json!({
        "serviceId": params.service_id,
        "useTrial": effective_use_trial,
        "serviceParams": params.service_params,
        "serviceTokenAmount": params.service_token_amount,
        "serviceTokenAddress": params.service_token_address,
        "autoRenew": params.auto_renew,
        "copyTrade": SUBSCRIPTION_DELIVERY_ENABLED,
        "title": params.title,
        "description": params.description,
        "serviceInterval": params.service_interval,
        "terms": terms_for_create,
        "termsSig": terms_sig,
        "deviceList": device_list,
    });
    if let Some(ref pid) = params.provider_agent_id {
        create_body["providerAgentId"] = serde_json::json!(pid);
    }
    create_body
}

/// The json-mode success envelope data — always carries the `deviceRoutingDegraded`
/// marker so the skill can render the degraded notice without a second query, plus
/// the `offlineReplaySupported` capability flag (always present). When the comm
/// package cannot honor an offline-replay preference, also carries
/// `offlineReplayFixCommands` (the upgrade commands) so the skill can prompt an
/// upgrade. These offline-replay fields are copy-only — they never change whether
/// or how the subscription was created.
fn build_create_success(
    sub_id: &str,
    tx_hash: &str,
    degraded: bool,
    offline_replay: &OfflineReplayCapability,
    autotrade_requested: bool,
    autotrade_configured: bool,
) -> serde_json::Value {
    let mut envelope = serde_json::json!({
        "subId": sub_id,
        "txHash": tx_hash,
        "deviceRoutingDegraded": degraded,
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

pub async fn handle_create_subscribe(
    client: &mut TaskApiClient,
    params: CreateSubscribeParams,
) -> Result<()> {
    params.validate()?;
    let autotrade_config = params.autotrade_config()?;

    let json_mode = params.format.eq_ignore_ascii_case("json");

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (user_agent_id, _) = super::create::resolve_user_agent().await?;
    if DEBUG_LOG {
        eprintln!("[create-subscribe] user identity check passed (agentId: {user_agent_id})");
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

    // Resolve the receive-device routing set: default = all logged-in devices
    // (device-list paged to completion) minus any --exclude-device. If that query
    // fails or returns empty, degrade to this device only and mark the result —
    // never abort the create. deviceList is ALWAYS sent explicitly so the
    // created record never depends on server-default semantics.
    let excluded = params.exclude_device.clone().unwrap_or_default();
    let fetched = super::device_routing::fetch_all_device_ids(client, &user_agent_id)
        .await
        .ok();
    let this_device_id = crate::device::id::get_cached_device_id();
    let (device_list, device_routing_degraded) =
        super::device_routing::resolve_create_device_set(fetched, &excluded, this_device_id);
    if DEBUG_LOG {
        eprintln!(
            "[create-subscribe] deviceList={device_list:?} degraded={device_routing_degraded}"
        );
    }

    let create_body = build_create_body(
        &params,
        effective_use_trial,
        terms_for_create,
        &terms_sig,
        &device_list,
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

    // Blocking balance pre-check (the "6th" balance checkpoint). Unlike
    // create-task — which only *registers* the task and involves no transfer, so
    // an advisory (non-blocking) warning is appropriate — create-subscribe's
    // broadcast performs an *immediate* ERC20 token transfer. An insufficient
    // business-token balance must therefore block *before* broadcast: the
    // previous advisory mode continued to broadcast, the transfer reverted
    // on-chain as an opaque `estimateGas` error ("ERC20: transfer amount exceeds
    // balance") propagated via `?`, and the already-computed deposit
    // address + QR were dropped (JSON output missing depositAddress; non-TTY
    // runtimes got no output at all). On insufficiency this bails the enriched
    // `InsufficientBalanceError`, which `main.rs` downcasts into the structured
    // `error_insufficient_balance` JSON envelope (carrying `depositAddress`); the
    // agent playbook reads that field and calls `onchainos wallet qrcode` to
    // render the QR in non-TTY runtimes.
    ensure_subscribe_balance(
        &params.service_token_amount,
        &params.service_token_address,
        &user_agent_id,
    )
    .await?;

    // Step 4 + 5: sign uopData → broadcast (reuses the standard task broadcast endpoint)
    let tx_hash = signing::sign_uop_and_broadcast(
        client, uop_data, &account_id, &address, &sub_id, biz_type, &user_agent_id, None,
    ).await?;

    // The jobId exists only after create + broadcast. Persist the user's final,
    // explicit subscription setup into the same fail-closed policy files used
    // by runtime execution. A write failure cannot roll back the subscription.
    let autotrade_requested = autotrade_config.is_some();
    let autotrade_configured = match autotrade_config.as_ref() {
        Some(config) => match persist_subscription_autotrade(&sub_id, config) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("[autotrade] subscription created, but automatic execution configuration could not be persisted: {err}");
                false
            }
        },
        None => false,
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
            format!("copyTrade={SUBSCRIPTION_DELIVERY_ENABLED}"),
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
            device_routing_degraded,
            &offline_replay,
            autotrade_requested,
            autotrade_configured,
        ));
        // Balance is verified *before* broadcast now (blocking), so a success
        // envelope no longer carries a `balanceWarning` — an insufficiency exits
        // earlier via the structured `error_insufficient_balance` envelope.
        if super::content::is_cli_mode() {
            println!();
            println!("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.");
            println!();
            println!("**Step 1 (MANDATORY GATE) — Read `skills/okx-ai/references/watch-core.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.");
            println!();
            println!("**Step 2 — Execute the watch per `skills/okx-ai/references/watch-core.md` §Run watch, scoped to job-id `{sub_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read watch-core.md first.)");
            println!();
            println!("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.");
        }
        return Ok(());
    }

    println!("✓ Subscription submitted (transaction broadcast, awaiting on-chain confirmation)");
    println!("  jobId:  {sub_id}");
    println!("  txHash: {tx_hash}");
    if autotrade_requested {
        println!(
            "  Automatic signal execution: {}",
            if autotrade_configured { "configured" } else { "configuration pending" }
        );
    }
    if device_routing_degraded {
        println!(
            "  ⚠ Device list unavailable — this subscription was set to receive on THIS device only; other devices can be added later."
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
        println!("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.");
        println!();
        println!("**Step 1 (MANDATORY GATE) — Read `skills/okx-ai/references/watch-core.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.");
        println!();
        println!("**Step 2 — Execute the watch per `skills/okx-ai/references/watch-core.md` §Run watch, scoped to job-id `{sub_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read watch-core.md first.)");
        println!();
        println!("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.");
    }

    Ok(())
}

/// Blocking balance pre-check for the subscription flow — the "6th" balance
/// checkpoint. Unlike create-task's advisory checkpoint, create-subscribe's
/// broadcast performs an *immediate* ERC20 transfer, so an insufficient balance
/// must block *before* broadcast. Subscribe carries a token *address* (not a
/// symbol), so the symbol is resolved first. On an insufficient XLayer
/// business-token balance this bails the enriched `InsufficientBalanceError`
/// (deposit address attached + XLayer QR rendered to stderr on TTY, silent-degrade
/// if the address can't be resolved), which `main.rs` downcasts into the
/// structured `error_insufficient_balance` JSON envelope. Pre-check inputs that
/// carry no balance obligation (a zero/unparsable amount) or that can't be mapped
/// to a symbol silent-degrade to `Ok(())` — the symbol lookup is a subscribe-only
/// pre-check step and must never introduce a new blocking failure mode for an
/// otherwise-fundable subscription (FR-6).
async fn ensure_subscribe_balance(
    service_token_amount: &str,
    service_token_address: &str,
    user_agent_id: &str,
) -> Result<()> {
    let required: f64 = service_token_amount.parse().unwrap_or(0.0);
    if required <= 0.0 {
        return Ok(());
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
            return Ok(());
        }
    };

    if let Err(e) = common::ensure_sufficient_balance(required, &symbol).await {
        // Blocking: enrich the insufficiency with the caller's XLayer deposit
        // address + stderr QR (silent-degrade if unresolved), then bail so main.rs
        // downcasts to the structured `error_insufficient_balance` JSON. A
        // non-insufficiency infra error (login expired, balance-query failure)
        // passes through `enrich_blocking` unchanged and blocks the same way the
        // sibling checkpoints (accept / dispute) do — consistent with a flow whose
        // very next step is an immediate on-chain transfer.
        return Err(common::deposit_qr::enrich_blocking(e, user_agent_id).await);
    }

    Ok(())
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
                assert_eq!(format, "");
                assert!(exclude_device.is_none());
            }
            _ => panic!("expected CreateSubscribe"),
        }
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
            format: "json".to_string(),
            exclude_device: None,
        }
    }

    #[test]
    fn create_body_always_embeds_device_list_even_when_empty() {
        // Degrade / all-excluded resolves to an empty set — the body must still
        // carry an explicit (empty) deviceList so routing never falls to a server default.
        let p = params_fixture(None);
        let body = super::build_create_body(&p, false, serde_json::json!({ "asp": "x" }), "0xsig", &[]);
        assert_eq!(body["deviceList"], serde_json::json!([]));
        assert!(body.get("deviceList").is_some());
        assert_eq!(body["termsSig"], serde_json::json!("0xsig"));
        assert!(body.get("providerAgentId").is_none());
    }

    #[test]
    fn create_body_carries_devices_and_provider_when_present() {
        let p = params_fixture(Some("agent-7"));
        let devices = vec!["d1".to_string(), "d2".to_string()];
        let body = super::build_create_body(&p, true, serde_json::json!({}), "0xsig", &devices);
        assert_eq!(body["deviceList"], serde_json::json!(["d1", "d2"]));
        assert_eq!(body["providerAgentId"], serde_json::json!("agent-7"));
        assert_eq!(body["useTrial"], serde_json::json!(true));
    }

    #[test]
    fn create_success_envelope_carries_degrade_marker() {
        use crate::commands::agent_commerce::task::common::okx_a2a::OfflineReplayCapability;
        // Supported comm package ⇒ offlineReplaySupported:true and NO fix-commands field.
        let supported = OfflineReplayCapability {
            supported: true,
            fix_commands: Vec::new(),
        };
        let degraded = super::build_create_success("0xjob", "0xhash", true, &supported, true, true);
        assert_eq!(degraded["deviceRoutingDegraded"], serde_json::json!(true));
        assert_eq!(degraded["subId"], serde_json::json!("0xjob"));
        assert_eq!(degraded["txHash"], serde_json::json!("0xhash"));
        assert_eq!(degraded["offlineReplaySupported"], serde_json::json!(true));
        assert_eq!(degraded["autoTradeConfigRequested"], serde_json::json!(true));
        assert_eq!(degraded["autoTradeConfigured"], serde_json::json!(true));
        assert!(degraded.get("offlineReplayFixCommands").is_none());
        let ok = super::build_create_success("0xjob", "0xhash", false, &supported, false, false);
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
        let env = super::build_create_success("0xjob", "0xhash", false, &unsupported, false, false);
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
        let env2 = super::build_create_success("0xjob", "0xhash", false, &unsupported_default, false, false);
        assert_eq!(
            env2["offlineReplayFixCommands"],
            serde_json::json!(["npm install -g @okxweb3/a2a-node@latest"])
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
    fn create_body_always_enables_subscription_delivery() {
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
            format: "json".to_string(),
            exclude_device: None,
        };

        let body = build_create_body(
            &params,
            false,
            serde_json::json!({"subId": 0}),
            "0xsignature",
            &[],
        );

        assert_eq!(body["copyTrade"], serde_json::json!(1));
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
        ]);
        let super::super::TaskCommand::CreateSubscribe {
            autotrade_mode, autotrade_amount, autotrade_cap, autotrade_quote, ..
        } = cli.cmd else {
            panic!("expected CreateSubscribe");
        };
        assert_eq!(autotrade_mode.as_deref(), Some("auto"));
        assert_eq!(autotrade_amount.as_deref(), Some("20.00"));
        assert_eq!(autotrade_cap.as_deref(), Some("50"));
        assert_eq!(autotrade_quote.as_deref(), Some("USDT"));
    }

    #[test]
    fn autotrade_config_reports_only_missing_fields() {
        let mut params = params_fixture(None);
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_amount = Some("20".to_string());
        let error = params.validate().unwrap_err().to_string();
        assert!(error.contains("--autotrade-cap"));
        assert!(error.contains("--autotrade-quote"));
        assert!(!error.contains("--autotrade-amount,"));
    }

    #[test]
    fn autotrade_config_normalizes_and_rejects_amount_above_cap() {
        let mut params = params_fixture(None);
        params.autotrade_mode = Some("auto".to_string());
        params.autotrade_amount = Some("20.00".to_string());
        params.autotrade_cap = Some("50.0".to_string());
        params.autotrade_quote = Some("USDT".to_string());
        let config = params.autotrade_config().unwrap().unwrap();
        assert_eq!(config.amount, "20");
        assert_eq!(config.cap, "50");
        assert_eq!(config.quote, "usdt");

        params.autotrade_amount = Some("51".to_string());
        assert!(params.validate().unwrap_err().to_string().contains("must not exceed"));
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
            amount: "20".to_string(),
            cap: "50".to_string(),
            quote: "usdt".to_string(),
        };
        persist_subscription_autotrade("job-subscribe-auto", &config).unwrap();

        let stored = consent::load_consent("job-subscribe-auto")
            .unwrap()
            .expect("consent must exist");
        assert_eq!(stored.mode, consent::ConsentMode::Auto);
        assert_eq!(stored.trade_amount_u.as_deref(), Some("20"));
        assert_eq!(stored.cap_u.as_deref(), Some("50"));
        assert_eq!(stored.quote_token.as_deref(), Some("usdt"));
        assert!(grants::check_grant("job-subscribe-auto", "dex", "buy", "50").is_ok());
        assert!(grants::check_grant("job-subscribe-auto", "trade_kit", "sell", "50").is_ok());
        assert!(grants::check_grant("job-subscribe-auto", "trade_kit", "sell", "51").is_err());

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }
}
