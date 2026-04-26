//! 卖家提交交付物
//!
//! 卖家动作：交付 — onchainos agent deliver

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// deliver — 提交交付物
///
/// 1. POST submit API（带身份头）→ 获取 uopData
/// 2. 签名 uopData + 广播上链
pub async fn handle_deliver(
    client: &mut TaskApiClient,
    job_id: &str,
    _file: &str,
    _message: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;
    // 后端 spec：submit endpoint Request 空（旧 evidenceHash 字段已划掉）
    // file/message 仅作 CLI 输入预留位，**不上链**——证据走 /evidence/upload 多 part 链下。
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "submit"), &body, &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobSubmit,
    ).await?;

    println!("✓ 交付物已提交，等待链上确认（job_submitted）");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动，不要主动给买家发消息：");
    println!("    - 禁止立即调 `xmtp_send` 告诉买家 \"交付物已上链，请验收\" 等文字");
    println!("    - 链上确认后会收到 `job_submitted` 系统通知");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus job_submitted --role provider`");
    Ok(())
}
