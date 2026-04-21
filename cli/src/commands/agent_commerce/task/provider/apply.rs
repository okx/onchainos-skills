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
    client: &TaskApiClient,
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
        &client.endpoint(job_id, "apply"), &body, agent_id, &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client.http(), &client.broadcast_url(), &resp["data"]["uopData"], &account_id, &address,
    ).await?;

    println!("✓ 已提交接单申请（apply），等待链上确认（TASK_APPLIED）");
    println!("  报价: {token_amount} {token_symbol}");
    println!("  txHash: {tx_hash}");
    Ok(())
}
