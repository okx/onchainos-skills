//! EIP-712 typed-data builders for x402 Permit2.

use serde_json::{json, Value};

use crate::chains::PERMIT2_ADDRESS;

/// All uint256 fields are decimal strings — Permit2 values exceed u64.
#[derive(Debug, Clone)]
pub struct ExactPermit2Input<'a> {
    pub token: &'a str,
    pub amount: &'a str,
    pub spender: &'a str,
    pub nonce: &'a str,
    pub deadline: &'a str,
    pub witness_to: &'a str,
    pub witness_valid_after: &'a str,
    pub chain_id: u64,
}

/// Returns the `signTypedData_v4` input shape (domain / types / primaryType
/// / message) that the TEE consumes via `msgType: "eip712"`.
pub fn build_exact_permit2_typed_data(input: &ExactPermit2Input<'_>) -> Value {
    json!({
        "domain": {
            "name": "Permit2",
            "chainId": input.chain_id,
            "verifyingContract": PERMIT2_ADDRESS,
        },
        "types": {
            // Permit2 domain has no `version` / `salt`.
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" },
            ],
            "PermitWitnessTransferFrom": [
                { "name": "permitted", "type": "TokenPermissions" },
                { "name": "spender",   "type": "address"          },
                { "name": "nonce",     "type": "uint256"          },
                { "name": "deadline",  "type": "uint256"          },
                { "name": "witness",   "type": "Witness"          },
            ],
            "TokenPermissions": [
                { "name": "token",  "type": "address" },
                { "name": "amount", "type": "uint256" },
            ],
            "Witness": [
                { "name": "to",         "type": "address" },
                { "name": "validAfter", "type": "uint256" },
            ],
        },
        "primaryType": "PermitWitnessTransferFrom",
        "message": {
            "permitted": {
                "token":  input.token,
                "amount": input.amount,
            },
            "spender":  input.spender,
            "nonce":    input.nonce,
            "deadline": input.deadline,
            "witness": {
                "to":         input.witness_to,
                "validAfter": input.witness_valid_after,
            },
        },
    })
}

/// `amount` is the cap; `witness_facilitator` is the on-chain `msg.sender`
/// the buyer authorizes (the upto proxy enforces this equality).
#[derive(Debug, Clone)]
pub struct UptoPermit2Input<'a> {
    pub token: &'a str,
    pub amount: &'a str,
    pub spender: &'a str,
    pub nonce: &'a str,
    pub deadline: &'a str,
    pub witness_to: &'a str,
    pub witness_facilitator: &'a str,
    pub witness_valid_after: &'a str,
    pub chain_id: u64,
}

