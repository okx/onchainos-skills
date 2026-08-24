//! Reclaims Bitcoin UTXOs occupied by removed or pending transactions.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::str::FromStr;

use crate::commands::agentic_wallet::chain_adapters::bitcoin::{
    api::BtcApi,
    context::BtcContext,
    error,
    models::{collect_outpoints, BtcOutPoint},
};
use crate::commands::agentic_wallet::common::WalletPreviewConfirming;
use crate::commands::agentic_wallet::support::json::shell_arg;
use crate::commands::sink::CodedError;
use crate::output;

/// Reviews or closes mempool-removed transactions to release service-side UTXO occupancy.
pub async fn cmd_reclaim(tx_hashes: &[String], force: bool) -> Result<()> {
    if tx_hashes.is_empty() {
        bail!("at least one --tx-hash is required");
    }
    let requested: BTreeSet<String> = tx_hashes
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect();
    if requested.len() != tx_hashes.len() {
        bail!("duplicate --tx-hash values are not allowed");
    }
    for tx_hash in &requested {
        bitcoin::Txid::from_str(tx_hash)
            .map_err(|error| anyhow::anyhow!("invalid --tx-hash '{tx_hash}': {error}"))?;
    }
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let unavailable = api
        .availability_details(&context, "UNAVAILABLE_BREAKDOWN")
        .await?;
    let mempool_removed = unavailable
        .pointer("/unavailableBreakdown/mempoolRemovedSpending")
        .cloned()
        .unwrap_or(Value::Null);
    let outpoints = collect_outpoints(&mempool_removed);
    if outpoints.is_empty() {
        return Err(CodedError::new(
            "NO_RECLAIMABLE_UTXO",
            None,
            "The latest UTXO snapshot has no mempool-removed spending occupancy to reclaim",
        )
        .with_data(json!({"mempoolRemovedSpending": mempool_removed}))
        .into());
    }
    let canonical_hashes: Vec<String> = requested.into_iter().collect();
    let mut transaction_details = Vec::with_capacity(canonical_hashes.len());
    for tx_hash in &canonical_hashes {
        transaction_details.push(api.order_detail(&context, Some(tx_hash), None).await?);
    }

    if !force {
        let next_hashes = canonical_hashes
            .iter()
            .map(|hash| format!(" --tx-hash {}", shell_arg(hash)))
            .collect::<String>();
        let preview = json!({
            "operationType": "RECLAIM_MEMPOOL_REMOVED_UTXOS",
            "chainIndex": context.profile.chain_index,
            "network": "bitcoin",
            "from": context.address.address,
            "txHashList": canonical_hashes,
            "currentMempoolRemovedInputOutpoints": outpoints.iter().map(BtcOutPoint::canonical).collect::<Vec<_>>(),
            "transactionDetails": transaction_details,
            "mempoolRemovedSpending": mempool_removed,
            "effect": "Close the removed original transaction and release service-side spending occupancy for inputs that remain unspent on chain.",
        });
        return Err(WalletPreviewConfirming {
            message: "Review the original transaction hashes and the current mempool-removed occupancy snapshot. The service validates their reclaim relationship. Reclaim closes removed transactions; it does not broadcast a transaction or create an unconfirmed change output.".to_string(),
            next: format!(
                "onchainos wallet utxo reclaim --chain bitcoin{next_hashes} --force"
            ),
            scene: "btc_utxo_reclaim".to_string(),
            preview,
        }
        .into());
    }

    let result = api
        .close_transactions(&context, &canonical_hashes)
        .await
        .map_err(error::map_api_error)?;
    let failed = validate_close_result(&result, &canonical_hashes)?;
    let latest_unavailable = api
        .availability_details(&context, "UNAVAILABLE_BREAKDOWN")
        .await?;
    if !failed.is_empty() {
        return Err(CodedError::new(
            "RECLAIM_NOT_CLOSED",
            None,
            "One or more mempool-removed transactions were not closed",
        )
        .with_data(json!({
            "failedTxHashes": failed,
            "result": result,
            "unavailable": latest_unavailable,
        }))
        .into());
    }
    output::success(json!({
        "message": "The close-transaction request finished. Use each returned closed value and the latest unavailable UTXO snapshot as the authoritative result.",
        "result": result,
        "unavailable": latest_unavailable,
    }));
    Ok(())
}

/// Validates that close results cover every requested hash and returns failed hashes.
fn validate_close_result(result: &Value, requested: &[String]) -> Result<Vec<String>> {
    let items = result
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("close-transaction response data must be an array"))?;
    let requested: BTreeSet<String> = requested
        .iter()
        .map(|hash| hash.to_ascii_lowercase())
        .collect();
    let mut returned = BTreeSet::new();
    let mut failed = Vec::new();
    for item in items {
        let tx_hash = item
            .get("txHash")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("close-transaction item is missing txHash"))?
            .to_ascii_lowercase();
        if !requested.contains(&tx_hash) {
            bail!("close-transaction returned an unexpected txHash {tx_hash}");
        }
        if !returned.insert(tx_hash.clone()) {
            bail!("close-transaction returned duplicate txHash {tx_hash}");
        }
        match item.get("closed").and_then(Value::as_bool) {
            Some(true) => {}
            Some(false) => failed.push(tx_hash),
            None => bail!("close-transaction item is missing closed"),
        }
    }
    if returned != requested {
        bail!("close-transaction response does not cover every requested txHash");
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reclaim_confirmation_command_stays_in_utxo_domain() {
        let flags = ["aa", "bb"]
            .iter()
            .map(|hash| format!(" --tx-hash {}", shell_arg(hash)))
            .collect::<String>();
        let next = format!("onchainos wallet utxo reclaim --chain bitcoin{flags} --force");
        assert!(next.contains("wallet utxo reclaim"));
        assert!(!next.contains("broadcast"));
    }

    #[test]
    fn close_result_preserves_partial_failure() {
        let requested = vec!["aa".to_string(), "bb".to_string()];
        let failed = validate_close_result(
            &json!([
                {"txHash": "aa", "closed": true},
                {"txHash": "bb", "closed": false}
            ]),
            &requested,
        )
        .unwrap();
        assert_eq!(failed, vec!["bb"]);
    }
}
