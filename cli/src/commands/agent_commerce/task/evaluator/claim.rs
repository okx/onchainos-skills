use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    claim as common_claim, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

pub async fn handle_claim(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let tx_hash =
        common_claim::submit_claim_and_broadcast(client, &account_id, &address, &agent_id).await?;

    audit::log(
        "cli",
        "evaluator/arbitration_claimed",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("account={address}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("reward claim submitted (account={address})");
    println!("  txHash:   {tx_hash}");
    println!("note: claims all rewards from settled disputes at once; settled amount will be notified after on-chain confirmation.");
    Ok(())
}
