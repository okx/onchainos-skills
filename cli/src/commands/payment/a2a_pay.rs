//! A2A Pay CLI: bridges Buyer Skills to the Smart-Account payment backend.
//!
//! Reference: design doc "A2A Payment 协议设计 (Smart-Account 分发方案) v0.2"
//!
//! Implemented (Phase A):
//!  - `--type charge`  (§4): EIP-3009 transferWithAuthorization, direct
//!    Buyer→Seller transfer.
//!  - `--type escrow`  (§5): EIP-3009 receiveWithAuthorization, funds locked
//!    in escrow contract; Phase 2 release lives on the escrow contract, not
//!    in this CLI.
//!  - `status`: GET /payment/{id}/status — poll payment state.
//!
//! Out of scope (Phase B+): Session (4 actions), Upto + report, cancel.

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

// ── Smart-Account API paths (per spec §9.1) ─────────────────────────────
const PATH_PAYMENT_CREATE: &str = "/api/v6/pay/a2a/payment/create";
fn path_credential(payment_id: &str) -> String {
    format!("/api/v6/pay/a2a/payment/{payment_id}/credential")
}
fn path_status(payment_id: &str) -> String {
    format!("/api/v6/pay/a2a/payment/{payment_id}/status")
}

// ── Constants ────────────────────────────────────────────────────────────
/// Default chain ID when caller doesn't specify (X Layer per spec v0.2).
const DEFAULT_CHAIN_ID: u64 = 196;
/// Default expiresIn for /payment/create (seconds).
const DEFAULT_EXPIRES_IN: u64 = 1800;
/// Default EIP-3009 authorization validBefore window (seconds beyond now).
const DEFAULT_VALID_BEFORE_SEC: u64 = 3600;

// ── Clap arg structs ─────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct CreateArgs {
    // ── Common ───────────────────────────────────────────────────────
    /// Payment type: `charge` (direct transfer) or `escrow` (lock funds).
    #[arg(long = "type")]
    pub r#type: String,
    /// Amount in token's minimal units (e.g. 50000000 = 50 USDC at 6 decimals).
    #[arg(long)]
    pub amount: String,

    /// ERC-20 token contract address (X Layer).
    #[arg(long)]
    pub currency: String,

    /// Human-readable description shown to the Buyer.
    #[arg(long)]
    pub description: String,

    /// Realm — Seller / provider domain (e.g. "provider.example.com").
    #[arg(long)]
    pub realm: String,

    /// Optional external business id (e.g. task id).
    #[arg(long = "external-id")]
    pub external_id: Option<String>,

    /// Payment-link expiration window in seconds. Default 1800 (30 min).
    #[arg(long = "expires-in")]
    pub expires_in: Option<u64>,

    /// EIP-712 domain name for the ERC-20 token (e.g. "USD Coin"). Required
    /// for local EIP-3009 signing.
    #[arg(long = "domain-name")]
    pub domain_name: String,

    /// EIP-712 domain version. Default "2" (matches USDC/USDT on most chains).
    #[arg(long = "domain-version", default_value = "2")]
    pub domain_version: String,

    /// EIP-155 chain ID. Default 196 (X Layer).
    #[arg(long = "chain-id")]
    pub chain_id: Option<u64>,

    /// EIP-3009 validBefore (unix seconds). Default: now + 3600.
    #[arg(long = "valid-before")]
    pub valid_before: Option<u64>,

    // ── Charge-only ──────────────────────────────────────────────────
    /// Seller wallet address (= EIP-3009 `to` for charge mode).
    #[arg(long)]
    pub recipient: Option<String>,

    // ── Escrow-only ──────────────────────────────────────────────────
    /// Escrow contract address (= EIP-3009 `to` for escrow mode).
    #[arg(long = "escrow-contract")]
    pub escrow_contract: Option<String>,
    /// Task executor (provider) address — recorded inside escrow.
    #[arg(long)]
    pub provider: Option<String>,
    /// Final receiver address (where funds settle to). Default: --provider.
    #[arg(long)]
    pub receiver: Option<String>,
    /// Arbitrator (dispute evaluator) address.
    #[arg(long)]
    pub arbitrator: Option<String>,
    /// Submit window seconds.
    #[arg(long = "submit-window")]
    pub submit_window: Option<u32>,
    /// Dispute window seconds.
    #[arg(long = "dispute-window")]
    pub dispute_window: Option<u32>,
    /// Arbitration window seconds.
    #[arg(long = "arbitration-window")]
    pub arbitration_window: Option<u32>,
    /// Termination window seconds.
    #[arg(long = "termination-window")]
    pub termination_window: Option<u32>,
    /// Business expiry timestamp (RFC 3339), e.g. "2026-05-01T00:00:00Z".
    #[arg(long = "expired-at")]
    pub expired_at: Option<String>,
    /// Settlement hook contract address.
    #[arg(long)]
    pub hook: Option<String>,
    /// Settlement hook calldata (hex, with or without 0x prefix).
    #[arg(long = "hook-data")]
    pub hook_data: Option<String>,
    /// Anti-replay salt (bytes32 hex). Default: random.
    #[arg(long)]
    pub salt: Option<String>,
    /// EIP-3009 validAfter (unix seconds). Default: 0.
    #[arg(long = "valid-after")]
    pub valid_after: Option<u64>,
}

