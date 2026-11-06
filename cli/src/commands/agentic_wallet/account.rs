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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_policy_reports_actual_values() {
        let policy = json!({
            "singleTxLimit": "100", "singleTxFlag": true,
            "dailyTransferTxLimit": "200", "dailyTransferTxFlag": true, "dailyTransferTxUsed": "50",
            "dailyTradeTxLimit": "300", "dailyTradeTxFlag": false, "dailyTradeTxUsed": "0",
            "doubleConfirmation": false
        });
        let whitelist =
            json!({ "addresses": [{ "address": "0x1", "name": "" }], "whiteFlag": true });
        let out = build_security_policy(Some(policy), Some(whitelist));
        assert_eq!(out["status"], "available");
        assert_eq!(out["singleTxLimit"], "100");
        assert_eq!(out["dailyTransferTxLimit"], "200");
        assert_eq!(out["dailyTransferTxUsed"], "50");
        // Flag off ⇒ not configured, even with a stored limit.
        assert!(out["dailyTradeTxLimit"].is_null());
        // A user's explicit "off" is reported as off, never as on.
        assert_eq!(out["doubleConfirmation"], false);
        assert_eq!(
            out["whitelist"],
            json!({ "enabled": true, "addressCount": 1 })
        );
    }

    #[test]
    fn security_policy_unset_values_stay_null_and_are_not_guessed() {
        let policy = json!({
            "singleTxLimit": "", "singleTxFlag": true,
            "dailyTransferTxLimit": "0", "dailyTransferTxFlag": false,
            "dailyTradeTxFlag": false, "doubleConfirmation": true
        });
        let out = build_security_policy(Some(policy), Some(serde_json::Value::Null));
        assert_eq!(out["status"], "available");
        assert!(out["singleTxLimit"].is_null());
        assert!(out["dailyTransferTxLimit"].is_null());
        assert!(out["dailyTradeTxLimit"].is_null());
        assert_eq!(out["doubleConfirmation"], true);
        assert_eq!(
            out["whitelist"],
            json!({ "enabled": false, "addressCount": 0 })
        );
    }

    #[test]
    fn security_policy_query_failure_is_unavailable_not_default() {
        let out = build_security_policy(None, Some(json!({ "whiteFlag": true })));
        assert_eq!(out, json!({ "status": "unavailable" }));
        let out = build_security_policy(Some(json!("bad")), None);
        assert_eq!(out, json!({ "status": "unavailable" }));
    }

    #[test]
    fn security_policy_empty_success_is_available_with_unset_values() {
        let out =
            build_security_policy(Some(serde_json::Value::Null), Some(serde_json::Value::Null));
        assert_eq!(out["status"], "available");
        assert!(out["singleTxLimit"].is_null());
        assert!(out["doubleConfirmation"].is_null());
        assert_eq!(
            out["whitelist"],
            json!({ "enabled": false, "addressCount": 0 })
        );
    }

    fn flags_off() -> serde_json::Value {
        json!({
            "singleTxFlag": false,
            "dailyTransferTxFlag": false,
            "dailyTradeTxFlag": false,
            "doubleConfirmation": false
        })
    }

    #[test]
    fn policy_record_without_limit_flags_is_unavailable() {
        for record in [
            json!({}),
            json!({ "doubleConfirmation": true }),
            json!({ "singleTxFlag": true, "dailyTransferTxFlag": false }),
            json!({ "singleTxFlag": "true", "dailyTransferTxFlag": false, "dailyTradeTxFlag": false }),
        ] {
            assert_eq!(
                build_security_policy(Some(record), Some(serde_json::Value::Null)),
                json!({ "status": "unavailable" })
            );
        }
        // doubleConfirmation is required too.
        let mut no_double = flags_off();
        no_double
            .as_object_mut()
            .unwrap()
            .remove("doubleConfirmation");
        assert_eq!(
            build_security_policy(Some(no_double), Some(serde_json::Value::Null)),
            json!({ "status": "unavailable" })
        );
        let out = build_security_policy(Some(flags_off()), Some(serde_json::Value::Null));
        assert_eq!(out["status"], "available");
        assert_eq!(out["doubleConfirmation"], false);
    }

    #[test]
    fn query_item_accepts_arrays_and_rejects_other_shapes() {
        assert_eq!(
            query_item(&json!([{ "a": 1 }]), "p").unwrap(),
            json!({ "a": 1 })
        );
        assert_eq!(
            query_item(&json!([]), "p").unwrap(),
            serde_json::Value::Null
        );
        for odd in [json!({ "a": 1 }), json!(null), json!("x")] {
            assert!(query_item(&odd, "p").is_err());
        }
    }

    #[test]
    fn whitelist_without_white_flag_is_unknown_not_off() {
        let out = build_security_policy(Some(flags_off()), Some(json!({ "addresses": [] })));
        assert_eq!(out["status"], "available");
        assert!(out["whitelist"].is_null());
    }

    #[test]
    fn security_policy_whitelist_failure_keeps_policy_and_marks_whitelist_unknown() {
        let mut policy = flags_off();
        policy["doubleConfirmation"] = json!(true);
        let out = build_security_policy(Some(policy), None);
        assert_eq!(out["status"], "available");
        assert_eq!(out["doubleConfirmation"], true);
        assert!(out["whitelist"].is_null());
    }

    #[test]
    fn logged_in_wallet_status_summary_has_no_subscription_or_device_snapshot() {
        let wallets = WalletsJson {
            email: "buyer@example.com".to_string(),
            login_type: "email".to_string(),
            selected_account_id: "account-1".to_string(),
            accounts: vec![crate::wallet_store::AccountInfo {
                project_id: "project-1".to_string(),
                account_id: "account-1".to_string(),
                account_name: "Trading".to_string(),
                is_default: true,
            }],
            ..Default::default()
        };

        let summary =
            build_wallet_status_summary(&wallets, true, "Trading", json!({ "enabled": true }));

        assert_eq!(summary["loggedIn"], json!(true));
        assert_eq!(summary["currentAccountId"], json!("account-1"));
        assert_eq!(summary["policy"], json!({ "enabled": true }));
        assert!(summary.get("postLoginSubscriptions").is_none());
        assert!(summary.get("devices").is_none());
    }

    // ── resolve_account_address_for_chain ─────────────────────────────

    fn addr(chain_index: &str, address: &str) -> crate::wallet_store::AddressInfo {
        crate::wallet_store::AddressInfo {
            account_id: "account-1".to_string(),
            address: address.to_string(),
            chain_index: chain_index.to_string(),
            chain_name: String::new(),
            address_type: String::new(),
            chain_path: String::new(),
        }
    }

    /// Build a `WalletsJson` whose selected account (`account-1`) owns the
    /// given address list.
    fn wallets_with_addresses(address_list: Vec<crate::wallet_store::AddressInfo>) -> WalletsJson {
        let mut accounts_map = std::collections::HashMap::new();
        accounts_map.insert(
            "account-1".to_string(),
            crate::wallet_store::AccountMapEntry { address_list },
        );
        WalletsJson {
            selected_account_id: "account-1".to_string(),
            accounts_map,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_account_address_for_chain_evm_returns_shared_evm_address() {
        let wallets =
            wallets_with_addresses(vec![addr("1", "0xEvmShared"), addr("501", "SoLaNaAddr")]);

        // Exact EVM chain match.
        assert_eq!(
            resolve_account_address_for_chain(&wallets, "1").unwrap(),
            "0xEvmShared"
        );
        // Another EVM chain with no dedicated entry shares the common EVM
        // address (spec §7.1 "EVM address (shared)").
        assert_eq!(
            resolve_account_address_for_chain(&wallets, "42161").unwrap(),
            "0xEvmShared"
        );
    }

    #[test]
    fn resolve_account_address_for_chain_solana_returns_solana_address() {
        let wallets =
            wallets_with_addresses(vec![addr("1", "0xEvmShared"), addr("501", "SoLaNaAddr")]);

        assert_eq!(
            resolve_account_address_for_chain(&wallets, "501").unwrap(),
            "SoLaNaAddr"
        );
    }

    #[test]
    fn resolve_account_address_for_chain_missing_chain_errs() {
        // Account has only an EVM address — Bitcoin (0) must NOT fall back to
        // the EVM address; it is a real resolution failure (spec §3.2).
        let wallets = wallets_with_addresses(vec![addr("1", "0xEvmShared")]);

        assert!(resolve_account_address_for_chain(&wallets, "0").is_err());
    }

    #[test]
    fn resolve_account_address_for_chain_tron_and_ton_never_fall_back_to_evm() {
        // Tron (195) / TON (607) are non-EVM account-model chains with their
        // own native address format — an account with only an EVM address
        // must NOT resolve a Tron/TON top-up target to it (adversarial review
        // Finding 1: wrong deposit address risk).
        let wallets = wallets_with_addresses(vec![addr("1", "0xEvmShared")]);

        assert!(resolve_account_address_for_chain(&wallets, "195").is_err());
        assert!(resolve_account_address_for_chain(&wallets, "607").is_err());
    }

    #[test]
    fn resolve_account_address_for_unknown_chain_never_falls_back_to_evm() {
        let wallets = wallets_with_addresses(vec![addr("1", "0xEvmShared")]);

        assert!(resolve_account_address_for_chain(&wallets, "999999").is_err());
    }

    #[test]
    fn resolve_account_address_for_chain_uses_selected_account_only() {
        let mut wallets = wallets_with_addresses(vec![addr("1", "0xSelected")]);
        wallets.accounts_map.insert(
            "account-2".to_string(),
            crate::wallet_store::AccountMapEntry {
                address_list: vec![addr("1", "0xOther")],
            },
        );

        // selected_account_id is "account-1" → resolves that account's address,
        // never account-2's.
        assert_eq!(
            resolve_account_address_for_chain(&wallets, "1").unwrap(),
            "0xSelected"
        );
    }
}

// ── status ───────────────────────────────────────────────────────────

fn build_wallet_status_summary(
    wallets: &WalletsJson,
    logged_in: bool,
    current_account_name: &str,
    policy: serde_json::Value,
) -> serde_json::Value {
    let login_type = if !logged_in || wallets.login_type.is_empty() {
        serde_json::Value::Null
    } else {
        json!(wallets.login_type)
    };

    json!({
        "email": wallets.email,
        "loggedIn": logged_in,
        "loginType": login_type,
        "currentAccountId": wallets.selected_account_id,
        "currentAccountName": current_account_name,
        "accountCount": wallets.accounts.len(),
        "policy": policy,
    })
}

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
    // Propagate keyring corruption instead of swallowing it: read_blob returns
    // Ok(empty) for "not logged in" (no entry) and Err only on a corrupted
    // store, so `?` surfaces "Credentials corrupted. Please login again" here
    // rather than silently reporting loggedIn:false (spec §3 / §8.5 #7, P1#7).
    let blob = keyring_store::read_blob()?;

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

    let summary = build_wallet_status_summary(&wallets, logged_in, &current_account_name, policy);
    output::success(summary);
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
        Some(input) => Some(super::chain_profile::resolve(input).await?.chain_index),
        None => None,
    };

    let mut xlayer = Vec::new();
    let mut evm = Vec::new();
    let mut solana = Vec::new();
    let mut bitcoin = Vec::new();
    let mut sui = Vec::new();

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
            "0" => bitcoin.push(item),
            "784" => sui.push(item),
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
        "bitcoin": bitcoin,
        "sui": sui,
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

