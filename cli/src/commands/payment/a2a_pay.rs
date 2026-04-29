//! A2A Pay CLI: bridges Buyer & Seller skills to the a2a-pay backend.
//!
//! Two-sided flow (sub-commands):
//!  - `create` (Seller): POST /payment/create — Seller defines amount / symbol /
//!    recipient and gets back `paymentId` + `challenge` to hand to the Buyer.
//!    No buyer wallet / signing involved.
//!  - `pay` (Buyer): GET /p/{id} → reconstruct EIP-3009 authorization from the
//!    `challenge.data.request`; TEE-sign; POST /p/{id}/credential.
//!  - `status`: GET /p/{id}/status — poll on-chain execution state.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::agentic_wallet::common::{require_evm_address, ERR_NOT_LOGGED_IN};
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::{keyring_store, wallet_store};

// ── Clap arg structs ─────────────────────────────────────────────────────

/// Seller-side `create` args. Buyer wallet / signing is NOT performed here.
#[derive(clap::Args)]
pub struct CreateArgs {
    /// Decimal amount of tokens (e.g. "50" or "0.01" USDT).
    #[arg(long)]
    pub amount: String,
    /// ERC-20 token symbol (e.g. "USDT")
    #[arg(long)]
    pub symbol: String,
    /// Seller wallet address (= EIP-3009 `to`).
    #[arg(long)]
    pub recipient: String,
    /// Human-readable description shown to the Buyer. Optional.
    #[arg(long)]
    pub description: Option<String>,
    /// Realm — Seller / provider domain (e.g. "provider.example.com"). Optional.
    #[arg(long)]
    pub realm: Option<String>,
    /// Payment-link expiration window in seconds. Default 1800 (30 min).
    #[arg(long = "expires-in")]
    pub expires_in: Option<u64>,
}

/// Buyer-side `pay` args. Everything else is taken from the on-server challenge.
#[derive(clap::Args)]
pub struct PayArgs {
    #[arg(long = "payment-id")]
    pub payment_id: String,
}

#[derive(Subcommand)]
pub enum A2aPayCommand {
    /// Seller: create a payment authorization, returns paymentId + challenge.
    Create(Box<CreateArgs>),
    /// Buyer: fetch challenge by id, sign EIP-3009, submit credential.
    Pay(PayArgs),
    /// Query payment status by id.
    Status {
        #[arg(long = "payment-id")]
        payment_id: String,
    },
}

pub async fn execute(cmd: A2aPayCommand) -> Result<()> {
    match cmd {
        A2aPayCommand::Create(args) => {
            let params = ChargeParams::from(*args);
            let out = create_payment_charge(params).await?;
            output::success(out);
            Ok(())
        }
        A2aPayCommand::Pay(args) => {
            let params = PayParams {
                payment_id: args.payment_id,
            };
            let out = pay(params).await?;
            output::success(out);
            Ok(())
        }
        A2aPayCommand::Status { payment_id } => {
            let out = status(payment_id).await?;
            output::success(out);
            Ok(())
        }
    }
}

// ── Param structs (1:1 with /payment/create request shape) ────────────
// `deliveries` is injected into the wire body inside `create_payment_charge`
// (always `{includeUrl: true}` for now) and intentionally not exposed here.

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChargeParams {
    pub amount: String,
    pub symbol: String,
    pub recipient: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
}

impl From<CreateArgs> for ChargeParams {
    fn from(a: CreateArgs) -> Self {
        Self {
            amount: a.amount,
            symbol: a.symbol,
            recipient: a.recipient,
            description: a.description,
            external_id: None,
            expires_in: a.expires_in,
            realm: a.realm,
        }
    }
}

#[derive(serde::Serialize)]
pub struct CreatePaymentOutput {
    pub payment_id: String,
    pub deliveries: Option<Value>,
}

// ── Seller side: charge create (§4) ─────────────────────────────────────

/// Seller side: POST /payment/create — produces `paymentId` + `challenge` for
/// the Buyer to consume. No buyer wallet / TEE signing here.
pub async fn create_payment_charge(params: ChargeParams) -> Result<CreatePaymentOutput> {
    validate_positive_decimal_amount(&params.amount)?;
    require_evm_address(&params.recipient, "--recipient")?;

    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;
    let mut value = serde_json::to_value(&params).context("serialize charge params")?;
    value["type"] = json!("charge");
    value["deliveries"] = json!({ "includeUrl": true });
    let resp: Value = wallet_client
        .post_authed("/api/v6/pay/a2a/payment/create", &access_token, &value)
        .await
        .map_err(format_api_error)
        .context("a2a-pay POST /payment/create failed")?;
    parse_create_payment_response(resp)
}

