//! Unified, read-only subscription-task listing for the current User identity.
//!
//! The backend exposes independent Active and Ended pages. This command presents
//! them as one logical stream, ordered Active then Ended, and owns the opaque
//! cursor needed to continue that stream. Existing `my-tasks` behavior remains
//! unchanged for compatibility.

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::subscription_ops::enrich_buyer_subscription_page;
use super::{content, device_routing};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::query as common_query;
use crate::commands::agent_commerce::task::common::AGENT_ROLE_USER;

const SUBSCRIPTION_MY_PATH: &str = "/priapi/v1/aieco/task/subscribe/my";
const CURSOR_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CursorStage {
    Active,
    Ended,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct SubscriptionCursor {
    version: u8,
    stage: CursorStage,
    page: u32,
    offset: usize,
    page_size: u32,
    active_count: u64,
    ended_count: u64,
}

#[derive(Debug)]
struct SubscriptionPage {
    items: Vec<Value>,
    page: u32,
    page_size: u32,
    total: u64,
    has_next: bool,
}

fn encode_cursor(cursor: &SubscriptionCursor) -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(cursor)?))
}

fn decode_cursor(raw: &str) -> Result<SubscriptionCursor> {
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| anyhow!("invalid subscription cursor"))?;
    let cursor: SubscriptionCursor =
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("invalid subscription cursor"))?;
    if cursor.version != CURSOR_VERSION || cursor.page == 0 || cursor.page_size == 0 {
        bail!("invalid subscription cursor");
    }
    Ok(cursor)
}

fn subscription_path(page: u32, page_size: u32, status_type: u8) -> String {
    format!("{SUBSCRIPTION_MY_PATH}?page={page}&pageSize={page_size}&statusType={status_type}")
}

fn parse_page(value: Value, stage: CursorStage) -> Result<SubscriptionPage> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("subscription page must be a JSON object"))?;
    let total = object
        .get("total")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("subscription page is missing numeric total"))?;
    let page = object
        .get("page")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow!("subscription page is missing numeric page"))?;
    let page_size = object
        .get("pageSize")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow!("subscription page is missing numeric pageSize"))?;
    let mut items = object
        .get("list")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("subscription page is missing list array"))?;
    let list_status = match stage {
        CursorStage::Active => "active",
        CursorStage::Ended => "ended",
    };
    for item in &mut items {
        if let Some(object) = item.as_object_mut() {
            object.insert(
                "listStatus".to_string(),
                Value::String(list_status.to_string()),
            );
            add_display_fields(object, stage);
        }
    }
    Ok(SubscriptionPage {
        items,
        page,
        page_size,
        total,
        has_next: u64::from(page).saturating_mul(u64::from(page_size)) < total,
    })
}

fn add_display_fields(object: &mut serde_json::Map<String, Value>, stage: CursorStage) {
    let amount = object
        .get("serviceTokenAmount")
        .and_then(Value::as_str)
        .unwrap_or_default();
    object.insert("feeLabel".to_string(), Value::String(amount.to_string()));

    let auto_renew = object.get("autoRenew").and_then(Value::as_i64);
    object.insert(
        "autoRenewLabel".to_string(),
        Value::String(
            match auto_renew {
                Some(1) => "Enabled",
                Some(0) => "Disabled",
                _ => "—",
            }
            .to_string(),
        ),
    );

    let billing_period = if object.get("trialType").and_then(Value::as_i64) == Some(1) {
        "Trial Period".to_string()
    } else {
        object
            .get("periodIndex")
            .and_then(Value::as_i64)
            .filter(|period| *period > 0)
            .map(|period| format!("Billing Period {period}"))
            .unwrap_or_else(|| "—".to_string())
    };
    object.insert(
        "billingPeriodLabel".to_string(),
        Value::String(billing_period),
    );

    let next_charge = if stage == CursorStage::Active && auto_renew == Some(1) {
        content::fmt_epoch(object.get("subEndTime").and_then(Value::as_i64))
    } else {
        None
    };
    object.insert(
        "nextChargeAt".to_string(),
        next_charge.map(Value::String).unwrap_or(Value::Null),
    );

    let no_receivers = object
        .get("deviceList")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    object.insert(
        "hasNoReceivingDevices".to_string(),
        Value::Bool(no_receivers),
    );
}

