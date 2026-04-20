use anyhow::{bail, Result};
use serde_json::json;

use crate::keyring_store;
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, WalletsJson};

use super::auth::{ensure_tokens_refreshed, is_session_key_expired, is_token_expired};

// ── switch ───────────────────────────────────────────────────────────

/// Core switch logic: load wallets, validate account_id, update selected, save.
pub(super) fn switch_to_account(account_id: &str) -> Result<()> {
    if account_id.is_empty() {
        bail!("account id is required");
    }

    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] switch_to_account: loaded wallets, selected_account_id={}, accounts_map keys={:?}",
            wallets.selected_account_id,
            wallets.accounts_map.keys().collect::<Vec<_>>()
        );
    }

    if !wallets.accounts_map.contains_key(account_id) {
        bail!("account not found");
    }

    wallets.selected_account_id = account_id.to_string();
    wallet_store::save_wallets(&wallets)?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] switch_to_account: switched to account_id={}",
            account_id
        );
    }

    Ok(())
}

/// onchainos wallet switch <account_id>
pub(super) async fn cmd_switch(account_id: &str) -> Result<()> {
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_switch: account_id={}", account_id);
    }

    switch_to_account(account_id)?;

    output::success_empty();
    Ok(())
}

// ── status ───────────────────────────────────────────────────────────

/// onchainos wallet status
pub(super) async fn cmd_status() -> Result<()> {
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG] cmd_status: start");
    }

    let wallets = match wallet_store::load_wallets()? {
        Some(w) => w,
        None => {
            if cfg!(feature = "debug-log") {
                eprintln!("[DEBUG] cmd_status: wallets.json not found, returning not logged in");
            }
            output::success(json!({
                "email": "",
                "loggedIn": false,
                "currentAccountId": "",
                "currentAccountName": "",
                "accountCount": 0,
            }));
            return Ok(());
        }
    };

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] cmd_status: loaded wallets, email={}, selected_account_id={}, accounts_count={}",
            wallets.email, wallets.selected_account_id, wallets.accounts.len()
        );
    }

    let session = wallet_store::load_session()?.unwrap_or_default();
    let blob = keyring_store::read_blob().unwrap_or_default();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] cmd_status: session.session_key_expire_at={}, keyring blob keys={:?}, refresh_token_len={}",
            session.session_key_expire_at,
            blob.keys().collect::<Vec<_>>(),
            blob.get("refresh_token").map(|t| t.len()).unwrap_or(0)
        );
    }

    let logged_in = !is_session_key_expired(&session.session_key_expire_at)
        && blob
            .get("refresh_token")
            .map(|t| !t.is_empty() && !is_token_expired(t))
            .unwrap_or(false);

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] cmd_status: session_key_expired={}, logged_in={}",
            is_session_key_expired(&session.session_key_expire_at),
            logged_in
        );
    }

    let current_account_name = wallets
        .accounts
        .iter()
        .find(|a| a.account_id == wallets.selected_account_id)
        .map(|a| a.account_name.clone())
        .unwrap_or_default();

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG] cmd_status: result email={}, logged_in={}, account_id={}, account_name={}",
            wallets.email, logged_in, wallets.selected_account_id, current_account_name
        );
    }

    // Determine loginType and apiKey
    let (login_type, api_key): (serde_json::Value, serde_json::Value) = if !logged_in {
        (serde_json::Value::Null, serde_json::Value::Null)
    } else if wallets.is_ak {
        let ak = if session.api_key.is_empty() {
            String::new()
        } else {
            session.api_key.clone()
        };
        (json!("ak"), json!(ak))
    } else {
        (json!("email"), serde_json::Value::Null)
    };

    // Query policy for the current account when logged in
    let policy = if logged_in && !wallets.selected_account_id.is_empty() {
        match query_policy(&wallets.selected_account_id).await {
            Ok(val) => val,
            Err(e) => {
                if cfg!(feature = "debug-log") {
                    eprintln!("[DEBUG] cmd_status: policy query failed: {e}");
                }
                serde_json::Value::Null
            }
        }
    } else {
        serde_json::Value::Null
    };

    output::success(json!({
        "email": wallets.email,
        "loggedIn": logged_in,
        "loginType": login_type,
        "apiKey": api_key,
        "currentAccountId": wallets.selected_account_id,
        "currentAccountName": current_account_name,
        "accountCount": wallets.accounts.len(),
        "policy": policy,
    }));
    Ok(())
}

// ── addresses ────────────────────────────────────────────────────────

/// onchainos wallet addresses [--chain <chain>]
/// Lists addresses for the current account, grouped by chain category.
/// When --chain is provided, only addresses matching that chain are returned.
pub(super) async fn cmd_addresses(chain: Option<&str>) -> Result<()> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let account_id = resolve_active_account_id(&wallets)?;
    let entry = wallets
        .accounts_map
        .get(&account_id)
        .ok_or_else(|| anyhow::anyhow!("account not found"))?;

    let account_name = wallets
        .accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .map(|a| a.account_name.as_str())
        .unwrap_or("");

    let chain_filter = match chain {
        Some(input) => {
            let entry = super::chain::get_chain_by_real_chain_index(input)
                .await?
                .ok_or_else(|| anyhow::anyhow!("unsupported chain: {input}"))?;
            let ci = entry["chainIndex"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| entry["chainIndex"].as_i64().map(|n| n.to_string()))
                .ok_or_else(|| anyhow::anyhow!("unsupported chain: {input}"))?;
            Some(ci)
        }
        None => None,
    };

    let mut xlayer = Vec::new();
    let mut evm = Vec::new();
    let mut solana = Vec::new();

    for addr in &entry.address_list {
        if let Some(ref filter) = chain_filter {
            if addr.chain_index != *filter {
                continue;
            }
        }
        let item = json!({
            "address": addr.address,
            "chainIndex": addr.chain_index,
            "chainName": addr.chain_name,
        });
        match addr.chain_index.as_str() {
            "196" => xlayer.push(item),
            "501" => solana.push(item),
            _ => evm.push(item),
        }
    }

    output::success(json!({
        "accountId": account_id,
        "accountName": account_name,
        "xlayer": xlayer,
        "evm": evm,
        "solana": solana,
    }));
    Ok(())
}

// ── query_policy ─────────────────────────────────────────────────────

/// Query policy settings for the given account via
/// GET /priapi/v5/wallet/agentic/policy/query.
/// Returns the first element of the `data` array, or Null on failure.
async fn query_policy(account_id: &str) -> Result<serde_json::Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;
    let data = client
        .get_authed(
            "/priapi/v5/wallet/agentic/policy/query",
            &access_token,
            &[("accountId", account_id)],
        )
        .await?;
    let first = data
        .as_array()
        .and_then(|arr| arr.first().cloned())
        .unwrap_or(serde_json::Value::Null);
    Ok(first)
}

// ── resolve_active_account_id ─────────────────────────────────────────

/// Resolve the active account ID: selected_account_id → is_default → first key.
/// `pub` so that sibling modules (balance, history, transfer) and external
/// modules (security) can call it.
pub fn resolve_active_account_id(wallets: &WalletsJson) -> Result<String> {
    if !wallets.selected_account_id.is_empty() {
        return Ok(wallets.selected_account_id.clone());
    }
    if let Some(acct) = wallets.accounts.iter().find(|a| a.is_default) {
        return Ok(acct.account_id.clone());
    }
    wallets
        .accounts_map
        .keys()
        .next()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no wallet accounts found"))
}
