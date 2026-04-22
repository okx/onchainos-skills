use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;

use crate::commands::agentic_wallet::payment_flow::PaymentTier;
use crate::doh::DohManager;
use crate::output::CliConfirming;
use crate::payment_cache::{self, PaymentCache};
use crate::payment_notify::{self, Flag, NotifyInput, UserType};

pub const DEFAULT_BASE_URL: &str = "https://web3.okx.com";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Market API config endpoint — returns the path→tier `endpointList` map plus
/// the default `accepts` signing parameters. Refreshed at most once per
/// `CONFIG_TTL_SECS`.
const CONFIG_PATH: &str = "/api/v6/dex/market/config";
const CONFIG_TTL_SECS: u64 = 3600;

/// Response header the server uses to flip charging state per tier.
/// Format: `Basic=1;Premium=0` — `1` means pre-sign the next request on that tier.
const PAYMENT_STATE_HEADER: &str = "ok-web3-openapi-pay";

/// In-memory payment snapshot. Initialised from the on-disk cache
/// (`~/.onchainos/payment_cache.json`) on first use, refreshed from
/// `/api/v6/dex/market/config` when the cache is stale, and mutated by
/// response headers on every request.
#[derive(Debug, Default)]
struct PaymentState {
    /// Path → tier mapping from the server's `endpointList`.
    endpoints: HashMap<String, PaymentTier>,
    accepts: Option<Value>,
    basic_charging: bool,
    premium_charging: bool,
    /// `true` once we've tried to populate state this process. Prevents
    /// redundant config fetches across concurrent requests on the same client.
    config_loaded: bool,

    // ── Notification state (mirrored in PaymentCache) ───────────────────
    /// Parsed from the `UserType=` field of `ok-web3-openapi-pay`.
    user_type: Option<UserType>,
    /// Unix seconds. From `/config`'s `graceExpiresAt`; `None` means the
    /// caller should fall back to `payment_notify::fallback_grace_expires_at()`.
    grace_expires_at: Option<i64>,
    /// Per-event dedupe — persisted so the prompt fires at most once per
    /// account lifetime (cache is cleared on logout).
    intro_shown: bool,
    grace_shown: bool,
    basic_over_shown: bool,
    premium_over_shown: bool,

    /// Transient (not persisted) — tiers whose `*_over_shown` flipped to
    /// true during the most recent `handle_response` cycle. The 402
    /// retry wrapper reads this to decide whether to block with a
    /// `CliConfirming` error (first-time charging, user must re-run) or
    /// auto-sign as usual. Cleared at the top of every `handle_response`.
    pending_over_quota_tiers: HashSet<PaymentTier>,
}

/// A cached 402 response converted into a recoverable error.
///
/// `get_with_headers` / `post_with_headers` catch this, sign a proof from
/// `accepts`, and retry the request once with the payment header attached.
#[derive(Debug)]
pub struct PaymentRequired {
    pub accepts: Value,
    pub raw_body: Value,
}

impl std::fmt::Display for PaymentRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP 402 Payment Required")
    }
}

impl std::error::Error for PaymentRequired {}

/// Build the `CliConfirming` used on the first request that flips a tier
/// from free to charging. Leaves `message` / `next` empty — all semantic
/// info (which tier, payment options) is carried by the paired
/// `OVER_QUOTA` notification in the response; the skill renders the
/// user-facing copy from that alone.
fn first_charge_confirming() -> CliConfirming {
    CliConfirming {
        message: String::new(),
        next: String::new(),
    }
}

/// Read the x402 V2 `PAYMENT-REQUIRED` response header (base64-encoded JSON)
/// and return its `accepts` array, if present. The header is the standard V2
/// carrier for payment requirements; OKX may also place `accepts` in the body
/// for convenience — callers should treat this as the preferred source.
fn extract_payment_required_accepts(headers: &reqwest::header::HeaderMap) -> Option<Value> {
    let raw = headers.get("payment-required")?.to_str().ok()?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
    let payload: Value = serde_json::from_slice(&decoded).ok()?;
    payload.get("accepts").cloned()
}

/// Authentication mode for API requests.
#[derive(Clone)]
enum AuthMode {
    /// User is logged in — use JWT Bearer token.
    Jwt(String),
    /// User is not logged in but AK credentials are available — use HMAC signing.
    Ak {
        api_key: String,
        secret_key: String,
        passphrase: String,
    },
    /// No credentials available — send only basic headers (Content-Type, ok-client-version).
    Anonymous,
}

pub struct ApiClient {
    http: Client,
    base_url: String,
    auth: AuthMode,
    doh: DohManager,
    payment: Arc<Mutex<PaymentState>>,
}

impl ApiClient {
    /// Create a client with automatic auth detection:
    /// 1. JWT from keyring  (user is logged in)
    /// 2. AK from env vars / ~/.onchainos/.env  (user is not logged in)
    pub fn new(base_url_override: Option<&str>) -> Result<Self> {
        let auth = Self::resolve_auth()?;
        let base_url = base_url_override
            .map(|s| s.to_string())
            .or_else(|| option_env!("OKX_BASE_URL").map(|s| s.to_string()))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let custom = base_url_override.is_some() || option_env!("OKX_BASE_URL").is_some();
        let mut doh = DohManager::new("web3.okx.com", &base_url, custom);
        doh.prepare();

        let mut builder = Client::builder().timeout(std::time::Duration::from_secs(10));
        if let Some((host, addr)) = doh.resolve_override() {
            builder = builder.resolve(&host, addr);
        }
        if doh.is_proxy() {
            builder = builder.user_agent(doh.doh_user_agent());
        }

        Ok(Self {
            http: builder.build()?,
            base_url,
            auth,
            doh,
            payment: Arc::new(Mutex::new(PaymentState::default())),
        })
    }

    /// Create a client with full JWT lifecycle check:
    /// 1. JWT exists and not expired                → use JWT
    /// 2. JWT expired + refresh token valid         → refresh JWT → use new JWT
    /// 3. JWT expired + refresh token expired       → prompt user + AK / Anonymous
    /// 4. No JWT                                    → AK / Anonymous
    pub async fn new_async(base_url_override: Option<&str>) -> Result<Self> {
        let auth = Self::resolve_auth_async().await?;
        let base_url = base_url_override
            .map(|s| s.to_string())
            .or_else(|| option_env!("OKX_BASE_URL").map(|s| s.to_string()))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let custom = base_url_override.is_some() || option_env!("OKX_BASE_URL").is_some();
        let mut doh = DohManager::new("web3.okx.com", &base_url, custom);
        doh.prepare();

        let mut builder = Client::builder().timeout(std::time::Duration::from_secs(10));
        if let Some((host, addr)) = doh.resolve_override() {
            builder = builder.resolve(&host, addr);
        }
        if doh.is_proxy() {
            builder = builder.user_agent(doh.doh_user_agent());
        }

        Ok(Self {
            http: builder.build()?,
            base_url,
            auth,
            doh,
            payment: Arc::new(Mutex::new(PaymentState::default())),
        })
    }

    /// Resolve authentication mode:
    /// 1. JWT from keyring (user is logged in)
    /// 2. AK from env vars / ~/.onchainos/.env (user has configured credentials)
    /// 3. Anonymous — no credentials, send only basic headers
    fn resolve_auth() -> Result<AuthMode> {
        // 1. Try JWT from keyring (no expiry check — sync path)
        if let Some(token) = crate::keyring_store::get_opt("access_token") {
            if !token.is_empty() {
                return Ok(AuthMode::Jwt(token));
            }
        }

        Self::resolve_ak_or_anonymous()
    }