#[derive(Subcommand)]
pub enum A2aPayCommand {
    /// Create a payment authorization.
    /// `--type charge` direct transfer; `--type escrow` lock funds in escrow.
    Create(Box<CreateArgs>),

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
                let out = create_charge(params).await?;
                println!("{}", serde_json::to_string_pretty(&out)?);
                Ok(())
            }
            "escrow" => {
                let params = EscrowParams::try_from(*args)?;
                let out = create_escrow(params).await?;
                println!("{}", serde_json::to_string_pretty(&out)?);
                Ok(())
            }
            other => bail!("unknown --type '{other}', expected 'charge' or 'escrow'"),
        },
        A2aPayCommand::Status { payment_id } => {
            let out = status(payment_id).await?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
    }
}

// ── Charge mode (§4) ────────────────────────────────────────────────────

/// Programmatic params for `create_charge` — cross-module callers use this
/// instead of the clap-decorated `CreateArgs`.
pub struct ChargeParams {
    /// Seller wallet — EIP-3009 `to`.
    pub recipient: String,
    /// Amount in token's minimal units (decimal string).
    pub amount: String,
    /// ERC-20 token contract address.
    pub currency: String,
    /// Human-readable description shown to the Buyer.
    pub description: String,
    /// Realm — Seller / provider domain.
    pub realm: String,
    /// Optional external business id (e.g. task id).
    pub external_id: Option<String>,
    /// Payment-link expiration window in seconds. Default 1800 (30 min).
    pub expires_in: Option<u64>,
    /// EIP-712 domain name (e.g. "USDT").
    pub domain_name: String,
    /// EIP-712 domain version. Default "2".
    pub domain_version: String,
    /// EIP-155 chain ID. None → 196 (X Layer).
    pub chain_id: Option<u64>,
    /// EIP-3009 validBefore (unix seconds). Default: now + 3600.
    pub valid_before: Option<u64>,
}

impl TryFrom<CreateArgs> for ChargeParams {
    type Error = anyhow::Error;
    fn try_from(a: CreateArgs) -> Result<Self> {
        Ok(Self {
            recipient: a
                .recipient
                .ok_or_else(|| anyhow!("--recipient is required for --type charge"))?,
            amount: a.amount,
            currency: a.currency,
            description: a.description,
            realm: a.realm,
            external_id: a.external_id,
            expires_in: a.expires_in,
            domain_name: a.domain_name,
            domain_version: a.domain_version,
            chain_id: a.chain_id,
            valid_before: a.valid_before,
        })
    }
}

