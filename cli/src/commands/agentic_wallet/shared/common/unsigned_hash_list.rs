use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use super::json::required_string;
use super::session::{decode_hex, SigningSeed};

#[derive(Clone, Copy)]
pub enum SigningProfile {
    Bitcoin,
    Sui,
}

/// Signs every `unsignedHashList` item using the encoding rules for `profile`.
pub fn sign_unsigned_hashes(
    response: &Value,
    seed: &SigningSeed,
    profile: SigningProfile,
) -> Result<Vec<Value>> {
    let items = response
        .get("unsignedHashList")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("signing response is missing unsignedHashList"))?;
    if items.is_empty() {
        bail!("unsignedHashList must not be empty");
    }
    let default_encoding = response
        .get("encoding")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("signing response is missing encoding"))?;
    validate_profile_encoding(default_encoding, profile)?;

    let mut indices = BTreeSet::new();
    let mut validated = Vec::with_capacity(items.len());
    for item in items {
        let index = item
            .get("index")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
            })
            .ok_or_else(|| anyhow::anyhow!("unsignedHashList item is missing index"))?;
        if !indices.insert(index) {
            bail!("unsignedHashList contains duplicate index {index}");
        }
        let unsigned_hash = item
            .get("unsignedHash")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("unsignedHashList[{index}] is missing unsignedHash"))?;
        if matches!(profile, SigningProfile::Bitcoin) {
            item.get("unsignedHashSig")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("unsignedHashList[{index}] is missing unsignedHashSig")
                })?;
        }
        let encoding = match profile {
            SigningProfile::Bitcoin => item
                .get("encoding")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or(default_encoding),
            SigningProfile::Sui => default_encoding,
        };
        validate_profile_encoding(encoding, profile)?;
        validated.push((item, unsigned_hash, encoding));
    }

    validated
        .into_iter()
        .map(|(item, unsigned_hash, encoding)| {
            let bytes = decode_unsigned_hash(unsigned_hash, encoding, profile)?;
            let signature = crate::crypto::ed25519_sign(seed.as_bytes(), &bytes)?;
            let mut signed_item = item.clone();
            signed_item["sessionSignature"] =
                Value::String(base64::engine::general_purpose::STANDARD.encode(signature));
            Ok(signed_item)
        })
        .collect()
}

/// Validates that a backend encoding is supported by the selected chain profile.
fn validate_profile_encoding(encoding: &str, profile: SigningProfile) -> Result<()> {
    let supported = match profile {
        SigningProfile::Bitcoin => matches!(encoding, "eip2519" | "hex" | "base64" | "base58"),
        SigningProfile::Sui => matches!(encoding, "eip2519" | "base64"),
    };
    if supported {
        Ok(())
    } else {
        bail!("unsupported transaction encoding: {encoding}")
    }
}

/// Decodes one unsigned hash according to its chain-specific encoding contract.
fn decode_unsigned_hash(value: &str, encoding: &str, profile: SigningProfile) -> Result<Vec<u8>> {
    let bytes = if value.starts_with("0x") || matches!(encoding, "eip2519" | "hex") {
        decode_hex(value, "unsignedHash")?
    } else {
        match encoding {
            "base64" => base64::engine::general_purpose::STANDARD
                .decode(value)
                .context("unsignedHash is not valid base64")?,
            "base58" => bs58::decode(value)
                .into_vec()
                .context("unsignedHash is not valid base58")?,
            _ => bail!("unsupported transaction encoding: {encoding}"),
        }
    };
    if matches!(profile, SigningProfile::Sui) && bytes.len() != 32 {
        bail!(
            "SUI unsignedHash must decode to 32 bytes, got {}",
            bytes.len()
        );
    }
    Ok(bytes)
}

/// Builds the serialized direct-broadcast payload from prepared data and signed hashes.
pub fn build_direct_extra_data(
    prepared: &Value,
    signed_hashes: &[Value],
    session_cert: &str,
    force: bool,
    chain_label: &str,
) -> Result<String> {
    if signed_hashes.is_empty() {
        bail!("signed hash list must not be empty");
    }
    let unsigned_tx = required_string(prepared, "unsignedTx", "unsignedInfo response")?;
    let sign_type = required_string(prepared, "signType", "unsignedInfo response")?;
    let encoding = required_string(prepared, "encoding", "unsignedInfo response")?;
    let tx_param = prepared
        .get("txParam")
        .filter(|value| !value.is_null())
        .cloned()
        .context("unsignedInfo response is missing txParam")?;
    for item in signed_hashes {
        required_string(item, "unsignedHash", "signed hash item")?;
        required_string(item, "sessionSignature", "signed hash item")?;
    }
    let mut msg_for_sign = json!({
        "unsignedTx": unsigned_tx,
        "sessionCert": session_cert,
        "txParam": tx_param,
        "unsignedHashList": signed_hashes,
    });
    if let Some(unsigned_tx_hash) = prepared
        .get("unsignedTxHash")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        msg_for_sign["unsignedTxHash"] = json!(unsigned_tx_hash);
    }

    let mut extra_data = prepared
        .get("extraData")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    extra_data["checkBalance"] = json!(true);
    extra_data["uopHash"] = prepared
        .get("uopHash")
        .cloned()
        .unwrap_or_else(|| json!(""));
    extra_data["encoding"] = json!(encoding);
    extra_data["signType"] = json!(sign_type);
    extra_data["msgForSign"] = msg_for_sign;
    if let Some(object) = extra_data.as_object_mut() {
        object.remove("signTx");
    }
    if force {
        extra_data["skipWarning"] = json!(true);
    }
    serde_json::to_string(&extra_data)
        .with_context(|| format!("failed to serialize {chain_label} extraData"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sui_signing_preserves_all_hash_items() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "base64",
                "unsignedHashList": [
                    {"index": 0, "unsignedHash": base64::engine::general_purpose::STANDARD.encode([1u8; 32])},
                    {"index": 1, "unsignedHash": base64::engine::general_purpose::STANDARD.encode([2u8; 32])}
                ]
            }),
            &seed,
            SigningProfile::Sui,
        )
        .unwrap();
        assert_eq!(signed.len(), 2);
        assert!(signed
            .iter()
            .all(|item| item["sessionSignature"].is_string()));
    }
}
