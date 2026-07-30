//! FR-7 pct holding resolution for `dex_trade` sell+pct.
//!
//! Queries the buyer's on-chain balance by address + chainIndex + tokenAddress
//! (reusing `portfolio::fetch_token_balances`), takes the readable balance, and
//! converts `pct` → absolute via floor-8dp exact arithmetic (never over-sell).
//! Runs **only** for `dex_trade` sell+pct.

use serde_json::Value;

use super::amount::Decimal;
use super::{AutoTradeError, DegradeReason};

/// Recursively find the readable `balance` string for `token_address`.
///
/// The DEX balance response nests token objects (sometimes under `tokenAssets`);
/// walk the tree, prefer an object whose `tokenContractAddress` matches
/// (case-insensitive), else fall back to the first object carrying a `balance`.
pub fn extract_balance(data: &Value, token_address: &str) -> Option<String> {
    let mut fallback: Option<String> = None;
    let want = token_address.to_ascii_lowercase();
    fn walk(v: &Value, want: &str, fallback: &mut Option<String>) -> Option<String> {
        match v {
            Value::Object(map) => {
                if let Some(bal) = map.get("balance").and_then(read_balance) {
                    let addr = map
                        .get("tokenContractAddress")
                        .and_then(|a| a.as_str())
                        .map(|s| s.to_ascii_lowercase());
                    if addr.as_deref() == Some(want) {
                        return Some(bal);
                    }
                    fallback.get_or_insert(bal);
                }
                for child in map.values() {
                    if let Some(found) = walk(child, want, fallback) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Array(arr) => {
                for child in arr {
                    if let Some(found) = walk(child, want, fallback) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }
    walk(data, &want, &mut fallback).or(fallback)
}

/// `balance` may be a JSON string or a number.
fn read_balance(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        if s.is_empty() {
            return None;
        }
        return Some(s.to_string());
    }
    if v.is_number() {
        return Some(v.to_string());
    }
    None
}

/// Pure conversion: readable balance string + pct → absolute sell amount, mapping
/// each failure to its stable degrade reason.
fn resolve_from_balance(balance: Option<String>, pct: &Decimal) -> Result<Decimal, AutoTradeError> {
    let balance = balance.ok_or(AutoTradeError::Degrade(DegradeReason::HoldingUnavailable))?;
    let holding = Decimal::parse(&balance)
        .map_err(|_| AutoTradeError::Degrade(DegradeReason::PctHoldingFail))?;
    if holding.is_zero() {
        return Err(AutoTradeError::Degrade(DegradeReason::HoldingTooSmall));
    }
    let absolute = Decimal::pct_to_absolute(&holding, pct)
        .map_err(|_| AutoTradeError::Degrade(DegradeReason::PctHoldingFail))?;
    if absolute.is_zero() {
        // Dust: floored to 0 at 8dp.
        return Err(AutoTradeError::Degrade(DegradeReason::HoldingTooSmall));
    }
    Ok(absolute)
}

/// Query the buyer's balance and resolve the absolute sell amount for `pct`.
pub async fn resolve_pct_holding(
    address: &str,
    chain_index: &str,
    token_address: &str,
    pct: &Decimal,
) -> Result<Decimal, AutoTradeError> {
    let mut client = crate::client::ApiClient::new_async(None)
        .await
        .map_err(|_| AutoTradeError::Degrade(DegradeReason::HoldingUnavailable))?;
    let tokens = format!("{chain_index}:{token_address}");
    // exclude_risk = "1" (include all) so a risk-flagged holding is still counted.
    let data =
        crate::commands::portfolio::fetch_token_balances(&mut client, address, &tokens, Some("1"))
            .await
            .map_err(|_| AutoTradeError::Degrade(DegradeReason::HoldingUnavailable))?;
    let balance = extract_balance(&data, token_address);
    resolve_from_balance(balance, pct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pct(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    #[test]
    fn extract_balance_prefers_matching_address() {
        let data = json!([{
            "tokenAssets": [
                {"tokenContractAddress": "0xAAA", "balance": "5"},
                {"tokenContractAddress": "0xBBB", "balance": "400.8"}
            ]
        }]);
        assert_eq!(extract_balance(&data, "0xbbb").as_deref(), Some("400.8"));
    }

    #[test]
    fn extract_balance_falls_back_to_first() {
        let data = json!([{"balance": "12.5"}]);
        assert_eq!(extract_balance(&data, "0xzzz").as_deref(), Some("12.5"));
    }

    #[test]
    fn extract_balance_none_when_absent() {
        let data = json!({"code": "0", "data": []});
        assert_eq!(extract_balance(&data, "0xabc"), None);
    }

    #[test]
    fn resolve_holding_success_floors_8dp() {
        // 400.8 × 25% = 100.2
        let got = resolve_from_balance(Some("400.8".to_string()), &pct("25")).unwrap();
        assert_eq!(got.to_plain_string(), "100.2");
    }

    #[test]
    fn resolve_holding_unavailable() {
        assert!(matches!(
            resolve_from_balance(None, &pct("25")),
            Err(AutoTradeError::Degrade(DegradeReason::HoldingUnavailable))
        ));
    }

    #[test]
    fn resolve_holding_parse_fail() {
        assert!(matches!(
            resolve_from_balance(Some("not-a-number".to_string()), &pct("25")),
            Err(AutoTradeError::Degrade(DegradeReason::PctHoldingFail))
        ));
    }

    #[test]
    fn resolve_holding_zero_balance_too_small() {
        assert!(matches!(
            resolve_from_balance(Some("0".to_string()), &pct("25")),
            Err(AutoTradeError::Degrade(DegradeReason::HoldingTooSmall))
        ));
    }

    #[test]
    fn resolve_holding_dust_floors_to_too_small() {
        // 0.0000000001 × 25% = 0.000000000025 → floor 8dp = 0 → too small
        assert!(matches!(
            resolve_from_balance(Some("0.0000000001".to_string()), &pct("25")),
            Err(AutoTradeError::Degrade(DegradeReason::HoldingTooSmall))
        ));
    }
}
