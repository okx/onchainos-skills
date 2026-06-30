use anyhow::Result;
use chrono::TimeZone;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;
use crate::commands::agent_commerce::task::evaluator::staking_types;

/// Render a unix-seconds timestamp. `0` means "not applicable".
fn fmt_unix_seconds(ts: i64, none_label: &str) -> String {
    if ts == 0 {
        format!("0 ({none_label})")
    } else if let Some(dt) = chrono::Local.timestamp_opt(ts, 0).single() {
        format!("{ts} ({})", dt.format("%Y-%m-%d %H:%M:%S %Z"))
    } else {
        format!("{ts} (unparseable)")
    }
}

pub async fn handle_my_stake(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let s = staking_types::get_my_stake(client, agent_id).await?;

    println!("my stake (on-chain staking state)");
    println!("  voter address      : {}", s.voter_address);
    println!(
        "  agentId            : {} (registered={})",
        s.agent_id, s.registered
    );
    println!(
        "  activeStake        : {} OKB  # currently staked (net of slashing)",
        s.active_stake_okb
    );
    println!(
        "  pendingUnstake     : {} OKB  # in cooldown, awaiting unlock",
        s.pending_unstake_okb
    );
    println!(
        "  validStake         : {} OKB  # weight-eligible = activeStake - pendingUnstake",
        s.valid_stake_okb
    );
    println!(
        "  activeDisputes     : {}  # disputes in progress (unstake blocked while >0)",
        s.active_disputes
    );
    println!(
        "  unstakeAvailableAt : {}",
        fmt_unix_seconds(s.unstake_available_at, "no pending unstake")
    );
    println!(
        "  cooldownEndsAt     : {}",
        fmt_unix_seconds(s.cooldown_ends_at, "not in slashing cooldown")
    );

    if DEBUG_LOG && !s.registered {
        eprintln!("registered false");
    }
    Ok(())
}
