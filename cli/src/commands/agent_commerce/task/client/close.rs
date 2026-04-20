//! 关单
//!
//! 买家动作：关单 — onchainos task close
//! 附带：领取仲裁奖金 (task claim)

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// close — 关闭任务
pub async fn handle_close(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/close");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({});

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;

    println!("✓ 任务已关闭，状态 → close");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

/// claim — 仲裁奖金领取
pub async fn handle_claim(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/claim");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({ "jobId": job_id });

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;

    println!("✓ 仲裁奖金已领取");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
