use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;

use super::auth::{ensure_tokens_refreshed, format_api_error};

// ── history ───────────────────────────────────────────────────────────

/// onchainos wallet history
#[allow(clippy::too_many_arguments)]
pub(super) async fn cmd_history(
    account_id: Option<&str>,
    chain: Option<&str>,
    address: Option<&str>,
    begin: Option<&str>,
    end: Option<&str>,
    page_num: Option<&str>,
    limit: Option<&str>,
    order_id: Option<&str>,
    tx_hash: Option<&str>,
    uop_hash: Option<&str>,
) -> Result<()> {
    let access_token = ensure_tokens_refreshed().await?;

    let resolved_account_id = match account_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            let wallets = wallet_store::load_wallets()?
                .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
            if wallets.selected_account_id.is_empty() {
                bail!(super::common::ERR_NOT_LOGGED_IN);
            }
            wallets.selected_account_id
        }
    };

    // Resolve realChainIndex → chainIndex
    let chain_index = match chain {
        Some(id) if !id.is_empty() => {
            let entry = super::chain::get_chain_by_real_chain_index(id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("unsupported chain: {id}"))?;
            entry["chainIndex"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| entry["chainIndex"].as_i64().map(|n| n.to_string()))
                .unwrap_or_default()
        }
        _ => String::new(),
    };

    let client = WalletApiClient::new()?;

    if let Some(tx_hash_val) = tx_hash {
        // ── Detail mode: --tx-hash present → /order/detail ──
        let addr = address.unwrap_or("").to_string();
        if chain_index.is_empty() {
            bail!("--chain is required when --tx-hash is present");
        }
        if addr.is_empty() {
            bail!("--address is required when --tx-hash is present");
        }

        let mut query: Vec<(&str, &str)> = vec![
            ("accountId", &resolved_account_id),
            ("txHash", tx_hash_val),
            ("chainIndex", &chain_index),
            ("address", &addr),
        ];
        if let Some(v) = order_id {
            query.push(("orderId", v));
        }
        if let Some(v) = uop_hash {
            query.push(("uopHash", v));
        }

        let data = client
            .get_authed(
                "/priapi/v5/wallet/agentic/order/detail",
                &access_token,
                &query,
            )
            .await
            .map_err(format_api_error)?;

        let filtered = filter_detail_response(&data);
        output::success(filtered);
    } else {
        // ── List mode: no --tx-hash → /order/list ──
        let mut query: Vec<(&str, &str)> = vec![("accountId", &resolved_account_id)];
        if let Some(v) = begin {
            query.push(("begin", v));
        }
        if let Some(v) = end {
            query.push(("end", v));
        }
        if let Some(v) = page_num {
            query.push(("cursor", v));
        }
        if let Some(v) = limit {
            query.push(("limit", v));
        }
        if !chain_index.is_empty() {
            query.push(("chainIndex", &chain_index));
        }
        if let Some(v) = order_id {
            query.push(("orderId", v));
        }
        if let Some(v) = uop_hash {
            query.push(("uopHash", v));
        }

        let data = client
            .get_authed(
                "/priapi/v5/wallet/agentic/order/list",
                &access_token,
                &query,
            )
            .await
            .map_err(format_api_error)?;

        let filtered = filter_list_response(&data);
        output::success(filtered);
    }

    Ok(())
}

// ── response formatters ───────────────────────────────────────────────

/// Map numeric direction to human-readable label.
fn map_direction(raw: &Value) -> Value {
    let label = match raw.as_str().unwrap_or("") {
        "1" => "IN",
        "2" => "OUT",
        other => other,
    };
    json!(label)
}

/// Map numeric txStatus to human-readable label.
fn map_tx_status(raw: &Value) -> Value {
    let label = match raw.as_str().unwrap_or("") {
        "1" | "2" => "PENDING",
        "3" => "ERROR",
        "4" => "SUCCESS",
        "6" => "CANCELLED",
        other => other,
    };
    json!(label)
}

