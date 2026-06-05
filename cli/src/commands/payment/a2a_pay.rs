//! A2A Pay CLI: bridges Buyer & Seller skills to the Smart-Account payment backend.
//!
//! Reference: design doc "A2A Payment 协议设计 (Smart-Account 分发方案) v0.2"
//!
//! Two-sided flow (sub-commands):
//!  - `create` (Seller): POST /payment/create — Seller defines amount / symbol /
//!    recipient and gets back `paymentId` + `challenge` to hand to the Buyer. No
//!    buyer wallet / signing involved.
//!  - `pay` (Buyer, `charge` intent only): GET /p/{id} → reconstruct EIP-3009
//!    authorization from the `challenge.data.request`; TEE-sign; POST
//!    /p/{id}/credential.
//!  - `status`: GET /p/{id}/status — poll on-chain execution state.
//!
//! For `escrow` intent the buyer signs offline via `sign_escrow` — caller
//! supplies the escrow auth fields directly; no payment-server round-trip.

use alloy_primitives::{keccak256, Address, FixedBytes, U256};
use alloy_sol_types::{sol, SolValue};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::{json, Value};
use zeroize::Zeroize;

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN;
use crate::wallet_api::WalletApiClient;
use crate::{keyring_store, wallet_store};

// ── Constants ────────────────────────────────────────────────────────────
const DEFAULT_VALID_BEFORE_SEC: u64 = 3600;

// ── Clap arg structs ─────────────────────────────────────────────────────

/// Seller-side `create` args. Buyer wallet / signing is NOT performed here.
#[derive(clap::Args)]
pub struct CreateArgs {
    /// Payment type: `charge` (direct transfer).
    #[arg(long = "type")]
    pub r#type: String,
    /// Decimal amount of tokens (e.g. "50" or "0.01" USDT).
    #[arg(long)]
    pub amount: String,
    /// ERC-20 token symbol (e.g. "USDT")
    #[arg(long)]
    pub symbol: String,
    /// Seller wallet address (= EIP-3009 `to`).
    #[arg(long)]
    pub recipient: Option<String>,
    /// Human-readable description shown to the Buyer. Optional.
    #[arg(long)]
    pub description: Option<String>,
    /// Realm — Seller / provider domain (e.g. "provider.example.com"). Optional.
    #[arg(long)]
    pub realm: Option<String>,
    /// Optional external business id (e.g. task id).
    #[arg(long = "external-id")]
    pub external_id: Option<String>,
    /// Payment-link expiration window in seconds. Default 1800 (30 min).
    #[arg(long = "expires-in")]
    pub expires_in: Option<u64>,
}

/// Buyer-side `pay` args. Picks up everything else from the on-server challenge.
#[derive(clap::Args)]
pub struct PayArgs {
    #[arg(long = "payment-id")]
    pub payment_id: String,
    /// Expected amount.
    #[arg(long)]
    pub amount: String,
    /// Expected ERC-20 token contract address.
    #[arg(long)]
    pub currency: String,
    /// Expected recipient address
    #[arg(long = "recipient-address")]
    pub recipient_address: String,
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
        A2aPayCommand::Create(args) => match args.r#type.as_str() {
            "charge" => {
                let params = ChargeParams::try_from(*args)?;
                let out = create_payment_charge(params).await?;
                println!("{}", serde_json::to_string_pretty(&out)?);
                Ok(())
            }
            other => bail!("unknown --type '{other}', expected 'charge'"),
        },
        A2aPayCommand::Pay(args) => {
            let params = PayParams {
                payment_id: args.payment_id,
                amount: args.amount,
                currency: args.currency,
                recipient_address: args.recipient_address,
            };
            let out = pay(params).await?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        A2aPayCommand::Status { payment_id } => {
            let out = status(payment_id).await?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
    }
}

// ── Param structs (1:1 with /payment/create request shape) ────────────
// `deliveries` is injected into the wire body inside `create_payment_*`
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

impl TryFrom<CreateArgs> for ChargeParams {
    type Error = anyhow::Error;
    fn try_from(a: CreateArgs) -> Result<Self> {
        Ok(Self {
            amount: a.amount,
            symbol: a.symbol,
            recipient: a
                .recipient
                .ok_or_else(|| anyhow!("--recipient is required for --type charge"))?,
            description: a.description,
            external_id: a.external_id,
            expires_in: a.expires_in,
            realm: a.realm,
        })
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
    require_evm_address(&params.recipient, "recipient")?;

    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;
    let mut value = serde_json::to_value(&params).context("serialize charge params")?;
    value["type"] = json!("charge");
    value["deliveries"] = json!({ "includeUrl": true });
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][a2a-pay] POST /payment/create (charge) body={}",
            value
        );
    }
    let resp: Value = wallet_client
        .post_authed("/api/v6/pay/a2a/payment/create", &access_token, &value)
        .await
        .map_err(format_api_error)
        .context("Smart-Account /payment/create failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] /payment/create response={resp}");
    }
    parse_create_payment_response(resp)
}

