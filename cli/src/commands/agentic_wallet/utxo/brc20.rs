//! Queries and selects transferable BRC-20 carrier UTXOs.

use anyhow::{bail, Result};
use num_bigint::BigUint;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::commands::agentic_wallet::shared::common::amount::{
    minimal_to_readable, parse_minimal, readable_to_minimal, value_as_decimal_string,
};
use crate::commands::agentic_wallet::shared::adapters::bitcoin::{
    api::{self, BtcApi},
    context::BtcContext,
    models::BtcOutPoint,
    validation,
};
use crate::output;

const MAX_COMBINATION_STATES: usize = 100_000;
const MAX_COMBINATION_RESULTS: usize = 3;

/// Builds template-ready BRC-20 holdings fields from the two read-only API responses.
fn build_brc20_template_values(
    token_address: &str,
    decimals: u32,
    balance: &Value,
    transferable_snapshot: &Value,
) -> Result<Value> {
    let asset = find_token_asset(balance, token_address).ok_or_else(|| {
        anyhow::anyhow!("BRC-20 balance response did not contain {token_address}")
    })?;
    let total_amount = value_as_decimal_string(&asset["balance"])
        .ok_or_else(|| anyhow::anyhow!("BRC-20 balance response is missing balance"))?;
    let transferable = parse_brc20_transferable_utxos(transferable_snapshot)?;
    let transferable_raw = transferable_snapshot
        .pointer("/brc20TransferableUtxoList/sumValueRaw")
        .and_then(value_as_decimal_string)
        .unwrap_or_else(|| {
            transferable
                .iter()
                .fold(BigUint::default(), |total, utxo| {
                    total
                        + parse_minimal(&utxo.value_raw, "transferable BRC-20 amount", true)
                            .expect("parsed transferable UTXOs have valid minimal amounts")
                })
                .to_string()
        });
    let transferable_amount = minimal_to_readable(&transferable_raw, decimals)?;
    let remaining_inscribable_amount = decimal_subtract(
        &total_amount,
        &transferable_amount,
        "BRC-20 total amount",
        "transferable BRC-20 amount",
    )?;
    let token_price = value_as_decimal_string(&asset["tokenPrice"])
        .filter(|value| parse_decimal(value, "tokenPrice").is_ok());
    let total_usd = value_as_decimal_string(&asset["usdValue"])
        .filter(|value| parse_decimal(value, "usdValue").is_ok());
    let transferable_usd = token_price
        .as_deref()
        .map(|price| {
            decimal_multiply(
                &transferable_amount,
                price,
                "transferable BRC-20 amount",
                "tokenPrice",
            )
        })
        .transpose()?;
    let remaining_inscribable_usd = token_price
        .as_deref()
        .map(|price| {
            decimal_multiply(
                &remaining_inscribable_amount,
                price,
                "remaining inscribable BRC-20 amount",
                "tokenPrice",
            )
        })
        .transpose()?;
    let ticker = token_address
        .strip_prefix("btc-brc20-")
        .unwrap_or(token_address);

    Ok(json!({
        "ticker": ticker,
        "totalAmount": total_amount,
        "transferableAmount": transferable_amount,
        "remainingInscribableAmount": remaining_inscribable_amount,
        "totalUsd": total_usd,
        "transferableUsd": transferable_usd,
        "remainingInscribableUsd": remaining_inscribable_usd,
        "tokenPrice": token_price,
        "count": transferable.len(),
        "denominations": transferable.iter().map(|utxo| utxo.build_choice(token_address, decimals).map(|choice| choice["tokenAmount"].clone())).collect::<Result<Vec<_>>>()?,
    }))
}

