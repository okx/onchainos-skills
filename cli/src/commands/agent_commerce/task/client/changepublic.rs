//! 设为 Public
//!
//! 买家动作：设为 Public — onchainos task set-public

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// set-public — 转为公开任务
pub async fn handle_set_public(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/setVisibility");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({"visibility": 1});

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;

    println!("✓ 任务已转为公开，其他卖家可以看到并报名");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
