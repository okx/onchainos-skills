//! Read-only agent commands and their query assembly:
//! - `agent get`         → `GET /agent/agent-list`
//! - `agent search`      → `GET /search/agent-search`
//! - `agent service-list`→ `GET /agent/services`
//! - `agent feedback-list`→ `GET /agent/reviews`

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context;
use crate::output;

use super::args::{FeedbackListArgs, GetArgs, GetByAddressArgs, SearchArgs, ServiceListArgs};
use super::models::XLAYER_CHAIN_INDEX;
use super::utils::{
    add_agent_list_cells, add_feedback_list_cells, add_search_cells, add_service_list_cells,
    convert_feedback_list_scores, enrich_agent_get_rows, normalize_singleton_object, parse_u32_arg,
    push_multi_query, push_optional_query, reconstruct_get_url_for_log, redact_token_for_debug,
    require_non_empty, wallet_client,
};

// ─── Public command entry points ──────────────────────────────────────────

pub async fn get(args: GetArgs, ctx: &Context) -> Result<()> {
    output::success(get_impl(&args, ctx).await?);
    Ok(())
}

pub async fn search(args: SearchArgs, ctx: &Context) -> Result<()> {
    output::success(search_impl(&args, ctx).await?);
    Ok(())
}

pub async fn service_list(args: ServiceListArgs, ctx: &Context) -> Result<()> {
    output::success(service_list_impl(&args, ctx).await?);
    Ok(())
}

pub async fn feedback_list(args: FeedbackListArgs, ctx: &Context) -> Result<()> {
    output::success(feedback_list_impl(&args, ctx).await?);
    Ok(())
}

pub async fn get_by_address(args: GetByAddressArgs, ctx: &Context) -> Result<()> {
    output::success(get_by_address_impl(&args, ctx).await?);
    Ok(())
}

pub async fn top_asps(limit: usize, ctx: &Context) -> Result<()> {
    output::success(top_asps_impl(limit, ctx).await?);
    Ok(())
}

// ─── `agent top-asps` ───────────────────────────────────────────────────────

const TOP_ASPS_PAGE_SIZE: u32 = 100;
/// Safety cap on pagination (100 × 50 = 5000 ASPs) so a backend that never
/// reports a final page can't loop forever.
const TOP_ASPS_MAX_PAGES: u32 = 50;
/// agent-search requires a non-empty `query` (omitting it → code 902) but has no
/// "list all" mode. A single common character matches the whole ASP population
/// (verified: "a" / "e" / any common character / … all return the same total), so we use it to
/// approximate a full listing. Swap for a real list-all/top-N endpoint once one exists.
const TOP_ASPS_BROAD_QUERY: &str = "a";

/// Pull the full marketplace ASP list (paginated agent-search), de-dup, then
/// return the top `limit` by `soldCount` (highest first). Returns fewer than
/// `limit` when the marketplace has fewer ASPs.
async fn top_asps_impl(limit: usize, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    let page_size_s = TOP_ASPS_PAGE_SIZE.to_string();
    let mut all: Vec<Value> = Vec::new();
    let mut total: u64 = 0;

    for page in 1..=TOP_ASPS_MAX_PAGES {
        let page_s = page.to_string();
        let query_refs: Vec<(&str, &str)> = vec![
            ("query", TOP_ASPS_BROAD_QUERY),
            ("page", page_s.as_str()),
            ("pageSize", page_size_s.as_str()),
        ];
        let data = normalize_singleton_object(
            client
                .get_authed(
                    "/priapi/v5/wallet/agentic/search/agent-search",
                    &access_token,
                    &query_refs,
                )
                .await?,
        );
        if page == 1 {
            total = data["total"].as_u64().unwrap_or(0);
        }
        let list = data["list"].as_array().cloned().unwrap_or_default();
        let got = list.len();
        all.extend(list);
        if got < TOP_ASPS_PAGE_SIZE as usize || (all.len() as u64) >= total {
            break;
        }
    }

    // De-dup by agentId (pagination guard), then rank by soldCount, highest first.
    let mut seen = std::collections::HashSet::new();
    all.retain(|a| {
        let id = a.get("agentId")
            .and_then(|v| v.as_u64().map(|n| n.to_string())
                .or_else(|| v.as_str().map(str::to_string)))
            .unwrap_or_default();
        seen.insert(id)
    });
    let total_pulled = all.len();
    all.sort_by(|a, b| {
        b["soldCount"]
            .as_i64()
            .unwrap_or(0)
            .cmp(&a["soldCount"].as_i64().unwrap_or(0))
    });
    all.truncate(limit);

    Ok(json!({ "totalPulled": total_pulled, "asps": all }))
}

// ─── `agent get` ──────────────────────────────────────────────────────────

