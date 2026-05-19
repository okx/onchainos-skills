use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Result};
use base64::Engine;
use serde_json::json;

use crate::audit;
use crate::keyring_store;
use crate::output;
use crate::wallet_api::{ApiCodeError, WalletApiClient};
use crate::wallet_store::{
    self, AccountMapEntry, AddressInfo, LoginCache, SessionJson, WalletsJson,
};

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
///   1. session_key expired           → try AK re-login, else anonymous fallback
///   2. no tokens in keychain         → try AK re-login, else anonymous fallback
///   3. refresh_token expired         → prompt user, try AK re-login, else anonymous fallback
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
                "[DEBUG][ensure_tokens_refreshed] session key expired → relogin_or_anonymous"
            );
        }
        return relogin_or_anonymous().await;
    }

    // ── Step 2: read tokens from keychain ────────────────────────────
    let blob = keyring_store::read_blob()?;

    let refresh_token = match blob.get("refresh_token").filter(|t| !t.is_empty()) {
        Some(t) => t.clone(),
        _ => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] no refresh_token → relogin_or_anonymous"
                );
            }
            return relogin_or_anonymous().await;
        }
    };

    let access_token = match blob.get("access_token").filter(|t| !t.is_empty()) {
        Some(t) => t.clone(),
        _ => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][ensure_tokens_refreshed] no access_token → relogin_or_anonymous"
                );
            }
            return relogin_or_anonymous().await;
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
                "[DEBUG][ensure_tokens_refreshed] refresh_token expired → relogin_or_anonymous"
            );
        }
        return relogin_or_anonymous().await;
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

/// When the session or refresh token is expired: attempt AK re-login using env vars.
///
/// - AK env vars present → auto re-login → return new JWT
/// - AK env vars absent → bail with a clear message (wallet APIs require a valid JWT;
///   returning "" would only cause an opaque 401 downstream)
async fn relogin_or_anonymous() -> Result<String> {
    let ak = std::env::var("OKX_API_KEY").or_else(|_| std::env::var("OKX_ACCESS_KEY"));
    let sk = std::env::var("OKX_SECRET_KEY");
    let pp = std::env::var("OKX_PASSPHRASE");

    match (ak, sk, pp) {
        (Ok(api_key), Ok(secret_key), Ok(passphrase)) => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG][relogin_or_anonymous] AK env vars found, attempting re-login");
            }
            cmd_login_ak(&api_key, &secret_key, &passphrase, None).await?;
            let blob = keyring_store::read_blob()?;
            let access_token = blob.get("access_token").cloned().unwrap_or_default();
            if access_token.is_empty() {
                bail!("AK re-login succeeded but access_token was not stored — please retry");
            }
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][relogin_or_anonymous] AK re-login successful, access_token_len={}",
                    access_token.len()
                );
            }
            Ok(access_token)
        }
        _ => {
            if cfg!(feature = "debug-log") {
                eprintln!(
                    "[DEBUG][relogin_or_anonymous] no AK env vars, session cannot be recovered"
                );
            }
            bail!("session expired, please login again: onchainos wallet login")
        }
    }
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

/// Check if token expires within `secs` seconds.
fn token_expires_within_secs(token: &str, secs: i64) -> bool {
    token_exp_timestamp(token)
        .map(|exp| {
            let now = chrono::Utc::now().timestamp();
            exp - now <= secs
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
        Ok(api_err) => anyhow::anyhow!("{}", api_err.msg),
        Err(e) => e,
    }
}

// ── Login ────────────────────────────────────────────────────────────

/// Derive the planned login mode for THIS invocation of `cmd_login`.
///
/// Rules (spec §1.3, M3):
/// - Email clap arg present (non-empty) → `Some("email")`
/// - Email arg absent AND all three `OKX_API_KEY` + `OKX_SECRET_KEY` +
///   `OKX_PASSPHRASE` env vars are non-empty → `Some("ak")`
/// - Otherwise → `None` (caller bails before mode-diff check matters).
fn derive_current_mode(email_arg: Option<&str>) -> Option<&'static str> {
    if let Some(e) = email_arg {
        if !e.is_empty() {
            return Some("email");
        }
    }
    let ak_ok = std::env::var("OKX_API_KEY")
        .or_else(|_| std::env::var("OKX_ACCESS_KEY"))
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let sk_ok = std::env::var("OKX_SECRET_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let pp_ok = std::env::var("OKX_PASSPHRASE")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if ak_ok && sk_ok && pp_ok {
        Some("ak")
    } else {
        None
    }
}

