use anyhow::{bail, Result};
use std::cmp::Ordering;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::{decimal_str, staking_types};
use crate::commands::agent_commerce::task::signing;

pub async fn handle_stake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    run(
        client,
        amount,
        agent_id,
        StakeUx {
            label: "stake",
            amount_prefix: "",
            next_hint: "质押交易已提交，等待链上确认；确认后即成为活跃仲裁者候选，可被选入陪审。",
        },
    )
    .await
}

pub async fn handle_increase_stake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    run(
        client,
        amount,
        agent_id,
        StakeUx {
            label: "increase-stake",
            amount_prefix: "+",
            next_hint: "追加质押已提交，等待链上确认。",
        },
    )
    .await
}

struct StakeUx {
    label: &'static str,
    amount_prefix: &'static str,
    next_hint: &'static str,
}

async fn run(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
    ux: StakeUx,
) -> Result<()> {
    let trimmed = validate_amount(amount)?;

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let (tx_hash, endpoint) =
        execute_stake_or_increase(client, trimmed, &account_id, &address, &agent_id).await?;

    let event = if endpoint == "increaseStake" {
        "evaluator/stake_increased"
    } else {
        "evaluator/staked"
    };
    audit::log(
        "cli",
        event,
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("amount={trimmed}"),
            format!("endpoint={endpoint}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("{} submitted (agentId={agent_id}, via={endpoint})", ux.label);
    println!("  amount:  {}{trimmed} OKB", ux.amount_prefix);
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: {}", ux.next_hint);
    Ok(())
}

fn validate_amount(amount: &str) -> Result<&str> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位）；如含小数点请使用 `.`，且不要包含千分位分隔符，got: {trimmed}");
    }
    Ok(trimmed)
}

/// 阈值校验 + 路由：
/// 1. 拉 my-stake / staking-config（任一失败直接报错结束）
/// 2. 强制 `activeStake + amount >= minCumulativeStakeOkb`（不分 registered 状态）
/// 3. 按 registered 路由：true → increaseStake；false → stake
///
/// 返回 (txHash, 端点 label)。
pub(super) async fn execute_stake_or_increase(
    client: &mut TaskApiClient,
    amount: &str,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<(String, &'static str)> {
    let m = staking_types::get_my_stake(client, agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("my-stake 拉取失败，无法判定 stake / increase-stake 路由：{e}"))?;
    let cfg = staking_types::get_staking_config(client, agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("staking-config 拉取失败，无法校验累计质押门槛：{e}"))?;

    // 累计质押门槛硬校验（无论 registered=true/false）：activeStake + amount >= min
    // 全部走字符串十进制运算：避免 f64 精度抖动把"恰好达标"误判为"差一点"。
    // 解析失败（API 异常字段）时静默跳过预检 — 与旧版 f64 行为一致，由后端兜底。
    let active = &m.active_stake_okb;
    let min_str = &cfg.min_cumulative_stake_okb;
    if let Ok(total) = decimal_str::add(amount, active) {
        if decimal_str::cmp(&total, min_str)
            .map(|o| o == Ordering::Less)
            .unwrap_or(false)
        {
            // total < min ∧ amount > 0 ⇒ active < min ⇒ min - active 不会 underflow
            let needed = decimal_str::sub(min_str, active).unwrap_or_else(|_| min_str.clone());
            bail!(
                "累计质押不足：本次 {amount} OKB + 当前 activeStake {active} OKB < 平台最低门槛 {min_str} OKB（minCumulativeStakeOkb）。\
                 请提高金额，至少需追加 {needed} OKB。"
            );
        }
    }

    let endpoint = if m.registered { "increaseStake" } else { "stake" };
    let tx = post_and_broadcast(client, endpoint, amount, account_id, address, agent_id).await?;
    Ok((tx, endpoint))
}

async fn post_and_broadcast(
    client: &mut TaskApiClient,
    endpoint: &str,
    amount: &str,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<String> {
    let path = format!("/priapi/v1/aieco/task/staking/{endpoint}");
    let body = serde_json::json!({ "amount": amount });
    let resp = client.post_with_identity(&path, &body, agent_id).await?;
    signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        account_id,
        address,
        "",
        signing::extract_biz_type(&resp),
        agent_id,
    )
    .await
}