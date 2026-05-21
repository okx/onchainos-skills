//! Provider applies for a job.
//!
//! Provider action: apply for a job — onchainos agent apply

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// apply — provider applies for a job
///
/// 1. POST apply API (with identity headers) → fetch uopData
/// 2. Sign uopData + broadcast on-chain
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

    audit::log(
        "cli",
        "provider/apply_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("tokenSymbol={token_symbol}"),
            format!("tokenAmount={token_amount}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

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
