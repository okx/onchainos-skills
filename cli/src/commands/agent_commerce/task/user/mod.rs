//! User-side task commands — enum definitions + routing dispatch.
//!
//! Files split by user action:
//! - `create.rs`       — publish task (scene 1)
//! - `asp_ops.rs`      — ASP match + set-asp (scene 1)
//! - `negotiate.rs`    — negotiation (scene 2, agent sub session)
//! - `accept.rs`       — confirm accept + fund (scene 3)
//! - `complete.rs`     — confirm completion (scene 5)
//! - `reject.rs`       — reject deliverable (scene 6)
//! - `close.rs`        — close task (scene 7) + claim arbitration reward
//!
//! Shared:
//! - `query.rs`        — read-only queries (status, list, pay)

mod accept;
mod asp_ops;
pub(crate) mod attachments;
mod claim_auto_refund;
mod close;
mod complete;
mod content;
mod create;
mod create_subscribe;
mod device_routing;
mod offline_receive;
pub(crate) use create::validate_draft_fields;
pub mod flow;
mod flow_lifecycle;
pub(crate) use flow_lifecycle::{try_recover_from_temp_file, route_subscription_delivery_to_skill};
mod flow_negotiate;
pub(crate) mod negotiate;
mod query;
mod reject;
mod reject_apply;
pub(crate) mod subscription_ops;
mod x402_flow;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::Context;

