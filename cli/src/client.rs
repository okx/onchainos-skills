use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;

use crate::payment_cache::{self, PaymentCache};

pub const DEFAULT_BASE_URL: &str = "https://web3.okx.com";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Market API config endpoint — returns `basic`/`premium` path lists plus the
/// default `accepts` signing parameters. Refreshed at most once per `CONFIG_TTL_SECS`.
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
    basic_paths: HashSet<String>,
    premium_paths: HashSet<String>,
    accepts: Option<Value>,
    basic_charging: bool,
    premium_charging: bool,
    /// `true` once we've tried to populate state this process. Prevents
    /// redundant config fetches across concurrent requests on the same client.
    config_loaded: bool,
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

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    base_url: String,
    auth: AuthMode,
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

        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            base_url,
            auth,
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
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            base_url,
            auth,
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

        let mut url =
            reqwest::Url::parse(&format!("{}{}", self.base_url.trim_end_matches('/'), path))?;

        if !filtered.is_empty() {
            url.query_pairs_mut().extend_pairs(filtered.iter().copied());
        }

        let query_string = url
            .query()
            .map(|query| format!("?{}", query))
            .unwrap_or_default();
        let request_path = format!("{}{}", path, query_string);

        Ok((url, request_path))
    }

    /// GET request with automatic auth (JWT or AK).
    pub async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.get_with_headers(path, query, None).await
    }

    /// GET request with automatic auth + optional extra headers.
    ///
    /// Wraps the request in the auto-payment flow:
    /// 1. Ensure payment config is loaded (first request only).
    /// 2. If the path is currently on a charging tier, pre-sign a payment header.
    /// 3. Send the request.
    /// 4. On 402, sign with the accepts returned by the server and retry once.
    pub async fn get_with_headers(
        &self,
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
                    self.update_accepts_cache(&pr.accepts);
                    let hdr = self
                        .sign_header_from_accepts(&pr.accepts, &resource)
                        .await?;
                    self.do_get_request(path, query, extra_headers, Some(&hdr))
                        .await
                }
                Err(e) => Err(e),
            },
        }
    }

    /// POST request with automatic auth (JWT or AK).
    /// Signature uses path only (no query string) + JSON body string.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.post_with_headers(path, body, None).await
    }

    /// POST request with automatic auth + optional extra headers.
    /// Mirrors `get_with_headers`: pre-signs on known-paid paths and retries once on 402.
    pub async fn post_with_headers(
        &self,
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
                    self.update_accepts_cache(&pr.accepts);
                    let hdr = self
                        .sign_header_from_accepts(&pr.accepts, &resource)
                        .await?;
                    self.do_post_request(path, body, extra_headers, Some(&hdr))
                        .await
                }
                Err(e) => Err(e),
            },
        }
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

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();

        // Read charging-state + V2 PAYMENT-REQUIRED headers before consuming
        // the body so even error responses (402, 5xx, code!=0) still keep our
        // state in sync and give us access to the accepts payload.
        self.update_payment_state_from_headers(resp.headers());
        let header_accepts = extract_payment_required_accepts(resp.headers());

        if status.as_u16() == 429 {
            bail!("Rate limited — retry with backoff");
        }
        if status.as_u16() >= 500 {
            bail!("Server error (HTTP {})", status.as_u16());
        }

        // An empty body is legitimate for a standard-compliant 402 — accepts
        // come from the PAYMENT-REQUIRED header in that case. Otherwise empty
        // is an error.
        let body_bytes = resp.bytes().await.context("failed to read response body")?;
        if body_bytes.is_empty() {
            if status.as_u16() == 402 && header_accepts.is_some() {
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

    async fn do_get_request(
        &self,
        path: &str,
        query: &[(&str, &str)],
        extra_headers: Option<&[(&str, &str)]>,
        payment_hdr: Option<&(&'static str, String)>,
    ) -> Result<Value> {
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

        let resp = req.send().await.context("request failed")?;
        self.handle_response(resp).await
    }

    async fn do_post_request(
        &self,
        path: &str,
        body: &Value,
        extra_headers: Option<&[(&str, &str)]>,
        payment_hdr: Option<&(&'static str, String)>,
    ) -> Result<Value> {
        let body_str = serde_json::to_string(body)?;
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
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

        let resp = req.send().await.context("request failed")?;
        self.handle_response(resp).await
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

    /// Load payment config once per process. Cache file is consulted first;
    /// a fresh fetch only runs if the cache is missing or older than
    /// `CONFIG_TTL_SECS`. Failures degrade silently — the 402 fallback in
    /// `handle_response` will still recover.
    async fn ensure_payment_config(&self) {
        if self.payment_state().config_loaded {
            return;
        }

        if let Some(cache) = PaymentCache::load() {
            if !cache.is_expired(CONFIG_TTL_SECS) {
                let mut state = self.payment_state();
                state.basic_paths = cache.basic_paths;
                state.premium_paths = cache.premium_paths;
                state.accepts = cache.accepts;
                state.basic_charging = cache.basic_charging;
                state.premium_charging = cache.premium_charging;
                state.config_loaded = true;
                return;
            }
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

    fn apply_config_response(&self, data: &Value) {
        let mut state = self.payment_state();
        state.basic_paths.clear();
        state.premium_paths.clear();
        for u in data["basic"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            state.basic_paths.insert(u.to_string());
        }
        for u in data["premium"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            state.premium_paths.insert(u.to_string());
        }
        if let Some(a) = data.get("accepts") {
            if !a.is_null() {
                state.accepts = Some(a.clone());
            }
        }
    }

    // ── Auto-payment: header parsing ─────────────────────────────────────────

    /// Update `basic_charging`/`premium_charging` from the
    /// `ok-web3-openapi-pay: Basic=1;Premium=0` response header. Writes to
    /// disk only when a flag actually flips — every other request is IO-free.
    fn update_payment_state_from_headers(&self, headers: &reqwest::header::HeaderMap) {
        let Some(raw) = headers
            .get(PAYMENT_STATE_HEADER)
            .and_then(|v| v.to_str().ok())
        else {
            return;
        };
        let basic = Self::extract_header_flag(raw, "Basic");
        let premium = Self::extract_header_flag(raw, "Premium");

        let changed = {
            let mut state = self.payment_state();
            let mut changed = false;
            if let Some(b) = basic {
                if state.basic_charging != b {
                    state.basic_charging = b;
                    changed = true;
                }
            }
            if let Some(p) = premium {
                if state.premium_charging != p {
                    state.premium_charging = p;
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
        header.split(';').find_map(|part| {
            let mut it = part.trim().splitn(2, '=');
            let k = it.next()?.trim();
            let v = it.next()?.trim();
            if k.eq_ignore_ascii_case(key) {
                match v {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                }
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

    /// Build a payment header for `path` if we know it's on a charging tier.
    /// Returns `None` if the path isn't charged, or if signing itself fails
    /// (e.g. the wallet isn't logged in — the request will then naturally hit
    /// the 402 fallback below).
    async fn maybe_sign_payment(
        &self,
        path: &str,
        resource: &str,
    ) -> Option<(&'static str, String)> {
        let (needs, accepts) = {
            let state = self.payment_state();
            let needs = (state.basic_paths.contains(path) && state.basic_charging)
                || (state.premium_paths.contains(path) && state.premium_charging);
            (needs, state.accepts.clone())
        };
        if !needs {
            return None;
        }
        let accepts = accepts?;
        self.sign_header_from_accepts(&accepts, resource).await.ok()
    }

    /// Sign a V2 payment header from a raw accepts value (from config or from
    /// a 402 response). OKX openapi follows standard x402 V2 (`PAYMENT-SIGNATURE`).
    async fn sign_header_from_accepts(
        &self,
        accepts: &Value,
        resource: &str,
    ) -> Result<(&'static str, String)> {
        let (proof, selected) =
            crate::commands::agentic_wallet::payment_flow::sign_payment(accepts, None).await?;
        crate::commands::agentic_wallet::payment_flow::build_payment_header(
            &proof,
            &selected,
            crate::commands::agentic_wallet::payment_flow::PaymentMode::V2 {
                resource: resource.to_string(),
            },
        )
    }

    /// Overwrite the in-memory `accepts` with a fresh copy from a 402 response
    /// and persist. Keeps subsequent requests from needing another round-trip.
    fn update_accepts_cache(&self, accepts: &Value) {
        if accepts.is_null() {
            return;
        }
        {
            let mut state = self.payment_state();
            state.accepts = Some(accepts.clone());
        }
        let _ = self.flush_payment_cache();
    }

    /// Write the current in-memory state to `~/.onchainos/payment_cache.json`.
    fn flush_payment_cache(&self) -> Result<()> {
        let state = self.payment_state();
        let cache = PaymentCache {
            basic_paths: state.basic_paths.clone(),
            premium_paths: state.premium_paths.clone(),
            accepts: state.accepts.clone(),
            basic_charging: state.basic_charging,
            premium_charging: state.premium_charging,
            updated_at: payment_cache::now_secs(),
        };
        drop(state);
        cache.save()
    }
}

#[cfg(test)]
mod tests {
    use super::ApiClient;

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
}