/// Returns the template-ready BRC-20 holdings values from the existing balance command.
pub async fn cmd_brc20_balance(token_address: &str) -> Result<()> {
    let token_address = validation::normalize_brc20_token_address(token_address)?;
    let context = BtcContext::load(None).await?;
    let mut metadata_api = BtcApi::new()?;
    let metadata = metadata_api
        .token_metadata(&context, &token_address)
        .await?;
    let decimals = api::extract_token_decimals(&metadata)?;
    let mut balance_api = BtcApi::new()?;
    let mut transferable_api = BtcApi::new()?;
    let (balance, transferable_snapshot) = tokio::try_join!(
        balance_api.brc20_balance(&context, &token_address),
        transferable_api.brc20_transferable_utxos(&context, &token_address),
    )?;
    let mut result = build_brc20_template_values(
        &token_address,
        decimals,
        &balance,
        &transferable_snapshot,
    )?;
    let result_object = result
        .as_object_mut()
        .expect("BRC-20 template values are an object");
    result_object.insert("tokenAddress".to_string(), json!(token_address));
    output::success(result);
    Ok(())
}

fn find_token_asset<'a>(value: &'a Value, token_address: &str) -> Option<&'a Value> {
    if let Some(tokens) = value.get("tokenAssets").and_then(Value::as_array) {
        if let Some(asset) = tokens.iter().find(|asset| {
            asset["tokenAddress"]
                .as_str()
                .is_some_and(|address| address.eq_ignore_ascii_case(token_address))
        }) {
            return Some(asset);
        }
    }
    match value {
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_token_asset(item, token_address)),
        Value::Object(fields) => fields
            .values()
            .find_map(|item| find_token_asset(item, token_address)),
        _ => None,
    }
}

/// Parses a non-negative plain decimal to an integer mantissa and decimal scale.
fn parse_decimal(value: &str, field: &str) -> Result<(BigUint, usize)> {
    let value = value.trim();
    let mut parts = value.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction
            .is_some_and(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        bail!("{field} must be a non-negative plain decimal");
    }
    let fraction = fraction.unwrap_or_default();
    let mantissa = format!("{integer}{fraction}")
        .parse::<BigUint>()
        .map_err(|_| anyhow::anyhow!("{field} is outside the supported decimal range"))?;
    Ok((mantissa, fraction.len()))
}

fn format_decimal(mantissa: BigUint, scale: usize) -> String {
    let mut digits = mantissa.to_string();
    if scale == 0 {
        return digits;
    }
    if digits.len() <= scale {
        digits = format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits);
    }
    let split = digits.len() - scale;
    let fraction = digits[split..].trim_end_matches('0');
    if fraction.is_empty() {
        digits[..split].to_string()
    } else {
        format!("{}.{}", &digits[..split], fraction)
    }
}

fn decimal_subtract(left: &str, right: &str, left_name: &str, right_name: &str) -> Result<String> {
    let (left_mantissa, left_scale) = parse_decimal(left, left_name)?;
    let (right_mantissa, right_scale) = parse_decimal(right, right_name)?;
    let scale = left_scale.max(right_scale);
    let left = left_mantissa * BigUint::from(10u8).pow((scale - left_scale) as u32);
    let right = right_mantissa * BigUint::from(10u8).pow((scale - right_scale) as u32);
    if right > left {
        bail!("{right_name} cannot exceed {left_name}");
    }
    Ok(format_decimal(left - right, scale))
}

