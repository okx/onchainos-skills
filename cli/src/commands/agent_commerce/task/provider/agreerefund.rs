//! Provider agrees to refund.
//!
//! Provider action: agree to refund — onchainos agent agree-refund

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// agree-refund — agree to refund
///
/// 1. POST agreeRefund API (with identity headers) → fetch uopData
/// 2. Sign uopData + broadcast on-chain
pub async fn handle_agree_refund(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
    }
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let body = serde_json::json!({});

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "agreeRefund"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
    ).await?;

    audit::log(
        "cli",
        "provider/agree_refund_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ 已同意退款，等待链上确认（job_refunded）");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动，不要主动给买家发消息：");
    println!("    - 禁止立即调 `xmtp_send` 告诉买家 \"已同意退款\" 等文字");
    println!("    - 链上确认后会收到 `job_refunded` 系统通知");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus job_refunded --role provider`");
    Ok(())
}
