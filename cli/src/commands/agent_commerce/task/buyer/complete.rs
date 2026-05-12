//! 确认完成
//!
//! 买家动作：确认完成 — onchainos agent complete
//!
//! 根据支付方式分流：
//! - escrow: pre-complete(orderId,deadline) → 签 digest → complete(signatureData) → 签 uopHash → broadcast（释放担保款）
//! - non-escrow: a2a_pay 支付 → /direct/complete 单签 → broadcast（先交付后支付）

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::util::{fetch_token_detail, fetch_provider_address};
use crate::commands::agent_commerce::task::common::{self, PaymentMode};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay::{self, PayParams};

/// complete — 验收通过
pub async fn handle_complete(
    client: &mut TaskApiClient,
    job_id: &str,
    payment_id: Option<&str>,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
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

        println!("✓ 任务验收通过（担保），状态 → complete，款项已释放");
        println!("  txHash: {}", result.tx_hash);
    } else if payment_mode == PaymentMode::NonEscrow {
        // ── 非担保：a2a_pay 支付 → /direct/complete 单签（先交付后支付）────────
        let pid = payment_id.ok_or_else(|| {
            anyhow::anyhow!(
                "非担保 complete 需要 --payment-id（由卖家通过 XMTP 传递）\n  \
                 用法：onchainos agent complete {job_id} --payment-id <paymentId> --token-symbol <sym> --token-amount <amt>"
            )
        })?;

        let symbol = token_symbol
            .map(|s| s.to_string())
            .or_else(|| task["paymentTokenSymbol"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "USDT".to_string());
        let amount_str = token_amount
            .map(|s| s.to_string())
            .or_else(|| task["tokenAmount"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        if amount_str.is_empty() {
            bail!("无法确定支付金额，请传入 --token-amount");
        }

        let provider_agent_id = task["providerAgentId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("任务详情缺少 providerAgentId"))?;
        let provider_address = fetch_provider_address(provider_agent_id).await?;

        // 余额预检
        let amt: f64 = amount_str.parse().unwrap_or(0.0);
        if amt > 0.0 {
            common::ensure_sufficient_balance(amt, &symbol).await?;
        }

        // 查询 token 合约地址和精度
        let (token_address, decimals) = fetch_token_detail(client, &symbol, &agent_id).await?;
        let amount_minimal = crate::commands::swap::readable_to_minimal_str(&amount_str, decimals)?;

        // a2a_pay::pay() — EIP-3009 支付
        let pay_result = a2a_pay::pay(PayParams {
            payment_id: pid.to_string(),
            amount: amount_minimal,
            currency: token_address,
            recipient_address: provider_address,
        }).await?;
        println!("✓ a2a_pay 支付完成: payment_id={}, status={}", pay_result.payment_id, pay_result.status);
        if let Some(ref tx) = pay_result.tx_hash {
            println!("  pay txHash: {tx}");
        }

        // direct/complete → calldata → 签名 → 广播
        let complete_resp = client.post_with_identity(
            &client.endpoint(job_id, "direct/complete"),
            &serde_json::json!({}),
            &agent_id,
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &complete_resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&complete_resp), &agent_id,
        ).await?;

        println!("✓ 非担保支付 + complete 完成，状态 → complete");
        println!("  txHash: {tx_hash}");
    } else {
        // ── x402 等：/direct/complete 单签（资金已在 accept 阶段支付）────────
        let resp = client.post_with_identity(
            &client.endpoint(job_id, "direct/complete"),
            &serde_json::json!({}),
            &agent_id,
        ).await?;

        let tx_hash = signing::sign_uop_and_broadcast(
            client, &resp["uopData"], &account_id, &address,
            job_id, signing::extract_biz_type(&resp), &agent_id,
        ).await?;

        println!("✓ 任务 complete 完成（x402），状态 → complete");
        println!("  txHash: {tx_hash}");
    }

    Ok(())
}
