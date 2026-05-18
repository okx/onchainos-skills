use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    claim as common_claim, network::task_api_client::TaskApiClient,
};

/// 查询当前 evaluator 账户可领取的奖励（跨 dispute 聚合）。
/// - 0 金额的代币也会出现在列表里（后端返回全量统计）
///
/// 发现有非 0 奖励时，调 `arbitration-claim`（account 级 pull，无 jobId）一次领走全部。
pub async fn handle_claimable(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let has_nonzero =
        common_claim::fetch_and_print_claimable(client, agent_id).await?;

    audit::log(
        "cli",
        "evaluator/arbitration_claimable_checked",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("hasClaimable={has_nonzero}"),
        ]),
        None,
    );

    if has_nonzero {
        println!("\nnext: rewards available — say 'claim rewards' to withdraw all at once; settles after on-chain confirm.");
        println!("hasClaimable: yes");
    } else {
        println!("\n(no claimable rewards)");
        println!("hasClaimable: no");
    }
    Ok(())
}
