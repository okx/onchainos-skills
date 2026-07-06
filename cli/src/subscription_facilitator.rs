//! Buyer-direct, read-only facilitator endpoints for `period`.
//!
//! The only facilitator calls the buyer makes directly; every write is
//! Seller-driven. Both reads are unauthenticated (IP rate-limited). Base path
//! `/api/v6/pay/x402`.
//!
//! - [`allowance_status`] — inputs for assembling a `PermitSingle`.
//! - [`my_subscriptions`] — the buyer's own subscriptions, for the
//!   `my-subscriptions` command and reconciling the local subId cache.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::subscription_types::{AllowanceStatus, BuyerSubscriptionListResp};
use crate::wallet_api::WalletApiClient;

const BASE: &str = "/api/v6/pay/x402";

/// If the envelope `data` is a non-empty array, return its first element;
/// otherwise return it as-is.
fn unwrap_first(data: Value) -> Value {
    match data {
        Value::Array(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    }
}

/// `GET /buyers/{buyer}/allowance-status?token=..&chainIndex=..`
///
/// Not cached — re-read immediately before signing each `PermitSingle` to
/// avoid over-commit / nonce races.
pub async fn allowance_status(
    buyer: &str,
    token: &str,
    chain_index: &str,
) -> Result<AllowanceStatus> {
    let mut client = WalletApiClient::new()?;
    let path =
        format!("{BASE}/buyers/{buyer}/allowance-status?token={token}&chainIndex={chain_index}");
    let data = client
        .get_no_okheaders(&path)
        .await
        .context("allowance-status query failed")?;
    serde_json::from_value(unwrap_first(data)).context("parse allowance-status response")
}

/// `GET /buyers/{buyer}/subscriptions?limit=..&offset=..`
///
/// `limit` is clamped to `1..=100`. Returns the buyer's subscriptions,
/// newest first.
pub async fn my_subscriptions(
    buyer: &str,
    limit: u32,
    offset: u32,
) -> Result<BuyerSubscriptionListResp> {
    let mut client = WalletApiClient::new()?;
    let limit = limit.clamp(1, 100);
    let path = format!("{BASE}/buyers/{buyer}/subscriptions?limit={limit}&offset={offset}");
    let data = client
        .get_no_okheaders(&path)
        .await
        .context("my-subscriptions query failed")?;
    parse_subscription_list(data)
}

/// Tolerate the three shapes the envelope `data` might take:
/// `{subscriptions:[...]}`, `[{subscriptions:[...]}]`, or a bare `[item, ...]`.
fn parse_subscription_list(data: Value) -> Result<BuyerSubscriptionListResp> {
    // Bare array → the first element decides the shape.
    if let Value::Array(items) = &data {
        let looks_like_envelope = items
            .first()
            .map(|f| f.get("subscriptions").is_some())
            .unwrap_or(false);
        if !looks_like_envelope {
            let subscriptions =
                serde_json::from_value(data).context("parse subscriptions array")?;
            return Ok(BuyerSubscriptionListResp { subscriptions });
        }
    }
    serde_json::from_value(unwrap_first(data)).context("parse my-subscriptions response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unwrap_first_takes_array_head() {
        assert_eq!(unwrap_first(json!([{"a": 1}, {"a": 2}])), json!({"a": 1}));
        assert_eq!(unwrap_first(json!({"a": 1})), json!({"a": 1}));
        assert_eq!(unwrap_first(json!([])), json!([]));
    }

    #[test]
    fn parse_subscription_list_handles_envelope_object() {
        let r = parse_subscription_list(json!({"subscriptions": [{"subId": "0x1"}]})).unwrap();
        assert_eq!(r.subscriptions.len(), 1);
        assert_eq!(r.subscriptions[0].sub_id, "0x1");
    }

    #[test]
    fn parse_subscription_list_handles_wrapped_envelope() {
        let r = parse_subscription_list(json!([{"subscriptions": [{"subId": "0x2"}]}])).unwrap();
        assert_eq!(r.subscriptions[0].sub_id, "0x2");
    }

    #[test]
    fn parse_subscription_list_handles_bare_item_array() {
        let r = parse_subscription_list(json!([{"subId": "0x3"}, {"subId": "0x4"}])).unwrap();
        assert_eq!(r.subscriptions.len(), 2);
        assert_eq!(r.subscriptions[1].sub_id, "0x4");
    }
}
