//! Shared x402 payment signing.
//!
//! Used by:
//! - `onchainos payment pay` (manual signing, prints JSON proof).
//! - `ApiClient` auto-payment (transparently attaches a signed header to paid
//!   requests and retries 402 responses).
//!
//! Signing always goes through the TEE session (session cert + Ed25519 session
//! key). Local EIP-3009 signing with a raw private key lives in
//! `payment.rs::cmd_eip3009_sign` and is intentionally kept separate.

use alloy_primitives::U256;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use zeroize::{Zeroize, Zeroizing};

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN;
use crate::commands::payment::addr::parse_recipient_addr;
use crate::{keyring_store, wallet_api::WalletApiClient, wallet_store};

/// Which pricing tier to sign for. The server's config response groups paths
/// into `BASIC` / `PREMIUM` tiers, and each accepts entry's `amount` is an
/// object keyed by these tier names. The caller decides which tier to sign
/// against based on the path being requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentTier {
    Basic,
    Premium,
}

impl PaymentTier {
    /// JSON key used in the `amount` object (e.g. `"basic"` / `"premium"`).
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Premium => "premium",
        }
    }

    /// Parse from the server tier string (case-insensitive). Returns `None`
    /// for anything not in `{BASIC, PREMIUM}`.
    pub fn from_server_str(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("basic") {
            Some(Self::Basic)
        } else if s.eq_ignore_ascii_case("premium") {
            Some(Self::Premium)
        } else {
            None
        }
    }
}

