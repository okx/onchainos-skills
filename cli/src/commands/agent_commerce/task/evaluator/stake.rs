use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::staking_types;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_stake(
    client: &mut TaskApiClient,
    amount: &str,
    agent_id: &str,
) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 500）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let cfg = staking_types::get_staking_config(client, &agent_id).await.ok();
    if let Some(c) = cfg.as_ref() {
        if let (Ok(amt), Ok(min)) = (
            trimmed.parse::<f64>(),
            c.min_cumulative_stake_okb.parse::<f64>(),
        ) {
            if amt < min {
                // my-stake 失败 → 跳过预检，由合约 1001 兜底（避免误拒合法的被罚补齐）。
                if let Ok(m) = staking_types::get_my_stake(client, &agent_id).await {
                    let active = m.active_stake_okb.parse::<f64>().unwrap_or(0.0);
                    if amt + active < min {
                        bail!(
                            "累计质押不足：本次 {trimmed} OKB + 当前 activeStake {active} OKB < 平台最低门槛 {min_str} OKB（minCumulativeStakeOkb）。\
                             首次质押需 本次 >= {min_str}；被罚后补齐需 本次 >= {min_str} - activeStake。",
                            min_str = c.min_cumulative_stake_okb,
                        );
                    }
                }
            }
        }
    }

    let path = "/priapi/v1/aieco/task/staking/stake";
    let body = serde_json::json!({ "amount": trimmed });
    let resp = client
        .post_with_identity(path, &body, &agent_id)
        .await?;

    // staking 不关联具体 jobId，用空字符串作 broadcast 的 bizContext.jobId。
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

    println!("stake submitted (agentId={agent_id})");
    println!("  amount:  {trimmed} OKB");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!(
        "next: 质押交易已提交，等待链上确认；确认后即成为活跃仲裁者候选，可被选入陪审。"
    );
    Ok(())
}
