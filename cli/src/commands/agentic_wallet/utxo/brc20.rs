//! Queries and selects transferable BRC-20 carrier UTXOs.

use anyhow::Result;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::chain_adapters::bitcoin::{
    api::{self, BtcApi},
    context::BtcContext,
    models::BtcOutPoint,
    validation,
};
use crate::commands::agentic_wallet::support::amount::{
    minimal_to_readable, value_as_decimal_string,
};
use crate::output;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brc20TransferableUtxo {
    pub outpoint: BtcOutPoint,
    pub utxo_id: String,
    pub utxo_amount_raw: String,
    pub value_raw: String,
    pub offset: Option<String>,
    pub inscription_id: String,
}

impl Brc20TransferableUtxo {
    /// Builds the unsignedInfo input object for this carrier UTXO and source address.
    pub fn build_tx_param_input(&self, address: &str) -> Value {
        json!({
            "txId": self.outpoint.tx_hash,
            "vout": self.outpoint.vout_index,
            "amount": self.utxo_amount_raw,
            "address": address,
        })
    }

    /// Builds one user-selectable BRC-20 UTXO entry with readable and raw amounts.
    fn build_choice(&self, token_address: &str, decimals: u32) -> Result<Value> {
        Ok(json!({
            "selection": self.outpoint.canonical(),
            "tokenAddress": token_address,
            "tokenAmount": minimal_to_readable(&self.value_raw, decimals)?,
            "tokenAmountRaw": self.value_raw,
            "utxoAmountSats": self.utxo_amount_raw,
            "utxoId": self.utxo_id,
            "offset": self.offset,
            "inscriptionId": self.inscription_id,
        }))
    }
}

/// Queries and emits the transferable inscription UTXOs for one BRC-20 token.
pub async fn cmd_brc20_transferable(token_address: &str) -> Result<()> {
    let token_address = validation::normalize_brc20_token_address(token_address)?;
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let metadata = api.token_metadata(&context, &token_address).await?;
    let decimals = api::extract_token_decimals(&metadata)?;
    let snapshot = api
        .brc20_transferable_utxos(&context, &token_address)
        .await?;
    let transferable = parse_brc20_transferable_utxos(&snapshot)?;
    let choices = transferable
        .iter()
        .map(|utxo| utxo.build_choice(&token_address, decimals))
        .collect::<Result<Vec<_>>>()?;
    let sum_value_raw = snapshot
        .pointer("/brc20TransferableUtxoList/sumValueRaw")
        .and_then(value_as_decimal_string);
    let sum_value = sum_value_raw
        .as_deref()
        .map(|value| minimal_to_readable(value, decimals))
        .transpose()?;
    output::success(json!({
        "message": "Queried transferable BRC-20 inscription UTXOs. Ask the user to select exactly one returned selection before transferring.",
        "queryType": "BRC20_TRANSFERABLE_UTXO_LIST",
        "accountId": context.account_id,
        "address": context.address.address,
        "tokenAddress": token_address,
        "count": choices.len(),
        "sumValue": sum_value,
        "sumValueRaw": sum_value_raw,
        "choices": choices,
        "brc20Transferable": snapshot,
    }));
    Ok(())
}

/// Finds a still-transferable outpoint in the latest BRC-20 snapshot.
pub fn select_brc20_transferable_utxo(
    snapshot: &Value,
    selection: &str,
) -> Result<Brc20TransferableUtxo> {
    let selected = BtcOutPoint::parse(selection)?;
    parse_brc20_transferable_utxos(snapshot)?
        .into_iter()
        .find(|utxo| utxo.outpoint == selected)
        .ok_or_else(|| anyhow::anyhow!("selected BRC-20 UTXO is no longer transferable"))
}

/// Parses transferable BRC-20 response items into validated carrier UTXOs.
fn parse_brc20_transferable_utxos(snapshot: &Value) -> Result<Vec<Brc20TransferableUtxo>> {
    let Some(items) = snapshot
        .pointer("/brc20TransferableUtxoList/utxos")
        .or_else(|| snapshot.get("utxos"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let tx_hash = item
                .get("txHash")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("transferable UTXO {index} is missing txHash"))?;
            let vout_index = item
                .get("voutIndex")
                .and_then(value_as_decimal_string)
                .ok_or_else(|| anyhow::anyhow!("transferable UTXO {index} is missing voutIndex"))?;
            let outpoint = BtcOutPoint::parse(&format!("{tx_hash}:{vout_index}"))?;
            let utxo_amount_raw = read_required_raw_field(item, "utxoAmountRaw", index)?;
            let value_raw = read_required_raw_field(item, "valueRaw", index)?;
            Ok(Brc20TransferableUtxo {
                outpoint,
                utxo_id: item
                    .get("utxoId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                utxo_amount_raw,
                value_raw,
                offset: item.get("offset").and_then(value_as_decimal_string),
                inscription_id: item
                    .get("inscriptionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

/// Reads one required raw integer field from a transferable UTXO item.
fn read_required_raw_field(item: &Value, field: &str, index: usize) -> Result<String> {
    item.get(field)
        .and_then(value_as_decimal_string)
        .ok_or_else(|| anyhow::anyhow!("transferable UTXO {index} is missing {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brc20_transferable_choice_keeps_token_and_btc_amounts_distinct() {
        let tx_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let snapshot = json!({
            "brc20TransferableUtxoList": {
                "sumValueRaw": "1000000000000000000",
                "count": 1,
                "utxos": [{
                    "txHash": tx_hash,
                    "voutIndex": 2,
                    "utxoId": "utxo-1",
                    "utxoAmountRaw": "546",
                    "valueRaw": "1000000000000000000",
                    "offset": "0",
                    "inscriptionId": "inscription-1"
                }]
            }
        });

        let selected = select_brc20_transferable_utxo(&snapshot, &format!("{tx_hash}:2")).unwrap();
        assert_eq!(selected.value_raw, "1000000000000000000");
        assert_eq!(selected.utxo_amount_raw, "546");
        assert_eq!(selected.build_tx_param_input("bc1pfrom")["amount"], "546");

        let choice = selected.build_choice("btc-brc20-pizza", 18).unwrap();
        assert_eq!(choice["tokenAmount"], "1");
        assert_eq!(choice["utxoAmountSats"], "546");
        assert_eq!(choice["selection"], format!("{tx_hash}:2"));
    }
}
