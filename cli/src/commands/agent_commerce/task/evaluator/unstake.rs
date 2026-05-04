use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::staking_types::{
    self, StakingConfig,
};
use crate::commands::agent_commerce::task::signing;

pub async fn handle_request_unstake(
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

    // best-effort 拉平台配置；失败不阻塞——合约会兜底。
    let cfg = staking_types::get_staking_config(client, &agent_id).await.ok();

    let path = "/priapi/v1/aieco/task/staking/requestUnstake";
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
        signing::BizContext::UnstakeRequest,
        &agent_id,
    )
    .await?;

    println!("request-unstake submitted (agentId={agent_id})");
    println!("  amount:  -{trimmed} OKB（申请中）");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    if let Some(days) = cfg.as_ref().map(StakingConfig::unstake_cooldown_days) {
        println!(
            "next: 申请已提交，等待链上确认；确认后进入 {days} 天冷却期，到时可领取，期间可撤销。"
        );
    } else {
        println!(
            "next: 申请已提交，等待链上确认；确认后进入冷却期（天数见 staking-config），到时可领取，期间可撤销。"
        );
    }
    if let Some(c) = cfg.as_ref() {
        println!(
            "  config: 部分赎回后余额最低保留 {} OKB（低于此值只能全额赎回）",
            c.partial_unstake_min_retain_okb
        );
    }
    Ok(())
}

/// 冷却期结束后领取已解质押的 OKB。合约内部知道金额与解锁时间，请求体为空。
///
/// Error codes: 4000 / 合约 revert（未到解锁时间 / 无待解质押）
pub async fn handle_claim_unstake(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let path = "/priapi/v1/aieco/task/staking/claimUnstake";
    let body = serde_json::json!({});
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::UnstakeClaim,
        &agent_id,
    )
    .await?;

    println!("claim-unstake submitted (agentId={agent_id})");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 领取交易已提交，等待链上确认到账。");
    Ok(())
}

pub async fn handle_cancel_unstake(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let path = "/priapi/v1/aieco/task/staking/cancelUnstake";
    let body = serde_json::json!({});
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::UnstakeCancel,
        &agent_id,
    )
    .await?;

    println!("cancel-unstake submitted (agentId={agent_id})");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 取消已提交，等待链上确认；确认后质押恢复。");
    Ok(())
}
