use alloy_primitives::{FixedBytes, U256};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::payment_flow;
use crate::output;

#[derive(Subcommand)]
pub enum PaymentCommand {
    /// Sign an x402 payment and return the payment proof
    X402Pay {
        /// JSON accepts array from the 402 response (decoded.accepts).
        /// The CLI selects the best scheme automatically
        /// (prefers "exact", falls back to "aggr_deferred", then first entry).
        #[arg(long)]
        accepts: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
    },
    /// Sign an EIP-3009 TransferWithAuthorization locally with a hex private key
    /// (reads EVM_PRIVATE_KEY env var). Accepts the same JSON accepts array as x402-pay;
    /// domain name/version are read from accepts[].extra.name / extra.version.
    Eip3009Sign {
        /// JSON accepts array from the 402 response (same format as x402-pay).
        /// domain name/version are extracted from the selected entry's `extra.name` / `extra.version`.
        #[arg(long)]
        accepts: String,
    },
}

/// Resolved parameters extracted from the accepts array.
struct ResolvedParams {
    network: String,
    amount: String,
    pay_to: String,
    asset: String,
    max_timeout_seconds: u64,
    scheme: Option<String>,
    /// EIP-712 domain name from `extra.name` (e.g. "USD Coin")
    domain_name: Option<String>,
    /// EIP-712 domain version from `extra.version` (e.g. "2")
    domain_version: Option<String>,
}

