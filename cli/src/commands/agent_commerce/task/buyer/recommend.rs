//! Fetch recommended providers.
//!
//! User action: fetch recommended providers — `onchainos agent recommend`.
//!
//! - Default: call the `/match` API, fetch the list and cache it locally (index=0).
//! - `--next`: advance to the next provider in local state and return it.
//! - `--current`: return the provider at the current index (do not advance).
//! - `--next-page`: advance to the next page.

use anyhow::Result;

use super::negotiate;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::pending_v2;
use crate::commands::agent_commerce::task::common::util::short_job_id;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;
use crate::commands::agent_commerce::task::signing;

/// Bundle of args controlling the "auto-enqueue recommend card as a pending
/// decision" behavior. When `enabled` is true, after writing the card file
/// `handle_recommend` directly forwards content to
/// `pending-decisions-v2 request --source-event recommend_pick` (in-process),
/// eliminating the LLM-driven `pending-decisions-v2 request` round-trip.
///
/// `user_content`: when set, used verbatim as the decision card body (e.g. a
/// sub-prepared localized version). When `None`, the auto-written canonical
/// English card file is read and used. This split exists because some runtimes
/// (notably OpenClaw) do not translate `xmtp_prompt_user.userContent` at
/// render time — the sub session must pre-localize before enqueueing.
#[derive(Default)]
pub struct EmitDecisionOpts {
    pub enabled: bool,
    pub sub_key: Option<String>,
    pub job_title: Option<String>,
    pub user_content: Option<String>,
}

/// Fetch recommended providers (default mode: call the API + cache).
pub async fn handle_recommend(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    page: usize,
    emit: EmitDecisionOpts,
) -> Result<()> {
    let resolved;
    let agent_id = if agent_id.is_empty() {
        use crate::commands::agent_commerce::task::common::AGENT_ROLE_BUYER;
        resolved = signing::resolve_agent_id_by_role(AGENT_ROLE_BUYER).await?;
        if resolved.is_empty() {
            anyhow::bail!("--agent-id was not provided and no local buyer identity exists; please register or pass --agent-id");
        }
        &resolved
    } else {
        agent_id
    };

    let url = client.endpoint(job_id, "match");
    let body = serde_json::json!({ "page": page + 1 });
    let resp = client.post_with_identity(&url, &body, agent_id).await?;
    let recs = resp["recommendations"].as_array()
        .cloned().unwrap_or_default();

    let failed = negotiate::load_failed(job_id);

    let providers: Vec<negotiate::ProviderInfo> = recs.iter().map(|r| {
        let services: Vec<negotiate::ServiceInfo> = r["services"].as_array()
            .map(|arr| arr.iter().map(|s| negotiate::ServiceInfo {
                service_id: s["serviceId"].as_str().unwrap_or("").to_string(),
                service_name: s["serviceName"].as_str().unwrap_or("").to_string(),
                service_description: s["serviceDescription"].as_str().unwrap_or("").to_string(),
                service_type: s["serviceType"].as_str().unwrap_or("").to_string(),
                endpoint: s["endpoint"].as_str().unwrap_or("").to_string(),
                sort_order: s["sortOrder"].as_i64().unwrap_or(0),
                fee_amount: s["feeAmount"].as_f64().unwrap_or(0.0),
                fee_token_symbol: s["feeTokenSymbol"].as_str().unwrap_or("").to_string(),
                fee_token: s["feeToken"].as_str().unwrap_or("").to_string(),
            }).collect())
            .unwrap_or_default();

        negotiate::ProviderInfo {
            provider_address: r["providerAddress"].as_str().unwrap_or("").to_string(),
            provider_agent_id: r["providerAgentId"].as_str().unwrap_or("").to_string(),
            provider_name: r["providerName"].as_str().unwrap_or("").to_string(),
            match_score: r["matchScore"].as_f64().unwrap_or(0.0),
            credit_score: r["creditScore"].as_i64().unwrap_or(0),
            capability_summary: r["capabilitySummary"].as_str().unwrap_or("").to_string(),
            completed_task_count: r["completedTaskCount"].as_i64().unwrap_or(0),
            support_a2mcp: r["supportA2MCP"].as_bool().unwrap_or(false),
            services,
        }
    }).collect();

    negotiate::save(job_id, providers.clone(), page)?;

    let visible: Vec<_> = providers.iter()
        .filter(|p| !failed.contains(&p.provider_agent_id))
        .collect();

    if visible.is_empty() {
        if !providers.is_empty() {
            println!("All providers on this page have failed negotiation; auto-advancing to the next page...");
            // Forward the caller's emit options (sub_key / title) so the auto-advanced
            // page can also short-circuit the LLM round-trips if it has visible cards.
            return Box::pin(handle_recommend(client, job_id, agent_id, page + 1, emit)).await;
        }
        println!("The recommended provider list is empty; no matching providers.");
        print_empty_guidance(job_id);
        return Ok(());
    }

    let cards_path = write_cards_file(job_id, &visible, page)?;

    // Short-circuit the LLM "request" round-trip by enqueueing the card
    // directly. Card body source order:
    //   1. `--user-content` (sub pre-localized) if provided
    //   2. otherwise read the canonical English card file just written
    // The OpenClaw runtime does not auto-translate xmtp_prompt_user.userContent,
    // so when the user's language is non-English the sub session is expected to
    // Read the card file, translate, and pass the result via `--user-content`.
    if emit.enabled {
        let sub_key = emit.sub_key.ok_or_else(|| {
            anyhow::anyhow!(
                "--emit-decision requires --sub-key (full sessionKey from session_status)"
            )
        })?;
        let card_body = match emit.user_content {
            Some(c) => c,
            None => std::fs::read_to_string(&cards_path)
                .map_err(|e| anyhow::anyhow!("read card file {cards_path}: {e}"))?,
        };
        let title = emit.job_title.as_deref().unwrap_or("<title>");
        let short_id = short_job_id(job_id);
        let list_label = format!("[Recommend {short_id}] {title} ASP-pick decision");
        if DEBUG_LOG {
            eprintln!(
                "[recommend] emit-decision: jobId={job_id} sub_key={sub_key} list_label={list_label} body_len={}",
                card_body.len()
            );
        }
        return pending_v2::enqueue_recommend_decision(
            sub_key,
            job_id.to_string(),
            "buyer".to_string(),
            agent_id.to_string(),
            card_body,
            list_label,
        );
    }

    println!(
        "Recommended ASPs (page {}, {} available). Card file: {}",
        page + 1,
        visible.len(),
        cards_path,
    );

    Ok(())
}

