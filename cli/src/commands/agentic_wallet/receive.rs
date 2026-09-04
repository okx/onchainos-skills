//! Read-only wallet receive orchestration.
//!
//! This module owns the CLI facts for active funding: current account,
//! wallet-supported chains, token candidates, receive address, and Common QR.
//! User-facing flow and copy stay in
//! `skills/okx-agentic-wallet/references/funding.md` and its output templates.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::client::ApiClient;
use crate::funding::{resolve_funding_target, FundingBundle};
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, WalletsJson};

use super::account::resolve_active_account_id;
use super::auth::ensure_tokens_refreshed;
use super::balance::refresh_wallet_accounts_strict;

const RECEIVE_TOKEN_PAGE_LIMIT: &str = "10";

pub(super) async fn cmd_receive(
    chain: Option<&str>,
    token: Option<&str>,
    cursor: Option<&str>,
) -> Result<()> {
    match (chain, token) {
        (Some(chain), None) => {
            let profile = super::chain_profile::resolve(chain).await?;
            let wallets = load_current_wallets().await?;
            let bundle = funding_bundle_from_loaded_wallets(&wallets, &profile.chain_index)?;
            output::success(receive_address_value(&bundle, None));
        }
        (None, Some(query)) => {
            let query = query.trim();
            if query.is_empty() {
                bail!("Parameter --token cannot be empty");
            }
            let wallets = load_current_wallets().await?;
            let chains = super::chain::get_all_chains().await?;
            let search_chains = supported_chain_indices(&chains);
            if search_chains.is_empty() {
                bail!("wallet receive could not resolve the supported-chain search scope");
            }
            let chain_names = supported_chain_names(&chains);
            let mut client = ApiClient::new_async(None).await?;
            let raw = crate::commands::token::fetch_search(
                &mut client,
                query,
                &search_chains,
                Some(RECEIVE_TOKEN_PAGE_LIMIT),
                cursor,
                None,
            )
            .await?;
            let candidates = normalize_candidates(&raw, &chain_names);
            if candidates.is_empty() {
                output::success(json!({
                    "phase": "funding",
                    "decision": "blocked",
                    "reason": "token_not_found",
                    "nextAction": [],
                    "payload": { "query": query },
                }));
            } else if candidates.len() == 1 {
                let candidate = &candidates[0];
                let chain_index = candidate["chainIndex"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("token search result missing chainIndex"))?;
                let bundle = funding_bundle_from_loaded_wallets(&wallets, chain_index)?;
                output::success(receive_address_value(&bundle, Some(candidate)));
            } else {
                output::success(selection_value(query, candidates));
            }
        }
        (None, None) => {
            let wallets = load_current_wallets().await?;
            output::success(generic_receive_value(&wallets)?);
        }
        (Some(_), Some(_)) => unreachable!("clap rejects --chain with --token"),
    }
    Ok(())
}

fn funding_bundle_from_loaded_wallets(
    wallets: &WalletsJson,
    chain_index: &str,
) -> Result<FundingBundle> {
    let target = resolve_funding_target(wallets, chain_index)?;
    let qr = crate::qr::build_qr_output(&target.receive_address, None);
    Ok(FundingBundle { target, qr })
}

/// Always refresh account/address facts for receive. A deposit address is a
/// funds-loss boundary, so the command must not repeat a conversationally or
/// locally stale address when the backend can provide the current one.
async fn load_current_wallets() -> Result<WalletsJson> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let mut client = WalletApiClient::new()?;
    refresh_wallet_accounts_strict(&mut client, &access_token, &mut wallets).await?;
    Ok(wallets)
}

fn receive_address_value(bundle: &FundingBundle, token: Option<&Value>) -> Value {
    let mut value = json!({
        "phase": "funding",
        "decision": "ready",
        "reason": "funding_target_ready",
        "nextAction": [],
        "payload": {
            "accountName": bundle.target.account_name,
            "chainIndex": bundle.target.chain_index,
            "chainName": bundle.target.chain_name,
            "receiveAddress": bundle.target.receive_address,
            "sameNetworkRequired": bundle.target.same_network_required,
            "gasFree": bundle.target.gas_free,
            "qr": bundle.qr,
        },
    });
    if let Some(token) = token {
        for key in [
            "tokenName",
            "tokenSymbol",
            "networkName",
            "tokenContractAddress",
        ] {
            value["payload"][key] = token.get(key).cloned().unwrap_or(Value::Null);
        }
    }
    value
}

