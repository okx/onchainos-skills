//! 开始接单 — 自动发现可接任务（对所有在线 Provider Agent 循环匹配）
//!
//! 流程：
//! 1. 调用 `onchainos agent get`（子进程）→ 拉取用户所有 Agent
//! 2. 过滤 status=1（在线）+ role=2（provider）
//! 3. 对每个 Agent 调用 `recommend-task`（POST /priapi/v1/aieco/task/job/match）
//! 4. 按 agent 分组打印匹配结果

use anyhow::{bail, Result};
use serde_json::Value;
use tokio::process::Command;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// Provider 角色值（后端约定：1=buyer, 2=provider, 3=evaluator）
const ROLE_PROVIDER: i64 = 2;
/// 在线状态值
const STATUS_ONLINE: i64 = 1;

pub async fn handle_find_jobs() -> Result<()> {
    // Step 1: 调用 `onchainos agent get` 子进程，获取当前钱包的 Agent 列表
    let agent_list = invoke_agent_get().await?;
    if agent_list.is_empty() {
        println!("⚠ 当前钱包没有已注册的 Agent。请先 `onchainos agent create --role provider ...` 创建一个。");
        return Ok(());
    }

    // Step 2: 过滤 online provider
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

    // Step 3: 对每个 online provider agent 调 recommend-task
    let mut task_client = TaskApiClient::new();
    let (_, address) = signing::resolve_wallet(None, None)?;
    let mut total_tasks = 0usize;
    let mut summary: Vec<(String, String, usize)> = Vec::new();

    for agent in &online_providers {
        let agent_id = agent["agentId"].as_str().unwrap_or("");
        let name = agent["name"].as_str().unwrap_or("(no name)");
        let desc = agent["profileDescription"].as_str().unwrap_or("(no description)");

        println!("━━━ Agent {agent_id} ({name}) ━━━");
        println!("  描述: {desc}");

        match fetch_tasks_for_agent(&mut task_client, agent_id, &address).await {
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

    // Step 4: 汇总
    println!("═══ 汇总 ═══");
    for (id, name, n) in &summary {
        println!("  Agent {id} ({name}): {n} 个任务");
    }
    println!("  合计：{total_tasks} 个任务");
    println!();
    println!("选择任务后，执行：");
    println!("  onchainos agent common context <jobId> --role seller   # 拿买家 agentId");
    println!("  onchainos agent contact-buyer --to <buyerAgentId> --job-id <jobId>");

    Ok(())
}

/// Spawn `onchainos agent get` 子进程，解析其 stdout JSON 取 `data.list`。
///
/// 使用 `std::env::current_exe()` 确保调自身（不依赖 PATH）。
async fn invoke_agent_get() -> Result<Vec<Value>> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("无法获取当前可执行文件路径: {e}"))?;
    let output = Command::new(&exe)
        .args(["agent", "get"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("调用 `onchainos agent get` 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`onchainos agent get` 退出码 {}: {stderr}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("解析 `onchainos agent get` 输出失败: {e}\n原始输出:\n{stdout}"))?;

    if parsed["ok"] != true {
        bail!(
            "`onchainos agent get` 返回失败: {}",
            parsed["error"].as_str().unwrap_or("unknown error")
        );
    }

    // data 可能是 { list: [...] } 或数组 [{ list: [...] }]（后端兼容格式）
    let data = &parsed["data"];
    let list = if data.is_array() {
        data.get(0)
            .and_then(|v| v.get("list"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        data["list"].as_array().cloned().unwrap_or_default()
    };
    Ok(list)
}

/// 对指定 agent 调 recommend-task 接口
async fn fetch_tasks_for_agent(
    client: &mut TaskApiClient,
    agent_id: &str,
    address: &str,
) -> Result<Vec<Value>> {
    let url = format!("{}/priapi/v1/aieco/task/job/match", client.base_url());
    let resp = client
        .post_with_identity(&url, &serde_json::json!({}), agent_id, address)
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
        println!(
            "  {}. jobId={} | {} | 预算 {} (token: {})",
            i + 1,
            t["jobId"].as_str().unwrap_or("?"),
            t["title"].as_str().unwrap_or("?"),
            token_amount,
            token_addr,
        );
    }
}
