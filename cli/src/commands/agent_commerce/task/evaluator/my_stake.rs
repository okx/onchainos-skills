//! 仲裁者读取当前账户的链上质押状态（只读）— onchainos agent evaluator my-stake
//!
//! API: GET /priapi/v1/aieco/task/staking/myStake
//! - Headers: Authorization (JWT) + `agenticId`（与 staking-config 一致；后端 interceptor
//!   要求 evaluator 身份头，纯 JWT 调用会被拒 code=3001）
//! - 无 Body；后端仍从 token/address 反查 `agentId` 写到响应,我们传的 agenticId 仅过 interceptor
//!
//! ⚠️ 关键概念区分（避免 skill / agent 把"钱包余额"当"已质押"）：
//!   - 钱包余额 = EOA 上可花费的 OKB（`onchainos wallet balance` 查的）
//!   - `activeStake` = 已经从余额转入 `VoterStaking` 合约锁仓的 OKB（已扣历史罚没）
//!
//! 首次质押 / 累计门槛判断 必须用 `activeStake`，不能拿余额顶替。

use anyhow::Result;
use chrono::TimeZone;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

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

pub async fn handle_my_stake(client: &mut TaskApiClient) -> Result<()> {
    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;
    let s = client.get_my_stake(&agent_id).await?;

    println!("my stake (链上质押状态)");
    println!("  voter address      : {}", s.voter_address);
    println!(
        "  agentId            : {} (registered={})",
        s.agent_id, s.registered
    );
    println!(
        "  activeStake        : {} OKB (wei: {})  # 当前已质押（已扣罚没）",
        s.active_stake_okb(),
        s.active_stake_wei
    );
    println!(
        "  pendingUnstake     : {} OKB (wei: {})  # 冷却期中待解锁",
        s.pending_unstake_okb(),
        s.pending_unstake_wei
    );
    println!(
        "  validStake         : {} OKB (wei: {})  # 可加权选取 = activeStake - pendingUnstake",
        s.valid_stake_okb(),
        s.valid_stake_wei
    );
    println!(
        "  activeDisputes     : {}  # 参与中的仲裁数（>0 时不可全额解质押）",
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
        println!(
            "note: 当前地址尚未注册为 voter（agentId=0）；先通过身份 skill 完成 evaluator 注册再质押。"
        );
    }
    Ok(())
}
