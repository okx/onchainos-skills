pub mod gateway;
pub mod market;
pub mod portfolio;
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

    /// Create an OKX API client with HMAC-SHA256 authentication.
    pub fn client(&self) -> Result<ApiClient> {
        ApiClient::new(self.base_url_override.as_deref())
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
