//! Validates Bitcoin addresses, previews, intent, and UTXO availability.

use anyhow::{bail, Result};
use bitcoin::address::{NetworkChecked, NetworkUnchecked};
use bitcoin::{Address, AddressType, Network};
use num_bigint::BigUint;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::str::FromStr;

use crate::commands::agentic_wallet::shared::common::amount::{
    minimal_to_readable, parse_minimal, value_as_decimal_string,
};

/// Parses and network-checks a mainnet Bitcoin address, naming `field` in errors.
fn parse_mainnet_address(value: &str, field: &str) -> Result<Address<NetworkChecked>> {
    let unchecked = Address::<NetworkUnchecked>::from_str(value.trim())
        .map_err(|error| anyhow::anyhow!("invalid {field} Bitcoin address: {error}"))?;
    unchecked
        .require_network(Network::Bitcoin)
        .map_err(|error| anyhow::anyhow!("{field} must be a Bitcoin mainnet address: {error}"))
}

/// Validates the wallet's source address for mainnet Bitcoin operations.
pub fn validate_wallet_address(value: &str) -> Result<()> {
    let address = parse_mainnet_address(value, "wallet")?;
    if address.address_type() != Some(AddressType::P2tr) {
        bail!("current Agentic Wallet Bitcoin address must be Taproot (P2TR)");
    }
    Ok(())
}

/// Validates a user-supplied mainnet Bitcoin recipient address.
pub fn validate_recipient(value: &str) -> Result<()> {
    parse_mainnet_address(value, "recipient")?;
    Ok(())
}

/// Parses a user-selected Bitcoin fee rate and enforces the 0.1 sat/vB minimum.
///
/// The returned JSON number is placed directly in `txParam.feeRate` for the
/// current BTC or BRC-20 transaction only.
pub fn parse_fee_rate(value: &str) -> Result<Value> {
    let value = value.trim();
    let (integer, fraction) = match value.split_once('.') {
        Some((integer, fraction)) => {
            if value.matches('.').count() != 1 || fraction.is_empty() {
                bail!("--fee-rate must be a decimal sat/vB value");
            }
            (integer, Some(fraction))
        }
        None => (value, None),
    };
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|digits| !digits.bytes().all(|byte| byte.is_ascii_digit()))
        || (integer.len() > 1 && integer.starts_with('0'))
    {
        bail!("--fee-rate must be a decimal sat/vB value");
    }

    let scale = fraction.map_or(0, str::len);
    let unscaled = format!("{integer}{}", fraction.unwrap_or_default())
        .parse::<BigUint>()
        .map_err(|_| anyhow::anyhow!("--fee-rate is outside the supported range"))?;
    let minimum = BigUint::from(10u8).pow(scale as u32);
    if unscaled * BigUint::from(10u8) < minimum {
        bail!("--fee-rate must be at least 0.1 sat/vB");
    }

    let parsed: Value = serde_json::from_str(value)
        .map_err(|_| anyhow::anyhow!("--fee-rate must be a decimal sat/vB value"))?;
    if !parsed.is_number() {
        bail!("--fee-rate must be a decimal sat/vB value");
    }
    Ok(parsed)
}

/// Normalizes a BRC-20 token address into the backend contract-address form.
pub fn normalize_brc20_token_address(value: &str) -> Result<String> {
    const PREFIX: &str = "btc-brc20-";
    let value = value.trim();
    if value.len() <= PREFIX.len()
        || !value
            .get(..PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
        || value.len() > PREFIX.len() + 64
    {
        bail!("BRC-20 token address must use btc-brc20-<ticker>");
    }
    let ticker = &value[PREFIX.len()..];
    if ticker
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == b'/')
    {
        bail!("BRC-20 ticker contains unsupported characters");
    }
    Ok(format!("{PREFIX}{}", ticker.to_ascii_lowercase()))
}

/// Compares two Bitcoin addresses after network-aware parsing.
pub fn same_address(left: &str, right: &str) -> Result<bool> {
    Ok(parse_mainnet_address(left, "from")?.script_pubkey()
        == parse_mainnet_address(right, "wallet")?.script_pubkey())
}