/// Read `EVM_PRIVATE_KEY` from the environment, falling back to
/// `~/.onchainos/.env`.
///
/// The error message explicitly mentions both remediation paths
/// (login OR configure `EVM_PRIVATE_KEY`) because this function is the
/// tripwire for the auto-payment local-sign fallback when the wallet
/// isn't logged in.
pub(crate) fn read_private_key() -> Result<String> {
    std::env::var("EVM_PRIVATE_KEY").or_else(|_| {
        let env_path = crate::home::onchainos_home()?.join(".env");
        let content = std::fs::read_to_string(&env_path).with_context(|| {
            format!(
                "Wallet not logged in and no EVM_PRIVATE_KEY configured. \
                 Either run `onchainos wallet login`, or create {} with \
                 a line `EVM_PRIVATE_KEY=0x<hex_key>`.",
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

/// Result of signing an x402 payment authorization.
///
/// Variants correspond to the three payload shapes the buyer can emit:
///
/// - **Eip3009**: original `exact` (EIP-3009 `transferWithAuthorization`) and
///   `aggr_deferred` (Ed25519 session-key signed) schemes. JSON wire key for
///   the body is `"authorization"`.
/// - **Permit2**: `exact` + Permit2 (any ERC-20 via the canonical Permit2
///   contract). JSON wire key for the body is `"permit2Authorization"`.
/// - **Upto**: `upto` + Permit2 (Permit2 scheme with `Witness.facilitator`
///   field + amount as a cap rather than the exact charge). JSON wire key
///   for the body is `"permit2Authorization"` (same Permit2 shape, different
///   typehash + facilitator binding).
///
/// The downstream JSON wire layout differs between Eip3009 and the Permit2
/// variants (different key for the authorization body) — making this an
/// enum forces every serializer to match on all variants, so adding a new
/// variant later (or accidentally missing one) is a compile error rather
/// than a runtime payload mismatch.
#[derive(Debug)]
pub enum PaymentProof {
    /// EIP-3009 `exact` scheme or `aggr_deferred` scheme.
    Eip3009 {
        /// Base64 EIP-3009 signature (for `exact` scheme) or base64 Ed25519
        /// session signature (for `aggr_deferred` scheme).
        signature: String,
        /// EIP-3009 `authorization` fields echoed back to the payer.
        authorization: Value,
        /// Session cert — populated only for `aggr_deferred` (the server
        /// needs it to verify the Ed25519 signature).
        session_cert: Option<String>,
    },
    /// `exact` scheme using the Permit2 + x402ExactPermit2Proxy flow.
    Permit2 {
        /// Hex-encoded 65-byte secp256k1 signature with `0x` prefix.
        signature: String,
        /// Full `Permit2Authorization` object (from / permitted / spender
        /// / nonce / deadline / witness).
        permit2_authorization: Value,
    },
    /// `upto` scheme using the Permit2 + x402UptoPermit2Proxy flow.
    Upto {
        /// Hex-encoded 65-byte secp256k1 signature with `0x` prefix.
        signature: String,
        /// Full `UptoPermit2Authorization` object — same shape as the exact
        /// variant but with `witness.facilitator` populated and a different
        /// EIP-712 typehash baked in.
        permit2_authorization: Value,
    },
    /// `period` scheme — buyer double-sign (SubscriptionTerms +
    /// Permit2 PermitSingle). Routed here from `payment pay` whenever the
    /// selected accepts entry's scheme is `period`.
    Subscription {
        /// Signed `SubscriptionTerms` (17 fields + unsigned `planId`), as JSON.
        terms: Value,
        /// Permit2 `PermitSingle`, as JSON.
        permit_single: Value,
        /// 0x 65-byte secp256k1 over the SubscriptionTerms digest (signer == payer).
        terms_signature: String,
        /// 0x 65-byte secp256k1 over the PermitSingle digest (signer == payer).
        permit_single_signature: String,
    },
}

impl PaymentProof {
    /// Wire-shape JSON emitted by `onchainos payment pay`. The exact top-level
    /// keys here are the routing inputs the agent dispatcher branches on, so
    /// any rename / restructure here is a breaking change for the skill.
    pub fn to_pay_json(&self) -> Value {
        match self {
            PaymentProof::Eip3009 {
                signature,
                authorization,
                session_cert,
            } => {
                let mut v = json!({
                    "signature": signature,
                    "authorization": authorization,
                });
                if let Some(cert) = session_cert {
                    v["sessionCert"] = json!(cert);
                }
                v
            }
            PaymentProof::Permit2 {
                signature,
                permit2_authorization,
            } => json!({
                "signature": signature,
                "permit2Authorization": permit2_authorization,
            }),
            PaymentProof::Upto {
                signature,
                permit2_authorization,
            } => json!({
                "signature": signature,
                "permit2Authorization": permit2_authorization,
            }),
            PaymentProof::Subscription {
                terms,
                permit_single,
                terms_signature,
                permit_single_signature,
            } => json!({
                "terms": terms,
                "permitSingle": permit_single,
                "termsSignature": terms_signature,
                "permitSingleSignature": permit_single_signature,
            }),
        }
    }
}

/// Select the best accepts entry.
///
/// If the user has saved a default payment asset (via
/// `onchainos payment default set`), prefers any entry whose
/// `(asset, network)` matches it. Otherwise falls back to scheme priority:
/// `exact` > `aggr_deferred` > first entry.
pub fn select_accept(accepts: &[Value]) -> Result<(Value, Option<String>)> {
    let preferred = crate::payment_cache::PaymentCache::load().and_then(|c| c.default_asset);
    select_accept_with_preference(accepts, preferred.as_ref())
}

/// Pure selection logic — exposed for direct testing without touching the
/// on-disk payment cache.
pub fn select_accept_with_preference(
    accepts: &[Value],
    preferred: Option<&crate::payment_cache::PaymentDefault>,
) -> Result<(Value, Option<String>)> {
    if accepts.is_empty() {
        bail!("accepts array is empty");
    }
    if let Some(pref) = preferred {
        if let Some(e) = accepts.iter().find(|a| {
            a["asset"].as_str() == Some(pref.asset.as_str())
                && a["network"].as_str() == Some(pref.network.as_str())
        }) {
            let scheme = e["scheme"].as_str().map(String::from);
            return Ok((e.clone(), scheme));
        }
    }
    if let Some(e) = accepts
        .iter()
        .find(|a| a["scheme"].as_str() == Some("exact"))
    {
        return Ok((e.clone(), Some("exact".into())));
    }
    if let Some(e) = accepts
        .iter()
        .find(|a| a["scheme"].as_str() == Some("aggr_deferred"))
    {
        return Ok((e.clone(), Some("aggr_deferred".into())));
    }
    Ok((
        accepts[0].clone(),
        accepts[0]["scheme"].as_str().map(|s| s.into()),
    ))
}

#[derive(Debug)]
pub(crate) struct ResolvedEntry {
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) pay_to: String,
    pub(crate) asset: String,
    pub(crate) max_timeout_seconds: u64,
    pub(crate) scheme: Option<String>,
}

/// Extract a minimal-unit amount string from an accepts entry, ignoring tiered
/// schemas (treats `amount` strictly as a scalar). Convenience wrapper around
/// `resolve_amount` for callers that only need the legacy `amount` /
/// `maxAmountRequired` shape.
pub fn extract_amount(entry: &Value) -> Result<String> {
    resolve_amount(entry, None)
}

/// Extract a minimal-unit amount string from an accepts entry.
///
/// Supports three server formats:
/// - `amount: {"basic": "100", "premium": "500"}` — new tiered schema; requires
///   `tier` to pick a side.
/// - `amount: "100"` / `amount: 100` — legacy scalar.
/// - `maxAmountRequired: "100"` / `maxAmountRequired: 100` — legacy alternate.
fn resolve_amount(entry: &Value, tier: Option<PaymentTier>) -> Result<String> {
    if let Some(obj) = entry.get("amount").and_then(|v| v.as_object()) {
        let tier = tier.ok_or_else(|| {
            anyhow!(
                "accepts.amount is a tiered object ({{basic, premium}}) but no tier was specified"
            )
        })?;
        let key = tier.as_key();
        let val = obj
            .get(key)
            .ok_or_else(|| anyhow!("accepts.amount is missing '{}' key", key))?;
        return match val {
            Value::String(s) => Ok(s.clone()),
            Value::Number(n) => Ok(n.to_string()),
            _ => bail!("accepts.amount.{} must be a string or number", key),
        };
    }
    if let Some(s) = entry.get("amount").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(n) = entry.get("amount").and_then(|v| v.as_u64()) {
        return Ok(n.to_string());
    }
    if let Some(s) = entry.get("maxAmountRequired").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(n) = entry.get("maxAmountRequired").and_then(|v| v.as_u64()) {
        return Ok(n.to_string());
    }
    bail!("missing 'amount' or 'maxAmountRequired' in accepts entry");
}

/// Extract the numeric EVM chain id from a CAIP-2 string like `"eip155:196"`.
/// Returns an error for non-EVM CAIP-2 namespaces or malformed inputs.
fn caip2_to_evm_chain_id(network: &str) -> Result<u64> {
    let suffix = network.strip_prefix("eip155:").ok_or_else(|| {
        anyhow!("network '{network}' is not a CAIP-2 EVM identifier (eip155:<id>)")
    })?;
    suffix
        .parse::<u64>()
        .with_context(|| format!("network '{network}' has non-numeric chain id"))
}

fn resolve_entry(
    entry: &Value,
    scheme: Option<String>,
    tier: Option<PaymentTier>,
) -> Result<ResolvedEntry> {
    let network = entry["network"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'network' in accepts entry"))?
        .to_string();
    let amount = resolve_amount(entry, tier)?;
    let pay_to_raw = entry["payTo"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'payTo' in accepts entry"))?;
    // Normalize XKO/0x to canonical 0x. The canonical 20-byte payload is what
    // EIP-3009 signing + on-chain verification operate on; we don't need to
    // preserve XKO display here because the proof JSON is consumed by the
    // server-side verifier, not surfaced to end users.
    let chain_id = caip2_to_evm_chain_id(&network)?;
    let (pay_to, _display) =
        parse_recipient_addr(pay_to_raw, chain_id).with_context(|| "accepts.payTo")?;
    let asset = entry["asset"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'asset' in accepts entry"))?
        .to_string();
    let max_timeout_seconds = entry["maxTimeoutSeconds"].as_u64().unwrap_or(300);
    Ok(ResolvedEntry {
        network,
        amount,
        pay_to,
        asset,
        max_timeout_seconds,
        scheme,
    })
}

/// Sign an x402 payment authorization. Applies the user's saved default
/// payment asset (via `onchainos payment default set`) when the
/// `accepts` array contains a matching entry — used by the auto-payment
/// flow so the user's configured preference wins over scheme priority.
///
/// `accepts` accepts either an array (we select the best entry with
/// `select_accept`) or a single object (used as-is). `tier` picks which amount
/// to sign when the server returns the new `amount: {basic, premium}` object
/// schema; for legacy scalar `amount` / `maxAmountRequired` it is ignored.
/// Returns the signed proof and the selected entry so callers can build a
/// header with the right scheme.
/// Parse a raw x402 `accepts` JSON string and produce a `PaymentProof` via
/// `sign_payment_with_preference`. Convenience wrapper for callers (e.g. the
/// agent-commerce buyer flow) that receive `accepts` as the raw HTTP-402 body
/// and want to sign exactly what the server said without consulting the
/// stored default-asset preference.
pub async fn x402_pay_from_accepts(accepts: &str, from: Option<String>) -> Result<PaymentProof> {
    let accepts_value: Value =
        serde_json::from_str(accepts).context("accepts must be a valid JSON array")?;
    let (proof, _entry) =
        sign_payment_with_preference(&accepts_value, from.as_deref(), None, None).await?;
    Ok(proof)
}

pub async fn sign_payment(
    accepts: &Value,
    from: Option<&str>,
    tier: Option<PaymentTier>,
) -> Result<(PaymentProof, Value)> {
    let preferred = crate::payment_cache::PaymentCache::load().and_then(|c| c.default_asset);
    sign_payment_with_preference(accepts, from, tier, preferred.as_ref()).await
}

/// Resolve `(chainIndex, realChainId, payerAddress)` from a chosen accepts
/// entry's `network` (CAIP-2) and an optional `--from`, reusing the same chain
/// lookup + address resolution as the x402 pay flow. Used by the
/// `period` scheme handlers.
///
/// `chainIndex` is the OKX chain index (routes the TEE / facilitator request);
/// `realChainId` is the EIP-712 `domain.chainId` (the eip155 id). They are
/// equal on X Layer but not in general.
pub(crate) async fn resolve_chain_and_payer(
    accepted: &Value,
    from: Option<&str>,
) -> Result<(String, u64, String)> {
    let network = accepted["network"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'network' in accepts entry"))?;
    let real_chain_id = caip2_to_evm_chain_id(network)?;
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
    let wallets =
        wallet_store::load_wallets()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    Ok((chain_index, real_chain_id, addr_info.address.clone()))
}

/// Like [`resolve_chain_and_payer`] but keyed off a chain name/index
/// (`--chain`) instead of an accepts entry — used by the buyer-direct
/// subscription commands (cancel / list / allowance-status) that have no 402.
pub(crate) async fn resolve_chain_and_payer_by_chain(
    chain: &str,
    from: Option<&str>,
) -> Result<(String, u64, String)> {
    let chain_index = crate::chains::resolve_chain(chain);
    let entry = crate::commands::agentic_wallet::chain::get_chain_by_index(&chain_index)
        .await?
        .ok_or_else(|| anyhow!("chain not found: {chain}"))?;
    let chain_name = entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow!("missing chainName in chain entry"))?;
    let real_chain_id =
        crate::commands::agentic_wallet::chain::get_real_chain_index(&chain_index).await?;
    let wallets =
        wallet_store::load_wallets()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    Ok((chain_index, real_chain_id, addr_info.address.clone()))
}

/// Pick the accepts entry (via scheme priority or user's saved default),
/// resolve it to concrete fields, and collapse any tiered `amount` object
/// (`{"basic": "100", "premium": "500"}`) to a scalar string.
///
/// Returns `(entry, params)`:
/// - `entry`: the selected accepts object with `amount` normalized to a scalar.
///   Callers embed this in the V2 payment header as `accepted`.
/// - `params`: typed fields extracted from the entry for signing.
pub(crate) fn prepare_resolved_entry(
    accepts: &Value,
    tier: Option<PaymentTier>,
    preferred: Option<&crate::payment_cache::PaymentDefault>,
) -> Result<(Value, ResolvedEntry)> {
    let (mut entry, scheme) = match accepts.as_array() {
        Some(arr) => select_accept_with_preference(arr, preferred)?,
        None => (
            accepts.clone(),
            accepts["scheme"].as_str().map(|s| s.to_string()),
        ),
    };
    let params = resolve_entry(&entry, scheme, tier)?;
    if entry.get("amount").map(Value::is_object).unwrap_or(false) {
        entry["amount"] = json!(params.amount);
    }
    Ok((entry, params))
}

/// Classify a resolved accepts entry into a Permit2 routing decision.
/// Returns `(is_upto, is_exact_permit2)`; `(false, false)` means the
/// entry should follow a non-Permit2 code path (e.g. exact + EIP-3009).
pub(crate) fn detect_permit2_route(entry: &Value, params: &ResolvedEntry) -> (bool, bool) {
    let scheme_lower = params.scheme.as_deref().unwrap_or("").to_ascii_lowercase();
    let asset_transfer_method = entry
        .get("extra")
        .and_then(|e| e.get("assetTransferMethod"))
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase);
    let is_upto = scheme_lower == "upto";
    let is_exact_permit2 =
        scheme_lower == "exact" && asset_transfer_method.as_deref() == Some("permit2");
    (is_upto, is_exact_permit2)
}

/// Pre-check Permit2 allowance for a Permit2/upto payment. Bails with
/// an actionable error if allowance < `required_amount`; degrades to a
/// warning + Ok if the RPC probe itself fails (so probe outages don't
/// block payment — the on-chain settle path still reverts).
pub(crate) async fn preflight_permit2_allowance(
    chain_index: &str,
    asset: &str,
    payer: &str,
    required_amount: &str,
) -> Result<()> {
    let required: alloy_primitives::U256 = required_amount
        .parse()
        .with_context(|| format!("invalid required amount (decimal uint256): {required_amount}"))?;
    match crate::payment::permit2::rpc::fetch_permit2_allowance(chain_index, asset, payer).await {
        Ok(allowance) if allowance < required => bail!(
            "Permit2 allowance insufficient on token {} for chain {}. \
             Current allowance is {}, but this payment needs {}. \
             The buyer must first call \
             IERC20.approve({}, MAX) once \
             before any x402 Permit2 payment can be settled.",
            asset,
            chain_index,
            allowance,
            required,
            crate::chains::PERMIT2_ADDRESS
        ),
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!(
                "Warning: Permit2 allowance pre-check unavailable on chain {chain_index} ({e:#}); falling back to on-chain settle revert"
            );
            Ok(())
        }
    }
}

/// Time-bounded fields + 256-bit random nonce shared by all Permit2 /
/// upto sign paths. Returns `(valid_after, deadline, nonce)` as decimal
/// strings ready to embed in `*Permit2Input`.
pub(crate) fn permit2_timing_and_nonce(
    max_timeout_seconds: u64,
) -> Result<(String, String, String)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_after = now
        .saturating_sub(crate::payment::permit2::types::CLOCK_SKEW_BACKDATE_SECS)
        .to_string();
    let deadline = now
        .checked_add(max_timeout_seconds)
        .ok_or_else(|| anyhow!("Permit2 deadline overflow"))?
        .to_string();
    let nonce = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        alloy_primitives::U256::from_be_slice(&bytes).to_string()
    };
    Ok((valid_after, deadline, nonce))
}

/// Variant of `sign_payment` that signs exactly what the caller's
/// `accepts` says, without consulting the saved default asset. Used by
/// the manual `onchainos payment pay` command so the user-supplied
/// `--payload` isn't silently reordered by a stored preference.
pub async fn sign_payment_with_preference(
    accepts: &Value,
    from: Option<&str>,
    tier: Option<PaymentTier>,
    preferred: Option<&crate::payment_cache::PaymentDefault>,
) -> Result<(PaymentProof, Value)> {
    let (entry, params) = prepare_resolved_entry(accepts, tier, preferred)?;

    let access_token = ensure_tokens_refreshed().await?;
    let real_chain_id = parse_eip155_chain_id(&params.network)?;

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

    let wallets =
        wallet_store::load_wallets()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let (_acct_id, addr_info) =
        crate::commands::agentic_wallet::transfer::resolve_address(&wallets, from, chain_name)?;
    let payer_addr = &addr_info.address;

    // Permit2 / upto branch — both require a prior PERMIT2 approve; we
    // pre-check allowance so an insufficient approval surfaces here rather
    // than as an on-chain settle revert.
    let (is_upto, is_exact_permit2) = detect_permit2_route(&entry, &params);

    // Subscription scheme: route to the dedicated double-sign (terms +
    // PermitSingle). `sign_subscribe` handles its own allowance-status fetch /
    // EIP-712, so it does not go through the Permit2/upto allowance block below.
    let is_subscription = params
        .scheme
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("period"))
        .unwrap_or(false);
    if is_subscription {
        let signed = crate::payment::subscription::sign::sign_subscribe(
            &chain_index,
            real_chain_id,
            payer_addr,
            &entry,
        )
        .await?;
        let terms = serde_json::to_value(&signed.payload.terms)?;
        let permit_single = serde_json::to_value(&signed.payload.permit)?;
        return Ok((
            PaymentProof::Subscription {
                terms,
                permit_single,
                terms_signature: signed.payload.terms_signature,
                permit_single_signature: signed.payload.permit_signature,
            },
            entry,
        ));
    }

    if is_upto || is_exact_permit2 {
        preflight_permit2_allowance(&chain_index, &params.asset, payer_addr, &params.amount)
            .await?;
        let (valid_after, deadline, nonce) = permit2_timing_and_nonce(params.max_timeout_seconds)?;
        let chain_id = real_chain_id;

        if is_exact_permit2 {
            let input = crate::payment::permit2::eip712::ExactPermit2Input {
                token: &params.asset,
                amount: &params.amount,
                spender: crate::chains::X402_EXACT_PERMIT2_PROXY,
                nonce: &nonce,
                deadline: &deadline,
                witness_to: &params.pay_to,
                witness_valid_after: &valid_after,
                chain_id,
            };
            let payload =
                crate::payment::permit2::sign::sign_exact_permit2(&chain_index, payer_addr, &input)
                    .await?;
            return Ok((
                PaymentProof::Permit2 {
                    signature: payload.signature,
                    permit2_authorization: serde_json::to_value(&payload.permit2_authorization)
                        .context("serialize Permit2Authorization")?,
                },
                entry,
            ));
        }

        // upto requires `extra.facilitatorAddress` — enforced on chain via
        // `msg.sender == witness.facilitator`. Refuse to sign blind.
        let facilitator_addr = entry
            .get("extra")
            .and_then(|e| e.get("facilitatorAddress"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "upto scheme requires extra.facilitatorAddress in the accepts entry, \
                     but it is missing or not a string"
                )
            })?
            .to_string();

        let input = crate::payment::permit2::eip712::UptoPermit2Input {
            token: &params.asset,
            amount: &params.amount, // cap, not exact charge
            spender: crate::chains::X402_UPTO_PERMIT2_PROXY,
            nonce: &nonce,
            deadline: &deadline,
            witness_to: &params.pay_to,
            witness_facilitator: &facilitator_addr,
            witness_valid_after: &valid_after,
            chain_id,
        };
        let payload =
            crate::payment::permit2::sign::sign_upto_permit2(&chain_index, payer_addr, &input)
                .await?;
        return Ok((
            PaymentProof::Upto {
                signature: payload.signature,
                permit2_authorization: serde_json::to_value(&payload.permit2_authorization)
                    .context("serialize UptoPermit2Authorization")?,
            },
            entry,
        ));
    }

    // ── EIP-3009 / aggr_deferred branch (unchanged) ──────────────
    let is_deferred = params
        .scheme
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("aggr_deferred"))
        .unwrap_or(false);
    let valid_before = if is_deferred {
        // aggr_deferred: authorization never expires — server settles later.
        U256::MAX.to_string()
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        now.checked_add(params.max_timeout_seconds)
            .ok_or_else(|| anyhow!("timeout overflow"))?
            .to_string()
    };
    let nonce = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        format!("0x{}", hex::encode(n))
    };

    let base_fields = json!({
        "chainIndex": chain_index,
        "from": payer_addr,
        "to": &params.pay_to,
        "value": &params.amount,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
        "verifyingContract": &params.asset,
    });

    let session =
        wallet_store::load_session()?.ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let encrypted_session_sk = &session.encrypted_session_sk;
    let session_cert = &session.session_cert;
    let session_key =
        keyring_store::get("session_key").map_err(|_| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;

    let mut client = WalletApiClient::new()?;

    let unsigned_hash_resp = client
        .post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/gen-msg-hash",
            &access_token,
            &base_fields,
        )
        .await
        .map_err(format_api_error)
        .context("payment gen-msg-hash failed")?;
    let msg_hash = unsigned_hash_resp[0]["msgHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing msgHash in gen-msg-hash response"))?;
    let domain_hash = unsigned_hash_resp[0]["domainHash"]
        .as_str()
        .ok_or_else(|| anyhow!("missing domainHash in gen-msg-hash response"))?;

    let mut signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, &session_key)?;
    let msg_hash_bytes =
        hex::decode(msg_hash.trim_start_matches("0x")).context("invalid msgHash hex")?;
    let session_signature = crate::crypto::ed25519_sign(&signing_seed, &msg_hash_bytes)?;
    signing_seed.zeroize();
    let session_signature_b64 = B64.encode(&session_signature);

    let authorization = json!({
        "from": payer_addr,
        "to": &params.pay_to,
        "value": &params.amount,
        "validAfter": "0",
        "validBefore": valid_before,
        "nonce": nonce,
    });

    if is_deferred {
        Ok((
            PaymentProof::Eip3009 {
                signature: session_signature_b64,
                authorization,
                session_cert: Some(session_cert.clone()),
            },
            entry,
        ))
    } else {
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
            .context("payment sign-msg failed")?;
        let eip3009_signature = signed_hash_resp[0]["signature"]
            .as_str()
            .ok_or_else(|| anyhow!("missing signature in sign-msg response"))?;

        Ok((
            PaymentProof::Eip3009 {
                signature: eip3009_signature.to_string(),
                authorization,
                session_cert: None,
            },
            entry,
        ))
    }
}

