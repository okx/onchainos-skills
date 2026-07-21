//! Cross-process `paymentId` state for the two-phase `payment quote` → `payment pay`
//! flow. A `payment quote` probes the merchant, ranks candidates, and
//! persists everything needed to *complete* the payment later to
//! `~/.onchainos/payments/{payment_id}.json`. `payment pay --payment-id` reads
//! it back, signs, and replays — without ever re-fetching the 402.
//!
//! Security invariants (PRD §6):
//! - The state file NEVER stores a private key or a signed blob — signing stays
//!   in the TEE and happens fresh in `payment pay`.
//! - An owner guard binds each file to the account that created it
//!   (`owner_wallet`), so a different logged-in account cannot complete a quote
//!   it did not create (`cross_user_payment_id`).
//! - A short TTL (`min(challenge.expires, created_at + 300)`) refuses stale
//!   quotes (`quote_expired_or_missing`).

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::home;

/// Machine token (leading word of the `output::error` message) emitted when a
/// paymentId is missing, unparseable, or past its TTL. Greppable by the skill.
pub const TOKEN_QUOTE_EXPIRED_OR_MISSING: &str = "quote_expired_or_missing";
/// Machine token emitted when the persisted `owner_wallet` does not match the
/// currently selected account — the quote belongs to a different user.
pub const TOKEN_CROSS_USER: &str = "cross_user_payment_id";

/// Max lifetime of a quote in seconds when the challenge does not pin a sooner
/// expiry. Kept short so a signed replay never rides on stale price/nonce data.
pub const MAX_QUOTE_TTL_SECS: u64 = 300;

/// One `accepts[]` entry, flattened to the fields the skill and the signer need.
/// `index` is the 0-based position in the original challenge `accepts[]` so
/// `payment pay --selected-index <n>` can pin the user's choice.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AcceptEntry {
    pub index: usize,
    pub scheme: String,
    pub amount: String,
    pub asset: String,
    pub network: String,
}

/// A ranked payment option surfaced to the user. `recommended` is
/// `Some(true)` for the auto-pick, `Some(false)` for alternatives, and `None`
/// when no candidate has a spendable balance (user must choose explicitly).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub scheme: String,
    /// 0-based back-reference to the source `accepts[]` entry this candidate was
    /// built from. Stable across `rank_candidates` reordering, so the confirming
    /// preview and `payment pay --selected-index` share one index space (the
    /// accepts[] order that the signer uses). The agent passes this value
    /// verbatim as `--selected-index`.
    #[serde(rename = "acceptsIndex")]
    pub accepts_index: usize,
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(rename = "chainName")]
    pub chain_name: String,
    #[serde(rename = "isMainnet")]
    pub is_mainnet: bool,
    #[serde(rename = "tokenSymbol")]
    pub token_symbol: String,
    /// Atomic integer amount (authoritative). Never rounded.
    pub amount: String,
    /// Display-only human amount (`amount / 10^decimals`). Never used for math.
    #[serde(rename = "amountHuman")]
    pub amount_human: String,
    #[serde(rename = "hasBalance")]
    pub has_balance: bool,
    pub recommended: Option<bool>,
}

/// The decoded 402 challenge, normalized to a stable shape. Money fields are
/// atomic-integer strings; `amount_human` is display-only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DecodedChallenge {
    pub amount: String,
    #[serde(rename = "amountHuman")]
    pub amount_human: String,
    pub decimals: u32,
    pub recipient: String,
    /// Challenge expiry as Unix seconds (0 when the challenge did not pin one).
    pub expires: u64,
    pub supported: bool,
    pub unsupported_reason: Option<String>,
}

/// Where a business param rides on the paid HTTP request (A2MCP
/// `outputSchema.input[].carrier`). `Query` is the default when
/// the schema omits a carrier, preserving the pre-carrier GET+query behavior.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ParamCarrier {
    #[default]
    Query,
    Body,
    Header,
    Path,
}

/// One resolved business-param spec from the merchant's `outputSchema.input`
/// (Source 1) or a flat required list (Source 2). Persisted so `payment pay`
/// can place each param on the right carrier when it replays the paid request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: String,
    #[serde(default)]
    pub carrier: ParamCarrier,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
}

