//! Provider-side task commands — enum definitions + routing dispatch.
//!
//! Files split by provider action:
//! - `apply.rs`             — apply for a job
//! - `deliver.rs`           — submit deliverable
//! - `agreerefund.rs`       — agree to refund
//! - `dispute_raise.rs`     — raise dispute (on-chain)
//! - `provider_claim.rs`    — claim after submit→complete timeout (claimAutoComplete)
//!
//! account-pull arbitration rewards (`claim-rewards` / `claimable`): called inline in
//! the dispatch arm via `common::claim`; no thin wrapper for provider — the logic is just
//! `signing::resolve_wallet(None, None)` + `common::claim::*`, with no role-specific resolution.
//!
//! Offchain evidence upload (`dispute upload`) is shared by both sides;
//! implementation lives in `common/dispute_upload.rs`.

mod agreerefund;
mod apply;
mod content;
mod deliver;
mod dispute_confirm;
mod dispute_raise;
pub mod find_jobs;
pub mod flow;
mod provider_claim;
pub mod recommend_task;

use anyhow::Result;
use clap::Subcommand;
use std::time::Duration;

use anyhow::bail;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    claim as common_claim, dispute_upload, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;

// ─── provider subcommands ─────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ProviderCommand {
    /// Provider applies for a task (apply API → calldata → sign → broadcast)
    Apply {
        job_id: String,
        #[arg(long = "token-amount", default_value = "0")]
        token_amount: String,
        /// Actual job token (USDT / USDG); read from job detail — do not assume USDT.
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Provider submits deliverable (submit API → sign → broadcast)
    Deliver {
        job_id: String,
        #[arg(long, default_value = "")]
        file: String,
        #[arg(long, default_value = "Task completed, please review")]
        message: String,
        /// Text deliverable content for auto-save. When non-empty and --file is empty,
        /// the CLI writes this to a temp file and persists it as a text deliverable.
        #[arg(long = "deliverable-text", default_value = "")]
        deliverable_text: String,
        /// Provider agentId (required). Beta backend rejects an empty agenticId header → 3001 auth fail;
        /// the providerAgentId field in job detail may be null, so reverse lookup is unreliable.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    AgreeRefund {
        job_id: String,
        /// Provider agentId (required).
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Provider claims after submit→complete timeout (claimAutoComplete API → sign → broadcast)
    ClaimAutoComplete {
        job_id: String,
        /// Provider agentId (required).
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Get current task status (provider view)
    Status {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// List my tasks (provider view)
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "1")]
        page: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Account-pull: query pending rewards (balance accumulated from arbitration wins, etc.).
    Claimable {
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Account-pull: claim all pending rewards in one go.
    ClaimRewards {
        #[arg(long = "agent-id")]
        agent_id: String,
    },
}

// ─── dispute subcommands ──────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DisputeCommand {
    /// Dispute stage 1: call the approve API to grant the dispute contract token approval (calldata → sign → broadcast).
    /// After completion, wait for the on-chain `dispute_approved` notification, then run `dispute confirm` for stage 2.
    Raise {
        job_id: String,
        #[arg(long)]
        reason: String,
        /// Provider agentId (required).
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Dispute stage 2: call the dispute API to actually raise the dispute (calldata → sign → broadcast).
    /// The `dispute_approved` system notification must have been received first. After completion, wait for the `job_disputed` notification.
    Confirm {
        job_id: String,
        /// Provider agentId (required).
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Upload offchain evidence (multipart, 1h preparation window only) — shared by both sides.
    Upload {
        job_id: String,
        /// Caller's own agentId (buyer or provider). Injected by the next-action script so the
        /// client doesn't have to do wallet-to-role mapping (a single wallet may have multiple registered agentIds).
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Text evidence (optional; at least one of text/images is required).
        #[arg(long)]
        text: Option<String>,
        /// Image path (repeatable; only jpg/jpeg/png/gif/webp are supported).
        #[arg(long = "image")]
        images: Vec<String>,
    },
}

// ─── routing dispatch ─────────────────────────────────────────────────────

pub async fn run_provider(cmd: ProviderCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();

    match cmd {
        ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            apply::handle_apply(&mut client, &job_id, &token_amount, &token_symbol, &agent_id).await,
        ProviderCommand::Deliver { job_id, file, message: _, deliverable_text, agent_id } =>
            deliver::handle_deliver(&mut client, &job_id, &file, &deliverable_text, &agent_id).await,
        ProviderCommand::AgreeRefund { job_id, agent_id } =>
            agreerefund::handle_agree_refund(&mut client, &job_id, &agent_id).await,
        ProviderCommand::ClaimAutoComplete { job_id, agent_id } =>
            provider_claim::handle_claim_auto_complete(&mut client, &job_id, &agent_id).await,
        ProviderCommand::Status { job_id, agent_id } => {
            use crate::commands::agent_commerce::task::common::{query as common_query, AGENT_ROLE_PROVIDER};
            common_query::handle_status(&mut client, &job_id, agent_id.as_deref().unwrap_or(""), AGENT_ROLE_PROVIDER).await
        }
        ProviderCommand::List { status, page, limit, agent_id } => {
            use crate::commands::agent_commerce::task::common::{query as common_query, AGENT_ROLE_PROVIDER};
            common_query::handle_list(&mut client, status.as_deref(), page, limit, agent_id.as_deref().unwrap_or(""), AGENT_ROLE_PROVIDER).await
        }

        // account-pull claim calls common::claim inline:
        // provider has no role-specific wallet/agent resolution (unlike evaluator),
        // so a dedicated wrapper file is unnecessary.
        ProviderCommand::Claimable { agent_id } => {
            if agent_id.is_empty() {
                bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
            }
            let has_nonzero =
                common_claim::fetch_and_print_claimable(&mut client, &agent_id).await?;
            audit::log(
                "cli",
                "provider/arbitration_claimable_checked",
                true,
                Duration::default(),
                Some(vec![
                    format!("agentId={agent_id}"),
                    format!("hasClaimable={has_nonzero}"),
                ]),
                None,
            );
            if has_nonzero {
                println!("\nnext: Claimable rewards available — run `onchainos agent provider-claim-rewards --agent-id {agent_id}` to withdraw all at once.");
            } else {
                println!("\n(No pending rewards at this time)");
            }
            Ok(())
        }
        ProviderCommand::ClaimRewards { agent_id } => {
            if agent_id.is_empty() {
                bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
            }
            let (account_id, address) = signing::resolve_wallet(None, None)?;
            let tx_hash =
                common_claim::submit_claim_and_broadcast(&mut client, &account_id, &address, &agent_id).await?;
            audit::log(
                "cli",
                "provider/arbitration_claimed",
                true,
                Duration::default(),
                Some(vec![
                    format!("agentId={agent_id}"),
                    format!("account={address}"),
                    format!("txHash={tx_hash}"),
                ]),
                None,
            );
            println!("✓ reward claim submitted (account={address})");
            println!("  txHash: {tx_hash}");
            println!("note: All settled dispute rewards are claimed in one go; the credited amount will be notified after on-chain confirmation.");
            Ok(())
        }
    }
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    let mut client = TaskApiClient::new();
    match cmd {
        DisputeCommand::Raise { job_id, reason, agent_id } =>
            dispute_raise::handle_dispute_raise(&mut client, &job_id, &reason, &agent_id).await,
        DisputeCommand::Confirm { job_id, agent_id } =>
            dispute_confirm::handle_dispute_confirm(&mut client, &job_id, &agent_id).await,
        DisputeCommand::Upload { job_id, agent_id, text, images } =>
            dispute_upload::handle_upload_evidence(
                &mut client, &job_id, &agent_id, text.as_deref(), &images,
            ).await,
    }
}
