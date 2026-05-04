//! 仲裁者追加质押（top-up / 被罚后补齐）— onchainos agent increase-stake

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// Evaluator 补充质押（top-up / 被罚后补齐）。
///
/// API: POST /priapi/v1/aieco/task/staking/increaseStake/// - Body: `{ "amount": "<OKB 金额, UI 单位>" }`（agentId 从 header 读）
/// - 后端打包 approve(VoterStaking, amount) + increaseStake(amount) 为 atomic UOP
/// - 无最低金额限制（只要 amount > 0）
///
/// Error codes:
///   4000 — agentId 无效
///   1001 — amount <= 0
///   3001 — 合约 ABI 生成失败
pub async fn handle_increase_stake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 50）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let path = "/priapi/v1/aieco/task/staking/increaseStake";
    let body = serde_json::json!({ "amount": trimmed });
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::StakeIncrease,
        &agent_id,
    )
    .await?;

    println!("increase-stake submitted (agentId={agent_id})");
    println!("  amount:  +{trimmed} OKB");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 追加质押已提交，等待链上确认到位。");
    Ok(())
}