#[allow(clippy::too_many_arguments)]
/// Builds confirmation data from a prepared Bitcoin transfer or inscription response.
pub fn preview_from_response(
    response: &Value,
    operation: &str,
    chain_index: &str,
    from: &str,
    to: &str,
    token_address: Option<&str>,
    amount: &str,
    readable_amount: &str,
    native_decimals: u32,
) -> Result<Value> {
    let execute_result = response
        .get("executeResult")
        .and_then(Value::as_bool)
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing executeResult"))?;
    if !execute_result {
        let message = response
            .get("executeErrorMsg")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("Bitcoin transaction pre-execution failed");
        bail!("PRE_EXECUTION_FAILED: {message}");
    }
    let transaction = response
        .get("txParam")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: response is missing txParam")
        })?;
    let inputs = transaction
        .get("inputs")
        .filter(|value| value.as_array().is_some_and(|items| !items.is_empty()))
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: txParam.inputs is empty")
        })?;
    let outputs = transaction
        .get("outputs")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    if operation != "BRC20_INSCRIBE" && outputs.as_array().is_none_or(|items| items.is_empty()) {
        bail!("INCOMPLETE_TRANSACTION_PREVIEW: txParam.outputs is empty");
    }
    let unsigned_count = response
        .get("unsignedHashList")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .map(Vec::len)
        .ok_or_else(|| {
            anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: unsignedHashList is empty")
        })?;
    let sign_type = response
        .get("signType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing signType"))?;
    let expected_sign_type = if operation == "BRC20_INSCRIBE" {
        "brc20Inscribe"
    } else {
        "transfer"
    };
    if sign_type != expected_sign_type {
        bail!("PREVIEW_INTENT_MISMATCH: expected signType {expected_sign_type}, got {sign_type}");
    }
    let encoding = response
        .get("encoding")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing encoding"))?;

    let fee = transaction.get("fee").cloned().unwrap_or(Value::Null);
    let fee_readable = value_as_decimal_string(&fee)
        .map(|value| minimal_to_readable(&value, native_decimals))
        .transpose()?;
    let symbol = token_address
        .and_then(|token_address| token_address.strip_prefix("btc-brc20-"))
        .unwrap_or("BTC");

    Ok(json!({
        "operationType": operation,
        "chainIndex": chain_index,
        "network": "bitcoin",
        "from": from,
        "to": to,
        "asset": {
            "tokenAddress": token_address,
            "symbol": symbol,
            "amount": amount,
            "readableAmount": readable_amount,
        },
        "feeRate": transaction.get("feeRate").cloned().unwrap_or(Value::Null),
        "fee": fee,
        "feeReadable": fee_readable,
        "feeSymbol": "BTC",
        "inputs": inputs,
        "outputs": outputs,
        "changeAddress": transaction.get("changeAddress").cloned().unwrap_or(Value::Null),
        "transaction": transaction,
        "preExecution": {
            "executeResult": true,
            "executeErrorMsg": response.get("executeErrorMsg").cloned().unwrap_or(Value::Null),
        },
        "signing": {
            "signType": sign_type,
            "encoding": encoding,
            "unsignedItemCount": unsigned_count,
        },
        "warnings": response.get("warnings").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
    }))
}

/// Attaches unavailable-UTXO facts and rejects any selected unavailable input.
pub fn bind_utxo_availability(preview: &mut Value, snapshot: Value) -> Result<()> {
    let selected = super::models::collect_outpoints(
        preview
            .get("inputs")
            .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing inputs"))?,
    );
    let unavailable = super::models::collect_outpoints(&snapshot);
    let reported_count = snapshot
        .pointer("/unavailableBreakdown/totalUnavailableCount")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
        });
    if reported_count.is_some_and(|count| count > unavailable.len() as u64) {
        bail!("INCOMPLETE_UTXO_SNAPSHOT: unavailable UTXO count exceeds the returned outpoint set");
    }

    let unavailable_set: BTreeSet<String> = unavailable
        .iter()
        .map(super::models::BtcOutPoint::canonical)
        .collect();
    let selected_outpoints: Vec<String> = selected
        .iter()
        .map(super::models::BtcOutPoint::canonical)
        .collect();
    let rejected: Vec<String> = selected_outpoints
        .iter()
        .filter(|outpoint| unavailable_set.contains(*outpoint))
        .cloned()
        .collect();
    if !rejected.is_empty() {
        bail!(
            "PREVIEW_UTXO_UNAVAILABLE: selected inputs are unavailable: {}",
            rejected.join(", ")
        );
    }

    preview["utxoAvailability"] = json!({
        "queryType": "UNAVAILABLE_BREAKDOWN",
        "selectedAvailableInputs": selected_outpoints,
        "unavailable": snapshot,
    });
    Ok(())
}

