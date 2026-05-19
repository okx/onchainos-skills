use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};

use crate::{output, wallet_api::WalletApiClient, wallet_store};

/// Chain cache TTL: 10 minutes.
pub const CHAIN_CACHE_TTL: i64 = 600;

#[derive(Subcommand)]
pub enum ChainCommand {
    /// List all supported chains (served from local cache; refreshes every 10 minutes)
    List,
}

pub async fn execute(command: ChainCommand) -> Result<()> {
    match command {
        ChainCommand::List => cmd_list().await,
    }
}

/// Show supported chains. Reads from cache if fresh; otherwise fetches from API and updates cache.
async fn cmd_list() -> Result<()> {
    let chains = get_all_chains().await?;
    output::success(Value::Array(chains));
    Ok(())
}

/// Return all supported chains (cache-first, then API fallback).
/// Can be called from other modules.
pub async fn get_all_chains() -> Result<Vec<Value>> {
    if let Some(cached) = wallet_store::get_chain_cache(CHAIN_CACHE_TTL)? {
        return Ok(cached.chains);
    }
    let chains = fetch_chains_from_api().await?;
    wallet_store::set_chain_cache(chains.clone())?;
    Ok(chains)
}

/// Look up a single chain entry by `chainIndex`.
/// Returns `None` if no chain matches.
pub async fn get_chain_by_index(chain_index: &str) -> Result<Option<Value>> {
    let chains = get_all_chains().await?;
    Ok(chains.into_iter().find(|c| {
        c.get("chainIndex")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .is_some_and(|idx| idx == chain_index)
    }))
}

/// Resolve the `realChainIndex` (EVM chain ID) for a given `chainIndex`.
pub async fn get_real_chain_index(chain_index: &str) -> Result<u64> {
    let entry = get_chain_by_index(chain_index).await?.ok_or_else(|| {
        anyhow::anyhow!("Chain index {} not found in supported chains", chain_index)
    })?;
    entry["realChainIndex"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| entry["realChainIndex"].as_u64())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot resolve realChainIndex for chain index {}",
                chain_index
            )
        })
}

/// Look up a single chain entry by `chainName` (case-insensitive).
/// Returns `None` if no chain matches.
pub async fn get_chain_by_name(chain_name: &str) -> Result<Option<Value>> {
    let chains = get_all_chains().await?;
    let lower = chain_name.to_lowercase();
    Ok(chains.into_iter().find(|c| {
        c.get("chainName")
            .and_then(|v| v.as_str())
            .is_some_and(|n| n.to_lowercase() == lower)
    }))
}

/// Look up a single chain entry by `realChainIndex` or chain name.
/// Accepts both numeric IDs (e.g. "1", "501") and names (e.g. "ethereum", "solana").
/// Returns `None` if no chain matches after resolution.
pub async fn get_chain_by_real_chain_index(input: &str) -> Result<Option<Value>> {
    let resolved = crate::chains::resolve_chain(input);
    let chains = get_all_chains().await?;
    Ok(chains.into_iter().find(|c| {
        c.get("realChainIndex")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .is_some_and(|idx| idx == resolved)
    }))
}

/// Force-refresh the chain cache from the remote API, bypassing TTL.
/// Errors are ignored so callers are not disrupted.
pub(crate) async fn force_refresh_chain_cache() {
    if let Ok(chains) = fetch_chains_from_api().await {
        let _ = wallet_store::set_chain_cache(chains);
    }
}

/// Resolve `showName` for an EVM chain by `realChainIndex`, reading only
/// from the locally cached chain list. No network I/O and no TTL check —
/// stale display names are still informative, and the notification path
/// must not block on a chain-list refresh.
pub fn show_name_for_real_id_sync(real_chain_id: u64) -> Option<String> {
    let cached = wallet_store::get_chain_cache(i64::MAX).ok().flatten()?;
    let target = real_chain_id.to_string();
    cached.chains.into_iter().find_map(|c| {
        let real = c.get("realChainIndex").and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })?;
        if real != target {
            return None;
        }
        c.get("showName").and_then(|v| v.as_str()).map(String::from)
    })
}

/// Fetch the supported chain list from the remote API.
async fn fetch_chains_from_api() -> Result<Vec<Value>> {
    let mut client = WalletApiClient::new()?;
    let data = client
        .post_public("/priapi/v5/wallet/agentic/chain/support/list", &json!({}))
        .await?;
    let chains = data
        .as_array()
        .or_else(|| data["chainList"].as_array())
        .cloned()
        .unwrap_or_default();
    Ok(chains)
}

/// Silently refresh the chain cache if it has expired. Errors are ignored so that
/// callers (e.g. `wallet balance`) are not disrupted by a background cache miss.
pub async fn ensure_chain_cache_fresh() {
    if wallet_store::get_chain_cache(CHAIN_CACHE_TTL)
        .ok()
        .flatten()
        .is_some()
    {
        return; // still fresh
    }
    force_refresh_chain_cache().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_cache_ttl_is_600() {
        assert_eq!(CHAIN_CACHE_TTL, 600);
    }
}