/// Sign an x402 payment authorization locally using a hex private key
/// (`EVM_PRIVATE_KEY`), without touching the wallet session or TEE.
///
/// Signs exactly what `accepts` carries — does NOT consult the saved
/// default asset. Used by the manual `payment pay-local` command,
/// which inherits `payment pay`'s "sign what --payload says" contract so
/// the caller's supplied entry isn't silently reordered by a stored
/// preference.
///
/// Auto-payment (`sign_payment_auto`) calls
/// `sign_payment_local_with_preference` directly instead so the user's
/// saved default applies to the unauthenticated flow just like it does
/// to the TEE path.
pub async fn sign_payment_local(
    accepts: &Value,
    tier: Option<PaymentTier>,
) -> Result<(PaymentProof, Value)> {
    sign_payment_local_with_preference(accepts, tier, None).await
}

/// Variant of `sign_payment_local` that honors a saved default payment
/// asset when one is supplied.
///
/// Shares scheme selection + amount resolution with the TEE path via
/// `prepare_resolved_entry`. If the first pick is `aggr_deferred` (which
/// we can't sign locally), retries against an accepts list with all
/// `aggr_deferred` entries filtered out — so any signable scheme
/// (`exact` / `upto`) is selected instead of failing.
///
/// Returns `(PaymentProof, Value)` with `session_cert = None`, matching
/// the TEE `exact` branch so the downstream `build_payment_header` path
/// is identical.
pub async fn sign_payment_local_with_preference(
    accepts: &Value,
    tier: Option<PaymentTier>,
    preferred: Option<&crate::payment_cache::PaymentDefault>,
) -> Result<(PaymentProof, Value)> {
    use alloy_primitives::FixedBytes;
    use alloy_signer_local::PrivateKeySigner;

    let is_deferred = |s: &Option<String>| {
        s.as_deref()
            .map(|v| v.eq_ignore_ascii_case("aggr_deferred"))
            .unwrap_or(false)
    };

    // First pass honors the saved default. If it lands on aggr_deferred
    // (preferred matched a deferred entry, or accepts contains deferred
    // alongside upto with no exact), retry with deferred entries filtered
    // out so any signable scheme wins.
    let (entry, params) = {
        let (e, p) = prepare_resolved_entry(accepts, tier, preferred)?;
        if is_deferred(&p.scheme) {
            let filtered: Vec<Value> = accepts
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| {
                    a["scheme"]
                        .as_str()
                        .map(|s| !s.eq_ignore_ascii_case("aggr_deferred"))
                        .unwrap_or(true)
                })
                .cloned()
                .collect();
            if !filtered.is_empty() {
                prepare_resolved_entry(&Value::Array(filtered), tier, preferred)?
            } else {
                (e, p)
            }
        } else {
            (e, p)
        }
    };

    if is_deferred(&params.scheme) {
        bail!(
            "aggr_deferred requires a TEE session key — not supported in local-key mode. \
             Run `onchainos wallet login` to enable TEE signing."
        );
    }

    // Permit2 / upto → dedicated local signer (same wire shape as TEE path).
    let (is_upto, is_exact_permit2) = detect_permit2_route(&entry, &params);
    if is_upto || is_exact_permit2 {
        return sign_permit2_local_inner(&entry, &params, is_upto).await;
    }

    // EIP-712 domain is on the selected entry's `extra`, not in ResolvedEntry.
    let domain_name = entry["extra"]["name"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'extra.name' (EIP-712 domain name) in accepts entry"))?;
    let domain_version = entry["extra"]["version"].as_str().unwrap_or("2");

    // `Zeroizing<String>` / `Zeroizing<Vec<u8>>` wipe the raw key material
    // on drop, covering every exit path — including the `bail!` below if
    // the decoded key is the wrong length.
    let pk_hex = Zeroizing::new(read_private_key()?);
    let pk_trimmed = pk_hex.trim();
    let pk_clean = pk_trimmed.strip_prefix("0x").unwrap_or(pk_trimmed);
    let pk_bytes =
        Zeroizing::new(hex::decode(pk_clean).context("EVM_PRIVATE_KEY is not valid hex")?);
    if pk_bytes.len() != 32 {
        bail!(
            "EVM_PRIVATE_KEY must be 32 bytes (64 hex chars), got {}",
            pk_bytes.len()
        );
    }
    let signer = PrivateKeySigner::from_slice(&pk_bytes)
        .map_err(|e| anyhow!("invalid secp256k1 private key: {e}"))?;
    let from = format!("{:#x}", signer.address());

    let chain_id = parse_eip155_chain_id(&params.network)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let valid_before = now
        .checked_add(params.max_timeout_seconds)
        .ok_or_else(|| anyhow!("timeout overflow"))?;
    let nonce_bytes = {
        use rand::RngCore;
        let mut n = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut n);
        n
    };

    let auth = crate::crypto::TransferWithAuthorization {
        from: from
            .parse()
            .context("derived address is not valid EVM hex")?,
        to: params
            .pay_to
            .parse()
            .context("payTo is not a valid EVM address")?,
        value: U256::from_str_radix(&params.amount, 10)
            .map_err(|e| anyhow!("amount not a valid integer: {e}"))?,
        validAfter: U256::ZERO,
        validBefore: U256::from(valid_before),
        nonce: FixedBytes::from(nonce_bytes),
    };
    let domain = crate::crypto::Eip3009DomainParams {
        name: domain_name.to_string(),
        version: domain_version.to_string(),
        chain_id,
        verifying_contract: params
            .asset
            .parse()
            .context("asset is not a valid EVM address")?,
    };

    let sig_b64 = crate::crypto::eip3009_sign(&auth, &domain, &pk_bytes)?;
    // pk_bytes drops here (Zeroizing wipes the Vec<u8> contents).

    // Match TEE exact path output: 0x-prefixed hex, 65 bytes.
    let sig_bytes = B64
        .decode(&sig_b64)
        .context("unexpected base64 decode error from eip3009_sign")?;
    let signature = format!("0x{}", hex::encode(&sig_bytes));
    let authorization = json!({
        "from": from,
        "to": &params.pay_to,
        "value": &params.amount,
        "validAfter": "0",
        "validBefore": valid_before.to_string(),
        "nonce": format!("0x{}", hex::encode(nonce_bytes)),
    });

    Ok((
        PaymentProof::Eip3009 {
            signature,
            authorization,
            session_cert: None,
        },
        entry,
    ))
}

/// Local-key signing for `exact + Permit2` / `upto`. Wire shape matches
/// TEE path; runs the same Permit2 allowance preflight upfront.
async fn sign_permit2_local_inner(
    entry: &Value,
    params: &ResolvedEntry,
    is_upto: bool,
) -> Result<(PaymentProof, Value)> {
    use alloy_signer_local::PrivateKeySigner;

    let pk_hex = Zeroizing::new(read_private_key()?);
    let pk_trimmed = pk_hex.trim();
    let pk_clean = pk_trimmed.strip_prefix("0x").unwrap_or(pk_trimmed);
    let pk_bytes =
        Zeroizing::new(hex::decode(pk_clean).context("EVM_PRIVATE_KEY is not valid hex")?);
    if pk_bytes.len() != 32 {
        bail!(
            "EVM_PRIVATE_KEY must be 32 bytes (64 hex chars), got {}",
            pk_bytes.len()
        );
    }
    // sign_*_permit2_local takes raw pk_bytes; we only need the signer to derive `from`.
    let signer = PrivateKeySigner::from_slice(&pk_bytes)
        .map_err(|e| anyhow!("invalid secp256k1 private key: {e}"))?;
    let payer_addr = format!("{:#x}", signer.address());
    drop(signer);

    let real_chain_id = parse_eip155_chain_id(&params.network)?;
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

    preflight_permit2_allowance(&chain_index, &params.asset, &payer_addr, &params.amount).await?;
    let (valid_after, deadline, nonce) = permit2_timing_and_nonce(params.max_timeout_seconds)?;

    if is_upto {
        // upto requires extra.facilitatorAddress (enforced on-chain).
        let facilitator_addr = entry
            .get("extra")
            .and_then(|e| e.get("facilitatorAddress"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "upto scheme requires extra.facilitatorAddress in the accepts entry, \
                     but it is missing or not a string"
                )
            })?
            .to_string();

        let input = crate::payment::permit2::eip712::UptoPermit2Input {
            token: &params.asset,
            amount: &params.amount, // cap, not exact charge
            spender: crate::chains::X402_UPTO_PERMIT2_PROXY,
            nonce: &nonce,
            deadline: &deadline,
            witness_to: &params.pay_to,
            witness_facilitator: &facilitator_addr,
            witness_valid_after: &valid_after,
            chain_id: real_chain_id,
        };
        let payload =
            crate::payment::permit2::sign::sign_upto_permit2_local(&pk_bytes, &payer_addr, &input)?;
        return Ok((
            PaymentProof::Upto {
                signature: payload.signature,
                permit2_authorization: serde_json::to_value(&payload.permit2_authorization)
                    .context("serialize UptoPermit2Authorization")?,
            },
            entry.clone(),
        ));
    }

    let input = crate::payment::permit2::eip712::ExactPermit2Input {
        token: &params.asset,
        amount: &params.amount,
        spender: crate::chains::X402_EXACT_PERMIT2_PROXY,
        nonce: &nonce,
        deadline: &deadline,
        witness_to: &params.pay_to,
        witness_valid_after: &valid_after,
        chain_id: real_chain_id,
    };
    let payload =
        crate::payment::permit2::sign::sign_exact_permit2_local(&pk_bytes, &payer_addr, &input)?;
    Ok((
        PaymentProof::Permit2 {
            signature: payload.signature,
            permit2_authorization: serde_json::to_value(&payload.permit2_authorization)
                .context("serialize Permit2Authorization")?,
        },
        entry.clone(),
    ))
}

/// Dispatch to TEE signing if the wallet is logged in, otherwise
/// fall back to local private-key signing.
///
/// "Logged in" is defined as `wallets.json` existing — this matches
/// `security.rs`'s existing precheck and keeps the fallback strictly
/// additive. Session-expiry cases (wallets.json present but session
/// revoked) surface the TEE error path unchanged so users are prompted
/// to re-login instead of silently being re-routed to a different
/// signing address.
///
/// Prints a one-shot stderr warning the first time the local branch
/// runs in a process.
pub async fn sign_payment_auto(
    accepts: &Value,
    tier: Option<PaymentTier>,
) -> Result<(PaymentProof, Value)> {
    // Read the saved default asset once and pass it into both branches.
    // The TEE branch would otherwise reload it inside `sign_payment`,
    // opening a TOCTOU where a concurrent `payment default set` writes
    // between the two reads and the signed asset diverges from what
    // the user was shown.
    let preferred = crate::payment_cache::PaymentCache::load().and_then(|c| c.default_asset);
    let logged_in = wallet_store::load_wallets()?.is_some();
    if logged_in {
        sign_payment_with_preference(accepts, None, tier, preferred.as_ref()).await
    } else {
        warn_local_signing_once();
        sign_payment_local_with_preference(accepts, tier, preferred.as_ref()).await
    }
}

/// Writes the local-signing disclaimer to the given sink. Separated from
/// the `Once`-gated public entry point so tests can assert the text
/// deterministically.
fn write_local_signing_warning<W: std::io::Write>(w: &mut W) {
    let _ = writeln!(
        w,
        "[onchainos] payment signed locally with EVM_PRIVATE_KEY (NOT protected by TEE); \
         run `onchainos wallet login` for TEE signing."
    );
}

/// Prints the local-signing disclaimer once per cache lifetime.
/// `Once` dedupes in-process; `PaymentCache.local_signing_warned`
/// dedupes across subprocess invocations. Resets on logout.
fn warn_local_signing_once() {
    use std::sync::Once;
    static WARN: Once = Once::new();
    WARN.call_once(|| {
        let prior = crate::payment_cache::PaymentCache::load().unwrap_or_default();
        if prior.local_signing_warned {
            return;
        }
        write_local_signing_warning(&mut std::io::stderr());
        // Re-read immediately before saving so we only patch the
        // `local_signing_warned` flag, not the whole struct. A concurrent
        // `flush_payment_cache` (fired by a response header) may have
        // written fresh `basic_state` / `premium_state` / `accepts`
        // between the `prior` load and here — saving `prior` back would
        // clobber them. Matches the read-modify-write pattern documented
        // on `flush_payment_cache`.
        let mut fresh = crate::payment_cache::PaymentCache::load().unwrap_or_default();
        fresh.local_signing_warned = true;
        let _ = fresh.save();
    });
}

/// Build the base64-encoded payment header for the chosen mode.
/// Returns `(header_name, header_value)`.
///
/// `entry` is the selected accepts entry (from `sign_payment`'s second return
/// value). For `aggr_deferred`, the caller's proof carries a `session_cert`
/// which is embedded into `entry.extra` so the server can verify the
/// Ed25519 signature.
pub fn build_payment_header(
    proof: &PaymentProof,
    entry: &Value,
    resource: &str,
) -> Result<(&'static str, String)> {
    // Internal callers (OKX openapi) only have the request URL, so reconstruct
    // the minimal `{url, mimeType}` resource object. Third-party x402 servers
    // may send a richer `resource` (with `description` etc.) — those go through
    // `assemble_v2_payment_header` with the object passed verbatim.
    assemble_v2_payment_header(
        proof,
        entry,
        &json!({ "url": resource, "mimeType": "application/json" }),
    )
}

