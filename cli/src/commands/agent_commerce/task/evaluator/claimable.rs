use anyhow::Result;

use crate::commands::agent_commerce::task::common::{
    claim as common_claim, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

/// 查询当前 evaluator 账户可领取的奖励（跨 dispute 聚合）。
/// - 0 金额的代币也会出现在列表里（后端返回全量统计）
/// 发现有非 0 奖励时，调 `arbitration-claim`（account 级 pull，无 jobId）一次领走全部。
pub async fn handle_claimable(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let (_account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let has_nonzero =
        common_claim::fetch_and_print_claimable(client, &agent_id, &address).await?;

    if has_nonzero {
        println!("\nnext: 有可领奖励 — 跟我说『领取奖励』即可一次性提走，确认上链后入账。");
    } else {
        println!("\n(当前无待领奖励)");
    }
    Ok(())
}