fn decimal_multiply(left: &str, right: &str, left_name: &str, right_name: &str) -> Result<String> {
    let (left_mantissa, left_scale) = parse_decimal(left, left_name)?;
    let (right_mantissa, right_scale) = parse_decimal(right, right_name)?;
    Ok(format_decimal(
        left_mantissa * right_mantissa,
        left_scale + right_scale,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brc20TransferableUtxo {
    pub outpoint: BtcOutPoint,
    pub utxo_id: String,
    pub utxo_amount_raw: String,
    pub value_raw: String,
    pub offset: Option<String>,
    pub inscription_id: String,
}

impl Brc20TransferableUtxo {
    /// Builds the unsignedInfo input object for this carrier UTXO and source address.
    pub fn build_tx_param_input(&self, address: &str) -> Value {
        json!({
            "txId": self.outpoint.tx_hash,
            "vout": self.outpoint.vout_index,
            "amount": self.utxo_amount_raw,
            "address": address,
        })
    }

    /// Builds one user-selectable BRC-20 UTXO entry with readable and raw amounts.
    fn build_choice(&self, token_address: &str, decimals: u32) -> Result<Value> {
        Ok(json!({
            "selection": self.outpoint.canonical(),
            "tokenAddress": token_address,
            "tokenAmount": minimal_to_readable(&self.value_raw, decimals)?,
            "tokenAmountRaw": self.value_raw,
            "utxoAmountSats": self.utxo_amount_raw,
            "utxoId": self.utxo_id,
            "offset": self.offset,
            "inscriptionId": self.inscription_id,
        }))
    }
}

/// Queries transferable inscription UTXOs and optionally finds an exact amount combination.
pub async fn cmd_brc20_transferable(
    token_address: &str,
    readable_amount: Option<&str>,
) -> Result<()> {
    let token_address = validation::normalize_brc20_token_address(token_address)?;
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let metadata = api.token_metadata(&context, &token_address).await?;
    let decimals = api::extract_token_decimals(&metadata)?;
    let snapshot = api
        .brc20_transferable_utxos(&context, &token_address)
        .await?;
    let transferable = parse_brc20_transferable_utxos(&snapshot)?;
    let mut choices = transferable
        .iter()
        .map(|utxo| utxo.build_choice(&token_address, decimals))
        .collect::<Result<Vec<_>>>()?;
    let outpoints = transferable
        .iter()
        .map(|utxo| utxo.outpoint.clone())
        .collect::<Vec<_>>();
    let asset_records = api.brc20_utxo_asset_info(&context, &outpoints).await?;
    enrich_brc20_choice_assets(&mut choices, &asset_records);
    let selection_plan = readable_amount
        .map(|amount| build_brc20_selection_plan(&transferable, &choices, amount, decimals))
        .transpose()?;
    let sum_value_raw = snapshot
        .pointer("/brc20TransferableUtxoList/sumValueRaw")
        .and_then(value_as_decimal_string);
    let sum_value = sum_value_raw
        .as_deref()
        .map(|value| minimal_to_readable(value, decimals))
        .transpose()?;
    output::success(json!({
        "message": "Queried transferable BRC-20 inscription UTXOs. A transfer may use one or more returned selections whose token amounts exactly match the requested amount.",
        "queryType": "BRC20_TRANSFERABLE_UTXO_LIST",
        "accountId": context.account_id,
        "address": context.address.address,
        "tokenAddress": token_address,
        "count": choices.len(),
        "sumValue": sum_value,
        "sumValueRaw": sum_value_raw,
        "choices": choices,
        "selectionPlan": selection_plan,
        "brc20Transferable": snapshot,
    }));
    Ok(())
}

/// Adds non-empty BRC-20 asset lists to the matching transferable UTXO choices.
fn enrich_brc20_choice_assets(choices: &mut [Value], asset_records: &[Value]) {
    let assets_by_outpoint = asset_records
        .iter()
        .filter_map(|record| {
            let assets = record.get("assets")?.as_array()?;
            if assets.is_empty() {
                return None;
            }
            let tx_hash = record.get("txHash")?.as_str()?;
            let vout_index = record.get("voutIndex")?;
            let vout_index = vout_index
                .as_u64()
                .map(|index| index.to_string())
                .or_else(|| vout_index.as_str().map(str::to_string))?;
            Some((format!("{tx_hash}:{vout_index}"), Value::Array(assets.clone())))
        })
        .collect::<BTreeMap<_, _>>();

    for choice in choices {
        let Some(selection) = choice.get("selection").and_then(Value::as_str) else {
            continue;
        };
        let Some(assets) = assets_by_outpoint.get(selection).cloned() else {
            continue;
        };
        if let Some(fields) = choice.as_object_mut() {
            fields.insert("assets".to_string(), assets);
        }
    }
}

/// Resolves unique user-selected outpoints against the latest transferable snapshot.
pub fn select_brc20_transferable_utxos(
    snapshot: &Value,
    selections: &[String],
) -> Result<Vec<Brc20TransferableUtxo>> {
    if selections.is_empty() {
        anyhow::bail!(
            "BRC-20 transfers require at least one --brc20-outpoint selected from wallet utxo brc20-transferable"
        );
    }

    let mut available = parse_brc20_transferable_utxos(snapshot)?
        .into_iter()
        .map(|utxo| (utxo.outpoint.clone(), utxo))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut selected = Vec::with_capacity(selections.len());
    for selection in selections {
        let outpoint = BtcOutPoint::parse(selection)?;
        let canonical = outpoint.canonical();
        if !seen.insert(outpoint.clone()) {
            anyhow::bail!("BRC-20 UTXO {canonical} was selected more than once");
        }
        let utxo = available.remove(&outpoint).ok_or_else(|| {
            anyhow::anyhow!("selected BRC-20 UTXO is no longer transferable: {canonical}")
        })?;
        selected.push(utxo);
    }
    Ok(selected)
}

#[derive(Debug, PartialEq, Eq)]
enum CombinationSearch {
    Exact(Vec<Vec<usize>>),
    NoExactMatch,
    SearchLimitExceeded,
}

/// Finds up to three exact subsets, ordered by the fewest selected UTXOs.
fn find_exact_combination(
    transferable: &[Brc20TransferableUtxo],
    target: &BigUint,
) -> Result<CombinationSearch> {
    let amounts = transferable
        .iter()
        .enumerate()
        .map(|(index, utxo)| {
            parse_minimal(
                &utxo.value_raw,
                &format!("transferable UTXO {index} valueRaw"),
                false,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let exact_single_inputs = amounts
        .iter()
        .enumerate()
        .filter(|(_, amount)| *amount == target)
        .take(MAX_COMBINATION_RESULTS)
        .map(|(index, _)| vec![index])
        .collect::<Vec<_>>();
    if !exact_single_inputs.is_empty() {
        return Ok(CombinationSearch::Exact(exact_single_inputs));
    }

    let mut states = BTreeMap::from([(BigUint::default(), vec![Vec::<usize>::new()])]);
    for (index, amount) in amounts.iter().enumerate() {
        if amount > target {
            continue;
        }
        let additions = states
            .iter()
            .filter_map(|(sum, combinations)| {
                let next = sum + amount;
                (next <= *target).then(|| {
                    let candidates = combinations
                        .iter()
                        .map(|indexes| {
                            let mut candidate = indexes.clone();
                            candidate.push(index);
                            candidate
                        })
                        .collect::<Vec<_>>();
                    (next, candidates)
                })
            })
            .collect::<Vec<_>>();
        for (sum, candidates) in additions {
            if let Some(existing) = states.get_mut(&sum) {
                existing.extend(candidates);
                existing.sort_by(|left, right| {
                    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
                });
                existing.dedup();
                existing.truncate(MAX_COMBINATION_RESULTS);
            } else {
                if states.len() >= MAX_COMBINATION_STATES {
                    return Ok(states
                        .get(target)
                        .cloned()
                        .map(CombinationSearch::Exact)
                        .unwrap_or(CombinationSearch::SearchLimitExceeded));
                }
                states.insert(sum, candidates);
            }
        }
    }

    Ok(states
        .get(target)
        .cloned()
        .map(CombinationSearch::Exact)
        .unwrap_or(CombinationSearch::NoExactMatch))
}

/// Builds the amount-aware selection plan returned by the transferable query.
fn build_brc20_selection_plan(
    transferable: &[Brc20TransferableUtxo],
    choices: &[Value],
    readable_amount: &str,
    decimals: u32,
) -> Result<Value> {
    let requested_amount_raw = readable_to_minimal(readable_amount, decimals)?;
    let target = parse_minimal(&requested_amount_raw, "requested BRC-20 amount", false)?;
    let (status, combinations) = match find_exact_combination(transferable, &target)? {
        CombinationSearch::Exact(combinations) => {
            let combinations = combinations
                .iter()
                .map(|indexes| {
                    let selected_choices = indexes
                        .iter()
                        .map(|index| choices[*index].clone())
                        .collect::<Vec<_>>();
                    let selected_outpoints = indexes
                        .iter()
                        .map(|index| transferable[*index].outpoint.canonical())
                        .collect::<Vec<_>>();
                    json!({
                        "selectedCount": indexes.len(),
                        "selectedOutpoints": selected_outpoints,
                        "selectedChoices": selected_choices,
                    })
                })
                .collect::<Vec<_>>();
            ("EXACT_MATCH", combinations)
        }
        CombinationSearch::NoExactMatch => ("NO_EXACT_MATCH", Vec::new()),
        CombinationSearch::SearchLimitExceeded => ("SEARCH_LIMIT_EXCEEDED", Vec::new()),
    };
    Ok(json!({
        "status": status,
        "requestedAmount": readable_amount,
        "requestedAmountRaw": requested_amount_raw,
        "maxCombinations": MAX_COMBINATION_RESULTS,
        "searchStateLimit": MAX_COMBINATION_STATES,
        "combinationCount": combinations.len(),
        "combinations": combinations,
    }))
}

/// Parses transferable BRC-20 response items into validated carrier UTXOs.
fn parse_brc20_transferable_utxos(snapshot: &Value) -> Result<Vec<Brc20TransferableUtxo>> {
    let Some(items) = snapshot
        .pointer("/brc20TransferableUtxoList/utxos")
        .or_else(|| snapshot.get("utxos"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let tx_hash = item
                .get("txHash")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("transferable UTXO {index} is missing txHash"))?;
            let vout_index = item
                .get("voutIndex")
                .and_then(value_as_decimal_string)
                .ok_or_else(|| anyhow::anyhow!("transferable UTXO {index} is missing voutIndex"))?;
            let outpoint = BtcOutPoint::parse(&format!("{tx_hash}:{vout_index}"))?;
            let utxo_amount_raw = read_required_raw_field(item, "utxoAmountRaw", index)?;
            let value_raw = read_required_raw_field(item, "valueRaw", index)?;
            Ok(Brc20TransferableUtxo {
                outpoint,
                utxo_id: item
                    .get("utxoId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                utxo_amount_raw,
                value_raw,
                offset: item.get("offset").and_then(value_as_decimal_string),
                inscription_id: item
                    .get("inscriptionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

/// Reads one required raw integer field from a transferable UTXO item.
fn read_required_raw_field(item: &Value, field: &str, index: usize) -> Result<String> {
    item.get(field)
        .and_then(value_as_decimal_string)
        .ok_or_else(|| anyhow::anyhow!("transferable UTXO {index} is missing {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transferable_snapshot() -> Value {
        json!({
            "brc20TransferableUtxoList": {
                "sumValueRaw": "3000000000000000000",
                "count": 2,
                "utxos": [{
                    "txHash": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "voutIndex": 2,
                    "utxoId": "utxo-1",
                    "utxoAmountRaw": "546",
                    "valueRaw": "1000000000000000000",
                    "offset": "0",
                    "inscriptionId": "inscription-1"
                }, {
                    "txHash": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                    "voutIndex": 3,
                    "utxoId": "utxo-2",
                    "utxoAmountRaw": "600",
                    "valueRaw": "2000000000000000000",
                    "offset": "1",
                    "inscriptionId": "inscription-2"
                }]
            }
        })
    }

    #[test]
    fn brc20_transferable_choice_keeps_token_and_btc_amounts_distinct() {
        let tx_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let snapshot = transferable_snapshot();

        let selected = select_brc20_transferable_utxos(&snapshot, &[format!("{tx_hash}:2")])
            .unwrap()
            .remove(0);
        assert_eq!(selected.value_raw, "1000000000000000000");
        assert_eq!(selected.utxo_amount_raw, "546");
        assert_eq!(selected.build_tx_param_input("bc1pfrom")["amount"], "546");

        let choice = selected.build_choice("btc-brc20-pizza", 18).unwrap();
        assert_eq!(choice["tokenAmount"], "1");
        assert_eq!(choice["utxoAmountSats"], "546");
        assert_eq!(choice["selection"], format!("{tx_hash}:2"));
    }

    #[test]
    fn transferable_choice_only_includes_non_empty_bound_assets() {
        let bound_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let plain_hash = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let mut choices = vec![
            json!({"selection": format!("{bound_hash}:2")}),
            json!({"selection": format!("{plain_hash}:3")}),
        ];
        let asset_records = vec![
            json!({
                "txHash": bound_hash,
                "voutIndex": 2,
                "assets": [{"protocol": "BRC20", "symbol": "pizza", "readableAmount": "1"}]
            }),
            json!({"txHash": plain_hash, "voutIndex": "3", "assets": []}),
        ];

        enrich_brc20_choice_assets(&mut choices, &asset_records);

        assert_eq!(
            choices[0]["assets"][0]["symbol"].as_str(),
            Some("pizza")
        );
        assert!(choices[1].get("assets").is_none());
    }

    #[test]
    fn summary_returns_every_balance_template_value_with_exact_decimals() {
        let balance = json!([{
            "tokenAssets": [{
                "tokenAddress": "btc-brc20-pizza",
                "balance": "4.25",
                "tokenPrice": "1.2",
                "usdValue": "5.1"
            }]
        }]);
        let summary = build_brc20_template_values(
            "btc-brc20-pizza",
            18,
            &balance,
            &transferable_snapshot(),
        )
        .unwrap();

        assert_eq!(summary["ticker"], "pizza");
        assert_eq!(summary["totalAmount"], "4.25");
        assert_eq!(summary["transferableAmount"], "3");
        assert_eq!(summary["remainingInscribableAmount"], "1.25");
        assert_eq!(summary["totalUsd"], "5.1");
        assert_eq!(summary["transferableUsd"], "3.6");
        assert_eq!(summary["remainingInscribableUsd"], "1.5");
        assert_eq!(summary["count"], 2);
        assert_eq!(summary["denominations"], json!(["1", "2"]));
    }

    #[test]
    fn summary_marks_derived_usd_as_unavailable_without_a_valid_price() {
        let balance = json!([{
            "tokenAssets": [{
                "tokenAddress": "btc-brc20-pizza",
                "balance": "3"
            }]
        }]);
        let summary = build_brc20_template_values(
            "btc-brc20-pizza",
            18,
            &balance,
            &transferable_snapshot(),
        )
        .unwrap();

        assert!(summary["totalUsd"].is_null());
        assert!(summary["transferableUsd"].is_null());
        assert!(summary["remainingInscribableUsd"].is_null());
    }

    #[test]
    fn selects_multiple_transferable_utxos_in_requested_order() {
        let selections = vec![
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd:3".to_string(),
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc:2".to_string(),
        ];

        let selected =
            select_brc20_transferable_utxos(&transferable_snapshot(), &selections).unwrap();

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].value_raw, "2000000000000000000");
        assert_eq!(selected[1].value_raw, "1000000000000000000");
    }

    #[test]
    fn rejects_duplicate_transferable_utxo_selections() {
        let outpoint = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc:2";
        let error = select_brc20_transferable_utxos(
            &transferable_snapshot(),
            &[outpoint.to_string(), outpoint.to_string()],
        )
        .unwrap_err();

        assert!(error.to_string().contains("selected more than once"));
    }

    #[test]
    fn rejects_selection_missing_from_refreshed_snapshot() {
        let error = select_brc20_transferable_utxos(
            &transferable_snapshot(),
            &["eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee:0".to_string()],
        )
        .unwrap_err();

        assert!(error.to_string().contains("no longer transferable"));
    }

    #[test]
    fn selection_plan_combines_multiple_utxos_for_an_exact_amount() {
        let snapshot = transferable_snapshot();
        let transferable = parse_brc20_transferable_utxos(&snapshot).unwrap();
        let choices = transferable
            .iter()
            .map(|utxo| utxo.build_choice("btc-brc20-pizza", 18).unwrap())
            .collect::<Vec<_>>();

        let plan = build_brc20_selection_plan(&transferable, &choices, "3", 18).unwrap();

        assert_eq!(plan["status"], "EXACT_MATCH");
        assert_eq!(plan["combinationCount"], 1);
        assert_eq!(plan["combinations"][0]["selectedCount"], 2);
        assert_eq!(
            plan["combinations"][0]["selectedOutpoints"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn selection_plan_reports_when_no_exact_combination_exists() {
        let snapshot = transferable_snapshot();
        let transferable = parse_brc20_transferable_utxos(&snapshot).unwrap();
        let choices = transferable
            .iter()
            .map(|utxo| utxo.build_choice("btc-brc20-pizza", 18).unwrap())
            .collect::<Vec<_>>();

        let plan = build_brc20_selection_plan(&transferable, &choices, "4", 18).unwrap();

        assert_eq!(plan["status"], "NO_EXACT_MATCH");
        assert_eq!(plan["combinations"], json!([]));
    }

    #[test]
    fn selection_plan_returns_at_most_three_single_input_options() {
        let transferable = ["a", "b", "c", "d"]
            .iter()
            .enumerate()
            .map(|(index, digit)| Brc20TransferableUtxo {
                outpoint: BtcOutPoint::parse(&format!("{}:{index}", digit.repeat(64))).unwrap(),
                utxo_id: format!("utxo-{index}"),
                utxo_amount_raw: "546".to_string(),
                value_raw: "1000000000000000000".to_string(),
                offset: Some("0".to_string()),
                inscription_id: format!("inscription-{index}"),
            })
            .collect::<Vec<_>>();
        let choices = transferable
            .iter()
            .map(|utxo| utxo.build_choice("btc-brc20-pizza", 18).unwrap())
            .collect::<Vec<_>>();

        let plan = build_brc20_selection_plan(&transferable, &choices, "1", 18).unwrap();

        assert_eq!(plan["status"], "EXACT_MATCH");
        assert_eq!(plan["maxCombinations"], 3);
        assert_eq!(plan["combinationCount"], 3);
        assert!(plan["combinations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|combination| combination["selectedCount"] == 1));
    }

    #[test]
    fn selection_plan_caps_multi_input_combinations_at_three() {
        let transferable = ["a", "b", "c", "d"]
            .iter()
            .enumerate()
            .map(|(index, digit)| Brc20TransferableUtxo {
                outpoint: BtcOutPoint::parse(&format!("{}:{index}", digit.repeat(64))).unwrap(),
                utxo_id: format!("utxo-{index}"),
                utxo_amount_raw: "546".to_string(),
                value_raw: "1".to_string(),
                offset: Some("0".to_string()),
                inscription_id: format!("inscription-{index}"),
            })
            .collect::<Vec<_>>();
        let choices = transferable
            .iter()
            .map(|utxo| utxo.build_choice("btc-brc20-pizza", 0).unwrap())
            .collect::<Vec<_>>();

        let plan = build_brc20_selection_plan(&transferable, &choices, "2", 0).unwrap();

        assert_eq!(plan["combinationCount"], 3);
        assert!(plan["combinations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|combination| combination["selectedCount"] == 2));
    }
}
