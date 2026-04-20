//! 卖家提交交付物
//!
//! 卖家动作：交付 — onchainos agent deliver

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// deliver — 提交交付物（单签：submit API → calldata → 签名 → 广播）
pub async fn handle_deliver(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    file: &str,
    message: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/submit");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({
        "deliverable": file,
        "message": message,
    });

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;

    println!("✓ 交付物已提交，等待链上确认（TASK_SUBMITTED）");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
