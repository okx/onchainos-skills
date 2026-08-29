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
pub(crate) mod my_tasks;
pub(crate) mod negotiate;
mod query;
mod reject;
mod reject_apply;
pub(crate) mod subscription_ops;
mod x402_flow;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::commands::agent_commerce::identity::ServiceMatchArgs;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::subscription_identity::{
    select_subscription_agent_id,
};
use crate::commands::Context;

// ─── task subcommands ──────────────────────────────────────────────────────

/// Task-flow wrapper around the canonical marketplace service search arguments.
#[derive(Args, Clone, Debug)]
pub struct TaskServiceSelectArgs {
    #[command(flatten)]
    pub service_match: ServiceMatchArgs,
    /// Output format: json
    #[arg(long, default_value = "json")]
    pub format: String,
}

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
        /// Explicit signal handling mode (`auto` or `notify_only`).
        #[arg(long = "autotrade-mode")]
        autotrade_mode: Option<String>,
        /// Fixed quote-currency amount used for every delivered signal.
        #[arg(long = "autotrade-amount")]
        autotrade_amount: Option<String>,
        /// Optional per-delivery cap metadata (not enforced).
        #[arg(long = "autotrade-cap")]
        autotrade_cap: Option<String>,
        /// Quote currency for amount/cap (`usdt` or `usdc`).
        #[arg(long = "autotrade-quote")]
        autotrade_quote: Option<String>,
        /// User-authorized Trade Kit environment (`live` or `demo`).
        #[arg(long = "autotrade-environment")]
        autotrade_environment: Option<String>,
        /// User-authorized Trade Kit derivative margin mode.
        #[arg(long = "autotrade-margin-mode")]
        autotrade_margin_mode: Option<String>,
        /// User-authorized signal-entry order policy.
        #[arg(long = "autotrade-order-policy")]
        autotrade_order_policy: Option<String>,
        /// User-selected Trade Kit credential source.
        #[arg(long = "autotrade-auth-mode")]
        autotrade_auth_mode: Option<String>,
        /// User-confirmed tool-specific settings as one JSON object.
        #[arg(long = "autotrade-settings-json")]
        autotrade_settings_json: Option<String>,
        /// Execution fields that the selected service requires the subscriber
        /// to confirm. Repeat per field. Core names and matching value flags:
        /// mode (--autotrade-mode), tradeAmount (--autotrade-amount),
        /// cap (--autotrade-cap), quote (--autotrade-quote), environment
        /// (--autotrade-environment), marginMode (--autotrade-margin-mode),
        /// orderPolicy (--autotrade-order-policy), authMode
        /// (--autotrade-auth-mode). Guide-defined fields must be supplied by
        /// --autotrade-settings-json. tradeAmountU is accepted only as a
        /// deprecated alias for the public tradeAmount name.
        #[arg(long = "autotrade-required-field")]
        autotrade_required_fields: Vec<String>,
        /// Output format: "json" for raw JSON
        #[arg(long, default_value = "")]
        format: String,
        /// Legacy compatibility input. Create-time device selection is rejected.
        #[arg(long = "exclude-device", hide = true)]
        exclude_device: Option<Vec<String>>,
    },
    /// Search matching ASPs for an existing task
    AspMatch {
        /// Existing task ID
        #[arg(long = "job-id")]
        job_id: String,
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
    /// Select task-creation candidate services via service-match
    #[command(name = "task-service-select")]
    TaskServiceSelect(TaskServiceSelectArgs),
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
    /// List subscription and one-time tasks for the current User identity.
    MyTasks {
        task_type: my_tasks::MyTaskType,
        status_type: u8,
        page: u32,
        page_size: u32,
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

/// Bounded execution hint derived from a subscription's service description.
/// The classifier is fail-closed: no recognized executable asset class means no
/// authorization prompt. The raw description is never treated as consent.
struct PostLoginExecutableService {
    description: String,
    description_source: &'static str,
    asset_classes: Vec<crate::asset_class::AssetClass>,
    explicit_tools:
        Vec<crate::commands::agent_commerce::task::common::autotrade::tooling::ExecutionTool>,
    service_guide: Option<String>,
    service_guide_hash: Option<String>,
    /// True only when the current provider catalog resolved this exact service.
    /// An unavailable catalog must never be mistaken for an empty guide.
    service_guide_hash_resolved: bool,
}

fn executable_service_from_description(
    description: &str,
    description_source: &'static str,
) -> Option<PostLoginExecutableService> {
    let description = description.trim();
    if description.is_empty() {
        return None;
    }
    let classified =
        crate::commands::agent_commerce::task::common::autotrade::tooling::classify_description(
            description,
        );
    if classified.classes.is_empty() {
        return None;
    }
    Some(PostLoginExecutableService {
        description: description.to_string(),
        description_source,
        asset_classes: classified.classes,
        explicit_tools: classified.explicit,
        service_guide: None,
        service_guide_hash: None,
        service_guide_hash_resolved: false,
    })
}

fn valid_service_guide_hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn service_guide_metadata(service: &serde_json::Value) -> (Option<String>, Option<String>) {
    let guide = service
        .get("serviceGuide")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let Some(guide) = guide else {
        return (None, None);
    };
    let supplied_hash = service
        .get("serviceGuideHash")
        .and_then(serde_json::Value::as_str)
        .filter(|value| valid_service_guide_hash(value))
        .map(|value| value.to_ascii_lowercase());
    let hash = supplied_hash.unwrap_or_else(|| {
        use sha2::Digest;
        format!("sha256:{}", hex::encode(sha2::Sha256::digest(guide.as_bytes())))
    });
    (Some(guide), Some(hash))
}

fn attach_service_guide(
    executable: &mut PostLoginExecutableService,
    service: &serde_json::Value,
    resolved_from_current_catalog: bool,
) {
    let (guide, hash) = service_guide_metadata(service);
    executable.service_guide = guide;
    executable.service_guide_hash = hash;
    executable.service_guide_hash_resolved = resolved_from_current_catalog;
}

fn requires_trade_kit(executable: &PostLoginExecutableService) -> bool {
    use crate::commands::agent_commerce::task::common::autotrade::tooling::ExecutionTool;

    executable.explicit_tools.contains(&ExecutionTool::TradeKit)
        || executable
            .asset_classes
            .contains(&crate::asset_class::AssetClass::Option)
}

fn trade_kit_required_fields(executable: &PostLoginExecutableService) -> Vec<String> {
    if !requires_trade_kit(executable) {
        return Vec::new();
    }
    let mut fields = vec!["environment".to_string(), "orderPolicy".to_string()];
    if executable
        .asset_classes
        .contains(&crate::asset_class::AssetClass::Perp)
    {
        fields.push("marginMode".to_string());
    }
    fields
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServiceGuideStatus {
    Unknown,
    Absent,
    Current,
    Unchanged,
    Baseline,
    Changed,
}

impl ServiceGuideStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Absent => "absent",
            Self::Current => "current",
            Self::Unchanged => "unchanged",
            Self::Baseline => "baseline",
            Self::Changed => "changed",
        }
    }

    fn requires_refresh(self) -> bool {
        matches!(self, Self::Baseline | Self::Changed)
    }
}

