//! Device routing for subscription messages (buyer side) — two commands:
//!
//! - `device-list`             — list the devices this agent is logged in on,
//!   with a CLI-derived local last-online time and a this-device marker.
//!   Paginates to completion (a dropped page would read as "not receiving",
//!   which is safety-relevant).
//! - `subscribe-device-update` — overwrite the receive-device list for one or
//!   more subscriptions (batch). The passed list wholly replaces the stored
//!   list; empty/omitted clears it.
//!
//! Both are backend-HTTP only — neither takes a `--chain` argument.
//!
//! Grounded pattern notes (source code wins over the architecture doc):
//! - The batch update POSTs via `post_with_identity` (JSON body). The
//!   `raw_post_with_identity` variant the arch named is for hand-rolled
//!   multipart bytes and unwraps `data` / errors on `code != "0"` exactly like
//!   `post_with_identity`, so it offers no envelope-preservation benefit here.
//! - The device-list GET uses `get_with_agent_id` (JWT + agenticId, no
//!   sessionCert): `get_with_identity` appends `?sessionCert=…`, which cannot
//!   coexist with the `?page=&pageSize=` query string (double `?`). The
//!   `/priapi/v5/wallet/agentic/agent/device-list` wallet endpoint authenticates
//!   with JWT + agenticId.

use anyhow::{anyhow, bail, Result};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::output;

use super::create::resolve_user_agent;
use super::create_subscribe::SUBSCRIBE_API_PREFIX;
use super::subscription_ops::select_subscription_agent_id;