fn parse_create_payment_response(resp: Value) -> Result<CreatePaymentOutput> {
    let payment_id = resp["paymentId"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'paymentId' in /payment/create response"))?
        .to_string();
    Ok(CreatePaymentOutput {
        payment_id,
        deliveries: resp.get("deliveries").cloned(),
    })
}

// ── Buyer side: pay (GET challenge → sign → POST credential) ────────────

pub struct PayParams {
    pub payment_id: String,
}

#[derive(serde::Serialize)]
pub struct PayOutput {
    pub payment_id: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub valid_after: u64,
    pub valid_before: u64,
    pub signature: String,
}

/// Buyer side:
/// 1. GET /p/{id} → reconstruct authorization params from `challenge.data.request`.
/// 2. Resolve Buyer agentic-wallet address on the chain named in `methodDetails.chainId`.
/// 3. Pick `validAfter` / `validBefore` (CLI override or defaults).
/// 4. Compute EIP-3009 nonce as random 32 bytes (charge intent).
/// 5. TEE-sign EIP-3009 (gen-msg-hash → ed25519 sign session → sign-msg).
/// 6. POST /p/{id}/credential.
#[allow(clippy::too_many_lines)]
pub async fn pay(p: PayParams) -> Result<PayOutput> {
    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;

    // ── 1. GET /p/{id} (public buyer link, no auth) ──────────────────
    let payment_path = format!("/api/v6/pay/a2a/p/{}", p.payment_id);
    let resp: Value = wallet_client
        .get_public(&payment_path, &[])
        .await
        .with_context(|| format!("a2a-pay GET /p/{} failed", p.payment_id))?;
    if resp.get("available").and_then(Value::as_bool) == Some(false) {
        let reason = resp
            .get("errorReason")
            .and_then(Value::as_str)
            .unwrap_or("unavailable");
        let detail = resp
            .get("errorMessage")
            .and_then(Value::as_str)
            .unwrap_or("");
        let status = resp
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        bail!("payment {} not payable (status={status}, reason={reason}): {detail}", p.payment_id);
    }
    let challenge = resp
        .get("challenge")
        .cloned()
        .ok_or_else(|| anyhow!("GET /p/{} response missing 'challenge'", p.payment_id))?;
    let data = challenge
        .get("data")
        .ok_or_else(|| anyhow!("challenge.data missing"))?;
    let intent = data
        .get("intent")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenge.data.intent missing"))?
        .to_string();
    if intent != "charge" {
        bail!("unsupported challenge intent '{intent}', only 'charge' is supported");
    }
    let expires_str = data
        .get("expires")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenge.data.expires missing"))?;
    let expires_at = chrono::DateTime::parse_from_rfc3339(expires_str)
        .with_context(|| format!("challenge.data.expires '{expires_str}' is not RFC3339"))?
        .with_timezone(&chrono::Utc);
    if expires_at <= chrono::Utc::now() {
        bail!("challenge expired at {expires_str}");
    }
    let request = data
        .get("request")
        .ok_or_else(|| anyhow!("challenge.data.request missing"))?;
    let amount = request
        .get("amount")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenge.data.request.amount missing"))?
        .to_string();
    let currency = request
        .get("currency")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenge.data.request.currency missing"))?
        .to_string();
    let recipient = request
        .get("recipient")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenge.data.request.recipient missing"))?
        .to_string();
    // Reject malformed server-side challenge fields up front so a bad
    // address doesn't surface as an opaque TEE error after gen-msg-hash.
    require_evm_address(&currency, "challenge.request.currency")?;
    require_evm_address(&recipient, "challenge.request.recipient")?;
    let method_details = request
        .get("methodDetails")
        .ok_or_else(|| anyhow!("challenge.data.request.methodDetails missing"))?;
    let chain_id = method_details
        .get("chainId")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("methodDetails.chainId missing"))?;
    let authorization_scheme = method_details
        .get("authorizationType")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("methodDetails.authorizationType missing"))?
        .to_string();

    // ── 2. Resolve buyer wallet on target chain ──────────────────────
    let chain_entry = crate::commands::agentic_wallet::chain::get_chain_by_real_chain_index(
        &chain_id.to_string(),
    )
    .await?
    .ok_or_else(|| anyhow!("chain (chainId={chain_id}) not found in chain registry"))?;
    let chain_index = chain_entry["chainIndex"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| chain_entry["chainIndex"].as_u64().map(|n| n.to_string()))
        .ok_or_else(|| anyhow!("missing chainIndex in chain entry"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName in chain entry"))?;
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) = crate::commands::agentic_wallet::transfer::resolve_address(
        &wallets, None, chain_name,
    )?;
    let from_addr_str = addr_info.address.clone();

    // ── 3. Pick timing ───────────────────────────────────────────────
    // Bind the EIP-3009 window to the seller's challenge expiry so the
    // signed authorization can't outlive what the seller advertised.
    let valid_after = 0u64;
    let valid_before: u64 = expires_at
        .timestamp()
        .try_into()
        .map_err(|_| anyhow!("challenge.data.expires precedes UNIX epoch"))?;

    // ── 4. Compute nonce (random 32 bytes for charge) ────────────────
    let nonce_hex = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        format!("0x{}", hex::encode(n))
    };

    // ── 5. TEE sign EIP-3009 ─────────────────────────────────────────
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    let base_fields = json!({
        "chainIndex": chain_index,
        "from": from_addr_str,
        "to": recipient,
        "value": amount,
        "validAfter": valid_after.to_string(),
        "validBefore": valid_before.to_string(),
        "nonce": nonce_hex,
        "verifyingContract": currency,
    });

    let unsigned_hash_resp: Value = wallet_client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay: gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'msgHash' in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'domainHash' in gen-msg-hash response"))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    let mut sign_body = base_fields.clone();
    sign_body["domainHash"] = json!(domain_hash);
    sign_body["sessionCert"] = json!(session.session_cert);
    sign_body["sessionSignature"] = json!(session_signature_b64);

    let signed_resp: Value = wallet_client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay: sign-msg failed")?;
    let signature_hex = signed_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'signature' in sign-msg response"))?
        .to_string();

    // ── 6. POST /p/{id}/credential ─────────────────────────────
    let credential_body = json!({
        "payload": {
            "type": "transaction",
            "signature": signature_hex,
            "authorization": {
                "type": authorization_scheme,
                "from": from_addr_str,
                "to": recipient,
                "value": amount,
                "validAfter": valid_after.to_string(),
                "validBefore": valid_before.to_string(),
                "nonce": nonce_hex,
            },
        },
    });
    let credential_path = format!("/api/v6/pay/a2a/p/{}/credential", p.payment_id);
    let cred_resp: Value = wallet_client
        .post_authed(&credential_path, &access_token, &credential_body)
        .await
        .map_err(format_api_error)
        .with_context(|| format!("a2a-pay POST /p/{}/credential failed", p.payment_id))?;

    Ok(PayOutput {
        payment_id: p.payment_id,
        status: cred_resp["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        valid_after,
        valid_before,
        tx_hash: cred_resp["txHash"].as_str().map(|s| s.to_string()),
        signature: signature_hex,
    })
}

