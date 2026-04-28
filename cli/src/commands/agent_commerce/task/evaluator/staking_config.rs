//! 仲裁者读取平台质押 & 仲裁配置（只读）— onchainos agent evaluator staking-config
//!
//! API: GET /priapi/v1/aieco/task/staking/config
//! - Headers: Authorization (JWT) + `agenticId`（后端 interceptor 校验 evaluator 身份）；无 Body
//! - 返回 Apollo `aitask.platform.*` 配置，重启生效，CLI/agent 模板都按此渲染

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_staking_config(client: &mut TaskApiClient) -> Result<()> {
    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;
    let cfg = client.get_staking_config(&agent_id).await?;
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
