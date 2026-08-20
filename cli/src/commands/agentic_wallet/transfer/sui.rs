//! Executes native SUI and `Coin<T>` transfers.

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::common::WalletPreviewConfirming;
use crate::commands::agentic_wallet::support::amount::{
    decimal_field, minimal_to_readable, readable_to_minimal, value_as_decimal_string,
};
use crate::commands::agentic_wallet::support::json::shell_arg;
use crate::commands::agentic_wallet::support::unsigned_hash_list::build_direct_extra_data;
use crate::commands::sink::CodedError;
use crate::validators::validate_non_negative_integer;
use crate::wallet_api::BroadcastResponse;

use super::super::chain_adapters::sui::{
    api::{self, SuiApi},
    context::SuiContext,
    identifiers, signing,
};

/// Prepares and signs one native or `Coin<T>` SUI transfer before broadcast confirmation.
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
    ensure_simulation_succeeded(&prepared)?;
    let seed = context
        .signing_seed()
        .map_err(|error| CodedError::new("LOCAL_SIGNING_FAILED", None, error.to_string()))?;
    let signatures = signing::sign_unsigned_hashes(&prepared, &seed)
        .map_err(|error| CodedError::new("LOCAL_SIGNING_FAILED", None, error.to_string()))?;
    if !force {
        return Err(WalletPreviewConfirming {
            message: "The transfer has been signed and is ready to broadcast. Review the transfer and current network fee before confirming.".to_string(),
            next: build_send_next_command(
                &recipient,
                &context.address.address,
                coin_type.as_deref(),
                readable_amount,
            ),
            scene: "sui_transfer".to_string(),
            preview: preview_from_prepared(
                &prepared,
                &context,
                &recipient,
                &effective_coin_type,
                &symbol,
                &amount,
                readable_amount,
            )?,
        }
        .into());
    }
    let broadcast = broadcast_prepared_transaction(
        &mut api,
        &context,
        &prepared,
        &signatures,
        force,
        None,
        None,
    )
    .await
    .map_err(|error| crate::commands::agentic_wallet::common::handle_confirming_error(error, force))
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

/// Builds the non-sensitive confirmation preview for one signed SUI transfer.
fn preview_from_prepared(
    prepared: &Value,
    context: &SuiContext,
    recipient: &str,
    coin_type: &str,
    symbol: &str,
    amount: &str,
    readable_amount: &str,
) -> Result<Value> {
    let unsigned_count = prepared
        .get("unsignedHashList")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .map(Vec::len)
        .ok_or_else(|| {
            anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: unsignedHashList is empty")
        })?;
    let sign_type = prepared
        .get("signType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing signType"))?;
    let encoding = prepared
        .get("encoding")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing encoding"))?;
    let transaction = prepared
        .get("txParam")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let fee = ["fee", "gasFee"]
        .iter()
        .find_map(|key| transaction.get(*key).filter(|value| !value.is_null()))
        .or_else(|| {
            ["fee", "gasFee"]
                .iter()
                .find_map(|key| prepared.get(*key).filter(|value| !value.is_null()))
        })
        .cloned()
        .unwrap_or(Value::Null);
    let fee_readable = value_as_decimal_string(&fee)
        .map(|value| minimal_to_readable(&value, context.profile.native_decimals))
        .transpose()?;

    Ok(json!({
        "operationType": "SUI_TRANSFER",
        "chainIndex": context.profile.chain_index,
        "network": "sui",
        "from": context.address.address,
        "to": recipient,
        "asset": {
            "coinType": coin_type,
            "symbol": symbol,
            "amount": amount,
            "readableAmount": readable_amount,
        },
        "feeRate": transaction
            .get("gasPrice")
            .or_else(|| prepared.get("gasPrice"))
            .cloned()
            .unwrap_or(Value::Null),
        "fee": fee,
        "feeReadable": fee_readable,
        "feeSymbol": context.profile.native_symbol,
        "preExecution": {
            "executeResult": prepared.get("executeResult").cloned().unwrap_or(Value::Null),
            "executeErrorMsg": prepared.get("executeErrorMsg").cloned().unwrap_or(Value::Null),
        },
        "signing": {
            "signType": sign_type,
            "encoding": encoding,
            "unsignedItemCount": unsigned_count,
        },
        "warnings": prepared.get("warnings").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
    }))
}

