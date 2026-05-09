use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::stake;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_increase_stake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    let trimmed = stake::validate_amount(amount, "50")?;

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let (tx_hash, endpoint) =
        stake::execute_stake_or_increase(client, trimmed, &account_id, &address, &agent_id)
            .await?;

    println!("increase-stake submitted (agentId={agent_id}, via={endpoint})");
    println!("  amount:  +{trimmed} OKB");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 追加质押已提交，等待链上确认到位。");
    Ok(())
}
