//! 拒绝交付物
//!
//! 买家动作：拒绝交付物 — onchainos task reject
//!
//! 流程：pre-refuse(orderId,deadline) → 签 digest → refuse(signatureData+reason) → 签 uopHash → broadcast

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// reject/refuse — 拒绝验收
pub async fn handle_reject(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    // TODO: deadline 策略待确认，暂时使用当前时间 + 1 小时
    let deadline = chrono::Utc::now().timestamp() + 3600;

    // Step 1: pre-refuse → digest + nonce (712 标准，不需要 sessionCert)
    let pre_body = serde_json::json!({
        "orderId": job_id,
        "deadline": deadline,
    });
    let pre_resp = client.post_with_identity(
        &client.endpoint(job_id, "pre-refuse"),
        &pre_body,
        &agent_id,
    ).await?;
    let digest = pre_resp["digest"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("pre-refuse 未返回 digest"))?;
    // nonce 由后端生成，refuse 请求时需要回传
    let nonce = pre_resp["nonce"]
        .as_str()
        .unwrap_or("");

    // Step 2: session key 签名 digest
    let signature = signing::sign_digest_with_session_key(digest)?;

    // Step 3: refuse (signatureData + reason + sessionCert)
    let main_body = serde_json::json!({
        "signatureData": {
            "signature": signature,
            "deadline": deadline.to_string(),
            "nonce": nonce,
        },
        "reason": reason,
    });
    let main_resp = client.post_with_identity(
        &client.endpoint(job_id, "refuse"),
        &main_body,
        &agent_id,
    ).await?;

    // Step 4: 签 uopHash + broadcast
    let tx_hash = signing::sign_uop_and_broadcast(
        client, &main_resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobRefuse, &agent_id,
    ).await?;

    println!("✓ 已拒绝验收（原因：{reason}），状态 → refused");
    println!("  卖家有 24 小时内可申请仲裁");
    println!("  txHash: {tx_hash}");
    Ok(())
}