/// Default paid-call HTTP method for legacy state files written before the
/// carrier/method work — preserves the historical GET behavior.
pub fn default_http_method() -> String {
    "GET".to_string()
}

/// Persisted quote state. NOTE: intentionally holds no key material and no
/// signed authorization — only the inputs needed to sign+replay in `pay`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PaymentState {
    pub payment_id: String,
    /// Selected account id that created the quote — cross-user guard.
    pub owner_wallet: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub accepts: Vec<AcceptEntry>,
    pub decoded_challenge: DecodedChallenge,
    pub candidates: Vec<Candidate>,
    pub known_params: Map<String, Value>,
    pub merchant_body: String,
    /// Merchant endpoint URL — replayed verbatim in `pay`.
    pub endpoint_url: String,
    /// The original challenge `accepts[]` entries, verbatim. Needed by `pay` to
    /// sign (the flattened `accepts` above drops `payTo` / `extra`). Never a key
    /// or signed blob.
    #[serde(default)]
    pub raw_accepts: Vec<Value>,
    /// The x402 v2 `resource` object from the decoded payload, echoed into the
    /// assembled `PAYMENT-SIGNATURE` header on `pay`. Absent for v1 challenges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<Value>,
    /// Paid-call HTTP method (A2MCP `outputSchema.method` / challenge `method`).
    /// Defaults to `GET` for legacy states and simple x402 endpoints.
    #[serde(default = "default_http_method")]
    pub method: String,
    /// Per-business-param carrier plan parsed from `outputSchema.input`.
    /// Empty for legacy states / endpoints that only use query params — `pay`
    /// then falls back to method-based defaults (query for GET, body otherwise).
    #[serde(default)]
    pub param_plan: Vec<ParamSpec>,
}

/// `min(challenge_expires, created_at + MAX_QUOTE_TTL_SECS)`.
///
/// When the challenge did not pin an expiry (`challenge_expires == 0`), only the
/// local ceiling applies.
pub fn compute_expires_at(challenge_expires: u64, created_at: u64) -> u64 {
    let ceiling = created_at.saturating_add(MAX_QUOTE_TTL_SECS);
    if challenge_expires == 0 {
        ceiling
    } else {
        challenge_expires.min(ceiling)
    }
}

/// `~/.onchainos/payments/`, created (0700 inherited from the home dir) on demand.
fn payments_dir() -> Result<PathBuf> {
    let dir = home::onchainos_home()?.join("payments");
    fs::create_dir_all(&dir).context("failed to create ~/.onchainos/payments")?;
    Ok(dir)
}

/// `~/.onchainos/payments/{id}.json`.
fn state_path(id: &str) -> Result<PathBuf> {
    Ok(payments_dir()?.join(format!("{id}.json")))
}

impl PaymentState {
    /// Atomic write: `to_string_pretty` → `{id}.json.tmp` → `rename`
    /// (mirrors `AppConfig::save`), so a concurrent reader never sees a
    /// half-written file.
    pub fn write(&self) -> Result<()> {
        let path = state_path(&self.payment_id)?;
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self).context("serialize payment state")?;
        fs::write(&tmp, body).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("rename into {}", path.display()))?;
        Ok(())
    }
}

/// Read the persisted state for `id`, enforcing the owner guard and TTL.
///
/// - File missing / unparseable → `quote_expired_or_missing`.
/// - `owner_wallet != current_owner` → `cross_user_payment_id` (checked first so
///   we never leak another user's quote details, expired or not).
/// - `now > expires_at` → lazy `cleanup` + `quote_expired_or_missing`.
pub fn read(id: &str, current_owner: &str, now: u64) -> Result<PaymentState> {
    let path = state_path(id)?;
    let body =
        fs::read_to_string(&path).map_err(|_| anyhow!("{TOKEN_QUOTE_EXPIRED_OR_MISSING}: {id}"))?;
    let state: PaymentState = serde_json::from_str(&body)
        .map_err(|_| anyhow!("{TOKEN_QUOTE_EXPIRED_OR_MISSING}: {id}"))?;
    if state.owner_wallet != current_owner {
        bail!("{TOKEN_CROSS_USER}: {id}");
    }
    if now > state.expires_at {
        cleanup(id);
        bail!("{TOKEN_QUOTE_EXPIRED_OR_MISSING}: {id}");
    }
    Ok(state)
}

