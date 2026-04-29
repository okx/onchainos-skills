use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::doh::DohManager;

/// Structured error for non-zero API response codes.
/// Preserves the original backend `code` and `msg` so callers can
/// output them directly via `output::error`.
#[derive(Debug)]
pub struct ApiCodeError {
    pub code: String,
    pub msg: String,
}

impl std::fmt::Display for ApiCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wallet API error (code={}): {}", self.code, self.msg)
    }
}

impl std::error::Error for ApiCodeError {}

/// Deserialize a value that may be null, a string, or a number into a String.
/// null → "".
fn string_or_number<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {}",
            other
        ))),
    }
}

/// Deserialize a value that may be null, a bool, or an integer (0/1) into a bool.
/// null → false.
fn bool_or_int<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(b),
        Value::Number(n) => Ok(n.as_i64().unwrap_or(0) != 0),
        other => Err(serde::de::Error::custom(format!(
            "expected bool or integer, got {}",
            other
        ))),
    }
}

/// Deserialize a nullable string: null → "".
fn nullable_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s),
        other => Err(serde::de::Error::custom(format!(
            "expected string or null, got {}",
            other
        ))),
    }
}

fn nullable_bool<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(b),
        other => Err(serde::de::Error::custom(format!(
            "expected bool or null, got {}",
            other
        ))),
    }
}

fn nullable_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let v: Option<Vec<T>> = Option::deserialize(deserializer)?;
    Ok(v.unwrap_or_default())
}

/// Build a URL-encoded query string from key-value pairs, filtering out empty values.
fn build_query_string(query: &[(&str, &str)]) -> String {
    let filtered: Vec<(&str, &str)> = query
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .copied()
        .collect();
    if filtered.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = filtered
        .iter()
        .map(|(k, v)| {
            let encoded_value = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("", v)
                .finish();
            // encoded_value starts with "=", strip the leading "="
            format!("{}={}", k, &encoded_value[1..])
        })
        .collect();
    format!("?{}", pairs.join("&"))
}

/// HTTP client for the agentic-wallet API endpoints.
pub struct WalletApiClient {
    http: Client,
    base_url: String,
    doh: DohManager,
}

// ── API response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitResponse {
    pub flow_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub refresh_token: String,
    pub access_token: String,
    pub tee_id: String,
    pub session_cert: String,
    pub encrypted_session_sk: String,
    #[serde(deserialize_with = "string_or_number")]
    pub session_key_expire_at: String,
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(deserialize_with = "bool_or_int")]
    pub is_new: bool,
    #[serde(default)]
    pub address_list: Vec<VerifyAddressInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyAddressInfo {
    #[serde(default)]
    pub account_id: String,
    pub address: String,
    #[serde(deserialize_with = "string_or_number")]
    pub chain_index: String,
    pub chain_name: String,
    pub address_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub chain_path: String,
}

#[derive(Debug, Deserialize)]
pub struct AkInitResponse {
    pub nonce: String,
    pub iss: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAccountItem {
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub addresses: Vec<VerifyAddressInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResponse {
    pub refresh_token: String,
    pub access_token: String,
    #[serde(default)]
    pub chain_updated: bool,
    #[serde(default)]
    pub all_account_address_list: Vec<RefreshAccountItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountResponse {
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub address_list: Vec<VerifyAddressInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountListItem {
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub is_default: bool,
}

/// Per-account entry from the address/list API.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressListAccountItem {
    pub account_id: String,
    #[serde(default)]
    pub addresses: Vec<VerifyAddressInfo>,
}

/// Wrapper for the account/address/list response `data` object.
/// `{ "accountCnt": N, "validAccountCnt": N, "addressCnt": N, "accounts": [...] }`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressListData {
    #[serde(default)]
    accounts: Vec<AddressListAccountItem>,
}

/// Gas Station status enum (mirrors the backend `gasStationStatus` field; see review.md Section 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GasStationStatus {
    /// Native-token transfer or unsupported chain — Gas Station is not used for this tx.
    NotApplicable,
    /// DB has no record + chain not delegated — user must pick a token to enable for the first time.
    FirstTimePrompt,
    /// Chain not delegated — sign 712 + 7702 auth to perform the first-time upgrade.
    PendingUpgrade,
    /// DB disabled + chain already delegated — flip the DB flag only; no on-chain action.
    ReenableOnly,
    /// DB enabled + chain already delegated — steady-state path; check whether `hash` is empty to decide if the default token covers this tx.
    ReadyToUse,
    /// None of the Gas Station stablecoins has enough balance to cover the gas fee.
    InsufficientAll,
    /// A pending Gas Station transaction is blocking this one.
    HasPendingTx,
    /// Enum value is unknown or empty — compatibility fallback for older backends.
    Unknown,
}

impl GasStationStatus {
    /// Keep as an infallible convenience parser for backward-compat; new code should prefer
    /// `FromStr` (`s.parse::<GasStationStatus>()` — which also never fails, just maps unknown
    /// values to `Unknown`).
    pub fn parse(s: &str) -> Self {
        s.parse().unwrap_or(Self::Unknown)
    }

    /// Canonical wire-format string for this variant. Inverse of `FromStr` for known values;
    /// `Unknown` renders as the empty string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotApplicable => "NOT_APPLICABLE",
            Self::FirstTimePrompt => "FIRST_TIME_PROMPT",
            Self::PendingUpgrade => "PENDING_UPGRADE",
            Self::ReenableOnly => "REENABLE_ONLY",
            Self::ReadyToUse => "READY_TO_USE",
            Self::InsufficientAll => "INSUFFICIENT_ALL",
            Self::HasPendingTx => "HAS_PENDING_TX",
            Self::Unknown => "",
        }
    }
}

impl std::str::FromStr for GasStationStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "NOT_APPLICABLE" => Self::NotApplicable,
            "FIRST_TIME_PROMPT" => Self::FirstTimePrompt,
            "PENDING_UPGRADE" => Self::PendingUpgrade,
            "REENABLE_ONLY" => Self::ReenableOnly,
            "READY_TO_USE" => Self::ReadyToUse,
            "INSUFFICIENT_ALL" => Self::InsufficientAll,
            "HAS_PENDING_TX" => Self::HasPendingTx,
            _ => Self::Unknown,
        })
    }
}

