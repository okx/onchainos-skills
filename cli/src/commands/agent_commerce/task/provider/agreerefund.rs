//! 卖家同意退款
//!
//! 卖家动作：同意退款 — onchainos agent agree-refund

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// agree-refund — 同意退款
///
/// 1. POST agreeRefund API（带身份头）→ 获取 uopData
/// 2. 签名 uopData + 广播上链
pub async fn handle_agree_refund(
    client: &TaskApiClient,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "agreeRefund"), &body, &agent_id, &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::TaskRefuse,
    ).await?;

    println!("✓ 已同意退款，等待链上确认（TASK_REJECTED）");
    println!("  txHash: {tx_hash}");
    Ok(())
}
