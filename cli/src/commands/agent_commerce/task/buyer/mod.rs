//! Buyer-side task commands — enum definitions + routing dispatch.
//!
//! Files split by user action:
//! - `create.rs`       — publish task (scene 1)
//! - `recommend.rs`    — fetch recommended providers (scene 1)
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
mod attachments;
mod changepublic;
mod claim_auto_refund;
mod close;
mod complete;
mod content;
mod create;
pub mod draft;
pub mod flow;
mod flow_lifecycle;
mod flow_negotiate;
pub(crate) mod negotiate;
mod query;
mod recommend;
mod reject;
mod set_terms;
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
        #[arg(long = "deadline-open")]
        deadline_open: String,
        #[arg(long = "deadline-submit")]
        deadline_submit: String,
        #[arg(long)]
        title: Option<String>,
        /// Designated provider agentId (skip recommend; negotiate or x402-accept with this provider directly).
        #[arg(long)]
        provider: Option<String>,
        /// Local file paths to attach to the task after creation.
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
        /// Designated service endpoint (persisted for multi-service providers)
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Get recommended providers for a task
    Recommend {
        job_id: String,
        /// Agent identity (agenticId header)
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// Show next provider (advance index) from cached list
        #[arg(long)]
        next: bool,
        /// Show current provider from cached list
        #[arg(long)]
        current: bool,
        /// Specify page number (0-based)
        #[arg(long)]
        page: Option<usize>,
        /// Advance to next page
        #[arg(long = "next-page")]
        next_page: bool,
        /// Emit a pending-decisions-v2 recommend_pick decision after fetching
        /// the list. Requires `--sub-key`. By default uses the auto-written
        /// canonical English card; pass `--user-content` to override with a
        /// sub-localized version.
        #[arg(long = "emit-decision")]
        emit_decision: bool,
        /// Full XMTP sessionKey (from `session_status`). Required with `--emit-decision`.
        #[arg(long = "sub-key")]
        sub_key: Option<String>,
        /// Task title used in the decision list label (defaults to `<title>`).
        #[arg(long = "job-title")]
        job_title: Option<String>,
        /// Pre-localized card body to enqueue instead of the auto-written
        /// canonical English card file.
        #[arg(long = "user-content")]
        user_content: Option<String>,
    },
    /// Mark a provider as failed negotiation (excluded from future recommend lists)
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
        /// x402 service endpoint URL (when omitted, fetched from the recommend cache or service-list API).
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Client confirms provider and executes payment (setPaymentMode must be done first)
    ConfirmAccept {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        /// When omitted, auto-fetched from the task detail's paymentType.
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        /// Payment token symbol agreed during negotiation (e.g. USDT); required for escrow.
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        /// Payment amount agreed during negotiation (human-readable, e.g. "50"); required for escrow.
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
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
        /// Buyer agent ID (used to authenticate token-detail lookups).
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// JSON business body to POST (for endpoints that require business parameters)
        #[arg(long)]
        body: Option<String>,
    },
    /// Change payment token and amount (on-chain, wait for task_token_budget_change)
    SetTokenAndBudget {
        job_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long)]
        budget: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Change provider (on-chain, does not wait for confirmation)
    SetProvider {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Change max budget (off-chain, succeeds immediately)
    SetMaxBudget {
        job_id: String,
        #[arg(long = "max-budget")]
        max_budget: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Save negotiated payment params locally (agent calls after negotiation)
    SaveAgreed {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Atomic save-agreed + set-payment-mode (escrow) — used by negotiate_ack
    /// to collapse the two steps into one CLI call so the LLM cannot reorder
    /// or skip either. payment-mode is hard-coded to "escrow" (A2A path only).
    SaveAgreedAndSetPayment {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Attach a local file to a task
    TaskAttach {
        job_id: String,
        /// Path to the file to attach
        #[arg(long = "file")]
        file_path: String,
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
        TaskCommand::Create { description, description_summary, budget, max_budget, currency, deadline_open, deadline_submit, title, provider, attachments, endpoint } =>
            create::handle_create(&mut client, create::CreateTaskParams {
                description, description_summary, budget, max_budget, currency,
                deadline_open, deadline_submit, title, provider, attachments, endpoint,
            }).await,
        TaskCommand::Recommend { job_id, agent_id, next, current, page, next_page, emit_decision, sub_key, job_title, user_content } => {
            if next {
                recommend::handle_recommend_next(&job_id)
            } else if current {
                recommend::handle_recommend_current(&job_id)
            } else if next_page {
                recommend::handle_recommend_next_page(&mut client, &job_id).await
            } else {
                let p = page.unwrap_or(0);
                recommend::handle_recommend(
                    &mut client,
                    &job_id,
                    agent_id.as_deref().unwrap_or(""),
                    p,
                    recommend::EmitDecisionOpts {
                        enabled: emit_decision,
                        sub_key,
                        job_title,
                        user_content,
                    },
                )
                .await
                .map(|_| ())
            }
        }
        TaskCommand::MarkFailed { job_id, provider_agent_id } => {
            negotiate::mark_failed(&job_id, &provider_agent_id)
        }
        TaskCommand::SetPaymentMode { job_id, payment_mode, token_symbol, token_amount, endpoint } =>
            accept::handle_set_payment_mode(&mut client, &job_id, payment_mode.as_deref(), token_symbol.as_deref(), token_amount.as_deref(), endpoint.as_deref()).await,
        TaskCommand::ConfirmAccept { job_id, provider_agent_id, payment_mode, token_symbol, token_amount } =>
            accept::handle_confirm_accept(&mut client, &job_id, &provider_agent_id, payment_mode.as_deref(), token_symbol.as_deref(), token_amount.as_deref()).await,
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
        TaskCommand::SetTokenAndBudget { job_id, token_symbol, budget, agent_id } =>
            set_terms::handle_set_token_and_budget(&mut client, &job_id, &token_symbol, &budget, agent_id.as_deref()).await,
        TaskCommand::SetProvider { job_id, provider_agent_id, agent_id } =>
            set_terms::handle_set_provider(&mut client, &job_id, &provider_agent_id, agent_id.as_deref()).await,
        TaskCommand::SetMaxBudget { job_id, max_budget, agent_id } =>
            set_terms::handle_set_max_budget(&mut client, &job_id, &max_budget, agent_id.as_deref()).await,
        TaskCommand::SaveAgreed { job_id, provider_agent_id, token_symbol, token_amount, agent_id } => {
            negotiate::save_agreed(&mut client, &job_id, &provider_agent_id, &token_symbol, &token_amount, agent_id.as_deref()).await
        }
        TaskCommand::SaveAgreedAndSetPayment { job_id, provider_agent_id, token_symbol, token_amount, agent_id } => {
            // Atomic two-step: save-agreed then set-payment-mode (escrow).
            // If save-agreed fails we short-circuit; if set-payment-mode fails
            // the agreement is already persisted, which is fine — the LLM
            // will surface the error via cli_failed and retry just step 2.
            negotiate::save_agreed(&mut client, &job_id, &provider_agent_id, &token_symbol, &token_amount, Some(&agent_id)).await?;
            accept::handle_set_payment_mode(&mut client, &job_id, Some("escrow"), Some(&token_symbol), Some(&token_amount), None).await
        }
        TaskCommand::TaskAttach { job_id, file_path } => {
            attachments::handle_task_attach(&mut client, &job_id, &file_path).await
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
        description: Option<String>,
        #[arg(long = "description-summary")]
        description_summary: Option<String>,
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long = "deadline-open")]
        deadline_open: Option<String>,
        #[arg(long = "deadline-submit")]
        deadline_submit: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
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
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long = "deadline-open")]
        deadline_open: Option<String>,
        #[arg(long = "deadline-submit")]
        deadline_submit: Option<String>,
        #[arg(long)]
        provider: Option<String>,
    },
    /// Delete a draft
    Delete {
        job_id: String,
    },
    /// Publish a draft on-chain (validates all required fields, signs, broadcasts)
    Publish {
        job_id: String,
    },
}

