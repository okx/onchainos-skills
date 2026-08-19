//! Executes native Bitcoin and BRC-20 transfers.

use anyhow::Result;
use num_bigint::BigUint;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::support::amount::{parse_minimal, readable_to_minimal};

use super::super::chain_adapters::bitcoin::{
    api::{self, BtcApi},
    broadcast,
    context::BtcContext,
    error, signing, validation,
};
use super::super::utxo::select_brc20_transferable_utxos;

/// Executes the BTC/BRC-20 send flow and emits confirmation or broadcast output.
pub async fn cmd_send(
    readable_amount: Option<&str>,
    recipient: &str,
    from: Option<&str>,
    token_address: Option<&str>,
    brc20_outpoints: &[String],
    force: bool,
) -> Result<()> {
    validation::validate_recipient(recipient)?;
    let normalized_token_address = token_address
        .map(validation::normalize_brc20_token_address)
        .transpose()?;
    let token_address = normalized_token_address.as_deref();
    let context = BtcContext::load(from).await?;
    let mut api = BtcApi::new()?;

    let (amount, symbol, selected_tx_param, selected_outpoints) = match token_address {
        Some(token_address) => {
            let snapshot = api
                .brc20_transferable_utxos(&context, token_address)
                .await?;
            let requested_amount = if let Some(readable_amount) = readable_amount {
                let metadata = api.token_metadata(&context, token_address).await?;
                let decimals = api::extract_token_decimals(&metadata)?;
                Some(readable_to_minimal(readable_amount, decimals)?)
            } else {
                None
            };
            let (selected_amount, tx_param, selected_outpoints) = build_brc20_transfer_parameters(
                &snapshot,
                brc20_outpoints,
                &context.address.address,
                requested_amount.as_deref(),
            )?;
            (
                selected_amount,
                token_address.to_string(),
                Some(tx_param),
                selected_outpoints,
            )
        }
        None => {
            if !brc20_outpoints.is_empty() {
                anyhow::bail!("--brc20-outpoint requires a BRC-20 --contract-token");
            }
            let readable_amount =
                readable_amount.ok_or_else(|| anyhow::anyhow!("--readable-amount is required"))?;
            let amount = readable_to_minimal(readable_amount, context.profile.native_decimals)?;
            (
                amount,
                context.profile.native_symbol.clone(),
                None,
                Vec::new(),
            )
        }
    };

    let prepared =
        if let (Some(token_address), Some(tx_param)) = (token_address, selected_tx_param) {
            api.prepare_selected_brc20_transfer(
                &context,
                recipient,
                &amount,
                token_address,
                tx_param,
            )
            .await
        } else {
            api.prepare_transaction(&context, recipient, &amount, None, None, None, None)
                .await
        }
        .map_err(error::map_api_error)?;

    let seed = context.signing_seed()?;
    let signatures = signing::sign_unsigned_hashes(&prepared, &seed)?;
    let broadcast =
        broadcast::submit_direct_transaction(&mut api, &context, &prepared, &signatures, force)
            .await
            .map_err(|error| {
                crate::commands::agentic_wallet::common::handle_confirming_error(error, force)
            })
            .map_err(error::map_api_error)?;
    let submitted = json!({
        "txHash": broadcast.tx_hash,
        "orderId": broadcast.order_id,
    });
    crate::output::success(json!({
        "message": "Bitcoin transaction submitted. The final result is pending network confirmation.",
        "state": "PENDING",
        "accountId": context.account_id,
        "chainIndex": context.profile.chain_index,
        "from": context.address.address,
        "to": recipient,
        "asset": symbol,
        "amount": amount,
        "selectedBrc20Outpoints": selected_outpoints,
        "txHash": submitted["txHash"],
        "orderId": submitted["orderId"],
        "broadcasts": [submitted],
    }));
    Ok(())
}

/// Builds one BRC-20 transfer request from the selected current carrier UTXOs.
fn build_brc20_transfer_parameters(
    snapshot: &Value,
    selections: &[String],
    address: &str,
    requested_amount: Option<&str>,
) -> Result<(String, Value, Vec<String>)> {
    let selected = select_brc20_transferable_utxos(snapshot, selections)?;
    let mut total = BigUint::default();
    for (index, utxo) in selected.iter().enumerate() {
        total += parse_minimal(
            &utxo.value_raw,
            &format!("selected BRC-20 UTXO {index} valueRaw"),
            false,
        )?;
    }
    let selected_amount = total.to_string();
    if requested_amount.is_some_and(|requested| requested != selected_amount) {
        anyhow::bail!("--readable-amount does not match the combined BRC-20 UTXO amount");
    }
    let inputs = selected
        .iter()
        .map(|utxo| utxo.build_tx_param_input(address))
        .collect::<Vec<_>>();
    let selected_outpoints = selected
        .iter()
        .map(|utxo| utxo.outpoint.canonical())
        .collect::<Vec<_>>();
    Ok((
        selected_amount.clone(),
        json!({
            "amount": selected_amount,
            "inputs": inputs,
        }),
        selected_outpoints,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transferable_snapshot() -> Value {
        json!({
            "brc20TransferableUtxoList": {
                "utxos": [{
                    "txHash": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "voutIndex": 2,
                    "utxoId": "utxo-1",
                    "utxoAmountRaw": "546",
                    "valueRaw": "1000000000000000000",
                    "offset": "0",
                    "inscriptionId": "inscription-1"
                }, {
                    "txHash": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                    "voutIndex": 3,
                    "utxoId": "utxo-2",
                    "utxoAmountRaw": "600",
                    "valueRaw": "2000000000000000000",
                    "offset": "1",
                    "inscriptionId": "inscription-2"
                }]
            }
        })
    }

    #[test]
    fn combines_selected_brc20_utxos_into_one_transfer_request() {
        let selections = vec![
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc:2".to_string(),
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd:3".to_string(),
        ];

        let (amount, tx_param, selected_outpoints) = build_brc20_transfer_parameters(
            &transferable_snapshot(),
            &selections,
            "bc1pfrom",
            Some("3000000000000000000"),
        )
        .unwrap();

        assert_eq!(amount, "3000000000000000000");
        assert_eq!(tx_param["amount"], "3000000000000000000");
        assert_eq!(tx_param["inputs"].as_array().unwrap().len(), 2);
        assert_eq!(tx_param["inputs"][0]["amount"], "546");
        assert_eq!(tx_param["inputs"][1]["amount"], "600");
        assert_eq!(selected_outpoints, selections);
    }

    #[test]
    fn rejects_brc20_requested_amount_that_differs_from_selected_sum() {
        let selections = vec![
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc:2".to_string(),
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd:3".to_string(),
        ];

        let error = build_brc20_transfer_parameters(
            &transferable_snapshot(),
            &selections,
            "bc1pfrom",
            Some("2000000000000000000"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("combined BRC-20 UTXO amount"));
    }
}
