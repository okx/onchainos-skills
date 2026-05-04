use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::AGENT_ROLE_EVALUATOR;
use crate::commands::agent_commerce::task::evaluator::staking_types;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_staking_config(
    client: &mut TaskApiClient,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    //todo zhangxin 应该不需要agentid
    let agent_id = match agent_id_hint.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => id.to_string(),
        None => {
            let id = signing::resolve_agent_id_by_role(AGENT_ROLE_EVALUATOR).await?;
            if id.is_empty() {
                bail!(
                    "当前账户没有 evaluator 身份，无法查 staking-config；\
                     请先注册 evaluator 或显式传 --agent-id"
                );
            }
            id
        }
    };
    let cfg = staking_types::get_staking_config(client, &agent_id).await?;
    println!("staking & arbitration config");
    println!("  minCumulativeStakeOkb       : {} OKB", cfg.min_cumulative_stake_okb);
    println!("  partialUnstakeMinRetainOkb  : {} OKB", cfg.partial_unstake_min_retain_okb);
    println!(
        "  unstakeCooldownSeconds      : {} ({} 天)",
        cfg.unstake_cooldown_seconds,
        cfg.unstake_cooldown_days(),
    );
    println!("  arbitrationFeeBps           : {}", cfg.arbitration_fee_bps);
    println!(
        "  commitPhaseSeconds          : {} ({} 小时)",
        cfg.commit_phase_seconds,
        cfg.commit_phase_hours(),
    );
    println!(
        "  revealPhaseSeconds          : {} ({} 小时)",
        cfg.reveal_phase_seconds,
        cfg.reveal_phase_hours(),
    );
    println!("  slashMinorityBps            : {}", cfg.slash_minority_bps);
    println!("  slashTimeoutBps             : {}", cfg.slash_timeout_bps);
    println!(
        "  slashedCooldownSeconds      : {} ({} 小时)",
        cfg.slashed_cooldown_seconds,
        cfg.slashed_cooldown_seconds / 3600,
    );
    Ok(())
}
