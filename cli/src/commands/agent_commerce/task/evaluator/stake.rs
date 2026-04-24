use anyhow::{bail, Result};

use super::helpers::evaluator_agent_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;

/// Evaluator OKB staking — onboarding handoff from identity skill.
///
/// API: POST /priapi/v1/aieco/task/staking/stake (Lark wiki §8.2)
/// - Body: `{ "amount": "<OKB 金额, UI 单位不带精度>" }`
/// - Headers: X-Agent-Id / X-Wallet-Address (interceptor 校验 evaluator 身份)
/// - Backend bundles approve(VoterStaking, amount) + stake(amount, agentId) as one
///   atomic UOP (AA executeBatch), returns uopData for signing.
///
/// Error codes:
///   4000 — agentId 无效 / 非 evaluator 身份
///   2004 — agentId 无 evaluator 身份 (identity=2)
///   1001 — 首次质押 amount < 100 OKB
pub async fn run_stake(amount: String, _ctx: &Context) -> Result<()> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("--amount 不能为空（OKB 金额，UI 单位，例如 500）");
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        bail!("--amount 必须是数字（OKB 金额，UI 单位不带精度），got: {trimmed}");
    }

    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();
    let mut client = TaskApiClient::new();

    let path = "/priapi/v1/aieco/task/staking/stake";
    let body = serde_json::json!({ "amount": trimmed });
    let resp = client
        .post_with_identity(path, &body, &agent_id, &address)
        .await?;

    // staking 不关联具体 jobId，用空字符串作 broadcast 的 bizContext.jobId。
    let tx_hash = signing::sign_uop_and_broadcast(
        &mut client,
        &resp["data"]["uopData"],
        &account_id,
        &address,
        "",
        signing::BizContext::Stake,
    )
    .await?;

    println!("stake submitted (agentId={agent_id})");
    println!("  amount:  {trimmed} OKB");
    println!("  voter:   {address}");
    println!("  txHash:  {tx_hash}");
    println!(
        "next: 等待 `staked` 事件（VoterStaking.Staked 上链）确认质押生效；\n\
         生效后 agentId={agent_id} 成为活跃仲裁者候选，可被选入陪审。"
    );
    Ok(())
}
