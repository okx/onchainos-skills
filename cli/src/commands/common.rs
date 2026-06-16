//! Shared helpers for DEX commands.
//!
//! Hosts the on-chain tx-waiting primitives extracted from `swap.rs` so that
//! both `swap.rs` and `cross_chain.rs` can compose the same approve → wait →
//! swap pattern (spec §10.4, TBC[1]).
//!
//! Scope is intentionally narrow (TBC[2]): only `wait_tx_onchain` and
//! `tx_confirmation_timeout` are extracted. `unwrap_api_array` is duplicated
//! here as a private helper used only by `wait_tx_onchain`; the original
//! copies in `swap.rs` / `cross_chain.rs` stay byte-for-byte untouched to
//! keep the blast radius minimal.

use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::Value;

use crate::client::ApiClient;

/// Per-chain confirmation timeout for [`wait_tx_onchain`]. Picked to cover a
/// typical block time with a small buffer; falls back to a generous default
/// for unknown chains so the poller still bounds.
pub(crate) fn tx_confirmation_timeout(chain_index: &str) -> Duration {
    match chain_index {
        // ETH, Linea
        "1" | "59144" => Duration::from_secs(20),
        _ => Duration::from_secs(10),
    }
}

/// Poll the public DEX tx-history endpoint until the tx confirms on-chain
/// (`txStatus == "success"`) or the per-chain timeout elapses.
///
/// GET `/api/v6/dex/post-transaction/transaction-detail-by-txhash`
pub(crate) async fn wait_tx_onchain(
    client: &mut ApiClient,
    tx_hash: &str,
    chain_index: &str,
) -> Result<()> {
    use std::time::Instant;

    let timeout = tx_confirmation_timeout(chain_index);
    let poll_interval = Duration::from_secs(1);
    let deadline = Instant::now() + timeout;

    loop {
        let result = client
            .get(
                "/api/v6/dex/post-transaction/transaction-detail-by-txhash",
                &[("chainIndex", chain_index), ("txHash", tx_hash)],
            )
            .await;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][wait_tx_onchain] tx={} chain={} response={:?}",
                tx_hash, chain_index, result
            );
        }

        if let Ok(data) = result {
            let detail = unwrap_api_array(&data);
            let status = detail["txStatus"].as_str().unwrap_or("");
            if status.eq_ignore_ascii_case("success") {
                return Ok(());
            }
            if status.eq_ignore_ascii_case("fail") {
                bail!("tx {} failed on-chain (chain={})", tx_hash, chain_index);
            }
        }

        if Instant::now() >= deadline {
            bail!(
                "tx {} not confirmed on-chain within {}s (chain={})",
                tx_hash,
                timeout.as_secs(),
                chain_index
            );
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// If the API returns an array, extract the first element; otherwise return as-is.
fn unwrap_api_array(data: &Value) -> Value {
    if data.is_array() {
        data.as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_confirmation_timeout_eth_is_twenty_seconds() {
        // Spec §6.2: ETH (chain index "1") → 20s.
        assert_eq!(tx_confirmation_timeout("1"), Duration::from_secs(20));
    }

    #[test]
    fn tx_confirmation_timeout_linea_is_twenty_seconds() {
        // Spec §6.2: Linea (chain index "59144") → 20s.
        assert_eq!(tx_confirmation_timeout("59144"), Duration::from_secs(20));
    }

    #[test]
    fn tx_confirmation_timeout_base_falls_back_to_ten_seconds() {
        // Spec §6.2: all other chains (e.g. Base "8453") → 10s.
        assert_eq!(tx_confirmation_timeout("8453"), Duration::from_secs(10));
    }
}
