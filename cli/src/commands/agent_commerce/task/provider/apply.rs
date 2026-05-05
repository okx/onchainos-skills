//! 卖家申请接单
//!
//! 卖家动作：申请接单 — onchainos agent apply

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// apply — 卖家申请接单
///
/// 1. POST apply API（带身份头）→ 获取 uopData
/// 2. 签名 uopData + 广播上链
pub async fn handle_apply(
    client: &mut TaskApiClient,
    job_id: &str,
    token_amount: &str,
    token_symbol: &str,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let body = serde_json::json!({
        "tokenAmount": token_amount,
        "tokenSymbol": token_symbol,
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "apply"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
    ).await?;

    println!("✓ 已提交接单申请（apply），等待链上确认（provider_applied）");
    println!("  报价: {token_amount} {token_symbol}");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动，不要主动给买家发消息：");
    println!("    - 禁止立即调 `xmtp_send` 告诉买家 \"已提交申请\" 等文字");
    println!("    - 链上确认后会收到 `provider_applied` 系统通知");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus provider_applied --role provider`，");
    println!("      按输出提示 `session_status` + `xmtp_send` 发付款单");
    Ok(())
}