/// --current: return the current providers (filtered to exclude failed ones).
pub fn handle_recommend_current(job_id: &str) -> Result<()> {
    let state = negotiate::load(job_id)?;
    let failed = &state.failed_providers;
    let visible: Vec<_> = state.providers.iter()
        .filter(|p| !failed.contains(&p.provider_agent_id))
        .collect();

    if visible.is_empty() {
        println!("No more available providers on the current page ({} already failed).", failed.len());
        print_empty_guidance(job_id);
    } else {
        println!("Available providers on the current page (page {}, {} total):", state.page + 1, visible.len());
        for (i, p) in visible.iter().enumerate() {
            print_provider(i, p);
        }
    }
    Ok(())
}

/// --next: advance to the next provider.
pub fn handle_recommend_next(job_id: &str) -> Result<()> {
    match negotiate::next(job_id)? {
        Some(p) => {
            let state = negotiate::load(job_id)?;
            println!("Switched to the next provider (index={}, {} total):", state.current_index, state.providers.len());
            print_provider(state.current_index, &p);
            print_routing_guide(&p, job_id);
        }
        None => {
            let state = negotiate::load(job_id)?;
            println!("Recommendation list fully iterated ({}/{}); no more providers.", state.current_index, state.providers.len());
            print_empty_guidance(job_id);
        }
    }
    Ok(())
}

/// --next-page: advance to the next page.
pub async fn handle_recommend_next_page(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let state = negotiate::load(job_id)?;
    let next_page = state.page + 1;
    let agent_id = {
        use crate::commands::agent_commerce::task::common::AGENT_ROLE_BUYER;
        signing::resolve_agent_id_by_role(AGENT_ROLE_BUYER).await?
    };
    if agent_id.is_empty() {
        anyhow::bail!("No local buyer identity; please register or pass --agent-id");
    }
    handle_recommend(client, job_id, &agent_id, next_page, EmitDecisionOpts::default()).await
}

const DESC_MAX_CHARS: usize = 120;