// ── security policy (login summary) ──────────────────────────────────

/// Upper bound for each login-time policy query, so a slow policy service
/// never delays or fails the login result.
const SECURITY_POLICY_TIMEOUT_SECS: u64 = 5;

/// Query the effective security policy and whitelist for `account_id` and
/// build the `securityPolicy` object of the login summary. Read-only: the CLI
/// never writes defaults here; the server initializes them.
pub(super) async fn fetch_security_policy(
    access_token: &str,
    account_id: &str,
) -> serde_json::Value {
    let timeout = std::time::Duration::from_secs(SECURITY_POLICY_TIMEOUT_SECS);
    let query = |path: &'static str| async move {
        let mut client = WalletApiClient::new()?;
        let data = tokio::time::timeout(
            timeout,
            client.get_authed(path, access_token, &[("accountId", account_id)]),
        )
        .await
        .map_err(|_| anyhow::anyhow!("{path} timed out"))??;
        query_item(&data, path)
    };
    let (policy, whitelist) = tokio::join!(
        query("/priapi/v5/wallet/agentic/policy/query"),
        query("/priapi/v5/wallet/agentic/white/query"),
    );
    if cfg!(feature = "debug-log") {
        if let Err(e) = &policy {
            eprintln!("[DEBUG][login] security policy query failed: {e:#}");
        }
        if let Err(e) = &whitelist {
            eprintln!("[DEBUG][login] whitelist query failed: {e:#}");
        }
    }
    build_security_policy(policy.ok(), whitelist.ok())
}

