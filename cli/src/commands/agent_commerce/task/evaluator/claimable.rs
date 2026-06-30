use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    claim as common_claim, network::task_api_client::TaskApiClient,
};

/// Query claimable rewards for the current evaluator account (aggregated across disputes).
/// - Zero-amount tokens still appear in the list (the backend returns the full breakdown).
///
/// When non-zero rewards are found, call `arbitration-claim` (an account-level
/// pull, no jobId) to sweep everything in a single call.
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
