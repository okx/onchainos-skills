//! Permit2 payload types for the x402 exact + Permit2 / upto schemes.
//!
//! Field order is canonical — the facilitator walks the JSON in declared
//! order to recompute the EIP-712 digest, so any reorder breaks signatures.

use serde::{Deserialize, Serialize};

/// Seconds to backdate `witness.validAfter` to absorb clock skew between
/// the client clock and the chain's block time. Callers that build a
/// `Permit2Witness` must subtract this from `now` when setting `valid_after`.
pub const CLOCK_SKEW_BACKDATE_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permit2Permitted {
    pub token: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    pub to: String,
    /// Lower bound on block time. Set to `now - CLOCK_SKEW_BACKDATE_SECS`
    /// to absorb clock skew.
    pub valid_after: String,
}

/// `from` is part of the wrapping payload — NOT the EIP-712 message body —
/// so the verifier knows whose signature to recover.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub from: String,
    pub permitted: Permit2Permitted,
    pub spender: String,
    pub nonce: String,
    pub deadline: String,
    pub witness: Permit2Witness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactPermit2Payload {
    /// 65-byte secp256k1 `r||s||v`, hex with `0x` prefix. EIP-2098 compact
    /// form is rejected on chain.
    pub signature: String,
    pub permit2_authorization: Permit2Authorization,
}

/// Upto witness — adds `facilitator` so the upto proxy can enforce
/// `msg.sender == witness.facilitator` on chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Witness {
    pub to: String,
    pub facilitator: String,
    pub valid_after: String,
}

/// `permitted.amount` is the cap, not the exact charge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Authorization {
    pub from: String,
    pub permitted: Permit2Permitted,
    pub spender: String,
    pub nonce: String,
    pub deadline: String,
    pub witness: UptoPermit2Witness,
}

/// Same wire key `permit2Authorization` as exact — facilitator distinguishes
/// by `accepted.scheme`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Payload {
    pub signature: String,
    pub permit2_authorization: UptoPermit2Authorization,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_authorization() -> Permit2Authorization {
        Permit2Authorization {
            from: "0xBuyer".to_string(),
            permitted: Permit2Permitted {
                token: "0xToken".to_string(),
                amount: "1234000".to_string(),
            },
            spender: "0x402085c248EeA27D92E8b30b2C58ed07f9E20001".to_string(),
            nonce: "1027389".to_string(),
            deadline: "1714813500".to_string(),
            witness: Permit2Witness {
                to: "0xMerchant".to_string(),
                valid_after: "1714812840".to_string(),
            },
        }
    }

    #[test]
    fn witness_serializes_as_camel_case() {
        let w = Permit2Witness {
            to: "0xMerchant".to_string(),
            valid_after: "1714812840".to_string(),
        };
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["to"], "0xMerchant");
        assert_eq!(v["validAfter"], "1714812840");
        assert!(v.get("valid_after").is_none());
    }

    #[test]
    fn authorization_field_order_matches_eip712_hash_input() {
        let auth = sample_authorization();
        let s = serde_json::to_string(&auth).unwrap();

        let positions = [
            s.find("\"from\"").unwrap(),
            s.find("\"permitted\"").unwrap(),
            s.find("\"spender\"").unwrap(),
            s.find("\"nonce\"").unwrap(),
            s.find("\"deadline\"").unwrap(),
            s.find("\"witness\"").unwrap(),
        ];
        for win in positions.windows(2) {
            assert!(win[0] < win[1], "field order drifted from EIP-712 layout");
        }
    }

    #[test]
    fn upto_witness_includes_facilitator_in_camel_case() {
        let w = UptoPermit2Witness {
            to: "0xMerchant".to_string(),
            facilitator: "0xFacilitator".to_string(),
            valid_after: "1714812840".to_string(),
        };
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["to"], "0xMerchant");
        assert_eq!(v["facilitator"], "0xFacilitator");
        assert_eq!(v["validAfter"], "1714812840");

        let s = serde_json::to_string(&w).unwrap();
        let to_pos = s.find("\"to\"").unwrap();
        let facilitator_pos = s.find("\"facilitator\"").unwrap();
        let valid_after_pos = s.find("\"validAfter\"").unwrap();
        assert!(to_pos < facilitator_pos && facilitator_pos < valid_after_pos);
    }

    #[test]
    fn upto_permit2_payload_round_trips_wire_json() {
        let wire = json!({
            "signature": "0xfa42c11c",
            "permit2Authorization": {
                "from": "0xBuyer",
                "permitted": { "token": "0xToken", "amount": "5000000" },
                "spender": "0x4020e7393B728A3939659E5732F87fdd8e680002",
                "nonce": "1027389",
                "deadline": "1714813500",
                "witness": {
                    "to": "0xMerchant",
                    "facilitator": "0xFacilitator",
                    "validAfter": "1714812840"
                }
            }
        });

        let parsed: UptoPermit2Payload = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(parsed.signature, "0xfa42c11c");
        assert_eq!(
            parsed.permit2_authorization.witness.facilitator,
            "0xFacilitator"
        );
        assert_eq!(parsed.permit2_authorization.permitted.amount, "5000000");

        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized, wire);
    }

    #[test]
    fn exact_permit2_payload_round_trips_wire_json() {
        let wire = json!({
            "signature": "0xfa42c11c",
            "permit2Authorization": {
                "from": "0xBuyer",
                "permitted": { "token": "0xToken", "amount": "1234000" },
                "spender": "0x402085c248EeA27D92E8b30b2C58ed07f9E20001",
                "nonce": "1027389",
                "deadline": "1714813500",
                "witness": { "to": "0xMerchant", "validAfter": "1714812840" }
            }
        });

        let parsed: ExactPermit2Payload = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(parsed.signature, "0xfa42c11c");
        assert_eq!(parsed.permit2_authorization.permitted.amount, "1234000");
        assert_eq!(parsed.permit2_authorization.witness.valid_after, "1714812840");

        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(reserialized, wire);
    }
}