async fn get_impl(args: &GetArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    // Product spec: agent-list identifies the user via JWT; `from` is never needed.
    let mut query = vec![("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string())];
    push_optional_query(&mut query, "agentIdList", args.agent_ids.as_deref());
    if let Some(page_raw) = args.page.as_deref() {
        let page = parse_u32_arg(Some(page_raw), "--page", 1, Some(1), None, false)?;
        query.push(("page".to_string(), page.to_string()));
    }
    let page_size = parse_u32_arg(args.page_size.as_deref(), "--page-size", 5, Some(1), None, false)?;
    query.push(("pageSize".to_string(), page_size.to_string()));

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    eprintln!(
        "[agent-identity] get request: url={} access_token_len={} access_token_prefix={} query={:?}",
        reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/agent-list", &query_refs),
        access_token.len(),
        redact_token_for_debug(&access_token),
        query_refs,
    );

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/agent-list",
            &access_token,
            &query_refs,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] get response: {}",
            {
                let s = serde_json::to_string(data).unwrap_or_else(|_| "<serialize failed>".to_string());
                if s.chars().count() > 256 { format!("{}...", s.chars().take(256).collect::<String>()) } else { s }
            }
        ),
        Err(e) => eprintln!("[agent-identity] get response err: {:#}", e),
    }

    let mut out = normalize_singleton_object(result?);
    // Additive: enrich each agent row with computed display fields (roleLabel
    // / statusLabel / approvalLabel / ratingStars). Rows are read from either
    // the single-layer shape (row = list[*]) or the legacy double-layer shape
    // (row = list[*].agentList[*]); both are tolerated. Raw role / status /
    // approvalDisplayStatus / reputation are left intact.
    enrich_agent_get_rows(&mut out);
    // Additive: in LIST mode (no --agent-ids) add a ready-to-render `cells`
    // array per row (references/discover.md §list columns). Detail mode (with
    // --agent-ids) already carries the `card`; the list-table `cells` are the
    // row analog and only meaningful for the list view.
    if args.agent_ids.is_none() {
        add_agent_list_cells(&mut out);
    }
    Ok(out)
}

// ─── `agent search` ───────────────────────────────────────────────────────

async fn search_impl(args: &SearchArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let query_text = require_non_empty(args.query.as_deref(), "--query")?;

    // query is required; page / pageSize / multi-value filter fields are optional — omit when not provided
    let mut query = vec![("query".to_string(), query_text.to_string())];
    if let Some(page_raw) = args.page.as_deref() {
        let page = parse_u32_arg(Some(page_raw), "--page", 1, Some(1), None, false)?;
        query.push(("page".to_string(), page.to_string()));
    }
    let page_size = parse_u32_arg(args.page_size.as_deref(), "--page-size", 5, Some(1), Some(100), true)?;
    query.push(("pageSize".to_string(), page_size.to_string()));
    push_multi_query(&mut query, "feedback", &args.feedback);
    push_multi_query(&mut query, "agentInfo", &args.agent_info);
    push_multi_query(&mut query, "status", &args.status);
    push_multi_query(&mut query, "service", &args.service);

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    eprintln!(
        "[agent-identity] search request: url={} access_token_len={} access_token_prefix={} query={:?}",
        reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/search/agent-search", &query_refs),
        access_token.len(),
        redact_token_for_debug(&access_token),
        query_refs,
    );

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/search/agent-search",
            &access_token,
            &query_refs,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] search response: {}",
            {
                let s = serde_json::to_string(data).unwrap_or_else(|_| "<serialize failed>".to_string());
                if s.chars().count() > 256 { format!("{}...", s.chars().take(256).collect::<String>()) } else { s }
            }
        ),
        Err(e) => eprintln!("[agent-identity] search response err: {:#}", e),
    }

    // Additive: add a ready-to-render `cells` array per search row (the §6
    // search columns — note the distinct search schema: feedbackRate is
    // already 0–5, serviceMinPrice is the price, services may be absent).
    let mut out = normalize_singleton_object(result?);
    add_search_cells(&mut out);
    Ok(out)
}

// ─── `agent service-list` ─────────────────────────────────────────────────

async fn service_list_impl(args: &ServiceListArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let agent_id = require_non_empty(args.agent_id.as_deref(), "--agent-id")?;
    let query = [("agentId".to_string(), agent_id.to_string())];
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    eprintln!(
        "[agent-identity] service-list request: url={} access_token_len={} access_token_prefix={} query={:?}",
        reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/services", &query_refs),
        access_token.len(),
        redact_token_for_debug(&access_token),
        query_refs,
    );

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/services",
            &access_token,
            &query_refs,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] service-list response: {}",
            {
                let s = serde_json::to_string(data).unwrap_or_else(|_| "<serialize failed>".to_string());
                if s.chars().count() > 256 { format!("{}...", s.chars().take(256).collect::<String>()) } else { s }
            }
        ),
        Err(e) => eprintln!("[agent-identity] service-list response err: {:#}", e),
    }

    // Additive: add a ready-to-render `cells` array per service (§4 columns).
    let mut out = result?;
    add_service_list_cells(&mut out);
    Ok(out)
}

