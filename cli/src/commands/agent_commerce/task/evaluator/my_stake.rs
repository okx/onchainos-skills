use anyhow::Result;
use chrono::TimeZone;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::staking_types;

/// 渲染 unix 秒时间戳。0 表示"不适用"。
fn fmt_unix_seconds(ts: i64, none_label: &str) -> String {
    if ts == 0 {
        format!("0 ({none_label})")
    } else if let Some(dt) = chrono::Local.timestamp_opt(ts, 0).single() {
        format!("{ts} ({})", dt.format("%Y-%m-%d %H:%M:%S %Z"))
    } else {
        format!("{ts} (无法解析)")
    }
}

pub async fn handle_my_stake(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let s = staking_types::get_my_stake(client, agent_id).await?;

    println!("my stake (链上质押状态)");
    println!("  voter address      : {}", s.voter_address);
    println!(
        "  agentId            : {} (registered={})",
        s.agent_id, s.registered
    );
    println!(
        "  activeStake        : {} OKB  # 当前已质押（已扣罚没）",
        s.active_stake_okb
    );
    println!(
        "  pendingUnstake     : {} OKB  # 冷却期中待解锁",
        s.pending_unstake_okb
    );
    println!(
        "  validStake         : {} OKB  # 可加权选取 = activeStake - pendingUnstake",
        s.valid_stake_okb
    );
    println!(
        "  activeDisputes     : {}  # 参与中的仲裁数（>0 时不可解质押）",
        s.active_disputes
    );
    println!(
        "  unstakeAvailableAt : {}",
        fmt_unix_seconds(s.unstake_available_at, "无待解锁")
    );
    println!(
        "  cooldownEndsAt     : {}",
        fmt_unix_seconds(s.cooldown_ends_at, "不在罚没冷却期")
    );

    if !s.registered {
        eprintln!(
            "registered false"
        );
    }
    Ok(())
}