/// Rebuilds the exact confirmed SUI send command without signing data.
fn build_send_next_command(
    recipient: &str,
    from: &str,
    coin_type: Option<&str>,
    readable_amount: &str,
) -> String {
    let mut command = format!(
        "onchainos wallet send --chain sui --recipient {} --readable-amount {}",
        shell_arg(recipient),
        shell_arg(readable_amount),
    );
    command.push_str(&format!(" --from {}", shell_arg(from)));
    if let Some(coin_type) = coin_type {
        command.push_str(&format!(" --contract-token {}", shell_arg(coin_type)));
    }
    command.push_str(" --force");
    command
}

/// Executes a DApp-built SUI TransactionData / PTB via Agentic Wallet signing.
pub async fn cmd_contract_call(
    tx_bytes: &str,
    to: Option<&str>,
    amount: &str,
    from: Option<&str>,
    force: bool,
    agent_biz_type: Option<&str>,
    agent_skill_name: Option<&str>,
) -> Result<()> {
    validate_tx_bytes(tx_bytes).map_err(map_local_input_error)?;
    validate_non_negative_integer(amount, "amt").map_err(map_local_input_error)?;
    let to = to
        .map(identifiers::normalize_address)
        .transpose()
        .map_err(map_local_input_error)?;
    let context = SuiContext::load(from).await?;
    let mut api = SuiApi::new()?;
    let prepared = api
        .prepare_contract_call(&context, to.as_deref(), amount, tx_bytes.trim())
        .await?;
    ensure_simulation_succeeded(&prepared)?;
    let seed = context
        .signing_seed()
        .map_err(|error| CodedError::new("LOCAL_SIGNING_FAILED", None, error.to_string()))?;
    let signatures = signing::sign_unsigned_hashes(&prepared, &seed)
        .map_err(|error| CodedError::new("LOCAL_SIGNING_FAILED", None, error.to_string()))?;
    let broadcast = broadcast_prepared_transaction(
        &mut api,
        &context,
        &prepared,
        &signatures,
        force,
        agent_biz_type,
        agent_skill_name,
    )
    .await
    .map_err(|error| crate::commands::agentic_wallet::common::handle_confirming_error(error, force))
    .map_err(api::map_api_error)?;
    crate::output::success(json!({
        "message": "SUI contract transaction submitted. The final result is pending network confirmation.",
        "state": "PENDING",
        "chainIndex": context.profile.chain_index,
        "txHash": broadcast.tx_hash,
        "orderId": broadcast.order_id,
    }));
    Ok(())
}

