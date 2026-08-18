//! Builds and submits Bitcoin direct-transfer and inscription broadcasts.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::support::json::required_string;
use crate::commands::agentic_wallet::support::unsigned_hash_list::build_direct_extra_data;
use crate::wallet_api::BroadcastResponse;

use super::api::BtcApi;
use super::context::BtcContext;

/// Broadcasts one prepared BTC or BRC-20 transfer using its signed hash list.
pub async fn submit_direct_transaction(
    api: &mut BtcApi,
    context: &BtcContext,
    prepared: &Value,
    signed_hashes: &[Value],
    force: bool,
) -> Result<BroadcastResponse> {
    let extra_data = build_direct_extra_data(
        prepared,
        signed_hashes,
        &context.session_cert()?,
        force,
        "Bitcoin",
    )?;
    api.broadcast_transaction(context, &extra_data).await
}

/// Runs `sign-tx` for a BRC-20 inscription and batch-broadcasts its ordered transactions.
pub async fn submit_inscription_transactions(
    api: &mut BtcApi,
    context: &BtcContext,
    prepared: &Value,
    signed_hashes: &[Value],
    token_address: &str,
    amount: &str,
    force: bool,
) -> Result<Vec<BroadcastResponse>> {
    let signed = api
        .sign_transaction(context, prepared, signed_hashes)
        .await?;
    let body =
        build_inscription_batch_body(context, prepared, &signed, token_address, amount, force)?;
    api.batch_broadcast_transactions(context, &body).await
}

/// Builds the inscription batch body from ordered `signTxList` items and service charges.
fn build_inscription_batch_body(
    context: &BtcContext,
    prepared: &Value,
    signed: &Value,
    token_address: &str,
    amount: &str,
    force: bool,
) -> Result<Value> {
    let signed_items: Vec<&Value> = signed
        .get("signedTxList")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .map(|items| items.iter().collect())
        .unwrap_or_else(|| vec![signed]);
    let sign_type = required_string(prepared, "signType", "unsignedInfo response")?;
    let encoding = required_string(prepared, "encoding", "unsignedInfo response")?;
    let tx_param = extract_tx_param_object(prepared);
    let commit_hash = signed_items
        .first()
        .and_then(|item| item.get("txHash"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let commit_address = tx_param
        .get("commitAddress")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());

    let elements = signed_items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let signed_tx = required_string(item, "signedTx", "sign-tx response item")?;
            let tx_hash = item
                .get("txHash")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut extra_data = prepared
                .get("extraData")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({}));
            extra_data["txHash"] = json!(tx_hash);
            extra_data["tokenAddress"] = json!(token_address);
            extra_data["txType"] = json!(51);
            extra_data["coinAmount"] = json!(amount);
            extra_data["toAdr"] = json!(context.address.address);
            extra_data["checkBalance"] = json!(true);
            extra_data["encoding"] = json!(encoding);
            extra_data["signType"] = json!(sign_type);
            if let Some(service_charge) = extract_inscription_service_charge(&tx_param, index) {
                extra_data["serviceCharge"] = json!(service_charge);
            }
            if index > 0 && !commit_hash.is_empty() {
                extra_data["dependTx"] = json!([commit_hash]);
            }
            let mut ext_json = extra_data
                .get("extJson")
                .and_then(parse_json_object)
                .unwrap_or_else(|| json!({}));
            ext_json["batchBroadcastType"] = json!(0);
            extra_data["extJson"] = ext_json;
            if force {
                extra_data["skipWarning"] = json!(true);
            }
            let address = if index == 0 {
                context.address.address.as_str()
            } else {
                commit_address.unwrap_or(context.address.address.as_str())
            };
            Ok(json!({
                "accountId": context.account_id,
                "address": address,
                "chainIndex": context.profile.chain_index,
                "signedTx": signed_tx,
                "extraData": serde_json::to_string(&extra_data)
                    .context("failed to serialize BRC-20 inscription batch extraData")?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Value::Array(elements))
}

/// Returns the prepared transaction parameters as an object or an empty object.
fn extract_tx_param_object(prepared: &Value) -> Value {
    match prepared.get("txParam") {
        Some(Value::Object(_)) => prepared["txParam"].clone(),
        Some(Value::String(value)) => serde_json::from_str(value).unwrap_or_else(|_| json!({})),
        _ => json!({}),
    }
}

/// Returns a cloned JSON object, accepting either an object or serialized object string.
fn parse_json_object(value: &Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value.clone()),
        Value::String(value) => serde_json::from_str(value).ok().filter(Value::is_object),
        _ => None,
    }
}