// ─── `agent feedback-list` ────────────────────────────────────────────────

async fn feedback_list_impl(args: &FeedbackListArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    // agentId is required; page / pageSize / sortBy are optional — omit when not provided, let the backend use its defaults
    let mut query = vec![(
        "agentId".to_string(),
        require_non_empty(args.agent_id.as_deref(), "--agent-id")?.to_string(),
    )];
    if let Some(page_raw) = args.page.as_deref() {
        let page = parse_u32_arg(Some(page_raw), "--page", 1, Some(1), None, false)?;
        query.push(("page".to_string(), page.to_string()));
    }
    if let Some(page_size_raw) = args.page_size.as_deref() {
        let page_size = parse_u32_arg(
            Some(page_size_raw),
            "--page-size",
            20,
            Some(1),
            Some(50),
            true,
        )?;
        query.push(("pageSize".to_string(), page_size.to_string()));
    }
    if let Some(sort_by_raw) = args.sort_by.as_deref() {
        let sort_by = match sort_by_raw {
            "time_desc" | "score_desc" => sort_by_raw,
            other => bail!("invalid value for --sort-by: {other}"),
        };
        query.push(("sortBy".to_string(), sort_by.to_string()));
    }
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    eprintln!(
        "[agent-identity] feedback-list request: url={} access_token_len={} access_token_prefix={} query={:?}",
        reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/reviews", &query_refs),
        access_token.len(),
        redact_token_for_debug(&access_token),
        query_refs,
    );

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/reviews",
            &access_token,
            &query_refs,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] feedback-list response: {}",
            {
                let s = serde_json::to_string(data).unwrap_or_else(|_| "<serialize failed>".to_string());
                if s.chars().count() > 256 { format!("{}...", s.chars().take(256).collect::<String>()) } else { s }
            }
        ),
        Err(e) => eprintln!("[agent-identity] feedback-list response err: {:#}", e),
    }

    // Convert backend 0–100 scores to 0.00–5.00 stars before surfacing to
    // the user. Both `average` and per-entry `score` become 2-decimal
    // floats — matches the 2-decimal input precision now accepted by
    // `feedback-submit`. Mapping rule: `utils::convert_feedback_list_scores`.
    let mut out = normalize_singleton_object(result?);
    convert_feedback_list_scores(&mut out);
    // Additive: add a ready-to-render `cells` array per feedback item (§5
    // columns). Runs AFTER score conversion so `score` is a 0.00–5.00 float.
    add_feedback_list_cells(&mut out);
    Ok(out)
}

// ─── `agent get-by-address` ───────────────────────────────────────────────
//
// Hidden command. Reverse-lookup an agent by its on-chain communication
// address + chainIndex. Same JWT-auth shape as the other read-side calls.

async fn get_by_address_impl(args: &GetByAddressArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    // clap already enforces required=true; this defensively trims against `--communication-address ""`.
    let communication_address =
        require_non_empty(Some(args.communication_address.as_str()), "--communication-address")?;
    let chain_index = args
        .chain_index
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(XLAYER_CHAIN_INDEX);

    let query = [
        (
            "communicationAddress".to_string(),
            communication_address.to_string(),
        ),
        ("chainIndex".to_string(), chain_index.to_string()),
    ];
    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    eprintln!(
        "[agent-identity] get-by-address request: url={} access_token_len={} access_token_prefix={} query={:?}",
        reconstruct_get_url_for_log(
            ctx,
            "/priapi/v5/wallet/agentic/agent/by-communication-address",
            &query_refs,
        ),
        access_token.len(),
        redact_token_for_debug(&access_token),
        query_refs,
    );

    let result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/by-communication-address",
            &access_token,
            &query_refs,
        )
        .await;

    match &result {
        Ok(data) => eprintln!(
            "[agent-identity] get-by-address response: {}",
            {
                let s = serde_json::to_string(data).unwrap_or_else(|_| "<serialize failed>".to_string());
                if s.chars().count() > 256 { format!("{}...", s.chars().take(256).collect::<String>()) } else { s }
            }
        ),
        Err(e) => eprintln!("[agent-identity] get-by-address response err: {:#}", e),
    }

    Ok(normalize_singleton_object(result?))
}

// ─── Tests ───────────────────────────────────────────────────────────────────
// NOTE: All public entry points in this module (get / search / service_list /
// feedback_list / get_by_address / top_asps) are async and require a live
// authenticated HTTP client. Integration-level coverage requires a mock HTTP
// layer (e.g. mockito) which is not yet wired into this crate's dev-dependencies.
//
// The testable pure-logic paths are:
//   - `sort_by` enum validation in feedback_list_impl (bail! on unknown value)
//   - `chain_index` default-fallback in get_by_address_impl (None/empty → XLAYER)
//   - `top_asps_impl` accumulation + dedup logic
//
// These are exercised at the integration layer. Add `mockito` to
// [dev-dependencies] in Cargo.toml to enable unit-level HTTP mocking here.
#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
}
