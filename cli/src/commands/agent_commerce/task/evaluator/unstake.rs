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
        .map(|d| format!("{ts} (local time {})", d.format("%Y-%m-%d %H:%M:%S %Z")))
        .unwrap_or_else(|| ts.to_string())
}

pub async fn handle_request_unstake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount must not be empty (OKB amount in UI units, e.g. 50)");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount must be numeric (OKB amount in UI units, no precision suffix), got: {trimmed}");
    }
    // > 0 check via cmp instead of f64 to handle extreme cases like "0.0000000000000000001"
    if decimal_str::cmp(trimmed, "0")
        .map_err(|e| anyhow::anyhow!("--amount parse failed (invalid format), got: {trimmed}: {e}"))?
        != Ordering::Greater
    {
        bail!("--amount must be > 0, got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    // 拉 my-stake / staking-config（任一失败 → 直接报错结束，不做 best-effort 猜测）
    let m = staking_types::get_my_stake(client, &agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch my-stake, cannot validate request-unstake preconditions: {e}"))?;

    // active dispute blocker
    let active_disputes = m.active_disputes.parse::<u64>().unwrap_or(0);
    if active_disputes > 0 {
        bail!(
            "{active_disputes} unresolved dispute(s) in progress; unstake is not allowed. Wait until disputes are settled before unstaking."
        );
    }

    let active = &m.active_stake_okb;
    // amount must not exceed activeStake
    if decimal_str::cmp(trimmed, active).map_err(|e| {
        anyhow::anyhow!("activeStake parse failed ({active}): {e}")
    })? == Ordering::Greater
    {
        bail!(
            "--amount {trimmed} OKB exceeds current activeStake {active} OKB; max unstake is {active} OKB (full redemption)."
        );
    }

    let cfg = staking_types::get_staking_config(client, &agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch staking-config, cannot validate partial-unstake min retain: {e}"))?;
    let retain = &cfg.partial_unstake_min_retain_okb;
    // 部分赎回后剩余必须 >= partialUnstakeMinRetainOkb（全额赎回 amt == active 不受此限）。
    // 全部走字符串十进制运算：避免 f64 精度抖动把"恰好达标"误判为"差一点"。
    //
    // 真实复现 case（active=0.0012, amt=0.0002, retain=0.001）：
    //   - f64    : 0.0012_f64 - 0.0002_f64 = 0.0009999999999999998 < 0.001 → 误报"低于最低保留"
    //   - 字符串 : sub("0.0012", "0.0002") = "0.001"               == 0.001 → 通过
    let remaining = decimal_str::sub(active, trimmed).map_err(|e| {
        anyhow::anyhow!(
            "unstake pre-check: activeStake {active} - amount {trimmed} computation failed: {e}"
        )
    })?;
    let is_full_unstake = decimal_str::cmp(&remaining, "0")
        .map(|o| o == Ordering::Equal)
        .unwrap_or(false);
    if !is_full_unstake {
        let below_retain = decimal_str::cmp(&remaining, retain)
            .map_err(|e| anyhow::anyhow!("partialUnstakeMinRetainOkb parse failed ({retain}): {e}"))?
            == Ordering::Less;
        if below_retain {
            bail!(
                "partial unstake would leave {remaining} OKB, below min retain {retain} OKB (partialUnstakeMinRetainOkb). \
                 switch to full redemption (amount = {active} OKB), or reduce --amount so remaining >= {retain} OKB."
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
    println!("  amount:  -{trimmed} OKB (pending)");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!(
        "next: request submitted, awaiting on-chain confirmation; after confirm, enters {}-day cooldown — claimable on expiry, cancellable during cooldown.",
        cfg.unstake_cooldown_days(),
    );
    println!(
        "  config: partial-unstake min retain {} OKB (below this, only full redemption is allowed)",
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
            bail!("no pending unstake request to claim. Submit an unstake request first.");
        }
        let now = chrono::Utc::now().timestamp();
        if now < m.unstake_available_at {
            bail!(
                "unstake cooldown not finished (unlocks at {}); claim after expiry.",
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
    println!("next: claim tx submitted, awaiting on-chain confirmation and settlement.");
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
            bail!("no pending unstake request to cancel.");
        }
        let now = chrono::Utc::now().timestamp();
        if now >= m.unstake_available_at {
            bail!(
                "unstake cooldown has finished and the request is already claimable; cancel is no longer valid. Use claim-unstake instead."
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
    println!("next: cancel tx submitted, awaiting on-chain confirmation; stake will be restored after confirm.");
    Ok(())
}