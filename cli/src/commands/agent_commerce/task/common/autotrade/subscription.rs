//! FR-3 copy-trade determination.
//!
//! Queries the subscription detail for `jobId` and decides whether auto-trade is
//! authorized. **Anti-fail-open:** `copyTrade` and `status` decisions use *exact
//! equality* to a confirmed integer — never a truthy / `as_bool` conversion. Any
//! query failure / 404 / non-Active state degrades to notify-only.

use serde_json::Value;

use super::super::network::task_api_client::TaskApiClient;
use super::{AutoTradeError, DegradeReason};

/// Confirmed "Active" subscription status code (Subscribe API doc §1.1: `1 = Active`).
const AUTOTRADE_ACTIVE_STATUS: i64 = 1;

/// The `copyTrade` flag value that means "copy-trade enabled".
const COPY_TRADE_ENABLED: i64 = 1;

/// A confirmed-active copy-trade subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSubscription {
    pub provider_agent_id: String,
}

/// Read an integer that the backend may serialize as a JSON number or a string.
fn as_int(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    v.as_str().and_then(|s| s.trim().parse::<i64>().ok())
}

/// Read a string that the backend may serialize as a JSON string or a number.
fn as_string(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    v.as_i64().map(|n| n.to_string())
}

/// Pure decision over the subscription `data` object. Exact-equality only.
pub fn decide(data: &Value) -> Result<ActiveSubscription, AutoTradeError> {
    // copyTrade must be exactly the enabled value.
    match as_int(data.get("copyTrade")) {
        Some(v) if v == COPY_TRADE_ENABLED => {}
        _ => {
            return Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive,
            ))
        }
    }
    // status must be exactly the Active code.
    match as_int(data.get("status")) {
        Some(v) if v == AUTOTRADE_ACTIVE_STATUS => {}
        _ => {
            return Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive,
            ))
        }
    }
    let provider_agent_id = as_string(data.get("providerAgentId")).unwrap_or_default();
    Ok(ActiveSubscription { provider_agent_id })
}

/// Query + decide. A query error (incl. 404) degrades to `lookup_off`; a parsed
/// non-Active / non-copy-trade subscription degrades to `subscription_not_active`.
pub async fn determine_copy_trade(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<ActiveSubscription, AutoTradeError> {
    match client.fetch_subscription(job_id, agent_id).await {
        Ok(data) => decide(&data),
        Err(_) => Err(AutoTradeError::Degrade(DegradeReason::LookupOff)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn active_copy_trade_number_form() {
        let data = json!({"copyTrade": 1, "status": 1, "providerAgentId": 1506});
        let got = decide(&data).unwrap();
        assert_eq!(got.provider_agent_id, "1506");
    }

    #[test]
    fn active_copy_trade_string_form() {
        let data = json!({"copyTrade": "1", "status": "1", "providerAgentId": "1506"});
        assert!(decide(&data).is_ok());
    }

    #[test]
    fn non_active_status_degrades() {
        let data = json!({"copyTrade": 1, "status": 3, "providerAgentId": "1"});
        assert!(matches!(
            decide(&data),
            Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive
            ))
        ));
    }

    #[test]
    fn copy_trade_not_one_degrades() {
        let data = json!({"copyTrade": 0, "status": 1, "providerAgentId": "1"});
        assert!(matches!(
            decide(&data),
            Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive
            ))
        ));
    }

    #[test]
    fn never_truthy_status_100_is_not_active() {
        let data = json!({"copyTrade": 1, "status": 100, "providerAgentId": "1"});
        assert!(matches!(
            decide(&data),
            Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive
            ))
        ));
    }

    #[test]
    fn missing_fields_degrade() {
        assert!(decide(&json!({})).is_err());
        assert!(decide(&json!({"copyTrade": 1})).is_err());
    }
}