/// CLI defensive login-diff pre-check.
///
/// Detects three scenarios that would silently overwrite the locally bound
/// account if not confirmed:
///
/// 1. **Mode switch** — `last_login_mode != current_mode` (email ↔ ak). Scene
///    line names the two modes.
/// 2. **Same-mode different-account (email)** — both sides are `email` and the
///    email address differs from the persisted one (PII-masked in the scene
///    line).
/// 3. **Same-mode different-account (ak)** — both sides are `ak` and the
///    incoming `api_key` differs from the one persisted in `session.json`
///    (PII-masked in the scene line). This subsumes the older standalone
///    AK-switch `bail!()` guard so the AK→AK case shares the unified exit-2
///    confirming envelope + audit events with the other two scenarios.
///
/// `current_email` is the email arg of `wallet login` (only populated on the
/// email path; `None` for the AK env-driven path).
///
/// `current_api_key` is the API key from `$OKX_API_KEY` (or alias) for the
/// AK env-driven path; `None` for the email path. Used only to detect the
/// AK→AK same-mode account-switch case.
///
/// Audit table (per spec §5.2 unified write rule, extended with `switch_kind`):
///
/// | `would_fire` | `force` | Audit writes (in order)                                     | Control flow                  |
/// |--------------|---------|-------------------------------------------------------------|-------------------------------|
/// | false        | —       | none                                                        | `Ok(())` — proceed normally   |
/// | true         | false   | `login_mode_prompt_shown`                                   | `Err(CliConfirming)` (exit 2) |
/// | true         | true    | `login_mode_prompt_shown` THEN `login_mode_prompt_user_choice` | `Ok(())` — proceed with login |
///
/// `switch_kind=mode|account` is included in audit args so off-box telemetry
/// can distinguish the two scenarios. `user_choice=no` is never written by the
/// CLI (accepted gap: the CLI cannot observe a No answer).
fn check_login_mode_diff(
    current_mode: &'static str,
    current_email: Option<&str>,
    current_api_key: Option<&str>,
    force: bool,
) -> Result<()> {
    let t = if cfg!(feature = "debug-log") {
        Some(std::time::Instant::now())
    } else {
        None
    };

    // Load wallets first — both scenarios need access to the persisted email.
    let wallets = match wallet_store::load_wallets() {
        Ok(Some(w)) => w,
        _ => {
            if let Some(start) = t {
                eprintln!(
                    "[DEBUG] mode-diff check (no wallets): {:?}",
                    start.elapsed()
                );
            }
            return Ok(());
        }
    };

    let last_mode = match super::common::derive_last_login_mode(&wallets.email, wallets.is_ak) {
        Some(m) => m,
        None => {
            if let Some(start) = t {
                eprintln!(
                    "[DEBUG] mode-diff check (no last mode): {:?}",
                    start.elapsed()
                );
            }
            return Ok(());
        }
    };

    // Decide scenario and the scene-specific line. The four scenarios are
    // dispatched by the (last_mode, current_mode) pair. Each arm builds the
    // user-facing scene line and tags the audit event via `switch_kind`.
    //
    // Display rules:
    //   - User-facing labels are `Email` and `API Key` (Title Case / spaced);
    //     internal mode tokens stay lowercase in audit args and comparisons.
    //   - On mode switch, the `Email` side carries the masked email of the
    //     *email* account in parentheses, regardless of direction. The
    //     `API Key` side never displays the key itself (api_keys are more
    //     sensitive than emails and the user already knows the env value).
    //   - On same-mode email switch, both old and new emails are masked
    //     using `mask_email`.
    //   - On same-mode AK switch, no key (masked or otherwise) appears in
    //     the message; a fixed line tells the user their env api_key has
    //     diverged from the persisted one.
    let (scene_line, switch_kind) = match (last_mode, current_mode) {
        ("email", "ak") => {
            // Mode switch: persisted=email, this login=ak. The Email side
            // is OLD — show its masked form in parens.
            (
                format!(
                    "Login method: Email ({}) → API Key",
                    super::common::mask_email(&wallets.email),
                ),
                "mode",
            )
        }
        ("ak", "email") => {
            // Mode switch: persisted=ak, this login=email. The Email side
            // is NEW — show the new email's masked form in parens.
            let new_email = current_email.unwrap_or("");
            (
                format!(
                    "Login method: API Key → Email ({})",
                    super::common::mask_email(new_email),
                ),
                "mode",
            )
        }
        ("email", "email") => {
            // Same-mode account switch (email). Compare addresses,
            // case-insensitive + whitespace-trimmed. Empty new email
            // shouldn't reach here in practice (cmd_login rejects empty
            // email arg before calling us), but guard anyway.
            let new_raw = current_email.unwrap_or("");
            let new_norm = new_raw.trim().to_lowercase();
            let old_norm = wallets.email.trim().to_lowercase();
            if new_norm.is_empty() || old_norm.is_empty() || new_norm == old_norm {
                if let Some(start) = t {
                    eprintln!(
                        "[DEBUG] mode-diff check (no-op, same email): {:?}",
                        start.elapsed()
                    );
                }
                return Ok(());
            }
            (
                format!(
                    "Account: {} → {}",
                    super::common::mask_email(&wallets.email),
                    super::common::mask_email(new_raw),
                ),
                "account",
            )
        }
        ("ak", "ak") => {
            // Same-mode account switch (ak). Compare api_keys; the persisted
            // key lives in session.json (not wallets.json). If session is
            // missing or empty, the comparison is undefined — fall through
            // to no-op (matches the previous AK-switch guard's
            // `if let Ok(Some(..))` tolerance). Same-value comparison is
            // also a no-op.
            //
            // When the gate fires the scene line is a fixed PII-clean
            // sentence — no key (masked or otherwise) appears in the
            // user-facing message. The user already knows the value of
            // their `OKX_API_KEY` env var; the CLI's job is to flag that
            // it diverges from the persisted one.
            let new_key = current_api_key.unwrap_or("").trim();
            let old_key = match wallet_store::load_session() {
                Ok(Some(session)) => session.api_key,
                _ => String::new(),
            };
            if new_key.is_empty() || old_key.is_empty() || new_key == old_key {
                if let Some(start) = t {
                    eprintln!(
                        "[DEBUG] mode-diff check (no-op, same ak / missing session): {:?}",
                        start.elapsed()
                    );
                }
                return Ok(());
            }
            (
                "The API Key in your env has changed".to_string(),
                "account",
            )
        }
        _ => {
            // Unknown (mode, mode) combination — should never occur given
            // `derive_last_login_mode` and `derive_current_mode` only
            // return "email" / "ak". Defensive: no-op.
            if let Some(start) = t {
                eprintln!(
                    "[DEBUG] mode-diff check (no-op, unknown mode pair): {:?}",
                    start.elapsed()
                );
            }
            return Ok(());
        }
    };

    audit::log(
        "cli",
        "login_mode_prompt_shown",
        true,
        Duration::ZERO,
        Some(vec![
            format!("current_mode={current_mode}"),
            format!("last_login_mode={last_mode}"),
            format!("switch_kind={switch_kind}"),
        ]),
        None,
    );

    if force {
        // Skill-first Yes path: paired prompt_shown + user_choice=yes events
        // back-to-back so the audit trail is complete.
        audit::log(
            "cli",
            "login_mode_prompt_user_choice",
            true,
            Duration::ZERO,
            Some(vec![
                format!("current_mode={current_mode}"),
                format!("last_login_mode={last_mode}"),
                format!("switch_kind={switch_kind}"),
                "user_choice=yes".to_string(),
            ]),
            None,
        );

        if let Some(start) = t {
            eprintln!("[DEBUG] mode-diff check (force): {:?}", start.elapsed());
        }
        return Ok(());
    }

    // CLI defensive path: emit confirming + exit 2. The first line is the
    // skill discriminator (substring `not the account you used last time` is
    // matched verbatim — never translated). Scene line is filled per scenario
    // above; reassurance + Yes/No prompt are shared.
    let message = format!(
        "⚠️ This is not the account you used last time.\n\
         {scene_line}\n\
         Your previous account's assets are untouched and still accessible — log in with the original account to view them.\n\
         Continue? [Yes / No]"
    );

    let confirming = output::CliConfirming {
        message,
        next: "If the user confirms, re-run the same command with --force flag appended to proceed.".to_string(),
    };

    if let Some(start) = t {
        eprintln!(
            "[DEBUG] mode-diff check (gate fires): {:?}",
            start.elapsed()
        );
    }
    Err(confirming.into())
}

