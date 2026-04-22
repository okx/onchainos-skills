//! 确认完成
//!
//! 买家动作：确认完成（验收通过，释放付款）— onchainos task complete
//!
//! 根据支付方式分流：
//! - escrow: pre-complete(712) → 签 digest → complete → 签 uopHash → broadcast
//! - non-escrow: 展示账单 → /direct/complete 单签 → broadcast

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PAYMENT_MODE_INT_ESCROW;
use crate::commands::agent_commerce::task::signing;

/// complete — 验收通过
pub async fn handle_complete(client: &TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client.http(), client.base_url(), job_id).await?;

    // 查询任务详情获取 paymentMode
    let url = format!("{}/priapi/v1/aieco/task/{job_id}", client.base_url());
    let resp = client.get(&url).await?;
    let task = &resp["data"]["task"];
    let payment_mode = task["paymentType"].as_i64().unwrap_or(0) as i32;

    if payment_mode == PAYMENT_MODE_INT_ESCROW {
        // ── 担保：双签 pre-complete → complete ──────────────────────
        let result = signing::task_dual_sign_and_broadcast(
            client,
            &client.endpoint(job_id, "pre-complete"),
            &serde_json::json!({}),
            &client.endpoint(job_id, "complete"),
            |signature| serde_json::json!({ "signature": signature }),
            &account_id,
            &address,
            &agent_id,
            signing::BizContext::TaskComplete,
        )
        .await?;

        println!("✓ 任务验收通过（担保），状态 → complete，款项已释放");
        println!("  txHash: {}", result.tx_hash);
    } else {
        // ── 非担保 / x402：展示账单 → /direct/complete 单签 ────────
        let amount = task["tokenAmount"].as_str().unwrap_or("?");
        let token_symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT");
        let provider_addr = task["providerAgentAddress"].as_str().unwrap_or("?");

        println!("── 卖家账单 ──────────────────────────");
        println!("  收款地址: {provider_addr}");
        println!("  金额:     {amount} {token_symbol}");
        println!("  链:       XLayer (chainId=196)");
        println!("──────────────────────────────────────");

        let resp = client.post_with_identity(
            &client.endpoint(job_id, "direct/complete"),
            &serde_json::json!({}),
            &agent_id,
            &address,
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &resp["data"]["uopData"], &account_id, &address,
            signing::BizContext::TaskComplete,
        ).await?;

        println!("✓ 任务验收通过（非担保），状态 → complete");
        println!("  请完成转账: onchainos agent pay {job_id}");
        println!("  txHash: {tx_hash}");
    }

    Ok(())
}