fn attach_device_receipts(output: &mut Value, device_snapshot: Option<&Value>) {
    let Some(payload) = output.get_mut("payload").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(devices) = device_snapshot
        .and_then(|snapshot| snapshot.get("list"))
        .and_then(Value::as_array)
    else {
        payload.insert("deviceDataAvailable".to_string(), Value::Bool(false));
        return;
    };

    payload.insert("deviceDataAvailable".to_string(), Value::Bool(true));
    payload.insert("devices".to_string(), Value::Array(devices.clone()));
    let Some(items) = payload.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        let configured = object.get("deviceList").and_then(Value::as_array);
        let default_all = object.get("deviceList").is_none_or(Value::is_null);
        let receipts = devices
            .iter()
            .map(|device| {
                let device_id = device
                    .get("deviceId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let receives = default_all
                    || configured.is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(device_id)));
                json!({
                    "deviceId": device_id,
                    "deviceName": device.get("deviceName").cloned().unwrap_or(Value::Null),
                    "isThisDevice": device.get("isThisDevice").cloned().unwrap_or(Value::Bool(false)),
                    "receives": receives
                })
            })
            .collect();
        object.insert("deviceReceipts".to_string(), Value::Array(receipts));
    }
}

async fn fetch_page(
    agent_id: &str,
    stage: CursorStage,
    page: u32,
    page_size: u32,
) -> Result<SubscriptionPage> {
    let status_type = match stage {
        CursorStage::Active => 1,
        CursorStage::Ended => 2,
    };
    let mut client = TaskApiClient::new();
    let raw = client
        .get_with_agent_id(&subscription_path(page, page_size, status_type), agent_id)
        .await
        .map_err(|error| anyhow!("failed to fetch subscription tasks: {error}"))?;
    parse_page(enrich_buyer_subscription_page(raw, agent_id)?, stage)
}

fn identity_required() -> Value {
    json!({
        "phase": "identity",
        "decision": "blocked",
        "reason": "user_identity_required",
        "nextAction": [{
            "id": "register_user_identity",
            "recommend": true,
            "params": {"role": "user"}
        }],
        "payload": {}
    })
}

fn ready_output(
    items: Vec<Value>,
    cursor: Option<SubscriptionCursor>,
    page_size: u32,
    active_count: u64,
    ended_count: u64,
) -> Result<Value> {
    let next_cursor = cursor.map(|cursor| encode_cursor(&cursor)).transpose()?;
    let next_actions = subscription_actions(&items, next_cursor.as_deref(), page_size);
    Ok(json!({
        "phase": "subscription_browsing",
        "decision": "ready",
        "reason": "subscription_list_loaded",
        "nextAction": next_actions,
        "payload": {
            "items": items,
            "nextCursor": next_cursor,
            "pageSize": page_size,
            "summary": {
                "activeCount": active_count,
                "endedCount": ended_count
            }
        }
    }))
}

