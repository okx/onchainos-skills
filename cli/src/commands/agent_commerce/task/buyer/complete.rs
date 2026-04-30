//! 确认完成
//!
//! 买家动作：确认完成（验收通过，释放付款）— onchainos task complete
//!
//! 根据支付方式分流：
//! - escrow: pre-complete(orderId,deadline) → 签 digest → complete(signatureData) → 签 uopHash → broadcast
//! - non-escrow: 展示账单 → /direct/complete 单签 → broadcast

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PAYMENT_MODE_INT_ESCROW;
use crate::commands::agent_commerce::task::signing;

/// complete — 验收通过
pub async fn handle_complete(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    // 查询任务详情获取 paymentMode
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
    let task = &resp;
    let payment_mode = task["paymentMode"].as_i64().unwrap_or(0) as i32;

    if payment_mode == PAYMENT_MODE_INT_ESCROW {
        // ── 担保：双签 pre-complete → complete ──────────────────────
        // TODO: deadline 策略待确认，暂时使用当前时间 + 1 小时
        let deadline = chrono::Utc::now().timestamp() + 3600;

        // Step 1: pre-complete → typedData (712 标准，不需要 sessionCert)
        let pre_body = serde_json::json!({
            "deadline": deadline,
        });
        let pre_resp = client.post_with_identity(
            &client.endpoint(job_id, "pre-complete"),
            &pre_body,
            &agent_id,
        ).await?;
        let typed_data = &pre_resp["typedData"];
        if typed_data.is_null() {
            anyhow::bail!("pre-complete 未返回 typedData");
        }
        let nonce = pre_resp["nonce"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Step 2: EIP-712 签名 typedData（gen-msg-hash → ed25519 → sign-msg）
        let signature = signing::sign_typed_data(typed_data, &address).await?;

        // Step 3: complete (signatureData + sessionCert，sessionCert 由 post_with_identity 自动注入)
        let mut sig_data = serde_json::json!({
            "signature": signature,
            "deadline": deadline,
        });
        if !nonce.is_empty() {
            sig_data["nonce"] = serde_json::json!(nonce);
        }
        let main_body = serde_json::json!({
            "signatureData": sig_data,
        });
        let main_resp = client.post_with_identity(
            &client.endpoint(job_id, "complete"),
            &main_body,
            &agent_id,
        ).await?;

        // Step 4: 签 uopHash + broadcast
        let tx_hash = signing::sign_uop_and_broadcast(
            client, &main_resp["uopData"], &account_id, &address,
            job_id, signing::BizContext::JobComplete, &agent_id,
        ).await?;

        println!("✓ 任务验收通过（担保），状态 → complete，款项已释放");
        println!("  txHash: {tx_hash}");
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
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::BizContext::JobComplete, &agent_id,
        ).await?;

        println!("✓ 任务验收通过（非担保），状态 → complete");
        println!("  请完成转账: onchainos agent pay {job_id}");
        println!("  txHash: {tx_hash}");
    }

    Ok(())
}