fn generic_receive_value(wallets: &WalletsJson) -> Result<Value> {
    let account_id = resolve_active_account_id(wallets)?;
    let entry = wallets
        .accounts_map
        .get(&account_id)
        .ok_or_else(|| anyhow::anyhow!("account not found"))?;
    let account_name = wallets
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)
        .map(|account| account.account_name.clone())
        .unwrap_or_default();

    let find_exact = |indices: &[&str]| {
        entry
            .address_list
            .iter()
            .find(|address| {
                indices.contains(&address.chain_index.as_str()) && !address.address.is_empty()
            })
            .map(|address| address.address.clone())
    };
    let evm_address = entry
        .address_list
        .iter()
        .find(|address| {
            address.chain_index != "196"
                && is_evm_receive_address(&address.chain_index)
                && !address.address.is_empty()
        })
        .or_else(|| {
            entry
                .address_list
                .iter()
                .find(|address| address.chain_index == "196" && !address.address.is_empty())
        })
        .map(|address| address.address.clone());
    let x_layer_address =
        find_exact(&["196"]).filter(|address| evm_address.as_deref() != Some(address.as_str()));
    let solana_address = find_exact(&["501"]);
    let bitcoin_address = find_exact(&["0", "5"]);
    let sui_address = find_exact(&["784"]);
    let evm_qr = evm_address
        .as_deref()
        .map(|address| crate::qr::build_qr_output(address, None));

    let payload = json!({
        "accountName": account_name,
        "evmAddress": evm_address,
        "evmQr": evm_qr,
        "xLayerAddress": x_layer_address,
        "solanaAddress": solana_address,
        "bitcoinAddress": bitcoin_address,
        "suiAddress": sui_address,
    });
    Ok(json!({
        "phase": "funding",
        "decision": "ready",
        "reason": "receive_addresses_ready",
        "nextAction": [
            { "id": "specify_funding_chain", "recommend": true, "params": {} },
            { "id": "search_receive_token", "recommend": false, "params": {} }
        ],
        "payload": payload,
    }))
}

fn is_evm_receive_address(chain_index: &str) -> bool {
    crate::chains::is_evm_chain(chain_index)
}

fn supported_chain_indices(chains: &[Value]) -> String {
    let mut seen = HashSet::new();
    chains
        .iter()
        .filter_map(|chain| value_as_string(chain.get("chainIndex")?))
        .filter(|index| !index.is_empty() && seen.insert(index.clone()))
        .collect::<Vec<_>>()
        .join(",")
}

fn supported_chain_names(chains: &[Value]) -> HashMap<String, String> {
    chains
        .iter()
        .filter_map(|chain| {
            let index = value_as_string(chain.get("chainIndex")?)?;
            let name = ["showName", "chainName"]
                .iter()
                .find_map(|key| chain.get(*key).and_then(Value::as_str))
                .filter(|name| !name.is_empty())?;
            Some((index, name.to_string()))
        })
        .collect()
}

