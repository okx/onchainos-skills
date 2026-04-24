//! Shared x402 payment signing.
//!
//! Used by:
//! - `onchainos wallet x402-pay` (manual signing, prints JSON proof).
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
#[derive(Debug)]
pub struct PaymentProof {
    /// Base64 EIP-3009 signature (for `exact` scheme) or base64 Ed25519 session
    /// signature (for `aggr_deferred` scheme).
    pub signature: String,
    /// EIP-3009 authorization fields echoed back to the payer.
    pub authorization: Value,
    /// Session cert — populated only for `aggr_deferred` (the server needs it to
    /// verify the Ed25519 signature).
    pub session_cert: Option<String>,
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

pub(crate) struct ResolvedEntry {
    pub(crate) network: String,
    pub(crate) amount: String,
    pub(crate) pay_to: String,
    pub(crate) asset: String,
    pub(crate) max_timeout_seconds: u64,
    pub(crate) scheme: Option<String>,
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
    let pay_to = entry["payTo"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'payTo' in accepts entry"))?
        .to_string();
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
pub async fn sign_payment(
    accepts: &Value,
    from: Option<&str>,
    tier: Option<PaymentTier>,
) -> Result<(PaymentProof, Value)> {
    let preferred = crate::payment_cache::PaymentCache::load().and_then(|c| c.default_asset);
    sign_payment_with_preference(accepts, from, tier, preferred.as_ref()).await
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

/// Variant of `sign_payment` that signs exactly what the caller's
/// `accepts` says, without consulting the saved default asset. Used by
/// the manual `onchainos payment x402-pay` command so the user-supplied
/// `--accepts` isn't silently reordered by a stored preference.
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
        .context("x402 gen-msg-hash failed")?;
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
            PaymentProof {
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
            .context("x402 sign-msg failed")?;
        let eip3009_signature = signed_hash_resp[0]["signature"]
            .as_str()
            .ok_or_else(|| anyhow!("missing signature in sign-msg response"))?;

        Ok((
            PaymentProof {
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
/// default asset. Used by the manual `payment eip3009-sign` command,
/// which inherits `x402-pay`'s "sign what --accepts says" contract so
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
/// `prepare_resolved_entry`. If `preferred` matches an accepts entry
/// whose scheme is `aggr_deferred` (a scheme we can't sign locally),
/// this falls back to scheme priority to pick any available `exact`
/// entry rather than failing — so the saved default wins when possible,
/// but never blocks progress when the only matching offering is a
/// scheme we can't use.
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

    // First pass honors the saved default. If that picks an
    // aggr_deferred-only entry (either because the default points at one,
    // or because the server only offered aggr_deferred for that asset),
    // retry without the preference so scheme priority finds any exact
    // entry elsewhere in `accepts`.
    let (entry, params) = {
        let (e, p) = prepare_resolved_entry(accepts, tier, preferred)?;
        if is_deferred(&p.scheme) && preferred.is_some() {
            prepare_resolved_entry(accepts, tier, None)?
        } else {
            (e, p)
        }
    };

    if is_deferred(&params.scheme) {
        bail!(
            "local private-key signing requires an 'exact' scheme accepts entry, \
             but the server only offered 'aggr_deferred' (session key required). \
             Run `onchainos wallet login` to enable TEE signing."
        );
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
        PaymentProof {
            signature,
            authorization,
            session_cert: None,
        },
        entry,
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
        "[onchainos] x402 signed locally with EVM_PRIVATE_KEY (NOT protected by TEE); \
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
    let payload_inner = json!({
        "signature": proof.signature,
        "authorization": proof.authorization,
    });

    // Embed sessionCert into entry.extra for aggr_deferred (server needs it).
    let mut accepted = entry.clone();
    if let Some(cert) = &proof.session_cert {
        if let Some(obj) = accepted.as_object_mut() {
            let extra = obj.entry("extra".to_string()).or_insert_with(|| json!({}));
            if let Some(extra_obj) = extra.as_object_mut() {
                extra_obj.insert("sessionCert".into(), json!(cert));
            }
        }
    }

    let body = json!({
        "x402Version": 2,
        "resource": {
            "url": resource,
            "mimeType": "application/json",
        },
        "accepted": accepted,
        "payload": payload_inner,
    });

    let encoded = B64.encode(serde_json::to_vec(&body).context("encode payment header body")?);
    Ok(("PAYMENT-SIGNATURE", encoded))
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let v = json!({"network":"eip155:1","maxAmountRequired":"999","payTo":"0xA","asset":"0xB"});
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.amount, "999");
    }

    #[test]
    fn resolve_entry_default_timeout() {
        let v = json!({"network":"eip155:1","amount":"1","payTo":"0xA","asset":"0xB"});
        let r = resolve_entry(&v, None, None).unwrap();
        assert_eq!(r.max_timeout_seconds, 300);
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
            "payTo": "0xA",
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
    fn build_payment_header_includes_resource() {
        let proof = PaymentProof {
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
        let proof = PaymentProof {
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

        // Hallmarks of the local path:
        assert!(proof.session_cert.is_none());
        assert!(proof.signature.starts_with("0x"));
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
        assert_eq!(proof.authorization["from"].as_str(), Some(TEST_PK_ADDR));
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

        // Signature shape: 0x + 130 hex chars (65 bytes r||s||v).
        assert!(proof.signature.starts_with("0x"));
        assert_eq!(proof.signature.len(), 2 + 130);
        assert!(proof.session_cert.is_none());
        assert_eq!(proof.authorization["from"].as_str(), Some(TEST_PK_ADDR));
        assert_eq!(
            proof.authorization["to"].as_str(),
            Some("0x1111111111111111111111111111111111111111")
        );
        assert_eq!(proof.authorization["value"].as_str(), Some("1000000"));
        assert_eq!(proof.authorization["validAfter"].as_str(), Some("0"));
        // selected entry is the exact entry
        assert_eq!(entry["scheme"].as_str(), Some("exact"));
    }

    #[tokio::test]
    async fn sign_payment_local_rejects_aggr_deferred_only() {
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
        // it signs exactly what the caller supplied via `--accepts`.
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
}
