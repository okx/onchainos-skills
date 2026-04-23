//! 卖家提交交付物
//!
//! 卖家动作：交付 — onchainos agent deliver

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// deliver — 提交交付物
///
/// 1. POST submit API（带身份头）→ 获取 uopData
/// 2. 签名 uopData + 广播上链
pub async fn handle_deliver(
    client: &TaskApiClient,
    job_id: &str,
    file: &str,
    message: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;
    let body = serde_json::json!({
        "deliverable": file,
        "message": message,
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "submit"), &body, &agent_id, &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::TaskSubmit,
    ).await?;

    println!("✓ 交付物已提交，等待链上确认（TASK_SUBMITTED）");
    println!("  txHash: {tx_hash}");
    Ok(())
}
