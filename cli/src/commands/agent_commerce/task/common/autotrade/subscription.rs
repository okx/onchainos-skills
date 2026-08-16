//! Active-subscription determination for model-routed deliveries.
//!
//! Queries the subscription detail for `jobId` and decides whether a signal may
//! enter model processing. `status` uses exact equality to `1`; lookup failure or
//! non-Active state falls back to ordinary deliverable handling.

use serde_json::Value;

use super::super::network::task_api_client::TaskApiClient;
use super::{AutoTradeError, DegradeReason};

/// Confirmed "Active" subscription status code (Subscribe API doc §1.1: `1 = Active`).
const AUTOTRADE_ACTIVE_STATUS: i64 = 1;

/// A confirmed-active subscription.
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
pub fn decide_active(data: &Value) -> Result<ActiveSubscription, AutoTradeError> {
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

async fn determine(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<ActiveSubscription, AutoTradeError> {
    match client.fetch_subscription(job_id, agent_id).await {
        Ok(data) => decide_active(&data),
        Err(_) => Err(AutoTradeError::Degrade(DegradeReason::LookupOff)),
    }
}

/// Query + decide. A query error (incl. 404) degrades to `lookup_off`; a parsed
/// non-Active subscription degrades to `subscription_not_active`.
pub async fn determine_active_delivery(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<ActiveSubscription, AutoTradeError> {
    determine(client, job_id, agent_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn active_status_number_form() {
        let data = json!({"status": 1, "providerAgentId": 1506});
        let got = decide_active(&data).unwrap();
        assert_eq!(got.provider_agent_id, "1506");
    }

    #[test]
    fn active_status_string_form() {
        let data = json!({"status": "1", "providerAgentId": "1506"});
        assert!(decide_active(&data).is_ok());
    }

    #[test]
    fn non_active_status_degrades() {
        let data = json!({"status": 3, "providerAgentId": "1"});
        assert!(matches!(
            decide_active(&data),
            Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive
            ))
        ));
    }

    #[test]
    fn server_compatibility_fields_do_not_control_active_routing() {
        let data = json!({"copyTrade": 0, "status": 1, "providerAgentId": "1"});
        assert!(decide_active(&data).is_ok());
    }

    #[test]
    fn active_delivery_uses_status_and_provider() {
        let data = json!({"status": 1, "providerAgentId": "1"});
        let got = decide_active(&data).unwrap();
        assert_eq!(got.provider_agent_id, "1");
    }

    #[test]
    fn active_delivery_allows_minimal_fields() {
        let data = json!({"status": "1", "providerAgentId": 1506});
        assert!(decide_active(&data).is_ok());
    }

    #[test]
    fn active_delivery_never_allows_non_active_subscription() {
        for status in [0, 2, 3, 100] {
            let data = json!({"status": status, "providerAgentId": "1"});
            assert!(matches!(
                decide_active(&data),
                Err(AutoTradeError::Degrade(
                    DegradeReason::SubscriptionNotActive
                ))
            ));
        }
    }

    #[test]
    fn never_truthy_status_100_is_not_active() {
        let data = json!({"status": 100, "providerAgentId": "1"});
        assert!(matches!(
            decide_active(&data),
            Err(AutoTradeError::Degrade(
                DegradeReason::SubscriptionNotActive
            ))
        ));
    }

    #[test]
    fn missing_fields_degrade() {
        assert!(decide_active(&json!({})).is_err());
        assert!(decide_active(&json!({"providerAgentId": "1"})).is_err());
    }
}
