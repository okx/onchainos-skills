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
            next_hint: "stake transaction submitted; waiting for on-chain confirmation. Once confirmed, you become an active evaluator candidate and may be drawn into a jury panel.",
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
            next_hint: "increase-stake submitted; waiting for on-chain confirmation.",
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
        bail!("--amount must not be empty (OKB amount in UI units)");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount must be numeric (OKB amount in UI units); use `.` for decimal point and no thousands separators, got: {trimmed}");
    }
    Ok(trimmed)
}

/// Threshold check + routing:
/// 1. Fetch my-stake / staking-config (any failure aborts).
/// 2. Enforce `activeStake + amount >= minCumulativeStakeOkb` (irrespective of `registered`).
/// 3. Route by `registered`: true → `increaseStake`; false → `stake`.
///
/// Returns (txHash, endpoint label).
pub(super) async fn execute_stake_or_increase(
    client: &mut TaskApiClient,
    amount: &str,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<(String, &'static str)> {
    let m = staking_types::get_my_stake(client, agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch my-stake, cannot route stake vs increase-stake: {e}"))?;
    let cfg = staking_types::get_staking_config(client, agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch staking-config, cannot validate cumulative stake threshold: {e}"))?;

    // Cumulative stake threshold hard check (regardless of registered=true/false):
    // activeStake + amount >= min. All arithmetic runs in string-decimal to avoid
    // f64 precision artifacts that would misclassify "exactly meets" as "just
    // short". On parse failure (API field anomaly) silently skip this preflight
    // — matches the prior f64 behavior; the backend is the ultimate guard.
    let active = &m.active_stake_okb;
    let min_str = &cfg.min_cumulative_stake_okb;
    if let Ok(total) = decimal_str::add(amount, active) {
        if decimal_str::cmp(&total, min_str)
            .map(|o| o == Ordering::Less)
            .unwrap_or(false)
        {
            // total < min ∧ amount > 0 ⇒ active < min ⇒ min - active cannot underflow.
            let needed = decimal_str::sub(min_str, active).unwrap_or_else(|_| min_str.clone());
            bail!(
                "cumulative stake too low: this {amount} OKB + current activeStake {active} OKB < platform minimum {min_str} OKB (minCumulativeStakeOkb). \
                 increase --amount by at least {needed} OKB."
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
        None,
    )
    .await
}