/// Wallet device-list endpoint (userId resolved from JWT — never passed).
const DEVICE_LIST_PATH: &str = "/priapi/v5/wallet/agentic/agent/device-list";
/// Max subscriptions per batch update (client pre-validation).
const MAX_UPDATE_ITEMS: usize = 100;
/// Default page size when the caller passes `< 1`.
const DEFAULT_PAGE_SIZE: i64 = 20;
/// Hard safety cap on pagination rounds (a buggy backend must not loop forever).
const MAX_PAGES: i64 = 10_000;
/// Durable marker directory for interrupted new-device fan-out. V2 markers are
/// keyed by API environment + buyer agent id + device id and deliberately
/// survive logout, allowing a later login to finish a partially successful
/// batch set without leaking state between production and pre-release.
const PENDING_ROUTING_DIR: &str = "subscription-device-routing-pending";
const ROUTING_MARKER_VERSION: u8 = 2;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum RoutingMarkerPhase {
    Detected,
    Routing,
    Completed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RoutingMarker {
    version: u8,
    phase: RoutingMarkerPhase,
    #[serde(default)]
    remaining_job_ids: Vec<String>,
}

fn normalize_routing_scope(api_base_url: &str) -> Result<String> {
    let mut url = url::Url::parse(api_base_url)
        .map_err(|e| anyhow!("invalid API base URL for device-routing state: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("device-routing state requires an HTTP(S) API origin");
    }
    url.set_query(None);
    url.set_fragment(None);
    let normalized_path = url.path().trim_end_matches('/').to_string();
    url.set_path(&normalized_path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn pending_routing_marker_path(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
) -> Result<std::path::PathBuf> {
    if agent_id.is_empty() || device_id.is_empty() {
        bail!("cannot address device-routing state without agent and device ids");
    }
    let scope = normalize_routing_scope(api_base_url)?;
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update([0]);
    hasher.update(agent_id.as_bytes());
    hasher.update([0]);
    hasher.update(device_id.as_bytes());
    let key = format!("{:x}", hasher.finalize());
    Ok(crate::home::onchainos_home()?
        .join(PENDING_ROUTING_DIR)
        .join(format!("{key}.pending")))
}

fn read_routing_marker(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
) -> Result<Option<RoutingMarker>> {
    let path = pending_routing_marker_path(api_base_url, agent_id, device_id)?;
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| {
        anyhow!(
            "failed to read device-routing state {}: {e}",
            path.display()
        )
    })?;
    let marker: RoutingMarker = serde_json::from_slice(&bytes).map_err(|e| {
        anyhow!(
            "failed to parse device-routing state {}: {e}",
            path.display()
        )
    })?;
    if marker.version != ROUTING_MARKER_VERSION {
        bail!(
            "unsupported device-routing state version {} in {}",
            marker.version,
            path.display()
        );
    }
    Ok(Some(marker))
}

fn write_routing_marker(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
    marker: &RoutingMarker,
) -> Result<()> {
    let path = pending_routing_marker_path(api_base_url, agent_id, device_id)?;
    let bytes = serde_json::to_vec(marker)
        .map_err(|e| anyhow!("failed to serialize device-routing state: {e}"))?;
    crate::home::atomic_write(&path, &bytes, true)
}

pub(crate) fn new_device_routing_is_pending(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
) -> Result<bool> {
    Ok(matches!(
        read_routing_marker(api_base_url, agent_id, device_id)?.map(|marker| marker.phase),
        Some(RoutingMarkerPhase::Detected | RoutingMarkerPhase::Routing)
    ))
}

pub(crate) fn mark_new_device_routing_pending(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
) -> Result<()> {
    write_routing_marker(
        api_base_url,
        agent_id,
        device_id,
        &RoutingMarker {
            version: ROUTING_MARKER_VERSION,
            phase: RoutingMarkerPhase::Detected,
            remaining_job_ids: Vec::new(),
        },
    )
}

pub(crate) fn mark_new_device_routing_completed(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
) -> Result<()> {
    write_routing_marker(
        api_base_url,
        agent_id,
        device_id,
        &RoutingMarker {
            version: ROUTING_MARKER_VERSION,
            phase: RoutingMarkerPhase::Completed,
            remaining_job_ids: Vec::new(),
        },
    )
}

pub(crate) fn clear_new_device_routing_state(
    api_base_url: &str,
    agent_id: &str,
    device_id: &str,
) -> Result<()> {
    let path = pending_routing_marker_path(api_base_url, agent_id, device_id)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| {
            anyhow!(
                "failed to clear device-routing state {}: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

// ─── ms → local time helper (device lastOnlineTime is milliseconds) ─────────

/// Format a Unix-**milliseconds** timestamp to a local wall-clock string for
/// display. Mirrors `evaluator::my_stake::fmt_unix_seconds` but for ms — a
/// seconds misread lands in the wrong year. Three sentinel rules:
/// `0 → "0"`; parseable → `"%Y-%m-%d %H:%M:%S %Z"`; unparseable →
/// `"{ts_ms} (unparseable)"`.
fn fmt_unix_millis(ts_ms: i64) -> String {
    if ts_ms == 0 {
        "0".to_string()
    } else if let Some(dt) = chrono::Local.timestamp_millis_opt(ts_ms).single() {
        dt.format("%Y-%m-%d %H:%M:%S %Z").to_string()
    } else {
        format!("{ts_ms} (unparseable)")
    }
}

// ─── device-list wire + output shapes ───────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceRow {
    #[serde(default)]
    device_id: String,
    #[serde(default)]
    device_name: String,
    /// Unix **milliseconds** — never seconds.
    #[serde(default)]
    last_online_time: i64,
}

/// A decoded device page. Only `list` + `total` are consumed: the echoed
/// `page`/`pageSize` reflect the request inputs (CLI spec), not the backend's,
/// so those wire fields are intentionally not modelled here.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevicePage {
    #[serde(default)]
    list: Vec<DeviceRow>,
    #[serde(default)]
    total: i64,
}

/// CLI-derived, ready-to-print device row — the skill never re-formats it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceOut {
    device_id: String,
    device_name: String,
    last_online_time: i64,
    last_online_local: String,
    is_this_device: bool,
}

/// Decode the dual-envelope `data`: a bare object OR a single-element array
/// (production wraps it). An empty array is "no such page" ⇒ empty page.
fn decode_device_page(data: Value) -> Result<DevicePage> {
    match data {
        Value::Array(arr) => match arr.into_iter().next() {
            Some(first) => serde_json::from_value(first)
                .map_err(|e| anyhow!("failed to parse device page: {e}")),
            None => Ok(DevicePage::default()),
        },
        Value::Null => Ok(DevicePage::default()),
        other => {
            serde_json::from_value(other).map_err(|e| anyhow!("failed to parse device page: {e}"))
        }
    }
}

/// Normalize request paging inputs: `page < 1 → 1`,
/// `page_size < 1 → DEFAULT_PAGE_SIZE`. `page_size > 100` is passed through
/// (the backend returns error `81001`).
fn normalize_page_params(page: i64, page_size: i64) -> (i64, i64) {
    let start_page = if page < 1 { 1 } else { page };
    let norm_size = if page_size < 1 {
        DEFAULT_PAGE_SIZE
    } else {
        page_size
    };
    (start_page, norm_size)
}

/// Pagination stop predicate: an empty page, a short (final) page, the
/// accumulated count reaching `total`, or the hard safety cap all terminate the
/// loop. Stopping early on a dropped page would misread as "device not
/// receiving", so the caller keeps fetching until one of these holds.
fn pagination_done(
    got: i64,
    norm_size: i64,
    page_total: i64,
    acc_len: i64,
    cur: i64,
    start_page: i64,
) -> bool {
    let reached_total = page_total > 0 && acc_len >= page_total;
    got == 0 || got < norm_size || reached_total || cur - start_page >= MAX_PAGES
}

/// Display-only total: never report fewer than the rows actually aggregated (an
/// empty terminal page can echo `total: 0` even after earlier pages returned rows).
fn resolve_total(page_total: i64, acc_len: i64) -> i64 {
    page_total.max(acc_len)
}

/// Non-empty device ids from a decoded page (empty ids are dropped — a blank id
/// can never match this device and would pollute the routing set).
fn device_ids(page: DevicePage) -> Vec<String> {
    page.list
        .into_iter()
        .map(|r| r.device_id)
        .filter(|id| !id.is_empty())
        .collect()
}

/// Fetch and aggregate **all** pages, looping until `pagination_done`.
async fn fetch_all_devices(
    client: &mut TaskApiClient,
    agent_id: &str,
    page: i64,
    page_size: i64,
) -> Result<DevicePage> {
    let validated_agent_id = select_subscription_agent_id(agent_id, "")?;
    let agent_id = validated_agent_id.as_str();
    let (start_page, norm_size) = normalize_page_params(page, page_size);

    let mut acc: Vec<DeviceRow> = Vec::new();
    let total: i64;
    let mut cur = start_page;
    loop {
        let path = format!("{DEVICE_LIST_PATH}?page={cur}&pageSize={norm_size}");
        let data = client.get_with_agent_id(&path, agent_id).await?;
        let dpage = decode_device_page(data)?;
        let page_total = dpage.total;
        let got = dpage.list.len() as i64;
        acc.extend(dpage.list);

        if pagination_done(
            got,
            norm_size,
            page_total,
            acc.len() as i64,
            cur,
            start_page,
        ) {
            total = resolve_total(page_total, acc.len() as i64);
            break;
        }
        cur += 1;
    }
    Ok(DevicePage { list: acc, total })
}

/// Fetch every logged-in device id (paginated to completion). Reuse convenience
/// for `create-subscribe`'s default all-devices routing set — NOT an MCP `fetch_*`
/// delegate. The caller decides how to handle an error / empty result (degrade).
pub(crate) async fn fetch_all_device_ids(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<Vec<String>> {
    let aggregated = fetch_all_devices(client, agent_id, 1, DEFAULT_PAGE_SIZE).await?;
    Ok(device_ids(aggregated))
}

/// Resolve `create-subscribe`'s `deviceList` + `deviceRoutingDegraded` flag:
/// - fetch succeeded with ≥ 1 device ⇒ all fetched ids minus `excluded`, not degraded;
/// - fetch failed (`None`) or returned no devices ⇒ **this device only**, degraded.
///
/// An unresolved this-device id in the degrade branch yields an empty list (still
/// degraded) — the create flow must not abort.
pub(crate) fn resolve_create_device_set(
    fetched: Option<Vec<String>>,
    excluded: &[String],
    this_device_id: Option<&str>,
) -> (Vec<String>, bool) {
    match fetched {
        Some(ids) if !ids.is_empty() => {
            let kept: Vec<String> = ids
                .into_iter()
                .filter(|id| !excluded.iter().any(|e| e == id))
                .collect();
            if kept.is_empty() {
                // Every fetched device was excluded → the subscription would
                // receive nowhere. Degrade + flag rather than return a silent
                // empty set; fall back to this device unless it too was excluded.
                let fallback = this_device_id
                    .filter(|id| !excluded.iter().any(|e| e == id))
                    .map(|id| vec![id.to_string()])
                    .unwrap_or_default();
                (fallback, true)
            } else {
                (kept, false)
            }
        }
        _ => (
            this_device_id
                .map(|id| vec![id.to_string()])
                .unwrap_or_default(),
            true,
        ),
    }
}

/// Fetch the complete device-list command payload for an already-resolved
/// buyer agent. Shared with the login post-condition so login can return the
/// subscription and device snapshots atomically without shelling out to a
/// second CLI process.
pub(crate) async fn fetch_device_list_snapshot(
    client: &mut TaskApiClient,
    agent_id: &str,
    page: i64,
    page_size: i64,
) -> Result<Value> {
    let aggregated = fetch_all_devices(client, agent_id, page, page_size).await?;
    let this_id = crate::device::id::get_cached_device_id();

    let list: Vec<DeviceOut> = aggregated
        .list
        .iter()
        .map(|row| DeviceOut {
            device_id: row.device_id.clone(),
            device_name: row.device_name.clone(),
            last_online_time: row.last_online_time,
            last_online_local: fmt_unix_millis(row.last_online_time),
            is_this_device: this_id.is_some_and(|id| id == row.device_id.as_str()),
        })
        .collect();

    let (echoed_page, echoed_size) = normalize_page_params(page, page_size);

    Ok(json!({
        "list": list,
        "total": aggregated.total,
        "page": echoed_page,
        "pageSize": echoed_size,
        "thisDeviceId": this_id,
    }))
}

/// `device-list` handler — full emit. Empty page / no devices ⇒ `success` with
/// `list: []`, `total: 0` (NOT an error). Transport / endpoint-unavailable
/// (endpoint not live in production yet) propagates as `output::error` (exit 1)
/// — the degraded path is a first-class deliverable.
pub async fn handle_device_list(
    client: &mut TaskApiClient,
    page: i64,
    page_size: i64,
) -> Result<()> {
    ensure_tokens_refreshed()
        .await
        .map_err(|e| anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;
    let (agent_id, _) = resolve_user_agent().await?;
    let snapshot = fetch_device_list_snapshot(client, &agent_id, page, page_size).await?;
    output::success(snapshot);
    Ok(())
}

// ─── subscribe-device-update ────────────────────────────────────────────────

/// One subscription's overwrite target. `device_list` empty ⇒ that subscription
/// receives on no device (clear).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UpdateItem {
    job_id: String,
    #[serde(default)]
    device_list: Vec<String>,
}

/// Split a comma-separated device-id list; blanks are dropped. `None` / empty ⇒
/// empty vec (clear).
fn parse_csv_devices(csv: Option<&str>) -> Vec<String> {
    csv.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

/// Normalize Form A (`--job-id` + `--device-list` csv) and Form B (`--items`
/// JSON) into one `items` array. Form B wins when both are supplied.
fn normalize_items(
    job_id: Option<&str>,
    device_list: Option<&str>,
    items: Option<&str>,
) -> Result<Vec<UpdateItem>> {
    if let Some(items_json) = items {
        let parsed: Vec<UpdateItem> = serde_json::from_str(items_json).map_err(|e| {
            anyhow!("--items must be a JSON array of {{jobId, deviceList}} objects: {e}")
        })?;
        // Form B mirrors Form A: an empty jobId is rejected, never silently sent.
        if parsed.iter().any(|it| it.job_id.is_empty()) {
            bail!("--items entries must each carry a non-empty jobId");
        }
        Ok(parsed)
    } else {
        let job_id = job_id
            .ok_or_else(|| anyhow!("either --job-id (form A) or --items (form B) is required"))?;
        if job_id.is_empty() {
            bail!("--job-id must not be empty");
        }
        Ok(vec![UpdateItem {
            job_id: job_id.to_string(),
            device_list: parse_csv_devices(device_list),
        }])
    }
}

/// Client pre-validation: the resolved `items` array must be non-empty and
/// `len <= 100` (boundaries 0 / 1 / 100 / 101).
fn validate_items_len(len: usize) -> Result<()> {
    if len == 0 {
        bail!("no subscriptions to update: provide --job-id or a non-empty --items array");
    }
    if len > MAX_UPDATE_ITEMS {
        bail!("too many items ({len}); at most {MAX_UPDATE_ITEMS} subscriptions per batch");
    }
    Ok(())
}

fn build_items_array(items: &[UpdateItem]) -> Vec<Value> {
    items
        .iter()
        .map(|it| json!({ "jobId": it.job_id, "deviceList": it.device_list }))
        .collect()
}

/// Byte-literal request body `{ "items": [ { "jobId", "deviceList": [...] } ] }`.
fn build_update_body(items: &[UpdateItem]) -> Value {
    json!({ "items": build_items_array(items) })
}

/// Build the overwrite operations needed to make a genuinely new device receive
/// every subscription. Historical/default-all rows (`deviceList: null`) already
/// include every device and must stay null. Explicit rows (including `[]`) are
/// copied as-is and receive the new id exactly once.
fn plan_new_device_updates(subscriptions: &Value, device_id: &str) -> Result<Vec<UpdateItem>> {
    if device_id.is_empty() {
        bail!("cannot enable subscription delivery for an empty device id");
    }

    let list = subscriptions
        .get("list")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("subscription snapshot is missing its list"))?;
    let mut updates = Vec::new();

    for row in list {
        let Some(raw_devices) = row.get("deviceList") else {
            // Missing has the same historical/default-all meaning as null.
            continue;
        };
        if raw_devices.is_null() {
            continue;
        }
        let devices = raw_devices
            .as_array()
            .ok_or_else(|| anyhow!("subscription snapshot contains a malformed deviceList"))?;
        if devices
            .iter()
            .any(|value| value.as_str() == Some(device_id))
        {
            continue;
        }

        let job_id = row
            .get("jobId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                anyhow!("subscription requiring a device update is missing its jobId")
            })?;
        let mut next_devices = Vec::with_capacity(devices.len() + 1);
        for value in devices {
            let id = value
                .as_str()
                .ok_or_else(|| anyhow!("subscription snapshot contains a non-string device id"))?;
            next_devices.push(id.to_string());
        }
        next_devices.push(device_id.to_string());
        updates.push(UpdateItem {
            job_id: job_id.to_string(),
            device_list: next_devices,
        });
    }

    Ok(updates)
}

/// Reflect only backend-confirmed updates into the already-fetched snapshot.
/// On a resumed flow, jobs removed from the durable remaining set must retain
/// their fresh server state: the user may have manually opted out after an
/// earlier batch completed.
fn reflect_new_device_in_snapshot(
    subscriptions: &mut Value,
    device_id: &str,
    updated_job_ids: &[String],
) -> Result<()> {
    let list = subscriptions
        .get_mut("list")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("subscription snapshot is missing its list"))?;

    for row in list {
        let Some(obj) = row.as_object_mut() else {
            bail!("subscription snapshot contains a malformed row");
        };
        let job_id = obj.get("jobId").and_then(Value::as_str).map(str::to_string);
        let this_device_receives = match obj.get_mut("deviceList") {
            None | Some(Value::Null) => {
                // Default-all already includes the new device; preserve the mode.
                true
            }
            Some(Value::Array(devices)) => {
                if job_id
                    .as_ref()
                    .is_some_and(|id| updated_job_ids.iter().any(|updated| updated == id))
                    && !devices
                        .iter()
                        .any(|value| value.as_str() == Some(device_id))
                {
                    devices.push(Value::String(device_id.to_string()));
                }
                devices
                    .iter()
                    .any(|value| value.as_str() == Some(device_id))
            }
            Some(_) => bail!("subscription snapshot contains a malformed deviceList"),
        };
        obj.insert(
            "thisDeviceReceives".to_string(),
            Value::Bool(this_device_receives),
        );
    }
    Ok(())
}

/// Success iff the backend `data` is boolean `true`. Any other shape (object,
/// null, `"true"` string) is a failure whose raw body is echoed.
fn is_update_success(data: &Value) -> bool {
    *data == Value::Bool(true)
}

async fn post_update_items(
    client: &mut TaskApiClient,
    agent_id: &str,
    items: &[UpdateItem],
) -> Result<()> {
    let validated_agent_id = select_subscription_agent_id(agent_id, "")?;
    let agent_id = validated_agent_id.as_str();
    validate_items_len(items.len())?;
    let body = build_update_body(items);
    let path = format!("{SUBSCRIBE_API_PREFIX}/device/batchUpdate");
    let resp = client
        .post_with_identity(&path, &body, agent_id)
        .await
        .map_err(|e| anyhow!("subscribe-device-update failed: {e}"))?;

    if is_update_success(&resp) {
        Ok(())
    } else {
        bail!(
            "subscribe-device-update failed: backend did not confirm the update (data != true): {}",
            serde_json::to_string(&resp).unwrap_or_else(|_| resp.to_string())
        )
    }
}

fn select_updates_for_routing_marker(
    planned: Vec<UpdateItem>,
    marker: RoutingMarker,
) -> Result<Vec<UpdateItem>> {
    let target_job_ids = match marker.phase {
        RoutingMarkerPhase::Detected => planned
            .iter()
            .map(|item| item.job_id.clone())
            .collect::<Vec<_>>(),
        RoutingMarkerPhase::Routing => marker.remaining_job_ids,
        RoutingMarkerPhase::Completed => {
            bail!("new-device routing is already completed; refusing to rewrite subscriptions")
        }
    };
    Ok(planned
        .into_iter()
        .filter(|item| target_job_ids.iter().any(|id| id == &item.job_id))
        .collect())
}

/// Add a newly registered device to every explicitly routed subscription.
/// The batch endpoint overwrites complete lists, so this always plans from the
/// fresh snapshot and preserves every existing receiver. More than 100 tasks
/// are split into sequential backend-supported batches. The snapshot is only
/// mutated after all batches have been confirmed.
pub(crate) async fn add_new_device_to_all_subscriptions(
    client: &mut TaskApiClient,
    api_base_url: &str,
    agent_id: &str,
    subscriptions: &mut Value,
    device_id: &str,
) -> Result<usize> {
    let planned = plan_new_device_updates(subscriptions, device_id)?;
    let marker = read_routing_marker(api_base_url, agent_id, device_id)?
        .ok_or_else(|| anyhow!("new-device routing state is missing"))?;
    // A fresh snapshot is authoritative. A remaining job that no longer needs
    // an update (already contains the device, changed to default-all, or no
    // longer exists) is complete. Jobs completed by an earlier batch are not
    // re-added, so a later manual opt-out is preserved during retry.
    let updates = select_updates_for_routing_marker(planned, marker)?;
    let mut remaining_job_ids = updates
        .iter()
        .map(|item| item.job_id.clone())
        .collect::<Vec<_>>();

    write_routing_marker(
        api_base_url,
        agent_id,
        device_id,
        &RoutingMarker {
            version: ROUTING_MARKER_VERSION,
            phase: if remaining_job_ids.is_empty() {
                RoutingMarkerPhase::Completed
            } else {
                RoutingMarkerPhase::Routing
            },
            remaining_job_ids: remaining_job_ids.clone(),
        },
    )?;

    for chunk in updates.chunks(MAX_UPDATE_ITEMS) {
        post_update_items(client, agent_id, chunk).await?;
        remaining_job_ids.retain(|job_id| !chunk.iter().any(|item| &item.job_id == job_id));
        // Persist progress after every confirmed batch. Once the final batch is
        // confirmed the durable state becomes Completed before any table can be
        // rendered; cleanup failure therefore cannot trigger a future re-enable.
        write_routing_marker(
            api_base_url,
            agent_id,
            device_id,
            &RoutingMarker {
                version: ROUTING_MARKER_VERSION,
                phase: if remaining_job_ids.is_empty() {
                    RoutingMarkerPhase::Completed
                } else {
                    RoutingMarkerPhase::Routing
                },
                remaining_job_ids: remaining_job_ids.clone(),
            },
        )?;
    }
    let updated_job_ids = updates
        .iter()
        .map(|item| item.job_id.clone())
        .collect::<Vec<_>>();
    reflect_new_device_in_snapshot(subscriptions, device_id, &updated_job_ids)?;
    Ok(updates.len())
}

/// `subscribe-device-update` handler. Client-validates locally (0 / >100 items
/// send no request), then POSTs the byte-literal body and asserts `data == true`.
pub async fn handle_subscribe_device_update(
    client: &mut TaskApiClient,
    job_id: Option<&str>,
    device_list: Option<&str>,
    items: Option<&str>,
) -> Result<()> {
    // Form A (--job-id/--device-list) and Form B (--items) are mutually exclusive
    // at the clap layer, so the combination is unrepresentable here. normalize_items
    // keeps its defensive either-or error for direct (non-clap) callers.
    let normalized = normalize_items(job_id, device_list, items)?;
    validate_items_len(normalized.len())?;

    ensure_tokens_refreshed()
        .await
        .map_err(|e| anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;
    let (user_agent_id, _) = resolve_user_agent().await?;

    post_update_items(client, &user_agent_id, &normalized).await?;
    // Echo what was written so the skill re-renders without a second fetch.
    output::success(json!({ "updated": build_items_array(&normalized) }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fmt_unix_millis ──────────────────────────────────────────────────
    #[test]
    fn fmt_unix_millis_zero_sentinel() {
        assert_eq!(fmt_unix_millis(0), "0");
    }

    #[test]
    fn fmt_unix_millis_unparseable_sentinel() {
        let out = fmt_unix_millis(i64::MAX);
        assert!(
            out.contains("unparseable"),
            "expected unparseable sentinel: {out}"
        );
    }

    #[test]
    fn fmt_unix_millis_uses_milliseconds_not_seconds() {
        // 1_784_620_000_000 ms → 2026 (UTC 2026-07-19); local offsets keep the year.
        let ms = 1_784_620_000_000i64;
        let as_ms = fmt_unix_millis(ms);
        assert!(
            as_ms.contains("2026"),
            "ms must format to year 2026: {as_ms}"
        );
        // RED assertion: reading the same integer as *seconds* lands in the wrong
        // (far-future) year — proving the helper is millisecond-based.
        if let Some(dt) = chrono::Local.timestamp_opt(ms, 0).single() {
            assert_ne!(
                dt.format("%Y").to_string(),
                "2026",
                "a seconds misread must NOT resolve to 2026"
            );
        }
    }

    // ── decode_device_page: three envelope shapes ───────────────────────
    #[test]
    fn decode_bare_object() {
        let obj = json!({
            "list": [{ "deviceId": "d1", "deviceName": "Phone", "lastOnlineTime": 1_784_620_000_000i64 }],
            "total": 1, "page": 1, "pageSize": 20
        });
        let p = decode_device_page(obj).unwrap();
        assert_eq!(p.list.len(), 1);
        assert_eq!(p.list[0].device_id, "d1");
        assert_eq!(p.list[0].last_online_time, 1_784_620_000_000);
        assert_eq!(p.total, 1);
    }

    #[test]
    fn decode_single_element_array() {
        let arr = json!([{ "list": [{ "deviceId": "d2" }], "total": 1 }]);
        let p = decode_device_page(arr).unwrap();
        assert_eq!(p.list.len(), 1);
        assert_eq!(p.list[0].device_id, "d2");
    }

    #[test]
    fn decode_empty_array_is_empty_page() {
        let p = decode_device_page(json!([])).unwrap();
        assert!(p.list.is_empty());
        assert_eq!(p.total, 0);
    }

    // ── subscribe-device-update normalization + body ────────────────────
    #[test]
    fn normalize_form_a_csv() {
        let items = normalize_items(Some("0xjob"), Some("d1, d2 ,,d3"), None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].job_id, "0xjob");
        assert_eq!(items[0].device_list, vec!["d1", "d2", "d3"]);
    }

    #[test]
    fn normalize_form_a_omitted_device_list_clears() {
        let items = normalize_items(Some("0xjob"), None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].device_list.is_empty());
    }

    #[test]
    fn subscribe_device_update_items_conflicts_with_form_a_at_parse_time() {
        use clap::Parser;
        #[derive(Parser)]
        struct T {
            #[command(subcommand)]
            cmd: crate::commands::agent_commerce::task::user::TaskCommand,
        }
        // --items together with --job-id is unrepresentable (clap conflict → error).
        assert!(T::try_parse_from([
            "test",
            "subscribe-device-update",
            "--job-id",
            "0xA",
            "--items",
            r#"[{"jobId":"0xB","deviceList":["d9"]}]"#,
        ])
        .is_err());
        // --items together with --device-list also conflicts.
        assert!(T::try_parse_from([
            "test",
            "subscribe-device-update",
            "--device-list",
            "d1,d2",
            "--items",
            r#"[{"jobId":"0xB","deviceList":["d9"]}]"#,
        ])
        .is_err());
        // Each single form still parses cleanly.
        assert!(T::try_parse_from([
            "test",
            "subscribe-device-update",
            "--items",
            r#"[{"jobId":"0xB","deviceList":["d9"]}]"#,
        ])
        .is_ok());
        assert!(T::try_parse_from([
            "test",
            "subscribe-device-update",
            "--job-id",
            "0xA",
            "--device-list",
            "d1,d2",
        ])
        .is_ok());
    }

    #[test]
    fn normalize_requires_job_id_or_items() {
        assert!(normalize_items(None, Some("d1"), None).is_err());
    }

    #[test]
    fn build_body_is_byte_literal_items_shape() {
        let items = vec![UpdateItem {
            job_id: "0x..".to_string(),
            device_list: vec!["device1".to_string(), "device2".to_string()],
        }];
        let body = build_update_body(&items);
        assert_eq!(
            body,
            json!({ "items": [ { "jobId": "0x..", "deviceList": ["device1", "device2"] } ] })
        );
    }

    #[test]
    fn validate_item_count_boundaries_0_1_100_101() {
        assert!(validate_items_len(0).is_err()); // 0 → local error
        assert!(validate_items_len(1).is_ok()); // 1 → ok
        assert!(validate_items_len(100).is_ok()); // 100 → ok
        assert!(validate_items_len(101).is_err()); // 101 → local error
    }

    #[test]
    fn only_boolean_true_is_success() {
        assert!(is_update_success(&json!(true)));
        assert!(!is_update_success(&json!(false)));
        assert!(!is_update_success(&json!("true")));
        assert!(!is_update_success(&json!({ "updated": 1 })));
        assert!(!is_update_success(&Value::Null));
    }

    #[test]
    fn new_device_plan_preserves_tri_state_and_existing_receivers() {
        let snapshot = json!({
            "list": [
                { "jobId": "default-all", "deviceList": null },
                { "jobId": "explicit-none", "deviceList": [] },
                { "jobId": "selected", "deviceList": ["d1", "d2"] },
                { "jobId": "already-enabled", "deviceList": ["d-new"] },
                { "jobId": "missing-default-all" }
            ]
        });

        let updates = plan_new_device_updates(&snapshot, "d-new").unwrap();
        assert_eq!(
            updates,
            vec![
                UpdateItem {
                    job_id: "explicit-none".into(),
                    device_list: vec!["d-new".into()],
                },
                UpdateItem {
                    job_id: "selected".into(),
                    device_list: vec!["d1".into(), "d2".into(), "d-new".into()],
                },
            ]
        );
    }

    #[test]
    fn new_device_plan_is_idempotent_for_an_existing_receiver() {
        let snapshot = json!({
            "list": [
                { "jobId": "j1", "deviceList": ["d1", "d-new"] },
                { "jobId": "j2", "deviceList": null }
            ]
        });
        assert!(plan_new_device_updates(&snapshot, "d-new")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn reflected_new_device_snapshot_keeps_null_and_marks_every_row_receiving() {
        let mut snapshot = json!({
            "list": [
                { "jobId": "j1", "deviceList": null, "thisDeviceReceives": true },
                { "jobId": "j2", "deviceList": [], "thisDeviceReceives": false },
                { "jobId": "j3", "deviceList": ["d1"], "thisDeviceReceives": false }
            ]
        });

        reflect_new_device_in_snapshot(
            &mut snapshot,
            "d-new",
            &["j2".to_string(), "j3".to_string()],
        )
        .unwrap();
        assert!(snapshot["list"][0]["deviceList"].is_null());
        assert_eq!(snapshot["list"][1]["deviceList"], json!(["d-new"]));
        assert_eq!(snapshot["list"][2]["deviceList"], json!(["d1", "d-new"]));
        assert!(snapshot["list"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["thisDeviceReceives"] == json!(true)));
    }

    #[test]
    fn resumed_snapshot_preserves_a_completed_jobs_manual_opt_out() {
        let mut snapshot = json!({
            "list": [
                { "jobId": "completed-before-retry", "deviceList": [], "thisDeviceReceives": false },
                { "jobId": "updated-now", "deviceList": [], "thisDeviceReceives": false }
            ]
        });
        reflect_new_device_in_snapshot(
            &mut snapshot,
            "d-new",
            &["updated-now".to_string()],
        )
        .unwrap();

        assert_eq!(snapshot["list"][0]["deviceList"], json!([]));
        assert_eq!(snapshot["list"][0]["thisDeviceReceives"], json!(false));
        assert_eq!(snapshot["list"][1]["deviceList"], json!(["d-new"]));
        assert_eq!(snapshot["list"][1]["thisDeviceReceives"], json!(true));
    }

    #[test]
    fn new_device_plan_rejects_an_unupdatable_explicit_row() {
        let snapshot = json!({ "list": [{ "deviceList": [] }] });
        assert!(plan_new_device_updates(&snapshot, "d-new").is_err());
    }

    #[test]
    fn routing_retry_only_selects_jobs_still_recorded_as_remaining() {
        let planned = vec![
            UpdateItem {
                job_id: "already-finished".into(),
                device_list: vec!["d-new".into()],
            },
            UpdateItem {
                job_id: "still-pending".into(),
                device_list: vec!["d-new".into()],
            },
        ];
        let selected = select_updates_for_routing_marker(
            planned,
            RoutingMarker {
                version: ROUTING_MARKER_VERSION,
                phase: RoutingMarkerPhase::Routing,
                remaining_job_ids: vec!["still-pending".into()],
            },
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].job_id, "still-pending");

        let completed = select_updates_for_routing_marker(
            vec![UpdateItem {
                job_id: "manual-opt-out".into(),
                device_list: vec!["d-new".into()],
            }],
            RoutingMarker {
                version: ROUTING_MARKER_VERSION,
                phase: RoutingMarkerPhase::Completed,
                remaining_job_ids: Vec::new(),
            },
        );
        assert!(
            completed.is_err(),
            "completed state must never rewrite routes"
        );
    }

    #[test]
    fn routing_state_is_environment_scoped_and_completed_is_not_pending() {
        let _env_lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let test_home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(format!("pending_device_routing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_home);
        std::env::set_var("ONCHAINOS_HOME", &test_home);

        let production = "https://web3.okx.com";
        let beta = "https://beta.okex.org";
        // 5.0.89 used an environment-agnostic hash and a plain-text marker.
        // Leave such a file in place and prove V2 never consumes it.
        let mut legacy_hasher = Sha256::new();
        legacy_hasher.update(b"agent-1");
        legacy_hasher.update([0]);
        legacy_hasher.update(b"device-1");
        let legacy_path = test_home
            .join(PENDING_ROUTING_DIR)
            .join(format!("{:x}.pending", legacy_hasher.finalize()));
        crate::home::atomic_write(&legacy_path, b"pending\n", true).unwrap();
        assert!(!new_device_routing_is_pending(production, "agent-1", "device-1").unwrap());
        assert!(!new_device_routing_is_pending(beta, "agent-1", "device-1").unwrap());
        mark_new_device_routing_pending(production, "agent-1", "device-1").unwrap();
        assert!(new_device_routing_is_pending(production, "agent-1", "device-1").unwrap());
        assert!(
            new_device_routing_is_pending("https://web3.okx.com/", "agent-1", "device-1").unwrap()
        );
        assert!(!new_device_routing_is_pending(beta, "agent-1", "device-1").unwrap());
        assert!(!new_device_routing_is_pending(production, "agent-1", "device-2").unwrap());

        mark_new_device_routing_completed(production, "agent-1", "device-1").unwrap();
        assert!(!new_device_routing_is_pending(production, "agent-1", "device-1").unwrap());
        assert_eq!(
            read_routing_marker(production, "agent-1", "device-1")
                .unwrap()
                .unwrap()
                .phase,
            RoutingMarkerPhase::Completed
        );
        clear_new_device_routing_state(production, "agent-1", "device-1").unwrap();
        assert!(read_routing_marker(production, "agent-1", "device-1")
            .unwrap()
            .is_none());

        std::env::remove_var("ONCHAINOS_HOME");
        let _ = std::fs::remove_dir_all(&test_home);
    }

    #[test]
    fn form_b_zero_items_fails_validation() {
        // `--items '[]'` resolves to an empty array → 0-item boundary.
        let items = normalize_items(None, None, Some("[]")).unwrap();
        assert!(validate_items_len(items.len()).is_err());
    }

    // ── create-subscribe device set resolution ──────────────────────────
    #[test]
    fn create_device_set_default_all_devices() {
        let fetched = Some(vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]);
        let (list, degraded) = resolve_create_device_set(fetched, &[], Some("d2"));
        assert_eq!(list, vec!["d1", "d2", "d3"]);
        assert!(!degraded);
    }

    #[test]
    fn create_device_set_excludes_named_devices() {
        let fetched = Some(vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]);
        let excluded = vec!["d2".to_string()];
        let (list, degraded) = resolve_create_device_set(fetched, &excluded, Some("d1"));
        assert_eq!(list, vec!["d1", "d3"]);
        assert!(!degraded); // exclusion is a user choice, not a degrade
    }

    #[test]
    fn create_device_set_degrades_to_this_device_on_fetch_failure() {
        // Fetch failed (None) → this-device only, degraded.
        let (list, degraded) = resolve_create_device_set(None, &[], Some("dME"));
        assert_eq!(list, vec!["dME"]);
        assert!(degraded);
    }

    #[test]
    fn create_device_set_degrades_on_empty_fetch() {
        // Fetch succeeded but returned no devices → degrade too.
        let (list, degraded) = resolve_create_device_set(Some(vec![]), &[], Some("dME"));
        assert_eq!(list, vec!["dME"]);
        assert!(degraded);
    }

    #[test]
    fn create_device_set_degrade_with_unresolved_this_device_is_empty() {
        let (list, degraded) = resolve_create_device_set(None, &[], None);
        assert!(list.is_empty());
        assert!(degraded); // still degraded; create must not abort
    }

    #[test]
    fn create_device_set_all_excluded_degrades_to_unexcluded_this_device() {
        // Every fetched device is excluded, but this device is NOT in the
        // exclusion list → degrade to this device (flagged), never a silent empty.
        let fetched = Some(vec!["d1".to_string(), "d2".to_string()]);
        let excluded = vec!["d1".to_string(), "d2".to_string()];
        let (list, degraded) = resolve_create_device_set(fetched, &excluded, Some("dME"));
        assert_eq!(list, vec!["dME"]);
        assert!(degraded);
    }

    #[test]
    fn create_device_set_all_excluded_incl_this_device_is_empty_but_flagged() {
        // All fetched devices excluded AND this device is one of them → empty
        // routing set, but degraded=true so it is never reported as a clean list.
        let fetched = Some(vec!["d1".to_string(), "d2".to_string()]);
        let excluded = vec!["d1".to_string(), "d2".to_string()];
        let (list, degraded) = resolve_create_device_set(fetched, &excluded, Some("d1"));
        assert!(list.is_empty());
        assert!(
            degraded,
            "all-excluded must not be reported as not-degraded"
        );
    }

    // ── pagination helpers ───────────────────────────────────────────────
    #[test]
    fn normalize_page_params_floors_below_one() {
        assert_eq!(normalize_page_params(0, 0), (1, DEFAULT_PAGE_SIZE));
        assert_eq!(normalize_page_params(-5, -1), (1, DEFAULT_PAGE_SIZE));
    }

    #[test]
    fn normalize_page_params_passes_through_valid_and_oversize() {
        assert_eq!(normalize_page_params(3, 50), (3, 50));
        // page_size > 100 is passed through unchanged (backend rejects with 81001).
        assert_eq!(normalize_page_params(1, 500), (1, 500));
    }

    #[test]
    fn pagination_done_stops_on_empty_short_total_and_cap() {
        assert!(pagination_done(0, 20, 0, 0, 1, 1)); // empty page → stop
        assert!(pagination_done(5, 20, 0, 5, 1, 1)); // short (final) page → stop
        assert!(pagination_done(20, 20, 40, 40, 2, 1)); // reached positive total → stop
                                                        // hard safety cap reached → stop even on a full page below `total`.
        assert!(pagination_done(20, 20, 0, 200_000, 1 + MAX_PAGES, 1));
    }

    #[test]
    fn pagination_done_continues_on_full_page_below_total() {
        // full page, total not yet reached, cap far away → keep going.
        assert!(!pagination_done(20, 20, 100, 20, 1, 1));
        // total unknown (0) but page is full → keep going (only empty/short stops).
        assert!(!pagination_done(20, 20, 0, 20, 1, 1));
    }

    #[test]
    fn resolve_total_never_under_reports_aggregated_rows() {
        assert_eq!(resolve_total(0, 3), 3); // empty terminal page echoed total 0
        assert_eq!(resolve_total(5, 3), 5); // backend total wins when larger
        assert_eq!(resolve_total(2, 2), 2);
    }

    #[test]
    fn device_ids_drops_empty_ids() {
        let page = DevicePage {
            list: vec![
                DeviceRow {
                    device_id: "d1".into(),
                    ..Default::default()
                },
                DeviceRow {
                    device_id: "".into(),
                    ..Default::default()
                },
                DeviceRow {
                    device_id: "d2".into(),
                    ..Default::default()
                },
            ],
            total: 3,
        };
        assert_eq!(device_ids(page), vec!["d1", "d2"]);
    }

    #[test]
    fn normalize_form_b_rejects_empty_job_id() {
        // Form B mirrors Form A: an empty jobId is rejected, not silently sent.
        let err = normalize_items(None, None, Some(r#"[{"jobId":"","deviceList":["d1"]}]"#));
        assert!(err.is_err());
    }
}