/// Write pre-formatted card text to `~/.onchainos/task/{jobId}/recommend-cards.txt`.
///
/// Returns the absolute path to the written file. The card format matches the
/// `--user-content` template specified in `match_provider.rs`, so the sub
/// agent can pass the file path directly instead of composing cards from raw
/// CLI output — eliminating one full copy of the card content from the sub
/// session context.
fn write_cards_file(
    job_id: &str,
    visible: &[&negotiate::ProviderInfo],
    page: usize,
) -> Result<String> {
    let sid = short_job_id(job_id);

    let mut buf = String::new();
    buf.push_str(&format!(
        "[Job {sid} — you are the User Agent] Recommended ASPs (page {}):\n",
        page + 1,
    ));

    for (i, p) in visible.iter().enumerate() {
        let svc = p.services.first();
        let svc_name = svc.map(|s| s.service_name.as_str()).unwrap_or("-");
        buf.push_str(&format!(
            "\n━━━ {}. #{} | {} ━━━\n",
            i + 1,
            p.provider_agent_id,
            svc_name,
        ));
        if let Some(s) = svc {
            let desc = truncate_desc(&s.service_description);
            buf.push_str(&format!("Description: {desc}\n"));
            let sym = if s.fee_token_symbol.is_empty() { &s.fee_token } else { &s.fee_token_symbol };
            buf.push_str(&format!("Fee: {} {}\n", s.fee_amount, sym));
        }
        let mode = if p.support_a2mcp { "x402" } else { "Escrow" };
        buf.push_str(&format!("Payment: {mode}\n"));

        // additional services
        for extra in p.services.iter().skip(1) {
            let extra_desc = truncate_desc(&extra.service_description);
            let extra_sym = if extra.fee_token_symbol.is_empty() { &extra.fee_token } else { &extra.fee_token_symbol };
            let extra_mode = if extra.service_type == "A2MCP" { "x402" } else { "Escrow" };
            buf.push_str(&format!(
                "  ┊ {} — {}\n  ┊ Fee: {} {} | Payment: {}\n",
                extra.service_name, extra_desc, extra.fee_amount, extra_sym, extra_mode,
            ));
        }
    }

    buf.push_str(concat!(
        "\n---\n",
        "Please choose:\n",
        "- Reply with a number (e.g. 1, 2, 3) or AgentID (e.g. 864) to pick an ASP\n",
        "- See more recommendations\n",
        "- List the task on the open marketplace for any suitable ASP to accept\n",
        "- Cancel the task\n",
    ));

    let base = match std::env::var("ONCHAINOS_HOME") {
        Ok(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not resolve HOME"))?
            .join(".onchainos"),
    };
    let dir = base.join("task").join(job_id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("recommend-cards.txt");
    std::fs::write(&path, &buf)?;
    Ok(path.to_string_lossy().to_string())
}

fn truncate_desc(desc: &str) -> String {
    let chars: Vec<char> = desc.chars().collect();
    if chars.len() <= DESC_MAX_CHARS {
        desc.to_string()
    } else {
        let truncated: String = chars[..DESC_MAX_CHARS].iter().collect();
        format!("{truncated}...")
    }
}

/// Print the routing guide: x402 direct accept vs. A2A negotiation.
fn print_routing_guide(p: &negotiate::ProviderInfo, job_id: &str) {
    println!();
    if p.support_a2mcp {
        let svc = p.services.first();
        let endpoint = svc.map(|s| s.endpoint.as_str()).unwrap_or("<endpoint>");
        let fee = svc.map(|s| s.fee_amount).unwrap_or(0.0);
        let symbol = svc
            .map(|s| if s.fee_token_symbol.is_empty() { "?" } else { s.fee_token_symbol.as_str() })
            .unwrap_or("?");
        println!("  ⚡ Route: x402 (no negotiation; accept directly)");
        println!("  → onchainos agent confirm-accept {job_id} --provider {} --payment-mode x402 --token-symbol {symbol} --token-amount {fee} --endpoint {endpoint}", p.provider_agent_id);
    } else {
        println!("  💬 Route: A2A (negotiation required)");
        println!("  → First call xmtp_start_conversation to create a group with provider {}, then use xmtp_send to negotiate the task details / price / payment mode, then wait for provider_applied.", p.provider_agent_id);
    }
    println!();
}

fn print_provider(index: usize, p: &negotiate::ProviderInfo) {
    let name_display = if p.provider_name.is_empty() { "-" } else { &p.provider_name };
    println!("  {}. Agent Name: {}  AgentID: {}  Credit: {}",
        index + 1, name_display, p.provider_agent_id, p.credit_score,
    );
    if !p.services.is_empty() {
        for svc in &p.services {
            println!("     Service: {} — {}", svc.service_name, svc.service_description);
            if svc.fee_amount > 0.0 {
                let sym = if svc.fee_token_symbol.is_empty() { &svc.fee_token } else { &svc.fee_token_symbol };
                println!("     Fee: {} {}  |  endpoint: {}", svc.fee_amount, sym, svc.endpoint);
            }
        }
    }
    if p.support_a2mcp {
        println!("     Payment mode: x402");
    } else {
        println!("     Payment mode: escrow");
    }
}

fn print_empty_guidance(job_id: &str) {
    println!("Choose the next step:");
    println!("  A. Designate a provider  → supply a provider agentId; a group will be created to negotiate.");
    println!("  B. Convert to public     → onchainos agent set-public {job_id}");
    println!("  C. Close the task        → onchainos agent close {job_id}");
}