// ── Escrow authorization (used by buyer pay for `escrow` intent) ────────

// 15-field tuple matching the on-chain Escrow nonce derivation:
//   keccak256(abi.encode(from, provider, receiver, arbitrator, token, amount,
//     submitWindow, disputeWindow, arbitrationWindow, terminationWindow,
//     hook, keccak256(hookData), salt, chainId, escrowAddress))
// Order and types are load-bearing; `hookData` is pre-hashed so the tuple
// is all fixed-size and `abi_encode_params` matches Solidity `abi.encode`.
sol! {
    #[derive(Debug)]
    struct EscrowAuthFields {
        address from;
        address provider;
        address receiver;
        address arbitrator;
        address currency;
        uint256 amount;
        uint64 submitWindow;
        uint64 disputeWindow;
        uint64 arbitrationWindow;
        uint64 terminationWindow;
        address hook;
        bytes32 hookDataHash;
        bytes32 salt;
        uint256 chainId;
        address escrowAddress;
    }
}

/// Compute the EIP-3009 nonce for an Escrow authorization.
pub fn compute_escrow_nonce(fields: &EscrowAuthFields) -> FixedBytes<32> {
    let encoded = SolValue::abi_encode_params(fields);
    keccak256(&encoded)
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
    /// Expected amount in minimal units (e.g. "10000" for 0.01 USDT)
    pub amount: String,
    /// Expected ERC-20 token contract address
    pub currency: String,
    /// Expected recipient address
    pub recipient_address: String,
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

/// Buyer side (charge intent only):
/// 1. GET /p/{id} → reconstruct authorization params from `challenge.data.request`.
/// 2. Resolve Buyer agentic-wallet address on the chain named in `methodDetails.chainId`.
/// 3. Pick `validAfter` / `validBefore` defaults.
/// 4. Random 32-byte nonce; TEE-sign EIP-3009 (gen-msg-hash → ed25519 → sign-msg).
/// 5. POST /p/{id}/credential.
///
/// For `escrow` intent use `sign_escrow` — no payment-server round-trip.
pub async fn pay(p: PayParams) -> Result<PayOutput> {
    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;

    // ── 1. GET /p/{id} (public buyer link, no auth) ──────────────────
    let payment_path = format!("/api/v6/pay/a2a/p/{}", p.payment_id);
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] GET {payment_path}");
    }
    let resp: Value = wallet_client
        .get_public(&payment_path, &[])
        .await
        .context("Smart-Account GET /p/{id} failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] GET /p/{} response={resp}", p.payment_id);
    }
    if let Some(err_msg) = resp.get("errorMessage").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        let status = resp["status"].as_str().unwrap_or("unknown");
        bail!("payment {} 不可用 (status={status}): {err_msg}", p.payment_id);
    }
    let challenge = resp
        .get("challenge")
        .cloned()
        .or_else(|| resp.get("type").is_some().then(|| resp.clone()))
        .ok_or_else(|| anyhow!("GET /payment/{} response missing 'challenge'", p.payment_id))?;
    let data = challenge
        .get("data")
        .ok_or_else(|| anyhow!("challenge.data missing"))?;
    let intent = data
        .get("intent")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("challenge.data.intent missing"))?
        .to_string();
    if intent != "charge" {
        bail!(
            "pay() supports only 'charge' intent; got '{intent}' — use sign_escrow() for escrow"
        );
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

    // Pre-sign safety check: caller's declared amount / symbol / recipient must match
    // the on-server challenge byte-for-byte (recipient is case-insensitive). Bail
    // before signing if the seller's challenge disagrees with what the buyer thinks
    // they're paying for.
    if p.amount != amount {
        bail!(
            "amount mismatch: expected {}, challenge has {amount}",
            p.amount
        );
    }
    if !p.currency.eq_ignore_ascii_case(&currency) {
        bail!(
            "currency mismatch: expected {}, challenge has {currency}",
            p.currency
        );
    }
    if !p.recipient_address.eq_ignore_ascii_case(&recipient) {
        bail!(
            "recipient address mismatch: expected {}, challenge has {recipient}",
            p.recipient_address
        );
    }

    // ── 2. Resolve buyer wallet on target chain ──────────────────────
    let (chain_index, from_addr_str) = resolve_buyer_wallet(chain_id).await?;

    // ── 3. Pick timing ───────────────────────────────────────────────
    let now = unix_now()?;
    let valid_after = 0u64;
    let valid_before = now
        .checked_add(DEFAULT_VALID_BEFORE_SEC)
        .ok_or_else(|| anyhow!("validBefore overflow"))?;

    // ── 4. Random 32-byte nonce + TEE sign EIP-3009 ──────────────────
    use rand::RngCore;
    let mut nonce_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce_hex = format!("0x{}", hex::encode(nonce_bytes));

    let signature_hex = tee_sign_eip3009(
        &mut wallet_client,
        &access_token,
        &chain_index,
        &from_addr_str,
        &recipient,
        &amount,
        valid_after,
        valid_before,
        &nonce_hex,
        &currency,
        None,
        Some("eip3009Auth"),
    )
    .await?;

    // ── 5. POST /p/{id}/credential ─────────────────────────────
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
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] POST {credential_path} body={credential_body}");
    }
    let cred_resp: Value = wallet_client
        .post_authed(&credential_path, &access_token, &credential_body)
        .await
        .map_err(format_api_error)
        .context("Smart-Account /p/{id}/credential failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][a2a-pay] /p/{}/credential response={cred_resp}",
            p.payment_id
        );
    }

    // Server returns code=0 even when the credential is rejected by business
    // logic (e.g. insufficient balance) — the per-payment outcome lives in
    // `data.success`. Surface errorReason so the caller doesn't mistake a
    // refusal for a settling payment.
    if cred_resp.get("success").and_then(Value::as_bool) == Some(false) {
        let reason = cred_resp
            .get("errorReason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        bail!("payment {} rejected (reason={reason})", p.payment_id);
    }

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

// ── Buyer side: sign_escrow (offline TEE sign, no payment-server I/O) ───

#[derive(Debug)]
pub struct SignEscrowParams {
    pub chain_id: u64,
    /// Provider address (escrow auth field).
    pub provider: String,
    /// Receiver address (escrow auth field).
    pub receiver: String,
    /// Arbitrator address (escrow auth field).
    pub arbitrator: String,
    /// ERC-20 token contract — also EIP-3009 `verifyingContract`.
    pub currency: String,
    /// Escrow contract address — also EIP-3009 `to` (where funds land).
    pub escrow_contract: String,
    /// Amount in minimal units (e.g. "10000" for 0.01 USDT @ 6 decimals).
    pub amount: String,
    pub submit_window: u64,
    pub dispute_window: u64,
    pub arbitration_window: u64,
    pub termination_window: u64,
    pub hook: String,
    /// Hex (with or without `0x` prefix).
    pub hook_data: String,
    /// 32-byte salt (`0x`-prefixed hex).
    pub salt: String,
    /// Escrow expiry as RFC 3339 (e.g. `"2026-05-01T00:00:00Z"`). Parsed to
    /// unix seconds and used as the EIP-3009 `validBefore`.
    pub expired_at: String,
}

/// Mirrors the inner `payload` of `pay()`'s `credential_body` — wraps the EIP-3009
/// signed authorization in the standard `{type, signature, authorization}`
/// envelope. Callers that need the full `{"payload": ...}` wire shape wrap one
/// more level themselves.
#[derive(Debug, serde::Serialize)]
pub struct SignEscrowOutput {
    pub r#type: String,
    pub signature: String,
    pub authorization: EscrowAuthorization,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EscrowAuthorization {
    pub r#type: String,
    pub from: String,
    pub to: String,
    pub value: String,
    /// Wire format is a decimal string (matches `pay()`'s credential body).
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
}

/// Buyer side (escrow intent): compute escrow nonce locally from caller-supplied
/// fields, TEE-sign EIP-3009 `ReceiveWithAuthorization`, and return the signed
/// authorization. No payment-server I/O — caller delivers the signature on-chain
/// or to the seller.
pub async fn sign_escrow(p: SignEscrowParams) -> Result<SignEscrowOutput> {
    for (label, v) in [
        ("provider", p.provider.as_str()),
        ("receiver", p.receiver.as_str()),
        ("arbitrator", p.arbitrator.as_str()),
        ("currency", p.currency.as_str()),
        ("escrow_contract", p.escrow_contract.as_str()),
        ("hook", p.hook.as_str()),
    ] {
        require_evm_address(v, label)?;
    }

    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;

    let (chain_index, from_addr_str) = resolve_buyer_wallet(p.chain_id).await?;

    let valid_after = 0u64;
    let valid_before: u64 = chrono::DateTime::parse_from_rfc3339(&p.expired_at)
        .with_context(|| format!("expired_at '{}' is not RFC 3339", p.expired_at))?
        .timestamp()
        .try_into()
        .context("expired_at predates unix epoch")?;

    let amount_u128: u128 = p
        .amount
        .parse()
        .context("amount must be a non-negative integer in minimal units")?;
    let from_addr: Address = from_addr_str
        .parse()
        .context("agentic-wallet address is not a valid EVM address")?;
    let hook_data_bytes = hex::decode(p.hook_data.trim_start_matches("0x"))
        .context("hook_data is not valid hex")?;
    let salt = parse_bytes32_hex(&p.salt, "salt")?;

    let escrow_addr: Address = p
        .escrow_contract
        .parse()
        .context("escrow_contract parse")?;
    let fields = EscrowAuthFields {
        from: from_addr,
        provider: p.hook.parse().context("provider parse")?,  // TODO: use provider or hook?
        receiver: p.receiver.parse().context("receiver parse")?,
        arbitrator: p.arbitrator.parse().context("arbitrator parse")?,
        currency: p.currency.parse().context("currency parse")?,
        amount: U256::from(amount_u128),
        submitWindow: p.submit_window,
        disputeWindow: p.dispute_window,
        arbitrationWindow: p.arbitration_window,
        terminationWindow: p.termination_window,
        hook: p.hook.parse().context("hook parse")?,
        hookDataHash: keccak256(&hook_data_bytes),
        salt,
        chainId: U256::from(p.chain_id),
        escrowAddress: escrow_addr,
    };
    let nonce_hex = format!("0x{}", hex::encode(compute_escrow_nonce(&fields)));

    let signature_hex = tee_sign_eip3009(
        &mut wallet_client,
        &access_token,
        &chain_index,
        &from_addr_str,
        &p.escrow_contract,
        &p.amount,
        valid_after,
        valid_before,
        &nonce_hex,
        &p.currency,
        Some("eip3009ReceiveAuth"),
        Some("eip3009ReceiveAuth"),
    )
    .await?;

    Ok(SignEscrowOutput {
        r#type: "transaction".to_string(),
        signature: signature_hex,
        authorization: EscrowAuthorization {
            r#type: "ReceiveWithAuthorization".to_string(),
            from: from_addr_str,
            to: p.escrow_contract,
            value: p.amount,
            valid_after: valid_after.to_string(),
            valid_before: valid_before.to_string(),
            nonce: nonce_hex,
        },
    })
}

// ── TEE-sign EIP-3009 helper (shared by pay and sign_escrow) ────────────

#[allow(clippy::too_many_arguments)]
async fn tee_sign_eip3009(
    wallet_client: &mut WalletApiClient,
    access_token: &str,
    chain_index: &str,
    from: &str,
    to: &str,
    value: &str,
    valid_after: u64,
    valid_before: u64,
    nonce_hex: &str,
    verifying_contract: &str,
    sign_type: Option<&str>,
    msg_type: Option<&str>,
) -> Result<String> {
    let session =
        wallet_store::load_session()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let session_key =
        keyring_store::get("session_key").map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    let mut base_fields = json!({
        "chainIndex": chain_index,
        "from": from,
        "to": to,
        "value": value,
        "validAfter": valid_after.to_string(),
        "validBefore": valid_before.to_string(),
        "nonce": nonce_hex,
        "verifyingContract": verifying_contract,
    });
    if let Some(t) = sign_type {
        base_fields["signType"] = json!(t);
    }

    let mut gen_msg_hash_body = base_fields.clone();
    if let Some(t) = msg_type {
        gen_msg_hash_body["msgType"] = json!(t);
    }
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] POST gen-msg-hash body={gen_msg_hash_body}");
    }
    let unsigned_hash_resp: Value = wallet_client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            access_token,
            &gen_msg_hash_body,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay: gen-msg-hash failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] gen-msg-hash response={unsigned_hash_resp}");
    }
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

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] POST sign-msg body={sign_body}");
    }
    let signed_resp: Value = wallet_client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay: sign-msg failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] sign-msg response={signed_resp}");
    }
    Ok(signed_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'signature' in sign-msg response"))?
        .to_string())
}