/// Best-effort delete of the state file — on expired-read and on `pay` success.
pub fn cleanup(id: &str) {
    if let Ok(path) = state_path(id) {
        let _ = fs::remove_file(path);
    }
}

/// The currently selected account id, used as `owner_wallet`. `None` when no
/// wallet is logged in.
pub fn current_owner_id() -> Option<String> {
    let wallets = crate::wallet_store::load_wallets().ok().flatten()?;
    let id = wallets.selected_account_id;
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_challenge() -> DecodedChallenge {
        DecodedChallenge {
            amount: "10000".into(),
            amount_human: "0.01".into(),
            decimals: 6,
            recipient: "0xabc".into(),
            expires: 0,
            supported: true,
            unsupported_reason: None,
        }
    }

    fn sample_state(id: &str, owner: &str, expires_at: u64) -> PaymentState {
        PaymentState {
            payment_id: id.into(),
            owner_wallet: owner.into(),
            created_at: 1_000,
            expires_at,
            accepts: vec![AcceptEntry {
                index: 0,
                scheme: "exact".into(),
                amount: "10000".into(),
                asset: "USDC".into(),
                network: "eip155:8453".into(),
            }],
            decoded_challenge: sample_challenge(),
            candidates: vec![],
            known_params: Map::new(),
            merchant_body: "body".into(),
            endpoint_url: "https://merchant.example/x".into(),
            raw_accepts: vec![],
            resource: None,
            method: default_http_method(),
            param_plan: vec![],
        }
    }

    /// Serialize env-mutating tests behind the shared home mutex + a unique dir.
    fn with_home<F: FnOnce()>(sub: &str, f: F) {
        let _lock = home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        let _ = fs::remove_dir_all(&dir);
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_then_read_round_trip() {
        with_home("state_round_trip", || {
            let st = sample_state("pid-rt", "acc-1", 9_999_999_999);
            st.write().unwrap();
            let got = read("pid-rt", "acc-1", 2_000).unwrap();
            assert_eq!(got.payment_id, "pid-rt");
            assert_eq!(got.owner_wallet, "acc-1");
            assert_eq!(got.accepts, st.accepts);
            assert_eq!(got.endpoint_url, st.endpoint_url);
        });
    }

    #[test]
    fn ttl_is_min_of_expires_and_five_minutes() {
        // challenge expiry sooner than the 5-min ceiling → challenge wins.
        assert_eq!(compute_expires_at(1_100, 1_000), 1_100);
        // challenge expiry later than the ceiling → ceiling (created_at+300) wins.
        assert_eq!(compute_expires_at(9_000, 1_000), 1_300);
        // no challenge expiry → ceiling only.
        assert_eq!(compute_expires_at(0, 1_000), 1_300);
    }

    #[test]
    fn expired_read_returns_token_and_cleans_up() {
        with_home("state_expired", || {
            let st = sample_state("pid-exp", "acc-1", 1_500);
            st.write().unwrap();
            // now (2000) > expires_at (1500) → quote_expired_or_missing + cleanup.
            let err = read("pid-exp", "acc-1", 2_000).unwrap_err();
            assert!(err.to_string().starts_with(TOKEN_QUOTE_EXPIRED_OR_MISSING));
            // File was cleaned up: a second read still reports missing.
            let err2 = read("pid-exp", "acc-1", 2_000).unwrap_err();
            assert!(err2.to_string().starts_with(TOKEN_QUOTE_EXPIRED_OR_MISSING));
        });
    }

    #[test]
    fn owner_mismatch_returns_cross_user_token() {
        with_home("state_owner", || {
            let st = sample_state("pid-own", "acc-1", 9_999_999_999);
            st.write().unwrap();
            let err = read("pid-own", "acc-2", 2_000).unwrap_err();
            assert!(err.to_string().starts_with(TOKEN_CROSS_USER));
        });
    }

    #[test]
    fn missing_file_returns_expired_or_missing() {
        with_home("state_missing", || {
            let err = read("does-not-exist", "acc-1", 2_000).unwrap_err();
            assert!(err.to_string().starts_with(TOKEN_QUOTE_EXPIRED_OR_MISSING));
        });
    }
}
