use std::collections::HashMap;

use anyhow::{bail, Result};
use base64::Engine;
use serde_json::json;

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
        let now_dt = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
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
            let now_dt = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
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
            let now_dt = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
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
pub(super) async fn ensure_tokens_refreshed() -> Result<String> {
    let (access_token, refresh_token) = ensure_tokens()?;

    if is_token_expired(&access_token) {
        let client = WalletApiClient::new()?;
        let resp = client
            .auth_refresh(&refresh_token)
            .await
            .map_err(format_api_error)?;

        if cfg!(feature = "debug-log") {
            let now_ts = chrono::Utc::now().timestamp();
            let now_dt = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
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

        Ok(resp.access_token)
    } else {
        Ok(access_token)
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
pub(super) fn format_api_error(e: anyhow::Error) -> anyhow::Error {
    match e.downcast::<ApiCodeError>() {
        Ok(api_err) => anyhow::anyhow!("{}", api_err.msg),
        Err(e) => e,
    }
}

// ── Login ────────────────────────────────────────────────────────────

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

        let client = WalletApiClient::new()?;
        let resp = client
            .auth_init(email, locale)
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

                // Check if switching API Keys — warn unless --force
                if !force {
                    if let Ok(Some(wallets)) = wallet_store::load_wallets() {
                        if wallets.is_ak {
                            if let Ok(Some(session)) = wallet_store::load_session() {
                                if !session.api_key.is_empty() && session.api_key != api_key {
                                    bail!(
                                        "You are about to switch from API Key \"{}\" to \"{}\". \
                                         If you are sure, re-run with --force to confirm.",
                                        session.api_key,
                                        api_key
                                    );
                                }
                            }
                        }
                    }
                }

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
    let client = WalletApiClient::new()?;

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

    save_verify_result(&client, &resp, &session_private_key, "", api_key).await
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

    let client = WalletApiClient::new()?;

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

    save_verify_result(&client, &resp, &session_private_key, email, "").await
}

/// Common post-verify logic: persist credentials, fetch accounts, output result.
async fn save_verify_result(
    client: &WalletApiClient,
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
    }));
    Ok(())
}

/// Fetch account/list and account/address/list, update wallets.json.
/// Non-fatal: failures are logged as warnings.
async fn fetch_and_save_account_list(
    client: &WalletApiClient,
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
        eprintln!(
            "[DEBUG] cmd_add: access_token_len={}",
            access_token.len()
        );
    }

    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    if wallets.project_id.is_empty() {
        bail!(super::common::ERR_NOT_LOGGED_IN);
    }

    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_add: project_id={}", wallets.project_id);
    }

    let client = WalletApiClient::new()?;

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

    output::success_empty();
    Ok(())
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
}
