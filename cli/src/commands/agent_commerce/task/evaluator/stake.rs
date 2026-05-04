//! 仲裁者首次质押（身份 skill 跳转入口）— onchainos agent stake

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::staking_types;
use crate::commands::agent_commerce::task::signing;

/// Evaluator OKB staking — onboarding handoff from identity skill.
///
/// API: POST /priapi/v1/aieco/task/staking/stake
/// - Body: `{ "amount": "<OKB 金额, UI 单位不带精度>" }`
/// - Headers: agenticId (interceptor 校验 evaluator 身份)
/// - Backend bundles approve(VoterStaking, amount) + stake(amount, agentId) as one
///   atomic UOP (AA executeBatch), returns uopData for signing.
///
/// 累计门槛规则语义：合约层按**累计**校验 `当前地址质押金额 + 本次质押金额 >= minCumulativeStakeOkb`，
/// 不足则 revert。首次质押场景天然等价于"本次 >= min"；被 slash 后余额低于门槛时
/// 追加质押也须一次性补齐到 minCumulativeStakeOkb（具体值由 staking-config 提供）。
///
/// Error codes:
///   4000 — agentId 无效 / 非 evaluator 身份
///   2004 — agentId 无 evaluator 身份 (identity=2)
///   1001 — 累计质押 < 最低门槛（由 `/staking/config.minCumulativeStakeOkb` 决定，合约/后端权威）
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

    // best-effort 拉平台配置，做 UX 友好预检（首次质押天然等价于"本次 >= 最低门槛"）。
    // 失败 / 余额不可知场景下不阻塞，由合约 1001 兜底。
    let cfg = staking_types::get_staking_config(client, &agent_id).await.ok();
    if let Some(c) = cfg.as_ref() {
        if let (Ok(amt), Ok(min)) = (
            trimmed.parse::<f64>(),
            c.min_cumulative_stake_okb.parse::<f64>(),
        ) {
            if amt < min {
                bail!(
                    "本次质押 {trimmed} OKB 低于平台最低累计门槛 {} OKB（minCumulativeStakeOkb）；\
                     首次质押或被罚后补齐都需一次性 >= {} OKB。",
                    c.min_cumulative_stake_okb, c.min_cumulative_stake_okb
                );
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
