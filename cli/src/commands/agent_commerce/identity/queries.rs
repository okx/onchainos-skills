//! Read-only agent commands and their query assembly:
//! - `agent get`         → `GET /agent-list`
//! - `agent search`      → `GET /search/cli-search`
//! - `agent service-list`→ `GET /agent/services`
//! - `agent feedback-list`→ `GET /agent/reviews`
//!
//! Also hosts `fetch_agent_for_update` and its JSON-probing helpers, because
//! reading the current agent snapshot (for the update full-overwrite flow) is
//! semantically a query too — even though the only consumer is `mutations`.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context;
use crate::output;

use super::args::{FeedbackListArgs, GetArgs, SearchArgs, ServiceListArgs};
use super::models::{AgentService, ExistingAgentCard, XLAYER_CHAIN_INDEX};
use super::utils::{
    normalize_service, normalize_singleton_object, parse_u32_arg, push_multi_query,
    push_optional_query, reconstruct_get_url_for_log, redact_token_for_debug,
    require_non_empty, resolve_agent_id, wallet_client,
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

// ─── `agent get` ──────────────────────────────────────────────────────────

async fn get_impl(args: &GetArgs, ctx: &Context) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let client = wallet_client(ctx)?;

    // Product spec: agent-list identifies the user via JWT; `from` is never needed.
    // page / pageSize 是选填——用户没传就不塞 query，让后端使用自身默认。
    let mut query = vec![("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string())];
    push_optional_query(&mut query, "agentIds", args.agent_ids.as_deref());
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
    let client = wallet_client(ctx)?;
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
    let client = wallet_client(ctx)?;
    let agent_id = resolve_agent_id(&args.agent_id, &args.agent_id_flag)?;
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
    let client = wallet_client(ctx)?;

    // agentId 必填；page / pageSize / sortBy 按文档都是选填，用户没传就不塞，让后端用自身默认
    let mut query = vec![(
        "agentId".to_string(),
        resolve_agent_id(&args.agent_id, &args.agent_id_flag)?.to_string(),
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

    Ok(normalize_singleton_object(result?))
}

// ─── fetch_agent_for_update + JSON probing helpers ────────────────────────
//
// Called only by `mutations::update_impl` to echo back the current agent
// state before applying the user's partial update. Even though "fetching"
// is semantically a query, keeping these helpers module-private keeps the
// read-side surface clean.

pub(super) async fn fetch_agent_for_update(
    ctx: &Context,
    access_token: &str,
    agent_id: &str,
) -> Result<ExistingAgentCard> {
    let client = wallet_client(ctx)?;
    // Product spec: agent-list identifies the user via JWT and returns all agents
    // owned by that user; `from` is never needed even when filtering by agentIds.
    let query = [
        ("chainIndex".to_string(), XLAYER_CHAIN_INDEX.to_string()),
        ("agentIds".to_string(), agent_id.to_string()),
        ("page".to_string(), "1".to_string()),
        ("pageSize".to_string(), "20".to_string()),
    ];

    let query_refs: Vec<(&str, &str)> = query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();

    eprintln!(
        "[agent-identity] update.fetch-agent request: url={} access_token_len={} access_token_prefix={} query={:?}",
        reconstruct_get_url_for_log(ctx, "/priapi/v5/wallet/agentic/agent/agent-list", &query_refs),
        access_token.len(),
        redact_token_for_debug(access_token),
        query_refs,
    );

    let fetch_result = client
        .get_authed(
            "/priapi/v5/wallet/agentic/agent/agent-list",
            access_token,
            &query_refs,
        )
        .await;

    match &fetch_result {
        Ok(data) => eprintln!(
            "[agent-identity] update.fetch-agent response: {}",
            serde_json::to_string(data)
                .unwrap_or_else(|_| "<serialize failed>".to_string())
        ),
        Err(e) => eprintln!("[agent-identity] update.fetch-agent response err: {:#}", e),
    }

    let data = normalize_singleton_object(fetch_result?);
    let list = data["list"]
        .as_array()
        .ok_or_else(|| anyhow!("agent get response missing list"))?;
    let item = list
        .iter()
        .find(|item| json_to_string(item.get("agentId")) == Some(agent_id.to_string()))
        .cloned()
        .ok_or_else(|| anyhow!("agent not found: {agent_id}"))?;
    extract_existing_agent_card(&item)
}

fn extract_existing_agent_card(item: &Value) -> Result<ExistingAgentCard> {
    let card_json = extract_embedded_card_json(item);

    let role = first_string_field(&[card_json.as_ref(), Some(item)], &[&["Role"], &["role"]]);
    let name = first_string_field(&[card_json.as_ref(), Some(item)], &[&["Name"], &["name"]]);
    let profile_picture = first_string_field(
        &[card_json.as_ref(), Some(item)],
        &[&["ProfilePicture"], &["profilePicture"]],
    );
    let profile_description = first_string_field(
        &[card_json.as_ref(), Some(item)],
        &[&["ProfileDescription"], &["profileDescription"]],
    );
    let communication_address = first_string_field(
        &[card_json.as_ref(), Some(item)],
        &[&["CommunicationAddress"], &["communicationAddress"]],
    );

    let services = first_value_field(
        &[card_json.as_ref(), Some(item)],
        &[&["Service"], &["service"], &["services"]],
    )
    .map(parse_services_value)
    .transpose()?;

    Ok(ExistingAgentCard {
        role,
        name,
        profile_picture,
        profile_description,
        communication_address,
        services,
    })
}

fn extract_embedded_card_json(item: &Value) -> Option<Value> {
    first_value_field(
        &[Some(item)],
        &[&["cardJson"], &["cardJSON"], &["card"], &["agentCard"]],
    )
    .and_then(|value| match value {
        Value::String(text) => serde_json::from_str::<Value>(text).ok(),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    })
}

fn first_string_field(sources: &[Option<&Value>], keys: &[&[&str]]) -> Option<String> {
    for source in sources {
        let Some(source) = source else {
            continue;
        };
        for group in keys {
            for key in *group {
                if let Some(value) = source.get(*key) {
                    if let Some(value) = json_to_string(Some(value)) {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

fn first_value_field<'a>(sources: &[Option<&'a Value>], keys: &[&[&str]]) -> Option<&'a Value> {
    for source in sources {
        let Some(source) = source else {
            continue;
        };
        for group in keys {
            for key in *group {
                if let Some(value) = source.get(*key) {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn parse_services_value(value: &Value) -> Result<Vec<AgentService>> {
    let services = match value {
        Value::Null => Vec::new(),
        Value::Array(_) => serde_json::from_value::<Vec<AgentService>>(value.clone())
            .map_err(|e| anyhow!("failed to parse current agent services: {e}"))?,
        _ => bail!("current agent services are not an array"),
    };
    services
        .into_iter()
        .map(normalize_service)
        .collect::<Result<Vec<_>>>()
}

fn json_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(if *boolean { "true" } else { "false" }.to_string()),
        _ => None,
    }
}
