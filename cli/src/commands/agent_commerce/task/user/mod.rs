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
//! - `changepublic.rs` — set to Public (scene 8)
//!
//! Shared:
//! - `query.rs`        — read-only queries (status, list, pay)

mod accept;
mod asp_ops;
pub(crate) mod attachments;
mod changepublic;
mod claim_auto_refund;
mod close;
mod complete;
mod content;
mod create;
pub mod draft;
pub mod flow;
mod flow_lifecycle;
pub(crate) use flow_lifecycle::try_recover_from_temp_file;
mod flow_negotiate;
pub(crate) mod negotiate;
mod query;
mod reject;
mod reject_apply;
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
        #[arg(long = "description-summary")]
        description_summary: Option<String>,
        #[arg(long)]
        budget: f64,
        #[arg(long = "max-budget")]
        max_budget: f64,
        #[arg(long)]
        currency: String,
        #[arg(long)]
        title: Option<String>,
        /// Designated provider agentId (skip asp-match; negotiate or x402-accept with this provider directly).
        #[arg(long)]
        provider: Option<String>,
        /// Local file paths to attach to the task after creation.
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
        /// Designated service endpoint (persisted for multi-service providers)
        #[arg(long)]
        endpoint: Option<String>,
        /// Payment mode to set at creation time (escrow / x402). When omitted the task is created with paymentMode=0 (unset).
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        /// Service ID from asp/match response
        #[arg(long = "service-id")]
        service_id: Option<String>,
        /// Service input parameters (natural language string)
        #[arg(long = "service-params")]
        service_params: Option<String>,
        /// Service token contract address
        #[arg(long = "service-token-address")]
        service_token_address: Option<String>,
        /// Service price (from asp/match feeAmount)
        #[arg(long = "service-token-amount")]
        service_token_amount: Option<String>,
        /// Task visibility: 1 = private (requires --provider), 0 = public
        #[arg(long, default_value = "1")]
        visibility: i32,
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
    /// Client confirms provider and executes payment (setPaymentMode must be done first).
    /// Provider, token symbol, and amount are read from the task detail API.
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
    /// Client converts private task to public listing
    SetPublic {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Provider generates payment invoice after provider_applied
    Payment {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Client claims auto-refund after seller timeout (submit_expired / reject_expired)
    ClaimAutoRefund {
        job_id: String,
    },
    /// x402 Phase 2b: direct/accept after job_payment_mode_changed + x402 endpoint interaction
    DirectAccept {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
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
}

// ─── Routing dispatch ──────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        // ── User actions ─────────────────────────────────────────
        TaskCommand::Create { description, description_summary, budget, max_budget, currency, title, provider, attachments, endpoint, payment_mode, service_id, service_params, service_token_address, service_token_amount, visibility } =>
            create::handle_create(&mut client, create::CreateTaskParams {
                description, description_summary, budget, max_budget, currency,
                title, provider, attachments, endpoint, payment_mode,
                service_id, service_params, service_token_address, service_token_amount, visibility,
            }).await,
        TaskCommand::AspMatch { task_desc, job_id, provider_agent_id, page, agent_id, format } =>
            asp_ops::handle_asp_match(&mut client, job_id.as_deref(), &task_desc, provider_agent_id.as_deref(), page, agent_id.as_deref(), &format).await,
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
        TaskCommand::DirectAccept { job_id, provider_agent_id, token_symbol, token_amount } =>
            accept::handle_direct_accept(&mut client, &job_id, &provider_agent_id, token_symbol.as_deref(), token_amount.as_deref()).await,
        TaskCommand::Task402Pay { job_id, provider_agent_id, accepts, endpoint, token_symbol, token_amount, from, body } =>
            accept::handle_task_402_pay(&mut client, &job_id, &provider_agent_id, &accepts, &endpoint, &token_symbol, &token_amount, from.as_deref(), body.as_deref()).await,
        TaskCommand::X402Check { endpoint, agent_id, body } =>
            accept::handle_x402_check(&mut client, &endpoint, agent_id.as_deref(), body.as_deref()).await,
        TaskCommand::Complete { job_id } =>
            complete::handle_complete(&mut client, &job_id).await,
        TaskCommand::Reject { job_id, reason } =>
            reject::handle_reject(&mut client, &job_id, &reason).await,
        TaskCommand::Close { job_id, agent_id } =>
            close::handle_close(&mut client, &job_id, agent_id.as_deref()).await,
        TaskCommand::SetPublic { job_id, agent_id } =>
            changepublic::handle_set_public(&mut client, &job_id, agent_id.as_deref()).await,
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

        // ── Read-only queries ────────────────────────────────────
        TaskCommand::Payment { job_id, agent_id } =>
            query::handle_payment(&mut client, &job_id, agent_id.as_deref().unwrap_or("")).await,

    }
}

