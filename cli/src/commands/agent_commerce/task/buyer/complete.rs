//! 确认完成
//!
//! 买家动作：确认完成 — onchainos agent complete
//!
//! 根据支付方式分流：
//! - escrow: pre-complete(orderId,deadline) → 签 digest → complete(signatureData) → 签 uopHash → broadcast（释放担保款）
//! - x402: /direct/complete 单签 → broadcast（资金已在 accept 阶段支付）

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PaymentMode;
use crate::commands::agent_commerce::task::signing;

/// complete — 验收通过
pub async fn handle_complete(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;

    // 查询任务详情获取 paymentMode
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
    let task = &resp;
    let payment_mode = PaymentMode::from_int(task["paymentMode"].as_i64().unwrap_or(0) as i32);

    if payment_mode == PaymentMode::Escrow {
        // ── 担保：双签 pre-complete → complete ──────────────────────
        let result = signing::task_dual_sign_and_broadcast(
            client, job_id, "pre-complete", "complete",
            None,
            &account_id, &address, &agent_id,
        ).await?;

        audit::log(
            "cli",
            "buyer/complete_submitted",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("paymentMode=escrow"),
                format!("txHash={}", result.tx_hash),
            ]),
            None,
        );
        println!("✓ 任务验收通过（担保），状态 → complete，款项已释放");
        println!("  txHash: {}", result.tx_hash);
    } else {
        // ── x402：/direct/complete 单签（资金已在 accept 阶段支付）────────
        let resp = client.post_with_identity(
            &client.endpoint(job_id, "direct/complete"),
            &serde_json::json!({}),
            &agent_id,
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), &agent_id,
        ).await?;

        audit::log(
            "cli",
            "buyer/complete_submitted",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("paymentMode=x402"),
                format!("txHash={tx_hash}"),
            ]),
            None,
        );
        println!("✓ 任务 complete 完成（x402），状态 → complete");
        println!("  txHash: {tx_hash}");
    }

    Ok(())
}