/// Validates only the transport encoding. The service remains authoritative for
/// parsing the BCS TransactionData and simulating its effects.
fn validate_tx_bytes(tx_bytes: &str) -> Result<()> {
    let tx_bytes = tx_bytes.trim();
    if tx_bytes.is_empty() {
        bail!("--sui-tx-bytes must not be empty");
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(tx_bytes)
        .context("--sui-tx-bytes must be valid base64")?;
    if decoded.is_empty() {
        bail!("--sui-tx-bytes must decode to non-empty TransactionData");
    }
    Ok(())
}

fn ensure_simulation_succeeded(prepared: &Value) -> Result<()> {
    if prepared.get("executeResult").and_then(Value::as_bool) == Some(false) {
        let message = prepared
            .get("executeErrorMsg")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("transaction simulation failed");
        bail!("transaction simulation failed: {message}");
    }
    Ok(())
}

/// Builds and broadcasts one prepared SUI transaction from its signed hash list.
async fn broadcast_prepared_transaction(
    api: &mut SuiApi,
    context: &SuiContext,
    prepared: &Value,
    signed_hashes: &[Value],
    force: bool,
    agent_biz_type: Option<&str>,
    agent_skill_name: Option<&str>,
) -> Result<BroadcastResponse> {
    let encoded = build_direct_extra_data(
        prepared,
        signed_hashes,
        &context.session_cert()?,
        force,
        "SUI",
    )?;
    let mut extra_data: Value =
        serde_json::from_str(&encoded).context("failed to parse serialized SUI extraData")?;
    if let Some(value) = agent_biz_type {
        extra_data["agentBizType"] = json!(value);
    }
    if let Some(value) = agent_skill_name {
        extra_data["agentSkillName"] = json!(value);
    }
    let extra_data =
        serde_json::to_string(&extra_data).context("failed to serialize SUI extraData")?;
    api.broadcast_transaction(context, &extra_data).await
}

/// Converts a local validation failure into the SUI CLI coded-error format.
fn map_local_input_error(error: anyhow::Error) -> anyhow::Error {
    CodedError::new("LOCAL_PRECHECK_FAILED", None, error.to_string()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::agentic_wallet::chain_profile::{
        AssetModel, ChainCapabilities, ChainKind, InscriptionDriver, MessageSignDriver,
        ResolvedChainProfile, TransferDriver,
    };

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

    #[test]
    fn validates_base64_ptb_transport() {
        assert!(validate_tx_bytes("AAECAwQ=").is_ok());
        assert!(validate_tx_bytes("").is_err());
        assert!(validate_tx_bytes("not-base64").is_err());
    }

    #[test]
    fn rejects_failed_simulation() {
        let error = ensure_simulation_succeeded(&json!({
            "executeResult": false,
            "executeErrorMsg": "MoveAbort"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("MoveAbort"));
    }

    #[test]
    fn sui_preview_omits_raw_transaction_bytes_and_formats_actual_fee() {
        let context = SuiContext {
            access_token: String::new(),
            account_id: "account".to_string(),
            profile: ResolvedChainProfile {
                kind: ChainKind::Sui,
                chain_index: "784".to_string(),
                real_chain_index: "784".to_string(),
                chain_name: "Sui".to_string(),
                native_symbol: "SUI".to_string(),
                native_decimals: 9,
                capabilities: ChainCapabilities {
                    transfer: TransferDriver::Sui,
                    inscription: InscriptionDriver::Unsupported,
                    contract_call: true,
                    message_sign: MessageSignDriver::Unsupported,
                    asset_model: AssetModel::Account,
                },
            },
            address: crate::wallet_store::AddressInfo {
                account_id: "account".to_string(),
                address: "0xsender".to_string(),
                chain_index: "784".to_string(),
                chain_name: "Sui".to_string(),
                address_type: String::new(),
                chain_path: String::new(),
            },
        };
        let preview = preview_from_prepared(
            &json!({
                "executeResult": true,
                "signType": "transfer",
                "encoding": "eip2519",
                "unsignedHashList": [{"index": 0}],
                "txParam": {"gasFee": "1200000", "gasPrice": "1000", "txBytes": "raw"}
            }),
            &context,
            "0xrecipient",
            "0x2::sui::SUI",
            "SUI",
            "1000000000",
            "1",
        )
        .unwrap();

        assert_eq!(preview["feeReadable"], "0.0012");
        assert_eq!(preview["feeRate"], "1000");
        assert!(!preview.to_string().contains("raw"));
    }

    #[test]
    fn confirmed_sui_next_command_keeps_coin_type() {
        assert_eq!(
            build_send_next_command(
                "0xrecipient",
                "0xsender",
                Some("0x2::coin::COIN"),
                "2.5",
            ),
            "onchainos wallet send --chain sui --recipient 0xrecipient --readable-amount 2.5 --from 0xsender --contract-token 0x2::coin::COIN --force"
        );
    }
}