/// Same as [`build_payment_header`] but takes the `resource` value verbatim
/// (the object exactly as it appeared in the decoded 402 payload) instead of
/// reconstructing it from a URL string. Used by `onchainos payment pay`
/// so the header round-trips whatever the server sent — including
/// fields like `description` that a reconstruction would drop.
///
/// v2 only (emits `PAYMENT-SIGNATURE`).
pub fn assemble_v2_payment_header(
    proof: &PaymentProof,
    entry: &Value,
    resource: &Value,
) -> Result<(&'static str, String)> {
    // The Permit2 variants ship `permit2Authorization` instead of
    // `authorization`. Keep the EIP-3009 branch responsible for its own
    // sessionCert embedding — that's a deferred-scheme concern only.
    let mut accepted = entry.clone();
    let payload_inner = match proof {
        PaymentProof::Eip3009 {
            signature,
            authorization,
            session_cert,
        } => {
            if let Some(cert) = session_cert {
                if let Some(obj) = accepted.as_object_mut() {
                    let extra = obj.entry("extra".to_string()).or_insert_with(|| json!({}));
                    if let Some(extra_obj) = extra.as_object_mut() {
                        extra_obj.insert("sessionCert".into(), json!(cert));
                    }
                }
            }
            json!({
                "signature": signature,
                "authorization": authorization,
            })
        }
        PaymentProof::Permit2 {
            signature,
            permit2_authorization,
        } => json!({
            "signature": signature,
            "permit2Authorization": permit2_authorization,
        }),
        PaymentProof::Upto {
            signature,
            permit2_authorization,
        } => json!({
            "signature": signature,
            "permit2Authorization": permit2_authorization,
        }),
        PaymentProof::Subscription {
            terms,
            permit_single,
            terms_signature,
            permit_single_signature,
        } => json!({
            "terms": terms,
            "permitSingle": permit_single,
            "termsSignature": terms_signature,
            "permitSingleSignature": permit_single_signature,
        }),
    };

    let body = json!({
        "x402Version": 2,
        "resource": resource,
        "accepted": accepted,
        "payload": payload_inner,
    });

    let encoded = B64.encode(serde_json::to_vec(&body).context("encode payment header body")?);
    Ok(("PAYMENT-SIGNATURE", encoded))
}

/// Build the `onchainos payment pay` header output: assemble the v2
/// `PAYMENT-SIGNATURE` header and wrap it with the routing metadata the skill
/// needs to replay the request — mirroring the `charge` / `session` contract so
/// the agent never has to hand-assemble the header.
///
/// `wallet` is read from the payer address inside the signed proof
/// (`authorization.from` for EIP-3009, `permit2Authorization.from` for
/// Permit2 / upto).
pub fn pay_with_header_json(
    proof: &PaymentProof,
    entry: &Value,
    resource: &Value,
) -> Result<Value> {
    let (header_name, header_value) = assemble_v2_payment_header(proof, entry, resource)?;
    let pay = proof.to_pay_json();
    let wallet = pay
        .get("authorization")
        .or_else(|| pay.get("permit2Authorization"))
        .and_then(|a| a.get("from"))
        .and_then(|v| v.as_str());
    Ok(json!({
        "authorization_header": header_value,
        "header_name": header_name,
        "scheme": entry.get("scheme").and_then(|v| v.as_str()).unwrap_or(""),
        "wallet": wallet,
    }))
}