#[derive(serde::Serialize)]
pub struct CreateChargeOutput {
    pub payment_id: String,
    pub status: String,
    pub tracking_url: Option<String>,
    pub tx_hash: Option<String>,
}

/// Phase 1 of Charge (TEE-signed):
/// 1. POST /payment/create → `{ paymentId, challenge, deliveries, ... }`
/// 2. Resolve Buyer's logged-in agentic-wallet address (X Layer / chainName=okb).
/// 3. TEE sign EIP-3009 `transferWithAuthorization`:
///    - POST `/pre-transaction/gen-msg-hash` → msgHash + domainHash
///    - Local: HPKE-decrypt session sk, ed25519-sign msgHash
///    - POST `/pre-transaction/sign-msg` → secp256k1 signature (TEE)
/// 4. POST /payment/{id}/credential — Smart-Account verifies + broadcasts.
#[allow(clippy::too_many_lines)]
pub async fn create_charge(p: ChargeParams) -> Result<CreateChargeOutput> {
    // ── 1. Validate inputs ────────────────────────────────────────────
    let parsed_amount: u128 = p
        .amount
        .parse()
        .context("amount must be a non-negative integer in minimal units")?;
    if parsed_amount == 0 {
        bail!("amount must be greater than zero");
    }
    require_evm_address(&p.recipient, "recipient")?;
    require_evm_address(&p.currency, "currency")?;

    // ── 2. POST /payment/create ──────────────────────────────────────
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;

    let create_body = json!({
        "type": "charge",
        "amount": p.amount,
        "currency": p.currency,
        "recipient": p.recipient,
        "description": p.description,
        "externalId": p.external_id,
        "expiresIn": p.expires_in.unwrap_or(DEFAULT_EXPIRES_IN),
        "realm": p.realm,
        "deliveries": json!({
            "includeUrl": true,
            "includeQrCode": true,
            "includeCard": true,
        }),
    });
    let create_resp: Value = client
        .post_authed(PATH_PAYMENT_CREATE, &access_token, &create_body)
        .await
        .context("Smart-Account /payment/create failed")?;

    let payment_id = create_resp["paymentId"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'paymentId' in /payment/create response"))?
        .to_string();
    let challenge = create_resp
        .get("challenge")
        .cloned()
        .ok_or_else(|| anyhow!("missing 'challenge' in /payment/create response"))?;

    // ── 3. Resolve Buyer wallet address on the target chain ─────────
    let chain_id = p.chain_id.unwrap_or(DEFAULT_CHAIN_ID);
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

    // ── 4. Compute timing + EIP-3009 nonce ───────────────────────────
    let now = unix_now()?;
    let valid_before = match p.valid_before {
        Some(v) => v,
        None => now
            .checked_add(DEFAULT_VALID_BEFORE_SEC)
            .ok_or_else(|| anyhow!("validBefore overflow"))?,
    };
    let nonce_hex = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        format!("0x{}", hex::encode(n))
    };

    // ── 5. TEE sign EIP-3009 transferWithAuthorization ───────────────
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    let base_fields = json!({
        "chainIndex": chain_index,
        "from": from_addr_str,
        "to": p.recipient,
        "value": p.amount,
        "validAfter": "0",
        "validBefore": valid_before.to_string(),
        "nonce": nonce_hex,
        "verifyingContract": p.currency,
    });

    // 5a. gen-msg-hash → backend computes EIP-712 typed-data hash
    let unsigned_hash_resp: Value = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay charge: gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'msgHash' in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'domainHash' in gen-msg-hash response"))?;

    // 5b. local: HPKE-decrypt session ed25519 seed → sign msgHash
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    // 5c. sign-msg → TEE produces final EIP-3009 secp256k1 signature
    let mut sign_body = base_fields.clone();
    sign_body["domainHash"] = json!(domain_hash);
    sign_body["sessionCert"] = json!(session.session_cert);
    sign_body["sessionSignature"] = json!(session_signature_b64);

    let signed_resp: Value = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay charge: sign-msg failed")?;
    let signature_hex = signed_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'signature' in sign-msg response"))?
        .to_string();

    // ── 6. POST /payment/{id}/credential ─────────────────────────────
    let credential_body = json!({
        "challenge": challenge,
        "payload": {
            "type": "transaction",
            "signature": signature_hex,
            "authorization": {
                "type": "eip-3009",
                "from": from_addr_str,
                "to": p.recipient,
                "value": p.amount,
                "validAfter": "0",
                "validBefore": valid_before.to_string(),
                "nonce": nonce_hex,
            },
        },
    });
    let cred_resp: Value = client
        .post_authed(&path_credential(&payment_id), &access_token, &credential_body)
        .await
        .context("Smart-Account /payment/{id}/credential failed")?;

    Ok(CreateChargeOutput {
        payment_id,
        status: cred_resp["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        tracking_url: cred_resp["trackingUrl"].as_str().map(|s| s.to_string()),
        tx_hash: cred_resp["txHash"].as_str().map(|s| s.to_string()),
    })
}

// ── Escrow mode (§5) — Phase 1 lock funds ───────────────────────────────

// 15-field tuple per Appendix C. ABI-encoded then keccak256 → nonce.
// Order is load-bearing: any reorder breaks signature verification.
sol! {
    #[derive(Debug)]
    struct EscrowAuthFields {
        address provider;
        address receiver;
        address arbitrator;
        address currency;
        uint256 amount;
        uint32 submitWindow;
        uint32 disputeWindow;
        uint32 arbitrationWindow;
        uint32 terminationWindow;
        address hook;
        bytes hookData;
        bytes32 salt;
        address from;
        uint256 validAfter;
        uint256 validBefore;
    }
}

/// Compute the EIP-3009 nonce for an Escrow authorization.
pub fn compute_escrow_nonce(fields: &EscrowAuthFields) -> FixedBytes<32> {
    let encoded = SolValue::abi_encode_params(fields);
    keccak256(&encoded)
}

/// Programmatic params for `create_escrow` — cross-module callers use this
/// instead of the clap-decorated `CreateArgs`.
pub struct EscrowParams {
    // ── Common ──────────────────────────────────────────────────────
    pub amount: String,
    pub currency: String,
    pub description: String,
    pub realm: String,
    pub external_id: Option<String>,
    pub expires_in: Option<u64>,
    pub domain_name: String,
    pub domain_version: String,
    /// EIP-155 chain ID. None → 196 (X Layer).
    pub chain_id: Option<u64>,
    pub valid_after: Option<u64>,
    pub valid_before: Option<u64>,
    // ── Escrow-specific ─────────────────────────────────────────────
    pub escrow_contract: String,
    pub provider: String,
    /// Final receiver. None → uses `provider`.
    pub receiver: Option<String>,
    pub arbitrator: String,
    pub submit_window: u32,
    pub dispute_window: u32,
    pub arbitration_window: u32,
    pub termination_window: u32,
    /// RFC 3339 timestamp, e.g. "2026-05-01T00:00:00Z".
    pub expired_at: String,
    pub hook: String,
    /// Hex (with or without 0x prefix).
    pub hook_data: String,
    /// Anti-replay salt (bytes32 hex). None → random.
    pub salt: Option<String>,
}

impl TryFrom<CreateArgs> for EscrowParams {
    type Error = anyhow::Error;
    fn try_from(a: CreateArgs) -> Result<Self> {
        let provider = a
            .provider
            .ok_or_else(|| anyhow!("--provider is required for --type escrow"))?;
        Ok(Self {
            amount: a.amount,
            currency: a.currency,
            description: a.description,
            realm: a.realm,
            external_id: a.external_id,
            expires_in: a.expires_in,
            domain_name: a.domain_name,
            domain_version: a.domain_version,
            chain_id: a.chain_id,
            valid_after: a.valid_after,
            valid_before: a.valid_before,
            escrow_contract: a
                .escrow_contract
                .ok_or_else(|| anyhow!("--escrow-contract is required for --type escrow"))?,
            receiver: a.receiver,
            arbitrator: a
                .arbitrator
                .ok_or_else(|| anyhow!("--arbitrator is required for --type escrow"))?,
            submit_window: a
                .submit_window
                .ok_or_else(|| anyhow!("--submit-window is required for --type escrow"))?,
            dispute_window: a
                .dispute_window
                .ok_or_else(|| anyhow!("--dispute-window is required for --type escrow"))?,
            arbitration_window: a
                .arbitration_window
                .ok_or_else(|| anyhow!("--arbitration-window is required for --type escrow"))?,
            termination_window: a
                .termination_window
                .ok_or_else(|| anyhow!("--termination-window is required for --type escrow"))?,
            expired_at: a
                .expired_at
                .ok_or_else(|| anyhow!("--expired-at is required for --type escrow"))?,
            hook: a
                .hook
                .ok_or_else(|| anyhow!("--hook is required for --type escrow"))?,
            hook_data: a
                .hook_data
                .ok_or_else(|| anyhow!("--hook-data is required for --type escrow"))?,
            salt: a.salt,
            provider,
        })
    }
}

#[derive(serde::Serialize)]
pub struct CreateEscrowOutput {
    pub payment_id: String,
    pub status: String,
    pub tracking_url: Option<String>,
    pub tx_hash: Option<String>,
    pub order_id: Option<String>,
}

/// Phase 1 of Escrow (TEE-signed):
/// 1. POST /payment/create with escrow params → `{ paymentId, challenge, ... }`
/// 2. Resolve Buyer's logged-in agentic-wallet address (X Layer / chainName=okb).
/// 3. Local: `compute_escrow_nonce` (15-field deterministic nonce per Appendix C).
/// 4. TEE sign EIP-3009 `ReceiveWithAuthorization` (to = escrow contract):
///    - POST `/pre-transaction/gen-msg-hash` with `authorizationType: "ReceiveWithAuthorization"`
///    - Local: HPKE-decrypt session sk, ed25519-sign msgHash
///    - POST `/pre-transaction/sign-msg` → secp256k1 signature (TEE)
/// 5. POST /payment/{id}/credential with full challenge echoed.
#[allow(clippy::too_many_lines)]
pub async fn create_escrow(p: EscrowParams) -> Result<CreateEscrowOutput> {
    // ── 1. Validate inputs ────────────────────────────────────────────
    let parsed_amount: u128 = p
        .amount
        .parse()
        .context("amount must be a non-negative integer in minimal units")?;
    if parsed_amount == 0 {
        bail!("amount must be greater than zero");
    }
    let receiver: String = p
        .receiver
        .clone()
        .unwrap_or_else(|| p.provider.clone());
    for (label, v) in [
        ("escrow-contract", p.escrow_contract.as_str()),
        ("provider", p.provider.as_str()),
        ("receiver", receiver.as_str()),
        ("arbitrator", p.arbitrator.as_str()),
        ("currency", p.currency.as_str()),
        ("hook", p.hook.as_str()),
    ] {
        require_evm_address(v, label)?;
    }

    let hook_data_bytes = hex::decode(p.hook_data.trim_start_matches("0x"))
        .context("hook-data is not valid hex")?;
    let salt: FixedBytes<32> = match &p.salt {
        Some(s) => parse_bytes32_hex(s, "salt")?,
        None => FixedBytes::from(rand_32()),
    };
    let salt_hex = format!("0x{}", hex::encode(salt));

    // ── 2. POST /payment/create ──────────────────────────────────────
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;

    let hook_data_hex_norm = format!("0x{}", hex::encode(&hook_data_bytes));
    let create_body = json!({
        "type": "escrow",
        "amount": p.amount,
        "currency": p.currency,
        "description": p.description,
        "externalId": p.external_id,
        "expiresIn": p.expires_in.unwrap_or(DEFAULT_EXPIRES_IN),
        "realm": p.realm,
        "escrow": json!({
            "escrowContract": p.escrow_contract,
            "provider": p.provider,
            "receiver": receiver,
            "arbitrator": p.arbitrator,
            "submitWindow": p.submit_window,
            "disputeWindow": p.dispute_window,
            "arbitrationWindow": p.arbitration_window,
            "terminationWindow": p.termination_window,
            "expiredAt": p.expired_at,
            "hook": p.hook,
            "hookData": hook_data_hex_norm,
            "salt": salt_hex,
        }),
        "deliveries": json!({
            "includeUrl": true,
            "includeQrCode": true,
            "includeCard": true,
        }),
    });
    let create_resp: Value = client
        .post_authed(PATH_PAYMENT_CREATE, &access_token, &create_body)
        .await
        .context("Smart-Account /payment/create failed")?;

    let payment_id = create_resp["paymentId"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'paymentId' in /payment/create response"))?
        .to_string();
    let challenge = create_resp
        .get("challenge")
        .cloned()
        .ok_or_else(|| anyhow!("missing 'challenge' in /payment/create response"))?;

    // ── 3. Resolve Buyer wallet address on the target chain ─────────
    let chain_id = p.chain_id.unwrap_or(DEFAULT_CHAIN_ID);
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
    let from_addr: Address = from_addr_str
        .parse()
        .context("agentic-wallet address is not a valid EVM address")?;

    // ── 4. Compute timing + escrow nonce (15-field, Appendix C) ─────
    let now = unix_now()?;
    let valid_after = p.valid_after.unwrap_or(0);
    let valid_before = match p.valid_before {
        Some(v) => v,
        None => now
            .checked_add(DEFAULT_VALID_BEFORE_SEC)
            .ok_or_else(|| anyhow!("validBefore overflow"))?,
    };

    let fields = EscrowAuthFields {
        provider: p.provider.parse().context("provider parse")?,
        receiver: receiver.parse().context("receiver parse")?,
        arbitrator: p.arbitrator.parse().context("arbitrator parse")?,
        currency: p.currency.parse().context("currency parse")?,
        amount: U256::from(parsed_amount),
        submitWindow: p.submit_window,
        disputeWindow: p.dispute_window,
        arbitrationWindow: p.arbitration_window,
        terminationWindow: p.termination_window,
        hook: p.hook.parse().context("hook parse")?,
        hookData: hook_data_bytes.into(),
        salt,
        from: from_addr,
        validAfter: U256::from(valid_after),
        validBefore: U256::from(valid_before),
    };
    let nonce = compute_escrow_nonce(&fields);
    let nonce_hex = format!("0x{}", hex::encode(nonce));

    // ── 5. TEE sign EIP-3009 ReceiveWithAuthorization ────────────────
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    // `authorizationType` hints the backend to use `ReceiveWithAuthorization`
    // typeHash instead of the default `TransferWithAuthorization`.
    let base_fields = json!({
        "chainIndex": chain_index,
        "from": from_addr_str,
        "to": p.escrow_contract,
        "value": p.amount,
        "validAfter": valid_after.to_string(),
        "validBefore": valid_before.to_string(),
        "nonce": nonce_hex,
        "verifyingContract": p.currency,
        "authorizationType": "ReceiveWithAuthorization",
    });

    // 5a. gen-msg-hash → backend computes EIP-712 typed-data hash
    let unsigned_hash_resp: Value = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay escrow: gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'msgHash' in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'domainHash' in gen-msg-hash response"))?;

    // 5b. local: HPKE-decrypt session ed25519 seed → sign msgHash
    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    // 5c. sign-msg → TEE produces final EIP-3009 secp256k1 signature
    let mut sign_body = base_fields.clone();
    sign_body["domainHash"] = json!(domain_hash);
    sign_body["sessionCert"] = json!(session.session_cert);
    sign_body["sessionSignature"] = json!(session_signature_b64);

    let signed_resp: Value = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/sign-msg",
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay escrow: sign-msg failed")?;
    let signature_hex = signed_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'signature' in sign-msg response"))?
        .to_string();

    // ── 6. POST /payment/{id}/credential ─────────────────────────────
    let credential_body = json!({
        "challenge": challenge,
        "payload": {
            "type": "transaction",
            "signature": signature_hex,
            "authorization": {
                "type": "eip-3009",
                "from": from_addr_str,
                "to": p.escrow_contract,
                "value": p.amount,
                "validAfter": valid_after.to_string(),
                "validBefore": valid_before.to_string(),
                "nonce": nonce_hex,
            },
        },
    });
    let cred_resp: Value = client
        .post_authed(&path_credential(&payment_id), &access_token, &credential_body)
        .await
        .context("Smart-Account /payment/{id}/credential failed")?;

    Ok(CreateEscrowOutput {
        payment_id,
        status: cred_resp["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        tracking_url: cred_resp["trackingUrl"].as_str().map(|s| s.to_string()),
        tx_hash: cred_resp["txHash"].as_str().map(|s| s.to_string()),
        order_id: cred_resp["orderId"].as_str().map(|s| s.to_string()),
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

/// GET /payment/{id}/status — current state.
pub async fn status(payment_id: String) -> Result<StatusOutput> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;
    let resp: Value = client
        .get_authed(&path_status(&payment_id), &access_token, &[])
        .await
        .context("Smart-Account /payment/{id}/status failed")?;
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
        bail!("{label} must be 32 bytes (64 hex chars), got {}", clean.len());
    }
    let bytes = hex::decode(clean).with_context(|| format!("{label} is not valid hex"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("{label} length mismatch"))?;
    Ok(FixedBytes::from(arr))
}

fn rand_32() -> [u8; 32] {
    use rand::RngCore;
    let mut n = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut n);
    n
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
    fn parses_bytes32_hex() {
        let s = "0x0000000000000000000000000000000000000000000000000000000000000001";
        let b = parse_bytes32_hex(s, "test").unwrap();
        assert_eq!(b.as_slice()[31], 1);
        let b2 = parse_bytes32_hex(&s[2..], "test").unwrap();
        assert_eq!(b, b2);
        assert!(parse_bytes32_hex("0x01", "test").is_err());
    }

    /// Lock in escrow nonce determinism for fixed input — guards against
    /// silent ABI-encoding regressions. The exact 32-byte output is whatever
    /// keccak256(abi.encode(15 fields per Appendix C order)) yields.
    #[test]
    fn escrow_nonce_is_deterministic_and_field_sensitive() {
        let f = EscrowAuthFields {
            provider: "0x1111111111111111111111111111111111111111".parse().unwrap(),
            receiver: "0x2222222222222222222222222222222222222222".parse().unwrap(),
            arbitrator: "0x3333333333333333333333333333333333333333".parse().unwrap(),
            currency: "0x4444444444444444444444444444444444444444".parse().unwrap(),
            amount: U256::from(50_000_000u64),
            submitWindow: 86400,
            disputeWindow: 86400,
            arbitrationWindow: 172800,
            terminationWindow: 86400,
            hook: "0x5555555555555555555555555555555555555555".parse().unwrap(),
            hookData: vec![0xde, 0xad, 0xbe, 0xef].into(),
            salt: parse_bytes32_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000007",
                "salt",
            )
            .unwrap(),
            from: "0x6666666666666666666666666666666666666666".parse().unwrap(),
            validAfter: U256::ZERO,
            validBefore: U256::from(2_000_000_000u64),
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
