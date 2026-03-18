use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, AccountMapEntry, AddressInfo, BalanceCacheEntry, WalletsJson};

use super::account::resolve_active_account_id;
use super::auth::{ensure_tokens_refreshed, format_api_error};

/// Cache TTL for the all-accounts batch balance query.
const BATCH_BALANCE_TTL: i64 = 60;

// ── account freshness helpers ─────────────────────────────────────────

/// Returns true if wallet accounts data needs a refresh:
///   - `wallets.accounts` is empty, OR
///   - any account in the list is missing from `accounts_map`
fn wallet_accounts_need_refresh(wallets: &WalletsJson) -> bool {
    wallets.accounts.is_empty()
        || wallets
            .accounts
            .iter()
            .any(|a| !wallets.accounts_map.contains_key(&a.account_id))
}

/// Extract the EVM address for a given account from accounts_map.
pub(super) fn get_evm_address<'a>(wallets: &'a WalletsJson, account_id: &str) -> &'a str {
    wallets
        .accounts_map
        .get(account_id)
        .and_then(|e| {
            e.address_list
                .iter()
                .find(|a| crate::chains::chain_family(&a.chain_index) == "evm")
        })
        .map(|a| a.address.as_str())
        .unwrap_or("")
}

/// Extract the Solana address for a given account from accounts_map.
pub(super) fn get_sol_address<'a>(wallets: &'a WalletsJson, account_id: &str) -> &'a str {
    wallets
        .accounts_map
        .get(account_id)
        .and_then(|e| e.address_list.iter().find(|a| a.chain_index == "501"))
        .map(|a| a.address.as_str())
        .unwrap_or("")
}