/// Verifies that prepared transaction data still matches the requested operation.
pub fn validate_preview_intent(
    preview: &Value,
    operation: &str,
    chain_index: &str,
    from: &str,
    to: Option<&str>,
    amount: Option<&str>,
) -> Result<()> {
    compare_if_present(
        preview,
        &["operationType", "operation"],
        operation,
        "operation",
    )?;
    compare_if_present(preview, &["chainIndex"], chain_index, "chainIndex")?;
    compare_if_present(preview, &["from", "fromAddr"], from, "from")?;
    if let Some(to) = to {
        compare_if_present(preview, &["to", "toAddr"], to, "to")?;
    }
    if let Some(amount) = amount {
        compare_if_present(&preview["asset"], &["amount"], amount, "amount")?;
    }
    validate_transaction_shape(preview, operation, from, to, amount)?;
    Ok(())
}

/// Validates the prepared inputs, outputs, change address, and amount.
fn validate_transaction_shape(
    preview: &Value,
    operation: &str,
    from: &str,
    to: Option<&str>,
    amount: Option<&str>,
) -> Result<()> {
    let inputs = preview
        .get("inputs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing inputs"))?;
    let mut outpoints = BTreeSet::new();
    for input in inputs {
        let tx_id = input
            .get("txId")
            .or_else(|| input.get("txHash"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: input txId missing"))?;
        let vout = input
            .get("vout")
            .or_else(|| input.get("voutIndex"))
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
            })
            .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: input vout missing"))?;
        let outpoint = super::models::BtcOutPoint::parse(&format!("{tx_id}:{vout}"))?;
        if !outpoints.insert(outpoint.canonical()) {
            bail!(
                "INCOMPLETE_TRANSACTION_PREVIEW: duplicate input {}",
                outpoint.canonical()
            );
        }
        let input_amount = input
            .get("amount")
            .and_then(value_as_decimal_string)
            .ok_or_else(|| {
                anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: input amount missing")
            })?;
        parse_minimal(&input_amount, "input amount", false)?;
        if let Some(address) = input.get("address").and_then(Value::as_str) {
            if !same_address(address, from)? {
                bail!("PREVIEW_INTENT_MISMATCH: input address is not the current account");
            }
        }
    }

    if let Some(change_address) = preview.get("changeAddress").and_then(Value::as_str) {
        if !change_address.is_empty() && !same_address(change_address, from)? {
            bail!("PREVIEW_INTENT_MISMATCH: change address is not the current account");
        }
    }

    let outputs = preview
        .get("outputs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: missing outputs"))?;
    for output in outputs {
        let output_amount = output
            .get("amount")
            .and_then(value_as_decimal_string)
            .ok_or_else(|| {
                anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: output amount missing")
            })?;
        parse_minimal(&output_amount, "output amount", true)?;
        let address = output
            .get("address")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!("INCOMPLETE_TRANSACTION_PREVIEW: output address missing")
            })?;
        validate_recipient(address)?;
    }

    if operation != "BRC20_INSCRIBE" {
        let to = to.ok_or_else(|| anyhow::anyhow!("preview recipient is required"))?;
        let recipient_output = outputs.iter().find(|output| {
            output
                .get("address")
                .and_then(Value::as_str)
                .is_some_and(|address| same_address(address, to).unwrap_or(false))
        });
        let recipient_output = recipient_output.ok_or_else(|| {
            anyhow::anyhow!("PREVIEW_INTENT_MISMATCH: recipient output is missing")
        })?;
        if operation == "BTC_TRANSFER" {
            let expected = amount.ok_or_else(|| anyhow::anyhow!("preview amount is required"))?;
            compare_if_present(recipient_output, &["amount"], expected, "recipient amount")?;
        }
    }
    Ok(())
}

