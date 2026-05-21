//! Provider submits deliverable.
//!
//! Provider action: deliver — onchainos agent deliver

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::signing;

/// deliver — submit deliverable
///
/// 1. Precondition: job must be in accepted state (status=1) — i.e. the buyer has confirm-accept on-chain
/// 2. POST submit API (with identity headers) → fetch uopData
/// 3. Sign uopData + broadcast on-chain
pub async fn handle_deliver(
    client: &mut TaskApiClient,
    job_id: &str,
    _file: &str,
    _message: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
    }

    // Precondition: job must be accepted (buyer has confirm-accept, on-chain job_accepted notification received) before delivery.
    // Prevents the agent from racing to deliver right after apply without waiting for buyer confirmation — backend rejects this, but an early bail makes the error clearer.
    let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
    let status_int = task_resp["status"]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 status 字段，无法判定能否交付"))?;
    let status = Status::from_int(status_int);
    if status != Status::Accepted {
        audit::log(
            "cli",
            "provider/deliver_blocked_wrong_status",
            false,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("statusInt={status_int}"),
                format!("status={}", status.as_str()),
            ]),
            Some("status != accepted(1)"),
        );
        bail!(
            "deliver 拒绝执行：任务当前 status = {} ({}), 必须为 accepted (1) 才能交付。\n\
             如果你刚 apply，需要等买家 confirm-accept 上链，收到 `job_accepted` 系统通知后再 deliver。\n\
             不要主动 xmtp_send 催买家——买家 confirm-accept 是用户决策，由对方 user session 推进。",
            status_int,
            status.as_str(),
        );
    }

    let (account_id, address) = signing::resolve_wallet(None, None)?;
    // Backend spec: submit endpoint accepts an `evidenceHash` field; for now pass an empty string placeholder (offchain
    // evidence is uploaded multipart via /evidence/upload — no hash is provided at submit stage). file/message are
    // kept as CLI input placeholders only; not put on-chain.
    let body = serde_json::json!({
        "evidenceHash": "",
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "submit"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
    ).await?;

    audit::log(
        "cli",
        "provider/deliver_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ 交付物已提交，等待链上确认（job_submitted）");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动，不要主动给买家发消息：");
    println!("    - 禁止立即调 `xmtp_send` 告诉买家 \"交付物已上链，请验收\" 等文字");
    println!("    - 链上确认后会收到 `job_submitted` 系统通知");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus job_submitted --role provider`");
    Ok(())
}
