use anyhow::{bail, Result};
use chrono::TimeZone;
use std::cmp::Ordering;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::{decimal_str, staking_types};
use crate::commands::agent_commerce::task::signing;

/// 把 unix 秒格式化为「ts（本地时间 YYYY-MM-DD HH:MM:SS TZ）」用于错误提示。
fn fmt_local_ts(ts: i64) -> String {
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| format!("{ts}（本地时间 {}）", d.format("%Y-%m-%d %H:%M:%S %Z")))
        .unwrap_or_else(|| ts.to_string())
}

pub async fn handle_request_unstake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 50）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }
    // > 0 校验：用 cmp 而非 f64，避免 "0.0000000000000000001" 这种被 f64 当成 0 的极端 case
    if decimal_str::cmp(trimmed, "0")
        .map_err(|e| anyhow::anyhow!("--amount 解析失败（格式非法），got: {trimmed}: {e}"))?
        != Ordering::Greater
    {
        bail!("--amount 必须 > 0，got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    // 拉 my-stake / staking-config（任一失败 → 直接报错结束，不做 best-effort 猜测）
    let m = staking_types::get_my_stake(client, &agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("my-stake 拉取失败，无法校验 request-unstake 前置条件：{e}"))?;

    // 活跃仲裁阻拦
    let active_disputes = m.active_disputes.parse::<u64>().unwrap_or(0);
    if active_disputes > 0 {
        bail!(
            "当前有 {active_disputes} 个未结仲裁，期间不允许解质押。请等仲裁结算后再申请。"
        );
    }

    let active = &m.active_stake_okb;
    // amount 不能 > activeStake
    if decimal_str::cmp(trimmed, active).map_err(|e| {
        anyhow::anyhow!("activeStake 解析失败 ({active}): {e}")
    })? == Ordering::Greater
    {
        bail!(
            "--amount {trimmed} OKB 超过当前 activeStake {active} OKB；最多可解 {active} OKB（全额赎回）。"
        );
    }

    let cfg = staking_types::get_staking_config(client, &agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("staking-config 拉取失败，无法校验部分赎回保留：{e}"))?;
    let retain = &cfg.partial_unstake_min_retain_okb;
    // 部分赎回后剩余必须 >= partialUnstakeMinRetainOkb（全额赎回 amt == active 不受此限）。
    // 全部走字符串十进制运算：避免 f64 精度抖动把"恰好达标"误判为"差一点"。
    //
    // 真实复现 case（active=0.0012, amt=0.0002, retain=0.001）：
    //   - f64    : 0.0012_f64 - 0.0002_f64 = 0.0009999999999999998 < 0.001 → 误报"低于最低保留"
    //   - 字符串 : sub("0.0012", "0.0002") = "0.001"               == 0.001 → 通过
    let remaining = decimal_str::sub(active, trimmed).map_err(|e| {
        anyhow::anyhow!(
            "解质押预检：activeStake {active} - amount {trimmed} 计算失败：{e}"
        )
    })?;
    let is_full_unstake = decimal_str::cmp(&remaining, "0")
        .map(|o| o == Ordering::Equal)
        .unwrap_or(false);
    if !is_full_unstake {
        let below_retain = decimal_str::cmp(&remaining, retain)
            .map_err(|e| anyhow::anyhow!("partialUnstakeMinRetainOkb 解析失败 ({retain}): {e}"))?
            == Ordering::Less;
        if below_retain {
            bail!(
                "部分赎回后余额 {remaining} OKB 将低于最低保留 {retain} OKB（partialUnstakeMinRetainOkb）。\
                 请改为全额赎回（金额 = {active} OKB），或减小本次数额使剩余 >= {retain} OKB。"
            );
        }
    }

    let path = "/priapi/v1/aieco/task/staking/requestUnstake";
    let body = serde_json::json!({ "amount": trimmed });
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::extract_biz_type(&resp),
        &agent_id,
    )
    .await?;

    audit::log(
        "cli",
        "evaluator/unstake_requested",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("amount={trimmed}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("request-unstake submitted (agentId={agent_id})");
    println!("  amount:  -{trimmed} OKB（申请中）");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!(
        "next: 申请已提交，等待链上确认；确认后进入 {} 天冷却期，到时可领取，期间可撤销。",
        cfg.unstake_cooldown_days(),
    );
    println!(
        "  config: 部分赎回后余额最低保留 {} OKB（低于此值只能全额赎回）",
        cfg.partial_unstake_min_retain_okb,
    );
    Ok(())
}

/// 冷却期结束后领取已解质押的 OKB。合约内部知道金额与解锁时间，请求体为空。
pub async fn handle_claim_unstake(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    // best-effort pre-check：my-stake 失败 → 跳过预检由合约兜底
    if let Ok(m) = staking_types::get_my_stake(client, &agent_id).await {
        if m.unstake_available_at == 0 {
            bail!("当前没有待领取的解质押申请。请先发起解质押申请。");
        }
        let now = chrono::Utc::now().timestamp();
        if now < m.unstake_available_at {
            bail!(
                "解质押冷却期未结束（解锁时间 {}），到期后再领取。",
                fmt_local_ts(m.unstake_available_at)
            );
        }
    }

    let path = "/priapi/v1/aieco/task/staking/claimUnstake";
    let body = serde_json::json!({});
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::extract_biz_type(&resp),
        &agent_id,
    )
    .await?;

    audit::log(
        "cli",
        "evaluator/unstake_claimed",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("claim-unstake submitted (agentId={agent_id})");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 领取交易已提交，等待链上确认到账。");
    Ok(())
}

pub async fn handle_cancel_unstake(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    // best-effort pre-check：my-stake 失败 → 跳过预检由合约兜底
    if let Ok(m) = staking_types::get_my_stake(client, &agent_id).await {
        if m.unstake_available_at == 0 {
            bail!("当前没有待撤销的解质押申请。");
        }
        let now = chrono::Utc::now().timestamp();
        if now >= m.unstake_available_at {
            bail!(
                "解质押冷却期已结束，已可直接领取，撤销已无效。请改为领取。"
            );
        }
    }

    let path = "/priapi/v1/aieco/task/staking/cancelUnstake";
    let body = serde_json::json!({});
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        "",
        signing::extract_biz_type(&resp),
        &agent_id,
    )
    .await?;

    audit::log(
        "cli",
        "evaluator/unstake_cancelled",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("cancel-unstake submitted (agentId={agent_id})");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!("next: 取消已提交，等待链上确认；确认后质押恢复。");
    Ok(())
}