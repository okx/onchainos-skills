//! 拉取推荐任务（Provider 主动发现 Public 任务）
//!
//! 对应后端 `POST /priapi/v1/aieco/task/job/match`。
//! 身份从 X-Agent-Id / X-Wallet-Address 头获取，无请求体。
//! 后端根据 provider 的 skill 描述匹配相关的 Public 任务列表。

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_recommend_task(client: &mut TaskApiClient, agent_id: Option<&str>) -> Result<()> {
    let (_, address) = signing::resolve_wallet(None, None)?;
    let agent_id = agent_id.unwrap_or("");

    let url = format!("{}/priapi/v1/aieco/task/job/match", client.base_url());
    let resp = client
        .post_with_identity(&url, &serde_json::json!({}), agent_id, &address)
        .await?;

    let tasks = resp["tasks"].as_array().cloned().unwrap_or_default();
    let agent_label = if agent_id.is_empty() { "(default)" } else { agent_id };

    if tasks.is_empty() {
        println!("【Agent {agent_label}】 无匹配任务");
        return Ok(());
    }

    println!("【Agent {agent_label}】 匹配到 {} 个 Public 任务：\n", tasks.len());
    for (i, t) in tasks.iter().enumerate() {
        let token_amount = t["tokenAmount"].as_str().unwrap_or("?");
        let token_addr = t["tokenAddress"].as_str().unwrap_or("");
        let min_credit = t["minCreditScore"].as_f64().unwrap_or(0.0);
        println!("  {}. jobId: {}", i + 1, t["jobId"].as_str().unwrap_or("?"));
        println!("     标题:     {}", t["title"].as_str().unwrap_or("?"));
        println!("     描述:     {}", t["description"].as_str().unwrap_or("?"));
        println!("     预算:     {token_amount}（token: {token_addr}）");
        println!("     最低信用: {min_credit}");
        println!("     创建时间: {}", t["createTime"].as_str().unwrap_or("?"));
        println!();
    }
    Ok(())
}