/// Parse the accepts JSON for `eip3009-sign` (local signing path).
/// Always picks the first entry — local signing is always "exact" semantics.
fn parse_payload_local(raw: &str) -> Result<ResolvedParams> {
    let accepts: Vec<Value> =
        serde_json::from_str(raw).context("--accepts must be a valid JSON array")?;
    if accepts.is_empty() {
        bail!("accepts array is empty");
    }
    let entry = accepts[0].clone();
    let selected_scheme = entry["scheme"].as_str().map(|s| s.to_string());
    let network = entry["network"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'network' in selected accepts entry"))?
        .to_string();
    // Extract amount (handle both string and number), fall back to maxAmountRequired
    let amount = if let Some(s) = entry.get("amount").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(n) = entry.get("amount").and_then(|v| v.as_u64()) {
        n.to_string()
    } else if let Some(s) = entry.get("maxAmountRequired").and_then(|v| v.as_str()) {
        s.to_string()
    } else if let Some(n) = entry.get("maxAmountRequired").and_then(|v| v.as_u64()) {
        n.to_string()
    } else {
        bail!("missing 'amount' or 'maxAmountRequired' in accepts entry");
    };
    let pay_to = entry["payTo"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'payTo' in selected accepts entry"))?
        .to_string();
    let asset = entry["asset"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'asset' in selected accepts entry"))?
        .to_string();
    let max_timeout = entry["maxTimeoutSeconds"].as_u64().unwrap_or(300);
    let domain_name = entry["extra"]["name"].as_str().map(|s| s.to_string());
    let domain_version = entry["extra"]["version"].as_str().map(|s| s.to_string());
    Ok(ResolvedParams {
        network,
        amount,
        pay_to,
        asset,
        max_timeout_seconds: max_timeout,
        scheme: selected_scheme,
        domain_name,
        domain_version,
    })
}

/// Read `EVM_PRIVATE_KEY` from the environment variable.
/// Falls back to `~/.onchainos/.env` if the env var is not set.
fn read_private_key() -> Result<String> {
    std::env::var("EVM_PRIVATE_KEY").or_else(|_| {
        let env_path = crate::home::onchainos_home()?.join(".env");
        let content = std::fs::read_to_string(&env_path).with_context(|| {
            format!(
                "EVM_PRIVATE_KEY not set and {} not found",
                env_path.display()
            )
        })?;
        for line in content.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("EVM_PRIVATE_KEY=") {
                if !val.is_empty() {
                    return Ok(val.to_string());
                }
            }
        }
        Err(anyhow!(
            "EVM_PRIVATE_KEY not found in {}",
            env_path.display()
        ))
    })
}

pub async fn execute(cmd: PaymentCommand) -> Result<()> {
    match cmd {
        PaymentCommand::X402Pay { accepts, from } => cmd_pay(&accepts, from.as_deref()).await,
        PaymentCommand::Eip3009Sign { accepts } => {
            let params = parse_payload_local(&accepts)?;
            let pk = read_private_key()?;
            let domain_name = params.domain_name.as_deref().ok_or_else(|| {
                anyhow!("missing 'extra.name' (EIP-712 domain name) in accepts entry")
            })?;
            let domain_version = params.domain_version.as_deref().unwrap_or("2");
            cmd_eip3009_sign(
                &pk,
                &params.network,
                &params.amount,
                &params.pay_to,
                &params.asset,
                params.max_timeout_seconds,
                domain_name,
                domain_version,
            )
        }
    }
}

/// Validate common payment inputs: amount, pay_to, asset.
/// Returns the parsed amount as u128.
fn validate_payment_inputs(amount: &str, pay_to: &str, asset: &str) -> Result<u128> {
    if amount.is_empty() {
        bail!("--amount must not be empty");
    }
    let parsed_amount = amount
        .parse::<u128>()
        .context("--amount must be a non-negative integer in minimal units")?;
    if parsed_amount == 0 {
        bail!("--amount must be greater than zero");
    }
    if !is_valid_evm_address(pay_to) {
        bail!("--pay-to must be a valid EVM address (0x + 40 hex chars)");
    }
    if !is_valid_evm_address(asset) {
        bail!("--asset must be a valid EVM contract address (0x + 40 hex chars)");
    }
    Ok(parsed_amount)
}

/// Sign an x402 payment authorization and print the proof as JSON.
/// All crypto happens in `payment_flow::sign_payment`, which is also driven by
/// the client-level auto-payment flow.
async fn cmd_pay(accepts_json: &str, from: Option<&str>) -> Result<()> {
    let accepts: Value =
        serde_json::from_str(accepts_json).context("--accepts must be a valid JSON array")?;
    let (proof, _entry) = payment_flow::sign_payment(&accepts, from).await?;
    let mut out = json!({
        "signature": proof.signature,
        "authorization": proof.authorization,
    });
    if let Some(cert) = proof.session_cert {
        out["sessionCert"] = json!(cert);
    }
    output::success(out);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_eip3009_sign(
    private_key_hex: &str,
    network: &str,
    amount: &str,
    pay_to: &str,
    asset: &str,
    max_timeout_secs: u64,
    domain_name: &str,
    domain_version: &str,
) -> Result<()> {
    use alloy_signer_local::PrivateKeySigner;

    let parsed_amount = validate_payment_inputs(amount, pay_to, asset)?;

    // ── Parse private key ────────────────────────────────────────────
    let pk_clean = private_key_hex
        .strip_prefix("0x")
        .unwrap_or(private_key_hex);
    let mut pk_bytes = hex::decode(pk_clean).context("EVM_PRIVATE_KEY is not valid hex")?;
    if pk_bytes.len() != 32 {
        bail!(
            "EVM_PRIVATE_KEY must be 32 bytes (64 hex chars), got {}",
            pk_bytes.len()
        );
    }

    // ── Derive from address from private key ────────────────────────
    let signer = PrivateKeySigner::from_slice(&pk_bytes)
        .map_err(|e| anyhow!("invalid secp256k1 private key: {e}"))?;
    let from = format!("{:#x}", signer.address());

    // ── Derive chain_id from network ─────────────────────────────────
    let chain_id = parse_eip155_chain_id(network)?;

    // ── Compute validBefore = now + max_timeout_secs ─────────────────
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_before = now
        .checked_add(max_timeout_secs)
        .ok_or_else(|| anyhow!("timeout overflow"))?;

    // ── Generate random nonce ────────────────────────────────────────
    let nonce = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        FixedBytes::from(n)
    };

    // ── Build auth struct & domain ───────────────────────────────────
    let auth = crate::crypto::TransferWithAuthorization {
        from: from.parse().context("--from is not a valid EVM address")?,
        to: pay_to
            .parse()
            .context("--pay-to is not a valid EVM address")?,
        value: U256::from(parsed_amount),
        validAfter: U256::ZERO,
        validBefore: U256::from(valid_before),
        nonce,
    };
    let domain = crate::crypto::Eip3009DomainParams {
        name: domain_name.to_string(),
        version: domain_version.to_string(),
        chain_id,
        verifying_contract: asset
            .parse()
            .context("--asset is not a valid EVM address")?,
    };

    // ── Sign ─────────────────────────────────────────────────────────
    let sig_b64 = crate::crypto::eip3009_sign(&auth, &domain, &pk_bytes)?;
    pk_bytes.zeroize();

    let sig_bytes = B64
        .decode(&sig_b64)
        .context("unexpected base64 decode error")?;

    let nonce_hex = format!("0x{}", hex::encode(nonce));
    output::success(json!({
        "signature": format!("0x{}", hex::encode(&sig_bytes)),
        "authorization": {
            "from": from,
            "to": pay_to,
            "value": amount,
            "validAfter": "0",
            "validBefore": valid_before.to_string(),
            "nonce": nonce_hex,
        }
    }));
    Ok(())
}

fn is_valid_evm_address(addr: &str) -> bool {
    addr.starts_with("0x") && addr.len() == 42 && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Extract numeric chain ID from a CAIP-2 "eip155:<chainId>" identifier.
fn parse_eip155_chain_id(network: &str) -> Result<u64> {
    let id_str = network.strip_prefix("eip155:").ok_or_else(|| {
        anyhow!(
            "unsupported network format: expected 'eip155:<chainId>', got '{}'",
            network
        )
    })?;
    id_str.parse::<u64>().map_err(|_| {
        anyhow!(
            "invalid chain ID '{}': must be a valid unsigned integer",
            id_str
        )
    })
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── parse_eip155_chain_id ─────────────────────────────────────────

    #[test]
    fn parse_eip155_base() {
        assert_eq!(parse_eip155_chain_id("eip155:8453").unwrap(), 8453);
    }

    #[test]
    fn parse_eip155_ethereum() {
        assert_eq!(parse_eip155_chain_id("eip155:1").unwrap(), 1);
    }

    #[test]
    fn parse_eip155_xlayer() {
        assert_eq!(parse_eip155_chain_id("eip155:196").unwrap(), 196);
    }

    #[test]
    fn parse_eip155_missing_prefix() {
        let err = parse_eip155_chain_id("8453").unwrap_err();
        assert!(err.to_string().contains("eip155:"));
    }

    #[test]
    fn parse_eip155_wrong_prefix() {
        let err = parse_eip155_chain_id("solana:101").unwrap_err();
        assert!(err.to_string().contains("eip155:"));
    }

    #[test]
    fn parse_eip155_empty() {
        assert!(parse_eip155_chain_id("").is_err());
    }

    #[test]
    fn parse_eip155_non_numeric() {
        let err = parse_eip155_chain_id("eip155:abc").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    #[test]
    fn parse_eip155_negative() {
        let err = parse_eip155_chain_id("eip155:-1").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    #[test]
    fn parse_eip155_overflow() {
        let err = parse_eip155_chain_id("eip155:99999999999999999999").unwrap_err();
        assert!(err.to_string().contains("invalid chain ID"));
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    /// Wrapper so clap can parse PaymentCommand as a top-level subcommand.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: PaymentCommand,
    }

    // ── parse_payload_local (eip3009-sign input parsing) ─────────────

    #[test]
    fn parse_payload_local_picks_first_entry() {
        // parse_payload_local always picks accepts[0] — scheme priority lives in payment_flow.
        let json = r#"[
            {"scheme":"aggr_deferred","network":"eip155:196","amount":"200","payTo":"0xC","asset":"0xD"},
            {"scheme":"exact","network":"eip155:1","amount":"100","payTo":"0xA","asset":"0xB"}
        ]"#;
        let p = parse_payload_local(json).unwrap();
        assert_eq!(p.scheme.as_deref(), Some("aggr_deferred"));
        assert_eq!(p.network, "eip155:196");
        assert_eq!(p.amount, "200");
    }

    #[test]
    fn parse_payload_local_max_amount_required() {
        let json = r#"[{"scheme":"aggr_deferred","network":"eip155:1","maxAmountRequired":"999","payTo":"0xA","asset":"0xB"}]"#;
        let p = parse_payload_local(json).unwrap();
        assert_eq!(p.amount, "999");
    }

    #[test]
    fn parse_payload_local_numeric_amount() {
        let json =
            r#"[{"scheme":"exact","network":"eip155:1","amount":500,"payTo":"0xA","asset":"0xB"}]"#;
        let p = parse_payload_local(json).unwrap();
        assert_eq!(p.amount, "500");
    }

    #[test]
    fn parse_payload_local_invalid_json() {
        assert!(parse_payload_local("not json").is_err());
    }

    #[test]
    fn parse_payload_local_missing_network() {
        let json = r#"[{"amount":"100","payTo":"0xA","asset":"0xB"}]"#;
        assert!(parse_payload_local(json).is_err());
    }

    #[test]
    fn parse_payload_local_empty_array() {
        assert!(parse_payload_local("[]").is_err());
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    #[test]
    fn cli_x402_pay_accepts_and_from() {
        let json = r#"[{"scheme":"aggr_deferred","network":"eip155:196","amount":"1000","payTo":"0xA","asset":"0xB"}]"#;
        let cli = TestCli::parse_from(["test", "x402-pay", "--accepts", json, "--from", "0xPayer"]);
        match cli.command {
            PaymentCommand::X402Pay { accepts, from } => {
                assert_eq!(accepts, json);
                assert_eq!(from.as_deref(), Some("0xPayer"));
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_accepts_only() {
        let json = r#"[{"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let cli = TestCli::parse_from(["test", "x402-pay", "--accepts", json]);
        match cli.command {
            PaymentCommand::X402Pay { accepts, from } => {
                assert_eq!(accepts, json);
                assert_eq!(from, None);
            }
            _ => panic!("expected X402Pay"),
        }
    }

    #[test]
    fn cli_x402_pay_missing_accepts() {
        let result = TestCli::try_parse_from(["test", "x402-pay"]);
        assert!(result.is_err());
    }

    // ── eip3009-sign CLI parsing ─────────────────────────────────────

    #[test]
    fn cli_eip3009_sign_accepts_and_from() {
        let json = r#"[{"scheme":"exact","network":"eip155:8453","amount":"1000000","payTo":"0xA","asset":"0xB","extra":{"name":"USD Coin","version":"2"}}]"#;
        let cli = TestCli::parse_from(["test", "eip3009-sign", "--accepts", json]);
        match cli.command {
            PaymentCommand::Eip3009Sign { accepts } => {
                assert_eq!(accepts, json);
            }
            _ => panic!("expected Eip3009Sign"),
        }
    }

    #[test]
    fn cli_eip3009_sign_rejects_unknown_from_flag() {
        // Eip3009Sign has no --from flag, so passing it should fail to parse.
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--from", "0xPayer"]);
        assert!(result.is_err());
    }

    // ── parse_payload_local with extra (domain) fields ───────────────

    #[test]
    fn parse_payload_local_extracts_domain_from_extra() {
        let json = r#"[{"scheme":"exact","network":"eip155:8453","amount":"1000000","payTo":"0xA","asset":"0xB","extra":{"name":"USD Coin","version":"2"}}]"#;
        let p = parse_payload_local(json).unwrap();
        assert_eq!(p.domain_name.as_deref(), Some("USD Coin"));
        assert_eq!(p.domain_version.as_deref(), Some("2"));
    }

    #[test]
    fn parse_payload_local_no_extra_returns_none() {
        let json = r#"[{"scheme":"exact","network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let p = parse_payload_local(json).unwrap();
        assert_eq!(p.domain_name, None);
        assert_eq!(p.domain_version, None);
    }
}
