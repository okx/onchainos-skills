//! Queries available, unavailable, and user-released Bitcoin UTXOs.

use anyhow::Result;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::shared::adapters::bitcoin::{
    api::BtcApi,
    context::BtcContext,
    models::{collect_outpoints, BtcOutPoint},
};
use crate::output;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UtxoQueryMode {
    UserIgnored,
    Unavailable,
    Available,
}

impl UtxoQueryMode {
    /// Returns the backend query type for this UTXO view.
    fn query_type(self) -> &'static str {
        match self {
            Self::UserIgnored => "USER_IGNORED_LIST",
            Self::Unavailable => "UNAVAILABLE_BREAKDOWN",
            Self::Available => "AVAILABLE_UTXO_LIST",
        }
    }

    /// Returns the response key used when emitting this UTXO view.
    fn result_key(self) -> &'static str {
        match self {
            Self::UserIgnored => "userIgnored",
            Self::Unavailable => "unavailable",
            Self::Available => "available",
        }
    }

    /// Returns the user-facing summary for this UTXO view.
    fn result_message(self) -> &'static str {
        match self {
            Self::UserIgnored => {
                "Queried Bitcoin UTXOs whose asset occupancy was explicitly removed by the user."
            }
            Self::Unavailable => {
                "Queried unavailable Bitcoin UTXO details and their current service reason categories."
            }
            Self::Available => {
                "Queried currently available Bitcoin UTXOs and their total spendable sats."
            }
        }
    }

    /// Extracts only the outpoints belonging to this response section.
    fn collect_response_outpoints(self, snapshot: &Value) -> Vec<BtcOutPoint> {
        match self {
            Self::UserIgnored => {
                collect_outpoints(snapshot.pointer("/userIgnoredList").unwrap_or(snapshot))
            }
            Self::Unavailable => collect_outpoints(
                snapshot
                    .pointer("/unavailableBreakdown")
                    .unwrap_or(snapshot),
            ),
            Self::Available => {
                collect_outpoints(snapshot.pointer("/availableUtxoList").unwrap_or(snapshot))
            }
        }
    }
}

/// Queries user-ignored Bitcoin UTXOs after asset protection is removed.
pub async fn cmd_user_ignored() -> Result<()> {
    query_utxos(UtxoQueryMode::UserIgnored).await
}

/// Queries and emits the unavailable Bitcoin UTXO breakdown.
pub async fn cmd_unavailable() -> Result<()> {
    query_utxos(UtxoQueryMode::Unavailable).await
}

/// Queries and emits currently spendable Bitcoin UTXOs and totals.
pub async fn cmd_available() -> Result<()> {
    query_utxos(UtxoQueryMode::Available).await
}

/// Executes one UTXO availability query and emits its normalized result.
async fn query_utxos(mode: UtxoQueryMode) -> Result<()> {
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let snapshot = api
        .availability_details(&context, mode.query_type())
        .await?;
    let outpoints = mode.collect_response_outpoints(&snapshot);
    let mut result = json!({
        "message": mode.result_message(),
        "queryType": mode.query_type(),
        "outpointCount": outpoints.len(),
        "accountId": context.account_id,
        "address": context.address.address,
    });
    result[mode.result_key()] = snapshot;
    if mode == UtxoQueryMode::Unavailable {
        let _ = brc20_asset_info(&mut api, &context, &outpoints).await;
    }
    output::success(result);
    Ok(())
}

/// Refreshes unavailable Bitcoin UTXOs and probes their bound BRC-20 assets.
///
/// BTC balance calls this read without adding the asset-detail response to its
/// current output.
pub async fn probe_unavailable_brc20_asset_info() -> Result<()> {
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let snapshot = api
        .availability_details(&context, UtxoQueryMode::Unavailable.query_type())
        .await?;
    let outpoints = UtxoQueryMode::Unavailable.collect_response_outpoints(&snapshot);

    let _ = brc20_asset_info(&mut api, &context, &outpoints).await;
    Ok(())
}

/// Queries BRC-20 asset records for already-normalized availability outpoints.
async fn brc20_asset_info(
    api: &mut BtcApi,
    context: &BtcContext,
    outpoints: &[BtcOutPoint],
) -> Result<Value> {
    let records = api.brc20_utxo_asset_info(context, outpoints).await?;
    Ok(json!({
        "assetProtocols": ["BRC20"],
        "outpointCount": outpoints.len(),
        "records": records,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utxo_query_modes_map_to_distinct_service_views() {
        assert_eq!(UtxoQueryMode::UserIgnored.query_type(), "USER_IGNORED_LIST");
        assert_eq!(UtxoQueryMode::UserIgnored.result_key(), "userIgnored");
        assert!(UtxoQueryMode::UserIgnored
            .result_message()
            .contains("asset occupancy was explicitly removed by the user"));
        assert_eq!(UtxoQueryMode::Unavailable.query_type(), "UNAVAILABLE_BREAKDOWN");
        assert_eq!(UtxoQueryMode::Available.query_type(), "AVAILABLE_UTXO_LIST");
        assert_eq!(UtxoQueryMode::Available.result_key(), "available");
    }

    #[test]
    fn utxo_query_mode_collects_outpoints_from_its_own_response_shape() {
        let user_ignored_hash = "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0";
        let unavailable_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let available_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let snapshot = json!({
            "userIgnoredList": [{"txHash": user_ignored_hash, "voutIndex": 0}],
            "unavailableBreakdown": {
                "assetLocked": [{"txHash": unavailable_hash, "voutIndex": 1}]
            },
            "availableUtxoList": {
                "sumSats": "546",
                "count": 1,
                "utxos": [{"txHash": available_hash, "voutIndex": 2, "valueRaw": "546"}]
            },
        });

        let user_ignored = UtxoQueryMode::UserIgnored.collect_response_outpoints(&snapshot);
        assert_eq!(user_ignored.len(), 1);
        assert_eq!(user_ignored[0].tx_hash, user_ignored_hash);

        let unavailable = UtxoQueryMode::Unavailable.collect_response_outpoints(&snapshot);
        assert_eq!(unavailable.len(), 1);
        assert_eq!(unavailable[0].tx_hash, unavailable_hash);

        let available = UtxoQueryMode::Available.collect_response_outpoints(&snapshot);
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].tx_hash, available_hash);
    }
}