/// Validate a user-supplied locale value against the OTP-email whitelist.
///
/// Returns `(validated_locale, did_fallback)`:
/// - If the input matches the whitelist (case-sensitive) -> pass through, `false`.
/// - Otherwise -> fall back to `"en-US"`, `true`.
///
/// Callers should emit a stderr warning when `did_fallback == true`.
pub(crate) fn validate_locale(locale: &str) -> (&'static str, bool) {
    match locale {
        "en-US" => ("en-US", false),
        "zh-CN" => ("zh-CN", false),
        _ => ("en-US", true),
    }
}

/// onchainos wallet login [email] [--locale <locale>] [--force]
pub(super) async fn cmd_login(
    email: Option<&str>,
    locale: Option<&str>,
    force: bool,
) -> Result<()> {
    if let Some(email) = email {
        if email.is_empty() {
            bail!("email is required");
        }

        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG] cmd_login: email={email}, locale={locale:?}");
        }

        // Login-diff pre-check: fires before any auth API call when (a) the
        // previous session used `ak` and this login uses `email` (mode
        // switch), or (b) this email differs from the persisted one (same-
        // mode different-account, prevents silent OTP redirect to a new
        // address). `current_api_key` is None on the email path. Runs
        // before locale validation so an exit-2 confirming response is
        // returned without any side-effect (including the stderr fallback
        // warning).
        check_login_mode_diff("email", Some(email), None, force)?;

        // Validate locale before calling auth_init.
        let validated_locale: Option<&str> = match locale {
            Some(loc) => {
                let (validated, did_fallback) = validate_locale(loc);
                if did_fallback {
                    eprintln!(
                        "locale '{}' not in supported list (en-US, zh-CN), falling back to en-US",
                        loc,
                    );
                }
                Some(validated)
            }
            None => None,
        };

        let mut client = WalletApiClient::new()?;
        let resp = client
            .auth_init(email, validated_locale)
            .await
            .map_err(format_api_error)?;

        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG] auth_init response: flow_id={}", resp.flow_id);
        }

        let mut cache = wallet_store::load_cache()?;
        cache.login = Some(LoginCache {
            email: email.to_string(),
            flow_id: resp.flow_id.clone(),
        });
        wallet_store::save_cache(&cache)?;

        output::success_empty();
        Ok(())
    } else {
        let ak = std::env::var("OKX_API_KEY").or_else(|_| std::env::var("OKX_ACCESS_KEY"));
        let sk = std::env::var("OKX_SECRET_KEY");
        let pp = std::env::var("OKX_PASSPHRASE");

        match (ak, sk, pp) {
            (Ok(api_key), Ok(secret_key), Ok(passphrase)) => {
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG] cmd_login: AK flow, api_key_len={}, secret_key_len={}, passphrase_len={}, locale={locale:?}",
                        api_key.len(), secret_key.len(), passphrase.len(),
                    );
                }

                // Login-diff pre-check: handles all three scenarios in a
                // single gate — mode switch (email → ak), same-mode email
                // account switch (N/A on this branch), and same-mode AK
                // account switch (different api_key vs persisted session).
                // The old standalone AK-switch `bail!()` guard has been
                // folded into this call; on AK→AK with a different key the
                // function returns `Err(CliConfirming)` with exit 2 and
                // emits `login_mode_prompt_shown` (switch_kind=account).
                check_login_mode_diff("ak", None, Some(&api_key), force)?;

                cmd_login_ak(&api_key, &secret_key, &passphrase, locale).await
            }
            _ => {
                bail!("please set OKX_API_KEY, OKX_SECRET_KEY, OKX_PASSPHRASE env vars for API Key login");
            }
        }
    }
}

