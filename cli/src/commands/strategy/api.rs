//! HTTP wrappers for 7 strategy endpoints (5 dex limitOrder + 2 wallet SD-A).
//!
//! Uses the `*_raw` client variants so we receive the full `{code, msg, data}`
//! body — `status::check_response` reads `code` typed for the 60018 retry
//! path (no string matching). Each fn returns the unwrapped DTO.

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::Value;

use crate::client::ApiClient;

use super::status::check_response;
use super::types::{
    CancelReq, CancelResp, CreateOrderReq, ListOrdersReq, ListOrdersResp, OrderListResp,
    ReactivateReq, ReactivateResp, RegisterTeeInfoReq,
};

/// BE distinguishes Agentic JWT requests from generic Bearer via this header.
const STRATEGY_AUTH_HEADERS: &[(&str, &str)] = &[("X-Web3-Auth-Type", "1")];

/// Extract `body.data` after envelope check. `data: null` is valid (some
/// endpoints have no payload, e.g. `cancel`). Non-object body bails.
fn data_field(body: Value) -> Result<Value> {
    match body {
        Value::Object(mut map) => Ok(map.remove("data").unwrap_or(Value::Null)),
        other => bail!(
            "strategy endpoint returned a non-object body — got: {}",
            serde_json::to_string(&other).unwrap_or_default()
        ),
    }
}

// ── DEX limit-order endpoints ──

/// POST `createOrder`. Caller drives SD-A retry on 60018 — see handlers.rs.
pub async fn create_order(
    client: &mut ApiClient,
    req: &CreateOrderReq,
) -> Result<OrderListResp> {
    let body = serde_json::to_value(req).context("createOrder: serialise request")?;
    let resp = client
        .post_with_headers_raw(
            "/api/v1/dex/strategy/agentic/limitOrder/createOrder",
            &body,
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    serde_json::from_value(data_field(resp)?)
        .context("createOrder: data shape did not match OrderListResp")
}

/// POST `cancel`.
pub async fn cancel(client: &mut ApiClient, req: &CancelReq) -> Result<CancelResp> {
    let body = serde_json::to_value(req).context("cancel: serialise request")?;
    let resp = client
        .post_with_headers_raw(
            "/api/v1/dex/strategy/agentic/limitOrder/cancel",
            &body,
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    let data = data_field(resp)?;
    if data.is_null() {
        return Ok(CancelResp::default());
    }
    serde_json::from_value(data).context("cancel: data shape did not match CancelResp")
}

/// POST `getOpenOrder`. BE returns `{cursor, dataList, hasNext}` under `data`.
pub async fn get_open_order(
    client: &mut ApiClient,
    req: &ListOrdersReq,
) -> Result<ListOrdersResp> {
    let body = serde_json::to_value(req).context("getOpenOrder: serialise request")?;
    let resp = client
        .post_with_headers_raw(
            "/api/v1/dex/strategy/agentic/limitOrder/getOpenOrder",
            &body,
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    let data = data_field(resp)?;
    if data.is_null() {
        return Ok(ListOrdersResp::default());
    }
    serde_json::from_value(data).context("getOpenOrder: response did not match ListOrdersResp")
}

/// GET `openOrderDetail`. `order_id` is Long — pass as string to avoid precision loss.
pub async fn open_order_detail(
    client: &mut ApiClient,
    account_id: &str,
    order_id: &str,
    strategy_mode: i32,
) -> Result<OrderListResp> {
    let mode_str = strategy_mode.to_string();
    let query: [(&str, &str); 3] = [
        ("accountId", account_id),
        ("orderId", order_id),
        ("strategyMode", mode_str.as_str()),
    ];
    let resp = client
        .get_with_headers_raw(
            "/api/v1/dex/strategy/agentic/limitOrder/openOrderDetail",
            &query,
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    serde_json::from_value(data_field(resp)?)
        .context("openOrderDetail: data shape did not match OrderListResp")
}

/// POST `reactivate`. BE-confirmed 2026-05-12: returns `{successIds, failIds}`.
/// Cancel-style `{updateNum}` fallback kept as defensive shim.
pub async fn reactivate(
    client: &mut ApiClient,
    req: &ReactivateReq,
) -> Result<ReactivateResp> {
    let body = serde_json::to_value(req).context("reactivate: serialise request")?;
    let resp = client
        .post_with_headers_raw(
            "/api/v1/dex/strategy/agentic/limitOrder/reactivate",
            &body,
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    let data = data_field(resp)?;

    if let Some(obj) = data.as_object() {
        if obj.contains_key("successIds") || obj.contains_key("failIds") {
            return serde_json::from_value(data)
                .context("reactivate: data shape did not match ReactivateResp");
        }
        if let Some(update_num) = obj.get("updateNum").and_then(|v| v.as_i64()) {
            return Ok(ReactivateResp {
                success_ids: if update_num > 0 {
                    req.order_ids.clone()
                } else {
                    Vec::new()
                },
                fail_ids: if update_num > 0 {
                    Vec::new()
                } else {
                    req.order_ids.clone()
                },
            });
        }
    }

    Ok(ReactivateResp::default())
}

// ── Wallet priapi (SD-A) ──

/// GET `getAttestDocHex`. BE-confirmed 2026-05-12: always single-element `["hex..."]`.
pub async fn request_attest_doc_hex_from_sa(client: &mut ApiClient) -> Result<String> {
    let resp = client
        .get_with_headers_raw(
            "/priapi/v5/wallet/agentic/strategy/getAttestDocHex",
            &[],
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    let data = data_field(resp)?;
    let arr = data.as_array().ok_or_else(|| {
        anyhow!(
            "getAttestDocHex: response not an array — got: {}",
            serde_json::to_string(&data).unwrap_or_default()
        )
    })?;
    let first = arr
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("getAttestDocHex: array is empty"))?;
    Ok(first.to_string())
}

/// POST `registerTeeInfo`. Failure aborts (no retry, per tech-design §5.1).
pub async fn register_tee_info(
    client: &mut ApiClient,
    req: &RegisterTeeInfoReq,
) -> Result<()> {
    let body: Value =
        serde_json::to_value(req).context("registerTeeInfo: serialise request")?;
    let resp = client
        .post_with_headers_raw(
            "/priapi/v5/wallet/agentic/strategy/registerTeeInfo",
            &body,
            Some(STRATEGY_AUTH_HEADERS),
        )
        .await?;
    check_response(&resp)?;
    Ok(())
}
