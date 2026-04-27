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
    /// Full token due diligence — price, security, holders, signals, optional launchpad.
    /// Accepts either --address (contract address) or --query (symbol/name search).
    /// When --query is used, returns top 5 search results for user selection.
    TokenResearch {
        /// Token contract address (use this OR --query)
        #[arg(long)]
        address: Option<String>,
        /// Token symbol or name to search (use this OR --address).
        /// Returns top 5 matches for selection before running the full workflow.
        #[arg(long)]
        query: Option<String>,
        /// Chain (e.g. solana, ethereum, base). Auto-detects from global --chain if omitted.
        #[arg(long)]
        chain: Option<String>,
    },

    /// Smart money signals — aggregate signals by token, run per-token due diligence
    SmartMoney {
        /// Chain (defaults to solana)
        #[arg(long)]
        chain: Option<String>,
    },

    /// New token screening — MIGRATED launchpad scan + safety enrichment for top 10
    NewTokens {
        /// Chain (defaults to solana)
        #[arg(long)]
        chain: Option<String>,
        /// Launchpad stage: MIGRATED (default) or MIGRATING
        #[arg(long)]
        stage: Option<String>,
    },

    /// Wallet analysis — 7d/30d performance, trading behaviour, recent activity
    WalletAnalysis {
        /// Wallet address to analyse
        #[arg(long)]
        address: String,
        /// Chain (defaults to solana)
        #[arg(long)]
        chain: Option<String>,
    },

    /// Portfolio check — balances, total value, 30d PnL overview
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
        WorkflowCommand::TokenResearch {
            address,
            query,
            chain,
        } => token_research::run(ctx, address.as_deref(), query.as_deref(), chain).await,
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
        // `CARGO_MANIFEST_DIR` points at `cli/`, so the docs live one level up.
        let names = [
            "token-research",
            "smart-money-signals",
            "new-token-screening",
            "wallet-analysis",
            "portfolio-check",
        ];
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        for name in names {
            let path = std::path::Path::new(manifest_dir)
                .join("..")
                .join("workflows")
                .join(format!("{name}.md"));
            assert!(
                path.exists(),
                "workflow doc file missing: {} (expected at {})",
                name,
                path.display()
            );
        }
    }
}
