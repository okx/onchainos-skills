use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Result};
use base64::Engine;
use serde_json::json;

use crate::keyring_store;
use crate::output;
use crate::wallet_api::{ApiCodeError, WalletApiClient};
use crate::wallet_store::{self, AccountMapEntry, AddressInfo, SessionJson, WalletsJson};

// ── Token / session helpers ──────────────────────────────────────────

/// Ensure accessToken and refreshToken exist and the session is still valid.
pub(super) fn ensure_tokens() -> Result<(String, String)> {
    let session = wallet_store::load_session()?;
    let expire_at = session
        .as_ref()
        .map(|s| s.session_key_expire_at.as_str())
        .unwrap_or("");

    if cfg!(feature = "debug-log") {
        let now_ts = chrono::Utc::now().timestamp();
        let exp_ts = expire_at.parse::<i64>().unwrap_or(0);
        let exp_dt = chrono::DateTime::from_timestamp(exp_ts, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "invalid".to_string());
        let now_dt = chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        let diff = exp_ts - now_ts;
        eprintln!(
            "[DEBUG][ensure_tokens] session_key_expire_at: value=\"{}\", parsed_exp={} ({}), now={} ({}), diff={}s ({:.1}min, {:.1}h), expired={}",
            expire_at, exp_ts, exp_dt, now_ts, now_dt, diff, diff as f64 / 60.0, diff as f64 / 3600.0, now_ts >= exp_ts
        );
    }

    if is_session_key_expired(expire_at) {
        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][session_key_expired] session key expired");
        }
        bail!("session expired, please login again: onchainos wallet login");
    }

    let blob = keyring_store::read_blob()?;

    let refresh_token = match blob.get("refresh_token").filter(|t| !t.is_empty()) {
        Some(t) => t.clone(),
        _ => bail!(super::common::ERR_NOT_LOGGED_IN),
    };
    if cfg!(feature = "debug-log") {
        let now_ts = chrono::Utc::now().timestamp();
        if let Some(exp_ts) = token_exp_timestamp(&refresh_token) {
            let exp_dt = chrono::DateTime::from_timestamp(exp_ts, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "invalid".to_string());
            let now_dt = chrono::Utc::now()
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string();
            let diff = exp_ts - now_ts;
            eprintln!(
                "[DEBUG][ensure_tokens] refresh_token: exp={} ({}), now={} ({}), diff={}s ({:.1}min, {:.1}h), expired={}",
                exp_ts, exp_dt, now_ts, now_dt, diff, diff as f64 / 60.0, diff as f64 / 3600.0, now_ts >= exp_ts
            );
        } else {
            eprintln!("[DEBUG][ensure_tokens] refresh_token: failed to parse exp from JWT");
        }
    }

    if is_token_expired(&refresh_token) {
        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][refresh_token] refresh token expired");
        }
        bail!("session expired, please login again: onchainos wallet login");
    }

    let access_token = match blob.get("access_token").filter(|t| !t.is_empty()) {
        Some(t) => t.clone(),
        _ => bail!(super::common::ERR_NOT_LOGGED_IN),
    };

    if cfg!(feature = "debug-log") {
        let now_ts = chrono::Utc::now().timestamp();
        if let Some(exp_ts) = token_exp_timestamp(&access_token) {
            let exp_dt = chrono::DateTime::from_timestamp(exp_ts, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "invalid".to_string());
            let now_dt = chrono::Utc::now()
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string();
            let diff = exp_ts - now_ts;
            eprintln!(
                "[DEBUG][ensure_tokens] access_token: exp={} ({}), now={} ({}), diff={}s ({:.1}min, {:.1}h), expired={}",
                exp_ts, exp_dt, now_ts, now_dt, diff, diff as f64 / 60.0, diff as f64 / 3600.0, now_ts >= exp_ts
            );
        } else {
            eprintln!("[DEBUG][ensure_tokens] access_token: failed to parse exp from JWT");
        }
    }

    Ok((access_token, refresh_token))
}

