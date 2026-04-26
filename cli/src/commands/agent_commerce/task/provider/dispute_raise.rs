//! 发起仲裁（卖家）— onchainos agent dispute raise <jobId> --reason "..."

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_dispute_raise(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;
    // 后端 spec：Request 空 {}，无 reason / evidence 字段（证据纯链下，走 /evidence/upload 多 part）
    // reason 仅作 user-facing log 文本，不上链
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "dispute"), &body, &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::DisputeCreate,
    ).await?;

    println!("✓ 已发起仲裁，等待链上确认（job_disputed）");
    println!("  原因: {reason}");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动，不要主动给买家发消息：");
    println!("    - 禁止立即调 `xmtp_send` 告诉买家 \"已发起仲裁\" 等文字");
    println!("    - 链上确认后会收到 `job_disputed` 系统通知");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus job_disputed --role provider`，");
    println!("      按输出提示上传证据 / 通知对方");
    Ok(())
}