fn stored_service_guide_hash(
    snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
) -> Option<&str> {
    snapshot
        .dynamic_settings
        .get("serviceGuideHash")
        .and_then(serde_json::Value::as_str)
}

fn service_guide_status(
    executable: &PostLoginExecutableService,
    snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
) -> ServiceGuideStatus {
    if !executable.service_guide_hash_resolved {
        return ServiceGuideStatus::Unknown;
    }
    if snapshot.status
        != crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshotStatus::Active
    {
        return if executable.service_guide_hash.is_some() {
            ServiceGuideStatus::Current
        } else {
            ServiceGuideStatus::Absent
        };
    }
    match (
        stored_service_guide_hash(snapshot),
        executable.service_guide_hash.as_deref(),
    ) {
        (None, None) => ServiceGuideStatus::Absent,
        (Some(stored), Some(current)) if stored.eq_ignore_ascii_case(current) => {
            ServiceGuideStatus::Unchanged
        }
        (None, Some(_)) => ServiceGuideStatus::Baseline,
        _ => ServiceGuideStatus::Changed,
    }
}

fn persisted_required_fields(
    snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
) -> Vec<String> {
    snapshot
        .dynamic_settings
        .get("requiredFields")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}

fn required_fields_for_precheck(
    executable: &PostLoginExecutableService,
    snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
    guide_status: ServiceGuideStatus,
) -> Vec<String> {
    let mut fields = trade_kit_required_fields(executable);
    if !guide_status.requires_refresh() {
        for field in persisted_required_fields(snapshot) {
            if !fields.contains(&field) {
                fields.push(field);
            }
        }
    }
    fields
}

fn missing_snapshot_fields(
    snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
    required_fields: &[String],
) -> Vec<String> {
    required_fields
        .iter()
        .filter(|field| match field.as_str() {
            "mode" => false,
            "tradeAmount" => snapshot.trade_amount_u.is_none(),
            "tradeAmountU" => snapshot.trade_amount_u.is_none(),
            "cap" => snapshot.cap_u.is_none(),
            "quote" => snapshot.quote_token.is_none(),
            "environment" => snapshot.trade_environment.is_none(),
            "marginMode" => snapshot.margin_mode.is_none(),
            "orderPolicy" => snapshot.order_policy.is_none(),
            "authMode" => snapshot.auth_mode.is_none(),
            other => !crate::commands::agent_commerce::task::common::autotrade::consent::dynamic_setting_present(
                &snapshot.dynamic_settings,
                other,
            ),
        })
        .cloned()
        .collect()
}

fn add_service_guide_precheck_context(
    result: &mut serde_json::Value,
    executable: &PostLoginExecutableService,
    snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
    status: ServiceGuideStatus,
) {
    let Some(object) = result.as_object_mut() else {
        return;
    };
    object.insert(
        "guideStatus".to_string(),
        serde_json::Value::String(status.as_str().to_string()),
    );
    object.insert(
        "guideHashResolved".to_string(),
        serde_json::Value::Bool(executable.service_guide_hash_resolved),
    );
    object.insert(
        "guideRefreshRequired".to_string(),
        serde_json::Value::Bool(status.requires_refresh()),
    );
    if let Some(guide) = executable.service_guide.as_ref() {
        object.insert(
            "serviceGuide".to_string(),
            serde_json::Value::String(guide.chars().take(8192).collect()),
        );
    }
    if let Some(hash) = executable.service_guide_hash.as_ref() {
        object.insert(
            "currentServiceGuideHash".to_string(),
            serde_json::Value::String(hash.clone()),
        );
    }
    if let Some(hash) = stored_service_guide_hash(snapshot) {
        object.insert(
            "storedServiceGuideHash".to_string(),
            serde_json::Value::String(hash.to_string()),
        );
    }
}

fn post_login_executable_service(
    subscription: &serde_json::Value,
) -> Option<PostLoginExecutableService> {
    let description = subscription
        .get("serviceDescription")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut executable =
        executable_service_from_description(description, "service_description")?;
    attach_service_guide(&mut executable, subscription, false);
    Some(executable)
}

