//! EIP-712 typed-data for x402 Permit2.
//!
//! Schema is a single source of truth via `sol!` macro — used both for
//! local-key signing (`SolStruct::eip712_signing_hash`) and for the JSON
//! shape the TEE `gen-msg-hash` API expects (`build_*_typed_data`).

use alloy_primitives::Address;
use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::chains::PERMIT2_ADDRESS;

// EIP-712 typehash is derived from struct names — both schemes use
// `Witness` + `PermitWitnessTransferFrom` on the wire, so we keep
// matching names inside two separate inner modules and re-export
// under distinct aliases (Rust forbids same-named structs per module).

pub use sol_exact::PermitWitnessTransferFrom as ExactPermitWitnessTransferFrom;
pub use sol_upto::PermitWitnessTransferFrom as UptoPermitWitnessTransferFrom;

pub(crate) mod sol_exact {
    use alloy_sol_types::sol;
    sol! {
        #[derive(Debug)]
        struct TokenPermissions {
            address token;
            uint256 amount;
        }

        #[derive(Debug)]
        struct Witness {
            address to;
            uint256 validAfter;
        }

        #[derive(Debug)]
        struct PermitWitnessTransferFrom {
            TokenPermissions permitted;
            address spender;
            uint256 nonce;
            uint256 deadline;
            Witness witness;
        }
    }
}

pub(crate) mod sol_upto {
    use alloy_sol_types::sol;
    sol! {
        #[derive(Debug)]
        struct TokenPermissions {
            address token;
            uint256 amount;
        }

        #[derive(Debug)]
        struct Witness {
            address to;
            address facilitator;
            uint256 validAfter;
        }

        #[derive(Debug)]
        struct PermitWitnessTransferFrom {
            TokenPermissions permitted;
            address spender;
            uint256 nonce;
            uint256 deadline;
            Witness witness;
        }
    }
}

/// EIP-712 domain shared by exact + Permit2 / upto (one per chain).
pub fn permit2_domain(chain_id: u64) -> alloy_sol_types::Eip712Domain {
    use alloy_sol_types::eip712_domain;
    eip712_domain! {
        name: "Permit2",
        chain_id: chain_id,
        verifying_contract: PERMIT2_ADDRESS.parse::<Address>().expect("PERMIT2_ADDRESS is a constant"),
    }
}

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

pub fn build_exact_permit2_struct(
    input: &ExactPermit2Input<'_>,
) -> Result<ExactPermitWitnessTransferFrom> {
    Ok(ExactPermitWitnessTransferFrom {
        permitted: sol_exact::TokenPermissions {
            token: input.token.parse().context("invalid token address")?,
            amount: input.amount.parse().context("invalid amount uint256")?,
        },
        spender: input.spender.parse().context("invalid spender address")?,
        nonce: input.nonce.parse().context("invalid nonce uint256")?,
        deadline: input.deadline.parse().context("invalid deadline uint256")?,
        witness: sol_exact::Witness {
            to: input.witness_to.parse().context("invalid witness.to address")?,
            validAfter: input
                .witness_valid_after
                .parse()
                .context("invalid witness.validAfter uint256")?,
        },
    })
}

pub fn build_upto_permit2_struct(
    input: &UptoPermit2Input<'_>,
) -> Result<UptoPermitWitnessTransferFrom> {
    Ok(UptoPermitWitnessTransferFrom {
        permitted: sol_upto::TokenPermissions {
            token: input.token.parse().context("invalid token address")?,
            amount: input.amount.parse().context("invalid amount uint256")?,
        },
        spender: input.spender.parse().context("invalid spender address")?,
        nonce: input.nonce.parse().context("invalid nonce uint256")?,
        deadline: input.deadline.parse().context("invalid deadline uint256")?,
        witness: sol_upto::Witness {
            to: input.witness_to.parse().context("invalid witness.to address")?,
            facilitator: input
                .witness_facilitator
                .parse()
                .context("invalid witness.facilitator address")?,
            validAfter: input
                .witness_valid_after
                .parse()
                .context("invalid witness.validAfter uint256")?,
        },
    })
}

