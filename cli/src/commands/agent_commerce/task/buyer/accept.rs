//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保双签 / 非担保单签 / x402 单签）— onchainos task confirm-accept
//!
//! 流程：
//! 1. setPaymentMode（单签上链）
//! 2. 按支付方式分支处理 accept

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use super::x402_flow;
use crate::commands::agent_commerce::task::common::{
    self, PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW, PAYMENT_MODE_X402,
};
use crate::commands::agent_commerce::task::signing;

/// confirm-accept — 确认接受卖家
pub async fn handle_confirm_accept(
    client: &TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client.http(), client.base_url(), job_id).await?;

    // ── Step 1: setPaymentMode（单签 + 广播上链）──────────────────────
    let mode_int = common::payment_mode_to_int(payment_mode);
    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setPaymentMode"),
        &serde_json::json!({ "paymentMode": mode_int }),
        &agent_id,
        &address,
    ).await?;

    signing::sign_uop_and_broadcast(
        client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::TaskAccept,
    ).await?;
    println!("✓ 支付方式已设置: {payment_mode} ({mode_int})");

    // ── Step 2: 按支付方式分支处理 ──────────────────────────────────
    match payment_mode {
        PAYMENT_MODE_ESCROW | "0" => {
            // 担保：双签 pre-accept → 签 digest → accept → 签 uopHash → broadcast
            let pre_body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
            });
            let provider_owned = provider.to_string();
            let result = signing::task_dual_sign_and_broadcast(
                client,
                &client.endpoint(job_id, "pre-accept"),
                &pre_body,
                &client.endpoint(job_id, "accept"),
                move |signature| serde_json::json!({
                    "providerAddress": provider_owned,
                    "providerAgentId": provider_owned,
                    "signature": signature,
                }),
                &account_id,
                &address,
                &agent_id,
                signing::BizContext::TaskAccept,
            )
            .await?;
            println!("✓ 已接受卖家 {provider}（担保支付），资金已托管");
            println!("  txHash: {}", result.tx_hash);
        }
        PAYMENT_MODE_NON_ESCROW | "direct" | "1" => {
            // 非担保：direct/accept 单签（账单在交付完成阶段通过 /direct/complete 展示）
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
            });
            let resp = client.post_with_identity(
                &client.endpoint(job_id, "direct/accept"),
                &body,
                &agent_id,
                &address,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client, &resp["data"]["uopData"], &account_id, &address,
                signing::BizContext::TaskAccept,
            ).await?;
            println!("✓ 已接受卖家 {provider}（非担保支付），状态 → accepted");
            println!("  txHash: {tx_hash}");
        }
        PAYMENT_MODE_X402 | "2" => {
            // x402：参数完全从缓存（/match 接口返回）获取
            let provider_info = super::negotiate::current(job_id)?
                .ok_or_else(|| anyhow::anyhow!("x402: 未找到当前 provider，请先执行 recommend"))?;
            let svc = provider_info.services.first()
                .ok_or_else(|| anyhow::anyhow!("x402: 当前 provider 无服务信息（services 为空）"))?;

            let sym = &svc.fee_token_symbol;
            if sym.is_empty() {
                bail!("x402: 服务信息中 feeTokenSymbol 为空");
            }
            let amt = svc.fee_amount;
            if amt <= 0.0 {
                bail!("x402: 服务信息中 feeAmount 无效 ({amt})");
            }
            let ep = &svc.endpoint;
            if ep.is_empty() {
                bail!("x402: 服务信息中 endpoint 为空");
            }

            // 检查 feeTokenSymbol 与任务创建时 currency 是否一致
            let task_url = format!("{}/priapi/v1/aieco/task/{job_id}", client.base_url());
            let task_resp = client.get(&task_url).await?;
            let task_currency = task_resp["data"]["task"]["paymentTokenSymbol"]
                .as_str().unwrap_or("");
            if !task_currency.is_empty() && !task_currency.eq_ignore_ascii_case(sym) {
                println!("⚠ 注意：Provider 要求的支付币种 ({sym}) 与任务发布时的币种 ({task_currency}) 不同");
                println!("  将以 Provider 要求的 {sym} 进行支付，金额: {amt} {sym}");
                println!("  如需取消，请 Ctrl+C 终止");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }

            // x402 Step 1-2：direct/accept 单签
            let amt_str = format!("{amt}");
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
                "tokenSymbol": sym,
                "tokenAmount": amt_str,
            });
            let resp = client.post_with_identity(
                &client.endpoint(job_id, "direct/accept"),
                &body,
                &agent_id,
                &address,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client, &resp["data"]["uopData"], &account_id, &address,
                signing::BizContext::TaskAccept,
            ).await?;
            println!("✓ 已接受卖家 {provider}（x402 支付），金额: {amt} {sym}");
            println!("  txHash: {tx_hash}");

            // x402 Step 3：调用 x402 支付（请求 endpoint → 402 → 签名 → 重放）
            println!("  x402: 开始调用 Provider endpoint 完成支付 ...");
            let flow_result = x402_flow::x402_request_sign_replay(
                client.http(),
                ep,
                Some(&address),
            ).await?;

            println!("✓ x402 支付完成");
            println!("  endpoint:  {ep}");
            println!("  HTTP 状态: {}", flow_result.response_status);
            if flow_result.response_status == 200 {
                println!("  服务响应: {}", serde_json::to_string_pretty(&flow_result.response_body)
                    .unwrap_or_else(|_| "ok".to_string()));
            }
        }
        other => {
            bail!("不支持的支付方式: {other}，可选: escrow / non_escrow / x402");
        }
    }

    // 清理本地协商状态
    if let Err(e) = super::negotiate::cleanup(job_id) {
        eprintln!("⚠ 清理协商状态失败（可忽略）: {e}");
    }

    Ok(())
}
