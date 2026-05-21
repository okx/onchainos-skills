//! Start finding jobs — auto-discover available jobs (loop-match for every online Provider Agent).
//!
//! Flow:
//! 1. Call `onchainos agent get` (subprocess) → fetch all of the user's Agents.
//! 2. Filter by status=1 (online) + role=2 (provider).
//! 3. For each Agent, call `recommend-task` (POST /priapi/v1/aieco/task/job/match).
//! 4. Print matched results grouped by agent.

use anyhow::Result;
use serde_json::Value;

use crate::commands::agent_commerce::task::common::{fetch_my_agents, network::task_api_client::TaskApiClient};
use crate::commands::agent_commerce::task::signing;

/// Provider role value (backend convention: 1=buyer, 2=provider, 3=evaluator).
const ROLE_PROVIDER: i64 = 2;
/// Online status value.
const STATUS_ONLINE: i64 = 1;

pub async fn handle_find_jobs() -> Result<()> {
    // Step 1: fetch the current active account's agents — `fetch_my_agents`
    // shells out to `onchainos agent get` and filters by XLayer ownerAddress.
    let agent_list = fetch_my_agents().await;
    if agent_list.is_empty() {
        println!("⚠ 当前钱包没有已注册的 Agent。请先 `onchainos agent create --role provider ...` 创建一个。");
        return Ok(());
    }

    // Step 2: filter online providers
    let online_providers: Vec<&Value> = agent_list
        .iter()
        .filter(|a| {
            a["role"].as_i64() == Some(ROLE_PROVIDER)
                && a["status"].as_i64() == Some(STATUS_ONLINE)
        })
        .collect();

    if online_providers.is_empty() {
        println!("⚠ 没有在线的 Provider Agent。");
        println!("  共 {} 个 Agent，但 status != 1（在线）或 role != 2（provider）", agent_list.len());
        return Ok(());
    }

    println!("发现 {} 个在线 Provider Agent，开始为每个匹配任务...\n", online_providers.len());

    // Step 3: call recommend-task for each online provider agent
    let mut task_client = TaskApiClient::new();
    let _ = signing::resolve_wallet(None, None)?;
    let mut total_tasks = 0usize;
    let mut summary: Vec<(String, String, usize)> = Vec::new();

    for agent in &online_providers {
        let agent_id = agent["agentId"].as_str().unwrap_or("");
        let name = agent["name"].as_str().unwrap_or("(no name)");
        let desc = agent["profileDescription"].as_str().unwrap_or("(no description)");

        println!("━━━ Agent {agent_id} ({name}) ━━━");
        println!("  描述: {desc}");

        match fetch_tasks_for_agent(&mut task_client, agent_id).await {
            Ok(tasks) => {
                print_tasks(&tasks);
                total_tasks += tasks.len();
                summary.push((agent_id.to_string(), name.to_string(), tasks.len()));
            }
            Err(e) => {
                println!("  ⚠ 拉取失败: {e}");
                summary.push((agent_id.to_string(), name.to_string(), 0));
            }
        }
        println!();
    }

    // Step 4: summary
    println!("═══ 汇总 ═══");
    for (id, name, n) in &summary {
        println!("  Agent {id} ({name}): {n} 个任务");
    }
    println!("  合计：{total_tasks} 个任务");
    println!();
    println!("⚠️  给 LLM agent 的硬规则（必读）：");
    println!("    1. **必须按 agent 分组完整呈现给用户**——上面每个 `━━━ Agent X (...) ━━━` 段落对应一个卖家 agent，全部列给用户看，不要挑一个 agent 总结");
    println!("    2. **0 任务的 agent 也要列出**——这通常是后端匹配异常的信号，需要让用户知道（例：「Agent 410 (天气小助手3): 0 个任务」要保留）");
    println!("    3. **禁止 LLM 自己挑「最佳推荐」**——不要根据 agent 描述/任务内容自作主张排序或筛选；展示给用户原始 per-agent 分组结果");
    println!("    4. **让用户决定**：呈现完后等用户说「用 <agentId> 接 <jobId>」再启动接单流程，不要替用户选");

    Ok(())
}

/// Call the recommend-task endpoint for the specified agent.
async fn fetch_tasks_for_agent(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<Vec<Value>> {
    let resp = client
        .post_with_identity("/priapi/v1/aieco/task/job/match", &serde_json::json!({}), agent_id)
        .await?;
    Ok(resp["tasks"].as_array().cloned().unwrap_or_default())
}

fn print_tasks(tasks: &[Value]) {
    if tasks.is_empty() {
        println!("  （无匹配任务）");
        return;
    }
    for (i, t) in tasks.iter().enumerate() {
        let token_amount = t["tokenAmount"].as_str().unwrap_or("?");
        let token_addr = t["tokenAddress"].as_str().unwrap_or("");
        let token_sym = t["tokenSymbol"].as_str().unwrap_or("UNKNOWN");
        println!(
            "  {}. jobId={} | {} | 预算 {} {} (token: {})",
            i + 1,
            t["jobId"].as_str().unwrap_or("?"),
            t["title"].as_str().unwrap_or("?"),
            token_amount,
            token_sym,
            token_addr,
        );
    }
}