/// `signTypedData_v4` shape consumed by TEE `gen-msg-hash` via `msgType: "eip712"`.
/// Must stay schema-aligned with the `sol!` structs above (tests assert).
pub fn build_exact_permit2_typed_data(input: &ExactPermit2Input<'_>) -> Value {
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

/// Same domain as exact; `Witness` gains a `facilitator` field.
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
    use alloy_primitives::U256;
    use alloy_sol_types::SolStruct;

    fn sample_input() -> ExactPermit2Input<'static> {
        ExactPermit2Input {
            token: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
            amount: "1234000",
            spender: X402_EXACT_PERMIT2_PROXY,
            nonce: "1027389",
            deadline: "1714813500",
            witness_to: "0x000000000000000000000000000000000000beef",
            witness_valid_after: "1714812840",
            chain_id: 196,
        }
    }

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
    fn build_exact_struct_parses_all_fields() {
        let s = build_exact_permit2_struct(&sample_input()).unwrap();
        assert_eq!(s.permitted.amount, U256::from(1234000u64));
        assert_eq!(s.deadline, U256::from(1714813500u64));
    }

    #[test]
    fn build_upto_struct_parses_facilitator() {
        let s = build_upto_permit2_struct(&sample_upto_input()).unwrap();
        let expected: Address = "0x000000000000000000000000000000000000cafe".parse().unwrap();
        assert_eq!(s.witness.facilitator, expected);
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
        let arr = td["types"]["EIP712Domain"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["name"], "name");
        assert_eq!(arr[1]["name"], "chainId");
        assert_eq!(arr[2]["name"], "verifyingContract");
    }

    #[test]
    fn upto_typed_data_witness_lists_facilitator() {
        let td = build_upto_permit2_typed_data(&sample_upto_input());
        let arr = td["types"]["Witness"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["name"], "to");
        assert_eq!(arr[1]["name"], "facilitator");
        assert_eq!(arr[2]["name"], "validAfter");

        assert_eq!(
            td["message"]["witness"]["facilitator"],
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
        let td = build_upto_permit2_typed_data(&sample_upto_input());
        let types = td["types"].as_object().unwrap();
        assert!(types.contains_key("Witness"));
        let outer = td["types"]["PermitWitnessTransferFrom"].as_array().unwrap();
        let witness_field = outer.iter().find(|f| f["name"] == "witness").unwrap();
        assert_eq!(witness_field["type"], "Witness");
    }

    #[test]
    fn exact_struct_eip712_signing_hash_is_non_zero() {
        let s = build_exact_permit2_struct(&sample_input()).unwrap();
        let domain = permit2_domain(196);
        let digest = s.eip712_signing_hash(&domain);
        assert!(!digest.is_zero());
    }

    #[test]
    fn upto_struct_eip712_signing_hash_is_non_zero() {
        let s = build_upto_permit2_struct(&sample_upto_input()).unwrap();
        let domain = permit2_domain(196);
        let digest = s.eip712_signing_hash(&domain);
        assert!(!digest.is_zero());
    }

    // Schema drift detection: parse the sol! `eip712_root_type()` string
    // and assert it matches what `build_*_typed_data` hand-writes in the
    // JSON `types` table. If sol! and JSON ever diverge (one updated and
    // not the other), these tests fail at compile/test time instead of
    // silently producing a wrong on-wire typehash.

    /// Parse `"Name(type1 field1,type2 field2,...)"` → `(name, [(field, type), ...])`.
    fn parse_eip712_type(s: &str) -> (String, Vec<(String, String)>) {
        let open = s.find('(').unwrap();
        let close = s.rfind(')').unwrap();
        let name = s[..open].to_string();
        let fields = s[open + 1..close]
            .split(',')
            .filter(|f| !f.is_empty())
            .map(|f| {
                let mut parts = f.trim().splitn(2, ' ');
                let ty = parts.next().unwrap().to_string();
                let fname = parts.next().unwrap().to_string();
                (fname, ty)
            })
            .collect();
        (name, fields)
    }

    fn assert_json_matches_sol<F: Fn(&str) -> String>(
        sol_type_str: &str,
        json_types_section: &Value,
        wire_name: F,
    ) {
        let (rust_name, sol_fields) = parse_eip712_type(sol_type_str);
        let wire = wire_name(&rust_name);
        let json_fields = json_types_section[&wire]
            .as_array()
            .unwrap_or_else(|| panic!("types JSON missing entry for {wire}"));
        assert_eq!(
            sol_fields.len(),
            json_fields.len(),
            "field count mismatch for {wire}: sol! has {}, JSON has {}",
            sol_fields.len(),
            json_fields.len()
        );
        for (i, (fname, ftype)) in sol_fields.iter().enumerate() {
            let expected_wire_type = wire_name(ftype);
            assert_eq!(
                json_fields[i]["name"], *fname,
                "field {i} name mismatch in {wire}"
            );
            assert_eq!(
                json_fields[i]["type"], expected_wire_type,
                "field {i} type mismatch in {wire}"
            );
        }
    }

    // Rust struct names == wire names == EIP-712 typehash names: no rename needed.
    #[test]
    fn json_types_match_sol_exact() {
        let td = build_exact_permit2_typed_data(&sample_input());
        let types = &td["types"];
        assert_json_matches_sol(
            &ExactPermitWitnessTransferFrom::eip712_root_type(),
            types,
            |s| s.to_string(),
        );
        for comp in ExactPermitWitnessTransferFrom::eip712_components() {
            assert_json_matches_sol(comp.as_ref(), types, |s| s.to_string());
        }
    }

    #[test]
    fn json_types_match_sol_upto() {
        let td = build_upto_permit2_typed_data(&sample_upto_input());
        let types = &td["types"];
        assert_json_matches_sol(
            &UptoPermitWitnessTransferFrom::eip712_root_type(),
            types,
            |s| s.to_string(),
        );
        for comp in UptoPermitWitnessTransferFrom::eip712_components() {
            assert_json_matches_sol(comp.as_ref(), types, |s| s.to_string());
        }
    }
}
