//! Raise dispute (provider) step 1 — onchainos agent dispute raise <jobId> --reason "..."
//!
//! Dispute is a two-stage on-chain flow; each stage has its own tx and its own chain event:
//!   Stage 1 (this command): POST /aieco/task/{jobId}/dispute/approve → ERC-20 token approve to the dispute contract
//!                     → wait for on-chain `dispute_approved` system notification
//!   Stage 2 (dispute confirm command): POST /aieco/task/{jobId}/dispute → actually raises the dispute
//!                     → wait for on-chain `job_disputed` system notification
//!
//! This command runs stage 1 only. After completion, wait for the `dispute_approved` notification
//! before calling `next-action` to fetch the stage 2 script — **do NOT call dispute confirm in the same turn**.
//! reason is a user-facing log only; not put on-chain.

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{self, network::task_api_client::TaskApiClient};
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

    // Dispute deposit precheck: wallet's matching token balance must be ≥ 5% of the job amount.
    // Insufficient balance bails immediately to avoid wasting gas on later approve / dispute on-chain txs.
    let task_resp = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .context("dispute raise: 拉取任务详情失败（保证金预检前置）")?;
    let task_amount: f64 = task_resp["tokenAmount"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let token_symbol = task_resp["tokenSymbol"].as_str().unwrap_or("USDT");
    if task_amount > 0.0 {
        let required = task_amount * 0.05;
        common::ensure_sufficient_balance(required, token_symbol)
            .await
            .context(format!(
                "发起仲裁需要保证金 ≥ 任务金额 5%（{required} {token_symbol}，任务金额 {task_amount} {token_symbol}）"
            ))?;
    }

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

    audit::log(
        "cli",
        "provider/dispute_approve_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("reasonLen={}", reason.chars().count()),
            format!("txHash={approve_tx}"),
        ]),
        None,
    );

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
