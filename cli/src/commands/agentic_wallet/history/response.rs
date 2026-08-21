use serde_json::{json, Value};

/// Maps the service direction code to the shared CLI label.
fn map_direction(raw: &Value) -> Value {
    let label = match raw.as_str().unwrap_or("") {
        "1" => "IN",
        "2" => "OUT",
        other => other,
    };
    json!(label)
}

/// Maps the service transaction status to the shared CLI label.
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

/// Selects the public fields from an order detail response.
pub(super) fn filter_detail_response(data: &Value) -> Value {
    let items = match data.as_array() {
        Some(items) => items.clone(),
        None => vec![data.clone()],
    };

    let filtered: Vec<Value> = items
        .iter()
        .map(|item| {
            let mut output = json!({
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

            if let Some(value) = item["serviceChargeUsd"].as_str().filter(|s| !s.is_empty()) {
                output["serviceChargeUsd"] = json!(value);
            }
            if let Some(value) = item["feeName"].as_str().filter(|s| !s.is_empty()) {
                output["serviceChargeSymbol"] = json!(value);
            }
            if let Some(value) = item["feeDecimalNum"].as_str().filter(|s| !s.is_empty()) {
                output["serviceChargeDecimal"] = json!(value);
            }
            if let Some(value) = item["feeRebate"].as_str().filter(|s| !s.is_empty()) {
                output["feeRebate"] = json!(value);
            }
            if let Some(value) = item["feeRebateUsd"].as_str().filter(|s| !s.is_empty()) {
                output["feeRebateUsd"] = json!(value);
            }
            if let Some(value) = item["feeContainCreateAccount"].as_bool() {
                output["networkFeeLabel"] = json!(if value {
                    "Network fee and Rent fee"
                } else {
                    "Network fee"
                });
            }
            if let Some(name) = item["contractInfo"]["name"]
                .as_str()
                .filter(|value| !value.is_empty())
            {
                output["contractName"] = json!(name);
            }
            if let Some(value) = item["tipsType"].as_str().filter(|s| !s.is_empty()) {
                output["tipsType"] = json!(value);
            }

            if let Some(input) = item["input"].as_array() {
                output["input"] = json!(input
                    .iter()
                    .map(|asset| json!({
                        "name": asset["name"],
                        "amount": asset["amount"],
                        "direction": map_direction(&asset["direction"]),
                    }))
                    .collect::<Vec<_>>());
            }
            if let Some(result) = item["output"].as_array() {
                output["output"] = json!(result
                    .iter()
                    .map(|asset| json!({
                        "name": asset["name"],
                        "amount": asset["amount"],
                        "direction": map_direction(&asset["direction"]),
                    }))
                    .collect::<Vec<_>>());
            }

            output
        })
        .collect();

    json!(filtered)
}

/// Selects the public fields from an order list response.
pub(super) fn filter_list_response(data: &Value) -> Value {
    let items = match data.as_array() {
        Some(items) => items.clone(),
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
                        .map(|order| {
                            let mut output = json!({
                                "txHash": order["txHash"],
                                "txStatus": map_tx_status(&order["txStatus"]),
                                "repeatTxType": order["repeatTxType"],
                                "txTime": order["txTime"],
                                "txCreateTime": order["txCreateTime"],
                                "from": order["from"],
                                "to": order["to"],
                                "direction": map_direction(&order["direction"]),
                                "chainSymbol": order["chainSymbol"],
                                "coinSymbol": order["coinSymbol"],
                                "coinAmount": order["coinAmount"],
                                "serviceCharge": order["serviceCharge"],
                                "confirmedCount": order["confirmedCount"],
                                "hideTxType": order["hideTxType"],
                            });

                            if let Some(value) =
                                order["failReason"].as_str().filter(|s| !s.is_empty())
                            {
                                output["failReason"] = json!(value);
                            }
                            if let Some(value) =
                                order["contractName"].as_str().filter(|s| !s.is_empty())
                            {
                                output["contractName"] = json!(value);
                            }
                            if let Some(value) = order["nftCollectionName"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                            {
                                output["nftCollectionName"] = json!(value);
                            }
                            if let Some(value) =
                                order["approveSymbol"].as_str().filter(|s| !s.is_empty())
                            {
                                output["approveSymbol"] = json!(value);
                            }
                            if let Some(value) =
                                order["tipsType"].as_str().filter(|s| !s.is_empty())
                            {
                                output["tipsType"] = json!(value);
                            }

                            if let Some(asset_changes) = order["assetChange"].as_array() {
                                let changes: Vec<Value> = asset_changes
                                    .iter()
                                    .map(|asset| {
                                        let mut change = json!({
                                            "coinSymbol": asset["coinSymbol"],
                                            "coinAmount": asset["coinAmount"],
                                            "direction": map_direction(&asset["direction"]),
                                        });
                                        if let Some(value) =
                                            asset["nftId"].as_str().filter(|s| !s.is_empty())
                                        {
                                            change["nftId"] = json!(value);
                                        }
                                        if let Some(value) =
                                            asset["nftImageUrl"].as_str().filter(|s| !s.is_empty())
                                        {
                                            change["nftImageUrl"] = json!(value);
                                        }
                                        change
                                    })
                                    .collect();

                                output["assetChange"] = json!(changes);
                                if let Some(first) = changes.first() {
                                    output["direction"] = first["direction"].clone();
                                    output["coinSymbol"] = first["coinSymbol"].clone();
                                    output["coinAmount"] = first["coinAmount"].clone();
                                }
                            }

                            output
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_history_response_preserves_status_mapping() {
        assert_eq!(map_tx_status(&json!("4")), json!("SUCCESS"));
    }

    #[test]
    fn shared_detail_response_handles_bitcoin_and_sui() {
        let response = filter_detail_response(&json!([
            { "chainIndex": "0", "txHash": "btc-hash", "txStatus": "4" },
            { "chainIndex": "784", "txHash": "sui-hash", "txStatus": "2" }
        ]));

        assert_eq!(response[0]["chainIndex"], json!("0"));
        assert_eq!(response[0]["txStatus"], json!("SUCCESS"));
        assert_eq!(response[1]["chainIndex"], json!("784"));
        assert_eq!(response[1]["txStatus"], json!("PENDING"));
    }
}