/// Ensure wallet accounts and address data is complete.
///
/// Triggers a refresh if:
///   - `wallets.accounts` is empty (no account list)
///   - Any account in the list is missing from `accounts_map` (incomplete address data)
///
/// On refresh, calls `account/list` + `account/address/list` and persists to wallets.json.
async fn ensure_wallet_accounts_fresh(
    client: &WalletApiClient,
    access_token: &str,
    wallets: &mut WalletsJson,
    force: bool,
) -> Result<()> {
    if !force && !wallet_accounts_need_refresh(wallets) {
        return Ok(());
    }

    match client.account_list(access_token, &wallets.project_id).await {
        Ok(account_list) => {
            wallets.accounts = account_list
                .iter()
                .map(|a| wallet_store::AccountInfo {
                    project_id: a.project_id.clone(),
                    account_id: a.account_id.clone(),
                    account_name: a.account_name.clone(),
                    is_default: a.is_default,
                })
                .collect();

            let account_ids: Vec<String> =
                account_list.iter().map(|a| a.account_id.clone()).collect();
            match client
                .account_address_list(access_token, &account_ids)
                .await
            {
                Ok(address_data) => {
                    for item in &address_data {
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
                }
                Err(e) => {
                    if cfg!(feature = "debug-log") {
                        eprintln!("Warning: failed to refresh address list: {e:#}");
                    }
                }
            }
            wallet_store::save_wallets(wallets)?;
        }
        Err(e) => {
            if cfg!(feature = "debug-log") {
                eprintln!("Warning: failed to refresh account list: {e:#}");
            }
        }
    }
    Ok(())
}

// ── usdValue enrichment ───────────────────────────────────────────────

/// XLayer chainIndex — used to pin it to the top of the results.
const XLAYER_CHAIN_INDEX: &str = "196";

/// Sort tokens inside `tokenAssets` (or `assets`): XLayer (196) first,
/// then remaining chains ordered by per-chain total USD descending.
/// Within the same chain, tokens are ordered by USD value descending.
///
/// `balance_single` returns `[{ tokenAssets: [ {chainIndex, ...}, ... ] }]` —
/// one group containing a flat list of tokens from all chains.
fn sort_token_assets(data: &mut Value) {
    let groups = match data.as_array_mut() {
        Some(arr) => arr.as_mut_slice(),
        None => std::slice::from_mut(data),
    };
    for group in groups {
        for key in &["tokenAssets", "assets"] {
            if let Some(tokens) = group.get_mut(*key).and_then(|v| v.as_array_mut()) {
                sort_tokens_vec(tokens);
                break;
            }
        }
    }
}

/// Core sorting logic on a token Vec.
fn sort_tokens_vec(tokens: &mut [Value]) {
    use std::collections::HashMap;

    // 1. Compute per-chain total USD for ordering chains.
    let mut chain_totals: HashMap<String, f64> = HashMap::new();
    for t in tokens.iter() {
        let ci = token_chain_index(t);
        let usd = token_usd(t);
        *chain_totals.entry(ci).or_default() += usd;
    }

    // 2. Sort: XLayer first, then by chain total USD desc, then by token USD desc.
    tokens.sort_by(|a, b| {
        let a_ci = token_chain_index(a);
        let b_ci = token_chain_index(b);
        let a_xlayer = a_ci == XLAYER_CHAIN_INDEX;
        let b_xlayer = b_ci == XLAYER_CHAIN_INDEX;

        // XLayer always first
        match (a_xlayer, b_xlayer) {
            (true, false) => return std::cmp::Ordering::Less,
            (false, true) => return std::cmp::Ordering::Greater,
            _ => {}
        }

        // Different chains → order by chain total USD desc
        if a_ci != b_ci {
            let a_total = chain_totals.get(&a_ci).copied().unwrap_or(0.0);
            let b_total = chain_totals.get(&b_ci).copied().unwrap_or(0.0);
            return b_total
                .partial_cmp(&a_total)
                .unwrap_or(std::cmp::Ordering::Equal);
        }

        // Same chain → order by token USD desc
        let a_usd = token_usd(a);
        let b_usd = token_usd(b);
        b_usd
            .partial_cmp(&a_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Extract chainIndex from a token object (handles both string and number).
fn token_chain_index(token: &Value) -> String {
    match &token["chainIndex"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Extract USD value from a token object.
fn token_usd(token: &Value) -> f64 {
    token["usdValue"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| token["usdValue"].as_f64())
        .unwrap_or(0.0)
}

/// Inject `usdValue` into each token within a single account group.
///
/// If `usdValue` is already present and non-null (from API), it is kept.
/// Otherwise it is computed as `balance × tokenPrice`.
fn enrich_group_usd_value(group: &mut Value) {
    for key in &["tokenAssets", "assets"] {
        if let Some(arr) = group.get_mut(*key).and_then(|v| v.as_array_mut()) {
            for asset in arr.iter_mut() {
                if asset
                    .get("usdValue")
                    .map(|v| !v.is_null() && v.as_str().map(|s| !s.is_empty()).unwrap_or(true))
                    .unwrap_or(false)
                {
                    continue;
                }
                let balance: f64 = asset["balance"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| asset["balance"].as_f64())
                    .unwrap_or(0.0);
                let price: f64 = asset["tokenPrice"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| asset["tokenPrice"].as_f64())
                    .unwrap_or(0.0);
                if let Some(obj) = asset.as_object_mut() {
                    obj.insert(
                        "usdValue".to_string(),
                        json!(format!("{:.6}", balance * price)),
                    );
                }
            }
            break;
        }
    }
}

/// Inject `usdValue` into every token in the API response.
/// Handles both a single group object and an array of groups.
fn enrich_with_usd_value(data: &mut Value) {
    if let Some(arr) = data.as_array_mut() {
        for group in arr.iter_mut() {
            enrich_group_usd_value(group);
        }
    } else {
        enrich_group_usd_value(data);
    }
}

/// Compute total USD value from a balance API response.
///
/// Prefers the `usdValue` field on each token if present;
/// falls back to `balance × tokenPrice` for backward compatibility.
fn compute_total_value_usd(data: &Value) -> String {
    let mut total: f64 = 0.0;

    let groups = match data.as_array() {
        Some(arr) => arr.as_slice(),
        None => std::slice::from_ref(data),
    };

    for group in groups {
        let token_assets = group["tokenAssets"]
            .as_array()
            .or_else(|| group["assets"].as_array());
        if let Some(assets) = token_assets {
            for asset in assets {
                let usd: f64 = asset["usdValue"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| asset["usdValue"].as_f64())
                    .unwrap_or_else(|| {
                        let balance: f64 = asset["balance"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .or_else(|| asset["balance"].as_f64())
                            .unwrap_or(0.0);
                        let price: f64 = asset["tokenPrice"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .or_else(|| asset["tokenPrice"].as_f64())
                            .unwrap_or(0.0);
                        balance * price
                    });
                total += usd;
            }
        }
    }

    let normalized = if total.is_sign_negative() && total == 0.0 {
        0.0_f64
    } else {
        total
    };
    format!("{:.2}", normalized)
}

/// Sum total_value_usd across all account entries in the cache.
fn sum_cache_total(cache: &wallet_store::BalanceCacheJson) -> String {
    let total: f64 = cache
        .accounts
        .values()
        .map(|e| e.total_value_usd.parse::<f64>().unwrap_or(0.0))
        .sum();
    let normalized = if total.is_sign_negative() && total == 0.0 {
        0.0_f64
    } else {
        total
    };
    format!("{:.2}", normalized)
}

// ── cmd_balance ───────────────────────────────────────────────────────

/// onchainos wallet balance [--all] [--chain <chain>] [--token-address <addr>] [--force]
pub(super) async fn cmd_balance(
    all: bool,
    chain: Option<&str>,
    token_address: Option<&str>,
    force: bool,
) -> Result<()> {
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_balance] enter: all={}, chain={:?}, token_address={:?}, force={}",
            all, chain, token_address, force
        );
    }

    // Silently refresh the chain cache if it has expired.
    super::chain::ensure_chain_cache_fresh().await;

    let access_token = ensure_tokens_refreshed().await?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_balance] access_token_len={}",
            access_token.len()
        );
    }

    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_balance] wallets loaded: project_id={}, accounts_count={}, accounts_map_count={}, selected_account_id={}",
            wallets.project_id, wallets.accounts.len(), wallets.accounts_map.len(), wallets.selected_account_id
        );
    }

    let client = WalletApiClient::new()?;

    // ── Scenario 1: All accounts (--all) ────────────────────────────
    if all {
        let account_ids: Vec<&str> = wallets.accounts_map.keys().map(|k| k.as_str()).collect();
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=all, account_ids_count={}",
                account_ids.len()
            );
        }
        if account_ids.is_empty() {
            bail!("no wallet accounts found");
        }

        if !force {
            if let Some(cached) = wallet_store::get_batch_balance_cache(BATCH_BALANCE_TTL)? {
                let total_usd = sum_cache_total(&cached);
                if cfg!(feature = "debug-log") {
                    eprintln!(
                        "[DEBUG][cmd_balance] scenario=all, cache_hit=true, total_usd={}, cached_accounts_count={}",
                        total_usd, cached.accounts.len()
                    );
                }
                output::success(json!({
                    "totalValueUsd": total_usd,
                    "details": cached.accounts,
                }));
                return Ok(());
            }
        }
        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][cmd_balance] scenario=all, cache_hit=false, calling balance_batch");
        }

        let ids_joined = account_ids.join(",");
        let data = client
            .balance_batch(&access_token, &ids_joined)
            .await
            .map_err(format_api_error)?;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=all, balance_batch response_len={}",
                data.to_string().len()
            );
        }

        let now = chrono::Utc::now().timestamp();
        let mut entries: Vec<(String, BalanceCacheEntry)> = Vec::new();
        if let Some(arr) = data.as_array() {
            for group in arr {
                if let Some(aid) = group["accountId"].as_str() {
                    let mut account_data = Value::Array(vec![group.clone()]);
                    enrich_with_usd_value(&mut account_data);
                    let total_usd = compute_total_value_usd(&account_data);
                    entries.push((
                        aid.to_string(),
                        BalanceCacheEntry {
                            updated_at: now,
                            data: account_data,
                            total_value_usd: total_usd,
                        },
                    ));
                }
            }
        }
        wallet_store::set_batch_balance_cache(&entries)?;

        let cached = wallet_store::load_balance_cache()?;
        let total_usd = sum_cache_total(&cached);
        output::success(json!({
            "totalValueUsd": total_usd,
            "details": cached.accounts,
        }));
        return Ok(());
    }

    let account_id = resolve_active_account_id(&wallets)?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][cmd_balance] resolved account_id={}", account_id);
    }

    // ── Scenario 4: Specific token (--token-address) ────────────────
    if let Some(token_addr_str) = token_address {
        let c = match chain {
            Some(c) => c,
            None => bail!("--chain is required when using --token-address"),
        };
        let chain_entry = super::chain::get_chain_by_real_chain_index(c)
            .await?
            .ok_or_else(|| anyhow::anyhow!("unknown chain: {c}"))?;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=token-address, chain_entry={}",
                chain_entry
            );
        }
        let chain_index = chain_entry["chainIndex"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| chain_entry["chainIndex"].as_i64().map(|n| n.to_string()))
            .ok_or_else(|| anyhow::anyhow!("chain entry missing chainIndex"))?;

        let mut query: Vec<(String, String)> = vec![("accountId".into(), account_id.clone())];
        query.push(("chains".into(), chain_index.clone()));
        query.push(("tokenAddresses[0].chainIndex".into(), chain_index));
        query.push((
            "tokenAddresses[0].tokenAddress".into(),
            token_addr_str.trim().to_string(),
        ));
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=token-address, query={:?}",
                query
            );
        }

        let query_refs: Vec<(&str, &str)> = query
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut data = client
            .balance_single(&access_token, &query_refs)
            .await
            .map_err(format_api_error)?;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=token-address, balance_single response_len={}",
                data.to_string().len()
            );
        }

        enrich_with_usd_value(&mut data);
        output::success(json!({ "details": data }));
        return Ok(());
    }

    // ── Scenario 3: Chain filter (--chain, no --token-address) ──────
    if let Some(c) = chain {
        let chain_entry = super::chain::get_chain_by_real_chain_index(c)
            .await?
            .ok_or_else(|| anyhow::anyhow!("unknown chain: {c}"))?;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=chain, chain_entry={}",
                chain_entry
            );
        }
        let chain_index = chain_entry["chainIndex"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| chain_entry["chainIndex"].as_i64().map(|n| n.to_string()))
            .ok_or_else(|| anyhow::anyhow!("chain entry missing chainIndex"))?;

        let query_refs: Vec<(&str, &str)> = vec![
            ("accountId", account_id.as_str()),
            ("chains", chain_index.as_str()),
        ];
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=chain, query_refs={:?}",
                query_refs
            );
        }
        let mut data = client
            .balance_single(&access_token, &query_refs)
            .await
            .map_err(format_api_error)?;
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=chain, balance_single response_len={}",
                data.to_string().len()
            );
        }

        enrich_with_usd_value(&mut data);
        let total_usd = compute_total_value_usd(&data);
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][cmd_balance] scenario=chain, total_usd={}",
                total_usd
            );
        }
        output::success(json!({
            "totalValueUsd": total_usd,
            "details": data,
        }));
        return Ok(());
    }

    // ── Scenario 2: No flags — show current account with balance_single ──
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][cmd_balance] scenario=default (no flags)");
    }

    ensure_wallet_accounts_fresh(&client, &access_token, &mut wallets, force).await?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_balance] scenario=default, after ensure_wallet_accounts_fresh: accounts_count={}, accounts_map_count={}",
            wallets.accounts.len(), wallets.accounts_map.len()
        );
    }

    let query_refs: Vec<(&str, &str)> = vec![("accountId", account_id.as_str())];
    let mut data = client
        .balance_single(&access_token, &query_refs)
        .await
        .map_err(format_api_error)?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][cmd_balance] scenario=default, balance_single response_len={}",
            data.to_string().len()
        );
    }

    enrich_with_usd_value(&mut data);
    sort_token_assets(&mut data);
    let total_usd = compute_total_value_usd(&data);

    let account_name = wallets
        .accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .map(|a| a.account_name.as_str())
        .unwrap_or("");
    let evm_address = get_evm_address(&wallets, &account_id);
    let sol_address = get_sol_address(&wallets, &account_id);
    let account_count = wallets.accounts.len().max(wallets.accounts_map.len());
    output::success(json!({
        "totalValueUsd": total_usd,
        "accountId": account_id,
        "accountName": account_name,
        "evmAddress": evm_address,
        "solAddress": sol_address,
        "accountCount": account_count,
        "details": data,
    }));
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::wallet_store::{AccountMapEntry, AddressInfo, WalletsJson};

    fn make_balance_group(tokens: &[(&str, &str, &str)]) -> Value {
        let assets: Vec<Value> = tokens
            .iter()
            .map(|(bal, price, usd)| {
                let mut obj = json!({ "balance": bal, "tokenPrice": price });
                if !usd.is_empty() {
                    obj["usdValue"] = json!(usd);
                }
                obj
            })
            .collect();
        json!({ "tokenAssets": assets })
    }

    #[test]
    fn enrich_injects_usd_value_when_missing() {
        let mut data = Value::Array(vec![make_balance_group(&[("2.0", "100.0", "")])]);
        enrich_with_usd_value(&mut data);
        let usd = data[0]["tokenAssets"][0]["usdValue"].as_str().unwrap();
        assert_eq!(usd, "200.000000");
    }

    #[test]
    fn enrich_keeps_existing_usd_value() {
        let mut data = Value::Array(vec![make_balance_group(&[("2.0", "100.0", "999.0")])]);
        enrich_with_usd_value(&mut data);
        let usd = data[0]["tokenAssets"][0]["usdValue"].as_str().unwrap();
        assert_eq!(usd, "999.0");
    }

    #[test]
    fn enrich_handles_zero_price() {
        let mut data = Value::Array(vec![make_balance_group(&[("5.0", "0", "")])]);
        enrich_with_usd_value(&mut data);
        let usd = data[0]["tokenAssets"][0]["usdValue"].as_str().unwrap();
        assert_eq!(usd, "0.000000");
    }

    #[test]
    fn enrich_handles_single_group_not_array() {
        let mut data = make_balance_group(&[("3.0", "50.0", "")]);
        enrich_with_usd_value(&mut data);
        let usd = data["tokenAssets"][0]["usdValue"].as_str().unwrap();
        assert_eq!(usd, "150.000000");
    }

    #[test]
    fn compute_total_prefers_usd_value_field() {
        let data = Value::Array(vec![make_balance_group(&[("2.0", "100.0", "300.0")])]);
        assert_eq!(compute_total_value_usd(&data), "300.00");
    }

    #[test]
    fn compute_total_falls_back_to_balance_x_price() {
        let data = Value::Array(vec![make_balance_group(&[("2.0", "100.0", "")])]);
        assert_eq!(compute_total_value_usd(&data), "200.00");
    }

    #[test]
    fn compute_total_sums_multiple_tokens() {
        let data = Value::Array(vec![make_balance_group(&[
            ("1.0", "0.0", "100.00"),
            ("2.0", "0.0", "50.00"),
        ])]);
        assert_eq!(compute_total_value_usd(&data), "150.00");
    }

    #[test]
    fn compute_total_empty_returns_zero() {
        let data = Value::Array(vec![json!({ "tokenAssets": [] })]);
        assert_eq!(compute_total_value_usd(&data), "0.00");
    }

    #[test]
    fn enrich_multiple_groups_all_enriched() {
        let mut data = Value::Array(vec![
            make_balance_group(&[("1.0", "10.0", "")]),
            make_balance_group(&[("2.0", "20.0", "")]),
        ]);
        enrich_with_usd_value(&mut data);
        assert_eq!(
            data[0]["tokenAssets"][0]["usdValue"].as_str().unwrap(),
            "10.000000"
        );
        assert_eq!(
            data[1]["tokenAssets"][0]["usdValue"].as_str().unwrap(),
            "40.000000"
        );
    }

    #[test]
    fn enrich_null_usd_value_triggers_computation() {
        let mut data = Value::Array(vec![json!({
            "tokenAssets": [{ "balance": "5.0", "tokenPrice": "10.0", "usdValue": null }]
        })]);
        enrich_with_usd_value(&mut data);
        assert_eq!(
            data[0]["tokenAssets"][0]["usdValue"].as_str().unwrap(),
            "50.000000"
        );
    }

    #[test]
    fn enrich_uses_assets_key_fallback() {
        let mut data = Value::Array(vec![json!({
            "assets": [{ "balance": "3.0", "tokenPrice": "4.0" }]
        })]);
        enrich_with_usd_value(&mut data);
        assert_eq!(
            data[0]["assets"][0]["usdValue"].as_str().unwrap(),
            "12.000000"
        );
    }

    #[test]
    fn enrich_preserves_other_token_fields() {
        let mut data = Value::Array(vec![json!({
            "tokenAssets": [{ "balance": "1.0", "tokenPrice": "5.0", "symbol": "ETH", "chainIndex": "1" }]
        })]);
        enrich_with_usd_value(&mut data);
        let token = &data[0]["tokenAssets"][0];
        assert_eq!(token["symbol"].as_str().unwrap(), "ETH");
        assert_eq!(token["chainIndex"].as_str().unwrap(), "1");
        assert_eq!(token["usdValue"].as_str().unwrap(), "5.000000");
    }

    #[test]
    fn enrich_multiple_tokens_in_one_group() {
        let mut data = Value::Array(vec![json!({
            "tokenAssets": [
                { "balance": "1.0", "tokenPrice": "10.0" },
                { "balance": "2.0", "tokenPrice": "5.0", "usdValue": "99.0" },
                { "balance": "3.0", "tokenPrice": "3.0" },
            ]
        })]);
        enrich_with_usd_value(&mut data);
        let assets = &data[0]["tokenAssets"];
        assert_eq!(assets[0]["usdValue"].as_str().unwrap(), "10.000000");
        assert_eq!(assets[1]["usdValue"].as_str().unwrap(), "99.0");
        assert_eq!(assets[2]["usdValue"].as_str().unwrap(), "9.000000");
    }

    #[test]
    fn compute_total_sums_across_multiple_groups() {
        let data = Value::Array(vec![
            make_balance_group(&[("1.0", "0.0", "100.0")]),
            make_balance_group(&[("1.0", "0.0", "200.0")]),
        ]);
        assert_eq!(compute_total_value_usd(&data), "300.00");
    }

    #[test]
    fn compute_total_numeric_usd_value() {
        let data = Value::Array(vec![json!({
            "tokenAssets": [{ "balance": "0", "tokenPrice": "0", "usdValue": 123.45 }]
        })]);
        assert_eq!(compute_total_value_usd(&data), "123.45");
    }

    #[test]
    fn compute_total_mixed_present_and_missing() {
        let data = Value::Array(vec![json!({
            "tokenAssets": [
                { "balance": "0",  "tokenPrice": "0",  "usdValue": "100.0" },
                { "balance": "2.0","tokenPrice": "50.0" },
            ]
        })]);
        assert_eq!(compute_total_value_usd(&data), "200.00");
    }

    #[test]
    fn enrich_idempotent_double_call() {
        let mut data = Value::Array(vec![make_balance_group(&[("2.0", "5.0", "")])]);
        enrich_with_usd_value(&mut data);
        let first = data[0]["tokenAssets"][0]["usdValue"]
            .as_str()
            .unwrap()
            .to_string();
        enrich_with_usd_value(&mut data);
        let second = data[0]["tokenAssets"][0]["usdValue"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(first, second);
        assert_eq!(first, "10.000000");
    }

    // ── wallet_accounts_need_refresh ─────────────────────────────────

    fn make_wallets_with_accounts(account_ids: &[&str], map_ids: &[&str]) -> WalletsJson {
        let mut accounts_map = HashMap::new();
        for id in map_ids {
            accounts_map.insert(
                id.to_string(),
                AccountMapEntry {
                    address_list: vec![AddressInfo {
                        account_id: id.to_string(),
                        address: format!("0x{}", id),
                        chain_index: "1".to_string(),
                        chain_name: "eth".to_string(),
                        address_type: "eoa".to_string(),
                        chain_path: "/evm/1".to_string(),
                    }],
                },
            );
        }
        let accounts = account_ids
            .iter()
            .map(|id| wallet_store::AccountInfo {
                project_id: "proj".to_string(),
                account_id: id.to_string(),
                account_name: format!("Wallet {}", id),
                is_default: false,
            })
            .collect();
        WalletsJson {
            accounts,
            accounts_map,
            selected_account_id: account_ids
                .first()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    #[test]
    fn need_refresh_when_accounts_empty() {
        let w = make_wallets_with_accounts(&[], &[]);
        assert!(wallet_accounts_need_refresh(&w));
    }

    #[test]
    fn need_refresh_when_account_missing_from_map() {
        let w = make_wallets_with_accounts(&["acc-1", "acc-2"], &["acc-1"]);
        assert!(wallet_accounts_need_refresh(&w));
    }

    #[test]
    fn no_refresh_when_all_accounts_in_map() {
        let w = make_wallets_with_accounts(&["acc-1", "acc-2"], &["acc-1", "acc-2"]);
        assert!(!wallet_accounts_need_refresh(&w));
    }

    #[test]
    fn no_refresh_single_complete_account() {
        let w = make_wallets_with_accounts(&["acc-1"], &["acc-1"]);
        assert!(!wallet_accounts_need_refresh(&w));
    }

    // ── get_evm_address / get_sol_address ─────────────────────────────

    fn make_wallets_multi_chain() -> WalletsJson {
        let mut accounts_map = HashMap::new();
        accounts_map.insert(
            "acc-1".to_string(),
            AccountMapEntry {
                address_list: vec![
                    AddressInfo {
                        account_id: "acc-1".to_string(),
                        address: "0xEVM".to_string(),
                        chain_index: "1".to_string(),
                        chain_name: "eth".to_string(),
                        address_type: "eoa".to_string(),
                        chain_path: "/evm/1".to_string(),
                    },
                    AddressInfo {
                        account_id: "acc-1".to_string(),
                        address: "SolanaAddr".to_string(),
                        chain_index: "501".to_string(),
                        chain_name: "sol".to_string(),
                        address_type: "eoa".to_string(),
                        chain_path: "/sol/501".to_string(),
                    },
                ],
            },
        );
        accounts_map.insert(
            "acc-evm-only".to_string(),
            AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "acc-evm-only".to_string(),
                    address: "0xEVMOnly".to_string(),
                    chain_index: "56".to_string(),
                    chain_name: "bsc".to_string(),
                    address_type: "eoa".to_string(),
                    chain_path: "/evm/56".to_string(),
                }],
            },
        );
        WalletsJson {
            accounts_map,
            ..Default::default()
        }
    }

    #[test]
    fn get_evm_address_returns_first_evm() {
        let w = make_wallets_multi_chain();
        assert_eq!(get_evm_address(&w, "acc-1"), "0xEVM");
    }

    #[test]
    fn get_sol_address_returns_sol() {
        let w = make_wallets_multi_chain();
        assert_eq!(get_sol_address(&w, "acc-1"), "SolanaAddr");
    }

    #[test]
    fn get_sol_address_empty_when_no_sol() {
        let w = make_wallets_multi_chain();
        assert_eq!(get_sol_address(&w, "acc-evm-only"), "");
    }

    #[test]
    fn get_evm_address_empty_for_unknown_account() {
        let w = make_wallets_multi_chain();
        assert_eq!(get_evm_address(&w, "unknown"), "");
    }

    #[test]
    fn get_sol_address_empty_for_unknown_account() {
        let w = make_wallets_multi_chain();
        assert_eq!(get_sol_address(&w, "unknown"), "");
    }

    // ── sum_cache_total ───────────────────────────────────────────────

    #[test]
    fn sum_cache_total_sums_correctly() {
        use wallet_store::{BalanceCacheEntry, BalanceCacheJson};
        let mut cache = BalanceCacheJson::default();
        cache.accounts.insert(
            "acc-1".to_string(),
            BalanceCacheEntry {
                updated_at: 0,
                data: Value::Null,
                total_value_usd: "100.50".to_string(),
            },
        );
        cache.accounts.insert(
            "acc-2".to_string(),
            BalanceCacheEntry {
                updated_at: 0,
                data: Value::Null,
                total_value_usd: "50.25".to_string(),
            },
        );
        assert_eq!(sum_cache_total(&cache), "150.75");
    }

    #[test]
    fn sum_cache_total_empty_returns_zero() {
        use wallet_store::BalanceCacheJson;
        let cache = BalanceCacheJson::default();
        assert_eq!(sum_cache_total(&cache), "0.00");
    }

    #[test]
    fn sum_cache_total_skips_unparseable_values() {
        use wallet_store::{BalanceCacheEntry, BalanceCacheJson};
        let mut cache = BalanceCacheJson::default();
        cache.accounts.insert(
            "acc-ok".to_string(),
            BalanceCacheEntry {
                updated_at: 0,
                data: Value::Null,
                total_value_usd: "75.00".to_string(),
            },
        );
        cache.accounts.insert(
            "acc-bad".to_string(),
            BalanceCacheEntry {
                updated_at: 0,
                data: Value::Null,
                total_value_usd: "N/A".to_string(),
            },
        );
        assert_eq!(sum_cache_total(&cache), "75.00");
    }

    // ── sort_token_assets ───────────────────────────────────────────

    fn make_token(chain_index: &str, symbol: &str, usd: &str) -> Value {
        json!({ "chainIndex": chain_index, "symbol": symbol, "usdValue": usd })
    }

    /// Wrap tokens in the structure returned by balance_single: `[{ tokenAssets: [...] }]`
    fn wrap_tokens(tokens: Vec<Value>) -> Value {
        Value::Array(vec![json!({ "tokenAssets": tokens })])
    }

    /// Extract (chainIndex, symbol) pairs from sorted result for assertion.
    fn sorted_order(data: &Value) -> Vec<(String, String)> {
        data.as_array()
            .and_then(|arr| arr[0]["tokenAssets"].as_array())
            .unwrap()
            .iter()
            .map(|t| {
                (
                    token_chain_index(t),
                    t["symbol"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn sort_xlayer_first() {
        let mut data = wrap_tokens(vec![
            make_token("501", "SOL", "100.0"),
            make_token("196", "OKB", "10.0"),
            make_token("1", "ETH", "50.0"),
        ]);
        sort_token_assets(&mut data);
        let order = sorted_order(&data);
        assert_eq!(order[0].0, "196"); // XLayer first
        assert_eq!(order[1].0, "501"); // SOL $100
        assert_eq!(order[2].0, "1"); // ETH $50
    }

    #[test]
    fn sort_xlayer_first_even_with_zero_value() {
        let mut data = wrap_tokens(vec![
            make_token("1", "ETH", "1000.0"),
            make_token("196", "OKB", "0"),
        ]);
        sort_token_assets(&mut data);
        let order = sorted_order(&data);
        assert_eq!(order[0].0, "196");
    }

    #[test]
    fn sort_by_chain_total_usd_descending() {
        let mut data = wrap_tokens(vec![
            make_token("56", "BNB", "50.0"),
            make_token("1", "ETH", "200.0"),
            make_token("1", "USDC", "100.0"),
            make_token("56", "USDT", "30.0"),
        ]);
        sort_token_assets(&mut data);
        let order = sorted_order(&data);
        // Chain 1 total = $300, Chain 56 total = $80
        assert_eq!(order[0], ("1".into(), "ETH".into()));
        assert_eq!(order[1], ("1".into(), "USDC".into()));
        assert_eq!(order[2], ("56".into(), "BNB".into()));
        assert_eq!(order[3], ("56".into(), "USDT".into()));
    }

    #[test]
    fn sort_within_same_chain_by_usd_desc() {
        let mut data = wrap_tokens(vec![
            make_token("501", "USDC", "10.0"),
            make_token("501", "SOL", "500.0"),
            make_token("501", "BONK", "1.0"),
        ]);
        sort_token_assets(&mut data);
        let order = sorted_order(&data);
        assert_eq!(order[0].1, "SOL");
        assert_eq!(order[1].1, "USDC");
        assert_eq!(order[2].1, "BONK");
    }

    #[test]
    fn sort_realistic_mixed_chains() {
        // Mimics the actual API response structure
        let mut data = wrap_tokens(vec![
            make_token("501", "SOL", "0.61"),
            make_token("196", "OKB", "0.40"),
            make_token("1", "ETH", "0.36"),
            make_token("56", "BNB", "0.34"),
            make_token("8453", "ETH", "0.23"),
            make_token("501", "CORGI", "0.11"),
            make_token("501", "USDC", "0.02"),
        ]);
        sort_token_assets(&mut data);
        let order = sorted_order(&data);
        // XLayer first
        assert_eq!(order[0], ("196".into(), "OKB".into()));
        // Solana next (total 0.61+0.11+0.02 = 0.74)
        assert_eq!(order[1].0, "501");
        assert_eq!(order[1].1, "SOL");
        assert_eq!(order[2].0, "501");
        assert_eq!(order[3].0, "501");
        // Then ETH chain (0.36), BNB (0.34), Base (0.23)
        assert_eq!(order[4], ("1".into(), "ETH".into()));
        assert_eq!(order[5], ("56".into(), "BNB".into()));
        assert_eq!(order[6], ("8453".into(), "ETH".into()));
    }

    #[test]
    fn sort_empty_token_assets_no_panic() {
        let mut data = wrap_tokens(vec![]);
        sort_token_assets(&mut data);
        assert_eq!(data[0]["tokenAssets"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn sort_non_array_data_no_panic() {
        let mut data = json!({ "tokenAssets": [] });
        sort_token_assets(&mut data);
    }
}
