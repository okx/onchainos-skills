pub mod agentic_wallet;
pub mod upgrade;
pub mod gateway;
pub mod market;
pub mod memepump;
pub mod portfolio;
pub mod security;
pub mod signal;
pub mod swap;
pub mod token;

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
}