pub async fn run_draft(cmd: DraftCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        DraftCommand::Create {
            title, description, description_summary, budget, max_budget, currency,
            deadline_open, deadline_submit, provider, attachments,
        } => {
            draft::handle_draft_create(
                &mut client,
                &title,
                description.as_deref(),
                description_summary.as_deref(),
                budget,
                max_budget,
                currency.as_deref(),
                deadline_open.as_deref(),
                deadline_submit.as_deref(),
                provider.as_deref(),
                attachments.as_deref(),
            ).await
        }
        DraftCommand::List { page, limit } => {
            draft::handle_draft_list(&mut client, page, limit).await
        }
        DraftCommand::Update {
            job_id, title, description, budget, max_budget, currency,
            deadline_open, deadline_submit, provider,
        } => {
            draft::handle_draft_update(
                &mut client,
                &job_id,
                title.as_deref(),
                description.as_deref(),
                budget,
                max_budget,
                currency.as_deref(),
                deadline_open.as_deref(),
                deadline_submit.as_deref(),
                provider.as_deref(),
            ).await
        }
        DraftCommand::Delete { job_id } => {
            draft::handle_draft_delete(&mut client, &job_id).await
        }
        DraftCommand::Publish { job_id } => {
            draft::handle_draft_publish(&mut client, &job_id).await
        }
    }
}

