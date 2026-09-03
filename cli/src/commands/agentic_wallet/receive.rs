//! Read-only wallet receive orchestration.
//!
//! This module owns the CLI facts for active funding: current account,
//! wallet-supported chains, token candidates, receive address, and Common QR.
//! User-facing flow and copy stay in `skills/_shared/funding.md` and the shared
//! output templates.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::client::ApiClient;
use crate::funding::{build_funding_bundle, FundingBundle};
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, WalletsJson};

use super::account::resolve_active_account_id;
use super::auth::ensure_tokens_refreshed;
use super::balance::ensure_wallet_accounts_fresh;

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
            let bundle = build_funding_bundle(&wallets, &profile.chain_index, None)?;
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
                    "scene": "receive_token_empty",
                    "phase": "funding",
                    "decision": "blocked",
                    "reason": "token_not_found",
                    "nextAction": [],
                    "payload": { "query": query },
                    "query": query,
                    "list": [],
                }));
            } else if candidates.len() == 1 {
                let candidate = &candidates[0];
                let chain_index = candidate["chainIndex"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("token search result missing chainIndex"))?;
                let bundle = build_funding_bundle(&wallets, chain_index, None)?;
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

/// Always refresh account/address facts for receive. A deposit address is a
/// funds-loss boundary, so the command must not repeat a conversationally or
/// locally stale address when the backend can provide the current one.
async fn load_current_wallets() -> Result<WalletsJson> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let mut client = WalletApiClient::new()?;
    ensure_wallet_accounts_fresh(&mut client, &access_token, &mut wallets, true).await?;
    Ok(wallets)
}

fn receive_address_value(bundle: &FundingBundle, token: Option<&Value>) -> Value {
    let mut value = json!({
        "scene": "receive_address",
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
        "accountName": bundle.target.account_name,
        "chainIndex": bundle.target.chain_index,
        "chainName": bundle.target.chain_name,
        "receiveAddress": bundle.target.receive_address,
        "sameNetworkRequired": bundle.target.same_network_required,
        "gasFree": bundle.target.gas_free,
        "qr": bundle.qr,
    });
    if let Some(token) = token {
        for key in [
            "tokenName",
            "tokenSymbol",
            "networkName",
            "tokenContractAddress",
            "contractAddressDisplay",
        ] {
            value[key] = token.get(key).cloned().unwrap_or(Value::Null);
            value["payload"][key] = value[key].clone();
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
        "scene": "receive_addresses",
        "phase": "funding",
        "decision": "ready",
        "reason": "receive_addresses_ready",
        "nextAction": [
            { "id": "specify_funding_chain", "recommend": true },
            { "id": "search_receive_token", "recommend": false }
        ],
        "payload": payload,
        "accountName": account_name,
        "evmAddress": evm_address,
        "evmQr": evm_qr,
        "xLayerAddress": x_layer_address,
        "solanaAddress": solana_address,
        "bitcoinAddress": bitcoin_address,
        "suiAddress": sui_address,
    }))
}

fn is_evm_receive_address(chain_index: &str) -> bool {
    !matches!(chain_index, "0" | "5" | "195" | "501" | "607" | "784")
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
                "contractAddressDisplay": contract_address_display(&contract),
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
        let command = format!(
            "onchainos wallet receive --token {} --cursor {}",
            shell_arg(query),
            shell_arg(&cursor)
        );
        actions.push(json!({
            "id": "more_receive_tokens",
            "recommend": false,
            "params": { "query": query, "cursor": cursor, "command": command }
        }));
        json!({ "limit": 10, "nextCursor": cursor, "next": { "command": command } })
    } else {
        json!({ "limit": 10, "nextCursor": Value::Null })
    };
    json!({
        "scene": "receive_token_selection",
        "phase": "funding",
        "decision": "requires_user_input",
        "reason": "token_selection_required",
        "nextAction": actions,
        "payload": { "query": query, "list": candidates, "pagination": pagination },
        "query": query,
        "list": candidates,
        "pagination": pagination,
        "selectionRequired": true,
    })
}

fn value_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|number| number.to_string()))
        .or_else(|| value.as_u64().map(|number| number.to_string()))
}

fn contract_address_display(address: &str) -> String {
    if address.is_empty() {
        return "主币，无 CA".to_string();
    }
    let chars: Vec<char> = address.chars().collect();
    if chars.len() <= 10 {
        return address.to_string();
    }
    format!(
        "{}...{}",
        chars[..6].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

fn shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
        assert_eq!(value["scene"], "receive_addresses");
        assert_eq!(value["evmAddress"], "0xEvm");
        assert!(value["evmQr"].is_object());
        assert_eq!(value["xLayerAddress"], "0xXLayerDifferent");
        assert_eq!(value["solanaAddress"], "SolanaAddress");
        assert!(value.get("solanaQr").is_none());
        assert!(value.get("bitcoinQr").is_none());
        assert!(value.get("suiQr").is_none());
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
        assert_eq!(candidates[0]["contractAddressDisplay"], "0x1234...cdef");
        assert_eq!(candidates[1]["networkName"], "X Layer");
        assert_eq!(candidates[1]["contractAddressDisplay"], "主币，无 CA");

        let selection = selection_value("USDT", candidates);
        assert_eq!(selection["decision"], "requires_user_input");
        assert!(selection["pagination"]["nextCursor"].is_null());
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
        assert_eq!(selection["pagination"]["nextCursor"], "cursor-10");
        assert_eq!(
            selection["nextAction"].as_array().unwrap().last().unwrap()["id"],
            "more_receive_tokens"
        );
    }

    #[test]
    fn shell_arg_prevents_query_command_substitution() {
        assert_eq!(shell_arg("a'b$(touch x)"), "'a'\"'\"'b$(touch x)'");
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