/// Extract numeric chain ID from a CAIP-2 `eip155:<chainId>` identifier.
pub(crate) fn parse_eip155_chain_id(network: &str) -> Result<u64> {
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

// ════════════════════════════════════════════════════════════════════════
//  Two-phase quote/pay support
// ════════════════════════════════════════════════════════════════════════

use super::state::{self, Candidate, ParamSpec, PaymentState};

/// True unless `chain_id` is classified as a testnet by the chain registry.
/// Delegates to [`crate::chains::is_mainnet_chain`], which consults the dynamic
/// chain cache and the static registry rather than a hardcoded testnet blacklist
/// (the old blacklist mis-judged unrecognised testnets like Sepolia as mainnet).
pub fn is_mainnet_chain(chain_id: &str) -> bool {
    crate::chains::is_mainnet_chain(chain_id)
}

/// Scheme priority within a tie (lower = preferred). `upto` is semantically
/// exact-with-a-Permit2-cap, so it is docked adjacent to `exact` (ahead of
/// `charge`) rather than left in the catch-all bucket. Note this rank only
/// breaks ties between candidates of the same token+network+mainnet class; it
/// does not decide multi-scheme *triggering* (that is the `{exact,
/// aggr_deferred, charge}` set in `rank_candidates`, which excludes `upto`).
fn scheme_rank(scheme: &str) -> u8 {
    match scheme {
        "aggr_deferred" => 0,
        "exact" => 1,
        "upto" => 2,
        "charge" => 3,
        _ => 4,
    }
}

/// Lexicographic candidate comparator (`Less` = `a` is the better pick):
/// ① same token → smaller atomic `amount` wins (never compared across tokens);
/// ② mainnet before testnet; ③ scheme priority `aggr_deferred > exact > upto > charge`.
fn cmp_candidates(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.token_symbol == b.token_symbol {
        let av = a.amount.parse::<u128>().unwrap_or(u128::MAX);
        let bv = b.amount.parse::<u128>().unwrap_or(u128::MAX);
        match av.cmp(&bv) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    match (a.is_mainnet, b.is_mainnet) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    scheme_rank(&a.scheme).cmp(&scheme_rank(&b.scheme))
}

/// Rank payment candidates into `(candidates, alternatives)` per architecture §5:
///
/// - Fewer than 2 distinct trigger schemes (`exact`/`aggr_deferred`/`charge`) →
///   no multi-scheme card: pick the single best, `recommended:true`, no alternatives.
/// - All candidates zero-balance → `recommended:null` on every candidate, list
///   all as `candidates`, no auto-pick.
/// - Otherwise → payable-first ranking; winner `recommended:true` in
///   `candidates`, the rest `recommended:false` (ordered) in `alternatives`.
pub fn rank_candidates(candidates: Vec<Candidate>) -> (Vec<Candidate>, Vec<Candidate>) {
    use std::collections::HashSet;
    if candidates.is_empty() {
        return (vec![], vec![]);
    }

    let distinct_trigger: HashSet<&str> = candidates
        .iter()
        .map(|c| c.scheme.as_str())
        .filter(|s| matches!(*s, "exact" | "aggr_deferred" | "charge"))
        .collect();
    let multi = distinct_trigger.len() >= 2;
    let any_balance = candidates.iter().any(|c| c.has_balance);

    // Payable candidates first, then by the lexicographic comparator.
    let mut ranked = candidates;
    ranked.sort_by(|a, b| match (a.has_balance, b.has_balance) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => cmp_candidates(a, b),
    });

    if !multi {
        // Single candidate card: best entry recommended, no alternatives.
        let mut best = ranked.remove(0);
        best.recommended = Some(true);
        return (vec![best], vec![]);
    }

    if !any_balance {
        // No spendable balance anywhere → no auto-pick.
        for c in ranked.iter_mut() {
            c.recommended = None;
        }
        return (ranked, vec![]);
    }

    let mut winner = ranked.remove(0);
    winner.recommended = Some(true);
    for c in ranked.iter_mut() {
        c.recommended = Some(false);
    }
    (vec![winner], ranked)
}

// ── Two-phase pay ───────────────────────────────────────────────────────

/// `payment pay --payment-id` `data` shape (stability contract).
#[derive(serde::Serialize)]
struct PayResult {
    /// Duplicated inside `data` per the task-system contract. The
    /// envelope-level `ok` governs the exit code; this mirrors success/failure.
    ok: bool,
    #[serde(rename = "paymentId")]
    payment_id: String,
    scheme: String,
    /// `success` | `failed` | `pending`.
    status: String,
    #[serde(rename = "txHash")]
    tx_hash: Option<String>,
    result: Value,
    error: Option<String>,
    #[serde(rename = "decodedReceipt")]
    decoded_receipt: Option<Value>,
}

fn now_unix() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// Parse repeatable `--param key=value`; malformed → `invalid_input`.
fn parse_kv(param: &[String]) -> Result<Vec<(String, String)>> {
    param
        .iter()
        .map(|raw| {
            let (k, v) = raw
                .split_once('=')
                .ok_or_else(|| anyhow!("invalid_input: --param must be key=value, got '{raw}'"))?;
            let k = k.trim();
            if k.is_empty() {
                bail!("invalid_input: --param key must not be empty");
            }
            Ok((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Human-facing confirming prompt for a two-phase pay (built from persisted state).
///
/// `selected_index` is an **accepts[] index** — the exact same index space
/// `pay_from_state` signs (`raw_accepts[i]`) and that `cli_command_spec.md`
/// defines `--selected-index` against. The preview therefore describes the
/// candidate whose `accepts_index` equals `selected_index` (candidates are
/// reordered by `rank_candidates`, so indexing `candidates[i]` directly would
/// describe a *different* entry than the one signed — a fund-moving
/// misrepresentation at the confirming gate). When no candidate carries that
/// back-reference (rare same-scheme entries dropped from the ranked card), the
/// preview falls back to the raw `accepts[i]` entry, still the signed one.
fn pay_confirming(
    st: &PaymentState,
    selected_index: Option<usize>,
) -> crate::output::CliConfirming {
    let ch = &st.decoded_challenge;
    // Rich preview: the candidate at the SAME accepts index the signer uses.
    let picked = match selected_index {
        Some(i) => st.candidates.iter().find(|c| c.accepts_index == i),
        None => st
            .candidates
            .iter()
            .find(|c| c.recommended == Some(true))
            .or_else(|| st.candidates.first()),
    };
    let message = if let Some(c) = picked {
        format!(
            "Will pay {} {} ({}, {}) to {} - confirm to proceed",
            c.amount_human, c.token_symbol, c.scheme, c.chain_name, ch.recipient
        )
    } else if let Some(a) = selected_index.and_then(|i| st.accepts.get(i)) {
        // Fallback: no matching candidate, but the raw accepts entry (which the
        // signer will use) is authoritative. Describe it directly.
        format!(
            "Will pay {} {} ({}) to {} - confirm to proceed",
            a.amount, a.asset, a.scheme, ch.recipient
        )
    } else {
        format!(
            "Will pay {} to {} - confirm to proceed",
            ch.amount_human, ch.recipient
        )
    };
    let mut next = format!("onchainos payment pay --payment-id {}", st.payment_id);
    if let Some(i) = selected_index {
        next.push_str(&format!(" --selected-index {i}"));
    }
    next.push_str(" --yes");
    crate::output::CliConfirming {
        message,
        next,
        scene: None,
    }
}

/// MCP / handler entry point for two-phase pay. Returns the `PayResult` `data`
/// value. When `yes` is false, returns `Err(CliConfirming)` so both the CLI
/// (exit 2) and MCP (`{confirming,...}`) render the gate identically.
pub async fn fetch_pay(
    payment_id: &str,
    selected_index: Option<usize>,
    param: &[String],
    yes: bool,
) -> Result<Value> {
    let owner = state::current_owner_id().unwrap_or_default();
    let st = state::read(payment_id, &owner, now_unix())?;

    // Validate --selected-index against the persisted accepts.
    if let Some(i) = selected_index {
        if i >= st.raw_accepts.len() {
            bail!(
                "invalid_input: --selected-index {i} is out of range (accepts has {} entr{})",
                st.raw_accepts.len(),
                if st.raw_accepts.len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            );
        }
    }
    let biz_params = parse_kv(param)?;

    // Confirming gate — a paymentId proves a quote occurred, not that the human
    // approved the spend. STOP here unless --yes/--force. No signing/network yet.
    if !yes {
        return Err(pay_confirming(&st, selected_index).into());
    }

    let data = pay_from_state(&st, selected_index, &biz_params).await?;
    Ok(data)
}

/// Sign (TEE) → assemble header → replay to the merchant → decode receipt.
/// Merchant non-200 *after* signing is a recorded `status:"failed"` (not an
/// error — funds may have moved), so this returns `Ok` with the outcome.
async fn pay_from_state(
    st: &PaymentState,
    selected_index: Option<usize>,
    biz_params: &[(String, String)],
) -> Result<Value> {
    // Narrow accepts to the user's choice (or let the signer auto-select).
    let accepts_for_sign = match selected_index {
        Some(i) => json!([st.raw_accepts[i].clone()]),
        None => Value::Array(st.raw_accepts.clone()),
    };

    let (proof, entry) = sign_payment_with_preference(&accepts_for_sign, None, None, None).await?;
    let scheme = entry
        .get("scheme")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Assemble the v2 PAYMENT-SIGNATURE header (needs the challenge `resource`).
    let (header_name, header_value) = match st.resource.as_ref() {
        Some(resource) => assemble_v2_payment_header(&proof, &entry, resource)?,
        None => {
            // v1 challenge — no resource to bind; fall back to raw proof header.
            let encoded = B64.encode(serde_json::to_vec(&proof.to_pay_json())?);
            ("PAYMENT-SIGNATURE", encoded)
        }
    };

    // Replay the request with the signed header + business params, honoring the
    // persisted paid-call method + per-param carrier plan (A2MCP outputSchema).
    let (status, tx_hash, result, error, decoded_receipt) = replay_merchant(
        &st.endpoint_url,
        &st.method,
        &st.param_plan,
        header_name,
        &header_value,
        biz_params,
        &proof,
        &entry,
    )
    .await;

    if status == "success" {
        state::cleanup(&st.payment_id);
    }

    let ok = status == "success";
    let out = PayResult {
        ok,
        payment_id: st.payment_id.clone(),
        scheme,
        status,
        tx_hash,
        result,
        error,
        decoded_receipt,
    };
    serde_json::to_value(out).map_err(Into::into)
}

/// Replay the paid request to the merchant. Never returns `Err` — a transport /
/// non-200 outcome after signing is reported as `status:"failed"` so the
/// already-signed authorization is recorded rather than lost.
///
/// The request is assembled per the persisted `method` + `param_plan`
/// (A2MCP `outputSchema`) via [`http_carrier::build_request`], so a POST/body,
/// header, or path-carrier endpoint is replayed correctly — not just GET+query.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
async fn replay_merchant(
    url: &str,
    method: &str,
    plan: &[ParamSpec],
    header_name: &str,
    header_value: &str,
    biz_params: &[(String, String)],
    proof: &PaymentProof,
    entry: &Value,
) -> (String, Option<String>, Value, Option<String>, Option<Value>) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = (proof, entry);
            return (
                "failed".into(),
                None,
                Value::Null,
                Some(e.to_string()),
                None,
            );
        }
    };
    let resp = super::http_carrier::build_request(&client, method, url, biz_params, plan)
        .header(header_name, header_value)
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return (
                "failed".into(),
                None,
                Value::Null,
                Some(e.to_string()),
                None,
            )
        }
    };
    let status_code = resp.status();
    let payment_response = resp
        .headers()
        .get("PAYMENT-RESPONSE")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body_text = resp.text().await.unwrap_or_default();
    let result: Value =
        serde_json::from_str(&body_text).unwrap_or(Value::String(body_text.clone()));

    let decoded_receipt = payment_response
        .as_deref()
        .and_then(|h| super::decode_receipt::decode_receipt(Some(h), None).ok())
        .and_then(|r| serde_json::to_value(r).ok());
    let tx_hash = decoded_receipt
        .as_ref()
        .and_then(|r| r.get("transaction"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    if status_code.is_success() {
        ("success".into(), tx_hash, result, None, decoded_receipt)
    } else if status_code.as_u16() == 402 {
        // Facilitator still settling — non-terminal.
        (
            "pending".into(),
            tx_hash,
            result,
            Some(format!(
                "facilitator non-terminal: HTTP {}",
                status_code.as_u16()
            )),
            decoded_receipt,
        )
    } else {
        (
            "failed".into(),
            tx_hash,
            result,
            Some(format!("merchant returned HTTP {}", status_code.as_u16())),
            decoded_receipt,
        )
    }
}

// ── Session down-sink decision math ─────────────────────────────────────

/// Parameters for `fetch_session` (mirrors the MCP `payment_session` tool).
#[derive(Default)]
pub struct SessionParams {
    pub action: String,
    pub channel_id: Option<String>,
    pub challenge: Option<String>,
    pub unit_amount: Option<String>,
    pub cumulative_amount: Option<String>,
    pub escrow: Option<String>,
    pub chain_id: Option<u64>,
    pub deposit: Option<String>,
    pub from: Option<String>,
    /// A previously-signed voucher signature. Its presence is the reuse-vs-sign
    /// signal: supplied ⇒ `strategy:"reuse"` (unless a drift or a classified
    /// rejection forces a resign). Never persisted; never logged as a secret here.
    pub reuse_signature: Option<String>,
    /// The seller-reported cumulative from a `70015` drift error. When it
    /// differs from `cumulative_amount`, the client's prior voucher is stale: the
    /// cumulative is recomputed on top of this figure and `strategy` is forced to
    /// `"sign"` (a fresh signature — reuse cannot ride a drifted base).
    pub server_cumulative: Option<String>,
}

/// `payment session` decision `data` shape (added fields).
#[derive(serde::Serialize, Default)]
pub struct SessionData {
    pub strategy: String,
    pub cumulative_amount: String,
    #[serde(rename = "needsTopUp")]
    pub needs_top_up: bool,
    #[serde(rename = "sessionSnapshot")]
    pub session_snapshot: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refund: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_text: Option<String>,
}

/// top-up guard: `current_cum + unit_amount > deposit`.
pub fn needs_top_up(current_cum: u128, unit_amount: u128, deposit: u128) -> bool {
    current_cum.saturating_add(unit_amount) > deposit
}

/// refund on close: `deposit - final_cum` (saturating; never negative).
pub fn compute_refund(deposit: u128, final_cum: u128) -> u128 {
    deposit.saturating_sub(final_cum)
}

/// Rejection classifier. Returns the machine token, or `None` when the
/// voucher is acceptable.
pub fn classify_recovery(
    current_cum: u128,
    unit_amount: u128,
    deposit: u128,
) -> Option<&'static str> {
    if current_cum.saturating_add(unit_amount) > deposit {
        Some("amount_exceeds_deposit")
    } else if unit_amount == 0 {
        Some("delta_too_small")
    } else {
        None
    }
}

fn parse_u128_or_zero(s: &Option<String>) -> u128 {
    s.as_deref()
        .and_then(|v| v.parse::<u128>().ok())
        .unwrap_or(0)
}

/// MCP / session entry point: compute the reuse-vs-sign strategy, cumulative
/// math, top-up need, refund, and recovery classification for a channel op.
/// Pure decision layer — the actual signing stays in the CLI session handlers,
/// which feed it the channel's persisted deposit / prior cumulative.
pub async fn fetch_session(params: SessionParams) -> Result<Value> {
    let current_cum = parse_u128_or_zero(&params.cumulative_amount);
    let unit = parse_u128_or_zero(&params.unit_amount);
    let deposit = parse_u128_or_zero(&params.deposit);
    let has_deposit = params.deposit.is_some();

    // server-reported cumulative drift (`70015`). When the seller reports a
    // cumulative that differs from the client's assumption, the previously-signed
    // voucher is stale: recompute the cumulative on top of the server figure and
    // resign (reuse cannot ride a drifted base).
    let drift = params
        .server_cumulative
        .as_deref()
        .and_then(|s| s.parse::<u128>().ok())
        .filter(|srv| *srv != current_cum);
    let base_cum = drift.unwrap_or(current_cum);
    let new_cum = base_cum.saturating_add(unit);

    let needs = has_deposit && needs_top_up(base_cum, unit, deposit);
    let recovery = if has_deposit {
        classify_recovery(base_cum, unit, deposit).map(|s| s.to_string())
    } else {
        None
    };

    // reuse-vs-sign: a supplied prior signature means reuse, unless a drift
    // or a classified rejection forces a fresh signature.
    //
    // top-up override: when the channel needs a top-up (the voucher would exceed the
    // deposit) neither `sign` nor `reuse` is a valid next action — signing an
    // over-deposit voucher cannot succeed. Emit an unambiguous `topup` so the
    // agent's only signalled action is to fund the channel first, rather than the
    // self-contradictory `sign`.
    let strategy = if needs {
        "topup"
    } else if drift.is_some() {
        "sign"
    } else if params.reuse_signature.is_some() && recovery.is_none() {
        "reuse"
    } else {
        "sign"
    }
    .to_string();

    let mut reason_text = recovery.as_deref().map(|r| match r {
        "amount_exceeds_deposit" => "voucher cumulative exceeds the channel deposit".to_string(),
        "delta_too_small" => "voucher delta is zero — nothing to authorize".to_string(),
        "invalid_signature" => "voucher signature failed verification".to_string(),
        other => other.to_string(),
    });
    if drift.is_some() && reason_text.is_none() {
        reason_text = Some(format!(
            "voucher cumulative drifted (70015): server cumulative {base_cum}, \
             recomputed to {new_cum} and resigned"
        ));
    }

    let refund = if params.action == "close" && has_deposit {
        Some(compute_refund(deposit, new_cum).to_string())
    } else {
        None
    };

    let data = SessionData {
        strategy,
        cumulative_amount: new_cum.to_string(),
        needs_top_up: needs,
        session_snapshot: json!({
            "channelId": params.channel_id,
            "deposit": params.deposit,
            "cumulative": new_cum.to_string(),
        }),
        refund,
        recovery,
        reason_text,
    };
    serde_json::to_value(data).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::await_holding_lock)] // TEST_ENV_MUTEX serializes process-wide env vars across async tests
    use super::*;

    // ── rank_candidates (business rule) ───────────────────────────────
    fn cand(
        scheme: &str,
        token: &str,
        amount: &str,
        mainnet: bool,
        has_balance: bool,
    ) -> Candidate {
        Candidate {
            scheme: scheme.into(),
            accepts_index: 0,
            chain_id: if mainnet {
                "8453".into()
            } else {
                "1952".into()
            },
            chain_name: if mainnet {
                "Base".into()
            } else {
                "X Layer Testnet".into()
            },
            is_mainnet: mainnet,
            token_symbol: token.into(),
            amount: amount.into(),
            amount_human: amount.into(),
            has_balance,
            recommended: None,
        }
    }

    #[test]
    fn rank_same_token_picks_smallest_amount() {
        let (c, alt) = rank_candidates(vec![
            cand("exact", "USDC", "10000", true, true),
            cand("aggr_deferred", "USDC", "5000", true, true),
        ]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].amount, "5000");
        assert_eq!(c[0].recommended, Some(true));
        assert_eq!(alt.len(), 1);
        assert_eq!(alt[0].recommended, Some(false));
    }

    #[test]
    fn rank_mainnet_beats_testnet() {
        let (c, _alt) = rank_candidates(vec![
            cand("exact", "DAI", "5000", false, true),
            cand("aggr_deferred", "USDC", "5000", true, true),
        ]);
        assert!(c[0].is_mainnet);
        assert_eq!(c[0].token_symbol, "USDC");
    }

    #[test]
    fn rank_scheme_priority_aggr_over_exact() {
        // Different tokens, both mainnet → amount criterion skipped, scheme decides.
        let (c, _alt) = rank_candidates(vec![
            cand("exact", "USDC", "5000", true, true),
            cand("aggr_deferred", "DAI", "5000", true, true),
        ]);
        assert_eq!(c[0].scheme, "aggr_deferred");
    }

    #[test]
    fn scheme_rank_docks_upto_adjacent_to_exact() {
        // upto (exact-with-a-Permit2-cap) must rank ahead of charge, adjacent to
        // exact — not in the catch-all bucket behind charge (tie-break fix).
        assert!(scheme_rank("aggr_deferred") < scheme_rank("exact"));
        assert!(scheme_rank("exact") < scheme_rank("upto"));
        assert!(scheme_rank("upto") < scheme_rank("charge"));
        assert!(scheme_rank("charge") < scheme_rank("period"));
    }

    #[test]
    fn rank_all_zero_balance_recommends_null() {
        let (c, alt) = rank_candidates(vec![
            cand("exact", "USDC", "5000", true, false),
            cand("aggr_deferred", "DAI", "5000", true, false),
        ]);
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|x| x.recommended.is_none()));
        assert!(alt.is_empty());
    }

    #[test]
    fn rank_single_scheme_no_trigger() {
        let (c, alt) = rank_candidates(vec![cand("exact", "USDC", "5000", true, false)]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].recommended, Some(true));
        assert!(alt.is_empty());
    }

    // ── pay_confirming index-space alignment (fund-safety) ────────────
    //
    // Regression guard: `pay_confirming`'s preview MUST describe the exact
    // accepts entry `pay_from_state` signs. `rank_candidates` reorders the
    // multi-scheme card, so a naive `candidates[selected_index]` lookup would
    // describe a different scheme/amount than the one signed.
    #[test]
    fn pay_confirming_previews_the_entry_the_signer_will_sign() {
        use super::state::{AcceptEntry, DecodedChallenge};

        // accepts[0] = exact / 10000 USDC ; accepts[1] = aggr_deferred / 5000 USDC.
        // rank picks the smaller same-token amount → aggr_deferred (accepts_index 1)
        // becomes the winner and is stored FIRST in st.candidates, ahead of the
        // exact candidate (accepts_index 0). Signing raw_accepts[0] must be
        // previewed as the exact/10000 entry, NOT candidates[0] (aggr_deferred).
        let winner = Candidate {
            scheme: "aggr_deferred".into(),
            accepts_index: 1,
            chain_id: "8453".into(),
            chain_name: "Base".into(),
            is_mainnet: true,
            token_symbol: "USDC".into(),
            amount: "5000".into(),
            amount_human: "0.005".into(),
            has_balance: true,
            recommended: Some(true),
        };
        let alt = Candidate {
            scheme: "exact".into(),
            accepts_index: 0,
            chain_id: "8453".into(),
            chain_name: "Base".into(),
            is_mainnet: true,
            token_symbol: "USDC".into(),
            amount: "10000".into(),
            amount_human: "0.01".into(),
            has_balance: true,
            recommended: Some(false),
        };
        let st = PaymentState {
            payment_id: "pid-reorder".into(),
            owner_wallet: "acc-1".into(),
            created_at: 1_000,
            expires_at: 9_999_999_999,
            accepts: vec![
                AcceptEntry {
                    index: 0,
                    scheme: "exact".into(),
                    amount: "10000".into(),
                    asset: "USDC".into(),
                    network: "eip155:8453".into(),
                },
                AcceptEntry {
                    index: 1,
                    scheme: "aggr_deferred".into(),
                    amount: "5000".into(),
                    asset: "USDC".into(),
                    network: "eip155:8453".into(),
                },
            ],
            decoded_challenge: DecodedChallenge {
                amount: "10000".into(),
                amount_human: "0.01".into(),
                decimals: 6,
                recipient: "0xRECIPIENT".into(),
                expires: 0,
                supported: true,
                unsupported_reason: None,
            },
            // Stored winner-first, exactly as quote.rs persists (winner ++ alternatives).
            candidates: vec![winner, alt],
            known_params: serde_json::Map::new(),
            merchant_body: String::new(),
            endpoint_url: "https://merchant.example/x".into(),
            raw_accepts: vec![
                json!({"scheme":"exact","amount":"10000","network":"eip155:8453"}),
                json!({"scheme":"aggr_deferred","amount":"5000","network":"eip155:8453"}),
            ],
            resource: None,
            method: state::default_http_method(),
            param_plan: vec![],
        };

        // selecting accepts index 0 → signer uses the exact/10000 entry, so the
        // preview must name exact + 0.01 (the exact amount_human), NOT the ranked
        // winner (aggr_deferred / 0.005).
        let c0 = pay_confirming(&st, Some(0));
        assert!(
            c0.message.contains("exact"),
            "preview must describe the signed scheme (exact): {}",
            c0.message
        );
        assert!(
            c0.message.contains("0.01"),
            "preview must show the exact entry amount (0.01): {}",
            c0.message
        );
        assert!(
            !c0.message.contains("aggr_deferred"),
            "preview must NOT describe the ranked winner when index 0 is chosen: {}",
            c0.message
        );
        assert!(c0.next.contains("--selected-index 0"));

        // selecting accepts index 1 → aggr_deferred / 0.005.
        let c1 = pay_confirming(&st, Some(1));
        assert!(
            c1.message.contains("aggr_deferred") && c1.message.contains("0.005"),
            "preview for index 1 must describe aggr_deferred/0.005: {}",
            c1.message
        );

        // no index → the recommended winner (aggr_deferred).
        let cn = pay_confirming(&st, None);
        assert!(cn.message.contains("aggr_deferred"), "got: {}", cn.message);
        assert!(!cn.next.contains("--selected-index"));
    }

    // ── session decision math ──────────────────────────────────────────
    #[test]
    fn is_mainnet_chain_flags_testnet_indices() {
        // Known testnet (X Layer testnet) → not mainnet; everything else → mainnet.
        assert!(!is_mainnet_chain("1952"));
        assert!(is_mainnet_chain("8453")); // Base
        assert!(is_mainnet_chain("196")); // X Layer
        assert!(is_mainnet_chain("1")); // Ethereum
    }

    #[test]
    fn session_needs_top_up_guard() {
        assert!(needs_top_up(40, 20, 50)); // 60 > 50
        assert!(!needs_top_up(10, 20, 50)); // 30 <= 50
    }

    #[test]
    fn session_refund_is_saturating() {
        assert_eq!(compute_refund(100, 30), 70);
        assert_eq!(compute_refund(30, 100), 0);
    }

    #[test]
    fn session_recovery_classification() {
        assert_eq!(
            classify_recovery(40, 20, 50),
            Some("amount_exceeds_deposit")
        );
        assert_eq!(classify_recovery(10, 0, 50), Some("delta_too_small"));
        assert_eq!(classify_recovery(10, 20, 50), None);
    }

    #[tokio::test]
    async fn fetch_session_close_computes_refund() {
        let params = SessionParams {
            action: "close".into(),
            deposit: Some("100000".into()),
            cumulative_amount: Some("40000".into()),
            unit_amount: Some("0".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["refund"], "60000");
        assert_eq!(data["cumulative_amount"], "40000");
    }

    #[tokio::test]
    async fn fetch_session_reuse_strategy_when_signature_supplied() {
        // a supplied prior signature ⇒ reuse (no drift, voucher acceptable).
        let params = SessionParams {
            action: "voucher".into(),
            reuse_signature: Some("0xdeadbeef".into()),
            cumulative_amount: Some("100".into()),
            unit_amount: Some("50".into()),
            deposit: Some("1000".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["strategy"], "reuse");
        assert_eq!(data["cumulative_amount"], "150");
        assert_eq!(data["needsTopUp"], false);
    }

    #[tokio::test]
    async fn fetch_session_sign_strategy_without_reuse_signature() {
        // no prior signature ⇒ sign.
        let params = SessionParams {
            action: "voucher".into(),
            cumulative_amount: Some("100".into()),
            unit_amount: Some("50".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["strategy"], "sign");
    }

    #[tokio::test]
    async fn fetch_session_voucher_computes_needs_top_up() {
        // 40 + 20 = 60 > 50 ⇒ needsTopUp true, computed by the CLI.
        let params = SessionParams {
            action: "voucher".into(),
            cumulative_amount: Some("40".into()),
            unit_amount: Some("20".into()),
            deposit: Some("50".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["needsTopUp"], true);
        // when a top-up is required, strategy must be the unambiguous `topup`
        // (never `sign`) — signing an over-deposit voucher cannot succeed.
        assert_eq!(data["strategy"], "topup");
    }

    #[tokio::test]
    async fn fetch_session_drift_forces_resign_and_recomputes() {
        // client assumed cum 100, server reports 130 (70015).
        // Even though a reuse signature is offered, the drift forces a resign and
        // the cumulative is recomputed on top of the server figure: 130 + 20 = 150.
        let params = SessionParams {
            action: "voucher".into(),
            reuse_signature: Some("0xstale".into()),
            cumulative_amount: Some("100".into()),
            server_cumulative: Some("130".into()),
            unit_amount: Some("20".into()),
            deposit: Some("1000".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["strategy"], "sign", "drift must resign, not reuse");
        assert_eq!(data["cumulative_amount"], "150");
        assert!(data["reason_text"]
            .as_str()
            .unwrap_or_default()
            .contains("70015"));
    }

    #[tokio::test]
    async fn fetch_session_no_drift_when_server_matches_client() {
        // server_cumulative equal to the client's assumption is NOT a drift.
        let params = SessionParams {
            action: "voucher".into(),
            reuse_signature: Some("0xok".into()),
            cumulative_amount: Some("100".into()),
            server_cumulative: Some("100".into()),
            unit_amount: Some("50".into()),
            deposit: Some("1000".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["strategy"], "reuse");
        assert_eq!(data["cumulative_amount"], "150");
    }

    #[tokio::test]
    async fn fetch_session_top_up_overrides_sign_and_reuse() {
        // an unacceptable voucher (amount exceeds deposit) needs a
        // top-up, so strategy must be `topup` — not `sign` and not `reuse` —
        // even when a reuse signature was offered. `topup` is the only
        // non-contradictory signal (recovery still classifies the reason).
        let params = SessionParams {
            action: "voucher".into(),
            reuse_signature: Some("0xreuse".into()),
            cumulative_amount: Some("40".into()),
            unit_amount: Some("20".into()),
            deposit: Some("50".into()),
            ..Default::default()
        };
        let data = fetch_session(params).await.unwrap();
        assert_eq!(data["strategy"], "topup");
        assert_eq!(data["needsTopUp"], true);
        assert_eq!(data["recovery"], "amount_exceeds_deposit");
    }

    #[test]
    fn select_accept_prefers_exact() {
        let accepts: Vec<Value> = serde_json::from_str(
            r#"[
            {"scheme":"aggr_deferred","network":"eip155:196"},
            {"scheme":"exact","network":"eip155:196"}
        ]"#,
        )
        .unwrap();
        let (_entry, scheme) = select_accept_with_preference(&accepts, None).unwrap();
        assert_eq!(scheme.as_deref(), Some("exact"));
    }

    #[test]
    fn select_accept_falls_back_to_aggr_deferred() {
        let accepts: Vec<Value> =
            serde_json::from_str(r#"[{"scheme":"aggr_deferred","network":"eip155:1"}]"#).unwrap();
        let (_entry, scheme) = select_accept_with_preference(&accepts, None).unwrap();
        assert_eq!(scheme.as_deref(), Some("aggr_deferred"));
    }

    #[test]
    fn select_accept_empty_array_errors() {
        assert!(select_accept_with_preference(&[], None).is_err());
    }

    #[test]
    fn select_accept_preference_wins_over_scheme_priority() {
        // Two entries: one is exact (scheme priority), one matches the
        // user's default asset. Preference should pick the latter even
        // though it's aggr_deferred.
        let accepts: Vec<Value> = serde_json::from_str(
            r#"[
                {"scheme":"exact","network":"eip155:196","asset":"0xUSDG","payTo":"0xP"},
                {"scheme":"aggr_deferred","network":"eip155:196","asset":"0xUSDT","payTo":"0xP"}
            ]"#,
        )
        .unwrap();
        let pref = crate::payment_cache::PaymentDefault {
            asset: "0xUSDT".to_string(),
            network: "eip155:196".to_string(),
            name: Some("USDT".to_string()),
        };
        let (entry, scheme) = select_accept_with_preference(&accepts, Some(&pref)).unwrap();
        assert_eq!(entry["asset"].as_str(), Some("0xUSDT"));
        assert_eq!(scheme.as_deref(), Some("aggr_deferred"));
    }

    #[test]
    fn select_accept_preference_falls_back_when_no_match() {
        // Preference asks for an asset not in the accepts list — fall
        // back to the existing scheme-priority rule.
        let accepts: Vec<Value> = serde_json::from_str(
            r#"[
                {"scheme":"aggr_deferred","network":"eip155:1","asset":"0xUSDC","payTo":"0xP"},
                {"scheme":"exact","network":"eip155:1","asset":"0xDAI","payTo":"0xP"}
            ]"#,
        )
        .unwrap();
        let pref = crate::payment_cache::PaymentDefault {
            asset: "0xUSDT".to_string(),
            network: "eip155:196".to_string(),
            name: None,
        };
        let (entry, scheme) = select_accept_with_preference(&accepts, Some(&pref)).unwrap();
        assert_eq!(entry["asset"].as_str(), Some("0xDAI"));
        assert_eq!(scheme.as_deref(), Some("exact"));
    }

    fn resolved_entry_with_scheme(scheme: &str) -> ResolvedEntry {
        ResolvedEntry {
            network: "eip155:196".into(),
            amount: "1".into(),
            pay_to: "0xP".into(),
            asset: "0xA".into(),
            max_timeout_seconds: 600,
            scheme: Some(scheme.into()),
        }
    }

    #[test]
    fn detect_permit2_route_classifies_three_branches() {
        // upto → upto branch regardless of assetTransferMethod
        let upto_entry = json!({"extra": {"assetTransferMethod": "permit2"}});
        assert_eq!(
            detect_permit2_route(&upto_entry, &resolved_entry_with_scheme("upto")),
            (true, false)
        );

        // exact + assetTransferMethod=permit2 → exact_permit2 branch
        let exact_p2 = json!({"extra": {"assetTransferMethod": "Permit2"}});
        assert_eq!(
            detect_permit2_route(&exact_p2, &resolved_entry_with_scheme("EXACT")),
            (false, true),
            "scheme + assetTransferMethod should both be case-insensitive"
        );

        // exact without permit2 marker → non-Permit2 (EIP-3009 path)
        let exact_eip3009 = json!({"extra": {"assetTransferMethod": "eip3009"}});
        assert_eq!(
            detect_permit2_route(&exact_eip3009, &resolved_entry_with_scheme("exact")),
            (false, false)
        );

        // Missing extra → non-Permit2
        let bare = json!({});
        assert_eq!(
            detect_permit2_route(&bare, &resolved_entry_with_scheme("exact")),
            (false, false)
        );

        // charge → neither Permit2 branch, so `sign_payment_with_preference`
        // takes the standard EIP-3009 TEE path (same as a plain exact) and
        // `pay_from_state` can sign + replay a `--selected-index` charge
        // candidate. Regression guard for the quote/pay charge-signability
        // question: charge must NOT be routed to Permit2/upto and must NOT be
        // rejected here.
        let charge_entry = json!({"extra": {"assetTransferMethod": "eip3009"}});
        assert_eq!(
            detect_permit2_route(&charge_entry, &resolved_entry_with_scheme("charge")),
            (false, false)
        );
    }

    #[test]
    fn permit2_timing_and_nonce_yields_consistent_window_and_random_nonce() {
        let (va1, dl1, n1) = permit2_timing_and_nonce(600).unwrap();
        let (_, _, n2) = permit2_timing_and_nonce(600).unwrap();
        let va: u64 = va1.parse().unwrap();
        let dl: u64 = dl1.parse().unwrap();
        assert!(dl > va, "deadline must be after valid_after");
        assert!(
            dl - va >= 600,
            "deadline - valid_after must cover max_timeout (got {})",
            dl - va
        );
        assert_ne!(n1, n2, "nonce must be random across calls");
        let _: alloy_primitives::U256 = n1.parse().expect("nonce must parse as decimal U256");
    }

    #[test]
    fn local_path_filters_out_aggr_deferred_when_upto_is_signable() {
        // Reproduces the bug fixed in `sign_payment_local_with_preference`:
        // accepts = [aggr_deferred, upto], no exact, no preferred.
        // Raw selection picks aggr_deferred by scheme priority; after
        // filtering deferred entries out, the second pass picks upto.
        let accepts: Vec<Value> = serde_json::from_str(
            r#"[
                {"scheme":"aggr_deferred","network":"eip155:196","asset":"0xA","payTo":"0xP"},
                {"scheme":"upto","network":"eip155:196","asset":"0xA","payTo":"0xP",
                 "extra":{"assetTransferMethod":"permit2"}}
            ]"#,
        )
        .unwrap();

        let (_, scheme) = select_accept_with_preference(&accepts, None).unwrap();
        assert_eq!(
            scheme.as_deref(),
            Some("aggr_deferred"),
            "raw select must reproduce the pre-fix behavior"
        );

        let filtered: Vec<Value> = accepts
            .iter()
            .filter(|a| {
                a["scheme"]
                    .as_str()
                    .map(|s| !s.eq_ignore_ascii_case("aggr_deferred"))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        let (entry, scheme) = select_accept_with_preference(&filtered, None).unwrap();
        assert_eq!(scheme.as_deref(), Some("upto"));
        assert_eq!(entry["scheme"], "upto");
    }

    #[test]
    fn select_accept_preference_requires_both_asset_and_network_match() {
        // Same asset on a different network — not a match.
        let accepts: Vec<Value> = serde_json::from_str(
            r#"[
                {"scheme":"exact","network":"eip155:1","asset":"0xUSDT","payTo":"0xP"}
            ]"#,
        )
        .unwrap();
        let pref = crate::payment_cache::PaymentDefault {
            asset: "0xUSDT".to_string(),
            network: "eip155:196".to_string(),
            name: None,
        };
        let (entry, scheme) = select_accept_with_preference(&accepts, Some(&pref)).unwrap();
        assert_eq!(entry["network"].as_str(), Some("eip155:1"));
        assert_eq!(scheme.as_deref(), Some("exact"));
    }

    #[test]
    fn resolve_entry_extracts_amount_from_max_amount_required() {
        let v = json!({
            "network":"eip155:1",
            "maxAmountRequired":"999",
            "payTo":"0x1111111111111111111111111111111111111111",
            "asset":"0xB"
        });
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.amount, "999");
    }

    #[test]
    fn resolve_entry_default_timeout() {
        let v = json!({
            "network":"eip155:1",
            "amount":"1",
            "payTo":"0x1111111111111111111111111111111111111111",
            "asset":"0xB"
        });
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.max_timeout_seconds, 300);
    }

    #[test]
    fn resolve_entry_xko_pay_to_normalizes_to_0x_on_xlayer() {
        // payTo arrives as XKO-prefixed on XLayer; ResolvedEntry stores the
        // canonical 0x form so EIP-3009 signing + on-chain verification work.
        let v = json!({
            "network":"eip155:196",
            "amount":"1",
            "payTo":"XKO1111111111111111111111111111111111111111",
            "asset":"0xB"
        });
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.pay_to, "0x1111111111111111111111111111111111111111");
    }

    #[test]
    fn resolve_entry_xko_pay_to_rejected_off_xlayer() {
        let v = json!({
            "network":"eip155:1",
            "amount":"1",
            "payTo":"XKO1111111111111111111111111111111111111111",
            "asset":"0xB"
        });
        let err = format!("{:#}", resolve_entry(&v, None, None).unwrap_err());
        assert!(err.contains("only supported on X Layer"), "got: {}", err);
    }

    #[test]
    fn resolve_amount_object_selects_by_tier() {
        let v = json!({"amount": {"basic": "100", "premium": "500"}});
        assert_eq!(resolve_amount(&v, Some(PaymentTier::Basic)).unwrap(), "100");
        assert_eq!(
            resolve_amount(&v, Some(PaymentTier::Premium)).unwrap(),
            "500"
        );
    }

    #[test]
    fn resolve_amount_object_requires_tier() {
        let v = json!({"amount": {"basic": "100", "premium": "500"}});
        assert!(resolve_amount(&v, None).is_err());
    }

    #[test]
    fn resolve_amount_object_missing_key_errors() {
        let v = json!({"amount": {"basic": "100"}});
        assert!(resolve_amount(&v, Some(PaymentTier::Premium)).is_err());
    }

    #[test]
    fn resolve_amount_scalar_ignores_tier() {
        let v = json!({"amount": "42"});
        assert_eq!(resolve_amount(&v, Some(PaymentTier::Basic)).unwrap(), "42");
        assert_eq!(resolve_amount(&v, None).unwrap(), "42");
    }

    #[test]
    fn resolve_amount_number_form() {
        let v = json!({"amount": 42});
        assert_eq!(resolve_amount(&v, None).unwrap(), "42");
    }

    #[test]
    fn resolve_entry_object_amount_with_tier() {
        let v = json!({
            "network": "eip155:196",
            "amount": {"basic": "100", "premium": "500"},
            "payTo": "0x1111111111111111111111111111111111111111",
            "asset": "0xB"
        });
        let r = resolve_entry(&v, None, Some(PaymentTier::Premium)).unwrap();
        assert_eq!(r.amount, "500");
    }

    #[test]
    fn payment_tier_roundtrip() {
        assert_eq!(
            PaymentTier::from_server_str("BASIC"),
            Some(PaymentTier::Basic)
        );
        assert_eq!(
            PaymentTier::from_server_str("basic"),
            Some(PaymentTier::Basic)
        );
        assert_eq!(
            PaymentTier::from_server_str("Premium"),
            Some(PaymentTier::Premium)
        );
        assert_eq!(PaymentTier::from_server_str("other"), None);
        assert_eq!(PaymentTier::Basic.as_key(), "basic");
        assert_eq!(PaymentTier::Premium.as_key(), "premium");
    }

    #[test]
    fn to_pay_json_eip3009_without_session_cert() {
        let proof = PaymentProof::Eip3009 {
            signature: "sig-eip3009".into(),
            authorization: json!({"from": "0xA", "to": "0xB"}),
            session_cert: None,
        };
        let v = proof.to_pay_json();
        assert_eq!(v["signature"], "sig-eip3009");
        assert_eq!(v["authorization"], json!({"from": "0xA", "to": "0xB"}));
        let obj = v.as_object().expect("top-level must be an object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["authorization", "signature"]);
    }

    #[test]
    fn to_pay_json_eip3009_with_session_cert_for_aggr_deferred() {
        let proof = PaymentProof::Eip3009 {
            signature: "sig-eip3009".into(),
            authorization: json!({}),
            session_cert: Some("cert-aggr".into()),
        };
        let v = proof.to_pay_json();
        assert_eq!(v["sessionCert"], "cert-aggr");
        let obj = v.as_object().expect("top-level must be an object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["authorization", "sessionCert", "signature"]);
    }

    #[test]
    fn to_pay_json_permit2_emits_permit2authorization_only() {
        let proof = PaymentProof::Permit2 {
            signature: "0xdeadbeef".into(),
            permit2_authorization: json!({"from": "0xA", "spender": "0x402085…0001"}),
        };
        let v = proof.to_pay_json();
        assert_eq!(v["signature"], "0xdeadbeef");
        assert_eq!(v["permit2Authorization"]["spender"], "0x402085…0001");
        let obj = v.as_object().expect("top-level must be an object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        // Exactly these two keys — no `authorization`, no `sessionCert`.
        assert_eq!(keys, ["permit2Authorization", "signature"]);
    }

    #[test]
    fn to_pay_json_upto_emits_permit2authorization_no_session_cert() {
        let proof = PaymentProof::Upto {
            signature: "0xdeadbeef".into(),
            permit2_authorization: json!({"witness": {"facilitator": "0xF"}}),
        };
        let v = proof.to_pay_json();
        assert_eq!(v["signature"], "0xdeadbeef");
        assert_eq!(v["permit2Authorization"]["witness"]["facilitator"], "0xF");
        assert!(
            v.get("sessionCert").is_none(),
            "upto no longer emits sessionCert"
        );
        let obj = v.as_object().expect("top-level must be an object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        // upto wire shape == exact+Permit2 (no sessionCert).
        assert_eq!(keys, ["permit2Authorization", "signature"]);
    }

    #[test]
    fn build_payment_header_includes_resource() {
        let proof = PaymentProof::Eip3009 {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: None,
        };
        let entry = json!({"scheme":"exact","network":"eip155:1"});
        let (name, value) =
            build_payment_header(&proof, &entry, "https://api.example.com/foo").unwrap();
        assert_eq!(name, "PAYMENT-SIGNATURE");
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["x402Version"], 2);
        assert_eq!(body["resource"]["url"], "https://api.example.com/foo");
        assert_eq!(body["resource"]["mimeType"], "application/json");
    }

    #[test]
    fn build_payment_header_embeds_session_cert_for_deferred() {
        let proof = PaymentProof::Eip3009 {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: Some("cert-123".into()),
        };
        let entry = json!({"scheme":"aggr_deferred","network":"eip155:196"});
        let (_name, value) =
            build_payment_header(&proof, &entry, "https://api.example.com/foo").unwrap();
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["accepted"]["extra"]["sessionCert"], "cert-123");
    }

    // ── P1: `payment pay` direct-header emission ──────────────────

    #[test]
    fn assemble_v2_header_embeds_resource_verbatim() {
        let proof = PaymentProof::Eip3009 {
            signature: "sig".into(),
            authorization: json!({"from": "0xPayer"}),
            session_cert: None,
        };
        let entry = json!({"scheme":"exact","network":"eip155:196"});
        // A server-provided resource object carrying an extra `description`
        // field and a non-default mimeType. The skill passes `decoded.resource`
        // verbatim today, so both must survive into the header unchanged —
        // unlike `build_payment_header`, which reconstructs `{url, mimeType}`.
        let resource = json!({
            "url": "https://api.example.com/data",
            "description": "Premium data",
            "mimeType": "text/plain"
        });
        let (name, value) = assemble_v2_payment_header(&proof, &entry, &resource).unwrap();
        assert_eq!(name, "PAYMENT-SIGNATURE");
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["x402Version"], 2);
        assert_eq!(body["resource"], resource); // verbatim — keeps description + text/plain
        assert_eq!(body["payload"]["authorization"]["from"], "0xPayer");
    }

    #[test]
    fn pay_with_header_json_emits_header_and_routing_metadata() {
        // aggr_deferred carries a sessionCert that must merge into
        // accepted.extra WITHOUT clobbering the existing `name` — the exact
        // step the skill used to do by hand and frequently got wrong.
        let proof = PaymentProof::Eip3009 {
            signature: "sig".into(),
            authorization: json!({"from": "0xPayer", "to": "0xMerchant"}),
            session_cert: Some("cert-aggr".into()),
        };
        let entry =
            json!({"scheme":"aggr_deferred","network":"eip155:196","extra":{"name":"USDG"}});
        let resource = json!({"url":"https://api.example.com/data","mimeType":"application/json"});

        let out = pay_with_header_json(&proof, &entry, &resource).unwrap();

        assert_eq!(out["header_name"], "PAYMENT-SIGNATURE");
        assert_eq!(out["scheme"], "aggr_deferred");
        assert_eq!(out["wallet"], "0xPayer");
        let header = out["authorization_header"]
            .as_str()
            .expect("header is a string");
        let body: Value = serde_json::from_slice(&B64.decode(header).unwrap()).unwrap();
        assert_eq!(body["accepted"]["extra"]["sessionCert"], "cert-aggr");
        assert_eq!(body["accepted"]["extra"]["name"], "USDG"); // not clobbered
    }

    #[test]
    fn pay_with_header_json_wallet_from_permit2_authorization() {
        // Permit2 / upto proofs put the payer under `permit2Authorization.from`,
        // not `authorization.from` — wallet extraction must handle both.
        let proof = PaymentProof::Permit2 {
            signature: "0xsig".into(),
            permit2_authorization: json!({"from": "0xPermitPayer"}),
        };
        let entry = json!({"scheme":"exact","network":"eip155:196",
            "extra":{"assetTransferMethod":"permit2"}});
        let resource = json!({"url":"https://api.example.com/data"});

        let out = pay_with_header_json(&proof, &entry, &resource).unwrap();

        assert_eq!(out["wallet"], "0xPermitPayer");
        let header = out["authorization_header"].as_str().unwrap();
        let body: Value = serde_json::from_slice(&B64.decode(header).unwrap()).unwrap();
        assert!(body["payload"].get("permit2Authorization").is_some());
        assert!(body["payload"].get("authorization").is_none());
    }

    #[test]
    fn read_private_key_reads_from_env_var() {
        // The env var is checked first, so this doesn't need a temp home.
        // Use a scoped SetEnv guard pattern to restore state regardless of outcome.
        struct EnvGuard(&'static str, Option<String>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.1 {
                    Some(v) => std::env::set_var(self.0, v),
                    None => std::env::remove_var(self.0),
                }
            }
        }
        let prev = std::env::var("EVM_PRIVATE_KEY").ok();
        let _guard = EnvGuard("EVM_PRIVATE_KEY", prev);
        std::env::set_var(
            "EVM_PRIVATE_KEY",
            "0x1111111111111111111111111111111111111111111111111111111111111111",
        );
        let got = read_private_key().unwrap();
        assert_eq!(
            got,
            "0x1111111111111111111111111111111111111111111111111111111111111111"
        );
    }

    const TEST_PK: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    // secp256k1 address derived from TEST_PK (lowercased hex, 0x-prefixed)
    const TEST_PK_ADDR: &str = "0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a";

    #[tokio::test]
    async fn sign_payment_auto_uses_local_when_no_wallets_json() {
        // Serialize access to process-wide env vars.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("sign_payment_auto_local");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);

        let accepts = json!([{
            "scheme": "exact",
            "network": "eip155:196",
            "amount": "1",
            "payTo": "0x1111111111111111111111111111111111111111",
            "asset": "0x2222222222222222222222222222222222222222",
            "maxTimeoutSeconds": 300,
            "extra": {"name": "USDG", "version": "1"}
        }]);
        let (proof, entry) = sign_payment_auto(&accepts, None).await.unwrap();

        std::env::remove_var("ONCHAINOS_HOME");
        std::env::remove_var("EVM_PRIVATE_KEY");
        std::fs::remove_dir_all(&dir).ok();

        // Hallmarks of the local path: the proof must be the EIP-3009
        // variant (sign_payment_local can only produce that), session_cert
        // is None, and the recovered signer is the address derived from
        // TEST_PK.
        let PaymentProof::Eip3009 {
            signature,
            authorization,
            session_cert,
        } = proof
        else {
            panic!("expected Eip3009 proof from local signing path");
        };
        assert!(session_cert.is_none());
        assert!(signature.starts_with("0x"));
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
        assert_eq!(authorization["from"].as_str(), Some(TEST_PK_ADDR));
    }

    #[test]
    fn write_local_signing_warning_contains_key_phrases() {
        let mut buf = Vec::new();
        write_local_signing_warning(&mut buf);
        let text = String::from_utf8(buf).unwrap();
        assert!(
            text.contains("EVM_PRIVATE_KEY"),
            "missing key env var name: {text}"
        );
        assert!(
            text.contains("NOT protected by TEE"),
            "missing TEE disclaimer: {text}"
        );
        assert!(
            text.contains("wallet login"),
            "missing recovery hint: {text}"
        );
    }

    #[tokio::test]
    async fn sign_payment_local_produces_exact_scheme_proof() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accepts = json!([{
            "scheme": "exact",
            "network": "eip155:196",
            "amount": "1000000",
            "payTo": "0x1111111111111111111111111111111111111111",
            "asset": "0x2222222222222222222222222222222222222222",
            "maxTimeoutSeconds": 300,
            "extra": {"name": "USDG", "version": "1"}
        }]);
        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);
        let (proof, entry) = sign_payment_local(&accepts, None).await.unwrap();
        std::env::remove_var("EVM_PRIVATE_KEY");

        let PaymentProof::Eip3009 {
            signature,
            authorization,
            session_cert,
        } = proof
        else {
            panic!("expected Eip3009 proof from sign_payment_local");
        };

        // Signature shape: 0x + 130 hex chars (65 bytes r||s||v).
        assert!(signature.starts_with("0x"));
        assert_eq!(signature.len(), 2 + 130);
        assert!(session_cert.is_none());
        assert_eq!(authorization["from"].as_str(), Some(TEST_PK_ADDR));
        assert_eq!(
            authorization["to"].as_str(),
            Some("0x1111111111111111111111111111111111111111")
        );
        assert_eq!(authorization["value"].as_str(), Some("1000000"));
        assert_eq!(authorization["validAfter"].as_str(), Some("0"));
        // selected entry is the exact entry
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
    }

    #[tokio::test]
    async fn sign_payment_local_rejects_aggr_deferred_only() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accepts = json!([{
            "scheme": "aggr_deferred",
            "network": "eip155:196",
            "amount": "1000000",
            "payTo": "0x1111111111111111111111111111111111111111",
            "asset": "0x2222222222222222222222222222222222222222",
            "maxTimeoutSeconds": 300,
            "extra": {"name": "USDG", "version": "1"}
        }]);
        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);
        let err = sign_payment_local(&accepts, None).await.unwrap_err();
        std::env::remove_var("EVM_PRIVATE_KEY");
        let msg = err.to_string();
        assert!(
            msg.contains("aggr_deferred") || msg.contains("exact"),
            "unexpected error message: {msg}"
        );
        assert!(
            msg.contains("wallet login"),
            "error should suggest login: {msg}"
        );
    }

    #[tokio::test]
    async fn sign_payment_local_with_preference_honors_default_matching_exact() {
        // Two exact entries, different assets. Without preferred, scheme
        // priority picks the first exact. With preferred pointing at the
        // second, the saved default wins.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accepts = json!([
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": "100",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            },
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": "200",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDT", "version": "1"}
            }
        ]);
        let preferred = crate::payment_cache::PaymentDefault {
            asset: "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".into(),
            network: "eip155:196".into(),
            name: Some("USDT".into()),
        };

        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);
        let (_proof, entry) = sign_payment_local_with_preference(&accepts, None, Some(&preferred))
            .await
            .unwrap();
        std::env::remove_var("EVM_PRIVATE_KEY");
        assert_eq!(
            entry["asset"].as_str(),
            Some("0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"),
            "saved default asset should win over accepts[0]"
        );
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
    }

    #[tokio::test]
    async fn sign_payment_local_with_preference_falls_back_when_default_is_aggr_deferred_only() {
        // The preferred asset is offered only as aggr_deferred (which we
        // can't sign locally); a different asset is offered as exact.
        // Local signing should fall back and pick the exact entry rather
        // than failing — the default is a preference, not a hard
        // requirement, when the matching offering is unusable.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accepts = json!([
            {
                "scheme": "aggr_deferred",
                "network": "eip155:196",
                "amount": "100",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            },
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": "200",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDT", "version": "1"}
            }
        ]);
        let preferred = crate::payment_cache::PaymentDefault {
            asset: "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            network: "eip155:196".into(),
            name: Some("USDG".into()),
        };

        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);
        let (_proof, entry) = sign_payment_local_with_preference(&accepts, None, Some(&preferred))
            .await
            .unwrap();
        std::env::remove_var("EVM_PRIVATE_KEY");
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
        assert_eq!(
            entry["asset"].as_str(),
            Some("0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"),
            "should fall back to USDT/exact when USDG only offers aggr_deferred"
        );
    }

    #[tokio::test]
    async fn sign_payment_local_with_preference_bails_when_every_entry_is_aggr_deferred() {
        // `preferred` points at an asset the server only offers as
        // `aggr_deferred`, and there is no other `exact` entry to fall
        // back to — local signing cannot proceed and must bail with a
        // message pointing the user at `wallet login` (TEE signing).
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accepts = json!([
            {
                "scheme": "aggr_deferred",
                "network": "eip155:196",
                "amount": "100",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            },
            {
                "scheme": "aggr_deferred",
                "network": "eip155:196",
                "amount": "200",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDT", "version": "1"}
            }
        ]);
        let preferred = crate::payment_cache::PaymentDefault {
            asset: "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            network: "eip155:196".into(),
            name: Some("USDG".into()),
        };

        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);
        let err = sign_payment_local_with_preference(&accepts, None, Some(&preferred))
            .await
            .unwrap_err();
        std::env::remove_var("EVM_PRIVATE_KEY");

        let msg = err.to_string();
        assert!(
            msg.contains("aggr_deferred"),
            "expected bail message to name the unsupported scheme: {msg}"
        );
        assert!(
            msg.contains("wallet login"),
            "expected bail message to point at TEE signing: {msg}"
        );
    }

    #[tokio::test]
    async fn sign_payment_auto_local_path_applies_saved_default_asset() {
        // End-to-end check: unauthenticated (no wallets.json) but a
        // default asset is saved in the payment cache. The auto path
        // must pick that entry instead of accepts[0].
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("sign_payment_auto_default");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);

        // Seed a default asset pointing at USDT.
        let cache = crate::payment_cache::PaymentCache {
            default_asset: Some(crate::payment_cache::PaymentDefault {
                asset: "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".into(),
                network: "eip155:196".into(),
                name: Some("USDT".into()),
            }),
            ..Default::default()
        };
        cache.save().unwrap();

        let accepts = json!([
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": "100",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            },
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": "200",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDT", "version": "1"}
            }
        ]);
        let (_proof, entry) = sign_payment_auto(&accepts, None).await.unwrap();

        std::env::remove_var("ONCHAINOS_HOME");
        std::env::remove_var("EVM_PRIVATE_KEY");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(
            entry["asset"].as_str(),
            Some("0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"),
            "auto-payment local branch must honor the saved default asset"
        );
    }

    #[tokio::test]
    async fn sign_payment_local_picks_exact_when_both_schemes_present() {
        // Even when aggr_deferred appears first in the array (and would be
        // the natural "first entry" fallback), sign_payment_local should
        // select the exact entry. This locks in scheme-priority behavior
        // for the manual-command variant, which passes `preferred = None`
        // down to `sign_payment_local_with_preference` unconditionally so
        // it signs exactly what the caller supplied via `--payload`.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let accepts = json!([
            {
                "scheme": "aggr_deferred",
                "network": "eip155:196",
                "amount": "200",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDT", "version": "1"}
            },
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": "100",
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            }
        ]);

        std::env::set_var("EVM_PRIVATE_KEY", TEST_PK);
        let (_proof, entry) = sign_payment_local(&accepts, None).await.unwrap();
        std::env::remove_var("EVM_PRIVATE_KEY");
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
        assert_eq!(
            entry["asset"].as_str(),
            Some("0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
        );
    }

    #[test]
    fn prepare_resolved_entry_selects_exact_and_collapses_tier_amount() {
        // accepts with both schemes; tiered amount object; no preferred.
        let accepts = json!([
            {
                "scheme": "aggr_deferred",
                "network": "eip155:196",
                "amount": {"basic": "100", "premium": "500"},
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0x2222222222222222222222222222222222222222",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            },
            {
                "scheme": "exact",
                "network": "eip155:196",
                "amount": {"basic": "100", "premium": "500"},
                "payTo": "0x1111111111111111111111111111111111111111",
                "asset": "0x2222222222222222222222222222222222222222",
                "maxTimeoutSeconds": 300,
                "extra": {"name": "USDG", "version": "1"}
            }
        ]);
        let (entry, params) =
            prepare_resolved_entry(&accepts, Some(PaymentTier::Basic), None).unwrap();
        // scheme priority picks exact
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
        assert_eq!(params.scheme.as_deref(), Some("exact"));
        // tier amount collapsed to scalar
        assert_eq!(params.amount, "100");
        assert_eq!(entry["amount"].as_str(), Some("100"));
        // other resolved params pass through
        assert_eq!(params.network, "eip155:196");
        assert_eq!(params.max_timeout_seconds, 300);
    }

    // ── replay_merchant status mapping (mock merchant, hermetic) ──────────
    //
    // The full `payment pay` command cannot run hermetically: the sign step
    // before the replay (`sign_payment_with_preference`) needs a logged-in
    // wallet, keyring session, and live OKX wallet-backend calls. But the
    // replay seam itself — the HTTP round-trip that maps the merchant status to
    // `success` / `pending` / `failed` — is fully mockable. These tests stand up
    // a one-shot local HTTP merchant and assert the 402 → `"pending"` and
    // 200 → `"success"` branches end-to-end through the real reqwest client
    // (closing the "402 → pending replay is only covered by unit status-mapping"
    // gap called out in review).

    /// Bind a one-shot HTTP merchant on an ephemeral loopback port that answers
    /// the first request with `status_line` + `body`, then closes. Returns the
    /// URL to hit and the server thread handle.
    fn spawn_mock_merchant(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock merchant");
        let addr = listener.local_addr().expect("mock merchant addr");
        let url = format!("http://{addr}/pay");
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Drain the request so the client's write completes before we
                // reply (a single read covers a small GET's headers).
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (url, handle)
    }

    fn dummy_proof() -> PaymentProof {
        PaymentProof::Eip3009 {
            signature: "sig".into(),
            authorization: json!({}),
            session_cert: None,
        }
    }

    #[tokio::test]
    async fn replay_merchant_maps_402_to_pending() {
        let (url, handle) = spawn_mock_merchant("402 Payment Required", r#"{"status":"settling"}"#);
        let proof = dummy_proof();
        let entry = json!({});
        let (status, _tx, result, error, _receipt) = replay_merchant(
            &url,
            "GET",
            &[],
            "PAYMENT-SIGNATURE",
            "dummy-header",
            &[],
            &proof,
            &entry,
        )
        .await;
        let _ = handle.join();
        assert_eq!(status, "pending", "402 must map to a non-terminal pending");
        assert!(
            error
                .as_deref()
                .unwrap_or_default()
                .contains("non-terminal"),
            "pending error must flag non-terminal: {error:?}"
        );
        // The merchant JSON body is surfaced verbatim for the caller/agent.
        assert_eq!(result["status"].as_str(), Some("settling"));
    }

    #[tokio::test]
    async fn replay_merchant_maps_200_to_success() {
        let (url, handle) = spawn_mock_merchant("200 OK", r#"{"ok":true}"#);
        let proof = dummy_proof();
        let entry = json!({});
        let (status, _tx, _result, error, _receipt) = replay_merchant(
            &url,
            "GET",
            &[],
            "PAYMENT-SIGNATURE",
            "dummy-header",
            &[],
            &proof,
            &entry,
        )
        .await;
        let _ = handle.join();
        assert_eq!(status, "success", "200 must map to success");
        assert!(error.is_none(), "success must carry no error: {error:?}");
    }
}