/// Same domain as exact (Permit2 has one DOMAIN_SEPARATOR per chain);
/// `Witness` gains a `facilitator` field between `to` and `validAfter`.
pub fn build_upto_permit2_typed_data(input: &UptoPermit2Input<'_>) -> Value {
    json!({
        "domain": {
            "name": "Permit2",
            "chainId": input.chain_id,
            "verifyingContract": PERMIT2_ADDRESS,
        },
        "types": {
            "EIP712Domain": [
                { "name": "name",              "type": "string"  },
                { "name": "chainId",           "type": "uint256" },
                { "name": "verifyingContract", "type": "address" },
            ],
            "PermitWitnessTransferFrom": [
                { "name": "permitted", "type": "TokenPermissions" },
                { "name": "spender",   "type": "address"          },
                { "name": "nonce",     "type": "uint256"          },
                { "name": "deadline",  "type": "uint256"          },
                { "name": "witness",   "type": "Witness"          },
            ],
            "TokenPermissions": [
                { "name": "token",  "type": "address" },
                { "name": "amount", "type": "uint256" },
            ],
            // Order is fixed — any swap changes the EIP-712 typehash.
            "Witness": [
                { "name": "to",          "type": "address" },
                { "name": "facilitator", "type": "address" },
                { "name": "validAfter",  "type": "uint256" },
            ],
        },
        "primaryType": "PermitWitnessTransferFrom",
        "message": {
            "permitted": {
                "token":  input.token,
                "amount": input.amount,
            },
            "spender":  input.spender,
            "nonce":    input.nonce,
            "deadline": input.deadline,
            "witness": {
                "to":          input.witness_to,
                "facilitator": input.witness_facilitator,
                "validAfter":  input.witness_valid_after,
            },
        },
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::chains::X402_EXACT_PERMIT2_PROXY;

    fn sample_input() -> ExactPermit2Input<'static> {
        ExactPermit2Input {
            token: "0x779ded0c9e1022225f8e0630b35a9b54be713736", // X Layer USDT
            amount: "1234000",
            spender: X402_EXACT_PERMIT2_PROXY,
            nonce: "1027389",
            deadline: "1714813500",
            witness_to: "0x000000000000000000000000000000000000beef",
            witness_valid_after: "1714812840",
            chain_id: 196,
        }
    }

    #[test]
    fn typed_data_uses_permit2_domain_without_version() {
        let td = build_exact_permit2_typed_data(&sample_input());
        let domain = &td["domain"];
        assert_eq!(domain["name"], "Permit2");
        assert_eq!(domain["chainId"], 196);
        assert_eq!(domain["verifyingContract"], PERMIT2_ADDRESS);
        assert!(domain.get("version").is_none());
        assert!(domain.get("salt").is_none());
    }

    #[test]
    fn typed_data_primary_type_and_message_shape() {
        let td = build_exact_permit2_typed_data(&sample_input());
        assert_eq!(td["primaryType"], "PermitWitnessTransferFrom");
        let m = &td["message"];
        assert_eq!(m["permitted"]["token"], "0x779ded0c9e1022225f8e0630b35a9b54be713736");
        assert_eq!(m["permitted"]["amount"], "1234000");
        assert_eq!(m["spender"], X402_EXACT_PERMIT2_PROXY);
        assert_eq!(m["witness"]["validAfter"], "1714812840");
    }

    #[test]
    fn typed_data_types_section_lists_eip712domain_without_version() {
        let td = build_exact_permit2_typed_data(&sample_input());
        let domain_type = &td["types"]["EIP712Domain"];
        let arr = domain_type.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["name"], "name");
        assert_eq!(arr[1]["name"], "chainId");
        assert_eq!(arr[2]["name"], "verifyingContract");
    }

    // ── upto-scheme tests ──────────────────────────────────────────

    fn sample_upto_input() -> UptoPermit2Input<'static> {
        UptoPermit2Input {
            token: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
            amount: "5000000",
            spender: "0x4020e7393B728A3939659E5732F87fdd8e680002",
            nonce: "1027389",
            deadline: "1714813500",
            witness_to: "0x000000000000000000000000000000000000beef",
            witness_facilitator: "0x000000000000000000000000000000000000cafe",
            witness_valid_after: "1714812840",
            chain_id: 196,
        }
    }

    #[test]
    fn upto_typed_data_witness_lists_facilitator() {
        let td = build_upto_permit2_typed_data(&sample_upto_input());
        let witness_type = &td["types"]["Witness"];
        let arr = witness_type.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["name"], "to");
        assert_eq!(arr[1]["name"], "facilitator");
        assert_eq!(arr[2]["name"], "validAfter");

        let m_witness = &td["message"]["witness"];
        assert_eq!(
            m_witness["facilitator"],
            "0x000000000000000000000000000000000000cafe"
        );
    }

    #[test]
    fn upto_typed_data_shares_permit2_domain_with_exact() {
        let exact = build_exact_permit2_typed_data(&sample_input());
        let upto = build_upto_permit2_typed_data(&sample_upto_input());
        assert_eq!(exact["domain"], upto["domain"]);
    }

    #[test]
    fn upto_types_section_uses_canonical_witness_name() {
        // Witness MUST be named "Witness" — backend pins this typehash.
        let td = build_upto_permit2_typed_data(&sample_upto_input());
        let types = td["types"].as_object().unwrap();
        assert!(types.contains_key("Witness"));
        assert!(!types.contains_key("UptoWitness"));
        let outer = td["types"]["PermitWitnessTransferFrom"].as_array().unwrap();
        let witness_field = outer.iter().find(|f| f["name"] == "witness").unwrap();
        assert_eq!(witness_field["type"], "Witness");
    }
}
