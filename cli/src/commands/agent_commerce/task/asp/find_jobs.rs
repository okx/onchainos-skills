//! Start finding jobs — auto-discover available jobs (loop-match for every online ASP Agent).
//!
//! Flow:
//! 1. Call `onchainos agent get-my-agents` (subprocess) → fetch all of the user's Agents.
//! 2. Filter by status=1 (online) + role=2 (ASP).
//! 3. For each Agent, call `recommend-task` (POST /priapi/v1/aieco/task/job/match).
//! 4. Print matched results grouped by agent.

use anyhow::Result;
use serde_json::Value;

use crate::commands::agent_commerce::task::common::{fetch_my_agents, network::task_api_client::TaskApiClient};
use crate::commands::agent_commerce::task::signing;

/// ASP role value (backend convention: 1=User Agent, 2=ASP, 3=evaluator).
const ROLE_ASP: i64 = 2;
/// Online status value.
const STATUS_ONLINE: i64 = 1;

pub async fn handle_find_jobs() -> Result<()> {
    // Step 1: fetch the current active account's agents — `fetch_my_agents`
    // shells out to `onchainos agent get-my-agents` and filters by XLayer ownerAddress.
    let agent_list = fetch_my_agents().await;
    if agent_list.is_empty() {
        println!("⚠ No registered Agents found for the current wallet. Please create one first.");
        return Ok(());
    }

    // Step 2: filter online ASPs
    let online_asps: Vec<&Value> = agent_list
        .iter()
        .filter(|a| {
            a["role"].as_i64() == Some(ROLE_ASP)
                && a["status"].as_i64() == Some(STATUS_ONLINE)
        })
        .collect();

    if online_asps.is_empty() {
        println!("⚠ No online ASP Agents found.");
        println!("  Total {} Agent(s), but status != 1 (online) or role != 2 (ASP)", agent_list.len());
        return Ok(());
    }

    println!("Found {} online ASP Agent(s), matching tasks for each...\n", online_asps.len());

    // Step 3: call recommend-task for each online ASP agent
    let mut task_client = TaskApiClient::new();
    let _ = signing::resolve_wallet(None, None)?;
    let mut total_tasks = 0usize;
    let mut summary: Vec<(String, String, usize)> = Vec::new();

    for agent in &online_asps {
        let agent_id = agent["agentId"].as_str().unwrap_or("");
        let name = agent["name"].as_str().unwrap_or("(no name)");
        let desc = agent["profileDescription"].as_str().unwrap_or("(no description)");

        println!("━━━ Agent {agent_id} ({name}) ━━━");
        println!("  Description: {desc}");

        match fetch_tasks_for_agent(&mut task_client, agent_id).await {
            Ok(tasks) => {
                print_tasks(&tasks);
                total_tasks += tasks.len();
                summary.push((agent_id.to_string(), name.to_string(), tasks.len()));
            }
            Err(e) => {
                println!("  ⚠ Fetch failed: {e}");
                summary.push((agent_id.to_string(), name.to_string(), 0));
            }
        }
        println!();
    }

    // Step 4: summary
    println!("═══ Summary ═══");
    for (id, name, n) in &summary {
        println!("  Agent {id} ({name}): {n} task(s)");
    }
    println!("  Total: {total_tasks} task(s)");
    println!();
    println!("⚠️  Hard rules for the LLM agent (must read):");
    println!("    1. **Present results grouped by agent in full** — each `━━━ Agent X (...) ━━━` section above corresponds to one ASP agent; show all of them to the user, do not cherry-pick one agent to summarize");
    println!("    2. **List agents with 0 tasks too** — this is typically a signal of backend matching anomalies and the user needs to know (e.g., \"Agent 410 (WeatherHelper3): 0 task(s)\" should be kept)");
    println!("    3. **Do NOT pick a \"best recommendation\"** — do not sort or filter based on agent description/task content on your own; present the raw per-agent grouped results to the user");
    println!("    4. **Let the user decide**: after presenting, wait for the user to say \"use <agentId> to accept <jobId>\" before starting the accept flow — do not choose for the user");

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
        println!("  (no matching tasks)");
        return;
    }
    for (i, t) in tasks.iter().enumerate() {
        let token_amount = t["tokenAmount"].as_str().unwrap_or("?");
        let token_addr = t["tokenAddress"].as_str().unwrap_or("");
        let token_sym = t["tokenSymbol"].as_str().unwrap_or("UNKNOWN");
        println!(
            "  {}. jobId={} | {} | Budget {} {} (token: {})",
            i + 1,
            t["jobId"].as_str().unwrap_or("?"),
            t["title"].as_str().unwrap_or("?"),
            token_amount,
            token_sym,
            token_addr,
        );
    }
}
