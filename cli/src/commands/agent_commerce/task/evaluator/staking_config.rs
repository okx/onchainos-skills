//! 仲裁者读取平台质押 & 仲裁配置（只读）— onchainos agent staking-config
//!
//! API: GET /priapi/v1/aieco/task/staking/config
//! - Headers: Authorization (JWT) + `agenticId`（后端 interceptor 校验 evaluator 身份）；无 Body
//! - 返回 Apollo `aitask.platform.*` 配置，重启生效，CLI/agent 模板都按此渲染
//!
//! 唯一允许 `--agent-id` 可选的 evaluator 命令：因为这是 platform-level 只读 API，
//! 不签名、不动钱包，仅需 agenticId 头过 interceptor。CLI 只解析 agentId（不走
//! `resolve_wallet_and_agent_for_evaluator`），传 `--agent-id` 直接用，否则按
//! evaluator role 在本地身份列表里反查首个匹配项。

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::AGENT_ROLE_EVALUATOR;
use crate::commands::agent_commerce::task::evaluator::staking_types;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_staking_config(
    client: &mut TaskApiClient,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    //todo 应该不需要agentid
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