/// Returns a valid accessToken, refreshing only when it is actually expired.
///
/// Flow:
///   1. session_key expired           → bail, ask the user to log in again
///   2. no tokens in keychain         → bail, ask the user to log in again
///   3. refresh_token expired         → bail, ask the user to log in again
///   4. access_token expired          → call auth_refresh, store new tokens, return new JWT
///   5. access_token still valid      → return as-is
pub(crate) async fn ensure_tokens_refreshed() -> Result<String> {
    // ── Step 1: session_key guard ────────────────────────────────────
    let session = wallet_store::load_session()?;
    let expire_at = session
        .as_ref()
        .map(|s| s.session_key_expire_at.as_str())
        .unwrap_or("");

    if cfg!(feature = "debug-log") {
        let now_ts = chrono::Utc::now().timestamp();
        let exp_ts = expire_at.parse::<i64>().unwrap_or(0);
        let diff = exp_ts - now_ts;
        eprintln!(
            "[DEBUG][ensure_tokens_refreshed] session_key_expire_at=\"{}\", diff={}s, expired={}",
            expire_at,
            diff,
            now_ts >= exp_ts
        );
    }

    if is_session_key_expired(expire_at) {
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][ensure_tokens_refreshed] session key expired → session expired (bail)"
            );
        }
        return session_expired_err();
    }

    // ── Step 2: read tokens from keychain ────────────────────────────
    let blob = keyring_store::read_blob()?;

    let refresh_token = match blob.get("refresh_token").filter(|t| !t.is_empty()) {
        Some(t) => t.clone(),
        _ => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] no refresh_token → session expired (bail)"
                );
            }
            return session_expired_err();
        }
    };

    let access_token = match blob.get("access_token").filter(|t| !t.is_empty()) {
        Some(t) => t.clone(),
        _ => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] no access_token → session expired (bail)"
                );
            }
            return session_expired_err();
        }
    };

    // ── Step 3: refresh_token expired → prompt + try AK re-login ────
    if cfg!(feature = "debug-log") {
        let now_ts = chrono::Utc::now().timestamp();
        if let Some(exp_ts) = token_exp_timestamp(&refresh_token) {
            let diff = exp_ts - now_ts;
            eprintln!(
                "[DEBUG][ensure_tokens_refreshed] refresh_token: diff={}s ({:.1}h), expired={}",
                diff,
                diff as f64 / 3600.0,
                now_ts >= exp_ts
            );
        }
    }

    if is_token_expired(&refresh_token) {
        eprintln!("Session expired. Please log in again: onchainos wallet login");
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][ensure_tokens_refreshed] refresh_token expired → session expired (bail)"
            );
        }
        return session_expired_err();
    }

    // ── Step 4: access_token expired → refresh via API ───────────────
    if is_token_expired(&access_token) {
        let mut client = WalletApiClient::new()?;
        let resp = client
            .auth_refresh(&refresh_token)
            .await
            .map_err(format_api_error)?;

        if cfg!(feature = "debug-log") {
            let now_ts = chrono::Utc::now().timestamp();
            let now_dt = chrono::Utc::now()
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string();
            if let Some(old_exp) = token_exp_timestamp(&access_token) {
                let old_exp_dt = chrono::DateTime::from_timestamp(old_exp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "invalid".to_string());
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] old access_token: exp={} ({}), was expired by {}s",
                    old_exp, old_exp_dt, now_ts - old_exp
                );
            }
            if let Some(new_exp) = token_exp_timestamp(&resp.access_token) {
                let new_exp_dt = chrono::DateTime::from_timestamp(new_exp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "invalid".to_string());
                let diff = new_exp - now_ts;
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] new access_token: exp={} ({}), now={} ({}), diff={}s ({:.1}min, {:.1}h)",
                    new_exp, new_exp_dt, now_ts, now_dt, diff, diff as f64 / 60.0, diff as f64 / 3600.0
                );
            }
            if let Some(new_rexp) = token_exp_timestamp(&resp.refresh_token) {
                let new_rexp_dt = chrono::DateTime::from_timestamp(new_rexp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "invalid".to_string());
                let diff = new_rexp - now_ts;
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] new refresh_token: exp={} ({}), now={} ({}), diff={}s ({:.1}min, {:.1}h)",
                    new_rexp, new_rexp_dt, now_ts, now_dt, diff, diff as f64 / 60.0, diff as f64 / 3600.0
                );
            }
        }

        keyring_store::store(&[
            ("access_token", &resp.access_token),
            ("refresh_token", &resp.refresh_token),
        ])?;

        if resp.chain_updated && !resp.all_account_address_list.is_empty() {
            apply_all_account_address_list(&resp.all_account_address_list);
            super::chain::force_refresh_chain_cache().await;
        }

        return Ok(resp.access_token);
    }

    // ── Step 5: access_token still valid ─────────────────────────────
    Ok(access_token)
}

/// Called when the session/refresh token is expired. Social login can't be
/// recovered automatically, so bail and ask the user to log in again.
fn session_expired_err() -> Result<String> {
    bail!("session expired, please login again: onchainos wallet login")
}

/// Decode JWT and check if it is expired.
pub(super) fn is_token_expired(token: &str) -> bool {
    token_exp_timestamp(token)
        .map(|exp| {
            let now = chrono::Utc::now().timestamp();
            now >= exp
        })
        .unwrap_or(true)
}

/// Extract `exp` claim from a JWT without signature verification.
fn token_exp_timestamp(token: &str) -> Option<i64> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let val: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    val["exp"].as_i64()
}

/// Check if `session_key_expire_at` timestamp has passed.
pub(super) fn is_session_key_expired(expire_at: &str) -> bool {
    if expire_at.is_empty() {
        return true;
    }
    match expire_at.parse::<i64>() {
        Ok(exp) => chrono::Utc::now().timestamp() >= exp,
        Err(_) => true,
    }
}

/// Format an API error for propagation.
pub(crate) fn format_api_error(e: anyhow::Error) -> anyhow::Error {
    match e.downcast::<ApiCodeError>() {
        Ok(api_err) => anyhow::anyhow!("code={} msg={}", api_err.code, api_err.msg),
        Err(e) => e,
    }
}

// ── Social login ─────────────────────────────────────────────────────

/// Path of the social-login page, joined onto the effective base URL.
const SOCIAL_LOGIN_PATH: &str = "/account/sociallogin";

/// Effective base URL for the login page (same resolution as `WalletApiClient`).
fn social_login_base_url() -> String {
    std::env::var("OKX_BASE_URL")
        .ok()
        .or_else(|| option_env!("OKX_BASE_URL").map(|s| s.to_string()))
        .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string())
}

/// Build the login-page URL
/// `<base>/account/sociallogin?authSessionId=..&tempPubKey=..&clientType=agent-cli`.
/// `temp_pub_key` is base64 (may contain `+ / =`), so params are percent-encoded.
fn build_login_url(base_url: &str, auth_session_id: &str, temp_pub_key: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("authSessionId", auth_session_id)
        .append_pair("tempPubKey", temp_pub_key)
        .append_pair("clientType", "agent-cli")
        .finish();
    format!(
        "{}{}?{}",
        base_url.trim_end_matches('/'),
        SOCIAL_LOGIN_PATH,
        query
    )
}

/// What one `session/result` poll means for the polling loop.
enum PollOutcome {
    /// accessToken present → login complete.
    Ready,
    /// Backend says not-ready yet (code 10018 or `Ok` without a token) → keep
    /// polling; this is the normal "user hasn't finished login" state.
    Pending,
    /// No-code transport/parse error (network down, persistent 5xx) → keep
    /// polling, but count toward the consecutive-failure short-circuit so a
    /// hard outage doesn't make the user wait the full timeout.
    Transient,
    /// Terminal backend error → stop.
    Terminal,
}

