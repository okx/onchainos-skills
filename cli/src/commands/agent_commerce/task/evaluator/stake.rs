use anyhow::{bail, Result};
use std::cmp::Ordering;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::{decimal_str, staking_types};
use crate::commands::agent_commerce::task::signing;

pub async fn handle_stake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    let trimmed = validate_amount(amount, "500")?;

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let (tx_hash, endpoint) =
        execute_stake_or_increase(client, trimmed, &account_id, &address, &agent_id).await?;

    println!("stake submitted (agentId={agent_id}, via={endpoint})");
    println!("  amount:  {trimmed} OKB");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 质押交易已提交，等待链上确认；确认后即成为活跃仲裁者候选，可被选入陪审。");
    Ok(())
}

pub(super) fn validate_amount<'a>(amount: &'a str, example: &str) -> Result<&'a str> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 {example}）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }
    Ok(trimmed)
}

/// 阈值校验 + 路由 + fallback：
/// 1. 拉 my-stake / staking-config（任一失败直接报错结束，不做路由猜测）
/// 2. 强制 `activeStake + amount >= minCumulativeStakeOkb`（不分 registered 状态）
/// 3. 按 registered 路由：true → primary=increaseStake / fallback=stake；
///                       false → primary=stake / fallback=increaseStake
/// 4. primary 报错 → eprintln warning 后 fallback 一次（覆盖 registered 读漏 / 链上滞后）
///
/// 返回 (txHash, 实际生效的端点 label)。两个端点都失败时把两边错误一起报出。
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

    let registered = m.registered;
    let (primary, fallback) = if registered {
        ("increaseStake", "stake")
    } else {
        ("stake", "increaseStake")
    };

    match try_post_and_broadcast(client, primary, amount, account_id, address, agent_id).await {
        Ok(tx) => Ok((tx, primary)),
        Err(primary_err) => {
            eprintln!("warning: 首次提交失败：{primary_err}；切换路径再试一次...");
            match try_post_and_broadcast(
                client,
                fallback,
                amount,
                account_id,
                address,
                agent_id,
            )
            .await
            {
                Ok(tx) => Ok((tx, fallback)),
                Err(fallback_err) => bail!(
                    "质押失败（两次提交都失败）：\n\
                     - {primary_err}\n\
                     - {fallback_err}"
                ),
            }
        }
    }
}

async fn try_post_and_broadcast(
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