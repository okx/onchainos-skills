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
    Ok(json!({
        "phase": "subscription_browsing",
        "decision": "ready",
        "reason": "subscription_list_loaded",
        "nextAction": [],
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

    let output = match cursor_raw {
        Some(raw) => {
            let cursor = decode_cursor(raw)?;
            if cursor.page_size != page_size {
                bail!("--page-size must match the subscription cursor page size");
            }
            continue_page(&agent_id, cursor).await?
        }
        None => initial_page(&agent_id, page_size).await?,
    };
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
}