/// Classify one `session_result` outcome. `Ok` is Ready only with a non-empty
/// accessToken (else Pending); an `Err` is Pending for code 10018, Transient
/// for a no-code transport/parse error, and Terminal for any other backend code.
fn classify_poll(result: &Result<serde_json::Value>) -> PollOutcome {
    match result {
        Ok(data) => {
            let has_token = data
                .get("accessToken")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
            if has_token {
                PollOutcome::Ready
            } else {
                PollOutcome::Pending
            }
        }
        Err(e) => match e.downcast_ref::<ApiCodeError>().map(|a| a.code.as_str()) {
            Some("10018") => PollOutcome::Pending,
            None => PollOutcome::Transient,
            Some(_) => PollOutcome::Terminal,
        },
    }
}

/// Consecutive no-code (transport) failures after which polling gives up
/// instead of retrying until the full timeout. At the 2s cadence this is ~10s
/// of continuous failure; any successful poll or backend `10018` resets it.
const MAX_CONSECUTIVE_TRANSIENT_POLLS: u32 = 5;

/// Default social-login poll timeout when `SOCIAL_LOGIN_TIMEOUT_SECS` is unset
/// or invalid (5 minutes).
const SOCIAL_LOGIN_TIMEOUT_DEFAULT_SECS: u64 = 300;
/// Minimum accepted override; values below this fall back to the default.
const SOCIAL_LOGIN_TIMEOUT_FLOOR_SECS: u64 = 10;

/// Resolve the poll timeout from a raw `SOCIAL_LOGIN_TIMEOUT_SECS` value.
/// Unset / unparseable / below-floor inputs all fall back to the default.
fn resolve_social_login_timeout_secs(raw: Option<&str>) -> u64 {
    raw.and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v >= SOCIAL_LOGIN_TIMEOUT_FLOOR_SECS)
        .unwrap_or(SOCIAL_LOGIN_TIMEOUT_DEFAULT_SECS)
}

/// Poll `session/result` until login completes or the deadline elapses.
/// Cadence: 3s interval, default 300s (5 min) timeout (override via
/// `SOCIAL_LOGIN_TIMEOUT_SECS`, floor 10s).
async fn poll_session_result(
    client: &mut WalletApiClient,
    auth_session_id: &str,
) -> Result<serde_json::Value> {
    let interval = Duration::from_secs(3);
    let timeout_secs = resolve_social_login_timeout_secs(
        std::env::var("SOCIAL_LOGIN_TIMEOUT_SECS").ok().as_deref(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut consecutive_transient: u32 = 0;

    loop {
        let result = client.session_result(auth_session_id).await;
        match classify_poll(&result) {
            PollOutcome::Ready => return result,
            PollOutcome::Terminal => {
                // Terminal is only produced for an Err result (see classify_poll);
                // move the error out without unwrapping.
                return Err(result
                    .err()
                    .map_or_else(|| anyhow::anyhow!("login failed"), format_api_error));
            }
            PollOutcome::Transient => {
                consecutive_transient += 1;
                if consecutive_transient >= MAX_CONSECUTIVE_TRANSIENT_POLLS {
                    // A sustained transport failure — stop instead of retrying
                    // silently until the full timeout.
                    return Err(result.err().map_or_else(
                        || anyhow::anyhow!("login polling failed"),
                        format_api_error,
                    ));
                }
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG][poll_session_result] transient error ({consecutive_transient}/{MAX_CONSECUTIVE_TRANSIENT_POLLS}), retrying"
                    );
                }
            }
            PollOutcome::Pending => {
                consecutive_transient = 0;
                if cfg!(feature = "debug-log") {
                    eprintln!("[DEBUG][poll_session_result] not ready, polling");
                }
            }
        }

        if std::time::Instant::now() >= deadline {
            bail!("login timed out waiting for the result. The login link is still valid — finish login in the browser, then check the result again (`onchainos wallet login --phase poll`), or start a fresh login (`onchainos wallet login --phase init`)");
        }
        tokio::time::sleep(interval).await;
    }
}


/// Best-effort, non-blocking open of a URL in the system browser: spawns the
/// opener detached and returns immediately, so the caller never blocks on it.
/// Returns `false` on spawn failure or when `ONCHAINOS_NO_BROWSER` is set.
fn try_open_browser(url: &str) -> bool {
    if std::env::var_os("ONCHAINOS_NO_BROWSER").is_some() {
        return false;
    }
    open::that_detached(url).is_ok()
}

/// Whether `url` is safe to hand to the system opener: only `http`/`https`,
/// never `file://` or other schemes the OS would dispatch to a local handler.
fn is_browsable_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https"))
        .unwrap_or(false)
}

/// Keyring keys holding the in-progress login state between phased `login`
/// invocations (`init` writes them, `poll` reads then clears them). The
/// session private key is a secret, so it lives in the keyring — never a
/// plaintext file.
///
/// `PENDING_AUTH_SESSION_ID` points at the most-recent `init` session and is
/// used by a `--phase poll` with no explicit `--session-id`. The session
/// private key is stored per-session under `pending_session_key:<id>`. A new
/// `init` discards the previous pending session's key, so at most one pending
/// session is retained: a timed-out `poll` can still be retried (its state is
/// only cleared on success), but starting over abandons the prior session.
const PENDING_AUTH_SESSION_ID: &str = "pending_auth_session_id";
const PENDING_SESSION_KEY_PREFIX: &str = "pending_session_key:";

/// Keyring key holding the session private key for a given auth session id.
fn pending_session_key_name(auth_session_id: &str) -> String {
    format!("{PENDING_SESSION_KEY_PREFIX}{auth_session_id}")
}

