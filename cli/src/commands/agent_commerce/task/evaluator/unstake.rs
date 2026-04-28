//! Evaluator 解质押生命周期 CLI。
//!
//! 对齐后端 staking API §12166–§12572：
//! - `request-unstake --amount N` → POST /staking/requestUnstake（进入冷却期）
//! - `claim-unstake`              → POST /staking/claimUnstake（冷却期后提走）
//! - `cancel-unstake`             → POST /staking/cancelUnstake（冷却期内取消）
//!
//! 三者都是 AA UOP：后端返回 uopData，CLI 签名 + 广播。无 jobId 绑定，bizContext.jobId=""。
//!
//! 冷却期天数与"部分赎回最低保留"由 `/staking/config` 提供（Apollo 配置，后端权威），
//! CLI 在发起前 best-effort 拉取做 UX 提示；拉取失败不阻塞主流程，由合约 revert 兜底。

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::{
    StakingConfig, TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

/// 申请解质押，OKB 进入冷却期。支持部分解质押。活跃仲裁期间会 revert。
///
/// 部分赎回保留规则：部分赎回后余额最低保留 `partialUnstakeMinRetainOkb` OKB
/// （低于此值只允许全额赎回）。CLI 在发起前 best-effort 拉 `/staking/config`，
/// 用于 UX 文案与（已知本地余额时的）友好预检；最终校验仍以合约 revert 为准。
///
/// Error codes: 4000（agentId 无效）/ 1001（amount <= 0）/ 合约 revert（余额不足 / 活跃争议 / 已在冷却 / 部分赎回后余额 < 保留值）
pub async fn handle_request_unstake(client: &mut TaskApiClient, amount: &str) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 50）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

    // best-effort 拉平台配置；失败不阻塞——合约会兜底。
    let cfg = client.get_staking_config(&agent_id).await.ok();

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
    let cooldown_days = cfg.as_ref().map(StakingConfig::unstake_cooldown_days).unwrap_or(7);
    println!(
        "next: 申请已提交，等待链上确认；确认后进入 {cooldown_days} 天冷却期，到时可领取，期间可撤销。"
    );
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
pub async fn handle_claim_unstake(client: &mut TaskApiClient) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

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

/// 在 7 天冷却期内撤销解质押请求，OKB 回到质押状态。
///
/// Error codes: 4000 / 合约 revert（无待解质押 / 冷却期已过）
pub async fn handle_cancel_unstake(client: &mut TaskApiClient) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

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
