//! 发起仲裁（卖家）第二步 — onchainos agent dispute confirm <jobId>
//!
//! 仲裁两阶段链上流程的第二步。前置条件：
//!   1. 已跑过 `dispute raise`（阶段 1 approve 上链）
//!   2. 已收到链上 `dispute_approved` 系统通知
//!
//! 本命令调 POST /aieco/task/{jobId}/dispute → uopData → sign + broadcast，
//! 完成后等链上 `job_disputed` 通知，再调 next-action 进入证据准备期。

use anyhow::{bail, Context, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_dispute_confirm(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
    }
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let body = serde_json::json!({});

    let dispute_resp = client.post_with_identity(
        &client.endpoint(job_id, "dispute"), &body, agent_id,
    ).await
        .context("dispute confirm (阶段 2): dispute 接口请求失败")?;

    let dispute_tx = signing::sign_uop_and_broadcast(
        client, &dispute_resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&dispute_resp), agent_id,
    ).await
        .context("dispute confirm (阶段 2): dispute 上链失败")?;

    println!("✓ 仲裁阶段 2: dispute 上链");
    println!("  txHash: {dispute_tx}");
    println!();
    println!("⚠️  阶段 2 已完成，**结束本轮 turn**，等待链上 `job_disputed` 系统通知：");
    println!("    - 禁止立即给买家 xmtp_send 任何「已发起仲裁」消息");
    println!("    - 收到 `job_disputed` 通知后再走证据上传剧本");
    Ok(())
}
