//! Applies SUI encoding rules to transaction digests.

use anyhow::Result;
use serde_json::Value;

use crate::commands::agentic_wallet::support::session::SigningSeed;
use crate::commands::agentic_wallet::support::unsigned_hash_list::{self, SigningProfile};

/// Signs a SUI `unsignedHashList` and returns every item with `sessionSignature`.
pub fn sign_unsigned_hashes(response: &Value, seed: &SigningSeed) -> Result<Vec<Value>> {
    unsigned_hash_list::sign_unsigned_hashes(response, seed, SigningProfile::Sui)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use serde_json::json;

    #[test]
    fn eip2519_signs_hex_decoded_digest_bytes() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let hash = format!("0x{}", "ab".repeat(32));
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "eip2519",
                "unsignedHashList": [{"index": 0, "unsignedHash": hash}]
            }),
            &seed,
        )
        .unwrap();
        let expected = crate::crypto::ed25519_sign(&[1u8; 32], &[0xabu8; 32]).unwrap();
        assert_eq!(
            signed[0]["sessionSignature"],
            base64::engine::general_purpose::STANDARD.encode(expected)
        );
    }

    #[test]
    fn base64_signs_decoded_digest_bytes() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let hash = base64::engine::general_purpose::STANDARD.encode([0xabu8; 32]);
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "base64",
                "unsignedHashList": [{"index": 0, "unsignedHash": hash}]
            }),
            &seed,
        )
        .unwrap();
        let expected = crate::crypto::ed25519_sign(&[1u8; 32], &[0xabu8; 32]).unwrap();
        assert_eq!(
            signed[0]["sessionSignature"],
            base64::engine::general_purpose::STANDARD.encode(expected)
        );
    }

    #[test]
    fn base64_encoding_accepts_explicit_hex_digest() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let hash = format!("0x{}", "ab".repeat(32));
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "base64",
                "unsignedHashList": [{"index": 0, "unsignedHash": hash}]
            }),
            &seed,
        )
        .unwrap();
        let expected = crate::crypto::ed25519_sign(&[1u8; 32], &[0xabu8; 32]).unwrap();
        assert_eq!(
            signed[0]["sessionSignature"],
            base64::engine::general_purpose::STANDARD.encode(expected)
        );
    }

    #[test]
    fn response_encoding_remains_authoritative_for_sui_items() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let hash = base64::engine::general_purpose::STANDARD.encode([0xabu8; 32]);
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "base64",
                "unsignedHashList": [{
                    "index": 0,
                    "encoding": "eip2519",
                    "unsignedHash": hash
                }]
            }),
            &seed,
        )
        .unwrap();
        assert!(signed[0]["sessionSignature"].is_string());
    }

    #[test]
    fn preserves_all_backend_hash_fields() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        let hash = base64::engine::general_purpose::STANDARD.encode([0xabu8; 32]);
        let signed = sign_unsigned_hashes(
            &json!({
                "encoding": "base64",
                "unsignedHashList": [{
                    "index": 0,
                    "unsignedHash": hash,
                    "unsignedHashSig": "service-proof",
                    "backendField": "keep-me"
                }]
            }),
            &seed,
        )
        .unwrap();

        assert_eq!(signed[0]["unsignedHashSig"], "service-proof");
        assert_eq!(signed[0]["backendField"], "keep-me");
        assert!(signed[0]["sessionSignature"].is_string());
    }

    #[test]
    fn rejects_wrong_digest_length_and_duplicate_index() {
        let seed = SigningSeed::from_bytes([1u8; 32]);
        assert!(sign_unsigned_hashes(
            &json!({"encoding":"eip2519","unsignedHashList":[{"index":0,"unsignedHash":"00"}]}),
            &seed,
        )
        .is_err());
        assert!(sign_unsigned_hashes(
            &json!({"encoding":"eip2519","unsignedHashList":[
                {"index":0,"unsignedHash":"00"}, {"index":0,"unsignedHash":"00"}
            ]}),
            &seed,
        )
        .is_err());
    }
}