// ─── task subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Create a new task (Client only)
    Create {
        #[arg(long)]
        description: String,
        #[arg(long)]
        budget: f64,
        #[arg(long = "max-budget")]
        max_budget: f64,
        #[arg(long)]
        currency: String,
        #[arg(long)]
        title: Option<String>,
        /// Designated provider agentId (required; skip asp-match; negotiate or x402-accept with this provider directly).
        #[arg(long)]
        provider: String,
        /// Local file paths to attach to the task after creation.
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
        /// Designated service endpoint (persisted for multi-service providers)
        #[arg(long)]
        endpoint: Option<String>,
        /// Payment mode to set at creation time (required; escrow / x402).
        #[arg(long = "payment-mode")]
        payment_mode: String,
        /// Service ID from asp/match response (required)
        #[arg(long = "service-id")]
        service_id: String,
        /// Service input parameters (natural language string)
        #[arg(long = "service-params")]
        service_params: Option<String>,
        /// Service token contract address
        #[arg(long = "service-token-address")]
        service_token_address: Option<String>,
        /// Service price (from asp/match feeAmount)
        #[arg(long = "service-token-amount")]
        service_token_amount: Option<String>,
    },
    /// Create a subscription task (providerConfirmStatus → EIP-712 sign → create → broadcast)
    CreateSubscribe {
        #[arg(long = "service-id")]
        service_id: String,
        /// Use trial period (if available)
        #[arg(long = "use-trial", default_value = "false")]
        use_trial: bool,
        /// Service input parameters (JSON string)
        #[arg(long = "service-params", default_value = "")]
        service_params: String,
        /// Service price (must match listing price)
        #[arg(long = "service-token-amount")]
        service_token_amount: String,
        /// Token contract address
        #[arg(long = "service-token-address")]
        service_token_address: String,
        /// Auto-renew: 0/false=off, 1/true=on
        #[arg(long = "auto-renew")]
        auto_renew: String,
        /// Subscription title (max 64 chars)
        #[arg(long)]
        title: String,
        /// Subscription description (max 4096 chars)
        #[arg(long)]
        description: String,
        /// Designated provider agent ID
        #[arg(long = "provider-agent-id")]
        provider_agent_id: Option<String>,
        /// Exact service description returned by asp-match. Used only to persist
        /// bounded asset/tool hints; the raw prose is never executed.
        #[arg(long = "service-description", default_value = "")]
        service_description: String,
        /// Service billing interval (from asp-match subscription.interval, e.g. "month")
        #[arg(long = "service-interval", default_value = "month")]
        service_interval: String,
        /// Explicit user-confirmed automatic signal execution (`auto`).
        #[arg(long = "autotrade-mode")]
        autotrade_mode: Option<String>,
        /// Fixed quote-currency amount used for every delivered signal.
        #[arg(long = "autotrade-amount")]
        autotrade_amount: Option<String>,
        /// Per-delivery automatic-execution cap.
        #[arg(long = "autotrade-cap")]
        autotrade_cap: Option<String>,
        /// Quote currency for amount/cap (`usdt` or `usdc`).
        #[arg(long = "autotrade-quote")]
        autotrade_quote: Option<String>,
        /// Output format: "json" for raw JSON
        #[arg(long, default_value = "")]
        format: String,
        /// Device ids to omit from the default all-devices routing set (repeatable).
        #[arg(long = "exclude-device")]
        exclude_device: Option<Vec<String>>,
    },
    /// Search matching ASPs (pre-publish or post-publish)
    AspMatch {
        /// Task description (required when no --job-id)
        #[arg(long = "task-desc", default_value = "")]
        task_desc: String,
        /// Job ID (required when task already exists)
        #[arg(long = "job-id")]
        job_id: Option<String>,
        /// Narrow to this ASP's services
        #[arg(long = "provider-agent-id")]
        provider_agent_id: Option<String>,
        /// Budget amount for backend filtering
        #[arg(long = "payment-token-amount")]
        payment_token_amount: Option<f64>,
        /// Page number
        #[arg(long, default_value = "1")]
        page: usize,
        /// User agent ID
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// Output format: "json" for raw JSON (no formatted list)
        #[arg(long, default_value = "")]
        format: String,
    },
    /// Set/replace ASP + service on existing task (off-chain, triggers job_asp_selected)
    SetAsp {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        #[arg(long = "service-id")]
        service_id: String,
        #[arg(long = "service-type")]
        service_type: String,
        #[arg(long = "service-params")]
        service_params: String,
        #[arg(long = "service-token-address")]
        service_token_address: String,
        #[arg(long = "service-token-amount")]
        service_token_amount: String,
        #[arg(long = "payment-token-symbol")]
        payment_token_symbol: Option<String>,
        #[arg(long = "payment-token-amount")]
        payment_token_amount: Option<String>,
        #[arg(long = "payment-most-token-amount")]
        payment_most_token_amount: Option<String>,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Clear ASP + service fields (off-chain)
    ResetAsp {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Reject current ASP (off-chain, clears asp + service fields, triggers job_user_reject)
    UserReject {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Mark a provider as failed negotiation (excluded from future asp-match lists)
    MarkFailed {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
    },
    /// Get current task status
    /// Set payment mode on-chain (standalone, before confirm-accept)
    SetPaymentMode {
        job_id: String,
        /// escrow / x402
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
        /// x402 service endpoint URL (when omitted, fetched from the negotiate cache or service-list API).
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Client confirms ASP and executes payment (setPaymentMode must be done first).
    /// ASP, token symbol, and amount are read from the task detail API.
    ConfirmAccept {
        job_id: String,
    },
    /// Client confirms task complete and releases payment
    Complete {
        job_id: String,
    },
    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Client closes task (only valid while Open)
    Close {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// ASP generates payment invoice after provider_applied
    Payment {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Client claims auto-refund after seller timeout (submit_expired / reject_expired)
    ClaimAutoRefund {
        job_id: String,
    },
    /// x402 Phase 2: x402_pay signing + direct/accept + endpoint replay.
    /// Returns replay result (deliverable) and Payment Credential.
    Task402Pay {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        /// JSON accepts array from the HTTP 402 response
        #[arg(long)]
        accepts: String,
        /// x402 provider endpoint URL (for replay after signing)
        #[arg(long)]
        endpoint: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
        /// JSON business body to POST during replay (for endpoints that require business parameters)
        #[arg(long)]
        body: Option<String>,
        /// Bypass the confirming gate and broadcast the on-chain accept immediately (FR-7.3).
        /// Automated playbooks pass this.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Validate an x402 endpoint and extract pricing info
    X402Check {
        /// x402 provider endpoint URL
        #[arg(long)]
        endpoint: String,
        /// User agent ID (used to authenticate token-detail lookups).
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// JSON business body to POST (for endpoints that require business parameters)
        #[arg(long)]
        body: Option<String>,
    },
    /// Reject a provider's apply (on-chain pass-through; status stays `created`)
    RejectApply {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Attach local file(s) to a task
    TaskAttach {
        job_id: String,
        /// Path(s) to the file(s) to attach (repeatable, at least one required)
        #[arg(long = "file", required = true)]
        file_paths: Vec<String>,
    },
    /// List attachments for a task
    ListAttachments {
        job_id: String,
    },
    /// Cancel a subscription (unified: trial cancel + close auto-renew)
    #[command(name = "subscribe-cancel")]
    SubscribeCancel {
        sub_id: String,
    },
    /// Enable auto-renew on a subscription (needs EIP-712 terms signing)
    #[command(name = "start-autorenew")]
    StartAutorenew {
        sub_id: String,
    },
    /// Reject a subscription delivery
    #[command(name = "subscribe-reject")]
    SubscribeReject {
        sub_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Show subscription detail
    #[command(name = "subscribe-detail")]
    SubscribeDetail {
        sub_id: String,
        #[arg(long, default_value = "")]
        format: String,
    },
    /// List the logged-in agent's subscriptions.
    MySubscriptions {
        role: subscription_ops::SubscriptionRole,
        status: Option<i32>,
    },
    /// Show total monthly cost of active subscriptions.
    #[command(name = "subscribe-cost")]
    SubscribeCost {},
    /// Overwrite the receive-device list for one or more subscriptions (batch).
    #[command(name = "subscribe-device-update")]
    SubscribeDeviceUpdate {
        #[arg(long = "job-id")]
        job_id: Option<String>,
        #[arg(long = "device-list")]
        device_list: Option<String>,
        #[arg(long, conflicts_with_all = ["job_id", "device_list"])]
        items: Option<String>,
    },
    /// Set a subscription's offline-receive flag (0 = keep backlog, 1 = discard backlog).
    #[command(name = "subscribe-offline-update")]
    SubscribeOfflineUpdate {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        flag: String,
    },
    /// List the devices this agent is logged in on (paginated to completion).
    #[command(name = "device-list")]
    DeviceList {
        page: i64,
        page_size: i64,
    },
}

// ─── Routing dispatch ──────────────────────────────────────────────────────

fn parse_bool_or_int(s: &str, flag: &str) -> Result<i32> {
    match s {
        "0" | "false" => Ok(0),
        "1" | "true" => Ok(1),
        _ => anyhow::bail!("--{flag} must be 0, 1, true, or false; got \"{s}\""),
    }
}

/// Build the optional post-login subscription block. An empty subscription
/// list deliberately produces no block (the product's zero-disturb contract),
/// while a missing device snapshot is kept as JSON null so the renderer uses
/// the documented this-device-only degraded view.
fn compose_post_login_subscriptions(
    subscriptions: serde_json::Value,
    subscriptions_empty: bool,
    devices: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    if subscriptions_empty {
        return None;
    }
    Some(serde_json::json!({
        "subscriptions": subscriptions,
        "devices": devices,
    }))
}

/// State captured before the login heartbeat registers this machine. Comparing
/// against the pre-heartbeat device table is what lets login distinguish a
/// genuinely new device from an existing device whose receipt was deliberately
/// disabled by the user.
pub(crate) struct PostLoginSubscriptionsPreparation {
    agent_id: String,
    current_device_id: String,
    routing_api_base_url: String,
    current_device_was_registered: bool,
    current_device_needs_default_routing: bool,
    pre_registration_devices: serde_json::Value,
}

fn device_snapshot_contains(devices: &serde_json::Value, device_id: &str) -> Option<bool> {
    let list = devices.get("list")?.as_array()?;
    Some(list.iter().any(|row| {
        row.get("deviceId").and_then(serde_json::Value::as_str) == Some(device_id)
    }))
}

fn device_needs_default_routing(was_registered: bool, already_pending: bool) -> bool {
    already_pending || !was_registered
}

/// Fetch the device table before the registration heartbeat. Device-query
/// failure deliberately suppresses only automatic routing/the login table; the
/// login orchestrator still sends the heartbeat so device registration is never
/// coupled to this optional classification step.
pub(crate) async fn prepare_post_login_subscriptions(
) -> Option<PostLoginSubscriptionsPreparation> {
    let mut client = TaskApiClient::new();
    let (agent_id, _) = match create::resolve_user_agent().await {
        Ok(identity) => identity,
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] buyer identity unavailable: {e:#}");
            }
            return None;
        }
    };

    let Some(current_device_id) = crate::device::id::get_cached_device_id().map(str::to_string)
    else {
        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][post-login] current device id unavailable");
        }
        return None;
    };
    let devices = match device_routing::fetch_device_list_snapshot(
        &mut client,
        &agent_id,
        1,
        20,
    )
    .await
    {
        Ok(snapshot) => snapshot,
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][post-login] pre-registration device snapshot unavailable: {e:#}"
                );
            }
            return None;
        }
    };
    let Some(current_device_was_registered) =
        device_snapshot_contains(&devices, &current_device_id)
    else {
        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][post-login] malformed pre-registration device snapshot");
        }
        return None;
    };

    let already_pending = match device_routing::new_device_routing_is_pending(
        &client.base_url,
        &agent_id,
        &current_device_id,
    ) {
        Ok(pending) => pending,
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] pending routing marker unavailable: {e:#}");
            }
            return None;
        }
    };
    let current_device_needs_default_routing =
        device_needs_default_routing(current_device_was_registered, already_pending);
    if !current_device_was_registered && !already_pending {
        if let Err(e) = device_routing::mark_new_device_routing_pending(
            &client.base_url,
            &agent_id,
            &current_device_id,
        ) {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] cannot persist pending routing marker: {e:#}");
            }
            // Automatic routing cannot safely start without durable state. The
            // login orchestrator still reports the device heartbeat.
            return None;
        }
    } else if current_device_was_registered && !already_pending {
        // A completed marker is not needed once this device is visible. Deletion
        // is merely garbage collection: Completed never counts as pending.
        let _ = device_routing::clear_new_device_routing_state(
            &client.base_url,
            &agent_id,
            &current_device_id,
        );
    }

    Some(PostLoginSubscriptionsPreparation {
        agent_id,
        current_device_id,
        routing_api_base_url: client.base_url.clone(),
        current_device_was_registered,
        current_device_needs_default_routing,
        pre_registration_devices: devices,
    })
}

