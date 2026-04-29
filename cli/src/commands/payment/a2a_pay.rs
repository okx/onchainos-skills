//! A2A Pay CLI: bridges Buyer & Seller skills to the Smart-Account payment backend.
//!
//! Reference: design doc "A2A Payment 协议设计 (Smart-Account 分发方案) v0.2"
//!
//! Two-sided flow (sub-commands):
//!  - `create` (Seller): POST /payment/create — Seller defines amount / symbol /
//!    recipient (or escrow params) and gets back `paymentId` + `challenge` to hand
//!    to the Buyer. No buyer wallet / signing involved.
//!  - `pay` (Buyer): GET /p/{id} → reconstruct EIP-3009 authorization from the
//!    `challenge.data.request`; TEE-sign; POST /p/{id}/credential.
//!  - `status`: GET /p/{id}/status — poll on-chain execution state.

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
    // ── Common ───────────────────────────────────────────────────────
    /// Payment type: `charge` (direct transfer) or `escrow` (lock funds).
    #[arg(long = "type")]
    pub r#type: String,
    /// Decimal amount of tokens (e.g. "50" or "0.01" USDT).
    #[arg(long)]
    pub amount: String,
    /// ERC-20 token symbol (e.g. "USDT")
    #[arg(long)]
    pub symbol: String,
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

    // ── Charge-only ──────────────────────────────────────────────────
    /// Seller wallet address (= EIP-3009 `to` for charge mode).
    #[arg(long)]
    pub recipient: Option<String>,

    // ── Escrow-only ──────────────────────────────────────────────────
    #[arg(long = "escrow-contract")]
    pub escrow_contract: Option<String>,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub receiver: Option<String>,
    #[arg(long)]
    pub arbitrator: Option<String>,
    #[arg(long = "submit-window")]
    pub submit_window: Option<u64>,
    #[arg(long = "dispute-window")]
    pub dispute_window: Option<u64>,
    #[arg(long = "arbitration-window")]
    pub arbitration_window: Option<u64>,
    #[arg(long = "termination-window")]
    pub termination_window: Option<u64>,
    /// Business expiry timestamp (RFC 3339), e.g. "2026-05-01T00:00:00Z".
    #[arg(long = "expired-at")]
    pub expired_at: Option<String>,
    #[arg(long)]
    pub hook: Option<String>,
    /// Settlement hook calldata (hex, with or without 0x prefix).
    #[arg(long = "hook-data")]
    pub hook_data: Option<String>,
    /// Anti-replay salt (bytes32 hex). Default: random.
    #[arg(long)]
    pub salt: Option<String>,
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
            "escrow" => {
                let params = EscrowParams::try_from(*args)?;
                let out = create_payment_escrow(params).await?;
                println!("{}", serde_json::to_string_pretty(&out)?);
                Ok(())
            }
            other => bail!("unknown --type '{other}', expected 'charge' or 'escrow'"),
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

