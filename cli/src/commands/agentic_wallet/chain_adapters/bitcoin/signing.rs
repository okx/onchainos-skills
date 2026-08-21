//! Applies Bitcoin encoding rules when signing `unsignedHashList` items.

use anyhow::Result;
use serde_json::Value;

use crate::commands::agentic_wallet::support::session::SigningSeed;
use crate::commands::agentic_wallet::support::unsigned_hash_list::{self, SigningProfile};

/// Signs a Bitcoin `unsignedHashList` and returns every item with `sessionSignature`.
pub fn sign_unsigned_hashes(response: &Value, seed: &SigningSeed) -> Result<Vec<Value>> {
    unsigned_hash_list::sign_unsigned_hashes(response, seed, SigningProfile::Bitcoin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use serde_json::json;

    #[test]
    fn rejects_duplicate_indices_before_signing() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let error = sign_unsigned_hashes(
            &json!({"encoding": "hex", "unsignedHashList": [
                {"index": 1, "unsignedHash": "00", "unsignedHashSig": "proof-1"},
                {"index": 1, "unsignedHash": "11", "unsignedHashSig": "proof-2"}
            ]}),
            &seed,
        )
        .unwrap_err();
        assert!(error.to_string().contains("duplicate index"));
    }

    #[test]
    fn eip2519_signs_hex_decoded_digest_bytes() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let hash = format!("0x{}", "ab".repeat(32));
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "eip2519",
                "unsignedHashList": [{
                    "index": 0,
                    "unsignedHash": hash,
                    "unsignedHashSig": "service-proof"
                }]
            }),
            &seed,
        )
        .unwrap();
        let expected = crate::crypto::ed25519_sign(&[1u8; 32], &[0xabu8; 32]).unwrap();
        assert_eq!(
            signed[0]["sessionSignature"],
            base64::engine::general_purpose::STANDARD.encode(expected)
        );
        assert_eq!(signed[0]["unsignedHashSig"], "service-proof");
    }

    #[test]
    fn preserves_all_backend_hash_fields() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "eip2519",
                "unsignedHashList": [{
                    "index": 0,
                    "unsignedHash": format!("0x{}", "ab".repeat(32)),
                    "unsignedHashSig": "service-proof",
                    "backendField": "keep-me"
                }]
            }),
            &seed,
        )
        .unwrap();

        assert_eq!(signed[0]["backendField"], "keep-me");
        assert!(signed[0]["sessionSignature"].is_string());
    }

    #[test]
    fn rejects_missing_unsigned_hash_signature() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let error = sign_unsigned_hashes(
            &json!({
                "encoding": "eip2519",
                "unsignedHashList": [{
                    "index": 0,
                    "unsignedHash": format!("0x{}", "ab".repeat(32))
                }]
            }),
            &seed,
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing unsignedHashSig"));
    }
}