/// Complete new-device routing after the heartbeat. Existing devices are never
/// rewritten, preserving any manual opt-out. A new device is merged into every
/// explicit subscription list and only then is the login snapshot returned.
pub(crate) async fn finalize_post_login_subscriptions(
    prepared: PostLoginSubscriptionsPreparation,
    device_registration_succeeded: bool,
) -> Option<serde_json::Value> {
    let mut client = TaskApiClient::new();
    let snapshot = match subscription_ops::fetch_my_subscriptions_snapshot_for_agent(
        &mut client,
        subscription_ops::SubscriptionRole::Buyer,
        None,
        prepared.agent_id.clone(),
    )
    .await
    {
        Ok(snapshot) => snapshot,
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] subscription snapshot unavailable: {e:#}");
            }
            // Keep a new device's pending marker. The next login retries from a
            // fresh subscription list; wallet login itself still succeeds.
            return None;
        }
    };
    if snapshot.is_empty {
        if prepared.current_device_needs_default_routing
            && (prepared.current_device_was_registered || device_registration_succeeded)
        {
            if let Err(e) = device_routing::mark_new_device_routing_completed(
                &prepared.routing_api_base_url,
                &prepared.agent_id,
                &prepared.current_device_id,
            ) {
                if cfg!(feature = "debug-log") {
                    eprintln!("[DEBUG][post-login] empty-list routing completion failed: {e:#}");
                }
                return None;
            }
            let _ = device_routing::clear_new_device_routing_state(
                &prepared.routing_api_base_url,
                &prepared.agent_id,
                &prepared.current_device_id,
            );
        }
        return None;
    }
    let mut subscriptions = snapshot.data;

    if prepared.current_device_needs_default_routing
        && !prepared.current_device_was_registered
        && !device_registration_succeeded
    {
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][post-login] new device registration failed; suppressing subscription table"
            );
        }
        return None;
    }

    let devices = if prepared.current_device_needs_default_routing {
        match device_routing::add_new_device_to_all_subscriptions(
            &mut client,
            &prepared.routing_api_base_url,
            &prepared.agent_id,
            &mut subscriptions,
            &prepared.current_device_id,
        )
        .await
        {
            Ok(updated) => {
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG][post-login] added new device to {updated} explicit subscription routes"
                    );
                }
            }
            Err(e) => {
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG][post-login] new-device subscription routing unavailable: {e:#}"
                    );
                }
                // The durable marker remains, so the next login retries only
                // subscriptions whose fresh lists still lack this device.
                return None;
            }
        }

        if let Err(e) = device_routing::clear_new_device_routing_state(
            &prepared.routing_api_base_url,
            &prepared.agent_id,
            &prepared.current_device_id,
        ) {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] routing completed; state cleanup deferred: {e:#}");
            }
        }

        if prepared.current_device_was_registered {
            Some(prepared.pre_registration_devices)
        } else {
            match device_routing::fetch_device_list_snapshot(
                &mut client,
                &prepared.agent_id,
                1,
                20,
            )
            .await
            {
                Ok(snapshot)
                    if device_snapshot_contains(&snapshot, &prepared.current_device_id)
                        == Some(true) =>
                {
                    Some(snapshot)
                }
                Ok(_) => {
                    if cfg!(feature = "debug-log") {
                        eprintln!(
                            "[DEBUG][post-login] registered device not visible yet; using degraded render"
                        );
                    }
                    None
                }
                Err(e) => {
                    if cfg!(feature = "debug-log") {
                        eprintln!(
                            "[DEBUG][post-login] post-registration device snapshot unavailable; degrading: {e:#}"
                        );
                    }
                    None
                }
            }
        }
    } else {
        // Existing device with no pending onboarding marker: never rewrite its
        // subscriptions, preserving every manual per-task opt-out.
        Some(prepared.pre_registration_devices)
    };

    compose_post_login_subscriptions(subscriptions, false, devices)
}