impl std::fmt::Display for GasStationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsignedInfoResponse {
    #[serde(default, deserialize_with = "nullable_string")]
    pub unsigned_tx_hash: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub unsign_hash: String, // Solana uses this instead of unsignedTxHash
    #[serde(default, deserialize_with = "nullable_string")]
    pub unsigned_tx: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub uop_hash: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub hash: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub auth_hash_for7702: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub execute_error_msg: String,
    #[serde(default)]
    pub execute_result: Value,
    #[serde(default)]
    pub extra_data: Value,
    #[serde(default, deserialize_with = "nullable_string")]
    pub sign_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub encoding: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub jito_unsigned_tx: String,
    /// backend 返的 712 message hash（contract-call 等场景）；非空时客户端需 ed25519_sign_encoded 算 sessionSignature
    #[serde(default, deserialize_with = "nullable_string")]
    pub eip712_message_hash: String,
    // ── Gas Station fields ──
    #[serde(default, deserialize_with = "nullable_bool")]
    pub gas_station_used: bool,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub gas_station_first_time_prompt: bool,
    #[serde(default, deserialize_with = "nullable_string")]
    pub service_charge: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub service_charge_symbol: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub service_charge_fee_token_address: String,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub need_update7702: bool,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub gas_station_token_list: Vec<GasStationToken>,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub has_pending_tx: bool,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub insufficient_all: bool,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub auto_selected_token: bool,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub gas_station_disabled: bool,
    #[serde(default, deserialize_with = "nullable_string")]
    pub gas_station_status: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub contract_nonce: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub eoa_nonce: String,
    #[serde(default)]
    pub user712_data: Value,
    #[serde(default)]
    pub user7702_data: Value,
    /// User's default gas token address on this chain (Phase 1 response; may be empty).
    /// CLI matches it against `gas_station_token_list`: hit + sufficient -> Scene B (auto
    /// Phase 2); otherwise -> Scene C (user picks a token).
    #[serde(default, deserialize_with = "nullable_string")]
    pub default_gas_token_address: String,
}

impl UnsignedInfoResponse {
    /// 解析后端返回的 gasStationStatus 字符串为枚举
    pub fn gs_status(&self) -> GasStationStatus {
        GasStationStatus::parse(&self.gas_station_status)
    }

    /// Find the entry in `gas_station_token_list` whose `fee_token_address` matches
    /// `default_gas_token_address` AND has `sufficient=true`. Returns a reference on hit so
    /// the CLI can run Scene B auto Phase 2 with that token.
    pub fn match_default_sufficient_token(&self) -> Option<&GasStationToken> {
        if self.default_gas_token_address.is_empty() {
            return None;
        }
        self.gas_station_token_list.iter().find(|t| {
            t.sufficient
                && t.fee_token_address.eq_ignore_ascii_case(&self.default_gas_token_address)
        })
    }

    /// When there is no default token but `gas_station_token_list` has exactly one
    /// `sufficient=true` entry, return it. Used as Scene B's "unambiguous fallback": the
    /// user has no other choice, so a manual pick would produce the same result. Skipping
    /// the Confirming round trip also lets downstream callers that don't understand
    /// `CliConfirming` (e.g. third-party plugins) complete successfully.
    ///
    /// Callers should only invoke this when `default_gas_token_address` is empty (see
    /// `auto_pick_gas_token`). Returns `None` (continue to Scene C and ask the user) when:
    ///   - 0 sufficient entries (already handled as insufficient_all upstream)
    ///   - 2+ sufficient entries (user must choose; we won't decide for them)
    pub fn only_sufficient_token(&self) -> Option<&GasStationToken> {
        let mut iter = self.gas_station_token_list.iter().filter(|t| t.sufficient);
        let first = iter.next()?;
        if iter.next().is_none() {
            Some(first)
        } else {
            None
        }
    }

    /// Unified entry point for Scene B auto token selection. Auto-selects in two
    /// unambiguous cases:
    ///   1. A default is set AND it hits the token list AND is sufficient (original Scene B).
    ///   2. **No default** AND exactly one sufficient token (plugin-compat fallback; no
    ///      default = no user preference).
    ///
    /// Explicitly excludes Scene 2a (default present but insufficient) — when a default is
    /// set, it represents an explicit user preference. Even if only one alternative is
    /// sufficient, route to Scene C so the user is told "your default is short, can we use
    /// XXX instead?"; do not silently override the preference.
    pub fn auto_pick_gas_token(&self) -> Option<&GasStationToken> {
        if self.default_gas_token_address.is_empty() {
            // No default → no user preference; safely auto-pick when there's exactly one option.
            self.only_sufficient_token()
        } else {
            // Default set → either hit & sufficient (Scene B), or route to Scene C (never silent override).
            self.match_default_sufficient_token()
        }
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GasStationToken {
    #[serde(default)]
    pub fee_coin_id: u64,
    #[serde(default, deserialize_with = "nullable_string")]
    pub symbol: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub fee_token_address: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub service_charge: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub balance: String,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub sufficient: bool,
    #[serde(default, deserialize_with = "nullable_string")]
    pub relayer_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub context: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastResponse {
    #[serde(default, deserialize_with = "nullable_string")]
    pub pkg_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub order_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub order_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub tx_hash: String,
}

impl WalletApiClient {
    pub fn new() -> Result<Self> {
        let base_url = std::env::var("OKX_BASE_URL")
            .ok()
            .or_else(|| option_env!("OKX_BASE_URL").map(|s| s.to_string()))
            .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string());

        let custom = std::env::var("OKX_BASE_URL").is_ok() || option_env!("OKX_BASE_URL").is_some();
        let mut doh = DohManager::new("web3.okx.com", &base_url, custom);
        doh.prepare();

        let mut builder = Client::builder().timeout(std::time::Duration::from_secs(30));
        if let Some((host, addr)) = doh.resolve_override() {
            builder = builder.resolve(&host, addr);
        }
        if doh.is_proxy() {
            builder = builder.user_agent(doh.doh_user_agent());
        }

        Ok(Self {
            http: builder.build()?,
            base_url,
            doh,
        })
    }

    fn rebuild_http_client(&mut self) -> Result<()> {
        let mut builder = Client::builder().timeout(std::time::Duration::from_secs(30));
        if let Some((host, addr)) = self.doh.resolve_override() {
            builder = builder.resolve(&host, addr);
        }
        if self.doh.is_proxy() {
            builder = builder.user_agent(self.doh.doh_user_agent());
        }
        self.http = builder.build()?;
        Ok(())
    }

    fn effective_base_url(&self) -> String {
        self.doh
            .proxy_base_url()
            .unwrap_or_else(|| self.base_url.clone())
    }

    // ── Low-level POST helpers ──────────────────────────────────────

    /// POST without Authorization header (for init / verify / refresh).
    /// Retries once after DoH failover.
    pub fn post_public<'a>(
        &'a mut self,
        path: &'a str,
        body: &'a Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            let effective = self.effective_base_url();
            let url = format!("{}{}", effective.trim_end_matches('/'), path);

            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post_public] url_path={}", &url);
            }

