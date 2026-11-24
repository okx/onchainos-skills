//! `payment decode-receipt` — decode an x402 `PAYMENT-RESPONSE` header or a
//! raw charge-receipt JSON into one normalized `{status, transaction,
//! amount, payer, chainId}` shape. Read-only; no auth, no funds.

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;

/// Machine token (leading word of the `output::error` message) for undecodable
/// input.
pub const TOKEN_INVALID_INPUT: &str = "invalid_input";

/// The normalized receipt. `chain_id` serializes as `chainId`.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct DecodedReceipt {
    pub status: String,
    pub transaction: String,
    pub amount: String,
    pub payer: String,
    #[serde(rename = "chainId")]
    pub chain_id: String,
}

/// Decode a `PAYMENT-RESPONSE` header (base64/base64url JSON) OR a raw charge
/// receipt JSON into a `DecodedReceipt`. Exactly one of `header` / `receipt`
/// should be `Some`; `header` takes precedence when both are present.
///
/// Malformed / undecodable input → `invalid_input: could not decode receipt`.
pub fn decode_receipt(header: Option<&str>, receipt: Option<&str>) -> Result<DecodedReceipt> {
    let value = if let Some(h) = header.map(str::trim).filter(|s| !s.is_empty()) {
        // Reuse the shared blob decoder — handles base64 / base64url, padded and
        // unpadded, and falls back to plain JSON.
        super::dispatcher::decode_payment_blob(h)
            .map_err(|_| anyhow!("{TOKEN_INVALID_INPUT}: could not decode receipt"))?
    } else if let Some(r) = receipt.map(str::trim).filter(|s| !s.is_empty()) {
        serde_json::from_str::<Value>(r)
            .map_err(|_| anyhow!("{TOKEN_INVALID_INPUT}: could not decode receipt"))?
    } else {
        return Err(anyhow!("{TOKEN_INVALID_INPUT}: could not decode receipt"));
    };
    Ok(normalize(&value))
}

/// Thin wrapper returning the `data` `Value` (for the MCP `payment_decode_receipt`
/// tool and any caller that wants the serialized shape directly).
pub fn fetch_decode_receipt(header: Option<&str>, receipt: Option<&str>) -> Result<Value> {
    let decoded = decode_receipt(header, receipt)?;
    serde_json::to_value(decoded).map_err(Into::into)
}

/// Pull the first present of a list of candidate keys as a string. Numbers are
/// stringified so atomic amounts / numeric chain ids survive either JSON form.
fn pick(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        match v.get(k) {
            Some(Value::String(s)) if !s.is_empty() => return Some(s.clone()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

/// Normalize an x402 settle response / charge receipt into the stable shape.
///
/// x402 `PAYMENT-RESPONSE` uses `{success, transaction, network, payer}`; the
/// charge receipt uses `{status, txHash, amount, from, chainId}`. We accept the
/// union of aliases so either source decodes to the same fields.
fn normalize(v: &Value) -> DecodedReceipt {
    // status: explicit string wins; else derive from a boolean `success`.
    let status = pick(v, &["status"]).unwrap_or_else(|| match v.get("success") {
        Some(Value::Bool(true)) => "success".to_string(),
        Some(Value::Bool(false)) => "failed".to_string(),
        _ => "unknown".to_string(),
    });
    DecodedReceipt {
        status,
        transaction: pick(v, &["transaction", "txHash", "transactionHash"]).unwrap_or_default(),
        amount: pick(v, &["amount", "value"]).unwrap_or_default(),
        payer: pick(v, &["payer", "from"]).unwrap_or_default(),
        chain_id: pick(v, &["chainId", "network", "chain_id"]).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    #[test]
    fn decodes_x402_payment_response_header() {
        // x402 settle response: {success, transaction, network, payer}.
        let body =
            r#"{"success":true,"transaction":"0xabc123","network":"8453","payer":"0xpayer"}"#;
        let header = B64.encode(body);
        let got = decode_receipt(Some(&header), None).unwrap();
        assert_eq!(
            got,
            DecodedReceipt {
                status: "success".into(),
                transaction: "0xabc123".into(),
                amount: "".into(),
                payer: "0xpayer".into(),
                chain_id: "8453".into(),
            }
        );
    }

    #[test]
    fn decodes_charge_receipt_json() {
        let receipt = r#"{"status":"success","txHash":"0xdef456","amount":"10000","from":"0xfrom","chainId":"196"}"#;
        let got = decode_receipt(None, Some(receipt)).unwrap();
        assert_eq!(
            got,
            DecodedReceipt {
                status: "success".into(),
                transaction: "0xdef456".into(),
                amount: "10000".into(),
                payer: "0xfrom".into(),
                chain_id: "196".into(),
            }
        );
    }

    #[test]
    fn malformed_input_yields_invalid_input() {
        let err = decode_receipt(None, Some("{bad json")).unwrap_err();
        assert!(err.to_string().starts_with(TOKEN_INVALID_INPUT));
        // Neither provided → still invalid_input.
        let err2 = decode_receipt(None, None).unwrap_err();
        assert!(err2.to_string().starts_with(TOKEN_INVALID_INPUT));
    }

    #[test]
    fn success_false_maps_to_failed_status() {
        let receipt = r#"{"success":false,"transaction":"0x0"}"#;
        let got = decode_receipt(None, Some(receipt)).unwrap();
        assert_eq!(got.status, "failed");
    }

    #[test]
    fn no_status_no_success_maps_to_unknown() {
        // Neither `status` nor a boolean `success` → status defaults to "unknown".
        let receipt = r#"{"transaction":"0x0","amount":"1"}"#;
        let got = decode_receipt(None, Some(receipt)).unwrap();
        assert_eq!(got.status, "unknown");
    }

    #[test]
    fn decodes_alias_keys_transaction_hash_value_from_and_numeric_chain() {
        // `transactionHash` / `value` / `from` aliases + numeric chainId.
        let receipt = r#"{"status":"success","transactionHash":"0xaaa","value":"42","from":"0xf","chainId":8453}"#;
        let got = decode_receipt(None, Some(receipt)).unwrap();
        assert_eq!(got.transaction, "0xaaa");
        assert_eq!(got.amount, "42");
        assert_eq!(got.payer, "0xf");
        assert_eq!(got.chain_id, "8453"); // numeric stringified
    }

    #[test]
    fn header_takes_precedence_when_both_supplied() {
        // header decodes to txHash 0xHEADER; receipt would decode to 0xRECEIPT.
        let header = B64.encode(r#"{"success":true,"transaction":"0xHEADER"}"#);
        let receipt = r#"{"status":"success","txHash":"0xRECEIPT"}"#;
        let got = decode_receipt(Some(&header), Some(receipt)).unwrap();
        assert_eq!(got.transaction, "0xHEADER");
    }

    #[test]
    fn empty_header_falls_through_to_receipt() {
        // A blank/whitespace header is ignored so the receipt is used.
        let receipt = r#"{"status":"success","txHash":"0xR"}"#;
        let got = decode_receipt(Some("   "), Some(receipt)).unwrap();
        assert_eq!(got.transaction, "0xR");
    }
}
