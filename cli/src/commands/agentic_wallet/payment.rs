use alloy_primitives::{FixedBytes, U256};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::json;
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::{keyring_store, output, wallet_api::WalletApiClient, wallet_store};

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

/// Select the best entry from the accepts array.
/// Priority: "exact" > "aggr_deferred" > first entry.
fn select_accept(accepts: &[serde_json::Value]) -> Result<(serde_json::Value, Option<String>)> {
    if accepts.is_empty() {
        bail!("accepts array is empty");
    }
    // Prefer exact
    if let Some(entry) = accepts
        .iter()
        .find(|a| a["scheme"].as_str() == Some("exact"))
    {
        return Ok((entry.clone(), Some("exact".to_string())));
    }
    // Then aggr_deferred
    if let Some(entry) = accepts
        .iter()
        .find(|a| a["scheme"].as_str() == Some("aggr_deferred"))
    {
        return Ok((entry.clone(), Some("aggr_deferred".to_string())));
    }
    // Fallback to first
    Ok((
        accepts[0].clone(),
        accepts[0]["scheme"].as_str().map(|s| s.to_string()),
    ))
}

fn parse_payload(raw: &str) -> Result<ResolvedParams> {
    parse_payload_inner(raw, false)
}

/// Like `parse_payload` but ignores scheme priority — just picks the first entry.
/// Used by `eip3009-sign` where local signing is always "exact" semantics.
fn parse_payload_local(raw: &str) -> Result<ResolvedParams> {
    parse_payload_inner(raw, true)
}