    /// Full async auth resolution with JWT expiry check and auto-refresh.
    async fn resolve_auth_async() -> Result<AuthMode> {
        // ── Step 1: is there a JWT? ──────────────────────────────────
        let access_token = crate::keyring_store::get_opt("access_token").filter(|t| !t.is_empty());

        let token = match access_token {
            None => return Self::resolve_ak_or_anonymous(),
            Some(t) => t,
        };

        // ── Step 2: JWT not expired → use it ────────────────────────
        if !Self::is_jwt_expired(&token) {
            return Ok(AuthMode::Jwt(token));
        }

        // ── Step 3: JWT expired → check refresh token ────────────────
        let refresh_token =
            crate::keyring_store::get_opt("refresh_token").filter(|t| !t.is_empty());

        let rt = match refresh_token {
            None => return Self::resolve_ak_or_anonymous(),
            Some(rt) => rt,
        };

        // ── Step 4: refresh token expired → prompt + fallback ────────
        if Self::is_jwt_expired(&rt) {
            eprintln!("Session expired. Please log in again: onchainos wallet login");
            return Self::resolve_ak_or_anonymous();
        }

        // ── Step 5: refresh token valid → refresh JWT ────────────────
        match Self::refresh_jwt_inline(&rt).await {
            Ok(new_token) => Ok(AuthMode::Jwt(new_token)),
            Err(e) => {
                eprintln!(
                    "Failed to refresh session ({}). Falling back to API key auth.",
                    e
                );
                Self::resolve_ak_or_anonymous()
            }
        }
    }

    /// Shared AK / Anonymous resolution used by both sync and async paths.
    fn resolve_ak_or_anonymous() -> Result<AuthMode> {
        // Load ~/.onchainos/.env if AK not yet in env
        if std::env::var("OKX_API_KEY").is_err() && std::env::var("OKX_ACCESS_KEY").is_err() {
            if let Ok(home) = crate::home::onchainos_home() {
                let env_path = home.join(".env");
                if env_path.exists() {
                    dotenvy::from_path(env_path).ok();
                }
            }
        }

        let api_key = std::env::var("OKX_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("OKX_ACCESS_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
            });