// ── Status query (§9.4) ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct StatusOutput {
    pub payment_id: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub block_number: Option<u64>,
    pub block_timestamp: Option<String>,
    pub fee_amount: Option<String>,
    pub fee_bps: Option<u64>,
}

/// GET /p/{id}/status — current state.
pub async fn status(payment_id: String) -> Result<StatusOutput> {
    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;
    let path = format!("/api/v6/pay/a2a/p/{}/status", payment_id);
    let resp: Value = wallet_client
        .get_authed(&path, &access_token, &[])
        .await
        .map_err(format_api_error)
        .with_context(|| format!("a2a-pay GET /p/{}/status failed", payment_id))?;
    Ok(StatusOutput {
        payment_id,
        status: resp["status"].as_str().unwrap_or("unknown").to_string(),
        tx_hash: resp["executed"]["txHash"].as_str().map(|s| s.to_string()),
        block_number: resp["executed"]["blockNumber"].as_u64(),
        block_timestamp: resp["executed"]["blockTimestamp"]
            .as_str()
            .map(|s| s.to_string()),
        fee_amount: resp["fee"]["amount"].as_str().map(|s| s.to_string()),
        fee_bps: resp["fee"]["bps"].as_u64(),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Validate that `s` is a positive decimal amount in whole tokens (e.g. "50",
/// "0.01", ".5"). Rejects empty / non-numeric / signed / scientific-notation /
/// zero values. The string is passed to the wire unchanged after validation —
/// minimal-unit conversion is the server's responsibility.
fn validate_positive_decimal_amount(s: &str) -> Result<()> {
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        bail!("amount must not be empty");
    }
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        bail!("amount must be a non-negative decimal number, got: {s}");
    }
    let nonzero = int_part.chars().any(|c| c != '0') || frac_part.chars().any(|c| c != '0');
    if !nonzero {
        bail!("amount must be greater than zero");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_positive_decimal_amount() {
        assert!(validate_positive_decimal_amount("50").is_ok());
        assert!(validate_positive_decimal_amount("0.01").is_ok());
        assert!(validate_positive_decimal_amount("10.5").is_ok());
        assert!(validate_positive_decimal_amount(".5").is_ok());
        assert!(validate_positive_decimal_amount("1.").is_ok());

        assert!(validate_positive_decimal_amount("").is_err());
        assert!(validate_positive_decimal_amount(".").is_err());
        assert!(validate_positive_decimal_amount("0").is_err());
        assert!(validate_positive_decimal_amount("0.0").is_err());
        assert!(validate_positive_decimal_amount("-1").is_err());
        assert!(validate_positive_decimal_amount("+1").is_err());
        assert!(validate_positive_decimal_amount("1e2").is_err());
        assert!(validate_positive_decimal_amount("1.2.3").is_err());
        assert!(validate_positive_decimal_amount(" 1").is_err());
        assert!(validate_positive_decimal_amount("abc").is_err());
    }
}
