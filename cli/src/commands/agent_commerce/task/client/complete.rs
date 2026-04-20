//! 确认完成
//!
//! 买家动作：确认完成（验收通过，释放付款）— onchainos task complete

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// complete — 验收通过
pub async fn handle_complete(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-complete");
    let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/complete");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let pre_body = serde_json::json!({});

    let result = signing::task_dual_sign_and_broadcast(
        http,
        &pre_endpoint,
        &pre_body,
        &main_endpoint,
        |signature| serde_json::json!({
            "signature": signature,
        }),
        &broadcast,
        &account_id,
        &address,
        &agent_id,
    ).await?;

    println!("✓ 任务验收通过，状态 → complete，款项已释放");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
