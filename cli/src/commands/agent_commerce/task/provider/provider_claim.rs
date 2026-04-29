//! 卖家在 submit→complete 超时后主动 claim（claimAutoComplete）
//!
//! 卖家动作：超时领取 — onchainos agent claim-auto-complete
//!
//! 触发场景：买家在 completedWindow 内未验收（既不 complete 也不 reject）
//! → 后端 keeper 给 provider 发 system notification
//! → provider 调本接口（permissionless 链上 claim）
//! → AP.complete → 状态 complete，资金乐观结算给 provider

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// claim-auto-complete — submit→complete 超时的乐观结算
///
/// 1. POST claimAutoComplete API（带身份头）→ 获取 uopData（spec：Request 无）
/// 2. 签名 uopData + 广播上链
pub async fn handle_claim_auto_complete(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "claimAutoComplete"), &body, &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobComplete, &agent_id,
    ).await?;

    println!("✓ 已发起超时领取（claimAutoComplete），等待链上确认（job_completed）");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动：");
    println!("    - 链上确认后会收到 `job_completed` 系统通知（资金已释放给你）");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus job_completed --role provider`");
    Ok(())
}
