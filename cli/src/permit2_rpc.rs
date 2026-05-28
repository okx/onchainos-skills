//! Permit2 allowance pre-check via direct `eth_call`. Surfaces a clean
//! "approve PERMIT2 first" prompt before signing instead of letting the
//! facilitator's on-chain settle revert.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use alloy_primitives::{Address, U256};
use alloy_sol_types::{sol, SolCall};

use crate::chains::{rpc_url_for_chain, PERMIT2_ADDRESS};

sol! {
    interface IERC20 {
        function allowance(address owner, address spender) external view returns (uint256);
    }
}

#[derive(serde::Serialize)]
struct EthCallRequest<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: serde_json::Value,
    id: u32,
}

#[derive(Deserialize)]
struct EthCallResponse {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<EthCallError>,
}

#[derive(Deserialize, Debug)]
struct EthCallError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

/// Returns the buyer's current uint256 allowance for `token` to PERMIT2.
pub async fn fetch_permit2_allowance(
    chain_index: &str,
    token_address: &str,
    owner_address: &str,
) -> Result<U256> {
    let rpc_url = rpc_url_for_chain(chain_index).ok_or_else(|| {
        anyhow!(
            "no RPC endpoint configured for chain {} — Permit2 allowance pre-check is only wired up for X Layer (196) right now",
            chain_index
        )
    })?;

    let token: Address = token_address
        .parse()
        .with_context(|| format!("invalid token address: {}", token_address))?;
    let owner: Address = owner_address
        .parse()
        .with_context(|| format!("invalid owner address: {}", owner_address))?;
    let spender: Address = PERMIT2_ADDRESS
        .parse()
        .expect("PERMIT2_ADDRESS const must be a valid address");

    let call = IERC20::allowanceCall { owner, spender };
    let calldata = call.abi_encode();
    let calldata_hex = format!("0x{}", hex::encode(&calldata));

    let req_body = EthCallRequest {
        jsonrpc: "2.0",
        method: "eth_call",
        params: json!([
            {
                "to":   format!("0x{:x}", token),
                "data": calldata_hex,
            },
            "latest"
        ]),
        id: 1,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("failed to build RPC HTTP client")?;

    let response = client
        .post(rpc_url)
        .json(&req_body)
        .send()
        .await
        .with_context(|| format!("Permit2 allowance RPC POST to {} failed", rpc_url))?;

    if !response.status().is_success() {
        bail!(
            "Permit2 allowance RPC returned HTTP {} from {}",
            response.status(),
            rpc_url
        );
    }

    let body: EthCallResponse = response
        .json()
        .await
        .context("Permit2 allowance RPC returned non-JSON body")?;

    if let Some(err) = body.error {
        bail!(
            "Permit2 allowance RPC error (code {}): {}",
            err.code,
            err.message
        );
    }

    let result_hex = body
        .result
        .ok_or_else(|| anyhow!("Permit2 allowance RPC response missing `result` field"))?;

    parse_uint256_hex(&result_hex)
        .with_context(|| format!("Permit2 allowance RPC returned malformed uint256: {}", result_hex))
}

/// Tolerates short / odd-length hex — some RPC providers strip leading
/// zeros (e.g. return `0xf4240` instead of `0x...00f4240`).
fn parse_uint256_hex(s: &str) -> Result<U256> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    if stripped.is_empty() {
        bail!("empty uint256 hex");
    }
    if stripped.len() > 64 {
        bail!(
            "uint256 hex too long: {} chars (max 64)",
            stripped.len()
        );
    }
    U256::from_str_radix(stripped, 16).with_context(|| format!("not hex: {}", s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uint256_hex_canonical_zero() {
        let s = "0x0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(parse_uint256_hex(s).unwrap(), U256::ZERO);
    }

    #[test]
    fn parse_uint256_hex_canonical_one_usdc() {
        // 1_000_000 (1 USDC at 6 decimals) = 0xf4240
        let s = "0x00000000000000000000000000000000000000000000000000000000000f4240";
        assert_eq!(parse_uint256_hex(s).unwrap(), U256::from(1_000_000u64));
    }

    #[test]
    fn parse_uint256_hex_canonical_max() {
        let s = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        assert_eq!(parse_uint256_hex(s).unwrap(), U256::MAX);
    }

    #[test]
    fn parse_uint256_hex_short_padded() {
        let s = "0xf4240";
        assert_eq!(parse_uint256_hex(s).unwrap(), U256::from(1_000_000u64));
    }

    #[test]
    fn parse_uint256_hex_rejects_garbage() {
        assert!(parse_uint256_hex("0xZZZZ").is_err());
        assert!(parse_uint256_hex("0x").is_err());
    }

    #[test]
    fn parse_uint256_hex_rejects_overflow() {
        let s = format!("0x{}", "ff".repeat(33));
        assert!(parse_uint256_hex(&s).is_err());
    }

    #[tokio::test]
    async fn fetch_allowance_rejects_unsupported_chain() {
        let err = fetch_permit2_allowance(
            "8453",
            "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            "0x000000000000000000000000000000000000beef",
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("no RPC endpoint configured"),
            "unexpected error: {}",
            err
        );
    }

    #[tokio::test]
    async fn fetch_allowance_rejects_bad_token_address() {
        let err = fetch_permit2_allowance("196", "not-an-address", "0x0000000000000000000000000000000000000beef")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid token address"));
    }

    #[tokio::test]
    async fn fetch_allowance_rejects_bad_owner_address() {
        let err = fetch_permit2_allowance(
            "196",
            "0x779ded0c9e1022225f8e0630b35a9b54be713736",
            "not-an-address",
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("invalid owner address"));
    }
}