/// Mint a fresh login session: `(auth_session_id, session_private_key, login_url)`.
/// `temp_pub_key` travels in the URL so the page can HPKE-encrypt the session
/// seed; `session_private_key` stays local and is later stored as `session_key`.
fn new_login_session() -> (String, String, String) {
    let auth_session_id = uuid::Uuid::new_v4().to_string();
    let (session_private_key, temp_pub_key) = crate::crypto::generate_x25519_session_keypair();
    let login_url = build_login_url(&social_login_base_url(), &auth_session_id, &temp_pub_key);
    (auth_session_id, session_private_key, login_url)
}

/// Poll for the verify result, persist the session, and emit the account
/// summary. Shared by the `poll` phase and the legacy all-in-one flow.
async fn complete_login(
    client: &mut WalletApiClient,
    auth_session_id: &str,
    session_private_key: &str,
) -> Result<()> {
    let result = poll_session_result(client, auth_session_id).await?;

    let resp: crate::wallet_api::VerifyResponse = serde_json::from_value(result)
        .map_err(|e| anyhow::anyhow!("social login: failed to parse result: {e}"))?;
    let email = resp.login_info.email.clone();
    let login_type = resp.login_info.login_type.clone();

    save_verify_result(client, &resp, session_private_key, &email).await?;

    // Best-effort balance + identity → login-success summary for the skill.
    let wallets = wallet_store::load_wallets()?.unwrap_or_default();
    let mut summary =
        super::balance::login_account_summary(client, &resp.access_token, &wallets, &resp.account_id)
            .await;
    if let Some(obj) = summary.as_object_mut() {
        obj.insert("accountId".to_string(), json!(resp.account_id));
        obj.insert("loginType".to_string(), json!(login_type));
        obj.insert("email".to_string(), json!(email));
        obj.insert("isNew".to_string(), json!(resp.is_new));
    }
    output::success(summary);
    Ok(())
}

/// Phase `init`: mint the login session, persist its state for `poll`,
/// best-effort open the URL, and return `{ loginUrl, authSessionId, opened }`.
pub(super) async fn cmd_login_init() -> Result<()> {
    // Drop the previous pending session's key so repeated `init`s don't accumulate.
    if let Some(prev) = keyring_store::get_opt(PENDING_AUTH_SESSION_ID).filter(|s| !s.is_empty()) {
        let _ = keyring_store::delete(&pending_session_key_name(&prev));
    }

    let (auth_session_id, session_private_key, login_url) = new_login_session();

    let pending_key = pending_session_key_name(&auth_session_id);
    keyring_store::store(&[
        (PENDING_AUTH_SESSION_ID, auth_session_id.as_str()),
        (pending_key.as_str(), session_private_key.as_str()),
    ])?;

    // Best-effort, non-blocking open; `loginUrl` is returned regardless.
    let opened = is_browsable_url(&login_url) && try_open_browser(&login_url);

    output::success(json!({
        "loginUrl": login_url,
        "authSessionId": auth_session_id,
        "opened": opened,
    }));
    Ok(())
}

/// Phase `open`: best-effort open of the login URL in the system browser.
/// Never fatal — a `false` result just means the user opens it manually.
pub(super) async fn cmd_login_open(url: &str) -> Result<()> {
    if !is_browsable_url(url) {
        bail!("`--url` must be an http(s) URL");
    }
    let opened = try_open_browser(url);
    output::success(json!({ "opened": opened }));
    Ok(())
}

/// Phase `poll`: using the state saved by `init`, poll for the verify result,
/// persist the session, then clear the ephemeral state.
///
/// `session_id` selects which `init` session to complete. When `None`, the
/// most-recent `init` session (`PENDING_AUTH_SESSION_ID`) is used — this is the
/// common single-login path. Passing an explicit id targets a specific session;
/// note a newer `init` discards the previous session's key, so only the most
/// recent pending session can still be polled.
pub(super) async fn cmd_login_poll(session_id: Option<&str>) -> Result<()> {
    let auth_session_id = match session_id.filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => keyring_store::get_opt(PENDING_AUTH_SESSION_ID)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no login in progress — run `onchainos wallet login --phase init` first"
                )
            })?,
    };

    let pending_key = pending_session_key_name(&auth_session_id);
    let session_private_key = keyring_store::get_opt(&pending_key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no login in progress for this session — run `onchainos wallet login --phase init` first"
            )
        })?;

    let mut client = WalletApiClient::new()?;
    complete_login(&mut client, &auth_session_id, &session_private_key).await?;

    // Clear the ephemeral state only after a successful login, so a timed-out
    // `poll` can be retried against the same session.
    let _ = keyring_store::delete(&pending_key);
    if keyring_store::get_opt(PENDING_AUTH_SESSION_ID).as_deref() == Some(auth_session_id.as_str()) {
        let _ = keyring_store::delete(PENDING_AUTH_SESSION_ID);
    }
    Ok(())
}