/// Resolve buyer's agentic-wallet address on `chain_id`, returning
/// `(chainIndex, address)` ready to plug into EIP-3009 signing.
async fn resolve_buyer_wallet(chain_id: u64) -> Result<(String, String)> {
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
    let wallets =
        wallet_store::load_wallets()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, None, chain_name)?;
    Ok((chain_index, addr_info.address))
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
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] GET {path}");
    }
    let resp: Value = wallet_client
        .get_authed(&path, &access_token, &[])
        .await
        .map_err(format_api_error)
        .context("Smart-Account /p/{id}/status failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] /p/{payment_id}/status response={resp}");
    }
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

fn is_valid_evm_address(addr: &str) -> bool {
    addr.starts_with("0x") && addr.len() == 42 && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn require_evm_address(addr: &str, label: &str) -> Result<()> {
    if is_valid_evm_address(addr) {
        Ok(())
    } else {
        bail!("--{label} is not a valid EVM address: {addr}")
    }
}

fn parse_bytes32_hex(s: &str, label: &str) -> Result<FixedBytes<32>> {
    let clean = s.strip_prefix("0x").unwrap_or(s);
    if clean.len() != 64 {
        bail!(
            "{label} must be 32 bytes (64 hex chars), got {}",
            clean.len()
        );
    }
    let bytes = hex::decode(clean).with_context(|| format!("{label} is not valid hex"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("{label} length mismatch"))?;
    Ok(FixedBytes::from(arr))
}

fn unix_now() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_evm_address() {
        assert!(is_valid_evm_address(
            "0x1d4eAbb31AfEd5Aa70E1cCEEf73DEbF4dB164aB7"
        ));
        assert!(!is_valid_evm_address("0x123"));
        assert!(!is_valid_evm_address(
            "1d4eAbb31AfEd5Aa70E1cCEEf73DEbF4dB164aB7"
        ));
        assert!(!is_valid_evm_address(
            "0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"
        ));
    }

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

    #[test]
    fn parses_bytes32_hex() {
        let s = "0x0000000000000000000000000000000000000000000000000000000000000001";
        let b = parse_bytes32_hex(s, "test").unwrap();
        assert_eq!(b.as_slice()[31], 1);
        let b2 = parse_bytes32_hex(&s[2..], "test").unwrap();
        assert_eq!(b, b2);
        assert!(parse_bytes32_hex("0x01", "test").is_err());
    }

    /// Lock in escrow nonce determinism for fixed input — guards against
    /// silent ABI-encoding regressions.
    #[test]
    fn escrow_nonce_is_deterministic_and_field_sensitive() {
        let f = EscrowAuthFields {
            from: "0x6666666666666666666666666666666666666666"
                .parse()
                .unwrap(),
            provider: "0x1111111111111111111111111111111111111111"
                .parse()
                .unwrap(),
            receiver: "0x2222222222222222222222222222222222222222"
                .parse()
                .unwrap(),
            arbitrator: "0x3333333333333333333333333333333333333333"
                .parse()
                .unwrap(),
            currency: "0x4444444444444444444444444444444444444444"
                .parse()
                .unwrap(),
            amount: U256::from(50_000_000u64),
            submitWindow: 86400,
            disputeWindow: 86400,
            arbitrationWindow: 172800,
            terminationWindow: 86400,
            hook: "0x5555555555555555555555555555555555555555"
                .parse()
                .unwrap(),
            hookDataHash: keccak256([0xde, 0xad, 0xbe, 0xef]),
            salt: parse_bytes32_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000007",
                "salt",
            )
            .unwrap(),
            chainId: U256::from(196u64),
            escrowAddress: "0x7777777777777777777777777777777777777777"
                .parse()
                .unwrap(),
        };
        let n1 = compute_escrow_nonce(&f);
        let n2 = compute_escrow_nonce(&f);
        assert_eq!(n1, n2, "nonce must be deterministic");

        let mut f2 = f.clone();
        f2.amount = U256::from(50_000_001u64);
        assert_ne!(n1, compute_escrow_nonce(&f2), "nonce must depend on amount");

        let mut f3 = f.clone();
        f3.salt = parse_bytes32_hex(
            "0x0000000000000000000000000000000000000000000000000000000000000008",
            "salt",
        )
        .unwrap();
        assert_ne!(n1, compute_escrow_nonce(&f3), "nonce must depend on salt");
    }
}
