//! Executes native SUI and `Coin<T>` transfers.

use anyhow::Result;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::support::amount::{decimal_field, readable_to_minimal};
use crate::commands::agentic_wallet::support::unsigned_hash_list::build_direct_extra_data;
use crate::commands::sink::CodedError;
use crate::wallet_api::BroadcastResponse;

use super::super::chain_adapters::sui::{
    api::{self, SuiApi},
    context::SuiContext,
    identifiers, signing,
};

/// Executes a native or `Coin<T>` SUI send and emits confirmation or broadcast output.
pub async fn cmd_send(
    readable_amount: &str,
    recipient: &str,
    from: Option<&str>,
    coin_type: Option<&str>,
    force: bool,
) -> Result<()> {
    let recipient = identifiers::normalize_address(recipient).map_err(map_local_input_error)?;
    let coin_type = coin_type
        .map(identifiers::normalize_coin_type)
        .transpose()
        .map_err(map_local_input_error)?;
    let context = SuiContext::load(from).await?;
    let mut api = SuiApi::new()?;

    let (decimals, symbol, effective_coin_type) = match coin_type.as_deref() {
        Some(coin_type) => {
            let metadata = api.token_metadata(&context, coin_type).await?;
            let decimals = decimal_field(&metadata).ok_or_else(|| {
                CodedError::new(
                    "INCOMPLETE_ASSET_METADATA",
                    Some("contract-token"),
                    "SUI token metadata is missing decimal",
                )
            })?;
            let symbol = metadata
                .get("symbol")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or(coin_type)
                .to_string();
            (decimals, symbol, coin_type.to_string())
        }
        None => (
            context.profile.native_decimals,
            context.profile.native_symbol.clone(),
            identifiers::NATIVE_COIN_TYPE.to_string(),
        ),
    };
    let amount = readable_to_minimal(readable_amount, decimals).map_err(map_local_input_error)?;
    let prepared = api
        .prepare_transaction(&context, &recipient, &amount, coin_type.as_deref())
        .await?;
    let seed = context
        .signing_seed()
        .map_err(|error| CodedError::new("LOCAL_SIGNING_FAILED", None, error.to_string()))?;
    let signatures = signing::sign_unsigned_hashes(&prepared, &seed)
        .map_err(|error| CodedError::new("LOCAL_SIGNING_FAILED", None, error.to_string()))?;
    let broadcast =
        broadcast_prepared_transaction(&mut api, &context, &prepared, &signatures, force)
            .await
            .map_err(|error| {
                crate::commands::agentic_wallet::common::handle_confirming_error(error, force)
            })
            .map_err(api::map_api_error)?;
    crate::output::success(json!({
        "message": "SUI transaction submitted. The final result is pending network confirmation.",
        "state": "PENDING",
        "chainIndex": context.profile.chain_index,
        "from": context.address.address,
        "to": recipient,
        "coinType": effective_coin_type,
        "symbol": symbol,
        "amount": amount,
        "txHash": broadcast.tx_hash,
        "orderId": broadcast.order_id,
    }));
    Ok(())
}

/// Builds and broadcasts one prepared SUI transaction from its signed hash list.
async fn broadcast_prepared_transaction(
    api: &mut SuiApi,
    context: &SuiContext,
    prepared: &Value,
    signed_hashes: &[Value],
    force: bool,
) -> Result<BroadcastResponse> {
    let extra_data = build_direct_extra_data(
        prepared,
        signed_hashes,
        &context.session_cert()?,
        force,
        "SUI",
    )?;
    api.broadcast_transaction(context, &extra_data).await
}

/// Converts a local validation failure into the SUI CLI coded-error format.
fn map_local_input_error(error: anyhow::Error) -> anyhow::Error {
    CodedError::new("LOCAL_PRECHECK_FAILED", None, error.to_string()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_hash_objects_are_nested_in_unsigned_hash_list() {
        let encoded = build_direct_extra_data(
            &json!({
                "signType": "transfer",
                "encoding": "eip2519",
                "unsignedTx": "unsigned-sui",
                "txParam": {"sender": "sender", "nonce": 1},
                "uopHash": "uop-1",
                "extraData": {"serviceField": "preserved"}
            }),
            &[
                json!({"index": 0, "unsignedHash": "hash-1", "unsignedHashSig": "proof-1", "sessionSignature": "sig-1"}),
                json!({"index": 1, "unsignedHash": "hash-2", "unsignedHashSig": "proof-2", "sessionSignature": "sig-2"}),
            ],
            "session-cert",
            true,
            "SUI",
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
        assert!(unsigned_hash_list[0].get("unsignedTxHash").is_none());
        assert!(msg_for_sign.get("unsignedTxHash").is_none());
        assert!(msg_for_sign.get("sessionSignature").is_none());
        assert_eq!(msg_for_sign["unsignedTx"], "unsigned-sui");
        assert_eq!(msg_for_sign["sessionCert"], "session-cert");
        assert_eq!(msg_for_sign["txParam"]["sender"], "sender");
        assert_eq!(msg_for_sign["txParam"]["nonce"], 1);
        assert_eq!(extra["serviceField"], "preserved");
        assert_eq!(extra["encoding"], "eip2519");
        assert_eq!(extra["signType"], "transfer");
        assert_eq!(extra["skipWarning"], true);
    }
}
