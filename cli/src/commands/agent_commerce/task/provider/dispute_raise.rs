//! 发起仲裁（卖家）第一步 — onchainos agent dispute raise <jobId> --reason "..."
//!
//! 仲裁是两阶段链上流程，每阶段独立 tx + 独立链事件：
//!   阶段 1（本命令）：POST /aieco/task/{jobId}/dispute/approve  → ERC-20 token approve 给 dispute 合约
//!                     → 等链上 `dispute_approved` 系统通知
//!   阶段 2（dispute confirm 命令）：POST /aieco/task/{jobId}/dispute → 实际发起仲裁
//!                     → 等链上 `job_disputed` 系统通知
//!
//! 本命令只跑阶段 1。完成后必须等 `dispute_approved` 通知到达，再调 `next-action`
//! 拿阶段 2 剧本，**不能在同一 turn 内连续调 dispute confirm**。
//! reason 仅作 user-facing log，不上链。

use anyhow::{bail, Context, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_dispute_raise(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
    }
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let body = serde_json::json!({});

    // POST /dispute/approve → uopData → sign + broadcast
    let approve_resp = client.post_with_identity(
        &client.endpoint(job_id, "dispute/approve"), &body, agent_id,
    ).await
        .context("dispute raise (阶段 1): dispute/approve 接口请求失败")?;

    let approve_tx = signing::sign_uop_and_broadcast(
        client, &approve_resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&approve_resp), agent_id,
    ).await
        .context("dispute raise (阶段 1): approve 上链失败")?;

    println!("✓ 仲裁阶段 1: approve 上链 (token 授权给 dispute 合约)");
    println!("  原因记录: {reason}");
    println!("  txHash: {approve_tx}");
    println!();
    println!("⚠️  阶段 1 已完成，**结束本轮 turn**，等待链上 `dispute_approved` 系统通知：");
    println!("    - 禁止立即给买家 xmtp_send 任何「已发起仲裁」消息");
    println!("    - 禁止在同一 turn 内连续调 `dispute confirm`");
    println!("    - 收到 `dispute_approved` 通知后调：");
    println!("      onchainos agent next-action --jobid {job_id} --jobStatus dispute_approved --role provider --agentId {agent_id}");
    println!("      next-action 会输出阶段 2 剧本（调 dispute confirm 触发实际仲裁）");
    Ok(())
}
