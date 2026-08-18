//! Executes native Bitcoin and BRC-20 transfers.

use anyhow::Result;
use serde_json::json;

use crate::commands::agentic_wallet::support::amount::readable_to_minimal;

use super::super::chain_adapters::bitcoin::{
    api::{self, BtcApi},
    broadcast,
    context::BtcContext,
    error, signing, validation,
};
use super::super::utxo::select_brc20_transferable_utxo;

/// Executes the BTC/BRC-20 send flow and emits confirmation or broadcast output.
pub async fn cmd_send(
    readable_amount: Option<&str>,
    recipient: &str,
    from: Option<&str>,
    token_address: Option<&str>,
    brc20_outpoint: Option<&str>,
    force: bool,
) -> Result<()> {
    validation::validate_recipient(recipient)?;
    let normalized_token_address = token_address
        .map(validation::normalize_brc20_token_address)
        .transpose()?;
    let token_address = normalized_token_address.as_deref();
    let context = BtcContext::load(from).await?;
    let mut api = BtcApi::new()?;

    let (amount, symbol, selected_tx_param) = match token_address {
        Some(token_address) => {
            let selection = brc20_outpoint.ok_or_else(|| {
                anyhow::anyhow!(
                    "BRC-20 transfers require --brc20-outpoint selected from wallet utxo brc20-transferable"
                )
            })?;
            let snapshot = api
                .brc20_transferable_utxos(&context, token_address)
                .await?;
            let selected = select_brc20_transferable_utxo(&snapshot, selection)?;
            if let Some(readable_amount) = readable_amount {
                let metadata = api.token_metadata(&context, token_address).await?;
                let decimals = api::extract_token_decimals(&metadata)?;
                let requested = readable_to_minimal(readable_amount, decimals)?;
                if requested != selected.value_raw {
                    anyhow::bail!(
                        "--readable-amount does not match the selected BRC-20 UTXO amount"
                    );
                }
            }
            let selected_amount = selected.value_raw.clone();
            let tx_param = json!({
                "amount": selected_amount,
                "inputs": [selected.build_tx_param_input(&context.address.address)],
            });
            (selected_amount, token_address.to_string(), Some(tx_param))
        }
        None => {
            if brc20_outpoint.is_some() {
                anyhow::bail!("--brc20-outpoint requires a BRC-20 --contract-token");
            }
            let readable_amount =
                readable_amount.ok_or_else(|| anyhow::anyhow!("--readable-amount is required"))?;
            let amount = readable_to_minimal(readable_amount, context.profile.native_decimals)?;
            (amount, context.profile.native_symbol.clone(), None)
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
        "selectedBrc20Outpoint": brc20_outpoint,
        "txHash": submitted["txHash"],
        "orderId": submitted["orderId"],
        "broadcasts": [submitted],
    }));
    Ok(())
}
