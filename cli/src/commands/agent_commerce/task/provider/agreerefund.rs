//! 卖家同意退款
//!
//! 卖家动作：同意退款 — onchainos agent agree-refund

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// agree-refund — 同意退款（单签：agreeRefund API → calldata → 签名 → 广播）
pub async fn handle_agree_refund(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/agreeRefund");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({});

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;

    println!("✓ 已同意退款，等待链上确认（TASK_REJECTED）");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