/// First item of a policy query response. An empty array is a valid "no
/// record"; any other shape is an unrecognized response and counts as a
/// failed query, so it is shown as unavailable instead of "not configured".
fn query_item(data: &serde_json::Value, path: &str) -> Result<serde_json::Value> {
    match data.as_array() {
        Some(items) => Ok(items.first().cloned().unwrap_or(serde_json::Value::Null)),
        None => Err(anyhow::anyhow!("{path}: unexpected response shape")),
    }
}

/// A limit counts as configured only when its flag is on and the limit is a
/// non-empty value (the backend uses `""` for "no limit").
fn configured_limit(policy: &serde_json::Value, limit: &str, flag: &str) -> serde_json::Value {
    let enabled = policy[flag].as_bool() == Some(true);
    let value = match &policy[limit] {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    match (enabled, value) {
        (true, Some(value)) => json!(value),
        _ => serde_json::Value::Null,
    }
}

/// Boolean flags every policy record carries (the backend returns `false`
/// when nothing is set).
const POLICY_LIMIT_FLAGS: [&str; 4] = [
    "singleTxFlag",
    "dailyTransferTxFlag",
    "dailyTradeTxFlag",
    "doubleConfirmation",
];

fn has_limit_flags(policy: &serde_json::Value) -> bool {
    POLICY_LIMIT_FLAGS
        .iter()
        .all(|flag| policy[*flag].is_boolean())
}

/// Build `securityPolicy` from the raw `policy/query` and `white/query` items.
/// `None` means the query failed; `Some(Null)` means it succeeded with no
/// record, so every value is unset. A record without the boolean flags is
/// unrecognized and counts as unavailable. Unknown values stay `null`;
/// nothing is guessed.
fn build_security_policy(
    policy: Option<serde_json::Value>,
    whitelist: Option<serde_json::Value>,
) -> serde_json::Value {
    let policy = match policy {
        Some(serde_json::Value::Null) => json!({}),
        Some(p) if p.is_object() && has_limit_flags(&p) => p,
        _ => return json!({ "status": "unavailable" }),
    };
    let used = |field: &str| match &policy[field] {
        serde_json::Value::String(s) if !s.is_empty() => json!(s),
        serde_json::Value::Number(n) => json!(n.to_string()),
        _ => serde_json::Value::Null,
    };
    let whitelist = match whitelist {
        // `whiteFlag` must be a boolean; a missing or odd value means the
        // state is unknown, never "off".
        Some(w) if w.is_object() => match w["whiteFlag"].as_bool() {
            Some(enabled) => json!({
                "enabled": enabled,
                "addressCount": w["addresses"].as_array().map(Vec::len).unwrap_or(0),
            }),
            None => serde_json::Value::Null,
        },
        // A successful query with no record means no whitelist is set.
        Some(serde_json::Value::Null) => json!({ "enabled": false, "addressCount": 0 }),
        _ => serde_json::Value::Null,
    };
    json!({
        "status": "available",
        "singleTxLimit": configured_limit(&policy, "singleTxLimit", "singleTxFlag"),
        "dailyTransferTxLimit": configured_limit(&policy, "dailyTransferTxLimit", "dailyTransferTxFlag"),
        "dailyTransferTxUsed": used("dailyTransferTxUsed"),
        "dailyTradeTxLimit": configured_limit(&policy, "dailyTradeTxLimit", "dailyTradeTxFlag"),
        "dailyTradeTxUsed": used("dailyTradeTxUsed"),
        "doubleConfirmation": policy["doubleConfirmation"].as_bool(),
        "whitelist": whitelist,
    })
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

// ── resolve_account_address_for_chain ─────────────────────────────────

/// Only chains explicitly classified as EVM may share the common EVM address.
/// Unknown chains fail closed instead of risking a wrong deposit address/QR.
fn shares_evm_address(chain_index: &str) -> bool {
    crate::chains::is_evm_chain(chain_index)
}

/// Resolve the **selected** account's own receive address for `chain_index`
/// from the locally-cached `accounts_map` in `wallets.json` (spec §2.5
/// `FundingTarget.receive_address`, §6.2, §7.1). This is a local lookup — no
/// external address input is accepted (§8.2 anti-spoof), the address always
/// belongs to the caller's own selected account.
///
/// Resolution order (spec §7.1):
/// 1. Exact `chain_index` match — covers Solana (501), Bitcoin, SUI, an
///    independent X Layer (196) address, and any specific EVM chain.
/// 2. EVM-family fallback — EVM chains (including X Layer without its own
///    entry) share one common EVM address, so any EVM-family address is used.
///
/// Returns `Err` when the selected account has no address for the target chain,
/// so callers can branch (T8/T10/T11 treat Wallet-Send/Swap as an error; A2A
/// degrades — spec §3.2 "Address resolution failure").
///
/// `pub` so sibling scenes (funding-target resolver) and external modules can
/// call it; refreshing a stale cache via `ensure_wallet_accounts_fresh` is the
/// caller's responsibility before invoking this pure lookup.
pub fn resolve_account_address_for_chain(
    wallets: &WalletsJson,
    chain_index: &str,
) -> Result<String> {
    let account_id = resolve_active_account_id(wallets)?;
    let entry = wallets
        .accounts_map
        .get(&account_id)
        .ok_or_else(|| anyhow::anyhow!("account not found"))?;

    // 1. Exact chain-index match on the selected account only.
    if let Some(a) = entry
        .address_list
        .iter()
        .find(|a| a.chain_index == chain_index && !a.address.is_empty())
    {
        return Ok(a.address.clone());
    }

    // 2. EVM-family chains share the common EVM address.
    if shares_evm_address(chain_index) {
        if let Some(a) = entry
            .address_list
            .iter()
            .find(|a| shares_evm_address(&a.chain_index) && !a.address.is_empty())
        {
            return Ok(a.address.clone());
        }
    }

    bail!("no address for chain \"{chain_index}\" on the selected account")
}