/// Persist credentials and fetch accounts. Emits no output — the caller prints
/// the login-success summary.
async fn save_verify_result(
    client: &mut WalletApiClient,
    resp: &crate::wallet_api::VerifyResponse,
    session_private_key: &str,
    email: &str,
) -> Result<()> {
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] save_verify_result: email={email}, is_new={}, project_id={}, account_id={}",
            resp.is_new, resp.project_id, resp.account_id
        );
        eprintln!(
            "[DEBUG] keyring data lengths: refresh_token={}, access_token={}, sa_tee_id={}, session_cert={}, encrypted_session_sk={}, session_key_expire_at={}, session_key={}",
            resp.refresh_token.len(), resp.access_token.len(), resp.sa_tee_id.len(),
            resp.session_cert.len(), resp.encrypted_session_sk.len(),
            resp.session_key_expire_at.len(), session_private_key.len()
        );
    }

    // Login silently rebinds the local wallet. We don't prompt, but when it
    // rebinds to a *different* wallet identity we leave an audit trail so the
    // switch is traceable. Keyed on `email` (the wallet identity) — NOT
    // `selected_account_id`, which `wallet switch` mutates to a sub-account and
    // would false-positive on every re-login after a switch. Emails are masked
    // in the log (never store raw PII). Best-effort.
    let previous_email = wallet_store::load_wallets()
        .ok()
        .flatten()
        .map(|w| w.email)
        .filter(|e| !e.is_empty());
    if let Some(prev) = previous_email {
        if prev != email {
            crate::audit::log(
                "cli",
                "login_account_switch",
                true,
                std::time::Duration::ZERO,
                Some(vec![
                    format!("previous_email={}", super::common::mask_email(&prev)),
                    format!("new_email={}", super::common::mask_email(email)),
                    format!("login_type={}", resp.login_info.login_type),
                ]),
                None,
            );
        }
    }

    let wallets = WalletsJson {
        email: email.to_string(),
        is_new: resp.is_new,
        project_id: resp.project_id.clone(),
        selected_account_id: resp.account_id.clone(),
        accounts_map: HashMap::new(),
        accounts: vec![],
        // Login method from the backend; empty if not provided.
        login_type: resp.login_info.login_type.clone(),
    };
    wallet_store::save_wallets(&wallets)?;

    // Login resets the account set → drop the insert-only batch balance cache
    // so `balance --all` won't keep summing previous identities' accounts.
    wallet_store::delete_balance_cache()?;

    wallet_store::save_session(&SessionJson {
        sa_tee_id: resp.sa_tee_id.clone(),
        session_cert: resp.session_cert.clone(),
        encrypted_session_sk: resp.encrypted_session_sk.clone(),
        session_key_expire_at: resp.session_key_expire_at.clone(),
    })?;

    keyring_store::store(&[
        ("refresh_token", &resp.refresh_token),
        ("access_token", &resp.access_token),
        ("session_key", session_private_key),
    ])?;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] session.json + keyring store: ok");
    }

    // Use the verify result's account/address set when present (social login);
    // otherwise fall back to the account/list API calls.
    if resp.all_account_address_list.is_empty() {
        fetch_and_save_account_list(client, &resp.access_token, &resp.project_id).await;
    } else {
        apply_all_account_address_list(&resp.all_account_address_list);
    }
    wallet_store::clear_login_cache()?;

    Ok(())
}

/// Fetch account/list and account/address/list, update wallets.json.
/// Non-fatal: failures are logged as warnings.
async fn fetch_and_save_account_list(
    client: &mut WalletApiClient,
    access_token: &str,
    project_id: &str,
) {
    match client.account_list(access_token, project_id).await {
        Ok(account_list) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] account_list count: {}", account_list.len());
            }
            if let Ok(Some(mut wallets)) = wallet_store::load_wallets() {
                wallets.accounts = account_list
                    .iter()
                    .map(|a| wallet_store::AccountInfo {
                        project_id: a.project_id.clone(),
                        account_id: a.account_id.clone(),
                        account_name: a.account_name.clone(),
                        is_default: a.is_default,
                    })
                    .collect();
                let _ = wallet_store::save_wallets(&wallets);
            }

            let account_ids: Vec<String> =
                account_list.iter().map(|a| a.account_id.clone()).collect();

            match client
                .account_address_list(access_token, &account_ids)
                .await
            {
                Ok(address_accounts) => {
                    if cfg!(feature = "debug-log") {
                        eprintln!("[DEBUG] address_accounts count: {}", address_accounts.len());
                    }
                    if let Ok(Some(mut wallets)) = wallet_store::load_wallets() {
                        for item in &address_accounts {
                            wallets.accounts_map.insert(
                                item.account_id.clone(),
                                AccountMapEntry {
                                    address_list: item
                                        .addresses
                                        .iter()
                                        .map(|a| AddressInfo {
                                            account_id: item.account_id.clone(),
                                            address: a.address.clone(),
                                            chain_index: a.chain_index.clone(),
                                            chain_name: a.chain_name.clone(),
                                            address_type: a.address_type.clone(),
                                            chain_path: a.chain_path.clone(),
                                        })
                                        .collect(),
                                },
                            );
                        }
                        let _ = wallet_store::save_wallets(&wallets);
                    }
                }
                Err(e) => {
                    if cfg!(feature = "debug-log") {
                        eprintln!("Warning: failed to fetch address list: {e:#}");
                    }
                }
            }
        }
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("Warning: failed to fetch account list: {e:#}");
            }
        }
    }
}

// ── Add ──────────────────────────────────────────────────────────────

