//! Read-only agent commands and their query assembly:
//! - `agent get`         → `GET /agent/agent-list`
//! - `agent search`      → `GET /search/agent-search`
//! - `agent service-list`→ `GET /agent/services`
//! - `agent feedback-list`→ `GET /agent/reviews`

use anyhow::{bail, Result};
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context;
use crate::output;

use super::args::{FeedbackListArgs, GetArgs, GetByAddressArgs, SearchArgs, ServiceListArgs};
use super::models::XLAYER_CHAIN_INDEX;
use super::utils::{
    convert_feedback_list_scores, normalize_singleton_object, parse_u32_arg, push_multi_query,
    push_optional_query, reconstruct_get_url_for_log, redact_token_for_debug, require_non_empty,
    wallet_client,
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

// ─── `agent get` ──────────────────────────────────────────────────────────

async fn get_impl(args: &GetArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    // Product spec: agent-list identifies the user via JWT; `from` is never needed.
    // page / pageSize 是选填——用户没传就不塞 query，让后端使用自身默认。
    let mut query = vec![("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string())];
    push_optional_query(&mut query, "agentIdList", args.agent_ids.as_deref());
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
            None,
            false,
        )?;
        query.push(("pageSize".to_string(), page_size.to_string()));
    }

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
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] get response err: {:#}", e),
    }

    Ok(normalize_singleton_object(result?))
}

// ─── `agent search` ───────────────────────────────────────────────────────

async fn search_impl(args: &SearchArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    let query_text = require_non_empty(args.query.as_deref(), "--query")?;

    // query 必填；page / pageSize / 多值过滤字段按文档都是选填，用户没传就不塞
    let mut query = vec![("query".to_string(), query_text.to_string())];
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
            Some(100),
            true,
        )?;
        query.push(("pageSize".to_string(), page_size.to_string()));
    }
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
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] search response err: {:#}", e),
    }

    Ok(normalize_singleton_object(result?))
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
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] service-list response err: {:#}", e),
    }

    result
}

// ─── `agent feedback-list` ────────────────────────────────────────────────

async fn feedback_list_impl(args: &FeedbackListArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;

    // agentId 必填；page / pageSize / sortBy 按文档都是选填，用户没传就不塞，让后端用自身默认
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
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] feedback-list response err: {:#}", e),
    }

    // Convert backend 0–100 scores to 0.00–5.00 stars before surfacing to
    // the user. Both `average` and per-entry `score` become 2-decimal
    // floats — matches the 2-decimal input precision now accepted by
    // `feedback-submit`. Mapping rule: `utils::convert_feedback_list_scores`.
    let mut out = normalize_singleton_object(result?);
    convert_feedback_list_scores(&mut out);
    Ok(out)
}

// ─── `agent get-by-address` ───────────────────────────────────────────────
//
// Hidden command. Reverse-lookup an agent by its on-chain communication
// address + chainIndex. Same JWT-auth shape as the other read-side calls.

async fn get_by_address_impl(args: &GetByAddressArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    // clap 已强制 required = true；这里再防御性 trim 防止 `--communication-address ""`。
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
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] get-by-address response err: {:#}", e),
    }

    Ok(normalize_singleton_object(result?))
}