/// Resolves the service charge for one inscription transaction by item or index.
fn extract_inscription_service_charge(tx_param: &Value, index: usize) -> Option<String> {
    let value = if index == 0 {
        tx_param.get("commitFee")
    } else {
        tx_param
            .get("revealFees")
            .and_then(Value::as_array)
            .and_then(|fees| fees.get(index - 1))
            .or_else(|| tx_param.get("revealFee"))
    }?;
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_transfer_nests_signed_hash_list_without_sign_tx() {
        let encoded = build_direct_extra_data(
            &json!({
                "signType": "transfer",
                "encoding": "hex",
                "unsignedTx": "unsigned-btc",
                "unsignedTxHash": "unsigned-tx-hash",
                "txParam": {"inputs": ["input-1"], "outputs": ["output-1"]},
                "uopHash": "uop-1",
                "extraData": {"serviceField": "preserved"}
            }),
            &[
                json!({"index": 0, "unsignedHash": "hash-1", "unsignedHashSig": "proof-1", "sessionSignature": "sig-1"}),
                json!({"index": 1, "unsignedHash": "hash-2", "unsignedHashSig": "proof-2", "sessionSignature": "sig-2"}),
            ],
            "session-cert",
            true,
            "Bitcoin",
        )
        .unwrap();
        let extra: Value = serde_json::from_str(&encoded).unwrap();

        assert!(extra.get("signedTx").is_none());
        assert!(extra.get("signTx").is_none());
        let msg_for_sign = extra["msgForSign"].as_object().unwrap();
        let unsigned_hash_list = msg_for_sign["unsignedHashList"].as_array().unwrap();
        assert_eq!(unsigned_hash_list.len(), 2);
        assert_eq!(unsigned_hash_list[0]["index"], 0);
        assert_eq!(unsigned_hash_list[0]["unsignedHash"], "hash-1");
        assert_eq!(unsigned_hash_list[0]["unsignedHashSig"], "proof-1");
        assert_eq!(unsigned_hash_list[0]["sessionSignature"], "sig-1");
        assert_eq!(unsigned_hash_list[1]["index"], 1);
        assert_eq!(unsigned_hash_list[1]["sessionSignature"], "sig-2");
        assert_eq!(msg_for_sign["unsignedTx"], "unsigned-btc");
        assert_eq!(msg_for_sign["unsignedTxHash"], "unsigned-tx-hash");
        assert_eq!(msg_for_sign["sessionCert"], "session-cert");
        assert_eq!(msg_for_sign["txParam"]["inputs"][0], "input-1");
        assert_eq!(extra["serviceField"], "preserved");
        assert_eq!(extra["checkBalance"], true);
        assert_eq!(extra["uopHash"], "uop-1");
        assert_eq!(extra["encoding"], "hex");
        assert_eq!(extra["signType"], "transfer");
        assert!(extra.get("txType").is_none());
        assert_eq!(extra["skipWarning"], true);
    }

    #[test]
    fn inscription_maps_sign_tx_list_to_ordered_batch_broadcast() {
        let context = BtcContext {
            access_token: "unused".to_string(),
            account_id: "account-1".to_string(),
            login_type: "email".to_string(),
            address: crate::wallet_store::AddressInfo {
                account_id: "account-1".to_string(),
                address: "bc1-user".to_string(),
                chain_index: "0".to_string(),
                chain_name: "Bitcoin".to_string(),
                address_type: "taproot".to_string(),
                chain_path: String::new(),
            },
            profile: crate::commands::agentic_wallet::chain_profile::from_entry(&json!({
                "chainIndex": "0",
                "realChainIndex": "5",
                "chainName": "Bitcoin"
            }))
            .unwrap(),
        };
        let body = build_inscription_batch_body(
            &context,
            &json!({
                "signType": "brc20Inscribe",
                "encoding": "hex",
                "txParam": {
                    "commitAddress": "bc1-commit",
                    "commitFee": "154",
                    "revealFee": "156"
                }
            }),
            &json!({
                "signedTxList": [
                    {"signedTx": "commit-tx", "txHash": "commit-hash"},
                    {"signedTx": "reveal-tx", "txHash": "reveal-hash"}
                ]
            }),
            "btc-brc20-pizza",
            "1",
            true,
        )
        .unwrap();
        let elements = body.as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["signedTx"], "commit-tx");
        assert_eq!(elements[0]["address"], "bc1-user");
        assert_eq!(elements[1]["signedTx"], "reveal-tx");
        assert_eq!(elements[1]["address"], "bc1-commit");
        let commit_extra: Value =
            serde_json::from_str(elements[0]["extraData"].as_str().unwrap()).unwrap();
        let reveal_extra: Value =
            serde_json::from_str(elements[1]["extraData"].as_str().unwrap()).unwrap();
        assert!(commit_extra.get("broadcastInSeq").is_none());
        assert_eq!(commit_extra["txHash"], "commit-hash");
        assert_eq!(commit_extra["serviceCharge"], "154");
        assert_eq!(commit_extra["txType"], 51);
        assert_eq!(commit_extra["extJson"]["batchBroadcastType"], 0);
        assert_eq!(reveal_extra["txHash"], "reveal-hash");
        assert_eq!(reveal_extra["serviceCharge"], "156");
        assert_eq!(reveal_extra["dependTx"], json!(["commit-hash"]));
    }
}