        match api_key {
            None => Ok(AuthMode::Anonymous),
            Some(key) => {
                let secret_key = std::env::var("OKX_SECRET_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("OKX_SECRET_KEY is required but not set"))?;
                let passphrase = std::env::var("OKX_PASSPHRASE")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("OKX_PASSPHRASE is required but not set"))?;
                Ok(AuthMode::Ak {
                    api_key: key,
                    secret_key,
                    passphrase,
                })
            }
        }
    }

    /// Inline JWT refresh — avoids circular dependency with WalletApiClient.
    /// Calls /priapi/v5/wallet/agentic/auth/refresh and stores the new tokens.
    async fn refresh_jwt_inline(refresh_token: &str) -> Result<String> {
        let base_url = option_env!("OKX_BASE_URL").unwrap_or(DEFAULT_BASE_URL);
        let url = format!("{}/priapi/v5/wallet/agentic/auth/refresh", base_url);
        let body = serde_json::json!({ "refreshToken": refresh_token });

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = http
            .post(&url)
            .headers(Self::anonymous_headers())
            .json(&body)
            .send()
            .await
            .context("JWT refresh request failed")?;

        let json: Value = resp
            .json()
            .await
            .context("failed to parse JWT refresh response")?;

        let code_ok = match &json["code"] {
            Value::String(s) => s == "0",
            Value::Number(n) => n.as_i64() == Some(0),
            _ => false,
        };
        if !code_ok {
            let msg = json["msg"].as_str().unwrap_or("unknown error");
            bail!("JWT refresh failed: {}", msg);
        }

        let arr = json["data"]
            .as_array()
            .context("refresh: expected data array")?;
        let item = arr.first().context("refresh: empty data array")?;
        let new_access = item["accessToken"]
            .as_str()
            .context("refresh: missing accessToken")?;
        let new_refresh = item["refreshToken"]
            .as_str()
            .context("refresh: missing refreshToken")?;

        crate::keyring_store::store(&[
            ("access_token", new_access),
            ("refresh_token", new_refresh),
        ])?;

        Ok(new_access.to_string())
    }

    /// Decode JWT payload and extract `exp` claim without signature verification.
    fn jwt_exp_timestamp(token: &str) -> Option<i64> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .ok()?;
        let val: Value = serde_json::from_slice(&payload).ok()?;
        val["exp"].as_i64()
    }

    /// Returns true if the JWT is expired or unparseable.
    fn is_jwt_expired(token: &str) -> bool {
        Self::jwt_exp_timestamp(token)
            .map(|exp| chrono::Utc::now().timestamp() >= exp)
            .unwrap_or(true)
    }

    /// HMAC-SHA256 signature for AK auth.
    fn hmac_sign(
        secret_key: &str,
        timestamp: &str,
        method: &str,
        request_path: &str,
        body: &str,
    ) -> String {
        let prehash = format!("{}{}{}{}", timestamp, method, request_path, body);
        let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(prehash.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    }

    /// Build the base header map shared by all auth modes.
    ///
    /// Headers set:
    /// - `Content-Type: application/json`
    /// - `ok-client-version: <version>`
    /// - `Ok-Access-Client-type: agent-cli`
    pub(crate) fn anonymous_headers() -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
        let mut map = HeaderMap::new();
        map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        map.insert(
            "ok-client-version",
            HeaderValue::from_static(CLIENT_VERSION),
        );
        map.insert(
            "Ok-Access-Client-type",
            HeaderValue::from_static("agent-cli"),
        );
        map
    }

    /// Build the header map for JWT auth (logged-in state).
    /// Extends anonymous_headers with Authorization: Bearer.
    ///
    /// Additional header:
    /// - `Authorization: Bearer <token>`
    pub(crate) fn jwt_headers(token: &str) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderValue, AUTHORIZATION};
        let mut map = Self::anonymous_headers();
        map.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token)).expect("valid header value"),
        );
        map
    }

    /// Build the header map for AK signing auth (not-logged-in state).
    /// Extends anonymous_headers with AK signing fields.
    ///
    /// Additional headers:
    /// - `OK-ACCESS-KEY / OK-ACCESS-SIGN / OK-ACCESS-PASSPHRASE / OK-ACCESS-TIMESTAMP`
    /// - `ok-client-type: cli`
    pub(crate) fn ak_headers(
        api_key: &str,
        passphrase: &str,
        timestamp: &str,
        sign: &str,
    ) -> reqwest::header::HeaderMap {
        use reqwest::header::HeaderValue;
        let mut map = Self::anonymous_headers();
        map.insert(
            "OK-ACCESS-KEY",
            HeaderValue::from_str(api_key).expect("valid header value"),
        );
        map.insert(
            "OK-ACCESS-SIGN",
            HeaderValue::from_str(sign).expect("valid header value"),
        );
        map.insert(
            "OK-ACCESS-PASSPHRASE",
            HeaderValue::from_str(passphrase).expect("valid header value"),
        );
        map.insert(
            "OK-ACCESS-TIMESTAMP",
            HeaderValue::from_str(timestamp).expect("valid header value"),
        );
        map.insert("ok-client-type", HeaderValue::from_static("cli"));
        map
    }

    /// Apply JWT Bearer auth headers to a request builder (logged-in state).
    fn apply_jwt(builder: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
        builder.headers(Self::jwt_headers(token))
    }

    /// Apply anonymous headers (no credentials available).
    fn apply_anonymous(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        builder.headers(Self::anonymous_headers())
    }

    /// Apply AK signing headers to a request builder (not-logged-in state).
    fn apply_ak(
        builder: reqwest::RequestBuilder,
        api_key: &str,
        passphrase: &str,
        timestamp: &str,
        sign: &str,
    ) -> reqwest::RequestBuilder {
        builder.headers(Self::ak_headers(api_key, passphrase, timestamp, sign))
    }

    fn rebuild_http_client(&mut self) -> Result<()> {
        let mut builder = Client::builder()
            .timeout(std::time::Duration::from_secs(10));
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
        self.doh.proxy_base_url()
            .unwrap_or_else(|| self.base_url.clone())
    }

    fn build_get_url_and_request_path(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<(reqwest::Url, String)> {
        let filtered: Vec<(&str, &str)> = query
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .copied()
            .collect();

        let effective = self.effective_base_url();
        let mut url =
            reqwest::Url::parse(&format!("{}{}", effective.trim_end_matches('/'), path))?;

        if !filtered.is_empty() {
            url.query_pairs_mut().extend_pairs(filtered.iter().copied());
        }

        let query_string = url
            .query()
            .map(|query| format!("?{}", query))
            .unwrap_or_default();
        // request_path uses original path (no proxy host) — used for HMAC signing
        let request_path = format!("{}{}", path, query_string);

        Ok((url, request_path))
    }

    /// GET request with automatic auth (JWT or AK).
    pub async fn get(&mut self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.get_with_headers(path, query, None).await
    }

    /// GET request with automatic auth + optional extra headers.
    ///
    /// Wraps the request in the auto-payment flow:
    /// 1. Ensure payment config is loaded (first request only).
    /// 2. If the path is currently on a charging tier, pre-sign a payment header.
    /// 3. Send the request (with DoH failover retry inside `do_get_request`).
    /// 4. On 402, sign with the accepts returned by the server and retry once.
    pub async fn get_with_headers(
        &mut self,
        path: &str,
        query: &[(&str, &str)],
        extra_headers: Option<&[(&str, &str)]>,
    ) -> Result<Value> {
        self.ensure_payment_config().await;
        let resource = self.resource_url(path);
        let payment_hdr = self.maybe_sign_payment(path, &resource).await;
        let result = self
            .do_get_request(path, query, extra_headers, payment_hdr.as_ref())
            .await;
        match result {
            Ok(data) => Ok(data),
            Err(e) => match e.downcast::<PaymentRequired>() {
                Ok(pr) => {
                    if self.consume_pending_confirmation(path) {
                        return Err(first_charge_confirming().into());
                    }
                    let accepts = self.resolve_retry_accepts(&pr)?;
                    // Fall back to Basic if we have no tier mapping — cheapest
                    // safe default; if the server wanted Premium it will 402
                    // again and the user sees the error.
                    let tier = self.tier_for_path(path).unwrap_or(PaymentTier::Basic);
                    let hdr = self
                        .sign_header_from_accepts(&accepts, &resource, tier)
                        .await?;
                    self.do_get_request(path, query, extra_headers, Some(&hdr))
                        .await
                }
                Err(e) => Err(e),
            },
        }
    }

    /// POST request with automatic auth (JWT or AK). Retries after DoH failover.
    /// Signature uses path only (no query string) + JSON body string.
    pub async fn post(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.post_with_headers(path, body, None).await
    }

    /// POST request with automatic auth + optional extra headers.
    /// Mirrors `get_with_headers`: pre-signs on known-paid paths and retries once on 402.
    /// DoH failover retry happens inside `do_post_request`.
    pub async fn post_with_headers(
        &mut self,
        path: &str,
        body: &Value,
        extra_headers: Option<&[(&str, &str)]>,
    ) -> Result<Value> {
        self.ensure_payment_config().await;
        let resource = self.resource_url(path);
        let payment_hdr = self.maybe_sign_payment(path, &resource).await;
        let result = self
            .do_post_request(path, body, extra_headers, payment_hdr.as_ref())
            .await;
        match result {
            Ok(data) => Ok(data),
            Err(e) => match e.downcast::<PaymentRequired>() {
                Ok(pr) => {
                    if self.consume_pending_confirmation(path) {
                        return Err(first_charge_confirming().into());
                    }
                    let accepts = self.resolve_retry_accepts(&pr)?;
                    let tier = self.tier_for_path(path).unwrap_or(PaymentTier::Basic);
                    let hdr = self
                        .sign_header_from_accepts(&accepts, &resource, tier)
                        .await?;
                    self.do_post_request(path, body, extra_headers, Some(&hdr))
                        .await
                }
                Err(e) => Err(e),
            },
        }
    }

    /// POST request with no DoH retry — use only for broadcast-transaction.
    /// On network failure, records the failure but does NOT retry, because the
    /// broadcast may have partially reached the server.
    pub async fn post_no_retry_with_headers(
        &mut self,
        path: &str,
        body: &Value,
        extra_headers: Option<&[(&str, &str)]>,
    ) -> Result<Value> {
        let body_str = serde_json::to_string(body)?;
        let effective = self.effective_base_url();
        let url = format!("{}{}", effective.trim_end_matches('/'), path);
        let req = self.http.post(&url).body(body_str.clone());
        let req = match &self.auth {
            AuthMode::Jwt(token) => Self::apply_jwt(req, token),
            AuthMode::Ak {
                api_key,
                secret_key,
                passphrase,
            } => {
                let timestamp =
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let sign = Self::hmac_sign(secret_key, &timestamp, "POST", path, &body_str);
                Self::apply_ak(req, api_key, passphrase, &timestamp, &sign)
            }
            AuthMode::Anonymous => Self::apply_anonymous(req),
        };
        let req = Self::apply_extra_headers(req, extra_headers);

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) if e.is_connect() || e.is_timeout() => {
                let _ = self.doh.handle_failure().await;
                if self.doh.is_proxy() {
                    let _ = self.rebuild_http_client();
                }
                return Err(e).context(
                    "Network error during broadcast — transaction was NOT sent. Safe to retry the same command.",
                );
            }
            Err(e) => return Err(e).context("request failed"),
        };
        self.doh.cache_direct_if_needed();
        self.handle_response(path, resp).await
    }

    /// Apply optional extra headers to a request builder.
    fn apply_extra_headers(
        builder: reqwest::RequestBuilder,
        extra_headers: Option<&[(&str, &str)]>,
    ) -> reqwest::RequestBuilder {
        match extra_headers {
            Some(headers) => {
                use reqwest::header::HeaderValue;
                let mut map = reqwest::header::HeaderMap::new();
                for (k, v) in headers {
                    if let (Ok(name), Ok(val)) = (
                        reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                        HeaderValue::from_str(v),
                    ) {
                        map.insert(name, val);
                    }
                }
                builder.headers(map)
            }
            None => builder,
        }
    }

    async fn handle_response(&mut self, path: &str, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();

        // Drop transient dispatch state from the previous request so the
        // 402 retry wrapper only sees flips emitted *this* response.
        self.payment_state().pending_over_quota_tiers.clear();

        // Read charging-state + V2 PAYMENT-REQUIRED headers before consuming
        // the body so even error responses (402, 5xx, code!=0) still keep our
        // state in sync and give us access to the accepts payload.
        self.update_payment_state_from_headers(resp.headers());
        let header_accepts = extract_payment_required_accepts(resp.headers());

        // `/config` is an internal fetch path: its response still
        // updates charging flags via headers above, but we never
        // dispatch user-facing notifications for it (it's not in the
        // tier map, so `path_tier` would be None and the fallback
        // would wrongly emit both tiers). The outer request's
        // `handle_response` dispatches once `endpoints` is populated.
        if path != CONFIG_PATH {
            // If charging just flipped on but `endpoints` is still
            // empty (`/config` was skipped at request time because
            // nothing was charging yet), fetch `/config` inline so
            // `dispatch_notifications` has a valid `path_tier` and
            // only emits OVER_QUOTA for the tier this path maps to.
            // Box::pin breaks the async recursion cycle:
            // handle_response → ensure_payment_config → do_get_request
            // → handle_response (short-circuits for /config).
            let needs_config = {
                let s = self.payment_state();
                s.endpoints.is_empty() && (s.basic_charging || s.premium_charging)
            };
            if needs_config {
                Box::pin(self.ensure_payment_config()).await;
            }
            // 402 responses carry tier-specific accepts — feed them
            // transiently into notification dispatch, but never
            // persist. Only `/config` writes `payment_cache.accepts`.
            self.dispatch_notifications(path, header_accepts.as_ref());
        }

        if status.as_u16() == 429 {
            bail!("Rate limited — retry with backoff");
        }
        if status.as_u16() >= 500 {
            bail!("Server error (HTTP {})", status.as_u16());
        }

        // 402 may come with an empty body — accepts resolved from header or
        // cached config upstream. Other empty bodies are still an error.
        let body_bytes = resp.bytes().await.context("failed to read response body")?;
        if body_bytes.is_empty() {
            if status.as_u16() == 402 {
                return Err(PaymentRequired {
                    accepts: header_accepts.unwrap_or(Value::Null),
                    raw_body: Value::Null,
                }
                .into());
            }
            bail!(
                "Empty response body (HTTP {}). The requested operation may not be supported for the given parameters.",
                status.as_u16()
            );
        }
        let body: Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(_) => {
                let text = String::from_utf8_lossy(&body_bytes);
                bail!(
                    "HTTP {} {}: {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Error"),
                    text.trim()
                );
            }
        };

        // HTTP 402 — return as a typed error so the request wrapper can sign
        // and retry. Prefer accepts from PAYMENT-REQUIRED header (standard
        // x402 V2); fall back to the body if absent (OKX convenience layout).
        if status.as_u16() == 402 {
            let accepts = header_accepts
                .or_else(|| body.get("accepts").cloned())
                .unwrap_or(Value::Null);
            return Err(PaymentRequired {
                accepts,
                raw_body: body,
            }
            .into());
        }

        // Handle code as either string "0" or number 0 (some endpoints return numeric)
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
            let msg = body["msg"].as_str().unwrap_or("unknown error");
            bail!("API error (code={}): {}", code_str, msg);
        }

        Ok(body["data"].clone())
    }

    // ── Auto-payment: request helpers ────────────────────────────────────────

    /// Issue a GET with DoH failover retry on connect/timeout errors.
    /// Retries at most once, after `DohManager::handle_failure` finds a
    /// new proxy node and the HTTP client is rebuilt.
    async fn do_get_request(
        &mut self,
        path: &str,
        query: &[(&str, &str)],
        extra_headers: Option<&[(&str, &str)]>,
        payment_hdr: Option<&(&'static str, String)>,
    ) -> Result<Value> {
        loop {
            let (url, request_path) = self.build_get_url_and_request_path(path, query)?;
            let req = self.http.get(url);
            let req = match &self.auth {
                AuthMode::Jwt(token) => Self::apply_jwt(req, token),
                AuthMode::Ak {
                    api_key,
                    secret_key,
                    passphrase,
                } => {
                    let timestamp =
                        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                    let sign = Self::hmac_sign(secret_key, &timestamp, "GET", &request_path, "");
                    Self::apply_ak(req, api_key, passphrase, &timestamp, &sign)
                }
                AuthMode::Anonymous => Self::apply_anonymous(req),
            };
            let req = Self::apply_extra_headers(req, extra_headers);
            let req = Self::apply_payment_header(req, payment_hdr);

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if self.doh.handle_failure().await {
                        self.rebuild_http_client()?;
                        continue;
                    }
                    return Err(e)
                        .context("Network unavailable — check your connection and try again");
                }
                Err(e) => return Err(e).context("request failed"),
            };
            self.doh.cache_direct_if_needed();
            return self.handle_response(path, resp).await;
        }
    }

    /// Issue a POST with DoH failover retry on connect/timeout errors.
    /// Retries at most once (safe for idempotent endpoints). For
    /// non-idempotent endpoints like broadcast, use `post_no_retry_with_headers`.
    async fn do_post_request(
        &mut self,
        path: &str,
        body: &Value,
        extra_headers: Option<&[(&str, &str)]>,
        payment_hdr: Option<&(&'static str, String)>,
    ) -> Result<Value> {
        let body_str = serde_json::to_string(body)?;
        loop {
            let effective = self.effective_base_url();
            let url = format!("{}{}", effective.trim_end_matches('/'), path);
            let req = self.http.post(&url).body(body_str.clone());
            let req = match &self.auth {
                AuthMode::Jwt(token) => Self::apply_jwt(req, token),
                AuthMode::Ak {
                    api_key,
                    secret_key,
                    passphrase,
                } => {
                    let timestamp =
                        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                    let sign = Self::hmac_sign(secret_key, &timestamp, "POST", path, &body_str);
                    Self::apply_ak(req, api_key, passphrase, &timestamp, &sign)
                }
                AuthMode::Anonymous => Self::apply_anonymous(req),
            };
            let req = Self::apply_extra_headers(req, extra_headers);
            let req = Self::apply_payment_header(req, payment_hdr);

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if self.doh.handle_failure().await {
                        self.rebuild_http_client()?;
                        continue;
                    }
                    return Err(e)
                        .context("Network unavailable — check your connection and try again");
                }
                Err(e) => return Err(e).context("request failed"),
            };
            self.doh.cache_direct_if_needed();
            return self.handle_response(path, resp).await;
        }
    }

    fn apply_payment_header(
        builder: reqwest::RequestBuilder,
        payment_hdr: Option<&(&'static str, String)>,
    ) -> reqwest::RequestBuilder {
        match payment_hdr {
            Some((name, value)) => builder.header(*name, value.as_str()),
            None => builder,
        }
    }

    // ── Auto-payment: config loading ────────────────────────────────────────

    /// Acquire the payment state lock. If a prior holder panicked, the lock
    /// is poisoned; we keep going by taking the inner guard — the state is a
    /// cache and is safe to reuse. Matches the pattern in `wallet_store.rs`
    /// and `file_keyring.rs`.
    fn payment_state(&self) -> std::sync::MutexGuard<'_, PaymentState> {
        self.payment.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lazily load payment config. Only fetches `/config` from the network
    /// when the server has signalled charging (via `ok-web3-openapi-pay`);
    /// during all-free periods the 402 fallback in `handle_response`
    /// recovers any paid request for free, so a fresh config map is not
    /// needed. Cache file is consulted on the way in to restore the last
    /// known charging flags and (if fresh) the endpoint→tier map.
    async fn ensure_payment_config(&mut self) {
        if self.payment_state().config_loaded {
            return;
        }

        if let Some(cache) = PaymentCache::load() {
            if self.restore_from_cache(cache) {
                return;
            }
        }

        // Defer the network fetch until we have reason to pre-sign. If no
        // tier is charging, `maybe_sign_payment` would short-circuit to
        // None anyway; once a response header flips a flag, the next
        // request re-enters this function and triggers the fetch.
        let any_charging = {
            let s = self.payment_state();
            s.basic_charging || s.premium_charging
        };
        if !any_charging {
            return;
        }

        // Mark as loaded eagerly so concurrent requests don't all race to fetch.
        self.payment_state().config_loaded = true;

        // Fetch /api/v6/dex/market/config. This path itself is not paid, so we
        // bypass the payment flow and call do_get_request directly. Failures
        // are logged under debug-log but never surface — 402 fallback handles
        // the degraded case.
        match self.do_get_request(CONFIG_PATH, &[], None, None).await {
            Ok(data) => {
                self.apply_config_response(&data);
                let _ = self.flush_payment_cache();
            }
            Err(e) => {
                if cfg!(feature = "debug-log") {
                    eprintln!("[DEBUG][payment] config fetch failed: {e:#}");
                }
            }
        }
    }

    /// Seed in-memory state from a disk cache. Charging flags +
    /// `user_type` + notification `shown` flags are always restored (they
    /// track per-user server signals, independent of config TTL);
    /// `endpoints`/`accepts`/`graceExpiresAt`/`docsUrl` are restored only
    /// when the cache isn't expired. Returns `true` if the config portion
    /// was fresh enough to skip the remote fetch.
    fn restore_from_cache(&self, cache: PaymentCache) -> bool {
        let mut state = self.payment_state();
        state.basic_charging = cache.basic_charging;
        state.premium_charging = cache.premium_charging;
        state.user_type = cache.user_type;
        state.intro_shown = cache.intro_shown;
        state.grace_shown = cache.grace_shown;
        state.basic_over_shown = cache.basic_over_shown;
        state.premium_over_shown = cache.premium_over_shown;
        if cache.is_expired(CONFIG_TTL_SECS) {
            return false;
        }
        let accepts_empty = match &cache.accepts {
            None => true,
            Some(v) => v.is_null() || v.as_array().is_some_and(|a| a.is_empty()),
        };
        if cache.endpoints.is_empty() || accepts_empty {
            return false;
        }
        state.endpoints = cache
            .endpoints
            .into_iter()
            .filter_map(|(p, t)| PaymentTier::from_server_str(&t).map(|tier| (p, tier)))
            .collect();
        state.accepts = cache.accepts;
        state.grace_expires_at = cache.grace_expires_at;
        state.config_loaded = true;
        true
    }

    fn apply_config_response(&self, data: &Value) {
        let mut state = self.payment_state();
        state.endpoints.clear();
        if let Some(obj) = data.get("endpointList").and_then(|v| v.as_object()) {
            for (path, tier_val) in obj {
                if let Some(tier) = tier_val.as_str().and_then(PaymentTier::from_server_str) {
                    state.endpoints.insert(path.clone(), tier);
                }
            }
        }
        if let Some(a) = data.get("accepts") {
            if !a.is_null() {
                state.accepts = Some(a.clone());
            }
        }
        if let Some(s) = data.get("graceExpiresAt").and_then(|v| v.as_str()) {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                state.grace_expires_at = Some(dt.timestamp());
            }
        }
    }

    // ── Auto-payment: header parsing ─────────────────────────────────────────

    /// Update `basic_charging`/`premium_charging`/`user_type` from the
    /// `ok-web3-openapi-pay: Basic=1;Premium=0;UserType=1` response header.
    /// Writes to disk only when a flag actually flips — every other
    /// request is IO-free.
    fn update_payment_state_from_headers(&self, headers: &reqwest::header::HeaderMap) {
        let Some(raw) = headers
            .get(PAYMENT_STATE_HEADER)
            .and_then(|v| v.to_str().ok())
        else {
            return;
        };
        let basic = Self::extract_header_flag(raw, "Basic");
        let premium = Self::extract_header_flag(raw, "Premium");
        let user_type = Self::extract_header_value(raw, "UserType")
            .and_then(UserType::from_header_value);

        let changed = {
            let mut state = self.payment_state();
            let mut changed = false;
            if let Some(b) = basic {
                if state.basic_charging != b {
                    state.basic_charging = b;
                    // Reset the one-shot over-quota marker when the tier
                    // goes back to free, so the next free→charging
                    // transition re-emits the notification and forces
                    // another confirming step.
                    if !b {
                        state.basic_over_shown = false;
                    }
                    changed = true;
                }
            }
            if let Some(p) = premium {
                if state.premium_charging != p {
                    state.premium_charging = p;
                    if !p {
                        state.premium_over_shown = false;
                    }
                    changed = true;
                }
            }
            if let Some(ut) = user_type {
                if state.user_type != Some(ut) {
                    state.user_type = Some(ut);
                    changed = true;
                }
            }
            changed
        };
        if changed {
            let _ = self.flush_payment_cache();
        }
    }

    /// Parse a single `Key=0|1` pair out of the `Key=V;Key=V` header value.
    fn extract_header_flag(header: &str, key: &str) -> Option<bool> {
        match Self::extract_header_value(header, key)? {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        }
    }

    /// Return the raw string value for `Key=VALUE` in a `Key=V;Key=V` header.
    fn extract_header_value<'a>(header: &'a str, key: &str) -> Option<&'a str> {
        header.split(';').find_map(|part| {
            let mut it = part.trim().splitn(2, '=');
            let k = it.next()?.trim();
            let v = it.next()?.trim();
            if k.eq_ignore_ascii_case(key) {
                Some(v)
            } else {
                None
            }
        })
    }

    // ── Auto-payment: signing ───────────────────────────────────────────────

    /// Full URL for `path`, used as the `resource` field in the V2 payment
    /// header payload.
    fn resource_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    /// Look up the tier for a path from the loaded config. Returns `None`
    /// if we don't have a mapping (e.g. config failed to load, or the path
    /// isn't on any paid tier).
    fn tier_for_path(&self, path: &str) -> Option<PaymentTier> {
        self.payment_state().endpoints.get(path).copied()
    }

    /// Build a payment header for `path` if we know it's on a charging tier.
    /// Returns `None` if the path isn't charged, or if signing itself fails
    /// (e.g. the wallet isn't logged in — the request will then naturally hit
    /// the 402 fallback below).
    async fn maybe_sign_payment(
        &self,
        path: &str,
        resource: &str,
    ) -> Option<(&'static str, String)> {
        let (tier, accepts) = {
            let state = self.payment_state();
            let tier = state.endpoints.get(path).copied()?;
            let charging = match tier {
                PaymentTier::Basic => state.basic_charging,
                PaymentTier::Premium => state.premium_charging,
            };
            if !charging {
                return None;
            }
            (tier, state.accepts.clone())
        };
        let accepts = accepts?;
        self.sign_header_from_accepts(&accepts, resource, tier)
            .await
            .ok()
    }

    /// Sign a V2 payment header from a raw accepts value (from config or from
    /// a 402 response). OKX openapi follows standard x402 V2 (`PAYMENT-SIGNATURE`).
    /// `tier` picks which amount to sign when the server returns the tiered
    /// `amount: {basic, premium}` schema.
    async fn sign_header_from_accepts(
        &self,
        accepts: &Value,
        resource: &str,
        tier: PaymentTier,
    ) -> Result<(&'static str, String)> {
        let (proof, selected) =
            crate::commands::agentic_wallet::payment_flow::sign_payment(accepts, None, Some(tier))
                .await?;
        crate::commands::agentic_wallet::payment_flow::build_payment_header(
            &proof, &selected, resource,
        )
    }

    /// Pick the `accepts` to sign with on a 402 retry. Prefers fresh
    /// accepts from the 402 response (x402 V2 header or body); falls
    /// back to the cached `accepts` loaded from
    /// `/api/v6/dex/market/config` when the server returned an empty
    /// 402 (OKX OpenAPI style). Returns an error only when neither
    /// source has anything — at that point the retry cannot succeed.
    ///
    /// Never caches `pr.accepts`: a single 402 response carries only
    /// the caller's tier, so persisting it would overwrite the
    /// tier-aware map from `/config` and break the other tier's
    /// pre-signed amount.
    fn resolve_retry_accepts(&self, pr: &PaymentRequired) -> Result<Value> {
        if !pr.accepts.is_null() {
            return Ok(pr.accepts.clone());
        }
        self.payment_state().accepts.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "HTTP 402 but no payment requirements available — response had no \
                 accepts and no cached config. Retry after /api/v6/dex/market/config \
                 becomes reachable."
            )
        })
    }

    /// Write the current in-memory state to `~/.onchainos/payment_cache.json`.
    /// Preserves `default_asset` from the existing cache — it's owned by the
    /// user via `onchainos payment default`, not by this client, and must
    /// survive server-state flushes.
    fn flush_payment_cache(&self) -> Result<()> {
        let default_asset = PaymentCache::load().and_then(|c| c.default_asset);
        let state = self.payment_state();
        let cache = PaymentCache {
            endpoints: state
                .endpoints
                .iter()
                .map(|(p, t)| (p.clone(), t.as_key().to_string()))
                .collect(),
            accepts: state.accepts.clone(),
            basic_charging: state.basic_charging,
            premium_charging: state.premium_charging,
            updated_at: payment_cache::now_secs(),
            user_type: state.user_type,
            grace_expires_at: state.grace_expires_at,
            intro_shown: state.intro_shown,
            grace_shown: state.grace_shown,
            basic_over_shown: state.basic_over_shown,
            premium_over_shown: state.premium_over_shown,
            default_asset,
        };
        drop(state);
        cache.save()
    }

    /// Emit notification events for any triggers that fired on this
    /// response. Pure decision logic lives in `payment_notify`; this
    /// wrapper handles the lock dance, pushes events onto the
    /// process-global buffer (drained into the CLI / MCP response
    /// envelope later), and persists the dedupe flags so each event
    /// fires at most once per account lifetime.
    fn dispatch_notifications(&self, path: &str, header_accepts: Option<&Value>) {
        let now = chrono::Utc::now().timestamp();
        let input = {
            let state = self.payment_state();
            NotifyInput {
                user_type: state.user_type,
                grace_expires_at: state
                    .grace_expires_at
                    .unwrap_or_else(payment_notify::fallback_grace_expires_at),
                basic_charging: state.basic_charging,
                premium_charging: state.premium_charging,
                intro_shown: state.intro_shown,
                grace_shown: state.grace_shown,
                basic_over_shown: state.basic_over_shown,
                premium_over_shown: state.premium_over_shown,
                accepts: header_accepts.cloned().or_else(|| state.accepts.clone()),
                path_tier: state.endpoints.get(path).copied(),
            }
        };
        let events = payment_notify::compute_events(&input, now);
        if events.is_empty() {
            return;
        }
        {
            let mut state = self.payment_state();
            for (event, flag) in events {
                payment_notify::push_event(event);
                match flag {
                    Flag::Grace => state.grace_shown = true,
                    Flag::Intro => state.intro_shown = true,
                    Flag::BasicOver => {
                        state.basic_over_shown = true;
                        state.pending_over_quota_tiers.insert(PaymentTier::Basic);
                    }
                    Flag::PremiumOver => {
                        state.premium_over_shown = true;
                        state.pending_over_quota_tiers.insert(PaymentTier::Premium);
                    }
                }
            }
        }
        let _ = self.flush_payment_cache();
    }

    /// Check whether a tier just flipped to charging on this response.
    /// Used by the 402 retry wrapper to short-circuit signing so the
    /// user has a chance to see the `OVER_QUOTA` notification and
    /// re-run the command before any actual payment happens.
    ///
    /// Two cases resolve to "yes, block":
    /// 1. We know the path's tier and it's in `pending_over_quota_tiers`
    ///    → consume that entry and block.
    /// 2. We don't know the path's tier yet (`/config` not fetched
    ///    because charging was only just observed *this* request) but
    ///    the pending set is non-empty → block conservatively and
    ///    drain the set. On the re-run `/config` will have been
    ///    fetched and the tier-specific path works normally.
    fn consume_pending_confirmation(&self, path: &str) -> bool {
        let mut state = self.payment_state();
        if let Some(tier) = state.endpoints.get(path).copied() {
            return state.pending_over_quota_tiers.remove(&tier);
        }
        if !state.pending_over_quota_tiers.is_empty() {
            state.pending_over_quota_tiers.clear();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiClient, PaymentCache, PaymentRequired, PaymentTier};
    use serde_json::Value;

    /// Set AK credential env vars to dummy test values so ApiClient::new() succeeds.
    fn set_test_credentials() {
        std::env::set_var("OKX_API_KEY", "test-api-key");
        std::env::set_var("OKX_SECRET_KEY", "test-secret-key");
        std::env::set_var("OKX_PASSPHRASE", "test-passphrase");
    }

    // ── constants ─────────────────────────────────────────────────────────────

    #[test]
    fn default_base_url_is_beta() {
        assert_eq!(super::DEFAULT_BASE_URL, "https://web3.okx.com");
    }

    #[test]
    fn client_version_matches_cargo() {
        assert_eq!(super::CLIENT_VERSION, env!("CARGO_PKG_VERSION"));
    }

    // ── JWT headers ──────────────────────────────────────────────────────────

    #[test]
    fn jwt_headers_authorization_bearer() {
        // All APIs (DEX, Security, Wallet) use Authorization: Bearer when logged in
        let h = ApiClient::jwt_headers("my-token");
        let v = h
            .get("authorization")
            .expect("authorization header")
            .to_str()
            .unwrap();
        assert_eq!(v, "Bearer my-token");
    }

    #[test]
    fn jwt_headers_client_type_agent_cli() {
        let h = ApiClient::jwt_headers("tok");
        assert_eq!(
            h.get("ok-access-client-type")
                .expect("ok-access-client-type")
                .to_str()
                .unwrap(),
            "agent-cli"
        );
    }

    #[test]
    fn jwt_headers_client_version_present() {
        let h = ApiClient::jwt_headers("tok");
        let v = h
            .get("ok-client-version")
            .expect("ok-client-version")
            .to_str()
            .unwrap();
        assert_eq!(v, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn jwt_headers_content_type_json() {
        let h = ApiClient::jwt_headers("tok");
        assert_eq!(
            h.get("content-type")
                .expect("content-type")
                .to_str()
                .unwrap(),
            "application/json"
        );
    }

    #[test]
    fn jwt_headers_no_ak_fields() {
        let h = ApiClient::jwt_headers("tok");
        assert!(h.get("ok-access-key").is_none());
        assert!(h.get("ok-access-sign").is_none());
        assert!(h.get("ok-access-passphrase").is_none());
        assert!(h.get("ok-access-token").is_none());
        assert!(h.get("ok-client-type").is_none());
    }

    // ── AK headers ───────────────────────────────────────────────────────────

    #[test]
    fn ak_headers_access_key() {
        let h = ApiClient::ak_headers("my-key", "pass", "2024-01-01T00:00:00.000Z", "sign123");
        assert_eq!(
            h.get("ok-access-key")
                .expect("ok-access-key")
                .to_str()
                .unwrap(),
            "my-key"
        );
    }

    #[test]
    fn ak_headers_sign_and_passphrase() {
        let h = ApiClient::ak_headers("key", "my-pass", "ts", "my-sign");
        assert_eq!(
            h.get("ok-access-sign")
                .expect("ok-access-sign")
                .to_str()
                .unwrap(),
            "my-sign"
        );
        assert_eq!(
            h.get("ok-access-passphrase")
                .expect("ok-access-passphrase")
                .to_str()
                .unwrap(),
            "my-pass"
        );
    }

    #[test]
    fn ak_headers_timestamp() {
        let ts = "2024-03-15T10:00:00.000Z";
        let h = ApiClient::ak_headers("k", "p", ts, "s");
        assert_eq!(
            h.get("ok-access-timestamp")
                .expect("ok-access-timestamp")
                .to_str()
                .unwrap(),
            ts
        );
    }

    #[test]
    fn ak_headers_client_type_cli() {
        let h = ApiClient::ak_headers("k", "p", "ts", "s");
        assert_eq!(
            h.get("ok-client-type")
                .expect("ok-client-type")
                .to_str()
                .unwrap(),
            "cli"
        );
    }

    #[test]
    fn ak_headers_client_version_present() {
        let h = ApiClient::ak_headers("k", "p", "ts", "s");
        let v = h
            .get("ok-client-version")
            .expect("ok-client-version")
            .to_str()
            .unwrap();
        assert_eq!(v, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn ak_headers_content_type_json() {
        let h = ApiClient::ak_headers("k", "p", "ts", "s");
        assert_eq!(
            h.get("content-type")
                .expect("content-type")
                .to_str()
                .unwrap(),
            "application/json"
        );
    }

    #[test]
    fn ak_headers_no_jwt_fields() {
        let h = ApiClient::ak_headers("k", "p", "ts", "s");
        assert!(h.get("authorization").is_none());
        // AK mode shares anonymous_headers base so has Ok-Access-Client-type
        assert!(h.get("ok-access-client-type").is_some());
    }

    // ── HMAC sign ─────────────────────────────────────────────────────────────

    #[test]
    fn hmac_sign_is_deterministic() {
        let s1 = ApiClient::hmac_sign(
            "secret",
            "2024-01-01T00:00:00.000Z",
            "GET",
            "/api/v6/test",
            "",
        );
        let s2 = ApiClient::hmac_sign(
            "secret",
            "2024-01-01T00:00:00.000Z",
            "GET",
            "/api/v6/test",
            "",
        );
        assert_eq!(s1, s2);
        assert!(!s1.is_empty());
    }

    #[test]
    fn hmac_sign_differs_by_method() {
        let get = ApiClient::hmac_sign("secret", "ts", "GET", "/path", "");
        let post = ApiClient::hmac_sign("secret", "ts", "POST", "/path", "");
        assert_ne!(get, post);
    }

    #[test]
    fn hmac_sign_differs_by_body() {
        let empty = ApiClient::hmac_sign("secret", "ts", "POST", "/path", "");
        let with_body = ApiClient::hmac_sign("secret", "ts", "POST", "/path", r#"{"foo":"bar"}"#);
        assert_ne!(empty, with_body);
    }

    #[test]
    fn hmac_sign_differs_by_secret() {
        let s1 = ApiClient::hmac_sign("secret-a", "ts", "GET", "/path", "");
        let s2 = ApiClient::hmac_sign("secret-b", "ts", "GET", "/path", "");
        assert_ne!(s1, s2);
    }

    #[test]
    fn hmac_sign_output_is_base64() {
        let sign = ApiClient::hmac_sign("key", "ts", "GET", "/path", "");
        // base64 standard alphabet: A-Z a-z 0-9 + / =
        assert!(sign
            .chars()
            .all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '='));
    }

    // ── URL building ─────────────────────────────────────────────────────────

    #[test]
    fn build_get_request_path_percent_encodes_query_values() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let (_, request_path) = client
            .build_get_url_and_request_path(
                "/api/v6/dex/market/memepump/tokenList",
                &[
                    ("chainIndex", "501"),
                    ("keywordsInclude", "dog wif"),
                    ("keywordsExclude", "狗"),
                    ("empty", ""),
                ],
            )
            .expect("request path");

        assert_eq!(
            request_path,
            "/api/v6/dex/market/memepump/tokenList?chainIndex=501&keywordsInclude=dog+wif&keywordsExclude=%E7%8B%97"
        );
    }

    #[test]
    fn build_get_request_path_no_query_has_no_question_mark() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let (_, request_path) = client
            .build_get_url_and_request_path("/api/v6/dex/token/search", &[])
            .expect("request path");
        assert_eq!(request_path, "/api/v6/dex/token/search");
        assert!(!request_path.contains('?'));
    }

    #[test]
    fn build_get_request_path_filters_empty_values() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let (_, request_path) = client
            .build_get_url_and_request_path("/api/test", &[("a", "1"), ("b", ""), ("c", "3")])
            .expect("request path");
        assert!(request_path.contains("a=1"));
        assert!(request_path.contains("c=3"));
        assert!(!request_path.contains("b="));
    }

    // ── Auth resolution priority (documented) ────────────────────────────────
    // 1. JWT from keyring (access_token) → AuthMode::Jwt — tested via integration/manual
    // 2. AK from env vars → AuthMode::Ak  — tested below
    // 3. No credentials → AuthMode::Anonymous (no error, empty auth headers)

    #[test]
    fn new_with_ak_credentials_succeeds() {
        set_test_credentials();
        assert!(ApiClient::new(None).is_ok());
    }

    #[test]
    fn anonymous_headers_has_no_auth_fields() {
        let h = ApiClient::anonymous_headers();
        assert!(h.get("authorization").is_none());
        assert!(h.get("ok-access-key").is_none());
        assert!(h.get("ok-access-sign").is_none());
    }

    #[test]
    fn anonymous_headers_base_fields() {
        let h = ApiClient::anonymous_headers();
        assert_eq!(
            h.get("content-type").unwrap().to_str().unwrap(),
            "application/json"
        );
        assert_eq!(
            h.get("ok-client-version").unwrap().to_str().unwrap(),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            h.get("ok-access-client-type").unwrap().to_str().unwrap(),
            "agent-cli"
        );
    }

    #[test]
    fn new_respects_base_url_override() {
        set_test_credentials();
        let client = ApiClient::new(Some("https://custom.example.com")).expect("client");
        let (url, _) = client
            .build_get_url_and_request_path("/priapi/v5/wallet/test", &[])
            .expect("url");
        assert!(url.as_str().starts_with("https://custom.example.com"));
    }

    #[test]
    fn dex_paths_respect_base_url_override() {
        set_test_credentials();
        let client = ApiClient::new(Some("https://custom.example.com")).expect("client");
        let (url, _) = client
            .build_get_url_and_request_path("/api/v6/dex/market/candles", &[])
            .expect("url");
        assert!(url.as_str().starts_with("https://custom.example.com"));
    }

    // ── Auto-payment config parsing ───────────────────────────────────────────

    #[test]
    fn apply_config_response_populates_endpoints_from_endpoint_list() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let data = serde_json::json!({
            "endpointList": {
                "/api/v6/dex/market/trades": "BASIC",
                "/api/v6/dex/market/memepump/tokenDeveloper": "PREMIUM",
                "/api/v6/dex/market/ignored": "UNKNOWN"
            },
            "accepts": [
                {"scheme":"exact","network":"eip155:196","amount":{"basic":"100","premium":"500"}}
            ]
        });
        client.apply_config_response(&data);

        let state = client.payment_state();
        assert_eq!(
            state.endpoints.get("/api/v6/dex/market/trades").copied(),
            Some(PaymentTier::Basic)
        );
        assert_eq!(
            state
                .endpoints
                .get("/api/v6/dex/market/memepump/tokenDeveloper")
                .copied(),
            Some(PaymentTier::Premium)
        );
        // Unknown tier strings are dropped silently.
        assert!(!state.endpoints.contains_key("/api/v6/dex/market/ignored"));
        assert!(state.accepts.is_some());
    }

    #[test]
    fn apply_config_response_tolerates_missing_endpoint_list() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let data = serde_json::json!({});
        client.apply_config_response(&data);
        assert!(client.payment_state().endpoints.is_empty());
    }

    #[test]
    fn restore_from_cache_preserves_charging_flags_when_expired() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let cache = PaymentCache {
            endpoints: [("/api/v6/dex/market/price".to_string(), "basic".to_string())]
                .into_iter()
                .collect(),
            accepts: Some(serde_json::json!([{"scheme": "exact"}])),
            basic_charging: true,
            premium_charging: true,
            // Stale enough to be expired at any sane TTL.
            updated_at: 0,
            ..Default::default()
        };
        let fresh = client.restore_from_cache(cache);
        assert!(!fresh, "expired cache should not satisfy config freshness");
        let state = client.payment_state();
        // Charging flags survive.
        assert!(state.basic_charging);
        assert!(state.premium_charging);
        // Config portion (endpoints/accepts) is left untouched so the fetch
        // path below refreshes them from the server.
        assert!(state.endpoints.is_empty());
        assert!(state.accepts.is_none());
        assert!(!state.config_loaded);
    }

    #[test]
    fn restore_from_cache_loads_full_state_when_fresh() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let cache = PaymentCache {
            endpoints: [("/api/v6/dex/market/price".to_string(), "basic".to_string())]
                .into_iter()
                .collect(),
            accepts: Some(serde_json::json!([{"scheme": "exact"}])),
            basic_charging: true,
            premium_charging: false,
            updated_at: crate::payment_cache::now_secs(),
            ..Default::default()
        };
        let fresh = client.restore_from_cache(cache);
        assert!(fresh);
        let state = client.payment_state();
        assert!(state.basic_charging);
        assert!(!state.premium_charging);
        assert_eq!(
            state.endpoints.get("/api/v6/dex/market/price").copied(),
            Some(PaymentTier::Basic)
        );
        assert!(state.accepts.is_some());
        assert!(state.config_loaded);
    }

    #[test]
    fn tier_for_path_returns_none_when_unknown() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        assert_eq!(client.tier_for_path("/api/v6/dex/market/unknown"), None);
    }

    #[test]
    fn consume_pending_confirmation_returns_true_then_clears() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        // Seed endpoints so the tier lookup succeeds.
        client.apply_config_response(&serde_json::json!({
            "endpointList": { "/api/v6/dex/market/price": "BASIC" },
            "accepts": [{"scheme":"exact"}]
        }));
        // Simulate dispatch adding the tier to the pending set.
        client
            .payment_state()
            .pending_over_quota_tiers
            .insert(PaymentTier::Basic);

        assert!(client.consume_pending_confirmation("/api/v6/dex/market/price"));
        // Second call should be false — pending set is one-shot.
        assert!(!client.consume_pending_confirmation("/api/v6/dex/market/price"));
    }

    #[test]
    fn consume_pending_confirmation_falls_back_when_endpoints_empty() {
        // First-flip race: /config has not been fetched yet, so endpoints
        // is empty. We should still block confirming based on the pending
        // set alone, otherwise the first paid request signs with the
        // wrong tier and the user never sees the notification.
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        client
            .payment_state()
            .pending_over_quota_tiers
            .insert(PaymentTier::Premium);

        assert!(client.consume_pending_confirmation("/api/v6/dex/market/price-info"));
        // Set is drained after a successful block.
        assert!(client.payment_state().pending_over_quota_tiers.is_empty());
        // Second call returns false — one-shot.
        assert!(!client.consume_pending_confirmation("/api/v6/dex/market/price-info"));
    }

    #[test]
    fn consume_pending_confirmation_returns_false_when_nothing_pending() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        // Empty pending set, empty endpoints → nothing to confirm.
        assert!(!client.consume_pending_confirmation("/api/v6/dex/market/whatever"));
    }

    #[test]
    fn resolve_retry_accepts_returns_fresh_without_touching_cache() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let fresh = serde_json::json!([
            {"scheme":"exact","network":"eip155:196","amount":{"basic":"200"}}
        ]);
        let pr = PaymentRequired {
            accepts: fresh.clone(),
            raw_body: Value::Null,
        };
        let got = client.resolve_retry_accepts(&pr).expect("accepts");
        assert_eq!(got, fresh);
        // 402 accepts carry a single tier — they must not overwrite the
        // tier-aware map persisted from `/config`.
        assert!(client.payment_state().accepts.is_none());
    }

    #[test]
    fn resolve_retry_accepts_falls_back_to_cached_when_response_empty() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let cached = serde_json::json!([
            {"scheme":"exact","network":"eip155:196","amount":{"basic":"100"}}
        ]);
        client.apply_config_response(&serde_json::json!({ "accepts": cached.clone() }));

        let pr = PaymentRequired {
            accepts: Value::Null,
            raw_body: Value::Null,
        };
        let got = client.resolve_retry_accepts(&pr).expect("cached accepts");
        assert_eq!(got, cached);
    }

    #[test]
    fn resolve_retry_accepts_errors_when_both_sources_empty() {
        set_test_credentials();
        let client = ApiClient::new(None).expect("client");
        let pr = PaymentRequired {
            accepts: Value::Null,
            raw_body: Value::Null,
        };
        assert!(client.resolve_retry_accepts(&pr).is_err());
    }

    #[test]
    fn extract_header_flag_parses_mixed_case_and_ignores_unknown() {
        assert_eq!(
            ApiClient::extract_header_flag("Basic=1;Premium=0", "basic"),
            Some(true)
        );
        assert_eq!(
            ApiClient::extract_header_flag("basic=0;Premium=1", "Premium"),
            Some(true)
        );
        assert_eq!(ApiClient::extract_header_flag("Basic=1", "Premium"), None);
        assert_eq!(ApiClient::extract_header_flag("Basic=maybe", "Basic"), None);
    }
}
