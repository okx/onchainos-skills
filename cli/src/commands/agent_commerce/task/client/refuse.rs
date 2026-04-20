//! 拒绝交付物
//!
//! 买家动作：拒绝交付物 — onchainos task refuse

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// reject/refuse — 拒绝验收
pub async fn handle_reject(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-refuse");
    let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/refuse");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let pre_body = serde_json::json!({});

    let reason_owned = reason.to_string();
    let result = signing::task_dual_sign_and_broadcast(
        http,
        &pre_endpoint,
        &pre_body,
        &main_endpoint,
        move |signature| serde_json::json!({
            "signature": signature,  // 【待确认】字段名
            "reason": reason_owned,
        }),
        &broadcast,
        &account_id,
        &address,
        &agent_id,
    ).await?;

    println!("✓ 已拒绝验收（原因：{reason}），状态 → refused");
    println!("  卖家有 24 小时内可申请仲裁");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