/// AK login: auth/ak/init → auth/ak/verify in one shot (no OTP needed).
async fn cmd_login_ak(
    api_key: &str,
    secret_key: &str,
    passphrase: &str,
    locale: Option<&str>,
) -> Result<()> {
    let mut client = WalletApiClient::new()?;

    let init_resp = client
        .ak_auth_init(api_key)
        .await
        .map_err(format_api_error)?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] ak_auth_init response: nonce={}, iss={}",
            init_resp.nonce, init_resp.iss
        );
    }

    let (session_private_key, temp_pub_key) = crate::crypto::generate_x25519_session_keypair();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] X25519 keypair: temp_pub_key={}, session_private_key_len={}",
            temp_pub_key,
            session_private_key.len()
        );
    }

    let locale_val = locale.unwrap_or("en-US");
    let timestamp = chrono::Utc::now().timestamp_millis() as u64;
    let method = "GET";
    let sign_path = "/web3/ak/agentic/login";
    let params = format!(
        "?locale={}&nonce={}&iss={}",
        locale_val, init_resp.nonce, init_resp.iss
    );
    let sign = crate::crypto::ak_sign(timestamp, method, sign_path, &params, secret_key);
    let timestamp_str = timestamp.to_string();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] ak_auth_verify request: api_key_len={}, passphrase_len={}, timestamp={}, method={}, sign_path={}, params={}, sign_len={}",
            api_key.len(), passphrase.len(), timestamp_str, method, sign_path, params, sign.len()
        );
    }

    let resp = client
        .ak_auth_verify(
            &temp_pub_key,
            api_key,
            passphrase,
            &timestamp_str,
            &sign,
            locale_val,
        )
        .await
        .map_err(format_api_error)?;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] ak_auth_verify response: ok, proceeding to save_verify_result");
    }

    save_verify_result(&mut client, &resp, &session_private_key, "", api_key).await
}

/// onchainos wallet verify <otp>
pub(super) async fn cmd_verify(otp: &str) -> Result<()> {
    if otp.is_empty() {
        bail!("otp is required");
    }

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_verify: otp_len={}", otp.len());
    }

    let cache = wallet_store::load_cache()?;
    let login = cache
        .login
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let email = &login.email;
    let flow_id = &login.flow_id;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_verify: email={email}, flow_id={flow_id}");
    }

    let (session_private_key, temp_pub_key) = crate::crypto::generate_x25519_session_keypair();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] X25519 keypair: temp_pub_key={}, session_private_key_len={}",
            temp_pub_key,
            session_private_key.len()
        );
    }

    let mut client = WalletApiClient::new()?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] auth_verify request: email={email}, flow_id={flow_id}, otp_len={}, temp_pub_key={}",
            otp.len(), temp_pub_key
        );
    }

    let resp = client
        .auth_verify(email, flow_id, otp, &temp_pub_key)
        .await
        .map_err(format_api_error)?;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] auth_verify response: ok, proceeding to save_verify_result");
    }

    save_verify_result(&mut client, &resp, &session_private_key, email, "").await
}