// ── Seller side: escrow create (§5) ─────────────────────────────────────

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
        uint64 submitWindow;
        uint64 disputeWindow;
        uint64 arbitrationWindow;
        uint64 terminationWindow;
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EscrowDetails {
    pub escrow_contract: String,
    pub provider: String,
    pub receiver: String,
    pub arbitrator: String,
    pub submit_window: u64,
    pub dispute_window: u64,
    pub arbitration_window: u64,
    pub termination_window: u64,
    /// RFC 3339 timestamp.
    pub expired_at: String,
    pub hook: String,
    /// Normalized hex string with `0x` prefix.
    pub hook_data: String,
    /// 32-byte salt as `0x`-prefixed hex.
    pub salt: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EscrowParams {
    pub amount: String,
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    pub escrow: EscrowDetails,
}

impl TryFrom<CreateArgs> for EscrowParams {
    type Error = anyhow::Error;
    fn try_from(a: CreateArgs) -> Result<Self> {
        let provider = a
            .provider
            .ok_or_else(|| anyhow!("--provider is required for --type escrow"))?;
        let escrow_contract = a
            .escrow_contract
            .ok_or_else(|| anyhow!("--escrow-contract is required for --type escrow"))?;
        let receiver = a.receiver.unwrap_or_else(|| provider.clone());
        let arbitrator = a
            .arbitrator
            .ok_or_else(|| anyhow!("--arbitrator is required for --type escrow"))?;
        let submit_window = a
            .submit_window
            .ok_or_else(|| anyhow!("--submit-window is required for --type escrow"))?;
        let dispute_window = a
            .dispute_window
            .ok_or_else(|| anyhow!("--dispute-window is required for --type escrow"))?;
        let arbitration_window = a
            .arbitration_window
            .ok_or_else(|| anyhow!("--arbitration-window is required for --type escrow"))?;
        let termination_window = a
            .termination_window
            .ok_or_else(|| anyhow!("--termination-window is required for --type escrow"))?;
        let expired_at = a
            .expired_at
            .ok_or_else(|| anyhow!("--expired-at is required for --type escrow"))?;
        let hook = a
            .hook
            .ok_or_else(|| anyhow!("--hook is required for --type escrow"))?;
        let hook_data_raw = a
            .hook_data
            .ok_or_else(|| anyhow!("--hook-data is required for --type escrow"))?;
        let hook_data_bytes = hex::decode(hook_data_raw.trim_start_matches("0x"))
            .context("hook-data is not valid hex")?;
        let hook_data = format!("0x{}", hex::encode(&hook_data_bytes));
        let salt = match a.salt {
            Some(s) => format!("0x{}", hex::encode(parse_bytes32_hex(&s, "salt")?)),
            None => format!("0x{}", hex::encode(rand_32())),
        };

        Ok(Self {
            amount: a.amount,
            symbol: a.symbol,
            description: a.description,
            external_id: a.external_id,
            expires_in: a.expires_in,
            realm: a.realm,
            escrow: EscrowDetails {
                escrow_contract,
                provider,
                receiver,
                arbitrator,
                submit_window,
                dispute_window,
                arbitration_window,
                termination_window,
                expired_at,
                hook,
                hook_data,
                salt,
            },
        })
    }
}

/// Seller side: POST /payment/create with escrow params — produces `paymentId`
/// + `challenge` for the Buyer.
pub async fn create_payment_escrow(params: EscrowParams) -> Result<CreatePaymentOutput> {
    validate_positive_decimal_amount(&params.amount)?;
    for (label, v) in [
        ("escrow-contract", params.escrow.escrow_contract.as_str()),
        ("provider", params.escrow.provider.as_str()),
        ("receiver", params.escrow.receiver.as_str()),
        ("arbitrator", params.escrow.arbitrator.as_str()),
        ("hook", params.escrow.hook.as_str()),
    ] {
        require_evm_address(v, label)?;
    }

    let mut wallet_client = WalletApiClient::new()?;
    let access_token = ensure_tokens_refreshed().await?;
    let mut value = serde_json::to_value(&params).context("serialize escrow params")?;
    value["type"] = json!("escrow");
    value["deliveries"] = json!({ "includeUrl": true });
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][a2a-pay] POST /payment/create (escrow) body={}",
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

/// Buyer side:
/// 1. GET /p/{id} → reconstruct authorization params from `challenge.data.request`.
/// 2. Resolve Buyer agentic-wallet address on the chain named in `methodDetails.chainId`.
/// 3. Pick `validAfter` / `validBefore` (CLI override or defaults).
/// 4. Compute EIP-3009 nonce:
///    - `charge` intent → random 32 bytes.
///    - `escrow` intent → keccak256(abi.encode(15 fields per Appendix C)).
/// 5. TEE-sign EIP-3009 (gen-msg-hash → ed25519 sign session → sign-msg).
/// 6. POST /p/{id}/credential.
#[allow(clippy::too_many_lines)]
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
    let from_addr_str = addr_info.address.clone();

    // ── 3. Pick timing ───────────────────────────────────────────────
    let now = unix_now()?;
    let valid_after = 0u64;
    let valid_before = now
        .checked_add(DEFAULT_VALID_BEFORE_SEC)
        .ok_or_else(|| anyhow!("validBefore overflow"))?;

    // ── 4. Compute nonce (intent-specific) + build authorization ─────
    let (nonce_hex, authorization_type) = match intent.as_str() {
        "charge" => {
            // random 32 bytes
            use rand::RngCore;
            let mut n = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut n);
            (format!("0x{}", hex::encode(n)), "TransferWithAuthorization")
        }
        "escrow" => {
            let escrow = method_details
                .get("escrow")
                .ok_or_else(|| anyhow!("methodDetails.escrow missing for escrow intent"))?;
            let parsed_amount: u128 = amount
                .parse()
                .context("challenge amount must be a non-negative integer in minimal units")?;
            let from_addr: Address = from_addr_str
                .parse()
                .context("agentic-wallet address is not a valid EVM address")?;
            let hook_data_str = escrow
                .get("hookData")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("escrow.hookData missing"))?;
            let hook_data_bytes = hex::decode(hook_data_str.trim_start_matches("0x"))
                .context("escrow.hookData not valid hex")?;
            let salt_str = escrow
                .get("salt")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("escrow.salt missing"))?;
            let salt = parse_bytes32_hex(salt_str, "escrow.salt")?;
            let fields = EscrowAuthFields {
                provider: parse_addr_field(escrow, "provider")?,
                receiver: parse_addr_field(escrow, "receiver")?,
                arbitrator: parse_addr_field(escrow, "arbitrator")?,
                currency: currency
                    .parse()
                    .context("challenge.request.currency parse")?,
                amount: U256::from(parsed_amount),
                submitWindow: parse_u64_field(escrow, "submitWindow")?,
                disputeWindow: parse_u64_field(escrow, "disputeWindow")?,
                arbitrationWindow: parse_u64_field(escrow, "arbitrationWindow")?,
                terminationWindow: parse_u64_field(escrow, "terminationWindow")?,
                hook: parse_addr_field(escrow, "hook")?,
                hookData: hook_data_bytes.into(),
                salt,
                from: from_addr,
                validAfter: U256::from(valid_after),
                validBefore: U256::from(valid_before),
            };
            let nonce = compute_escrow_nonce(&fields);
            (
                format!("0x{}", hex::encode(nonce)),
                "ReceiveWithAuthorization",
            )
        }
        other => bail!("unknown challenge intent '{other}', expected 'charge' or 'escrow'"),
    };

    // ── 5. TEE sign EIP-3009 ─────────────────────────────────────────
    let session =
        wallet_store::load_session()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let session_key =
        keyring_store::get("session_key").map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    let mut base_fields = json!({
        "chainIndex": chain_index,
        "from": from_addr_str,
        "to": recipient,
        "value": amount,
        "validAfter": valid_after.to_string(),
        "validBefore": valid_before.to_string(),
        "nonce": nonce_hex,
        "verifyingContract": currency,
    });
    // Charge uses backend default (TransferWithAuthorization); escrow needs the hint.
    if authorization_type == "ReceiveWithAuthorization" {
        base_fields["authorizationType"] = json!(authorization_type);
    }

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] POST gen-msg-hash body={base_fields}");
    }
    let unsigned_hash_resp: Value = wallet_client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
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
            &access_token,
            &sign_body,
        )
        .await
        .map_err(format_api_error)
        .context("a2a-pay: sign-msg failed")?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][a2a-pay] sign-msg response={signed_resp}");
    }
    let signature_hex = signed_resp[0]["signature"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'signature' in sign-msg response"))?
        .to_string();

    // ── 6. POST /p/{id}/credential ─────────────────────────────
    // Charge omits `challenge`; escrow includes it (backend rebinds escrow nonce).
    let mut credential_body = json!({
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
    if intent == "escrow" {
        credential_body["challenge"] = challenge;
    }
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

fn parse_addr_field(obj: &Value, key: &str) -> Result<Address> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("escrow.{key} missing"))?
        .parse()
        .with_context(|| format!("escrow.{key} parse"))
}

fn parse_u64_field(obj: &Value, key: &str) -> Result<u64> {
    let v = obj
        .get(key)
        .ok_or_else(|| anyhow!("escrow.{key} missing"))?;
    if let Some(n) = v.as_u64() {
        return Ok(n);
    }
    if let Some(s) = v.as_str() {
        return s
            .parse::<u64>()
            .with_context(|| format!("escrow.{key} parse u64"));
    }
    bail!("escrow.{key} must be a number or numeric string")
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
            hookData: vec![0xde, 0xad, 0xbe, 0xef].into(),
            salt: parse_bytes32_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000007",
                "salt",
            )
            .unwrap(),
            from: "0x6666666666666666666666666666666666666666"
                .parse()
                .unwrap(),
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
