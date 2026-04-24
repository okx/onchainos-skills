//! 拒绝交付物
//!
//! 买家动作：拒绝交付物 — onchainos task refuse

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// reject/refuse — 拒绝验收
pub async fn handle_reject(
    client: &TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    let reason_owned = reason.to_string();
    let result = signing::task_dual_sign_and_broadcast(
        client,
        &client.endpoint(job_id, "pre-refuse"),
        &serde_json::json!({}),
        &client.endpoint(job_id, "refuse"),
        move |signature| serde_json::json!({
            "signature": signature,
            "reason": reason_owned,
        }),
        &account_id,
        &address,
        &agent_id,
        job_id,
        signing::BizContext::JobRefuse,
    ).await?;

    println!("✓ 已拒绝验收（原因：{reason}），状态 → refused");
    println!("  卖家有 24 小时内可申请仲裁");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