/// onchainos wallet add
pub(super) async fn cmd_add() -> Result<()> {
    let access_token = ensure_tokens_refreshed().await?;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_add: access_token_len={}", access_token.len());
    }

    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    if wallets.project_id.is_empty() {
        bail!(super::common::ERR_NOT_LOGGED_IN);
    }

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_add: project_id={}", wallets.project_id);
    }

    let mut client = WalletApiClient::new()?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] account_create request: access_token_len={}, project_id={}",
            access_token.len(),
            wallets.project_id
        );
    }

    let resp = client
        .account_create(&access_token, &wallets.project_id)
        .await
        .map_err(format_api_error)?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] account_create response: project_id={}, account_id={}, account_name={}, address_list_count={}",
            resp.project_id, resp.account_id, resp.account_name, resp.address_list.len()
        );
        for (i, a) in resp.address_list.iter().enumerate() {
            eprintln!(
                "[DEBUG]   address[{i}]: chain_index={}, chain_name={}, address={}, address_type={}",
                a.chain_index, a.chain_name, a.address, a.address_type
            );
        }
    }

    let mut wallets = wallet_store::load_wallets()?.unwrap_or_default();

    wallets.accounts.push(wallet_store::AccountInfo {
        project_id: resp.project_id.clone(),
        account_id: resp.account_id.clone(),
        account_name: resp.account_name.clone(),
        is_default: false,
    });

    wallets.accounts_map.insert(
        resp.account_id.clone(),
        AccountMapEntry {
            address_list: resp
                .address_list
                .iter()
                .map(|a| AddressInfo {
                    account_id: resp.account_id.clone(),
                    address: a.address.clone(),
                    chain_index: a.chain_index.clone(),
                    chain_name: a.chain_name.clone(),
                    address_type: a.address_type.clone(),
                    chain_path: a.chain_path.clone(),
                })
                .collect(),
        },
    );

    wallet_store::save_wallets(&wallets)?;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] wallets.json updated with new account");
    }

    match client
        .account_list(&access_token, &wallets.project_id)
        .await
    {
        Ok(account_list) => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG] account_list refresh: {} accounts",
                    account_list.len()
                );
            }
            let mut wallets = wallet_store::load_wallets()?.unwrap_or_default();
            wallets.accounts = account_list
                .iter()
                .map(|a| wallet_store::AccountInfo {
                    project_id: a.project_id.clone(),
                    account_id: a.account_id.clone(),
                    account_name: a.account_name.clone(),
                    is_default: a.is_default,
                })
                .collect();
            wallet_store::save_wallets(&wallets)?;
        }
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("Warning: failed to refresh account list: {e:#}");
            }
        }
    }

    super::account::switch_to_account(&resp.account_id)?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] cmd_add: switched to new account_id={}",
            resp.account_id
        );
    }

    output::success(json!({
        "accountId": resp.account_id,
        "accountName": resp.account_name,
        "addressList": resp.address_list.iter().map(|a| json!({
            "chainIndex": a.chain_index,
            "chainName": a.chain_name,
            "address": a.address,
        })).collect::<Vec<_>>(),
    }));
    Ok(())
}

// ── Logout ───────────────────────────────────────────────────────────

/// onchainos wallet logout
pub(super) async fn cmd_logout() -> Result<()> {
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: start");
    }

    keyring_store::clear_all()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: keyring cleared");
    }

    wallet_store::delete_session()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: session.json deleted");
    }

    wallet_store::delete_wallets()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: wallets.json deleted");
    }

    wallet_store::delete_cache()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: cache.json deleted");
    }

    wallet_store::delete_balance_cache()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: balance_cache.json deleted");
    }

    crate::payment_cache::PaymentCache::delete()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: payment_cache.json deleted");
    }

    crate::payment::subscription::cache::SubscriptionCache::delete()?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_logout: subscriptions.json deleted");
    }

    output::success_empty();
    Ok(())
}

// ── Chain update helpers ─────────────────────────────────────────────

