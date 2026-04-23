pub mod new_tokens;
pub mod portfolio;
pub mod smart_money;
pub mod token_research;
pub mod wallet_analysis;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use super::Context;

#[derive(Subcommand)]
pub enum WorkflowCommand {
    /// W1: Full token due diligence — price, security, holders, signals, optional launchpad
    TokenResearch {
        /// Token contract address
        #[arg(long)]
        address: String,
        /// Chain (e.g. solana, ethereum, base). Auto-detects from global --chain if omitted.
        #[arg(long)]
        chain: Option<String>,
    },

    /// W3: Smart money signals — aggregate signals by token, run per-token due diligence
    SmartMoney {
        /// Chain (defaults to solana)
        #[arg(long)]
        chain: Option<String>,
    },

    /// W4: New token screening — MIGRATED launchpad scan + safety enrichment for top 10
    NewTokens {
        /// Chain (defaults to solana)
        #[arg(long)]
        chain: Option<String>,
        /// Launchpad stage: MIGRATED (default) or MIGRATING
        #[arg(long)]
        stage: Option<String>,
    },

    /// W5: Wallet analysis — 7d/30d performance, trading behaviour, recent activity
    WalletAnalysis {
        /// Wallet address to analyse
        #[arg(long)]
        address: String,
        /// Chain (defaults to solana)
        #[arg(long)]
        chain: Option<String>,
    },

    /// W7: Portfolio check — balances, total value, 30d PnL overview
    Portfolio {
        /// Wallet address
        #[arg(long)]
        address: String,
        /// Comma-separated chains (defaults to all supported)
        #[arg(long)]
        chains: Option<String>,
    },
}

pub async fn execute(ctx: &Context, cmd: WorkflowCommand) -> Result<()> {
    match cmd {
        WorkflowCommand::TokenResearch { address, chain } => {
            token_research::run(ctx, &address, chain).await
        }
        WorkflowCommand::SmartMoney { chain } => smart_money::run(ctx, chain).await,
        WorkflowCommand::NewTokens { chain, stage } => new_tokens::run(ctx, chain, stage).await,
        WorkflowCommand::WalletAnalysis { address, chain } => {
            wallet_analysis::run(ctx, &address, chain).await
        }
        WorkflowCommand::Portfolio { address, chains } => {
            portfolio::run(ctx, &address, chains).await
        }
    }
}

/// Convert a Result<Value> to Value, replacing errors with null.
/// Used throughout all workflow steps so partial failures degrade gracefully.
pub fn ok_or_null(r: Result<Value>) -> Value {
    r.unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ok_or_null ────────────────────────────────────────────────────

    #[test]
    fn ok_or_null_passes_through_ok_value() {
        let val = json!({ "price": "1.23" });
        assert_eq!(ok_or_null(Ok(val.clone())), val);
    }

    #[test]
    fn ok_or_null_converts_error_to_null() {
        let err: Result<Value> = Err(anyhow::anyhow!("API timeout"));
        assert_eq!(ok_or_null(err), Value::Null);
    }

    #[test]
    fn ok_or_null_passes_through_null_value() {
        assert_eq!(ok_or_null(Ok(Value::Null)), Value::Null);
    }

    #[test]
    fn ok_or_null_passes_through_empty_array() {
        assert_eq!(ok_or_null(Ok(json!([]))), json!([]));
    }

    // ── workflow discriminator fields ─────────────────────────────────
    // Sanity-check the string literals used in output JSON so they stay
    // consistent with the workflow doc file names.

    #[test]
    fn workflow_names_match_doc_filenames() {
        // These must match the filenames in workflows/*.md exactly.
        let names = [
            "token-research",
            "smart-money",
            "new-tokens",
            "wallet-analysis",
            "portfolio",
        ];
        for name in names {
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-'),
                "workflow name '{}' contains invalid characters",
                name
            );
        }
    }
}
