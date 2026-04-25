//! Evaluator 解质押生命周期 CLI。
//!
//! 对齐后端 staking API §12166–§12572：
//! - `request-unstake --amount N` → POST /staking/requestUnstake（进入 7 天冷却）
//! - `claim-unstake`              → POST /staking/claimUnstake（冷却期后提走）
//! - `cancel-unstake`             → POST /staking/cancelUnstake（冷却期内取消）
//!
//! 三者都是 AA UOP：后端返回 uopData，CLI 签名 + 广播。无 jobId 绑定，bizContext.jobId=""。
//
// TODO(backend-config): 7 天冷却期当前是合约硬编码；`/staking/config` 上线后应读
// `unstakeCooldownSeconds` 并用在所有用户可见提示里。参见 evaluator.md §13。
//
// TODO(backend-config): 部分赎回保留规则 规定部分赎回后余额最低保留 100 OKB。
// 当前前置校验写死 100，`/staking/config` 上线后应改读 `partialUnstakeMinRetainOkb`，
// 并在发起前查当前质押余额 `balance - amount < min` 则拒绝并引导全额赎回。
// 现阶段合约/后端为权威校验源，CLI 层仅做 UX 前置提示，不阻塞。

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 申请解质押，OKB 进入 7 天冷却期。支持部分解质押。活跃仲裁期间会 revert。
///
/// 部分赎回保留规则：部分赎回后余额最低保留 100 OKB（低于此额只允许全额赎回）。
/// 当前 CLI 不做前置校验，依赖合约 revert；`/staking/config` 上线后改为本地预检（见下方 TODO）。
///
/// Error codes: 4000（agentId 无效）/ 1001（amount <= 0）/ 合约 revert（余额不足 / 活跃争议 / 已在冷却 / 部分赎回后余额 < 保留值）
pub async fn handle_request_unstake(client: &mut TaskApiClient, amount: &str) -> Result<()> {
    // TODO(backend-config): 部分赎回保留规则 的"部分赎回最低保留"本地预检。上线步骤：
    //   1) 从 `/priapi/v1/aieco/task/staking/config` 拉 `partialUnstakeMinRetainOkb`
    //   2) 从 `/priapi/v1/aieco/task/staking/info`（或类似）拉当前质押余额 `stakedBalance`
    //   3) 若 `stakedBalance - amount < partialUnstakeMinRetainOkb && stakedBalance - amount > 0`
    //      → bail!("部分赎回后余额不足最低保留值，请选择全额赎回")
    // 当前占位：不做本地校验，直接发 UOP，让合约 revert 把关。
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 50）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

    let path = "/priapi/v1/aieco/task/staking/requestUnstake";
    let body = serde_json::json!({ "amount": trimmed });
    let resp = client
        .post_with_identity(path, &body, &agent_id, &address)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::UnstakeRequest,
    )
    .await?;

    println!("request-unstake submitted (agentId={agent_id})");
    println!("  amount:  -{trimmed} OKB（申请中）");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!(
        "next: 等待 `unstake_requested` 事件（VoterStaking.UnstakeRequested 上链）。\n\
         事件 payload 会带 `availableAt`（7 天后），到点后跑 `evaluator claim-unstake` 领取；\n\
         冷却期内若改主意可跑 `evaluator cancel-unstake` 撤销。"
    );
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
        .post_with_identity(path, &body, &agent_id, &address)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::UnstakeClaim,
    )
    .await?;

    println!("claim-unstake submitted (agentId={agent_id})");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 等待 `unstake_claimed` 事件（VoterStaking.UnstakeClaimed 上链）确认到账。");
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
        .post_with_identity(path, &body, &agent_id, &address)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::UnstakeCancel,
    )
    .await?;

    println!("cancel-unstake submitted (agentId={agent_id})");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 等待 `unstake_cancelled` 事件（VoterStaking.UnstakeCancelled 上链），stake 将恢复。");
    Ok(())
}
