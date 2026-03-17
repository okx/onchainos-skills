use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::json;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::{keyring_store, output, wallet_api::WalletApiClient, wallet_store};

#[derive(Subcommand)]
pub enum PaymentCommand {
    /// Sign an x402 payment and return the payment proof
    X402Pay {
        /// CAIP-2 network identifier (e.g. eip155:8453)
        #[arg(long)]
        network: String,
        /// Payment amount in minimal units
        #[arg(long)]
        amount: String,
        /// Recipient address
        #[arg(long)]
        pay_to: String,
        /// Token contract address (asset)
        #[arg(long)]
        asset: String,
        /// Payer address (optional, defaults to selected account)
        #[arg(long)]
        from: Option<String>,
        /// Maximum timeout in seconds
        #[arg(long, default_value = "300")]
        max_timeout_seconds: u64,
    },
}

pub async fn execute(cmd: PaymentCommand) -> Result<()> {
    match cmd {
        PaymentCommand::X402Pay {
            network,
            amount,
            pay_to,
            asset,
            from,
            max_timeout_seconds,
        } => pay(&network, &amount, &pay_to, &asset, from.as_deref(), max_timeout_seconds).await,
    }
}

async fn pay(
    network: &str,
    amount: &str,
    pay_to: &str,
    asset: &str,
    from: Option<&str>,
    max_timeout_secs: u64,
) -> Result<()> {
    let access_token = ensure_tokens_refreshed().await?;

    let real_chain_id = parse_eip155_chain_id(network)
        .ok_or_else(|| anyhow!("unsupported network: {}", network))?;

    // Resolve realChainIndex → OKX chainIndex
    let chain_entry = crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(
        &real_chain_id.to_string(),
    )
    .await?
    .ok_or_else(|| anyhow!("chain not found for realChainIndex {}", real_chain_id))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainIndex in chain entry"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName in chain entry"))?;

    // 1. Build EIP-3009 authorization message
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    let from = &addr_info.address;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let nonce = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut n);
        format!("0x{}", hex::encode(n))
    };
    let message = json!({
        "chainIndex": chain_index,
        "from": from,
        "to": pay_to,
        "value": amount,
        "validAfter": "0",
        "validBefore": (now + max_timeout_secs).to_string(),
        "nonce": nonce,
        "verifyingContract": asset,
    });

    let client = WalletApiClient::new()?;

    // 2. Get EIP-3009 unsigned hash
    let unsigned_hash_resp = client
        .post_authed("/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash", &access_token, &message)
        .await
        .map_err(format_api_error)
        .context("x402 gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing domainHash in gen-msg-hash response"))?;

    // 3. Sign msgHash locally with Ed25519 session key
    let blob = keyring_store::read_blob()?;
    let session_key = blob
        .get("session_key")
        .ok_or_else(|| anyhow!("session_key not found in keyring. Please login again"))?;
    let encrypted_session_sk = blob
        .get("encrypted_session_sk")
        .ok_or_else(|| anyhow!("encrypted_session_sk not found in keyring. Please login again"))?;
    let session_cert = blob
        .get("session_cert")
        .ok_or_else(|| anyhow!("session_cert not found in keyring. Please login again"))?
        .clone();

    let signing_seed = crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    let session_signature_b64 = B64.encode(&session_signature);

    // 4. Get EIP-3009 signature
    let signed_hash_body = json!({
        "chainIndex": chain_index,
        "from": from,
        "to": pay_to,
        "value": amount,
        "validAfter": "0",
        "validBefore": (now + max_timeout_secs).to_string(),
        "nonce": nonce,
        "verifyingContract": asset,
        "domainHash": domain_hash,
        "sessionCert": session_cert,
        "sessionSignature": session_signature_b64,
    });

    let signed_hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &signed_hash_body,
        )
        .await
        .map_err(format_api_error)
        .context("x402 get-signed-hash failed")?;
    let eip3009_signature = signed_hash_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing signature in get-signed-hash response"))?;

    output::success(json!({
        "signature": eip3009_signature,
        "authorization": signed_hash_body,
    }));
    Ok(())
}

/// Extract numeric chain ID from a CAIP-2 "eip155:<chainId>" identifier.
fn parse_eip155_chain_id(network: &str) -> Option<u64> {
    network.strip_prefix("eip155:")?.parse().ok()
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── parse_eip155_chain_id ─────────────────────────────────────────

    #[test]
    fn parse_eip155_base() {
        assert_eq!(parse_eip155_chain_id("eip155:8453"), Some(8453));
    }

    #[test]
    fn parse_eip155_ethereum() {
        assert_eq!(parse_eip155_chain_id("eip155:1"), Some(1));
    }

    #[test]
    fn parse_eip155_xlayer() {
        assert_eq!(parse_eip155_chain_id("eip155:196"), Some(196));
    }

    #[test]
    fn parse_eip155_missing_prefix() {
        assert_eq!(parse_eip155_chain_id("8453"), None);
    }

    #[test]
    fn parse_eip155_wrong_prefix() {
        assert_eq!(parse_eip155_chain_id("solana:101"), None);
    }

    #[test]
    fn parse_eip155_empty() {
        assert_eq!(parse_eip155_chain_id(""), None);
    }

    #[test]
    fn parse_eip155_non_numeric() {
        assert_eq!(parse_eip155_chain_id("eip155:abc"), None);
    }

    #[test]
    fn parse_eip155_negative() {
        assert_eq!(parse_eip155_chain_id("eip155:-1"), None);
    }

    // ── CLI argument parsing ──────────────────────────────────────────

    /// Wrapper so clap can parse PaymentCommand as a top-level subcommand.
    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: PaymentCommand,
    }

    #[test]
    fn cli_x402_pay_all_args() {
        let cli = TestCli::parse_from([
            "test",
            "x402-pay",
            "--network", "eip155:8453",
            "--amount", "1000000",
            "--pay-to", "0xRecipient",
            "--asset", "0xUSDC",
            "--from", "0xPayer",
            "--max-timeout-seconds", "600",
        ]);
        match cli.command {
            PaymentCommand::X402Pay {
                network,
                amount,
                pay_to,
                asset,
                from,
                max_timeout_seconds,
            } => {
                assert_eq!(network, "eip155:8453");
                assert_eq!(amount, "1000000");
                assert_eq!(pay_to, "0xRecipient");
                assert_eq!(asset, "0xUSDC");
                assert_eq!(from.as_deref(), Some("0xPayer"));
                assert_eq!(max_timeout_seconds, 600);
            }
        }
    }

    #[test]
    fn cli_x402_pay_defaults() {
        let cli = TestCli::parse_from([
            "test",
            "x402-pay",
            "--network", "eip155:1",
            "--amount", "500",
            "--pay-to", "0xRecipient",
            "--asset", "0xToken",
        ]);
        match cli.command {
            PaymentCommand::X402Pay {
                from,
                max_timeout_seconds,
                ..
            } => {
                assert_eq!(from, None);
                assert_eq!(max_timeout_seconds, 300);
            }
        }
    }

    #[test]
    fn cli_x402_pay_missing_required() {
        // --asset is missing, should fail
        let result = TestCli::try_parse_from([
            "test",
            "x402-pay",
            "--network", "eip155:8453",
            "--amount", "1000000",
            "--pay-to", "0xRecipient",
        ]);
        assert!(result.is_err());
    }
}