/// Best-effort login post-condition: fetch the buyer's subscriptions and, only
/// when non-empty, the complete logged-in-device table. Subscription failures
/// and empty lists are intentionally silent; device failures preserve the
/// subscription data and select the documented degraded render.
pub(crate) async fn fetch_post_login_subscriptions() -> Option<serde_json::Value> {
    let mut client = TaskApiClient::new();
    let subscriptions = match subscription_ops::fetch_my_subscriptions_snapshot(
        &mut client,
        subscription_ops::SubscriptionRole::Buyer,
        None,
    )
    .await
    {
        Ok(snapshot) => snapshot,
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] subscription snapshot unavailable: {e:#}");
            }
            return None;
        }
    };

    if subscriptions.is_empty {
        return None;
    }

    let devices = match device_routing::fetch_device_list_snapshot(
        &mut client,
        &subscriptions.agent_id,
        1,
        20,
    )
    .await
    {
        Ok(snapshot) => Some(snapshot),
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] device snapshot unavailable; degrading: {e:#}");
            }
            None
        }
    };

    compose_post_login_subscriptions(subscriptions.data, false, devices)
}

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        // ── User actions ─────────────────────────────────────────
        TaskCommand::Create { description, budget, max_budget, currency, title, provider, attachments, endpoint, payment_mode, service_id, service_params, service_token_address, service_token_amount } =>
            create::handle_create(&mut client, create::CreateTaskParams {
                description, budget, max_budget, currency,
                title, provider, attachments, endpoint, payment_mode,
                service_id, service_params, service_token_address, service_token_amount,
            }).await,
        TaskCommand::CreateSubscribe { service_id, use_trial, service_params, service_token_amount, service_token_address, auto_renew, title, description, provider_agent_id, service_description, service_interval, autotrade_mode, autotrade_amount, autotrade_cap, autotrade_quote, format, exclude_device } => {
            let auto_renew = parse_bool_or_int(&auto_renew, "auto-renew")?;
            create_subscribe::handle_create_subscribe(&mut client, create_subscribe::CreateSubscribeParams {
                service_id, use_trial, service_params, service_token_amount, service_token_address,
                auto_renew, title, description, provider_agent_id, service_description, service_interval,
                autotrade_mode, autotrade_amount, autotrade_cap, autotrade_quote, format, exclude_device,
            }).await
        }
        TaskCommand::AspMatch { task_desc, job_id, provider_agent_id, payment_token_amount, page, agent_id, format } =>
            asp_ops::handle_asp_match(&mut client, job_id.as_deref(), &task_desc, provider_agent_id.as_deref(), payment_token_amount, page, agent_id.as_deref(), &format).await,
        TaskCommand::SetAsp { job_id, provider_agent_id, service_id, service_type, service_params, service_token_address, service_token_amount, payment_token_symbol, payment_token_amount, payment_most_token_amount, agent_id } =>
            asp_ops::handle_set_asp(&mut client, &job_id, &provider_agent_id, &service_id, &service_type, &service_params, &service_token_address, &service_token_amount, payment_token_symbol.as_deref(), payment_token_amount.as_deref(), payment_most_token_amount.as_deref(), agent_id.as_deref()).await,
        TaskCommand::ResetAsp { job_id, agent_id } =>
            asp_ops::handle_reset_asp(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::UserReject { job_id, agent_id } =>
            asp_ops::handle_user_reject(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::MarkFailed { job_id, provider_agent_id } => {
            negotiate::mark_failed(&job_id, &provider_agent_id)
        }
        TaskCommand::SetPaymentMode { job_id, payment_mode, token_symbol, token_amount, endpoint } =>
            accept::handle_set_payment_mode(&mut client, &job_id, payment_mode.as_deref(), token_symbol.as_deref(), token_amount.as_deref(), endpoint.as_deref()).await,
        TaskCommand::ConfirmAccept { job_id } =>
            accept::handle_confirm_accept(&mut client, &job_id, None).await,
        TaskCommand::Task402Pay { job_id, provider_agent_id, accepts, endpoint, token_symbol, token_amount, from, body, force } =>
            accept::handle_task_402_pay(&mut client, &job_id, &provider_agent_id, &accepts, &endpoint, &token_symbol, &token_amount, from.as_deref(), body.as_deref(), force).await,
        TaskCommand::X402Check { endpoint, agent_id, body } =>
            accept::handle_x402_check(&mut client, &endpoint, agent_id.as_deref(), body.as_deref()).await,
        TaskCommand::Complete { job_id } =>
            complete::handle_complete(&mut client, &job_id).await,
        TaskCommand::Reject { job_id, reason } =>
            reject::handle_reject(&mut client, &job_id, &reason).await,
        TaskCommand::Close { job_id, agent_id } =>
            close::handle_close(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::ClaimAutoRefund { job_id } =>
            claim_auto_refund::handle_claim_auto_refund(&mut client, &job_id).await,
        TaskCommand::RejectApply { job_id, agent_id } =>
            reject_apply::handle_reject_apply(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::TaskAttach { job_id, file_paths } => {
            if file_paths.is_empty() {
                anyhow::bail!("at least one --file <path> is required");
            }
            for fp in &file_paths {
                attachments::handle_task_attach(&mut client, &job_id, fp).await?;
            }
            Ok(())
        }
        TaskCommand::ListAttachments { job_id } => {
            attachments::handle_task_attachments(&job_id)
        }

        // ── Subscription management ─────────────────────────────
        TaskCommand::SubscribeCancel { sub_id } =>
            subscription_ops::handle_subscribe_cancel(&mut client, &sub_id).await,
        TaskCommand::StartAutorenew { sub_id } =>
            subscription_ops::handle_start_autorenew(&mut client, &sub_id).await,
        TaskCommand::SubscribeReject { sub_id, reason } =>
            reject::handle_reject(&mut client, &sub_id, &reason).await,
        TaskCommand::SubscribeDetail { sub_id, format } =>
            subscription_ops::handle_subscribe_detail(&mut client, &sub_id, &format).await,
        TaskCommand::SubscribeDeviceUpdate { job_id, device_list, items } =>
            device_routing::handle_subscribe_device_update(&mut client, job_id.as_deref(), device_list.as_deref(), items.as_deref()).await,
        TaskCommand::SubscribeOfflineUpdate { job_id, flag } =>
            offline_receive::handle_subscribe_offline_update(&mut client, &job_id, &flag).await,
        TaskCommand::DeviceList { page, page_size } =>
            device_routing::handle_device_list(&mut client, page, page_size).await,

        // ── Read-only queries ────────────────────────────────────
        TaskCommand::Payment { job_id, agent_id } =>
            query::handle_payment(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,
        TaskCommand::MySubscriptions { role, status } => {
            subscription_ops::handle_my_subscriptions(&mut client, role, status).await
        }
        TaskCommand::SubscribeCost {} =>
            subscription_ops::handle_subscribe_cost(&mut client).await,

    }
}

#[cfg(test)]
mod post_login_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_subscriptions_produce_no_post_login_block() {
        let block = compose_post_login_subscriptions(
            json!({ "list": [] }),
            true,
            Some(json!({ "list": [{ "deviceId": "d1" }] })),
        );
        assert!(block.is_none());
    }

    #[test]
    fn non_empty_subscriptions_include_complete_device_snapshot() {
        let subscriptions = json!({ "list": [{ "jobId": "j1" }] });
        let devices = json!({ "list": [{ "deviceId": "d1" }], "total": 1 });
        let block =
            compose_post_login_subscriptions(subscriptions.clone(), false, Some(devices.clone()))
                .expect("non-empty subscriptions must produce a block");
        assert_eq!(block["subscriptions"], subscriptions);
        assert_eq!(block["devices"], devices);
    }

    #[test]
    fn device_failure_keeps_subscriptions_and_selects_degraded_render() {
        let subscriptions = json!({ "list": [{ "jobId": "j1" }] });
        let block = compose_post_login_subscriptions(subscriptions.clone(), false, None)
            .expect("subscription data must survive a device-list failure");
        assert_eq!(block["subscriptions"], subscriptions);
        assert!(block["devices"].is_null());
    }

    #[test]
    fn pre_heartbeat_device_snapshot_distinguishes_new_and_existing_devices() {
        let devices = json!({
            "list": [
                { "deviceId": "d1", "deviceName": "Mac 1" },
                { "deviceId": "d2", "deviceName": "Mac 2" }
            ]
        });
        assert_eq!(device_snapshot_contains(&devices, "d1"), Some(true));
        assert_eq!(device_snapshot_contains(&devices, "d-new"), Some(false));
        assert_eq!(device_snapshot_contains(&json!({}), "d1"), None);
    }

    #[test]
    fn only_new_or_interrupted_devices_need_default_routing() {
        assert!(device_needs_default_routing(false, false));
        assert!(device_needs_default_routing(false, true));
        assert!(device_needs_default_routing(true, true));
        assert!(
            !device_needs_default_routing(true, false),
            "ordinary re-login must preserve this device's manual opt-outs"
        );
    }
}