/// Filter detail response: data is an array, pick allowed fields from each item.
fn filter_detail_response(data: &Value) -> Value {
    let items = match data.as_array() {
        Some(arr) => arr.clone(),
        None => vec![data.clone()],
    };

    let filtered: Vec<Value> = items
        .iter()
        .map(|item| {
            let mut out = json!({
                "txHash": item["txHash"],
                "txTime": item["txTime"],
                "txStatus": map_tx_status(&item["txStatus"]),
                "failReason": item["failReason"],
                "direction": map_direction(&item["txType"]),
                "repeatTxType": item["repeatTxType"],
                "from": item["from"],
                "to": item["to"],
                "chainSymbol": item["chainSymbol"],
                "chainIndex": item["chainIndex"],
                "coinSymbol": item["coinSymbol"],
                "coinAmount": item["coinAmount"],
                "serviceCharge": item["serviceCharge"],
                "confirmedCount": item["confirmedCount"],
                "explorerUrl": item["explorerUrl"],
                "hideTxType": item["hideTxType"],
            });

            if let Some(v) = item["serviceChargeUsd"].as_str().filter(|s| !s.is_empty()) {
                out["serviceChargeUsd"] = json!(v);
            }
            if let Some(v) = item["feeRebate"].as_str().filter(|s| !s.is_empty()) {
                out["feeRebate"] = json!(v);
            }
            if let Some(v) = item["feeRebateUsd"].as_str().filter(|s| !s.is_empty()) {
                out["feeRebateUsd"] = json!(v);
            }
            if let Some(name) = item["contractInfo"]["name"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                out["contractName"] = json!(name);
            }
            if let Some(v) = item["tipsType"].as_str().filter(|s| !s.is_empty()) {
                out["tipsType"] = json!(v);
            }

            if let Some(arr) = item["input"].as_array() {
                out["input"] = json!(arr
                    .iter()
                    .map(|a| json!({
                        "name": a["name"],
                        "amount": a["amount"],
                        "direction": map_direction(&a["direction"]),
                    }))
                    .collect::<Vec<_>>());
            }
            if let Some(arr) = item["output"].as_array() {
                out["output"] = json!(arr
                    .iter()
                    .map(|a| json!({
                        "name": a["name"],
                        "amount": a["amount"],
                        "direction": map_direction(&a["direction"]),
                    }))
                    .collect::<Vec<_>>());
            }

            out
        })
        .collect();

    json!(filtered)
}

/// Filter list response: data[].orderList[] → pick allowed fields.
fn filter_list_response(data: &Value) -> Value {
    let items = match data.as_array() {
        Some(arr) => arr.clone(),
        None => vec![data.clone()],
    };

    let filtered: Vec<Value> = items
        .iter()
        .map(|item| {
            let cursor = item["cursor"].as_str().unwrap_or("").to_string();

            let order_list = item["orderList"]
                .as_array()
                .map(|orders| {
                    orders
                        .iter()
                        .map(|o| {
                            let mut out = json!({
                                "txHash": o["txHash"],
                                "txStatus": map_tx_status(&o["txStatus"]),
                                "repeatTxType": o["repeatTxType"],
                                "txTime": o["txTime"],
                                "txCreateTime": o["txCreateTime"],
                                "from": o["from"],
                                "to": o["to"],
                                "direction": map_direction(&o["direction"]),
                                "chainSymbol": o["chainSymbol"],
                                "coinSymbol": o["coinSymbol"],
                                "coinAmount": o["coinAmount"],
                                "serviceCharge": o["serviceCharge"],
                                "confirmedCount": o["confirmedCount"],
                                "hideTxType": o["hideTxType"],
                            });

                            if let Some(v) = o["failReason"].as_str().filter(|s| !s.is_empty()) {
                                out["failReason"] = json!(v);
                            }
                            if let Some(v) = o["contractName"].as_str().filter(|s| !s.is_empty()) {
                                out["contractName"] = json!(v);
                            }
                            if let Some(v) =
                                o["nftCollectionName"].as_str().filter(|s| !s.is_empty())
                            {
                                out["nftCollectionName"] = json!(v);
                            }
                            if let Some(v) = o["approveSymbol"].as_str().filter(|s| !s.is_empty()) {
                                out["approveSymbol"] = json!(v);
                            }
                            if let Some(v) = o["tipsType"].as_str().filter(|s| !s.is_empty()) {
                                out["tipsType"] = json!(v);
                            }

                            // assetChange takes precedence over outer direction/coinSymbol/coinAmount
                            if let Some(arr) = o["assetChange"].as_array() {
                                let changes: Vec<Value> = arr
                                    .iter()
                                    .map(|a| {
                                        let mut ac = json!({
                                            "coinSymbol": a["coinSymbol"],
                                            "coinAmount": a["coinAmount"],
                                            "direction": map_direction(&a["direction"]),
                                        });
                                        if let Some(v) =
                                            a["nftId"].as_str().filter(|s| !s.is_empty())
                                        {
                                            ac["nftId"] = json!(v);
                                        }
                                        if let Some(v) =
                                            a["nftImageUrl"].as_str().filter(|s| !s.is_empty())
                                        {
                                            ac["nftImageUrl"] = json!(v);
                                        }
                                        ac
                                    })
                                    .collect();

                                out["assetChange"] = json!(changes);

                                // Override outer fields with first assetChange entry
                                if let Some(first) = changes.first() {
                                    out["direction"] = first["direction"].clone();
                                    out["coinSymbol"] = first["coinSymbol"].clone();
                                    out["coinAmount"] = first["coinAmount"].clone();
                                }
                            }

                            out
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            json!({
                "cursor": cursor,
                "orderList": order_list,
            })
        })
        .collect();

    json!(filtered)
}