fn subscription_actions(items: &[Value], next_cursor: Option<&str>, page_size: u32) -> Vec<Value> {
    let all_job_ids = items
        .iter()
        .filter_map(|item| item.get("jobId").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let active_job_ids = items
        .iter()
        .filter(|item| item.get("listStatus").and_then(Value::as_str) == Some("active"))
        .filter_map(|item| item.get("jobId").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let mut actions = Vec::new();
    if !active_job_ids.is_empty() {
        actions.push(json!({
            "id": "manage_subscription_devices",
            "actionLabel": "Adjust receiving devices",
            "recommend": true,
            "params": {"allowedJobIds": active_job_ids}
        }));
    }
    if !all_job_ids.is_empty() {
        actions.push(json!({
            "id": "view_subscription_detail",
            "actionLabel": "View subscription details",
            "recommend": actions.is_empty(),
            "params": {"allowedJobIds": all_job_ids}
        }));
    }
    if !active_job_ids.is_empty() {
        actions.push(json!({
            "id": "cancel_subscription",
            "actionLabel": "Cancel subscription",
            "recommend": false,
            "params": {"allowedJobIds": active_job_ids}
        }));
    }
    if let Some(cursor) = next_cursor {
        actions.push(json!({
            "id": "next_subscription_page",
            "actionLabel": "View next page",
            "recommend": actions.is_empty(),
            "params": {"cursor": cursor, "pageSize": page_size}
        }));
    }
    actions
}

fn next_from_active(
    page: &SubscriptionPage,
    active_count: u64,
    ended_count: u64,
) -> Option<SubscriptionCursor> {
    if page.has_next {
        Some(SubscriptionCursor {
            version: CURSOR_VERSION,
            stage: CursorStage::Active,
            page: page.page + 1,
            offset: 0,
            page_size: page.page_size,
            active_count,
            ended_count,
        })
    } else if ended_count > 0 {
        Some(SubscriptionCursor {
            version: CURSOR_VERSION,
            stage: CursorStage::Ended,
            page: 1,
            offset: 0,
            page_size: page.page_size,
            active_count,
            ended_count,
        })
    } else {
        None
    }
}

fn next_from_ended(
    page: &SubscriptionPage,
    consumed: usize,
    active_count: u64,
    ended_count: u64,
) -> Option<SubscriptionCursor> {
    if consumed < page.items.len() {
        return Some(SubscriptionCursor {
            version: CURSOR_VERSION,
            stage: CursorStage::Ended,
            page: page.page,
            offset: consumed,
            page_size: page.page_size,
            active_count,
            ended_count,
        });
    }
    if page.has_next {
        return Some(SubscriptionCursor {
            version: CURSOR_VERSION,
            stage: CursorStage::Ended,
            page: page.page + 1,
            offset: 0,
            page_size: page.page_size,
            active_count,
            ended_count,
        });
    }
    None
}

fn append_ended(items: &mut Vec<Value>, ended: &SubscriptionPage, page_size: u32) -> usize {
    let remaining = (page_size as usize).saturating_sub(items.len());
    let take = remaining.min(ended.items.len());
    items.extend(ended.items.iter().take(take).cloned());
    take
}

async fn initial_page(agent_id: &str, page_size: u32) -> Result<Value> {
    let (active, ended) = tokio::try_join!(
        fetch_page(agent_id, CursorStage::Active, 1, page_size),
        fetch_page(agent_id, CursorStage::Ended, 1, page_size),
    )?;
    let active_count = active.total;
    let ended_count = ended.total;
    let mut items = active.items.clone();
    items.truncate(page_size as usize);
    let cursor = if active.has_next {
        Some(SubscriptionCursor {
            version: CURSOR_VERSION,
            stage: CursorStage::Active,
            page: active.page + 1,
            offset: 0,
            page_size,
            active_count,
            ended_count,
        })
    } else {
        let consumed = append_ended(&mut items, &ended, page_size);
        next_from_ended(&ended, consumed, active_count, ended_count)
    };
    ready_output(items, cursor, page_size, active_count, ended_count)
}

async fn continue_page(agent_id: &str, cursor: SubscriptionCursor) -> Result<Value> {
    let page = fetch_page(agent_id, cursor.stage, cursor.page, cursor.page_size).await?;
    let mut items = page
        .items
        .iter()
        .skip(cursor.offset)
        .cloned()
        .collect::<Vec<_>>();
    items.truncate(cursor.page_size as usize);
    let next = match cursor.stage {
        CursorStage::Active if page.has_next => {
            next_from_active(&page, cursor.active_count, cursor.ended_count)
        }
        CursorStage::Active => {
            let ended = fetch_page(agent_id, CursorStage::Ended, 1, cursor.page_size).await?;
            let consumed = append_ended(&mut items, &ended, cursor.page_size);
            next_from_ended(&ended, consumed, cursor.active_count, cursor.ended_count)
        }
        CursorStage::Ended => next_from_ended(
            &page,
            cursor.offset + items.len(),
            cursor.active_count,
            cursor.ended_count,
        ),
    };
    ready_output(
        items,
        next,
        cursor.page_size,
        cursor.active_count,
        cursor.ended_count,
    )
}

pub async fn handle_subscription_list(cursor_raw: Option<&str>, page_size: u32) -> Result<()> {
    let agent_id = common_query::resolve_agent_id("", AGENT_ROLE_USER).await;
    if agent_id.trim().is_empty() {
        crate::output::success(identity_required());
        return Ok(());
    }

    let mut output = match cursor_raw {
        Some(raw) => {
            let cursor = decode_cursor(raw)?;
            if cursor.page_size != page_size {
                bail!("--page-size must match the subscription cursor page size");
            }
            continue_page(&agent_id, cursor).await?
        }
        None => initial_page(&agent_id, page_size).await?,
    };
    let mut device_client = TaskApiClient::new();
    let device_snapshot =
        device_routing::fetch_device_list_snapshot(&mut device_client, &agent_id, 1, 100)
            .await
            .ok();
    attach_device_receipts(&mut output, device_snapshot.as_ref());
    crate::output::success(output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        stage: CursorStage,
        items: usize,
        page: u32,
        page_size: u32,
        total: u64,
    ) -> SubscriptionPage {
        SubscriptionPage {
            items: (0..items)
                .map(|n| json!({"jobId": format!("{stage:?}-{n}")}))
                .collect(),
            page,
            page_size,
            total,
            has_next: u64::from(page) * u64::from(page_size) < total,
        }
    }

    #[test]
    fn cursor_round_trip_is_stable() {
        let cursor = SubscriptionCursor {
            version: CURSOR_VERSION,
            stage: CursorStage::Ended,
            page: 2,
            offset: 3,
            page_size: 10,
            active_count: 11,
            ended_count: 12,
        };
        assert_eq!(
            decode_cursor(&encode_cursor(&cursor).unwrap()).unwrap(),
            cursor
        );
    }

    #[test]
    fn invalid_cursor_is_rejected() {
        assert!(decode_cursor("not-a-cursor").is_err());
    }

    #[test]
    fn final_active_page_fills_from_ended_page() {
        let active = page(CursorStage::Active, 2, 1, 3, 2);
        let ended = page(CursorStage::Ended, 3, 1, 3, 3);
        let mut items = active.items.clone();
        let consumed = append_ended(&mut items, &ended, 3);
        assert_eq!(items.len(), 3);
        let next = next_from_ended(&ended, consumed, 2, 3).unwrap();
        assert_eq!(next.stage, CursorStage::Ended);
        assert_eq!(next.offset, 1);
    }

    #[test]
    fn active_page_continues_before_ended() {
        let active = page(CursorStage::Active, 3, 1, 3, 4);
        let next = next_from_active(&active, 4, 2).unwrap();
        assert_eq!(next.stage, CursorStage::Active);
        assert_eq!(next.page, 2);
    }

    #[test]
    fn identity_block_uses_the_shared_action_envelope() {
        let output = identity_required();
        assert_eq!(output["phase"], "identity");
        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["nextAction"][0]["id"], "register_user_identity");
    }

    #[test]
    fn display_fields_follow_active_subscription_contract() {
        let mut item = serde_json::Map::from_iter([
            ("serviceTokenAmount".to_string(), json!("10")),
            ("autoRenew".to_string(), json!(1)),
            ("trialType".to_string(), json!(0)),
            ("periodIndex".to_string(), json!(2)),
            ("subEndTime".to_string(), json!(1_700_000_000i64)),
            ("deviceList".to_string(), json!([])),
        ]);
        add_display_fields(&mut item, CursorStage::Active);
        assert_eq!(item["feeLabel"], "10");
        assert_eq!(item["autoRenewLabel"], "Enabled");
        assert_eq!(item["billingPeriodLabel"], "Billing Period 2");
        assert_eq!(item["nextChargeAt"], "2023-11-14 22:13 UTC");
        assert_eq!(item["hasNoReceivingDevices"], true);
    }

    #[test]
    fn trial_and_ended_rows_do_not_invent_a_next_charge() {
        let mut item = serde_json::Map::from_iter([
            ("serviceTokenAmount".to_string(), json!("0.1")),
            ("autoRenew".to_string(), json!(1)),
            ("trialType".to_string(), json!(1)),
            ("periodIndex".to_string(), json!(0)),
            ("subEndTime".to_string(), json!(1_700_000_000i64)),
            ("deviceList".to_string(), Value::Null),
        ]);
        add_display_fields(&mut item, CursorStage::Ended);
        assert_eq!(item["billingPeriodLabel"], "Trial Period");
        assert!(item["nextChargeAt"].is_null());
        assert_eq!(item["hasNoReceivingDevices"], false);
    }

    #[test]
    fn device_receipts_preserve_null_empty_and_allowlist_semantics() {
        let mut output = json!({"payload": {"items": [
            {"jobId":"all", "deviceList": null},
            {"jobId":"none", "deviceList": []},
            {"jobId":"one", "deviceList": ["d2"]}
        ]}});
        let devices = json!({"list": [
            {"deviceId":"d1", "deviceName":"Mac", "isThisDevice":true},
            {"deviceId":"d2", "deviceName":"Phone", "isThisDevice":false}
        ]});
        attach_device_receipts(&mut output, Some(&devices));
        assert_eq!(
            output["payload"]["items"][0]["deviceReceipts"][1]["receives"],
            true
        );
        assert_eq!(
            output["payload"]["items"][1]["deviceReceipts"][0]["receives"],
            false
        );
        assert_eq!(
            output["payload"]["items"][2]["deviceReceipts"][0]["receives"],
            false
        );
        assert_eq!(
            output["payload"]["items"][2]["deviceReceipts"][1]["receives"],
            true
        );
    }

    #[test]
    fn unavailable_device_data_has_an_explicit_degraded_signal() {
        let mut output = json!({"payload": {"items": []}});
        attach_device_receipts(&mut output, None);
        assert_eq!(output["payload"]["deviceDataAvailable"], false);
    }

    #[test]
    fn actions_follow_current_page_capabilities() {
        let items = vec![
            json!({"jobId":"active-1", "listStatus":"active"}),
            json!({"jobId":"ended-1", "listStatus":"ended"}),
        ];
        let actions = subscription_actions(&items, Some("next-cursor"), 10);
        assert_eq!(actions.len(), 4);
        assert_eq!(actions[0]["id"], "manage_subscription_devices");
        assert_eq!(actions[0]["actionLabel"], "Adjust receiving devices");
        assert_eq!(actions[1]["id"], "view_subscription_detail");
        assert_eq!(actions[1]["actionLabel"], "View subscription details");
        assert_eq!(actions[2]["id"], "cancel_subscription");
        assert_eq!(actions[2]["actionLabel"], "Cancel subscription");
        assert_eq!(actions[3]["id"], "next_subscription_page");
        assert_eq!(actions[3]["actionLabel"], "View next page");
        assert_eq!(actions[0]["params"]["allowedJobIds"], json!(["active-1"]));
        assert_eq!(
            actions[1]["params"]["allowedJobIds"],
            json!(["active-1", "ended-1"])
        );
        assert_eq!(actions[3]["params"]["cursor"], "next-cursor");
    }

    #[test]
    fn ended_only_actions_omit_mutations() {
        let actions = subscription_actions(
            &[json!({"jobId":"ended-1", "listStatus":"ended"})],
            None,
            10,
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["id"], "view_subscription_detail");
        assert_eq!(actions[0]["actionLabel"], "View subscription details");
        assert_eq!(actions[0]["recommend"], true);
    }
}