/// Overwrite `accounts` and `accounts_map` in wallets.json from the refresh response.
fn apply_all_account_address_list(list: &[crate::wallet_api::RefreshAccountItem]) {
    let Ok(Some(mut wallets)) = wallet_store::load_wallets() else {
        return;
    };

    wallets.accounts = list
        .iter()
        .map(|a| wallet_store::AccountInfo {
            project_id: wallets.project_id.clone(),
            account_id: a.account_id.clone(),
            account_name: a.account_name.clone(),
            is_default: a.is_default,
        })
        .collect();

    wallets.accounts_map.clear();
    for item in list {
        let address_list = item
            .addresses
            .iter()
            .map(|a| wallet_store::AddressInfo {
                account_id: item.account_id.clone(),
                address: a.address.clone(),
                chain_index: a.chain_index.clone(),
                chain_name: a.chain_name.clone(),
                address_type: a.address_type.clone(),
                chain_path: a.chain_path.clone(),
            })
            .collect();
        wallets.accounts_map.insert(
            item.account_id.clone(),
            wallet_store::AccountMapEntry { address_list },
        );
    }

    let _ = wallet_store::save_wallets(&wallets);
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn make_jwt(exp: i64) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"exp":{}}}"#, exp));
        let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("fake_sig");
        format!("{}.{}.{}", header, payload, sig)
    }

    #[test]
    fn token_exp_timestamp_parses_valid_jwt() {
        let jwt = make_jwt(1700000000);
        assert_eq!(token_exp_timestamp(&jwt), Some(1700000000));
    }

    #[test]
    fn token_exp_timestamp_returns_none_for_garbage() {
        assert_eq!(token_exp_timestamp("not.a.jwt"), None);
        assert_eq!(token_exp_timestamp(""), None);
        assert_eq!(token_exp_timestamp("onlyone"), None);
    }

    #[test]
    fn is_token_expired_true_for_past() {
        let past = chrono::Utc::now().timestamp() - 3600;
        assert!(is_token_expired(&make_jwt(past)));
    }

    #[test]
    fn is_token_expired_false_for_future() {
        let future = chrono::Utc::now().timestamp() + 3600;
        assert!(!is_token_expired(&make_jwt(future)));
    }

    #[test]
    fn is_token_expired_true_for_invalid() {
        assert!(is_token_expired("garbage"));
    }

    #[test]
    fn ed25519_sign_hex_basic() {
        use ed25519_dalek::{SigningKey, Verifier, VerifyingKey};

        let seed = [42u8; 32];
        let session_key_b64 = base64::engine::general_purpose::STANDARD.encode(seed);
        let hex_hash = "0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

        let sig_b64 = crate::crypto::ed25519_sign_hex(hex_hash, &session_key_b64).unwrap();
        assert!(!sig_b64.is_empty());

        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(&sig_b64)
            .unwrap();
        let sig = ed25519_dalek::Signature::from_slice(&sig_bytes).unwrap();
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg = hex::decode("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
            .unwrap();
        assert!(verifying_key.verify(&msg, &sig).is_ok());
    }

    #[test]
    fn ed25519_sign_hex_without_0x_prefix() {
        let seed = [7u8; 32];
        let sk_b64 = base64::engine::general_purpose::STANDARD.encode(seed);
        let sig1 = crate::crypto::ed25519_sign_hex("0xaabb", &sk_b64).unwrap();
        let sig2 = crate::crypto::ed25519_sign_hex("aabb", &sk_b64).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn ed25519_sign_hex_empty_returns_empty() {
        let seed = [1u8; 32];
        let sk_b64 = base64::engine::general_purpose::STANDARD.encode(seed);
        let result = crate::crypto::ed25519_sign_hex("", &sk_b64).unwrap();
        assert!(result.is_empty());
        let result2 = crate::crypto::ed25519_sign_hex("0x", &sk_b64).unwrap();
        assert!(result2.is_empty());
    }

    #[test]
    fn ed25519_sign_hex_deterministic() {
        let seed = [99u8; 32];
        let sk_b64 = base64::engine::general_purpose::STANDARD.encode(seed);
        let hash = "0x1234567890abcdef1234567890abcdef";
        let sig1 = crate::crypto::ed25519_sign_hex(hash, &sk_b64).unwrap();
        let sig2 = crate::crypto::ed25519_sign_hex(hash, &sk_b64).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn apply_all_account_address_list_overwrites_accounts_and_map() {
        use crate::wallet_store::{AccountInfo, AccountMapEntry, AddressInfo, WalletsJson};
        use std::collections::HashMap;

        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("apply_chain_update");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).ok();
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);

        // Seed wallets.json with existing state
        let mut initial_map = HashMap::new();
        initial_map.insert(
            "acc-old".to_string(),
            AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "acc-old".to_string(),
                    address: "0xold".to_string(),
                    chain_index: "1".to_string(),
                    chain_name: "eth".to_string(),
                    address_type: "eoa".to_string(),
                    chain_path: "m/44/60".to_string(),
                }],
            },
        );
        let initial = WalletsJson {
            email: "user@test.com".to_string(),
            project_id: "proj-1".to_string(),
            selected_account_id: "acc-old".to_string(),
            accounts: vec![AccountInfo {
                project_id: "proj-1".to_string(),
                account_id: "acc-old".to_string(),
                account_name: "Old Wallet".to_string(),
                is_default: true,
            }],
            accounts_map: initial_map,
            ..Default::default()
        };
        wallet_store::save_wallets(&initial).unwrap();

        // Build the new address list (simulates allAccountAddressList from refresh)
        let new_list = vec![crate::wallet_api::RefreshAccountItem {
            account_id: "acc-1".to_string(),
            account_name: "Wallet 1".to_string(),
            is_default: true,
            addresses: vec![
                crate::wallet_api::VerifyAddressInfo {
                    account_id: "acc-1".to_string(),
                    address: "0xabc".to_string(),
                    chain_index: "4217".to_string(),
                    chain_name: "tempo".to_string(),
                    address_type: "eoa".to_string(),
                    chain_path: "m/44/60/0/0".to_string(),
                },
                crate::wallet_api::VerifyAddressInfo {
                    account_id: "acc-1".to_string(),
                    address: "0xdef".to_string(),
                    chain_index: "1".to_string(),
                    chain_name: "eth".to_string(),
                    address_type: "eoa".to_string(),
                    chain_path: "m/44/60/0/0".to_string(),
                },
            ],
        }];

        apply_all_account_address_list(&new_list);

        let saved = wallet_store::load_wallets().unwrap().unwrap();

        // preserved fields unchanged
        assert_eq!(saved.email, "user@test.com");
        assert_eq!(saved.project_id, "proj-1");
        assert_eq!(saved.selected_account_id, "acc-old");

        // accounts overwritten
        assert_eq!(saved.accounts.len(), 1);
        assert_eq!(saved.accounts[0].account_id, "acc-1");
        assert_eq!(saved.accounts[0].account_name, "Wallet 1");
        assert_eq!(saved.accounts[0].project_id, "proj-1");
        assert!(saved.accounts[0].is_default);

        // accounts_map overwritten — old entry gone, new entries present
        assert!(!saved.accounts_map.contains_key("acc-old"));
        let entry = saved.accounts_map.get("acc-1").unwrap();
        assert_eq!(entry.address_list.len(), 2);
        assert_eq!(entry.address_list[0].chain_index, "4217");
        assert_eq!(entry.address_list[0].chain_name, "tempo");
        assert_eq!(entry.address_list[1].chain_index, "1");

        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn hpke_decrypt_session_sk_known_vector() {
        let encrypted_b64 =
            "D77ghrSZD4FhOjt8h6irNQS9OBxaq7Ry6LobgKyBuV4rPLTulIoZSsEt5pZYptfSFo8AX+XwIYw8RRJXPNRhRSJDno4F0CLdPNFeat16/90=";
        let priv_key_hex = "7e0e4cb4ce949dcee0ca600713d37a0ecec71e3f20b7a834680ba2306e06c671";
        let priv_key_bytes = hex::decode(priv_key_hex).unwrap();
        let session_key_b64 = base64::engine::general_purpose::STANDARD.encode(&priv_key_bytes);
        let expected_hex = "d84197bf9417d10a74cfba304f487868bb41708623e1d61823df44c734cda122";
        let expected = hex::decode(expected_hex).unwrap();

        let seed = crate::crypto::hpke_decrypt_session_sk(encrypted_b64, &session_key_b64).unwrap();
        assert_eq!(seed.len(), 32);
        assert_eq!(seed.as_slice(), expected.as_slice());
    }

    #[test]
    fn hpke_decrypt_then_sign_verify_roundtrip() {
        use ed25519_dalek::{Signature, Verifier};

        let encrypted_b64 =
            "D77ghrSZD4FhOjt8h6irNQS9OBxaq7Ry6LobgKyBuV4rPLTulIoZSsEt5pZYptfSFo8AX+XwIYw8RRJXPNRhRSJDno4F0CLdPNFeat16/90=";
        let priv_key_hex = "7e0e4cb4ce949dcee0ca600713d37a0ecec71e3f20b7a834680ba2306e06c671";
        let priv_key_bytes = hex::decode(priv_key_hex).unwrap();
        let session_key_b64 = base64::engine::general_purpose::STANDARD.encode(&priv_key_bytes);

        let seed = crate::crypto::hpke_decrypt_session_sk(encrypted_b64, &session_key_b64).unwrap();
        let seed_b64 = base64::engine::general_purpose::STANDARD.encode(seed);

        let hex_hash = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let sig_b64 = crate::crypto::ed25519_sign_hex(hex_hash, &seed_b64).unwrap();

        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(&sig_b64)
            .expect("valid base64 signature");
        let signature = Signature::from_bytes(&sig_bytes.try_into().expect("64 bytes"));
        let msg_bytes = hex::decode(hex_hash.strip_prefix("0x").unwrap()).unwrap();

        assert!(verifying_key.verify(&msg_bytes, &signature).is_ok());
    }

    #[test]
    fn hpke_decrypt_session_sk_too_short() {
        let short_b64 = base64::engine::general_purpose::STANDARD.encode([0u8; 30]);
        let key_b64 = base64::engine::general_purpose::STANDARD.encode([1u8; 32]);
        assert!(crate::crypto::hpke_decrypt_session_sk(&short_b64, &key_b64).is_err());
    }

    #[test]
    fn build_login_url_percent_encodes_temp_pub_key() {
        // base64 chars + / = must be percent-encoded so the page gets them intact.
        let url = build_login_url("https://web3pre.okex.org", "sess-123", "AB+/=cd");
        assert!(url.starts_with("https://web3pre.okex.org/account/sociallogin?"));
        assert!(url.contains("authSessionId=sess-123"));
        assert!(url.contains("tempPubKey=AB%2B%2F%3Dcd"));
        assert!(url.contains("clientType=agent-cli"));
    }

    #[test]
    fn build_login_url_trims_base_trailing_slash() {
        let url = build_login_url("https://x.com/", "s", "k");
        assert!(url.starts_with("https://x.com/account/sociallogin?"));
        assert!(!url.contains(".com//account"));
    }

    #[test]
    fn is_browsable_url_accepts_http_and_https_only() {
        assert!(is_browsable_url(
            "https://web3pre.okex.org/account/sociallogin?x=1"
        ));
        assert!(is_browsable_url("http://localhost:8080/login"));
        // Non-http(s) schemes and unparseable inputs are rejected so the OS
        // opener never dispatches them to a local handler.
        assert!(!is_browsable_url("file:///etc/passwd"));
        assert!(!is_browsable_url("javascript:alert(1)"));
        assert!(!is_browsable_url("ftp://example.com"));
        assert!(!is_browsable_url("not a url"));
        assert!(!is_browsable_url(""));
    }

    #[test]
    fn classify_poll_ready_only_with_non_empty_token() {
        let with_token: Result<serde_json::Value> =
            Ok(serde_json::json!({ "accessToken": "tok" }));
        let no_token: Result<serde_json::Value> = Ok(serde_json::json!({ "foo": 1 }));
        let empty_token: Result<serde_json::Value> =
            Ok(serde_json::json!({ "accessToken": "" }));
        assert!(matches!(classify_poll(&with_token), PollOutcome::Ready));
        assert!(matches!(classify_poll(&no_token), PollOutcome::Pending));
        assert!(matches!(classify_poll(&empty_token), PollOutcome::Pending));
    }

    #[test]
    fn classify_poll_pending_for_10018_backend_code() {
        let not_ready: Result<serde_json::Value> = Err(ApiCodeError {
            code: "10018".to_string(),
            msg: "not ready".to_string(),
            http_status: 200,
        }
        .into());
        assert!(matches!(classify_poll(&not_ready), PollOutcome::Pending));
    }

    #[test]
    fn classify_poll_transient_for_no_code_transport_error() {
        // A no-code transport/parse error keeps polling but counts toward the
        // consecutive-failure short-circuit (distinct from backend 10018).
        let transport: Result<serde_json::Value> = Err(anyhow::anyhow!("connection refused"));
        assert!(matches!(classify_poll(&transport), PollOutcome::Transient));
    }

    #[test]
    fn classify_poll_terminal_for_other_backend_codes() {
        let err: Result<serde_json::Value> = Err(ApiCodeError {
            code: "-1".to_string(),
            msg: "failed".to_string(),
            http_status: 200,
        }
        .into());
        assert!(matches!(classify_poll(&err), PollOutcome::Terminal));
    }

    #[test]
    fn social_login_timeout_defaults_when_unset_or_invalid() {
        assert_eq!(resolve_social_login_timeout_secs(None), 300);
        assert_eq!(resolve_social_login_timeout_secs(Some("")), 300);
        assert_eq!(resolve_social_login_timeout_secs(Some("abc")), 300);
        assert_eq!(resolve_social_login_timeout_secs(Some("-5")), 300);
    }

    #[test]
    fn social_login_timeout_honors_valid_override() {
        assert_eq!(resolve_social_login_timeout_secs(Some("10")), 10);
        assert_eq!(resolve_social_login_timeout_secs(Some("60")), 60);
    }

    #[test]
    fn social_login_timeout_below_floor_falls_back_to_default() {
        // Sub-floor values are rejected and fall back to the default (not clamped).
        assert_eq!(resolve_social_login_timeout_secs(Some("9")), 300);
        assert_eq!(resolve_social_login_timeout_secs(Some("0")), 300);
    }
}
