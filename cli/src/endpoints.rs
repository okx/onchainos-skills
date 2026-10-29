//! Centralized OKX service endpoints used by the CLI.
//!
//! These values are compile-time constants sourced exclusively from `cli/.env`
//! by `build.rs`. The hidden CLI development mode can select the beta HTTP
//! origin; process environment variables and persisted user configuration
//! cannot override endpoints at runtime.

use std::sync::atomic::{AtomicBool, Ordering};

/// Primary OnchainOS HTTP API origin.
pub const BASE_URL: &str = env!("ONCHAINOS_COMPILED_BASE_URL");

/// Internal development HTTP origin selected by the hidden `--dev` switch.
pub const DEV_BASE_URL: &str = "https://beta.okex.org";

static DEV_MODE: AtomicBool = AtomicBool::new(false);

/// Select the internal development HTTP origin for this process.
pub fn set_dev_mode(enabled: bool) {
    DEV_MODE.store(enabled, Ordering::Relaxed);
}

/// Effective HTTP origin for the current process.
pub fn base_url() -> &'static str {
    if DEV_MODE.load(Ordering::Relaxed) {
        DEV_BASE_URL
    } else {
        BASE_URL
    }
}

/// Whether the effective HTTP origin should bypass production DoH failover.
pub fn base_url_is_custom() -> bool {
    DEV_MODE.load(Ordering::Relaxed) || env!("ONCHAINOS_COMPILED_BASE_URL_CUSTOM") == "1"
}

/// Production hostname used by DoH failover when the HTTP endpoint is not customized.
pub const API_HOST: &str = "web3.okx.com";

/// Agent identity push endpoint compiled from `OKX_AGENTIC_WS_URL` in `cli/.env`.
pub const AGENT_IDENTITY_WS_URL: &str = env!("ONCHAINOS_COMPILED_AGENT_IDENTITY_WS_URL");

/// DEX WebSocket endpoint compiled from `ONCHAINOS_WS_URL` in `cli/.env`.
pub const WS_URL: &str = env!("ONCHAINOS_COMPILED_WS_URL");

/// Internal development DEX WebSocket endpoint selected by the hidden `--dev` switch.
pub const DEV_WS_URL: &str = "wss://wsdexpre.okx.com:8443/ws/v6/dex";

/// Effective DEX WebSocket endpoint for a persisted production or development session.
pub fn dex_ws_url(dev_mode: bool) -> &'static str {
    if dev_mode {
        DEV_WS_URL
    } else {
        WS_URL
    }
}

/// X Layer mainnet JSON-RPC endpoint.
pub const XLAYER_RPC_URL: &str = "https://rpc.xlayer.tech";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_endpoints_have_expected_schemes() {
        assert!(BASE_URL.starts_with("http://") || BASE_URL.starts_with("https://"));
        assert_eq!(DEV_BASE_URL, "https://beta.okex.org");
        assert_eq!(API_HOST, "web3.okx.com");
        assert!(
            AGENT_IDENTITY_WS_URL.starts_with("ws://")
                || AGENT_IDENTITY_WS_URL.starts_with("wss://")
        );
        assert!(WS_URL.starts_with("ws://") || WS_URL.starts_with("wss://"));
        assert_eq!(DEV_WS_URL, "wss://wsdexpre.okx.com:8443/ws/v6/dex");
        assert_eq!(XLAYER_RPC_URL, "https://rpc.xlayer.tech");
    }

    #[test]
    fn dex_websocket_uses_pre_endpoint_in_dev_mode() {
        assert_eq!(dex_ws_url(true), DEV_WS_URL);
    }

    #[test]
    fn dex_websocket_uses_compiled_endpoint_by_default() {
        assert_eq!(dex_ws_url(false), WS_URL);
    }
}
