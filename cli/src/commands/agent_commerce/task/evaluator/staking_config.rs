use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::staking_types;

pub async fn handle_staking_config(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let cfg = staking_types::get_staking_config(client, agent_id).await?;
    println!("staking & arbitration config");
    println!("  minCumulativeStakeOkb       : {} OKB", cfg.min_cumulative_stake_okb);
    println!("  partialUnstakeMinRetainOkb  : {} OKB", cfg.partial_unstake_min_retain_okb);
    println!("  unstakeCooldownDays         : {}", cfg.unstake_cooldown_days());
    println!("  arbitrationFeeBps           : {}", cfg.arbitration_fee_bps);
    println!("  commitPhaseHours            : {}", cfg.commit_phase_hours());
    println!("  revealPhaseHours            : {}", cfg.reveal_phase_hours());
    println!("  slashMinorityBps            : {}", cfg.slash_minority_bps);
    println!("  slashTimeoutBps             : {}", cfg.slash_timeout_bps);
    println!("  slashedCooldownHours        : {}", cfg.slashed_cooldown_hours());
    Ok(())
}
