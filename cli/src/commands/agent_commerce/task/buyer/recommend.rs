//! 获取推荐卖家
//!
//! 买家动作：获取推荐卖家 — onchainos task recommend
//!
//! - 默认：调用 /match API 获取推荐列表并缓存到本地（index=0）
//! - --next：从本地状态推进到下一个 provider 并返回
//! - --current：返回当前 index 的 provider（不推进）

use anyhow::Result;

use super::negotiate;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 查询推荐卖家（默认模式：调用 API + 缓存）
pub async fn handle_recommend(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    // --agent-id 未传时，从本地身份列表解析 buyer agentId（不查任务详情）
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
    let resp = client.post_with_identity(&url, &serde_json::json!({}), agent_id).await?;
    let recs = resp["recommendations"].as_array()
        .cloned().unwrap_or_default();

    // 构造 ProviderInfo 列表并缓存
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
            match_score: r["matchScore"].as_f64().unwrap_or(0.0),
            credit_score: r["creditScore"].as_i64().unwrap_or(0),
            capability_summary: r["capabilitySummary"].as_str().unwrap_or("").to_string(),
            completed_task_count: r["completedTaskCount"].as_i64().unwrap_or(0),
            support_a2mcp: r["supportA2MCP"].as_bool().unwrap_or(false),
            services,
        }
    }).collect();

    negotiate::save(job_id, providers.clone())?;

    // 输出列表
    println!("推荐卖家列表（共 {} 个，已缓存，当前 index=0）：", providers.len());
    for (i, p) in providers.iter().enumerate() {
        print_provider(i, p);
    }

    // 路由指引：告诉 Agent 当前 provider 走 x402 还是协商
    if let Some(first) = providers.first() {
        print_routing_guide(first, job_id);
    }
    Ok(())
}

/// --current：返回当前 provider
pub fn handle_recommend_current(job_id: &str) -> Result<()> {
    let state = negotiate::load(job_id)?;
    match state.providers.get(state.current_index) {
        Some(p) => {
            println!("当前协商卖家（index={}，共 {} 个）：", state.current_index, state.providers.len());
            print_provider(state.current_index, p);
            print_routing_guide(p, job_id);
        }
        None => {
            println!("推荐列表已全部遍历（{}/{}），无更多卖家", state.current_index, state.providers.len());
        }
    }
    Ok(())
}

/// --next：推进到下一个 provider
pub fn handle_recommend_next(job_id: &str) -> Result<()> {
    match negotiate::next(job_id)? {
        Some(p) => {
            let state = negotiate::load(job_id)?;
            println!("切换到下一个卖家（index={}，共 {} 个）：", state.current_index, state.providers.len());
            print_provider(state.current_index, &p);
            print_routing_guide(&p, job_id);
        }
        None => {
            let state = negotiate::load(job_id)?;
            println!("推荐列表已全部遍历（{}/{}），无更多卖家", state.current_index, state.providers.len());
            println!("建议：onchainos agent set-public {job_id} 或 onchainos agent close {job_id}");
        }
    }
    Ok(())
}

/// 输出路由指引：x402 直接 accept vs A2A 走协商
fn print_routing_guide(p: &negotiate::ProviderInfo, job_id: &str) {
    println!();
    if p.support_a2mcp {
        // x402 路径：无需协商，直接 confirm-accept
        let svc = p.services.first();
        let endpoint = svc.map(|s| s.endpoint.as_str()).unwrap_or("<endpoint>");
        let fee = svc.map(|s| s.fee_amount).unwrap_or(0.0);
        let symbol = svc
            .map(|s| if s.fee_token_symbol.is_empty() { "USDT" } else { s.fee_token_symbol.as_str() })
            .unwrap_or("USDT");
        println!("  ⚡ 路由: x402（无需协商，直接接单）");
        println!("  → onchainos agent confirm-accept {job_id} --provider {} --payment-mode x402 --token-symbol {symbol} --token-amount {fee} --endpoint {endpoint}", p.provider_agent_id);
    } else {
        // A2A 路径：需要协商
        println!("  💬 路由: A2A（需协商）");
        println!("  → 先调 xmtp_start_conversation 与卖家 {} 建群，再通过 xmtp_send 协商任务详情 / 价格 / 支付方式，等待 provider_applied", p.provider_agent_id);
    }
    println!();
}

fn print_provider(index: usize, p: &negotiate::ProviderInfo) {
    println!("  {}. AgentID: {}  匹配分: {}  信用分: {}  已完成: {}",
        index + 1, p.provider_agent_id, p.match_score, p.credit_score, p.completed_task_count,
    );
    println!("     能力: {}", p.capability_summary);
    println!("     地址: {}", p.provider_address);
    if p.support_a2mcp {
        println!("     支付方式: x402");
    } else {
        println!("     支付方式: escrow/direct");
    }
    if !p.services.is_empty() {
        println!("     服务 ({}):", p.services.len());
        for svc in &p.services {
            println!("       - [{}] {} ({})", svc.service_type, svc.service_name, svc.service_id);
            if svc.fee_amount > 0.0 {
                let sym = if svc.fee_token_symbol.is_empty() { &svc.fee_token } else { &svc.fee_token_symbol };
                println!("         费用: {} {}", svc.fee_amount, sym);
            }
            println!("         endpoint: {}", svc.endpoint);
        }
    }
}