fn normalize_candidates(raw: &Value, chain_names: &HashMap<String, String>) -> Vec<Value> {
    let list = raw
        .as_array()
        .or_else(|| raw.get("list").and_then(Value::as_array))
        .or_else(|| raw.get("items").and_then(Value::as_array))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    list.iter()
        .take(10)
        .enumerate()
        .filter_map(|(index, candidate)| {
            let chain_index = value_as_string(candidate.get("chainIndex")?)?;
            if chain_index.is_empty() {
                return None;
            }
            let contract = candidate
                .get("tokenContractAddress")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let network_name = chain_names
                .get(&chain_index)
                .cloned()
                .unwrap_or_else(|| crate::chains::chain_display_name(&chain_index).to_string());
            Some(json!({
                "sequence": index + 1,
                "tokenName": candidate.get("tokenName").cloned().unwrap_or(Value::Null),
                "tokenSymbol": candidate.get("tokenSymbol").cloned().unwrap_or(Value::Null),
                "chainIndex": chain_index,
                "networkName": network_name,
                "tokenContractAddress": contract,
                "cursor": candidate.get("cursor").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect()
}

fn selection_value(query: &str, candidates: Vec<Value>) -> Value {
    let next_cursor = (candidates.len() == 10)
        .then(|| {
            candidates
                .last()
                .and_then(|candidate| candidate.get("cursor"))
                .and_then(value_as_string)
                .filter(|cursor| !cursor.is_empty())
        })
        .flatten();
    let mut actions: Vec<Value> = candidates
        .iter()
        .map(|candidate| {
            json!({
                "id": "select_receive_token",
                "recommend": false,
                "params": {
                    "sequence": candidate["sequence"],
                    "chainIndex": candidate["chainIndex"],
                    "tokenContractAddress": candidate["tokenContractAddress"],
                }
            })
        })
        .collect();
    let pagination = if let Some(cursor) = next_cursor {
        actions.push(json!({
            "id": "more_receive_tokens",
            "recommend": false,
            "params": { "query": query, "cursor": cursor }
        }));
        json!({ "limit": 10, "nextCursor": cursor })
    } else {
        json!({ "limit": 10, "nextCursor": Value::Null })
    };
    json!({
        "phase": "funding",
        "decision": "requires_user_input",
        "reason": "token_selection_required",
        "nextAction": actions,
        "payload": { "query": query, "list": candidates, "pagination": pagination },
    })
}

fn value_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet_store::{AccountInfo, AccountMapEntry, AddressInfo};

    fn address(chain_index: &str, value: &str) -> AddressInfo {
        AddressInfo {
            account_id: "account-1".into(),
            address: value.into(),
            chain_index: chain_index.into(),
            chain_name: String::new(),
            address_type: String::new(),
            chain_path: String::new(),
        }
    }

    fn wallets(addresses: Vec<AddressInfo>) -> WalletsJson {
        WalletsJson {
            selected_account_id: "account-1".into(),
            accounts: vec![AccountInfo {
                project_id: "project-1".into(),
                account_id: "account-1".into(),
                account_name: "Trading".into(),
                is_default: true,
            }],
            accounts_map: HashMap::from([(
                "account-1".into(),
                AccountMapEntry {
                    address_list: addresses,
                },
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn generic_receive_generates_only_the_evm_qr() {
        let value = generic_receive_value(&wallets(vec![
            address("196", "0xXLayerDifferent"),
            address("1", "0xEvm"),
            address("501", "SolanaAddress"),
            address("0", "BitcoinAddress"),
            address("784", "SuiAddress"),
        ]))
        .unwrap();
        assert_eq!(value["reason"], "receive_addresses_ready");
        assert_eq!(value["payload"]["evmAddress"], "0xEvm");
        assert!(value["payload"]["evmQr"].is_object());
        assert_eq!(value["payload"]["xLayerAddress"], "0xXLayerDifferent");
        assert_eq!(value["payload"]["solanaAddress"], "SolanaAddress");
        assert!(value["payload"].get("solanaQr").is_none());
        assert!(value["payload"].get("bitcoinQr").is_none());
        assert!(value["payload"].get("suiQr").is_none());
    }

    #[test]
    fn token_candidates_keep_order_full_identity_and_hide_partial_page_cursor() {
        let raw = json!([
            {"tokenName":"Tether","tokenSymbol":"USDT","chainIndex":"1","tokenContractAddress":"0x1234567890abcdef","cursor":"8"},
            {"tokenName":"Tether","tokenSymbol":"USDT","chainIndex":196,"tokenContractAddress":"","cursor":"9"}
        ]);
        let names = HashMap::from([
            ("1".into(), "Ethereum".into()),
            ("196".into(), "X Layer".into()),
        ]);
        let candidates = normalize_candidates(&raw, &names);
        assert_eq!(candidates[0]["sequence"], 1);
        assert_eq!(candidates[0]["tokenContractAddress"], "0x1234567890abcdef");
        assert_eq!(candidates[1]["networkName"], "X Layer");
        assert_eq!(candidates[1]["tokenContractAddress"], "");

        let selection = selection_value("USDT", candidates);
        assert_eq!(selection["decision"], "requires_user_input");
        assert!(selection["payload"]["pagination"]["nextCursor"].is_null());
        assert_eq!(selection["nextAction"][0]["params"]["chainIndex"], "1");
    }

    #[test]
    fn full_token_page_exposes_only_the_last_opaque_cursor() {
        let candidates = (1..=10)
            .map(|sequence| {
                json!({
                    "sequence": sequence,
                    "chainIndex": "1",
                    "tokenContractAddress": format!("0x{sequence}"),
                    "cursor": format!("cursor-{sequence}"),
                })
            })
            .collect();
        let selection = selection_value("USDT", candidates);
        assert_eq!(selection["payload"]["pagination"]["nextCursor"], "cursor-10");
        assert_eq!(
            selection["nextAction"].as_array().unwrap().last().unwrap()["id"],
            "more_receive_tokens"
        );
        let more = selection["nextAction"].as_array().unwrap().last().unwrap();
        assert_eq!(more["params"]["query"], "USDT");
        assert_eq!(more["params"]["cursor"], "cursor-10");
        assert!(more["params"].get("command").is_none());
    }

    #[test]
    fn supported_chain_scope_is_deduplicated_and_keeps_order() {
        let chains = vec![
            json!({"chainIndex":"1"}),
            json!({"chainIndex":196}),
            json!({"chainIndex":"1"}),
        ];
        assert_eq!(supported_chain_indices(&chains), "1,196");
    }
}