/// Hashes prepared data and confirmation facts into a local continuation token.
pub fn local_transaction_token(response: &Value, preview: &Value) -> Result<String> {
    let binding = json!({
        "preview": preview,
        "unsignedHashList": response.get("unsignedHashList").cloned().unwrap_or(Value::Null),
        "signType": response.get("signType").cloned().unwrap_or(Value::Null),
        "encoding": response.get("encoding").cloned().unwrap_or(Value::Null),
        "extraData": response.get("extraData").cloned().unwrap_or(Value::Null),
    });
    Ok(format!(
        "sha256:{}",
        hex::encode(Sha256::digest(serde_jcs::to_string(&binding)?.as_bytes()))
    ))
}

/// Checks whether a continuation token contains one complete SHA-256 digest.
pub fn is_local_continuation(operation_token: &str) -> bool {
    operation_token
        .strip_prefix("sha256:")
        .is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// Compares an optional response field with its expected command input.
fn compare_if_present(preview: &Value, keys: &[&str], expected: &str, field: &str) -> Result<()> {
    if let Some(actual) = keys.iter().find_map(|key| preview.get(*key)) {
        let actual = actual
            .as_str()
            .map(str::to_string)
            .or_else(|| actual.as_u64().map(|number| number.to_string()))
            .unwrap_or_default();
        if actual != expected {
            bail!("PREVIEW_INTENT_MISMATCH: {field} changed from '{expected}' to '{actual}'");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::{Keypair, Secp256k1, SecretKey, XOnlyPublicKey};
    use serde_json::json;

    fn taproot_address() -> String {
        let secp = Secp256k1::new();
        let secret = SecretKey::from_slice(&[1u8; 32]).unwrap();
        let keypair = Keypair::from_secret_key(&secp, &secret);
        let (x_only, _) = XOnlyPublicKey::from_keypair(&keypair);
        Address::p2tr(&secp, x_only, None, Network::Bitcoin).to_string()
    }

    #[test]
    fn validates_bitcoin_address_network() {
        assert!(validate_recipient("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh").is_ok());
        assert!(validate_recipient("tb1qfm7wy7vazh5u5u8nmw4t27n3q8q4xu3z5m8v4j").is_err());
    }

    #[test]
    fn parses_fee_rate_as_json_number_with_exact_minimum() {
        assert_eq!(parse_fee_rate("0.1").unwrap(), json!(0.1));
        assert_eq!(parse_fee_rate("8").unwrap(), json!(8));
        assert_eq!(parse_fee_rate("1.25").unwrap(), json!(1.25));
        assert!(parse_fee_rate("0.01").is_err());
        assert!(parse_fee_rate("0").is_err());
        assert!(parse_fee_rate("1e2").is_err());
        assert!(parse_fee_rate("1.").is_err());
    }

    #[test]
    fn validates_single_preview_continuation_token() {
        assert!(is_local_continuation(&format!("sha256:{}", "a".repeat(64))));
        assert!(!is_local_continuation("sha256:short"));
        assert!(!is_local_continuation(&"a".repeat(64)));
    }

    #[test]
    fn builds_and_validates_native_txparam_preview() {
        let from = taproot_address();
        let to = "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh";
        let response = json!({
            "executeResult": true,
            "executeErrorMsg": "",
            "txParam": {
                "inputs": [{
                    "txId": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0",
                    "vout": 1,
                    "amount": "10000",
                    "address": from,
                }],
                "outputs": [{"address": to, "amount": "5000"}],
                "changeAddress": from,
                "feeRate": "10",
                "fee": "1000",
            },
            "unsignedHashList": [{"index": 0, "unsignedHash": "00"}],
            "signType": "transfer",
            "encoding": "hex",
            "extraData": {},
        });
        let preview = preview_from_response(
            &response,
            "BTC_TRANSFER",
            "0",
            &from,
            to,
            None,
            "5000",
            "0.00005",
            8,
        )
        .unwrap();
        validate_preview_intent(&preview, "BTC_TRANSFER", "0", &from, Some(to), Some("5000"))
            .unwrap();
        assert!(local_transaction_token(&response, &preview)
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(preview["feeReadable"], "0.00001");
    }

    #[test]
    fn passes_through_plugin_style_brc20_inscription_txparam() {
        let from = taproot_address();
        let response = json!({
            "executeResult": true,
            "executeErrorMsg": "",
            "txParam": {
                "commitTxPrevOutputList": [{
                    "txId": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0",
                    "vout": 0,
                    "amount": "10000"
                }],
                "commitFeeRate": "10",
                "inscriptionDataList": [{
                    "body": "{\"p\":\"brc-20\",\"op\":\"transfer\",\"tick\":\"ordi\",\"amt\":\"100\"}"
                }],
                "inputs": [{
                    "txId": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0",
                    "vout": 0,
                    "amount": "10000",
                    "address": from,
                }],
                "toAddr": from,
                "changeAddress": from,
            },
            "unsignedHashList": [{"index": 0, "unsignedHash": "00"}],
            "signType": "brc20Inscribe",
            "encoding": "hex",
            "extraData": {},
        });
        let preview = preview_from_response(
            &response,
            "BRC20_INSCRIBE",
            "0",
            &from,
            &from,
            Some("btc-brc20-ordi"),
            "100",
            "100",
            8,
        )
        .unwrap();
        validate_preview_intent(
            &preview,
            "BRC20_INSCRIBE",
            "0",
            &from,
            Some(&from),
            Some("100"),
        )
        .unwrap();
        assert_eq!(preview["transaction"], response["txParam"]);
    }

    #[test]
    fn rejects_sign_type_for_another_bitcoin_operation() {
        let from = taproot_address();
        let to = "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh";
        let response = json!({
            "executeResult": true,
            "txParam": {
                "inputs": [{
                    "txId": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0",
                    "vout": 1,
                    "amount": "10000",
                    "address": from,
                }],
                "outputs": [{"address": to, "amount": "5000"}],
                "changeAddress": from,
            },
            "unsignedHashList": [{"index": 0, "unsignedHash": "00"}],
            "signType": "brc20Inscribe",
            "encoding": "eip2519",
        });
        let error = preview_from_response(
            &response,
            "BTC_TRANSFER",
            "0",
            &from,
            to,
            None,
            "5000",
            "0.00005",
            8,
        )
        .unwrap_err();
        assert!(error.to_string().contains("expected signType transfer"));
    }

    #[test]
    fn normalizes_brc20_token_address_without_panicking_on_unicode() {
        assert_eq!(
            normalize_brc20_token_address("BTC-BRC20-ORDI").unwrap(),
            "btc-brc20-ordi"
        );
        assert!(normalize_brc20_token_address("铭文btc-brc20-ordi").is_err());
    }

    #[test]
    fn binds_selected_inputs_to_complete_unavailable_snapshot() {
        let mut preview = json!({
            "inputs": [{
                "txId": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0",
                "vout": 1,
                "amount": "10000"
            }]
        });
        bind_utxo_availability(
            &mut preview,
            json!({"unavailableBreakdown": {"totalUnavailableCount": 0}}),
        )
        .unwrap();
        assert_eq!(
            preview["utxoAvailability"]["selectedAvailableInputs"][0],
            "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0:1"
        );
    }

    #[test]
    fn rejects_selected_input_in_unavailable_snapshot() {
        let tx_hash = "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0";
        let mut preview = json!({
            "inputs": [{"txId": tx_hash, "vout": 1, "amount": "10000"}]
        });
        let error = bind_utxo_availability(
            &mut preview,
            json!({
                "unavailableBreakdown": {
                    "assetLocked": {
                        "count": 1,
                        "utxos": [{"txHash": tx_hash, "voutIndex": "1"}]
                    },
                    "totalUnavailableCount": 1
                }
            }),
        )
        .unwrap_err();
        assert!(error.to_string().contains("PREVIEW_UTXO_UNAVAILABLE"));
    }
}
