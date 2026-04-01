pub mod agentic_wallet;
pub mod defi;
pub mod gateway;
pub mod leaderboard;
pub mod market;
pub mod memepump;
pub mod portfolio;
pub mod security;
pub mod signal;
pub mod swap;
pub mod token;
pub mod tracker;
pub mod upgrade;

use crate::chains;
use crate::client::ApiClient;
use crate::config::AppConfig;
use crate::Cli;
use anyhow::Result;

/// Shared execution context for all commands.
pub struct Context {
    pub config: AppConfig,
    pub base_url_override: Option<String>,
    pub chain_override: Option<String>,
}

impl Context {
    pub fn new(cli: &Cli) -> Self {
        let config = AppConfig::load().unwrap_or_default();
        Self {
            config,
            base_url_override: cli.base_url.clone(),
            chain_override: cli.chain.clone(),
        }
    }

    /// Create an OKX API client with HMAC-SHA256 authentication (no JWT expiry check).
    /// Prefer `client_async()` in async command handlers.
    pub fn client(&self) -> Result<ApiClient> {
        ApiClient::new(self.base_url_override.as_deref())
    }

    /// Create an OKX API client with full JWT lifecycle check:
    /// expired JWT → auto-refresh; refresh token expired → AK / anonymous fallback.
    pub async fn client_async(&self) -> Result<ApiClient> {
        ApiClient::new_async(self.base_url_override.as_deref()).await
    }

    /// Resolve chain to OKX chainIndex (e.g. "ethereum" -> "1", "solana" -> "501").
    pub fn chain_index(&self) -> Option<String> {
        let chain = self
            .chain_override
            .as_deref()
            .or(if self.config.default_chain.is_empty() {
                None
            } else {
                Some(self.config.default_chain.as_str())
            })?;
        Some(chains::resolve_chain(chain).to_string())
    }

    pub fn chain_index_or(&self, default: &str) -> String {
        self.chain_index()
            .unwrap_or_else(|| chains::resolve_chain(default).to_string())
    }

    /// Resolve an optional `--chains` arg: use the explicit value if provided,
    /// fall back to the global `--chain` CLI override only (ignores persisted
    /// default_chain to avoid silently narrowing cross-chain queries).
    pub fn resolve_chains_or(&self, explicit: Option<String>, default: &str) -> String {
        explicit.unwrap_or_else(|| {
            self.chain_override
                .as_ref()
                .map(|c| chains::resolve_chain(c).to_string())
                .unwrap_or_else(|| default.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_no_override() -> Context {
        Context {
            config: AppConfig::default(),
            base_url_override: None,
            chain_override: None,
        }
    }

    fn ctx_with_override(chain: &str) -> Context {
        Context {
            config: AppConfig::default(),
            base_url_override: None,
            chain_override: Some(chain.to_string()),
        }
    }

    #[test]
    fn resolve_chains_or_uses_explicit_when_provided() {
        let ctx = ctx_with_override("solana");
        assert_eq!(
            ctx.resolve_chains_or(Some("ethereum".into()), "1,501"),
            "ethereum"
        );
    }

    #[test]
    fn resolve_chains_or_falls_back_to_chain_override() {
        let ctx = ctx_with_override("solana");
        assert_eq!(ctx.resolve_chains_or(None, "1,501"), "501");
    }

    #[test]
    fn resolve_chains_or_uses_default_when_no_override() {
        let ctx = ctx_no_override();
        assert_eq!(ctx.resolve_chains_or(None, "1,501"), "1,501");
    }

    #[test]
    fn resolve_chains_or_ignores_persisted_default_chain() {
        let mut ctx = ctx_no_override();
        ctx.config.default_chain = "xlayer".to_string();
        // Should still return the hardcoded default, NOT resolve xlayer
        assert_eq!(ctx.resolve_chains_or(None, "1,501"), "1,501");
    }
}
