//! 仲裁者首次质押（身份 skill 跳转入口）— onchainos agent evaluator stake

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// Evaluator OKB staking — onboarding handoff from identity skill.
///
/// API: POST /priapi/v1/aieco/task/staking/stake
/// - Body: `{ "amount": "<OKB 金额, UI 单位不带精度>" }`
/// - Headers: agenticId (interceptor 校验 evaluator 身份)
/// - Backend bundles approve(VoterStaking, amount) + stake(amount, agentId) as one
///   atomic UOP (AA executeBatch), returns uopData for signing.
///
/// 累计门槛规则语义：合约层按**累计**校验 `当前地址质押金额 + 本次质押金额 >= 100 OKB`，
/// 不足则 revert。首次质押场景天然等价于"本次 >= 100"；被 slash 后余额低于 100 时
/// 追加质押也须一次性补齐到 100。
///
/// Error codes:
///   4000 — agentId 无效 / 非 evaluator 身份
///   2004 — agentId 无 evaluator 身份 (identity=2)
///   1001 — 累计质押 < 最低门槛（当前 100 OKB；合约/后端权威）
///
// TODO(backend-config): 最低累计质押门槛 100 OKB 当前是合约硬规则；
// `/staking/config` 上线后读取 `minCumulativeStakeOkb`（字段名待定）替换，
// 并在 CLI 本地拉 `stakedBalance` 做预检 `stakedBalance + amount < min` → 友好提示。
// 参见 skills/okx-agent-task/evaluator.md §13。
pub async fn handle_stake(client: &mut TaskApiClient, amount: &str) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 500）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

    let path = "/priapi/v1/aieco/task/staking/stake";
    let body = serde_json::json!({ "amount": trimmed });
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    // staking 不关联具体 jobId，用空字符串作 broadcast 的 bizContext.jobId。
    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::Stake,
    )
    .await?;

    println!("stake submitted (agentId={agent_id})");
    println!("  amount:  {trimmed} OKB");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!(
        "next: 等待 `staked` 事件（VoterStaking.Staked 上链）确认质押生效；\n\
         生效后 agentId={agent_id} 成为活跃仲裁者候选，可被选入陪审。"
    );
    Ok(())
}