            let resp = match self
                .http
                .post(&url)
                .headers(crate::client::ApiClient::anonymous_headers())
                .json(body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if self.doh.handle_failure().await {
                        self.rebuild_http_client()?;
                        return self.post_public(path, body).await;
                    }
                    return Err(e)
                        .context("Network unavailable — check your connection and try again");
                }
                Err(e) => return Err(e).context("request failed"),
            };
            self.doh.cache_direct_if_needed();
            self.handle_response(resp).await
        })
    }

    /// Retries once after DoH failover.
    pub async fn post_authed(
        &mut self,
        path: &str,
        access_token: &str,
        body: &Value,
    ) -> Result<Value> {
        self.post_authed_with_headers(path, access_token, body, None)
            .await
    }

    /// Retries once after DoH failover.
    pub fn post_authed_with_headers<'a>(
        &'a mut self,
        path: &'a str,
        access_token: &'a str,
        body: &'a Value,
        extra_headers: Option<&'a [(&'a str, &'a str)]>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            let effective = self.effective_base_url();
            let url = format!("{}{}", effective.trim_end_matches('/'), path);

            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][post_authed] url_path={}", &url);
            }

            let mut headers = crate::client::ApiClient::jwt_headers(access_token);
            if let Some(extra) = extra_headers {
                for (k, v) in extra {
                    if let (Ok(name), Ok(val)) = (
                        reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                        reqwest::header::HeaderValue::from_str(v),
                    ) {
                        headers.insert(name, val);
                    }
                }
            }

            let resp = match self
                .http
                .post(&url)
                .headers(headers)
                .json(body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if self.doh.handle_failure().await {
                        self.rebuild_http_client()?;
                        return self
                            .post_authed_with_headers(path, access_token, body, extra_headers)
                            .await;
                    }
                    return Err(e)
                        .context("Network unavailable — check your connection and try again");
                }
                Err(e) => return Err(e).context("request failed"),
            };
            self.doh.cache_direct_if_needed();
            self.handle_response(resp).await
        })
    }

    async fn post_authed_no_retry_with_headers(
        &mut self,
        path: &str,
        access_token: &str,
        body: &Value,
        extra_headers: Option<&[(&str, &str)]>,
    ) -> Result<Value> {
        let effective = self.effective_base_url();
        let url = format!("{}{}", effective.trim_end_matches('/'), path);

        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][post_authed_no_retry] url_path={}", &url);
        }

        let mut headers = crate::client::ApiClient::jwt_headers(access_token);
        if let Some(extra) = extra_headers {
            for (k, v) in extra {
                if let (Ok(name), Ok(val)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(v),
                ) {
                    headers.insert(name, val);
                }
            }
        }

        let resp = match self
            .http
            .post(&url)
            .headers(headers)
            .json(body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_timeout() => {
                let _ = self.doh.handle_failure().await;
                if self.doh.is_proxy() {
                    let _ = self.rebuild_http_client();
                }
                return Err(e).context("Network error during broadcast — transaction was NOT sent. Safe to retry the same command.");
            }
            Err(e) => return Err(e).context("request failed"),
        };
        self.doh.cache_direct_if_needed();
        self.handle_response(resp).await
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        let raw_text = resp.text().await.context("failed to read response body")?;

        if status.as_u16() >= 500 {
            bail!("Wallet API server error (HTTP {}): {}", status.as_u16(), &raw_text);
        }

        let body: Value = serde_json::from_str(&raw_text)
            .context("failed to parse wallet API response")?;

        // Handle code as either string "0" or number 0
        let code_ok = match &body["code"] {
            Value::String(s) => s == "0",
            Value::Number(n) => n.as_i64() == Some(0),
            _ => false,
        };
        if !code_ok {
            let code_str = match &body["code"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            let msg = body["msg"].as_str().unwrap_or("unknown error").to_string();
            return Err(ApiCodeError {
                code: code_str,
                msg,
            }
            .into());
        }

        Ok(body["data"].clone())
    }

    /// GET without Authorization header (for public buyer payment links etc).
    /// Retries once after DoH failover.
    pub fn get_public<'a>(
        &'a mut self,
        path: &'a str,
        query: &'a [(&'a str, &'a str)],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            let query_string = build_query_string(query);
            let effective = self.effective_base_url();
            let url = format!("{}{}{}", effective.trim_end_matches('/'), path, query_string);

            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][get_public] url_path={}", &url);
            }

            let resp = match self
                .http
                .get(&url)
                .headers(crate::client::ApiClient::anonymous_headers())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if self.doh.handle_failure().await {
                        self.rebuild_http_client()?;
                        return self.get_public(path, query).await;
                    }
                    return Err(e).context("Network unavailable — check your connection and try again");
                }
                Err(e) => return Err(e).context("request failed"),
            };
            self.doh.cache_direct_if_needed();
            self.handle_response(resp).await
        })
    }

    pub fn get_authed<'a>(
        &'a mut self,
        path: &'a str,
        access_token: &'a str,
        query: &'a [(&'a str, &'a str)],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            let query_string = build_query_string(query);
            let effective = self.effective_base_url();
            let url = format!(
                "{}{}{}",
                effective.trim_end_matches('/'),
                path,
                query_string
            );
            let resp = match self
                .http
                .get(&url)
                .headers(crate::client::ApiClient::jwt_headers(access_token))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if self.doh.handle_failure().await {
                        self.rebuild_http_client()?;
                        return self.get_authed(path, access_token, query).await;
                    }
                    return Err(e)
                        .context("Network unavailable — check your connection and try again");
                }
                Err(e) => return Err(e).context("request failed"),
            };
            self.doh.cache_direct_if_needed();
            self.handle_response(resp).await
        })
    }

    // ── Public API methods ──────────────────────────────────────────

    /// POST /priapi/v5/wallet/agentic/auth/init
    pub async fn auth_init(&mut self, email: &str, locale: Option<&str>) -> Result<InitResponse> {
        let mut body = json!({ "email": email });
        if let Some(loc) = locale {
            body["locale"] = serde_json::Value::String(loc.to_string());
        }
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/init", &body)
            .await?;
        // data is an array, take first element
        let arr = data
            .as_array()
            .context("auth/init: expected data to be an array")?;
        let item = arr.first().context("auth/init: data array is empty")?;
        let resp: InitResponse =
            serde_json::from_value(item.clone()).context("auth/init: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/verify
    pub async fn auth_verify(
        &mut self,
        email: &str,
        flow_id: &str,
        otp: &str,
        temp_pub_key: &str,
    ) -> Result<VerifyResponse> {
        let body = json!({
            "email": email,
            "flowId": flow_id,
            "otp": otp,
            "tempPubKey": temp_pub_key,
        });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/verify", &body)
            .await?;
        let arr = data
            .as_array()
            .context("auth/verify: expected data to be an array")?;
        let item = arr.first().context("auth/verify: data array is empty")?;
        let resp: VerifyResponse = serde_json::from_value(item.clone())
            .context("auth/verify: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/ak/init
    pub async fn ak_auth_init(&mut self, api_key: &str) -> Result<AkInitResponse> {
        let body = json!({ "apiKey": api_key });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/ak/init", &body)
            .await?;
        let arr = data
            .as_array()
            .context("ak/init: expected data to be an array")?;
        let item = arr.first().context("ak/init: data array is empty")?;
        let resp: AkInitResponse =
            serde_json::from_value(item.clone()).context("ak/init: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/ak/verify
    pub async fn ak_auth_verify(
        &mut self,
        temp_pub_key: &str,
        api_key: &str,
        passphrase: &str,
        timestamp: &str,
        sign: &str,
        locale: &str,
    ) -> Result<VerifyResponse> {
        let body = json!({
            "tempPubKey": temp_pub_key,
            "apiKey": api_key,
            "passphrase": passphrase,
            "timestamp": timestamp,
            "sign": sign,
            "locale": locale,
        });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/ak/verify", &body)
            .await?;
        let arr = data
            .as_array()
            .context("ak/verify: expected data to be an array")?;
        let item = arr.first().context("ak/verify: data array is empty")?;
        let resp: VerifyResponse =
            serde_json::from_value(item.clone()).context("ak/verify: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/refresh
    pub async fn auth_refresh(&mut self, refresh_token: &str) -> Result<RefreshResponse> {
        let body = json!({ "refreshToken": refresh_token });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/refresh", &body)
            .await?;
        let arr = data
            .as_array()
            .context("auth/refresh: expected data to be an array")?;
        let item = arr.first().context("auth/refresh: data array is empty")?;
        let resp: RefreshResponse = serde_json::from_value(item.clone())
            .context("auth/refresh: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/account/create
    pub async fn account_create(
        &mut self,
        access_token: &str,
        project_id: &str,
    ) -> Result<CreateAccountResponse> {
        let body = json!({
            "projectId": project_id,
        });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/account/create",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("account/create: expected data to be an array")?;
        let item = arr.first().context("account/create: data array is empty")?;
        let resp: CreateAccountResponse = serde_json::from_value(item.clone())
            .context("account/create: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/account/list
    pub async fn account_list(
        &mut self,
        access_token: &str,
        project_id: &str,
    ) -> Result<Vec<AccountListItem>> {
        let body = json!({ "projectId": project_id });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/account/list",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("account/list: expected data to be an array")?;
        let items: Vec<AccountListItem> = serde_json::from_value(Value::Array(arr.clone()))
            .context("account/list: failed to parse response")?;
        Ok(items)
    }

    /// POST /priapi/v5/wallet/agentic/account/address/list
    ///
    /// Batch-fetch address lists for multiple accounts.
    pub async fn account_address_list(
        &mut self,
        access_token: &str,
        account_ids: &[String],
    ) -> Result<Vec<AddressListAccountItem>> {
        let body = json!({ "accountIds": account_ids });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/account/address/list",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("account/address/list: expected data to be an array")?;
        let item = arr
            .first()
            .context("account/address/list: data array is empty")?;
        let resp: AddressListData = serde_json::from_value(item.clone())
            .context("account/address/list: failed to parse response")?;
        Ok(resp.accounts)
    }

    // ── Balance API methods ─────────────────────────────────────────

    /// GET /priapi/v5/wallet/agentic/asset/wallet-all-token-balances-batch
    ///
    /// Fetch balances for multiple accounts at once.
    pub async fn balance_batch(&mut self, access_token: &str, account_ids: &str) -> Result<Value> {
        self.get_authed(
            "/priapi/v5/wallet/agentic/asset/wallet-all-token-balances-batch",
            access_token,
            &[("accountIds", account_ids)],
        )
        .await
    }

    /// GET /priapi/v5/wallet/agentic/asset/wallet-all-token-balances
    ///
    /// Fetch balances for a single account with optional chain / token filters.
    pub async fn balance_single(
        &mut self,
        access_token: &str,
        query: &[(&str, &str)],
    ) -> Result<Value> {
        self.get_authed(
            "/priapi/v5/wallet/agentic/asset/wallet-all-token-balances",
            access_token,
            query,
        )
        .await
    }

    /// POST /priapi/v5/wallet/agentic/token/get-token-info
    ///
    /// Fetch token metadata (decimal/name/symbol/...) via the wallet-side endpoint.
    /// Use this instead of the DEX `token/basic-info` for chains not covered by DEX
    /// (e.g. Tempo). `source = 0` (web3).
    pub async fn get_token_info(
        &mut self,
        access_token: &str,
        chain_index: u64,
        token_address: &str,
    ) -> Result<Value> {
        let body = json!({
            "chainIndex": chain_index,
            "tokenAddress": token_address,
            "source": 0,
        });
        self.post_authed(
            "/priapi/v5/wallet/agentic/token/get-token-info",
            access_token,
            &body,
        )
        .await
    }

    /// POST /priapi/v5/wallet/agentic/pre-transaction/unsignedInfo
    #[allow(clippy::too_many_arguments)]
    pub async fn pre_transaction_unsigned_info(
        &mut self,
        access_token: &str,
        chain_path: &str,
        chain_index: u64,
        from_addr: &str,
        to_addr: &str,
        amount: &str,
        contract_addr: Option<&str>,
        session_cert: &str,
        input_data: Option<&str>,
        unsigned_tx: Option<&str>,
        gas_limit: Option<&str>,
        aa_dex_token_addr: Option<&str>,
        aa_dex_token_amount: Option<&str>,
        jito_unsigned_tx: Option<&str>,
        trace_headers: Option<&[(&str, &str)]>,
        // Gas Station params
        enable_gas_station: Option<bool>,
        gas_token_address: Option<&str>,
        relayer_id: Option<&str>,
    ) -> Result<UnsignedInfoResponse> {
        let mut body = json!({
            "chainPath": chain_path,
            "chainIndex": chain_index,
            "fromAddr": from_addr,
            "toAddr": to_addr,
            "amount": amount,
            "sessionCert": session_cert,
        });
        if let Some(ca) = contract_addr {
            body["contractAddr"] = Value::String(ca.to_string());
        }
        if let Some(data) = input_data {
            body["inputData"] = Value::String(data.to_string());
        }
        if let Some(tx) = unsigned_tx {
            body["unsignedTx"] = Value::String(tx.to_string());
        }
        if let Some(gl) = gas_limit {
            body["gasLimit"] = Value::String(gl.to_string());
        }
        if let Some(addr) = aa_dex_token_addr {
            body["aaDexTokenAddr"] = Value::String(addr.to_string());
        }
        if let Some(amount) = aa_dex_token_amount {
            body["aaDexTokenAmount"] = Value::String(amount.to_string());
        }
        if let Some(jito_tx) = jito_unsigned_tx {
            body["jitoUnsignedTx"] = Value::String(jito_tx.to_string());
        }
        // Gas Station params
        if let Some(true) = enable_gas_station {
            body["enableGasStation"] = json!(true);
        }
        if let Some(addr) = gas_token_address {
            body["gasTokenAddress"] = Value::String(addr.to_string());
        }
        if let Some(rid) = relayer_id {
            body["relayerId"] = Value::String(rid.to_string());
        }
        let data = self
            .post_authed_with_headers(
                "/priapi/v5/wallet/agentic/pre-transaction/unsignedInfo",
                access_token,
                &body,
                trace_headers,
            )
            .await?;
        let arr = data
            .as_array()
            .context("unsignedInfo: expected data to be an array")?;
        let item = arr.first().context("unsignedInfo: data array is empty")?;
        let resp: UnsignedInfoResponse = serde_json::from_value(item.clone())
            .context("unsignedInfo: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/pre-transaction/report-plugin-info
    pub async fn report_plugin_info(
        &mut self,
        access_token: &str,
        plugin_parameter: &str,
    ) -> Result<Value> {
        let body = json!({
            "pluginParameter": plugin_parameter,
        });
        self.post_authed(
            "/priapi/v5/wallet/agentic/pre-transaction/report-plugin-info",
            access_token,
            &body,
        )
        .await
    }

    /// POST /priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction
    pub async fn broadcast_transaction(
        &mut self,
        access_token: &str,
        account_id: &str,
        address: &str,
        chain_index: &str,
        extra_data: &str,
        trace_headers: Option<&[(&str, &str)]>,
    ) -> Result<BroadcastResponse> {
        let body = json!({
            "accountId": account_id,
            "address": address,
            "chainIndex": chain_index,
            "extraData": extra_data,
        });
        let data = self
            .post_authed_no_retry_with_headers(
                "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
                access_token,
                &body,
                trace_headers,
            )
            .await?;
        let arr = data
            .as_array()
            .context("broadcast: expected data to be an array")?;
        let item = arr.first().context("broadcast: data array is empty")?;
        let resp: BroadcastResponse =
            serde_json::from_value(item.clone()).context("broadcast: failed to parse response")?;
        Ok(resp)
    }

    // ── Gas Station management APIs ────────────────────────────────

    /// POST /priapi/v5/wallet/agentic/gas-station/update-default-token
    pub async fn gas_station_update_default_token(
        &mut self,
        access_token: &str,
        chain_index: &str,
        gas_token_address: &str,
        from_addr: &str,
    ) -> Result<Value> {
        let body = json!({
            "chainIndex": chain_index,
            "gasTokenAddress": gas_token_address,
            "fromAddr": from_addr,
        });
        self.post_authed(
            "/priapi/v5/wallet/agentic/gas-station/update-default-token",
            access_token,
            &body,
        )
        .await
    }

    /// POST /priapi/v5/wallet/agentic/gas-station/update
    /// Flip Gas Station DB flag (enabled=true / false), no on-chain action.
    /// `from_addr` is required by backend when `enabled=true`; disable (`enabled=false`)
    /// does not need it. On-chain 7702 delegation is preserved on disable; re-enable
    /// requires 7702 already present (backend returns msg in body if not).
    pub async fn gas_station_update(
        &mut self,
        access_token: &str,
        chain_index: &str,
        enable: bool,
        from_addr: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "chainIndex": chain_index,
            "enabled": enable,
        });
        if let Some(addr) = from_addr {
            body["fromAddr"] = Value::String(addr.to_string());
        }
        self.post_authed(
            "/priapi/v5/wallet/agentic/gas-station/update",
            access_token,
            &body,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_init_response() {
        let json = r#"{"flowId": "abc-123"}"#;
        let resp: InitResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.flow_id, "abc-123");
    }

    #[test]
    fn parse_verify_response() {
        // New format: addressList items no longer contain accountId
        let json = r#"{
            "refreshToken": "rt",
            "accessToken": "at",
            "teeId": "tee1",
            "sessionCert": "cert",
            "encryptedSessionSk": "esk",
            "sessionKeyExpireAt": "2025-12-31",
            "projectId": "proj",
            "accountId": "acc",
            "accountName": "My Wallet",
            "isNew": true,
            "addressList": [
                {"chainIndex": "1", "address": "0xabc", "chainName": "ETH", "addressType": "eoa", "chainPath": "m/44/60"},
                {"chainIndex": "501", "address": "SoLaddr", "chainName": "SOL", "addressType": "eoa", "chainPath": "m/44/501"}
            ]
        }"#;
        let resp: VerifyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.refresh_token, "rt");
        assert_eq!(resp.access_token, "at");
        assert_eq!(resp.tee_id, "tee1");
        assert_eq!(resp.session_cert, "cert");
        assert_eq!(resp.encrypted_session_sk, "esk");
        assert_eq!(resp.session_key_expire_at, "2025-12-31");
        assert_eq!(resp.project_id, "proj");
        assert_eq!(resp.account_id, "acc");
        assert_eq!(resp.account_name, "My Wallet");
        assert!(resp.is_new);
        assert_eq!(resp.address_list.len(), 2);
        assert_eq!(resp.address_list[0].chain_index, "1");
        assert_eq!(resp.address_list[0].account_id, ""); // no accountId in new format → default ""
        assert_eq!(resp.address_list[1].address, "SoLaddr");
    }

    #[test]
    fn parse_refresh_response() {
        let json = r#"{"refreshToken": "new_rt", "accessToken": "new_at"}"#;
        let resp: RefreshResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.refresh_token, "new_rt");
        assert_eq!(resp.access_token, "new_at");
        assert!(!resp.chain_updated);
        assert!(resp.all_account_address_list.is_empty());
    }

    #[test]
    fn parse_refresh_response_with_chain_updated() {
        let json = r#"{
            "refreshToken": "rt2",
            "accessToken": "at2",
            "chainUpdated": true,
            "allAccountAddressList": [
                {
                    "accountId": "acc-1",
                    "accountName": "Wallet 1",
                    "isDefault": true,
                    "addresses": [
                        {"chainIndex": 4217, "address": "0xabc", "chainName": "tempo", "addressType": "eoa", "chainPath": "m/44/60/0/0"},
                        {"chainIndex": "1",  "address": "0xdef", "chainName": "eth",   "addressType": "eoa", "chainPath": "m/44/60/0/0"}
                    ]
                },
                {
                    "accountId": "acc-2",
                    "accountName": "Wallet 2",
                    "isDefault": false,
                    "addresses": []
                }
            ]
        }"#;
        let resp: RefreshResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.refresh_token, "rt2");
        assert_eq!(resp.access_token, "at2");
        assert!(resp.chain_updated);
        assert_eq!(resp.all_account_address_list.len(), 2);

        let acc1 = &resp.all_account_address_list[0];
        assert_eq!(acc1.account_id, "acc-1");
        assert_eq!(acc1.account_name, "Wallet 1");
        assert!(acc1.is_default);
        assert_eq!(acc1.addresses.len(), 2);
        // int chainIndex → String via string_or_number
        assert_eq!(acc1.addresses[0].chain_index, "4217");
        assert_eq!(acc1.addresses[0].chain_name, "tempo");
        // string chainIndex passes through
        assert_eq!(acc1.addresses[1].chain_index, "1");

        let acc2 = &resp.all_account_address_list[1];
        assert!(!acc2.is_default);
        assert!(acc2.addresses.is_empty());
    }

    #[test]
    fn parse_create_account_response() {
        let json = r#"{
            "projectId": "proj2",
            "accountId": "acc2",
            "accountName": "Wallet 2",
            "addressList": [{"chainIndex": "1", "address": "0xdef", "chainName": "ETH", "addressType": "eoa", "chainPath": "m/44/60"}]
        }"#;
        let resp: CreateAccountResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.project_id, "proj2");
        assert_eq!(resp.account_id, "acc2");
        assert_eq!(resp.account_name, "Wallet 2");
        assert_eq!(resp.address_list.len(), 1);
    }

    #[test]
    fn parse_account_list_item() {
        let json = r#"[
            {"projectId": "p1", "accountId": "a1", "accountName": "Default", "isDefault": true},
            {"projectId": "p1", "accountId": "a2", "accountName": "Second"}
        ]"#;
        let items: Vec<AccountListItem> = serde_json::from_str(json).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_default);
        assert!(!items[1].is_default); // #[serde(default)]
        assert_eq!(items[1].account_name, "Second");
    }

    #[test]
    fn parse_unsigned_info_response_evm() {
        let json = r#"{
            "unsignedTxHash": "0xabc123",
            "unsignedTx": "0xrawtx",
            "uopHash": "0xuop",
            "hash": "0xhash",
            "authHashFor7702": "0xauth",
            "executeErrorMsg": "",
            "executeResult": true,
            "extraData": {
                "to": "0xrecipient",
                "value": "0x0",
                "data": "0x",
                "chainId": "0x1",
                "nonce": "0x1",
                "gasLimit": "0x5208",
                "maxFeePerGas": "0x3b9aca00",
                "maxPriorityFeePerGas": "0x59682f00"
            },
            "signType": "eip1559Tx",
            "encoding": "hex"
        }"#;
        let resp: UnsignedInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.unsigned_tx_hash, "0xabc123");
        assert_eq!(resp.unsigned_tx, "0xrawtx");
        assert_eq!(resp.uop_hash, "0xuop");
        assert_eq!(resp.hash, "0xhash");
        assert_eq!(resp.auth_hash_for7702, "0xauth");
        assert_eq!(resp.sign_type, "eip1559Tx");
        assert_eq!(resp.encoding, "hex");
        assert_eq!(resp.execute_result, Value::Bool(true));
        assert!(resp.unsign_hash.is_empty()); // EVM doesn't use this
                                              // Verify extraData is parsed as an object with expected fields
        assert!(resp.extra_data.is_object());
        assert_eq!(resp.extra_data["to"], "0xrecipient");
        assert_eq!(resp.extra_data["chainId"], "0x1");
        assert_eq!(resp.extra_data["gasLimit"], "0x5208");
    }

    #[test]
    fn parse_unsigned_info_response_solana() {
        let json = r#"{
            "unsignHash": "sol_hash",
            "unsignedTx": "sol_raw",
            "executeResult": true,
            "signType": "solTx",
            "encoding": "base58"
        }"#;
        let resp: UnsignedInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.unsign_hash, "sol_hash");
        assert_eq!(resp.unsigned_tx, "sol_raw");
        assert_eq!(resp.sign_type, "solTx");
        assert!(resp.unsigned_tx_hash.is_empty()); // Solana uses unsignHash
    }

    #[test]
    fn parse_broadcast_response() {
        let json = r#"{
            "pkgId": "pkg-1",
            "orderId": "order-1",
            "orderType": "normal",
            "txHash": "0xtxhash123"
        }"#;
        let resp: BroadcastResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.pkg_id, "pkg-1");
        assert_eq!(resp.order_id, "order-1");
        assert_eq!(resp.order_type, "normal");
        assert_eq!(resp.tx_hash, "0xtxhash123");
    }

    #[test]
    fn parse_unsigned_info_from_numeric_code_envelope() {
        // unsignedInfo API returns numeric code 0, not string "0"
        let api_json = r#"{
            "code": 0,
            "msg": "",
            "data": [{
                "unsignedTxHash": "0xabc",
                "unsignedTx": "0xraw",
                "hash": "0xhash",
                "executeResult": true,
                "signType": "eip1559Tx",
                "encoding": "hex"
            }]
        }"#;
        let body: Value = serde_json::from_str(api_json).unwrap();
        // Verify numeric code handling
        let code_ok = match &body["code"] {
            Value::String(s) => s == "0",
            Value::Number(n) => n.as_i64() == Some(0),
            _ => false,
        };
        assert!(code_ok);
        let data = &body["data"];
        let arr = data.as_array().unwrap();
        let item = arr.first().unwrap();
        let resp: UnsignedInfoResponse = serde_json::from_value(item.clone()).unwrap();
        assert_eq!(resp.unsigned_tx_hash, "0xabc");
        assert_eq!(resp.sign_type, "eip1559Tx");
    }

    #[test]
    fn parse_verify_response_from_api_envelope() {
        // Simulate the full API response shape: { "code": "0", "data": [...] }
        let api_json = r#"{
            "code": "0",
            "msg": "success",
            "data": [{
                "refreshToken": "rt",
                "accessToken": "at",
                "apiKey": "ak",
                "passphrase": "pp",
                "teeId": "t",
                "sessionCert": "c",
                "encryptedSessionSk": "e",
                "sessionKeyExpireAt": "2025-12-31",
                "projectId": "p",
                "accountId": "a",
                "accountName": "W",
                "isNew": false,
                "addressList": []
            }]
        }"#;
        let body: Value = serde_json::from_str(api_json).unwrap();
        let data = &body["data"];
        let arr = data.as_array().unwrap();
        let item = arr.first().unwrap();
        let resp: VerifyResponse = serde_json::from_value(item.clone()).unwrap();
        assert_eq!(resp.project_id, "p");
        assert!(!resp.is_new);
        assert!(resp.address_list.is_empty());
    }

    #[test]
    fn parse_verify_address_info_with_null_fields() {
        // New format: no accountId in addressList items, chainPath can be null
        let json = r#"{
            "address": "0xabc",
            "chainIndex": 196,
            "chainName": "okb",
            "addressType": "aa",
            "chainPath": null
        }"#;
        let info: VerifyAddressInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.account_id, ""); // not present → default ""
        assert_eq!(info.address, "0xabc");
        assert_eq!(info.chain_index, "196");
        assert_eq!(info.chain_name, "okb");
        assert!(info.chain_path.is_empty()); // null → ""
    }

    #[test]
    fn parse_verify_response_with_number_and_bool_fields() {
        // sessionKeyExpireAt comes as Number, isNew may come as 0/1 or bool
        let json = r#"{
            "refreshToken": "rt",
            "accessToken": "at",
            "apiKey": "ak",
            "passphrase": "pp",
            "teeId": "t",
            "sessionCert": "c",
            "encryptedSessionSk": "e",
            "sessionKeyExpireAt": 1781959290,
            "projectId": "p",
            "accountId": "a",
            "accountName": "W",
            "isNew": 1,
            "addressList": []
        }"#;
        let resp: VerifyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.session_key_expire_at, "1781959290"); // Number → String
        assert!(resp.is_new); // 1 → true
    }

    #[test]
    fn parse_address_list_data() {
        // API returns accounts as an array; chainIndex is a number.
        let json = r#"{
            "accountCnt": 2,
            "validAccountCnt": 2,
            "addressCnt": 3,
            "accounts": [
                {
                    "accountId": "acc-1",
                    "addresses": [
                        {"address": "0xabc", "chainIndex": 1, "chainName": "ETH", "addressType": "EOA", "chainPath": "m/44/60"},
                        {"address": "SoLaddr", "chainIndex": 501, "chainName": "SOL", "addressType": "EOA", "chainPath": "m/44/501"}
                    ]
                },
                {
                    "accountId": "acc-2",
                    "addresses": [
                        {"address": "0xdef", "chainIndex": 1, "chainName": "ETH", "addressType": "EOA", "chainPath": "m/44/60"}
                    ]
                }
            ]
        }"#;
        let data: AddressListData = serde_json::from_str(json).unwrap();
        assert_eq!(data.accounts.len(), 2);
        assert_eq!(data.accounts[0].account_id, "acc-1");
        assert_eq!(data.accounts[0].addresses.len(), 2);
        assert_eq!(data.accounts[0].addresses[0].chain_name, "ETH");
        assert_eq!(data.accounts[0].addresses[0].chain_index, "1"); // number → string
        assert_eq!(data.accounts[0].addresses[1].chain_name, "SOL");
        assert_eq!(data.accounts[1].account_id, "acc-2");
        assert_eq!(data.accounts[1].addresses.len(), 1);
    }

    #[test]
    fn parse_address_list_data_empty_accounts() {
        let json = r#"{"accounts": []}"#;
        let data: AddressListData = serde_json::from_str(json).unwrap();
        assert!(data.accounts.is_empty());
    }

    #[test]
    fn parse_address_list_data_missing_accounts() {
        // accounts field missing entirely → default to empty vec
        let json = r#"{"accountCnt": 0}"#;
        let data: AddressListData = serde_json::from_str(json).unwrap();
        assert!(data.accounts.is_empty());
    }

    #[test]
    fn parse_address_list_data_from_api_envelope() {
        // Full API response: data is an array with a single element (like all other endpoints).
        let json = r#"{
            "code": 0,
            "msg": "success",
            "data": [{
                "accountCnt": 2,
                "validAccountCnt": 2,
                "addressCnt": 3,
                "accounts": [
                    {
                        "accountId": "acc-001",
                        "addresses": [
                            {"address": "0xabc", "chainIndex": 1, "chainName": "ETH", "addressType": "EOA", "chainPath": "m/44/60"}
                        ]
                    },
                    {
                        "accountId": "acc-002",
                        "addresses": [
                            {"address": "0xdef", "chainIndex": 56, "chainName": "BSC", "addressType": "EOA", "chainPath": "m/44/56"},
                            {"address": "SoLaddr", "chainIndex": 501, "chainName": "SOL", "addressType": "EOA", "chainPath": "m/44/501"}
                        ]
                    }
                ]
            }]
        }"#;
        let body: Value = serde_json::from_str(json).unwrap();
        let data_val = &body["data"];
        let arr = data_val.as_array().unwrap();
        let item = arr.first().unwrap();
        let resp: AddressListData = serde_json::from_value(item.clone()).unwrap();
        assert_eq!(resp.accounts.len(), 2);
        assert_eq!(resp.accounts[0].account_id, "acc-001");
        assert_eq!(resp.accounts[0].addresses.len(), 1);
        assert_eq!(resp.accounts[0].addresses[0].chain_index, "1");
        assert_eq!(resp.accounts[1].account_id, "acc-002");
        assert_eq!(resp.accounts[1].addresses.len(), 2);
        assert_eq!(resp.accounts[1].addresses[0].chain_index, "56");
        assert_eq!(resp.accounts[1].addresses[1].chain_index, "501");
    }

    // ── Gas Station routing: GasStationStatus::parse ───────────────

    #[test]
    fn gas_station_status_parses_all_known_values() {
        assert_eq!(GasStationStatus::parse("NOT_APPLICABLE"), GasStationStatus::NotApplicable);
        assert_eq!(GasStationStatus::parse("FIRST_TIME_PROMPT"), GasStationStatus::FirstTimePrompt);
        assert_eq!(GasStationStatus::parse("PENDING_UPGRADE"), GasStationStatus::PendingUpgrade);
        assert_eq!(GasStationStatus::parse("REENABLE_ONLY"), GasStationStatus::ReenableOnly);
        assert_eq!(GasStationStatus::parse("READY_TO_USE"), GasStationStatus::ReadyToUse);
        assert_eq!(GasStationStatus::parse("INSUFFICIENT_ALL"), GasStationStatus::InsufficientAll);
        assert_eq!(GasStationStatus::parse("HAS_PENDING_TX"), GasStationStatus::HasPendingTx);
    }

    #[test]
    fn gas_station_status_unknown_values_map_to_unknown() {
        assert_eq!(GasStationStatus::parse(""), GasStationStatus::Unknown);
        assert_eq!(GasStationStatus::parse("not_applicable"), GasStationStatus::Unknown); // case-sensitive
        assert_eq!(GasStationStatus::parse("UNKNOWN"), GasStationStatus::Unknown);
        assert_eq!(GasStationStatus::parse("garbage"), GasStationStatus::Unknown);
    }

    #[test]
    fn unsigned_gs_status_dispatches_to_enum() {
        let json = r#"{"gasStationStatus": "READY_TO_USE"}"#;
        let resp: UnsignedInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.gs_status(), GasStationStatus::ReadyToUse);

        let empty: UnsignedInfoResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.gs_status(), GasStationStatus::Unknown);
    }

    // ── Gas Station routing: match_default_sufficient_token ────────

    use crate::test_helpers::gas_station::{make_token, make_unsigned_with_tokens};

    #[test]
    fn match_default_returns_none_when_default_empty() {
        let resp = make_unsigned_with_tokens("", vec![make_token("USDT", "0xaaa", true)]);
        assert!(resp.match_default_sufficient_token().is_none());
    }

    #[test]
    fn match_default_returns_some_on_hit_and_sufficient() {
        let resp = make_unsigned_with_tokens(
            "0xaaa",
            vec![
                make_token("USDT", "0xaaa", true),
                make_token("USDC", "0xbbb", true),
            ],
        );
        let matched = resp.match_default_sufficient_token().unwrap();
        assert_eq!(matched.symbol, "USDT");
    }

    #[test]
    fn match_default_is_case_insensitive_on_address() {
        let resp = make_unsigned_with_tokens(
            "0xAAA",
            vec![make_token("USDT", "0xaaa", true)],
        );
        assert!(resp.match_default_sufficient_token().is_some());
    }

    #[test]
    fn match_default_returns_none_when_default_hits_but_insufficient() {
        let resp = make_unsigned_with_tokens(
            "0xaaa",
            vec![
                make_token("USDT", "0xaaa", false),
                make_token("USDC", "0xbbb", true),
            ],
        );
        assert!(resp.match_default_sufficient_token().is_none());
    }

    #[test]
    fn match_default_returns_none_when_default_not_in_list() {
        let resp = make_unsigned_with_tokens(
            "0xdeadbeef",
            vec![make_token("USDT", "0xaaa", true)],
        );
        assert!(resp.match_default_sufficient_token().is_none());
    }

    // ── Gas Station routing: only_sufficient_token ─────────────────

    #[test]
    fn only_sufficient_returns_none_when_no_sufficient() {
        let resp = make_unsigned_with_tokens(
            "",
            vec![
                make_token("USDT", "0xaaa", false),
                make_token("USDC", "0xbbb", false),
            ],
        );
        assert!(resp.only_sufficient_token().is_none());
    }

    #[test]
    fn only_sufficient_returns_the_single_sufficient_token() {
        let resp = make_unsigned_with_tokens(
            "",
            vec![
                make_token("USDT", "0xaaa", false),
                make_token("USDC", "0xbbb", true),
                make_token("USDG", "0xccc", false),
            ],
        );
        let token = resp.only_sufficient_token().unwrap();
        assert_eq!(token.symbol, "USDC");
    }

    #[test]
    fn only_sufficient_returns_none_when_multiple_sufficient() {
        let resp = make_unsigned_with_tokens(
            "",
            vec![
                make_token("USDT", "0xaaa", true),
                make_token("USDC", "0xbbb", true),
            ],
        );
        assert!(resp.only_sufficient_token().is_none());
    }

    #[test]
    fn only_sufficient_returns_none_on_empty_list() {
        let resp = make_unsigned_with_tokens("", vec![]);
        assert!(resp.only_sufficient_token().is_none());
    }

    // ── Gas Station routing: auto_pick_gas_token ───────────────────

    #[test]
    fn auto_pick_default_sufficient_scene_b() {
        // Scene B classic: default present, hits list, sufficient.
        let resp = make_unsigned_with_tokens(
            "0xaaa",
            vec![
                make_token("USDT", "0xaaa", true),
                make_token("USDC", "0xbbb", true),
            ],
        );
        assert_eq!(resp.auto_pick_gas_token().unwrap().symbol, "USDT");
    }

    #[test]
    fn auto_pick_no_default_single_sufficient_plugin_fallback() {
        // Plugin-compat fallback: no default, exactly one sufficient.
        let resp = make_unsigned_with_tokens(
            "",
            vec![
                make_token("USDT", "0xaaa", false),
                make_token("USDC", "0xbbb", true),
            ],
        );
        assert_eq!(resp.auto_pick_gas_token().unwrap().symbol, "USDC");
    }

    #[test]
    fn auto_pick_excludes_scene_2a_default_present_but_insufficient() {
        // Critical invariant: default is set (user preference), default is short, but
        // another token is sufficient — MUST return None so Scene C asks the user before
        // silently overriding the user's pinned default.
        let resp = make_unsigned_with_tokens(
            "0xaaa",
            vec![
                make_token("USDT", "0xaaa", false), // default, insufficient
                make_token("USDC", "0xbbb", true),  // alt, sufficient
            ],
        );
        assert!(resp.auto_pick_gas_token().is_none());
    }

    #[test]
    fn auto_pick_no_default_multiple_sufficient_scene_c() {
        // Multiple sufficient + no default → Scene C (user picks).
        let resp = make_unsigned_with_tokens(
            "",
            vec![
                make_token("USDT", "0xaaa", true),
                make_token("USDC", "0xbbb", true),
            ],
        );
        assert!(resp.auto_pick_gas_token().is_none());
    }

    #[test]
    fn auto_pick_none_when_default_not_in_list_even_if_others_sufficient() {
        // Default points at a token not in list (unusual), one other is sufficient.
        // Must still return None (Scene C) — the default being set is user preference.
        let resp = make_unsigned_with_tokens(
            "0xdeadbeef",
            vec![make_token("USDC", "0xbbb", true)],
        );
        assert!(resp.auto_pick_gas_token().is_none());
    }

    #[test]
    fn auto_pick_none_on_insufficient_all() {
        let resp = make_unsigned_with_tokens(
            "",
            vec![
                make_token("USDT", "0xaaa", false),
                make_token("USDC", "0xbbb", false),
            ],
        );
        assert!(resp.auto_pick_gas_token().is_none());
    }
}