// ─── Draft subcommands ───────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DraftCommand {
    /// Save a new task draft (off-chain)
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        description: String,
        #[arg(long = "description-summary")]
        description_summary: String,
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
        #[arg(long = "service-id")]
        service_id: Option<String>,
        #[arg(long = "service-params")]
        service_params: Option<String>,
        #[arg(long = "service-token-address")]
        service_token_address: Option<String>,
        #[arg(long = "service-token-amount")]
        service_token_amount: Option<String>,
        /// Payment mode: escrow or x402. When omitted the draft is created with paymentMode=0 (unset).
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        /// Task visibility: 1 = private (requires --provider), 0 = public
        #[arg(long, default_value = "1")]
        visibility: i32,
    },
    /// List my drafts
    List {
        #[arg(long, default_value = "1")]
        page: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Update a draft's fields (partial update)
    Update {
        job_id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long = "description-summary")]
        description_summary: Option<String>,
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
        #[arg(long = "service-id")]
        service_id: Option<String>,
        #[arg(long = "service-params")]
        service_params: Option<String>,
        #[arg(long = "service-token-address")]
        service_token_address: Option<String>,
        #[arg(long = "service-token-amount")]
        service_token_amount: Option<String>,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        #[arg(long)]
        visibility: Option<i32>,
    },
    /// Delete a draft
    Delete {
        job_id: String,
    },
    /// Publish a draft on-chain (validates all required fields, signs, broadcasts)
    Publish {
        job_id: String,
    },
    /// Pure local validation of task fields — no network calls.
    /// Returns structured JSON with per-field pass/fail.
    Validate {
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: Option<String>,
    },
}

pub async fn run_draft(cmd: DraftCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        DraftCommand::Create {
            title, description, description_summary, budget, max_budget, currency,
            provider, attachments,
            service_id, service_params, service_token_address, service_token_amount,
            payment_mode, visibility,
        } => {
            draft::handle_draft_create(
                &mut client,
                &title,
                &description,
                &description_summary,
                budget,
                max_budget,
                currency.as_deref(),
                provider.as_deref(),
                attachments.as_deref(),
                service_id.as_deref(),
                service_params.as_deref(),
                service_token_address.as_deref(),
                service_token_amount.as_deref(),
                payment_mode.as_deref(),
                visibility,
            ).await
        }
        DraftCommand::List { page, limit } => {
            draft::handle_draft_list(&mut client, page, limit).await
        }
        DraftCommand::Update {
            job_id, title, description, description_summary, budget, max_budget, currency,
            provider, attachments, service_id, service_params, service_token_address,
            service_token_amount, endpoint, payment_mode, visibility,
        } => {
            draft::handle_draft_update(
                &mut client,
                &job_id,
                title.as_deref(),
                description.as_deref(),
                description_summary.as_deref(),
                budget,
                max_budget,
                currency.as_deref(),
                provider.as_deref(),
                attachments.as_deref(),
                service_id.as_deref(),
                service_params.as_deref(),
                service_token_address.as_deref(),
                service_token_amount.as_deref(),
                endpoint.as_deref(),
                payment_mode.as_deref(),
                visibility,
            ).await
        }
        DraftCommand::Delete { job_id } => {
            draft::handle_draft_delete(&mut client, &job_id).await
        }
        DraftCommand::Publish { job_id } => {
            draft::handle_draft_publish(&mut client, &job_id).await
        }
        DraftCommand::Validate {
            description, title, budget, max_budget, currency,
        } => {
            draft::handle_validate_draft(
                description.as_deref(),
                title.as_deref(),
                budget,
                max_budget,
                currency.as_deref(),
            )
        }
    }
}