/// Resolve the executable description and the provider's current service guide
/// without ever treating either prose field as authorization. Description
/// resolution remains backward compatible, while guide freshness is known only
/// when the current provider catalog resolves the exact service.
async fn resolve_subscription_executable_service(
    client: &mut TaskApiClient,
    agent_id: &str,
    subscription: &serde_json::Value,
    resolve_current_guide: bool,
) -> Result<Option<PostLoginExecutableService>> {
    let inline_description = subscription
        .get("serviceDescription")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if !resolve_current_guide {
        if let Some(description) = inline_description {
            let Some(mut executable) =
                executable_service_from_description(description, "service_description")
            else {
                return Ok(None);
            };
            attach_service_guide(&mut executable, subscription, false);
            return Ok(Some(executable));
        }
    }

    let job_id = subscription
        .get("jobId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let mut detail = None;
    if inline_description.is_none() && !job_id.is_empty() {
        match subscription_ops::fetch_subscribe_detail_for_agent(client, job_id, agent_id).await {
            Ok(value) => detail = Some(value),
            Err(error) if cfg!(feature = "debug-log") => {
                eprintln!(
                    "[DEBUG][watch-precheck] subscription detail unavailable for {job_id}: {error:#}"
                );
            }
            Err(_) => {}
        }
    }
    if !resolve_current_guide {
        if let Some(detail) = detail.as_ref() {
            if let Some(description) = detail
                .get("serviceDescription")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let Some(mut executable) =
                    executable_service_from_description(description, "subscription_detail")
                else {
                    return Ok(None);
                };
                attach_service_guide(&mut executable, detail, false);
                return Ok(Some(executable));
            }
        }
    }

    let provider_agent_id = subscription
        .get("providerAgentId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let service_id = subscription
        .get("serviceId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let mut catalog_error = None;
    let catalog_service = if provider_agent_id.is_empty() || service_id.is_empty() {
        None
    } else {
        match crate::commands::agent_commerce::task::common::find_service(
            provider_agent_id,
            service_id,
        )
        .await
        {
            Ok(service) => service,
            Err(error) => {
                catalog_error = Some(error);
                None
            }
        }
    };

    let detail_description = detail
        .as_ref()
        .and_then(|value| value.get("serviceDescription"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let catalog_description = catalog_service
        .as_ref()
        .and_then(|value| value.get("serviceDescription"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (description, source) = if let Some(description) = inline_description {
        (description, "service_description")
    } else if let Some(description) = detail_description {
        (description, "subscription_detail")
    } else if let Some(description) = catalog_description {
        (description, "provider_service_catalog")
    } else if let Some(error) = catalog_error {
        return Err(error);
    } else {
        return Ok(None);
    };

    let Some(mut executable) = executable_service_from_description(description, source) else {
        return Ok(None);
    };
    if let Some(service) = catalog_service.as_ref() {
        attach_service_guide(&mut executable, service, resolve_current_guide);
    } else if let Some(detail) = detail.as_ref() {
        attach_service_guide(&mut executable, detail, false);
    } else {
        attach_service_guide(&mut executable, subscription, false);
    }
    Ok(Some(executable))
}

/// Restore bounded execution-profile hints for active executable subscriptions
/// on this device. Consent remains untouched so an explicit restore can collect
/// ASP-requested user configuration before scoped watch begins.
async fn add_post_login_autotrade_prechecks(
    client: &mut TaskApiClient,
    subscriptions: &mut serde_json::Value,
    agent_id: &str,
) {
    use crate::commands::agent_commerce::task::common::autotrade::profile;

    let Some(list) = subscriptions
        .get("list")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    for subscription in list {
        if subscription
            .get("status")
            .and_then(serde_json::Value::as_i64)
            != Some(
                crate::commands::agent_commerce::task::common::state_machine::SubStatus::Active
                    .code(),
            )
            || subscription
                .get("thisDeviceReceives")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            continue;
        }
        let job_id = subscription
            .get("jobId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let service_id = subscription
            .get("serviceId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let provider_agent_id = subscription
            .get("providerAgentId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if job_id.is_empty() || service_id.is_empty() || provider_agent_id.is_empty() {
            continue;
        }
        let executable = match resolve_subscription_executable_service(
            client,
            agent_id,
            subscription,
            false,
        )
        .await
        {
            Ok(Some(executable)) => executable,
            Ok(None) => continue,
            Err(error) => {
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG][post-login] service description restore skipped for {job_id}: {error:#}"
                    );
                }
                continue;
            }
        };

        // A new device has no local profile either. Rebuild only bounded routing
        // hints; this write never creates consent or an executable command.
        if let Err(error) = profile::save_from_description(
            job_id,
            service_id,
            Some(provider_agent_id),
            &executable.description,
        ) {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] execution profile restore skipped: {error}");
            }
        }
    }
}

fn compose_scoped_watch_autotrade_precheck(
    job_id: &str,
    agent_id: &str,
    subscription: Option<&serde_json::Value>,
    consent_snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
    grant_refresh_required: bool,
) -> serde_json::Value {
    let executable = subscription.and_then(post_login_executable_service);
    compose_scoped_watch_autotrade_precheck_with_executable(
        job_id,
        agent_id,
        subscription,
        executable.as_ref(),
        consent_snapshot,
        grant_refresh_required,
    )
}

fn compose_scoped_watch_autotrade_precheck_with_executable(
    job_id: &str,
    agent_id: &str,
    subscription: Option<&serde_json::Value>,
    executable: Option<&PostLoginExecutableService>,
    consent_snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
    grant_refresh_required: bool,
) -> serde_json::Value {
    use crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshotStatus;
    use crate::commands::agent_commerce::task::common::state_machine::SubStatus;

    let Some(subscription) = subscription else {
        return serde_json::json!({
            "jobId": job_id,
            "agentId": agent_id,
            "applicable": false,
            "watchAllowed": true,
            "shouldPromptAuthorization": false,
            "reason": "not_subscription",
        });
    };
    if subscription
        .get("status")
        .and_then(serde_json::Value::as_i64)
        != Some(SubStatus::Active.code())
    {
        return serde_json::json!({
            "jobId": job_id,
            "agentId": agent_id,
            "applicable": true,
            "watchAllowed": true,
            "shouldPromptAuthorization": false,
            "reason": "subscription_not_active",
        });
    }
    if subscription
        .get("thisDeviceReceives")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return serde_json::json!({
            "jobId": job_id,
            "agentId": agent_id,
            "applicable": true,
            "watchAllowed": true,
            "shouldPromptAuthorization": false,
            "reason": "not_receiving_on_this_device",
        });
    }
    let Some(executable) = executable else {
        return serde_json::json!({
            "jobId": job_id,
            "agentId": agent_id,
            "applicable": true,
            "watchAllowed": true,
            "shouldPromptAuthorization": false,
            "reason": "non_executable_service",
        });
    };

    match consent_snapshot.status {
        ConsentSnapshotStatus::Active => {
            let guide_status = service_guide_status(executable, consent_snapshot);
            let required_fields =
                required_fields_for_precheck(executable, consent_snapshot, guide_status);
            let missing_fields = missing_snapshot_fields(consent_snapshot, &required_fields);
            let legacy_consent = consent_snapshot
                .version
                .map(|version| {
                    version
                        < crate::commands::agent_commerce::task::common::autotrade::consent::CONSENT_VERSION
                })
                .unwrap_or(true);
            let refresh_required = consent_snapshot.mode
                != Some(crate::commands::agent_commerce::task::common::autotrade::consent::ConsentMode::Decline)
                && (legacy_consent
                    || !missing_fields.is_empty()
                    || grant_refresh_required
                    || guide_status.requires_refresh());
            if refresh_required {
                let authorization_refresh_required =
                    legacy_consent || !missing_fields.is_empty() || grant_refresh_required;
                let mut refresh_reasons = Vec::new();
                if legacy_consent {
                    refresh_reasons.push("legacy_consent");
                }
                if !missing_fields.is_empty() {
                    refresh_reasons.push("missing_required_fields");
                }
                if grant_refresh_required {
                    refresh_reasons.push("stale_trade_kit_grant");
                }
                if guide_status.requires_refresh() {
                    refresh_reasons.push("service_guide_changed");
                }
                let guide_refresh_only = guide_status.requires_refresh()
                    && !legacy_consent
                    && missing_fields.is_empty()
                    && !grant_refresh_required;
                let mut result = serde_json::json!({
                    "jobId": job_id,
                    "agentId": agent_id,
                    "applicable": true,
                    "watchAllowed": false,
                    "shouldPromptAuthorization": false,
                    "shouldPromptConfiguration": true,
                    "authorizationRefreshRequired": authorization_refresh_required,
                    "reason": "configuration_required",
                    "consentStatus": consent_snapshot.status,
                    "title": subscription
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                    "serviceDescription": executable.description.chars().take(4096).collect::<String>(),
                    "descriptionSource": executable.description_source,
                    "assetClasses": executable.asset_classes,
                    "requiredFields": required_fields,
                    "missingFields": missing_fields,
                    "existingConsent": consent_snapshot,
                    "refreshReasons": refresh_reasons,
                    "modeConfirmationRequired": !guide_refresh_only,
                });
                add_service_guide_precheck_context(
                    &mut result,
                    executable,
                    consent_snapshot,
                    guide_status,
                );
                result
            } else {
                let mut result = serde_json::json!({
                    "jobId": job_id,
                    "agentId": agent_id,
                    "applicable": true,
                    "watchAllowed": true,
                    "shouldPromptAuthorization": false,
                    "reason": "consent_active",
                    "consentStatus": consent_snapshot.status,
                });
                add_service_guide_precheck_context(
                    &mut result,
                    executable,
                    consent_snapshot,
                    guide_status,
                );
                result
            }
        }
        ConsentSnapshotStatus::NotSet => {
            let mut result = serde_json::json!({
                "jobId": job_id,
                "agentId": agent_id,
                "applicable": true,
                "watchAllowed": false,
                "shouldPromptAuthorization": false,
                "shouldPromptConfiguration": true,
                "reason": "configuration_required",
                "consentStatus": consent_snapshot.status,
                "title": subscription
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
                "serviceDescription": executable.description.chars().take(4096).collect::<String>(),
                "descriptionSource": executable.description_source,
                "assetClasses": executable.asset_classes,
            });
            add_service_guide_precheck_context(
                &mut result,
                executable,
                consent_snapshot,
                service_guide_status(executable, consent_snapshot),
            );
            result
        }
        ConsentSnapshotStatus::Unreadable => serde_json::json!({
            "jobId": job_id,
            "agentId": agent_id,
            "applicable": true,
            "watchAllowed": false,
            "shouldPromptAuthorization": false,
            "reason": "consent_unreadable",
            "consentStatus": consent_snapshot.status,
            "repairCommand": format!(
                "onchainos agent autotrade-consent-set --job-id {job_id} --mode pause"
            ),
        }),
    }
}

fn compose_existing_consent_review(
    job_id: &str,
    agent_id: &str,
    subscription: &serde_json::Value,
    executable: &PostLoginExecutableService,
    consent_snapshot: &crate::commands::agent_commerce::task::common::autotrade::consent::ConsentSnapshot,
) -> serde_json::Value {
    let guide_status = service_guide_status(executable, consent_snapshot);
    let required_fields =
        required_fields_for_precheck(executable, consent_snapshot, guide_status);
    let mut result = serde_json::json!({
        "jobId": job_id,
        "agentId": agent_id,
        "applicable": true,
        "watchAllowed": false,
        "shouldPromptAuthorization": false,
        "shouldPromptConfiguration": true,
        "configurationReviewRequired": true,
        "reason": "configuration_required",
        "consentStatus": consent_snapshot.status,
        "title": subscription
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        "serviceDescription": executable.description.chars().take(4096).collect::<String>(),
        "descriptionSource": executable.description_source,
        "assetClasses": executable.asset_classes,
        "requiredFields": required_fields,
        "missingFields": [],
        "existingConsent": consent_snapshot,
        "modeConfirmationRequired": true,
    });
    add_service_guide_precheck_context(
        &mut result,
        executable,
        consent_snapshot,
        guide_status,
    );
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreDeliveryConsentContext {
    pub agent_id: String,
    pub asset_class: crate::asset_class::AssetClass,
    pub title: String,
    pub required_fields: Vec<String>,
    pub service_guide_hash: Option<String>,
    pub service_guide_hash_resolved: bool,
    pub preserve_existing_mode: bool,
    pub existing_mode: Option<String>,
}

/// Bind a restore-configuration continuation to the canonical authenticated
/// subscription. ASP prose may select which fields to ask for, but it never
/// supplies an authorization value.
pub(crate) fn bind_subscription_restore_consent_context(
    precheck: &serde_json::Value,
    requested_job_id: &str,
    requested_agent_id: &str,
    requested_asset_class: crate::asset_class::AssetClass,
) -> Result<PreDeliveryConsentContext> {
    let reason = precheck
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing_reason");
    let consent_status = precheck
        .get("consentStatus")
        .and_then(serde_json::Value::as_str);
    let valid_restore_state = consent_status == Some("not_set")
        || (consent_status == Some("active")
            && precheck
                .get("authorizationRefreshRequired")
                .and_then(serde_json::Value::as_bool)
                == Some(true))
        || (consent_status == Some("active")
            && precheck
                .get("configurationReviewRequired")
                .and_then(serde_json::Value::as_bool)
                == Some(true))
        || (consent_status == Some("active")
            && precheck
                .get("guideRefreshRequired")
                .and_then(serde_json::Value::as_bool)
                == Some(true));
    if precheck
        .get("watchAllowed")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
        || precheck
            .get("shouldPromptConfiguration")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || reason != "configuration_required"
        || !valid_restore_state
    {
        anyhow::bail!(
            "subscription restore configuration is not allowed for this subscription (reason: {reason})"
        );
    }

    let canonical_job_id = precheck
        .get("jobId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("restore precheck returned no jobId"))?;
    if canonical_job_id != requested_job_id {
        anyhow::bail!("restore jobId does not match the canonical precheck");
    }

    let canonical_agent_id = precheck
        .get("agentId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("restore precheck returned no buyer agentId"))?;
    if canonical_agent_id != requested_agent_id {
        anyhow::bail!("restore agentId does not match the authenticated buyer");
    }

    let canonical_asset_classes = precheck
        .get("assetClasses")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("restore precheck returned no asset classes"))?;
    if !canonical_asset_classes
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|value| value.parse::<crate::asset_class::AssetClass>().ok())
        .any(|value| value == requested_asset_class)
    {
        anyhow::bail!("restore signal type does not match the canonical precheck");
    }

    Ok(PreDeliveryConsentContext {
        agent_id: canonical_agent_id.to_string(),
        asset_class: requested_asset_class,
        title: precheck
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        required_fields: precheck
            .get("requiredFields")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect(),
        service_guide_hash: precheck
            .get("currentServiceGuideHash")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        service_guide_hash_resolved: precheck
            .get("guideHashResolved")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        preserve_existing_mode: precheck
            .get("modeConfirmationRequired")
            .and_then(serde_json::Value::as_bool)
            == Some(false),
        existing_mode: precheck
            .pointer("/existingConsent/mode")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

/// Bind a pre-delivery authorization card to the canonical scoped-watch
/// precheck result. Caller-provided fields are retained for CLI compatibility,
/// but none of them are trusted: every value must match the authenticated
/// buyer subscription before a consent prompt can be rendered.
pub(crate) fn bind_pre_delivery_consent_context(
    precheck: &serde_json::Value,
    requested_job_id: &str,
    requested_agent_id: &str,
    requested_asset_class: crate::asset_class::AssetClass,
) -> Result<PreDeliveryConsentContext> {
    let reason = precheck
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing_reason");
    if precheck
        .get("watchAllowed")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
        || precheck
            .get("shouldPromptAuthorization")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || reason != "authorization_required"
        || precheck
            .get("consentStatus")
            .and_then(serde_json::Value::as_str)
            != Some("not_set")
    {
        anyhow::bail!(
            "pre-delivery authorization is not allowed for this subscription (reason: {reason})"
        );
    }

    let canonical_job_id = precheck
        .get("jobId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("pre-delivery precheck returned no jobId"))?;
    if canonical_job_id != requested_job_id {
        anyhow::bail!("pre-delivery jobId does not match the canonical precheck");
    }

    let canonical_agent_id = precheck
        .get("agentId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("pre-delivery precheck returned no buyer agentId"))?;
    if canonical_agent_id != requested_agent_id {
        anyhow::bail!("pre-delivery agentId does not match the authenticated buyer");
    }

    let canonical_asset_classes = precheck
        .get("assetClasses")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("pre-delivery precheck returned no asset classes"))?;
    let asset_class_matches = canonical_asset_classes
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|value| value.parse::<crate::asset_class::AssetClass>().ok())
        .any(|value| value == requested_asset_class);
    if !asset_class_matches {
        anyhow::bail!("pre-delivery signal type does not match the canonical precheck");
    }

    Ok(PreDeliveryConsentContext {
        agent_id: canonical_agent_id.to_string(),
        asset_class: requested_asset_class,
        title: precheck
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        required_fields: Vec::new(),
        service_guide_hash: None,
        service_guide_hash_resolved: false,
        preserve_existing_mode: false,
        existing_mode: None,
    })
}

/// First-entry gate for an explicitly scoped watch. Non-subscription jobs and
/// subscriptions that do not need execution authorization pass through; an
/// executable Active subscription with no local consent returns the ASP
/// description and any live bounded configuration continuation before watch.
pub(crate) async fn scoped_watch_autotrade_precheck(job_id: &str) -> Result<serde_json::Value> {
    scoped_watch_autotrade_precheck_inner(job_id, false).await
}

pub(crate) async fn scoped_watch_autotrade_precheck_for_review(
    job_id: &str,
) -> Result<serde_json::Value> {
    scoped_watch_autotrade_precheck_inner(job_id, true).await
}

async fn scoped_watch_autotrade_precheck_inner(
    job_id: &str,
    review_existing: bool,
) -> Result<serde_json::Value> {
    use crate::commands::agent_commerce::task::common::autotrade::{
        self, consent, grants, profile,
    };

    if !grants::job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    let agent_id = resolve_post_login_agentic_id().await?;
    let mut client = TaskApiClient::new();
    let snapshot = subscription_ops::fetch_my_subscriptions_snapshot_for_agent(
        &mut client,
        subscription_ops::SubscriptionRole::Buyer,
        None,
        agent_id.clone(),
    )
    .await?;
    let list = snapshot
        .data
        .get("list")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("subscription list is malformed"))?;
    let subscription = list.iter().find(|item| {
        item.get("jobId").and_then(serde_json::Value::as_str) == Some(job_id)
    });
    let mut resolved_executable = None;

    if let Some(subscription) = subscription {
        let active = subscription
            .get("status")
            .and_then(serde_json::Value::as_i64)
            == Some(
                crate::commands::agent_commerce::task::common::state_machine::SubStatus::Active
                    .code(),
            );
        let receives = subscription
            .get("thisDeviceReceives")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        if active && receives {
            resolved_executable = resolve_subscription_executable_service(
                &mut client,
                &snapshot.agent_id,
                subscription,
                true,
            )
            .await?;
            if let Some(executable) = resolved_executable.as_ref() {
                let service_id = subscription
                    .get("serviceId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let provider_agent_id = subscription
                    .get("providerAgentId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if !service_id.is_empty() && !provider_agent_id.is_empty() {
                    if let Err(error) = profile::save_from_description(
                        job_id,
                        service_id,
                        Some(provider_agent_id),
                        &executable.description,
                    ) {
                        if cfg!(feature = "debug-log") {
                            eprintln!(
                                "[DEBUG][watch-precheck] execution profile restore skipped: {error}"
                            );
                        }
                    }
                }
            }
        }
    }

    let consent_snapshot = consent::consent_snapshot(job_id);
    let grant_refresh_required = resolved_executable.as_ref().is_some_and(requires_trade_kit)
        && consent_snapshot.mode == Some(consent::ConsentMode::Auto)
        && (grants::check_grant(job_id, "trade_kit", "buy", "1").is_err()
            || grants::check_grant(job_id, "trade_kit", "sell", "1").is_err());

    // A scoped watch must not surface authorization cards left pending by the
    // retired delivery-time flow, regardless of whether a replacement policy
    // has already been configured.
    // This changes only those exact control decisions to handled; notification
    // history and unrelated decisions remain untouched.
    if resolved_executable.is_some()
        && consent_snapshot.status == consent::ConsentSnapshotStatus::Active
    {
        crate::commands::agent_commerce::task::common::okx_a2a::mark_retired_autotrade_decisions_handled(
            job_id,
        )?;
    } else if resolved_executable.is_some() {
        crate::commands::agent_commerce::task::common::okx_a2a::mark_retired_autotrade_mode_decisions_handled(
            job_id,
        )?;
    }

    let mut result = compose_scoped_watch_autotrade_precheck_with_executable(
        job_id,
        &snapshot.agent_id,
        subscription,
        resolved_executable.as_ref(),
        &consent_snapshot,
        grant_refresh_required,
    );
    if review_existing
        && result.get("reason").and_then(serde_json::Value::as_str) == Some("consent_active")
        && matches!(
            consent_snapshot.mode,
            Some(consent::ConsentMode::Auto | consent::ConsentMode::Manual)
        )
    {
        if let (Some(subscription), Some(executable)) = (subscription, resolved_executable.as_ref()) {
            result = compose_existing_consent_review(
                job_id,
                &snapshot.agent_id,
                subscription,
                executable,
                &consent_snapshot,
            );
        }
    }
    if result.get("reason").and_then(serde_json::Value::as_str)
        == Some("configuration_required")
    {
        if let Some(file) = autotrade::continuation::load_live_for_job(
            job_id,
            &snapshot.agent_id,
        )? {
            if file.origin == autotrade::continuation::Origin::SubscriptionRestore {
                let guide_hash_resolved = result
                    .get("guideHashResolved")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let current_guide_hash = result
                    .get("currentServiceGuideHash")
                    .and_then(serde_json::Value::as_str);
                let continuation_matches_guide = !guide_hash_resolved
                    || (file.service_guide_hash_resolved
                        && file.service_guide_hash.as_deref() == current_guide_hash);
                if continuation_matches_guide {
                    let missing_fields = file.missing_fields();
                    if let Some(object) = result.as_object_mut() {
                        object.insert(
                            "continuationId".to_string(),
                            serde_json::Value::String(file.continuation_id),
                        );
                        object.insert(
                            "selectedMode".to_string(),
                            serde_json::to_value(file.selected_mode)?,
                        );
                        object.insert(
                            "modeConfirmed".to_string(),
                            serde_json::Value::Bool(file.mode_confirmed),
                        );
                        object.insert(
                            "requiredFields".to_string(),
                            serde_json::to_value(file.required_fields)?,
                        );
                        object.insert(
                            "missingFields".to_string(),
                            serde_json::to_value(missing_fields)?,
                        );
                    }
                } else {
                    autotrade::continuation::clear(job_id);
                }
            } else {
                anyhow::bail!(
                    "a different live auto-trade configuration is already bound to this job"
                );
            }
        }
    }
    Ok(result)
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

pub(crate) async fn resolve_post_login_agentic_id() -> Result<String> {
    create::resolve_user_agent()
        .await
        .map(|(agent_id, _)| agent_id)
}

/// Fetch the device table before the registration heartbeat. Device-query
/// failure deliberately suppresses only automatic routing/the login table; the
/// login orchestrator still sends the heartbeat so device registration is never
/// coupled to this optional classification step.
pub(crate) async fn prepare_post_login_subscriptions(
    agentic_id: &str,
) -> Option<PostLoginSubscriptionsPreparation> {
    let agent_id = match select_subscription_agent_id(agentic_id, "") {
        Ok(agent_id) => agent_id,
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post-login] buyer identity unavailable: {e:#}");
            }
            return None;
        }
    };
    let mut client = TaskApiClient::new();

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

    add_post_login_autotrade_prechecks(&mut client, &mut subscriptions, &prepared.agent_id).await;
    compose_post_login_subscriptions(subscriptions, false, devices)
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
        TaskCommand::CreateSubscribe { service_id, use_trial, service_params, service_token_amount, service_token_address, auto_renew, title, description, provider_agent_id, service_description, service_interval, autotrade_mode, autotrade_amount, autotrade_cap, autotrade_quote, autotrade_environment, autotrade_margin_mode, autotrade_order_policy, autotrade_auth_mode, autotrade_settings_json, autotrade_required_fields, format, exclude_device } => {
            let auto_renew = parse_bool_or_int(&auto_renew, "auto-renew")?;
            create_subscribe::handle_create_subscribe(&mut client, create_subscribe::CreateSubscribeParams {
                service_id, use_trial, service_params, service_token_amount, service_token_address,
                auto_renew, title, description, provider_agent_id, service_description, service_interval,
                autotrade_mode, autotrade_amount, autotrade_cap, autotrade_quote, autotrade_environment,
                autotrade_margin_mode, autotrade_order_policy, autotrade_required_fields, format, exclude_device,
                autotrade_auth_mode, autotrade_settings_json,
            }).await
        }
        TaskCommand::AspMatch { job_id, provider_agent_id, payment_token_amount, page, agent_id, format } =>
            asp_ops::handle_asp_match(&mut client, &job_id, provider_agent_id.as_deref(), payment_token_amount, page, agent_id.as_deref(), &format).await,
        TaskCommand::TaskServiceSelect(args) =>
            asp_ops::handle_task_service_select(&mut client, &args.service_match, &args.format).await,
        TaskCommand::SetAsp { job_id, provider_agent_id, service_id, service_type, service_params, service_token_address, service_token_amount, payment_token_symbol, agent_id } =>
            asp_ops::handle_set_asp(&mut client, &job_id, &provider_agent_id, &service_id, &service_type, &service_params, &service_token_address, &service_token_amount, payment_token_symbol.as_deref(), agent_id.as_deref()).await,
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
        TaskCommand::MyTasks {
            task_type,
            status_type,
            page,
            page_size,
        } => {
            my_tasks::handle_my_tasks(&mut client, task_type, status_type, page, page_size).await
        }
        TaskCommand::SubscribeCost {} =>
            subscription_ops::handle_subscribe_cost(&mut client).await,

    }
}

#[cfg(test)]
mod post_login_tests {
    use super::*;
    use crate::asset_class::AssetClass;
    use crate::commands::agent_commerce::task::common::autotrade::consent::{
        ConsentMode, ConsentSnapshot, ConsentSnapshotStatus, MarginMode, OrderPolicy,
        TradeKitAuthMode, CONSENT_VERSION,
    };
    use crate::commands::agent_commerce::task::common::autotrade::trade_kit::TradeEnvironment;
    use serde_json::json;

    fn consent_snapshot(status: ConsentSnapshotStatus) -> ConsentSnapshot {
        let active = status == ConsentSnapshotStatus::Active;
        ConsentSnapshot {
            status,
            version: active.then_some(CONSENT_VERSION),
            mode: active.then_some(ConsentMode::Auto),
            cap_u: active.then(|| "100".to_string()),
            trade_amount_u: active.then(|| "10".to_string()),
            quote_token: active.then(|| "usdt".to_string()),
            trade_environment: active.then_some(TradeEnvironment::Live),
            margin_mode: active.then_some(MarginMode::Cross),
            order_policy: active.then_some(OrderPolicy::Market),
            auth_mode: active.then_some(TradeKitAuthMode::OAuth),
            dynamic_settings: Default::default(),
            created_at: active.then_some(1),
            expires_at: active.then_some(u64::MAX),
        }
    }

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

    #[test]
    fn post_login_precheck_prefers_canonical_service_description() {
        let subscription = json!({
            "serviceDescription": "Prediction market signals with BUY YES entries",
            "description": "generic subscription text"
        });
        let executable = post_login_executable_service(&subscription).unwrap();
        assert_eq!(executable.description_source, "service_description");
        assert_eq!(
            executable.asset_classes,
            vec![crate::asset_class::AssetClass::Prediction]
        );
    }

    #[test]
    fn post_login_precheck_never_classifies_the_user_task_description() {
        let subscription = json!({
            "description": "Spot trading signals with executable buy entries"
        });
        assert!(post_login_executable_service(&subscription).is_none());
    }

    #[test]
    fn post_login_precheck_fails_closed_for_non_executable_service() {
        let subscription = json!({
            "serviceDescription": "Read-only market news and risk reports"
        });
        assert!(post_login_executable_service(&subscription).is_none());
    }

    #[test]
    fn post_login_precheck_classifies_a_dex_spot_description() {
        let subscription = json!({
            "serviceDescription": "Continuously pushes DEX spot buy/sell direction and position signals"
        });
        let executable = post_login_executable_service(&subscription).unwrap();
        assert_eq!(
            executable.asset_classes,
            vec![crate::asset_class::AssetClass::Spot]
        );
    }

    #[test]
    fn scoped_watch_precheck_uses_a_recovered_service_description() {
        let mut subscription = active_executable_subscription();
        subscription
            .as_object_mut()
            .unwrap()
            .remove("serviceDescription");
        let executable = executable_service_from_description(
            "Spot trading signals with BUY entries",
            "subscription_detail",
        )
        .unwrap();
        let result = compose_scoped_watch_autotrade_precheck_with_executable(
            "job-watch",
            "user-1",
            Some(&subscription),
            Some(&executable),
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["shouldPromptAuthorization"], false);
        assert_eq!(result["shouldPromptConfiguration"], true);
        assert_eq!(result["reason"], "configuration_required");
        assert_eq!(result["descriptionSource"], "subscription_detail");
    }

    fn active_executable_subscription() -> serde_json::Value {
        json!({
            "jobId": "job-watch",
            "status": crate::commands::agent_commerce::task::common::state_machine::SubStatus::Active.code(),
            "thisDeviceReceives": true,
            "title": "Spot signal subscription",
            "providerAgentId": "provider-1",
            "serviceId": "service-1",
            "serviceDescription": "Spot trading signals with BUY entries; user must set a fixed amount, per-trade cap, and USDT or USDC"
        })
    }

    fn executable_with_current_guide(guide: Option<&str>) -> PostLoginExecutableService {
        let mut executable = executable_service_from_description(
            "Spot trading signals with BUY entries",
            "service_description",
        )
        .unwrap();
        let service = match guide {
            Some(guide) => json!({"serviceGuide": guide}),
            None => json!({}),
        };
        attach_service_guide(&mut executable, &service, true);
        executable
    }

    #[test]
    fn scoped_watch_precheck_returns_bounded_restore_configuration() {
        let subscription = active_executable_subscription();
        let result = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&subscription),
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        assert_eq!(result["applicable"], true);
        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["shouldPromptAuthorization"], false);
        assert_eq!(result["shouldPromptConfiguration"], true);
        assert_eq!(result["reason"], "configuration_required");
        assert_eq!(result["assetClasses"], json!(["spot"]));
        assert_eq!(
            result["serviceDescription"],
            active_executable_subscription()["serviceDescription"]
        );
    }

    #[test]
    fn restore_binding_accepts_only_the_canonical_configuration_precheck() {
        let precheck = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&active_executable_subscription()),
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        let bound = bind_subscription_restore_consent_context(
            &precheck,
            "job-watch",
            "user-1",
            AssetClass::Spot,
        )
        .unwrap();
        assert_eq!(bound.agent_id, "user-1");
        assert_eq!(bound.asset_class, AssetClass::Spot);
        assert_eq!(bound.title, "Spot signal subscription");

        assert!(bind_subscription_restore_consent_context(
            &precheck,
            "job-other",
            "user-1",
            AssetClass::Spot,
        )
        .is_err());
        assert!(bind_subscription_restore_consent_context(
            &precheck,
            "job-watch",
            "user-1",
            AssetClass::Perp,
        )
        .is_err());
    }

    #[test]
    fn pre_delivery_binding_rejects_cardless_prechecks() {
        let precheck = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&active_executable_subscription()),
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        assert!(
            bind_pre_delivery_consent_context(
                &precheck,
                "job-watch",
                "user-1",
                AssetClass::Spot
            )
            .is_err()
        );
    }

    #[test]
    fn pre_delivery_binding_rejects_cross_job_agent_or_asset_mixups() {
        let precheck = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&active_executable_subscription()),
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        for result in [
            bind_pre_delivery_consent_context(&precheck, "job-other", "user-1", AssetClass::Spot),
            bind_pre_delivery_consent_context(&precheck, "job-watch", "user-2", AssetClass::Spot),
            bind_pre_delivery_consent_context(&precheck, "job-watch", "user-1", AssetClass::Perp),
        ] {
            assert!(result.is_err());
        }
    }

    #[test]
    fn pre_delivery_binding_rejects_stale_or_already_authorized_prechecks() {
        let precheck = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&active_executable_subscription()),
            &consent_snapshot(ConsentSnapshotStatus::Active),
            false,
        );
        let error =
            bind_pre_delivery_consent_context(&precheck, "job-watch", "user-1", AssetClass::Spot)
                .expect_err("live consent must suppress a stale pre-delivery card");
        assert!(error.to_string().contains("consent_active"));
    }

    #[test]
    fn scoped_watch_precheck_allows_existing_live_consent() {
        let subscription = active_executable_subscription();
        let result = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&subscription),
            &consent_snapshot(ConsentSnapshotStatus::Active),
            false,
        );
        assert_eq!(result["watchAllowed"], true);
        assert_eq!(result["shouldPromptAuthorization"], false);
        assert_eq!(result["reason"], "consent_active");
        assert!(result.get("serviceDescription").is_none());
    }

    #[test]
    fn unchanged_service_guide_does_not_interrupt_existing_consent() {
        let subscription = active_executable_subscription();
        let executable = executable_with_current_guide(Some("Choose a fixed amount."));
        let mut snapshot = consent_snapshot(ConsentSnapshotStatus::Active);
        snapshot.dynamic_settings.insert(
            "serviceGuideHash".to_string(),
            json!(executable.service_guide_hash.clone().unwrap()),
        );
        let result = compose_scoped_watch_autotrade_precheck_with_executable(
            "job-watch",
            "user-1",
            Some(&subscription),
            Some(&executable),
            &snapshot,
            false,
        );
        assert_eq!(result["watchAllowed"], true);
        assert_eq!(result["guideStatus"], "unchanged");
        assert_eq!(result["guideRefreshRequired"], false);
    }

    #[test]
    fn changed_service_guide_enters_incremental_refresh_without_reconfirming_mode() {
        let subscription = active_executable_subscription();
        let executable = executable_with_current_guide(Some("Choose a risk bucket."));
        let mut snapshot = consent_snapshot(ConsentSnapshotStatus::Active);
        snapshot.dynamic_settings.insert(
            "serviceGuideHash".to_string(),
            json!(format!("sha256:{}", "a".repeat(64))),
        );
        snapshot.dynamic_settings.insert(
            "requiredFields".to_string(),
            json!(["extra.legacyRiskBucket"]),
        );
        snapshot.dynamic_settings.insert(
            "extra".to_string(),
            json!({
                "legacyRiskBucket": {
                    "label": "Legacy risk bucket",
                    "type": "string",
                    "value": "medium"
                }
            }),
        );
        let result = compose_scoped_watch_autotrade_precheck_with_executable(
            "job-watch",
            "user-1",
            Some(&subscription),
            Some(&executable),
            &snapshot,
            false,
        );
        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["guideStatus"], "changed");
        assert_eq!(result["guideRefreshRequired"], true);
        assert_eq!(result["authorizationRefreshRequired"], false);
        assert_eq!(result["modeConfirmationRequired"], false);
        assert_eq!(result["requiredFields"], json!([]));
        assert_eq!(result["missingFields"], json!([]));
        assert_eq!(result["refreshReasons"], json!(["service_guide_changed"]));
        assert_eq!(
            result["currentServiceGuideHash"],
            executable.service_guide_hash.unwrap()
        );
    }

    #[test]
    fn removed_service_guide_is_a_resolved_change_not_a_fetch_failure() {
        let subscription = active_executable_subscription();
        let executable = executable_with_current_guide(None);
        let mut snapshot = consent_snapshot(ConsentSnapshotStatus::Active);
        snapshot.dynamic_settings.insert(
            "serviceGuideHash".to_string(),
            json!(format!("sha256:{}", "b".repeat(64))),
        );
        let result = compose_scoped_watch_autotrade_precheck_with_executable(
            "job-watch",
            "user-1",
            Some(&subscription),
            Some(&executable),
            &snapshot,
            false,
        );
        assert_eq!(result["guideStatus"], "changed");
        assert_eq!(result["guideHashResolved"], true);
        assert!(result.get("currentServiceGuideHash").is_none());
    }

    #[test]
    fn explicit_restore_can_review_and_modify_existing_live_consent() {
        let subscription = active_executable_subscription();
        let executable = post_login_executable_service(&subscription).unwrap();
        let snapshot = consent_snapshot(ConsentSnapshotStatus::Active);
        let result = compose_existing_consent_review(
            "job-watch",
            "user-1",
            &subscription,
            &executable,
            &snapshot,
        );

        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["shouldPromptConfiguration"], true);
        assert_eq!(result["configurationReviewRequired"], true);
        assert_eq!(result["reason"], "configuration_required");
        assert_eq!(result["missingFields"], json!([]));
        assert_eq!(result["existingConsent"]["mode"], "auto");
        assert_eq!(result["existingConsent"]["tradeAmountU"], "10");

        bind_subscription_restore_consent_context(
            &result,
            "job-watch",
            "user-1",
            AssetClass::Spot,
        )
        .expect("review must remain bound to the canonical subscription");
    }

    #[test]
    fn scoped_watch_precheck_requires_confirmation_for_legacy_consent() {
        let subscription = active_executable_subscription();
        let mut snapshot = consent_snapshot(ConsentSnapshotStatus::Active);
        snapshot.version = Some(CONSENT_VERSION - 1);
        let result = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&subscription),
            &snapshot,
            false,
        );
        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["reason"], "configuration_required");
        assert_eq!(result["authorizationRefreshRequired"], true);
        assert_eq!(result["existingConsent"]["version"], CONSENT_VERSION - 1);
        assert_eq!(result["refreshReasons"], json!(["legacy_consent"]));
        let bound = bind_subscription_restore_consent_context(
            &result,
            "job-watch",
            "user-1",
            AssetClass::Spot,
        )
        .expect("legacy active consent must enter bounded restore");
        assert!(bound.required_fields.is_empty());
    }

    #[test]
    fn scoped_watch_precheck_requires_missing_trade_kit_settings_and_grant_refresh() {
        let mut subscription = active_executable_subscription();
        subscription["serviceDescription"] =
            json!("Trade Kit perp trading signals with LONG and SHORT entries");
        let mut snapshot = consent_snapshot(ConsentSnapshotStatus::Active);
        snapshot.trade_environment = None;
        snapshot.margin_mode = None;
        snapshot.order_policy = None;
        let result = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&subscription),
            &snapshot,
            true,
        );
        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["authorizationRefreshRequired"], true);
        assert_eq!(
            result["requiredFields"],
            json!(["environment", "orderPolicy", "marginMode"])
        );
        assert_eq!(result["missingFields"], result["requiredFields"]);
        assert_eq!(
            result["refreshReasons"],
            json!(["missing_required_fields", "stale_trade_kit_grant"])
        );
        let bound = bind_subscription_restore_consent_context(
            &result,
            "job-watch",
            "user-1",
            AssetClass::Perp,
        )
        .expect("incomplete active consent must enter bounded restore");
        assert_eq!(
            bound.required_fields,
            ["environment", "orderPolicy", "marginMode"]
        );
    }

    #[test]
    fn scoped_watch_precheck_blocks_unreadable_consent() {
        let subscription = active_executable_subscription();
        let result = compose_scoped_watch_autotrade_precheck(
            "job-watch",
            "user-1",
            Some(&subscription),
            &consent_snapshot(ConsentSnapshotStatus::Unreadable),
            false,
        );
        assert_eq!(result["watchAllowed"], false);
        assert_eq!(result["shouldPromptAuthorization"], false);
        assert_eq!(result["reason"], "consent_unreadable");
        assert!(result["repairCommand"].as_str().unwrap().contains("--mode pause"));
    }

    #[test]
    fn scoped_watch_precheck_does_not_gate_non_subscription_or_read_only_jobs() {
        let non_subscription = compose_scoped_watch_autotrade_precheck(
            "job-regular",
            "user-1",
            None,
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        assert_eq!(non_subscription["watchAllowed"], true);
        assert_eq!(non_subscription["reason"], "not_subscription");

        let read_only = json!({
            "jobId": "job-news",
            "status": crate::commands::agent_commerce::task::common::state_machine::SubStatus::Active.code(),
            "thisDeviceReceives": true,
            "serviceDescription": "Read-only market news and risk reports"
        });
        let read_only_result = compose_scoped_watch_autotrade_precheck(
            "job-news",
            "user-1",
            Some(&read_only),
            &consent_snapshot(ConsentSnapshotStatus::NotSet),
            false,
        );
        assert_eq!(read_only_result["watchAllowed"], true);
        assert_eq!(read_only_result["reason"], "non_executable_service");
    }

    #[tokio::test]
    async fn post_login_preparation_rejects_blank_agentic_id_before_network() {
        assert!(prepare_post_login_subscriptions("   ").await.is_none());
    }
}
