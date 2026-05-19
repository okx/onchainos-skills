//! 获取推荐服务商
//!
//! 用户动作：获取推荐服务商 — onchainos agent recommend
//!
//! - 默认：调用 /match API 获取推荐列表并缓存到本地（index=0）
//! - --next：从本地状态推进到下一个 provider 并返回
//! - --current：返回当前 index 的 provider（不推进）
//! - --next-page：翻到下一页

use anyhow::Result;

use super::negotiate;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 查询推荐服务商（默认模式：调用 API + 缓存）
pub async fn handle_recommend(client: &mut TaskApiClient, job_id: &str, agent_id: &str, page: usize) -> Result<()> {
    let resolved;
    let agent_id = if agent_id.is_empty() {
        use crate::commands::agent_commerce::task::common::AGENT_ROLE_BUYER;
        resolved = signing::resolve_agent_id_by_role(AGENT_ROLE_BUYER).await?;
        if resolved.is_empty() {
            anyhow::bail!("未传 --agent-id 且本地无 buyer 身份，请先注册或传入 --agent-id");
        }
        &resolved
    } else {
        agent_id
    };

    let url = client.endpoint(job_id, "match");
    let body = serde_json::json!({ "pageNo": page + 1 });
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
            println!("当前页所有服务商均已协商失败，自动翻到下一页...");
            return Box::pin(handle_recommend(client, job_id, agent_id, page + 1)).await;
        }
        println!("推荐服务商列表为空，无匹配服务商。");
        print_empty_guidance(job_id);
        return Ok(());
    }

    println!("推荐服务商列表（第 {} 页，共 {} 个可选）：", page + 1, visible.len());
    for (i, p) in visible.iter().enumerate() {
        print_provider(i, p);
    }
    println!();
    println!("请选择一个服务商（输入序号对应的 AgentID），或输入 `onchainos agent recommend {} --next-page` 查看下一页。", job_id);

    Ok(())
}

/// --current：返回当前 provider（过滤已失败的）
pub fn handle_recommend_current(job_id: &str) -> Result<()> {
    let state = negotiate::load(job_id)?;
    let failed = &state.failed_providers;
    let visible: Vec<_> = state.providers.iter()
        .filter(|p| !failed.contains(&p.provider_agent_id))
        .collect();

    if visible.is_empty() {
        println!("当前页推荐列表已无可选服务商（{} 个已失败）", failed.len());
        print_empty_guidance(job_id);
    } else {
        println!("当前页可选服务商（第 {} 页，共 {} 个）：", state.page + 1, visible.len());
        for (i, p) in visible.iter().enumerate() {
            print_provider(i, p);
        }
    }
    Ok(())
}

/// --next：推进到下一个 provider
pub fn handle_recommend_next(job_id: &str) -> Result<()> {
    match negotiate::next(job_id)? {
        Some(p) => {
            let state = negotiate::load(job_id)?;
            println!("切换到下一个服务商（index={}，共 {} 个）：", state.current_index, state.providers.len());
            print_provider(state.current_index, &p);
            print_routing_guide(&p, job_id);
        }
        None => {
            let state = negotiate::load(job_id)?;
            println!("推荐列表已全部遍历（{}/{}），无更多服务商", state.current_index, state.providers.len());
            print_empty_guidance(job_id);
        }
    }
    Ok(())
}

/// --next-page：翻到下一页
pub async fn handle_recommend_next_page(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let state = negotiate::load(job_id)?;
    let next_page = state.page + 1;
    let agent_id = {
        use crate::commands::agent_commerce::task::common::AGENT_ROLE_BUYER;
        signing::resolve_agent_id_by_role(AGENT_ROLE_BUYER).await?
    };
    if agent_id.is_empty() {
        anyhow::bail!("本地无 buyer 身份，请先注册或传入 --agent-id");
    }
    handle_recommend(client, job_id, &agent_id, next_page).await
}

/// 输出路由指引：x402 直接 accept vs A2A 走协商
fn print_routing_guide(p: &negotiate::ProviderInfo, job_id: &str) {
    println!();
    if p.support_a2mcp {
        let svc = p.services.first();
        let endpoint = svc.map(|s| s.endpoint.as_str()).unwrap_or("<endpoint>");
        let fee = svc.map(|s| s.fee_amount).unwrap_or(0.0);
        let symbol = svc
            .map(|s| if s.fee_token_symbol.is_empty() { "USDT" } else { s.fee_token_symbol.as_str() })
            .unwrap_or("USDT");
        println!("  ⚡ 路由: x402（无需协商，直接接单）");
        println!("  → onchainos agent confirm-accept {job_id} --provider {} --payment-mode x402 --token-symbol {symbol} --token-amount {fee} --endpoint {endpoint}", p.provider_agent_id);
    } else {
        println!("  💬 路由: A2A（需协商）");
        println!("  → 先调 xmtp_start_conversation 与服务商 {} 建群，再通过 xmtp_send 协商任务详情 / 价格 / 支付方式，等待 provider_applied", p.provider_agent_id);
    }
    println!();
}

fn print_provider(index: usize, p: &negotiate::ProviderInfo) {
    let name_display = if p.provider_name.is_empty() { "-" } else { &p.provider_name };
    println!("  {}. Agent Name: {}  AgentID: {}  信用分: {}",
        index + 1, name_display, p.provider_agent_id, p.credit_score,
    );
    if !p.services.is_empty() {
        for svc in &p.services {
            println!("     服务: {} — {}", svc.service_name, svc.service_description);
            if svc.fee_amount > 0.0 {
                let sym = if svc.fee_token_symbol.is_empty() { &svc.fee_token } else { &svc.fee_token_symbol };
                println!("     费用: {} {}  |  endpoint: {}", svc.fee_amount, sym, svc.endpoint);
            }
        }
    }
    if p.support_a2mcp {
        println!("     支付方式: x402");
    } else {
        println!("     支付方式: escrow");
    }
}

fn print_empty_guidance(job_id: &str) {
    println!("请选择下一步操作：");
    println!("  A. 指定服务商      → 提供服务商 agentId，将与其建群协商");
    println!("  B. 转为公开任务  → onchainos agent set-public {job_id}");
    println!("  C. 关闭任务      → onchainos agent close {job_id}");
}