fn parse_payload_inner(raw: &str, first_only: bool) -> Result<ResolvedParams> {
    let accepts: Vec<serde_json::Value> =
        serde_json::from_str(raw).context("--accepts must be a valid JSON array")?;
    let (entry, selected_scheme) = if first_only {
        if accepts.is_empty() {
            bail!("accepts array is empty");
        }
        (
            accepts[0].clone(),
            accepts[0]["scheme"].as_str().map(|s| s.to_string()),
        )
    } else {
        select_accept(&accepts)?
    };
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
        PaymentCommand::X402Pay { accepts, from } => {
            let params = parse_payload(&accepts)?;
            cmd_pay(
                &params.network,
                &params.amount,
                &params.pay_to,
                &params.asset,
                from.as_deref(),
                params.max_timeout_seconds,
                params.scheme.as_deref(),
            )
            .await
        }
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

async fn cmd_pay(
    network: &str,
    amount: &str,
    pay_to: &str,
    asset: &str,
    from: Option<&str>,
    max_timeout_secs: u64,
    scheme: Option<&str>,
) -> Result<()> {
    validate_payment_inputs(amount, pay_to, asset)?;

    let access_token = ensure_tokens_refreshed().await?;

    let real_chain_id = parse_eip155_chain_id(network)?;

    // Resolve realChainIndex → OKX chainIndex
    let chain_entry = crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(
        &real_chain_id.to_string(),
    )
    .await?
    .ok_or_else(|| anyhow!("chain not found for realChainIndex {}", real_chain_id))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| chain_entry["chainIndex"].as_u64().map(|n| n.to_string()))
        .ok_or_else(|| anyhow!("missing chainIndex in chain entry"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName in chain entry"))?;

    // 1. Build EIP-3009 authorization message
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    let payer_addr = &addr_info.address;
    let is_deferred = scheme
        .map(|s| s.eq_ignore_ascii_case("aggr_deferred"))
        .unwrap_or(false);
    let valid_before = if is_deferred {
        // aggr_deferred: use max uint256 so the authorization never expires
        U256::MAX.to_string()
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        now.checked_add(max_timeout_secs)
            .ok_or_else(|| anyhow!("timeout overflow"))?
            .to_string()
    };
    let nonce = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        format!("0x{}", hex::encode(n))
    };

    // Shared EIP-3009 fields used across API calls
    let base_fields = json!({
        "chainIndex": chain_index,
        "from": payer_addr,
        "to": pay_to,
        "value": amount,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
        "verifyingContract": asset,
    });

    // 2. Read session data before constructing API client (fail early)
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_cert = &session.session_cert;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let mut client = WalletApiClient::new()?;

    // 3. Get EIP-3009 unsigned hash
    let unsigned_hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("x402 gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing domainHash in gen-msg-hash response"))?;

    // 4. Sign msgHash locally with Ed25519 session key
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    // Return only the standard x402 EIP-3009 authorization fields
    let authorization = json!({
        "from": payer_addr,
        "to": pay_to,
        "value": amount,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
    });

    if is_deferred {
        // aggr_deferred scheme: return session-key signature only, skip EOA signing
        output::success(json!({
            "signature": session_signature_b64,
            "authorization": authorization,
            "sessionCert": session_cert,
        }));
    } else {
        // Exact scheme (default): full EIP-3009 signing via TEE
        let mut signed_hash_body = base_fields.clone();
        signed_hash_body["domainHash"] = json!(domain_hash);
        signed_hash_body["sessionCert"] = json!(session_cert);
        signed_hash_body["sessionSignature"] = json!(session_signature_b64);

        let signed_hash_resp = client
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
                &access_token,
                &signed_hash_body,
            )
            .await
            .map_err(format_api_error)
            .context("x402 sign-msg failed")?;
        let eip3009_signature = signed_hash_resp[0]["signature"]
            .as_str()
            .ok_or_else(|| anyhow!("missing signature in sign-msg response"))?;

        output::success(json!({
            "signature": eip3009_signature,
            "authorization": authorization,
        }));
    }
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

    // ── select_accept ────────────────────────────────────────────────

    #[test]
    fn select_accept_prefers_exact() {
        let accepts: Vec<serde_json::Value> = serde_json::from_str(r#"[
            {"scheme":"aggr_deferred","network":"eip155:196","amount":"2000000","payTo":"0xABC","asset":"0xDEF"},
            {"scheme":"exact","network":"eip155:196","amount":"1000000","payTo":"0xABC","asset":"0xDEF"}
        ]"#).unwrap();
        let (entry, scheme) = select_accept(&accepts).unwrap();
        assert_eq!(scheme.as_deref(), Some("exact"));
        assert_eq!(entry["amount"].as_str().unwrap(), "1000000");
    }

    #[test]
    fn select_accept_falls_back_to_aggr_deferred() {
        let accepts: Vec<serde_json::Value> = serde_json::from_str(r#"[
            {"scheme":"aggr_deferred","network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}
        ]"#).unwrap();
        let (_entry, scheme) = select_accept(&accepts).unwrap();
        assert_eq!(scheme.as_deref(), Some("aggr_deferred"));
    }

    #[test]
    fn select_accept_falls_back_to_first() {
        let accepts: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
            {"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}
        ]"#,
        )
        .unwrap();
        let (_entry, scheme) = select_accept(&accepts).unwrap();
        assert_eq!(scheme, None);
    }

    #[test]
    fn select_accept_empty_array() {
        let accepts: Vec<serde_json::Value> = vec![];
        assert!(select_accept(&accepts).is_err());
    }

    // ── parse_payload ─────────────────────────────────────────────────

    #[test]
    fn parse_payload_prefers_exact() {
        let json = r#"[
            {"scheme":"aggr_deferred","network":"eip155:196","amount":"200","payTo":"0xC","asset":"0xD"},
            {"scheme":"exact","network":"eip155:1","amount":"100","payTo":"0xA","asset":"0xB","maxTimeoutSeconds":600}
        ]"#;
        let p = parse_payload(json).unwrap();
        assert_eq!(p.scheme.as_deref(), Some("exact"));
        assert_eq!(p.network, "eip155:1");
        assert_eq!(p.amount, "100");
        assert_eq!(p.pay_to, "0xA");
        assert_eq!(p.asset, "0xB");
        assert_eq!(p.max_timeout_seconds, 600);
    }

    #[test]
    fn parse_payload_falls_back_to_aggr_deferred() {
        let json = r#"[
            {"scheme":"aggr_deferred","network":"eip155:196","amount":"200","payTo":"0xC","asset":"0xD"}
        ]"#;
        let p = parse_payload(json).unwrap();
        assert_eq!(p.scheme.as_deref(), Some("aggr_deferred"));
        assert_eq!(p.network, "eip155:196");
        assert_eq!(p.amount, "200");
        assert_eq!(p.pay_to, "0xC");
        assert_eq!(p.asset, "0xD");
        assert_eq!(p.max_timeout_seconds, 300); // no maxTimeoutSeconds → default
    }

    #[test]
    fn parse_payload_max_amount_required() {
        let json = r#"[{"scheme":"aggr_deferred","network":"eip155:1","maxAmountRequired":"999","payTo":"0xA","asset":"0xB"}]"#;
        let p = parse_payload(json).unwrap();
        assert_eq!(p.amount, "999");
    }

    #[test]
    fn parse_payload_numeric_amount() {
        let json =
            r#"[{"scheme":"exact","network":"eip155:1","amount":500,"payTo":"0xA","asset":"0xB"}]"#;
        let p = parse_payload(json).unwrap();
        assert_eq!(p.amount, "500");
    }

    #[test]
    fn parse_payload_invalid_json() {
        assert!(parse_payload("not json").is_err());
    }

    #[test]
    fn parse_payload_missing_network() {
        let json = r#"[{"amount":"100","payTo":"0xA","asset":"0xB"}]"#;
        assert!(parse_payload(json).is_err());
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
    fn cli_eip3009_sign_missing_from() {
        let json = r#"[{"network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--accepts", json]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_eip3009_sign_missing_accepts() {
        let result = TestCli::try_parse_from(["test", "eip3009-sign", "--from", "0xPayer"]);
        assert!(result.is_err());
    }

    // ── parse_payload with extra (domain) fields ────────────────────

    #[test]
    fn parse_payload_extracts_domain_from_extra() {
        let json = r#"[{"scheme":"exact","network":"eip155:8453","amount":"1000000","payTo":"0xA","asset":"0xB","extra":{"name":"USD Coin","version":"2"}}]"#;
        let p = parse_payload(json).unwrap();
        assert_eq!(p.domain_name.as_deref(), Some("USD Coin"));
        assert_eq!(p.domain_version.as_deref(), Some("2"));
    }

    #[test]
    fn parse_payload_no_extra_returns_none() {
        let json = r#"[{"scheme":"exact","network":"eip155:1","amount":"500","payTo":"0xA","asset":"0xB"}]"#;
        let p = parse_payload(json).unwrap();
        assert_eq!(p.domain_name, None);
        assert_eq!(p.domain_version, None);
    }
}