/// Common post-verify logic: persist credentials, fetch accounts, output result.
async fn save_verify_result(
    client: &mut WalletApiClient,
    resp: &crate::wallet_api::VerifyResponse,
    session_private_key: &str,
    email: &str,
    api_key: &str,
) -> Result<()> {
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] save_verify_result: email={email}, is_new={}, project_id={}, account_id={}",
            resp.is_new, resp.project_id, resp.account_id
        );
        eprintln!(
            "[DEBUG] keyring data lengths: refresh_token={}, access_token={}, tee_id={}, session_cert={}, encrypted_session_sk={}, session_key_expire_at={}, session_key={}",
            resp.refresh_token.len(), resp.access_token.len(), resp.tee_id.len(),
            resp.session_cert.len(), resp.encrypted_session_sk.len(),
            resp.session_key_expire_at.len(), session_private_key.len()
        );
    }

    let wallets = WalletsJson {
        email: email.to_string(),
        is_new: resp.is_new,
        project_id: resp.project_id.clone(),
        selected_account_id: resp.account_id.clone(),
        accounts_map: HashMap::new(),
        accounts: vec![],
        is_ak: !api_key.is_empty(),
    };
    wallet_store::save_wallets(&wallets)?;

    wallet_store::save_session(&SessionJson {
        tee_id: resp.tee_id.clone(),
        session_cert: resp.session_cert.clone(),
        encrypted_session_sk: resp.encrypted_session_sk.clone(),
        session_key_expire_at: resp.session_key_expire_at.clone(),
        api_key: api_key.to_string(),
    })?;

    keyring_store::store(&[
        ("refresh_token", &resp.refresh_token),
        ("access_token", &resp.access_token),
        ("session_key", session_private_key),
    ])?;

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] session.json + keyring store: ok");
    }

    fetch_and_save_account_list(client, &resp.access_token, &resp.project_id).await;
    wallet_store::clear_login_cache()?;

    let account_name = wallet_store::load_wallets()?
        .and_then(|w| {
            w.accounts
                .iter()
                .find(|a| a.account_id == resp.account_id)
                .map(|a| a.account_name.clone())
        })
        .unwrap_or_default();

    output::success(json!({
        "accountId": resp.account_id,
        "accountName": account_name,
        "isNew": resp.is_new,
    }));
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
    fn token_expires_within_secs_true_when_close() {
        let exp = chrono::Utc::now().timestamp() + 30;
        assert!(token_expires_within_secs(&make_jwt(exp), 60));
    }

    #[test]
    fn token_expires_within_secs_false_when_far() {
        let exp = chrono::Utc::now().timestamp() + 3600;
        assert!(!token_expires_within_secs(&make_jwt(exp), 60));
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
            is_ak: true,
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
        assert!(saved.is_ak);

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
        let short_b64 = base64::engine::general_purpose::STANDARD.encode(&[0u8; 30]);
        let key_b64 = base64::engine::general_purpose::STANDARD.encode(&[1u8; 32]);
        assert!(crate::crypto::hpke_decrypt_session_sk(&short_b64, &key_b64).is_err());
    }

    // ── cmd_login mode-diff tests (T5) ───────────────────────────────

    /// Build a fresh sandbox dir under `target/test_tmp/<name>`, set
    /// `ONCHAINOS_HOME` to it, and remove all OKX_* AK env vars so each test
    /// starts from a clean slate. The caller MUST hold `TEST_ENV_MUTEX`.
    fn cmd_login_mode_diff_sandbox(name: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).ok();
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &dir);
        std::env::remove_var("OKX_API_KEY");
        std::env::remove_var("OKX_ACCESS_KEY");
        std::env::remove_var("OKX_SECRET_KEY");
        std::env::remove_var("OKX_PASSPHRASE");
        dir
    }

    fn cmd_login_mode_diff_cleanup() {
        std::env::remove_var("ONCHAINOS_HOME");
        std::env::remove_var("OKX_API_KEY");
        std::env::remove_var("OKX_ACCESS_KEY");
        std::env::remove_var("OKX_SECRET_KEY");
        std::env::remove_var("OKX_PASSPHRASE");
    }

    /// Read `audit.jsonl` from the sandbox dir and return only the entry
    /// lines whose `command` starts with `login_mode_prompt_` (skipping the
    /// device-header line + any unrelated entries).
    fn cmd_login_mode_diff_audit_lines(dir: &std::path::Path) -> Vec<serde_json::Value> {
        let path = dir.join("audit.jsonl");
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        content
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| {
                v.get("command")
                    .and_then(|c| c.as_str())
                    .map(|c| c.starts_with("login_mode_prompt_"))
                    .unwrap_or(false)
            })
            .collect()
    }

    // ── derive_current_mode ───────────────────────────────────────────

    #[test]
    fn derive_current_mode_email_arg_returns_email() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cmd_login_mode_diff_sandbox("cmd_login_mode_diff_derive_email_arg");
        assert_eq!(derive_current_mode(Some("user@example.com")), Some("email"));
        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn derive_current_mode_email_empty_string_falls_through() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cmd_login_mode_diff_sandbox("cmd_login_mode_diff_derive_empty_email");
        // Empty email string is treated like absent → falls through to AK env,
        // which has been cleared, so result is None.
        assert_eq!(derive_current_mode(Some("")), None);
        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn derive_current_mode_all_three_ak_envs_returns_ak() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cmd_login_mode_diff_sandbox("cmd_login_mode_diff_derive_ak");
        std::env::set_var("OKX_API_KEY", "ak-test");
        std::env::set_var("OKX_SECRET_KEY", "sk-test");
        std::env::set_var("OKX_PASSPHRASE", "pp-test");
        assert_eq!(derive_current_mode(None), Some("ak"));
        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn derive_current_mode_partial_ak_envs_returns_none() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cmd_login_mode_diff_sandbox("cmd_login_mode_diff_derive_partial_ak");
        std::env::set_var("OKX_API_KEY", "ak-test");
        // SECRET_KEY and PASSPHRASE missing → not "ak"
        assert_eq!(derive_current_mode(None), None);
        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn derive_current_mode_email_arg_wins_over_ak_envs() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cmd_login_mode_diff_sandbox("cmd_login_mode_diff_derive_email_beats_ak");
        std::env::set_var("OKX_API_KEY", "ak-test");
        std::env::set_var("OKX_SECRET_KEY", "sk-test");
        std::env::set_var("OKX_PASSPHRASE", "pp-test");
        assert_eq!(derive_current_mode(Some("u@e.com")), Some("email"));
        cmd_login_mode_diff_cleanup();
    }

    // ── check_login_mode_diff truth table (spec §5.2) ────────────────

    #[test]
    fn cmd_login_mode_diff_fires_when_email_to_ak_no_force() {
        // Scenario (a): wallets.json has lastLoginMode=email (is_ak=false,
        // email non-empty); current_mode=ak; force=false → exit 2 +
        // CliConfirming with discriminator; ONE prompt_shown audit, NO
        // user_choice audit.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_email_to_ak");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let res = check_login_mode_diff("ak", None, None, false);
        let err = res.expect_err("mode-diff must fire");
        let confirming = err
            .downcast_ref::<output::CliConfirming>()
            .expect("must be CliConfirming");
        assert!(confirming
            .message
            .contains("not the account you used last time"));
        // Scene line for Email→AK mode switch: Email side carries the
        // masked persisted email in parens; API Key side does not show
        // the key.
        assert!(confirming
            .message
            .contains("Login method: Email (p***v@example.com) → API Key"));
        assert!(confirming
            .message
            .starts_with("⚠️ This is not the account you used last time"));
        assert_eq!(
            confirming.next,
            "If the user confirms, re-run the same command with --force flag appended to proceed."
        );

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert_eq!(entries.len(), 1, "exactly one prompt_shown entry");
        assert_eq!(entries[0]["command"], "login_mode_prompt_shown");
        let args = entries[0]["args"].as_array().expect("args present");
        let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
        // Audit args keep the internal lowercase tokens; display form is
        // user-facing only.
        assert!(arg_strs.contains(&"current_mode=ak"));
        assert!(arg_strs.contains(&"last_login_mode=email"));
        assert!(arg_strs.contains(&"switch_kind=mode"));
        assert!(!arg_strs.iter().any(|s| s.contains("user_choice")));

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_fires_when_ak_to_email_no_force() {
        // Scenario (b): wallets.json has lastLoginMode=ak (is_ak=true);
        // current_mode=email; force=false → exit 2 + audit pair reversed.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_ak_to_email");

        let prev = WalletsJson {
            email: String::new(),
            is_ak: true,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let res = check_login_mode_diff("email", Some("new@example.com"), None, false);
        let err = res.expect_err("mode-diff must fire");
        let confirming = err
            .downcast_ref::<output::CliConfirming>()
            .expect("must be CliConfirming");
        assert!(confirming
            .message
            .contains("not the account you used last time"));
        // Mirror direction: ak → email. The Email side is NEW — show the
        // new email's masked form in parens.
        assert!(confirming
            .message
            .contains("Login method: API Key → Email (n***w@example.com)"));

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["command"], "login_mode_prompt_shown");
        let args = entries[0]["args"].as_array().expect("args present");
        let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
        assert!(arg_strs.contains(&"current_mode=email"));
        assert!(arg_strs.contains(&"last_login_mode=ak"));
        assert!(arg_strs.contains(&"switch_kind=mode"));

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_no_prompt_when_no_prior_wallets() {
        // Scenario (c): no wallets.json on disk → derive returns None → no
        // prompt; no audit entries.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_no_prior");

        let res = check_login_mode_diff("email", Some("anyone@example.com"), None, false);
        assert!(res.is_ok(), "no wallets.json → proceed normally");

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert!(entries.is_empty(), "no audit entries written");

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_no_prompt_when_same_email() {
        // Scenario (d): wallets.json has lastLoginMode=email with email
        // "prev@example.com"; current_mode=email AND current_email matches
        // (case-insensitive after trim) → no prompt; no audit entries.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_same_email");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        // Same email (varied casing + surrounding whitespace) must still be
        // treated as the same account.
        let res = check_login_mode_diff("email", Some("  PREV@Example.com  "), None, false);
        assert!(res.is_ok());

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert!(entries.is_empty());

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_fires_when_email_to_different_email_no_force() {
        // Same-mode different-account scenario: wallets.json has
        // lastLoginMode=email with email "prev@example.com"; current_mode=email
        // BUT current_email="new@example.com" → fires the same exit-2
        // confirming envelope. Scene line uses masked emails (PII §8.1).
        // Audit args include switch_kind=account so off-box telemetry can
        // split this case from a mode switch.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_email_to_email");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let res = check_login_mode_diff("email", Some("newalice@example.com"), None, false);
        let err = res.expect_err("same-mode different-account must fire");
        let confirming = err
            .downcast_ref::<output::CliConfirming>()
            .expect("must be CliConfirming");
        assert!(confirming
            .message
            .contains("not the account you used last time"));
        // PII §8.1 regression: raw email local parts must NOT appear.
        assert!(
            !confirming.message.contains("prev@example.com"),
            "raw old email leaked: {}",
            confirming.message
        );
        assert!(
            !confirming.message.contains("newalice@example.com"),
            "raw new email leaked: {}",
            confirming.message
        );
        // Masked forms (first char + last char + domain) should appear.
        assert!(
            confirming.message.contains("p***v@example.com"),
            "old masked email missing: {}",
            confirming.message
        );
        assert!(
            confirming.message.contains("n***e@example.com"),
            "new masked email missing: {}",
            confirming.message
        );

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["command"], "login_mode_prompt_shown");
        let args = entries[0]["args"].as_array().expect("args present");
        let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
        assert!(arg_strs.contains(&"current_mode=email"));
        assert!(arg_strs.contains(&"last_login_mode=email"));
        assert!(arg_strs.contains(&"switch_kind=account"));
        // Audit args MUST NOT carry the raw email either.
        assert!(
            !arg_strs.iter().any(|s| s.contains("prev@example.com")
                || s.contains("newalice@example.com")),
            "audit args leaked raw email"
        );

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_force_bypasses_and_writes_paired_audit() {
        // Scenario (f): mode-diff with --force → no prompt; login proceeds;
        // audit contains TWO new entries back-to-back: prompt_shown THEN
        // user_choice (user_choice=yes).
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_force_pair");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let res = check_login_mode_diff("ak", None, None, true);
        assert!(res.is_ok(), "--force must bypass the gate");

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert_eq!(entries.len(), 2, "paired prompt_shown + user_choice");
        assert_eq!(entries[0]["command"], "login_mode_prompt_shown");
        assert_eq!(entries[1]["command"], "login_mode_prompt_user_choice");

        let user_args = entries[1]["args"].as_array().expect("args");
        let user_arg_strs: Vec<&str> = user_args.iter().filter_map(|v| v.as_str()).collect();
        assert!(user_arg_strs.contains(&"current_mode=ak"));
        assert!(user_arg_strs.contains(&"last_login_mode=email"));
        assert!(user_arg_strs.contains(&"switch_kind=mode"));
        assert!(user_arg_strs.contains(&"user_choice=yes"));

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_force_no_diff_writes_no_audit() {
        // Last row of the truth table: would_fire=false + force=true → no
        // audit writes; proceed normally.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_force_no_diff");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        // Same mode (email) + same email + force=true → would_fire=false → no audit.
        let res = check_login_mode_diff("email", Some("prev@example.com"), None, true);
        assert!(res.is_ok());

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert!(entries.is_empty());

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_confirming_payload_shape() {
        // Confirming payload contract — message is the 4-line template:
        //   1. discriminator "This is not the account you used last time."
        //   2. scene line (depends on scenario — for Email→AK mode switch,
        //      "Login method: Email (<masked>) → API Key")
        //   3. asset-safety reassurance
        //   4. "Continue? [Yes / No]"
        // `next` is the verbatim skill re-invocation instruction. Re-verified
        // standalone so message-format regressions are easy to spot.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cmd_login_mode_diff_sandbox("cmd_login_mode_diff_payload_shape");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let err = check_login_mode_diff("ak", None, None, false).expect_err("fires");
        let c = err
            .downcast_ref::<output::CliConfirming>()
            .expect("CliConfirming");
        let expected_msg = "⚠️ This is not the account you used last time.\n\
             Login method: Email (p***v@example.com) → API Key\n\
             Your previous account's assets are untouched and still accessible — log in with the original account to view them.\n\
             Continue? [Yes / No]";
        assert_eq!(c.message, expected_msg);
        assert_eq!(
            c.next,
            "If the user confirms, re-run the same command with --force flag appended to proceed."
        );

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_no_prompt_when_same_ak_mode_no_session() {
        // Edge: last=ak, current=ak, but session.json is missing on disk.
        // Without the previous api_key, the comparison is undefined →
        // no-op (preserves the old AK-switch guard's `if let Ok(Some(..))`
        // tolerance). Documents the no-session fall-through path.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_same_ak_no_session");

        let prev = WalletsJson {
            email: String::new(),
            is_ak: true,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();
        // Note: no save_session — session.json is absent.

        let res = check_login_mode_diff("ak", None, Some("anyKey-abcd-1234-5678"), false);
        assert!(res.is_ok(), "missing session → no prompt");

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert!(entries.is_empty(), "no audit entries written");

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_no_prompt_when_same_ak_same_key() {
        // last=ak with api_key="abcd1234-5678-90ab-cdef-1234567890ab",
        // current=ak with the SAME api_key → no prompt; no audit entries.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_same_ak_same_key");

        let prev = WalletsJson {
            email: String::new(),
            is_ak: true,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let same_key = "abcd1234-5678-90ab-cdef-1234567890ab";
        wallet_store::save_session(&wallet_store::SessionJson {
            api_key: same_key.to_string(),
            ..Default::default()
        })
        .unwrap();

        let res = check_login_mode_diff("ak", None, Some(same_key), false);
        assert!(res.is_ok(), "same ak + same key → no prompt");

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert!(entries.is_empty(), "no audit entries written");

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_fires_when_same_ak_different_key() {
        // Same-mode different-account scenario for AK: wallets.json has
        // is_ak=true, session.json has api_key="OLDKEY...". Caller passes
        // a different api_key → fires exit-2 confirming envelope with
        // mask_api_key applied to BOTH keys in the scene line. Audit args
        // include switch_kind=account (mirrors the email1→email2 case).
        // Replaces the previous AK-switch `bail!()` guard with the unified
        // confirming flow.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_ak_to_different_ak");

        let prev = WalletsJson {
            email: String::new(),
            is_ak: true,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        let old_key = "OLDKEY1234-5678-90ab-cdef-1234567890ab";
        let new_key = "NEWKEY5678-9012-34cd-ef01-5678901234ef";
        wallet_store::save_session(&wallet_store::SessionJson {
            api_key: old_key.to_string(),
            ..Default::default()
        })
        .unwrap();

        let res = check_login_mode_diff("ak", None, Some(new_key), false);
        let err = res.expect_err("same-mode different-ak must fire");
        let confirming = err
            .downcast_ref::<output::CliConfirming>()
            .expect("must be CliConfirming");
        assert!(confirming
            .message
            .contains("not the account you used last time"));
        // PII §8.1 regression: raw api_keys must NOT appear in the message.
        assert!(
            !confirming.message.contains(old_key),
            "raw old api_key leaked: {}",
            confirming.message
        );
        assert!(
            !confirming.message.contains(new_key),
            "raw new api_key leaked: {}",
            confirming.message
        );
        // Same-mode AK switch uses a fixed PII-clean sentence — no key
        // (masked or otherwise) appears in the user-facing message. The
        // user already knows the env value; we only flag the divergence.
        assert!(
            confirming
                .message
                .contains("The API Key in your env has changed"),
            "scene line shape wrong: {}",
            confirming.message
        );

        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["command"], "login_mode_prompt_shown");
        let args = entries[0]["args"].as_array().expect("args present");
        let arg_strs: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();
        assert!(arg_strs.contains(&"current_mode=ak"));
        assert!(arg_strs.contains(&"last_login_mode=ak"));
        assert!(arg_strs.contains(&"switch_kind=account"));
        // Audit args MUST NOT carry raw api_keys either.
        assert!(
            !arg_strs.iter().any(|s| s.contains(old_key) || s.contains(new_key)),
            "audit args leaked raw api_key"
        );

        cmd_login_mode_diff_cleanup();
    }

    #[test]
    fn cmd_login_mode_diff_audit_disk_failure_control_flow_unchanged() {
        // Scenario (k) / FR-4-AC-3: when audit::log cannot write to disk
        // (simulated by replacing audit.jsonl with a directory so the append
        // path fails), control flow MUST remain identical to the
        // writable-disk case. `audit::log` swallows I/O errors internally
        // (see `audit.rs:try_log`), so `check_login_mode_diff` keeps
        // returning the expected `Err(CliConfirming)` for the (would_fire,
        // !force) row and the expected `Ok(())` for the (would_fire, force)
        // row regardless of audit success.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = cmd_login_mode_diff_sandbox("cmd_login_mode_diff_audit_disk_fail");

        let prev = WalletsJson {
            email: "prev@example.com".to_string(),
            is_ak: false,
            ..Default::default()
        };
        wallet_store::save_wallets(&prev).unwrap();

        // Force audit writes to fail: place a directory at audit.jsonl so
        // OpenOptions::append(true).open(...) returns an error every call.
        let audit_path = dir.join("audit.jsonl");
        std::fs::create_dir_all(&audit_path).unwrap();

        // (would_fire=true, force=false) — control flow must still be Err.
        let err = check_login_mode_diff("ak", None, None, false).expect_err("must still fire");
        let confirming = err
            .downcast_ref::<output::CliConfirming>()
            .expect("must be CliConfirming");
        assert!(confirming
            .message
            .contains("not the account you used last time"));

        // (would_fire=true, force=true) — control flow must still be Ok.
        let ok = check_login_mode_diff("ak", None, None, true);
        assert!(ok.is_ok(), "--force path must still succeed");

        // Audit lines remain unreadable (directory at audit.jsonl), so the
        // helper returns an empty list — confirms writes silently failed.
        let entries = cmd_login_mode_diff_audit_lines(&dir);
        assert!(
            entries.is_empty(),
            "audit writes must be silently dropped when disk is unwritable"
        );

        cmd_login_mode_diff_cleanup();
    }

    // ── validate_locale tests ────────────────────────────────────────

    #[test]
    fn validate_locale_passes_en_us() {
        assert_eq!(validate_locale("en-US"), ("en-US", false));
    }

    #[test]
    fn validate_locale_passes_zh_cn() {
        assert_eq!(validate_locale("zh-CN"), ("zh-CN", false));
    }

    #[test]
    fn validate_locale_falls_back_for_ja_jp() {
        assert_eq!(validate_locale("ja_JP"), ("en-US", true));
    }

    #[test]
    fn validate_locale_falls_back_for_underscore_en() {
        assert_eq!(validate_locale("en_US"), ("en-US", true));
    }

    #[test]
    fn validate_locale_falls_back_for_arbitrary() {
        assert_eq!(validate_locale("xx-YY"), ("en-US", true));
    }

    #[test]
    fn validate_locale_falls_back_for_empty_string() {
        assert_eq!(validate_locale(""), ("en-US", true));
    }
}
