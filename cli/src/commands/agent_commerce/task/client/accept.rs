//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保双签 / 非担保单签 / x402 单签）— onchainos task confirm-accept
//!
//! 流程：
//! 1. setPaymentMode（单签上链）
//! 2. 按支付方式分支处理 accept

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    self, PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW, PAYMENT_MODE_X402,
};
use crate::commands::agent_commerce::task::signing;

/// confirm-accept — 确认接受卖家
#[allow(clippy::too_many_arguments)]
pub async fn handle_confirm_accept(
    client: &TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
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
            // x402：direct/accept 单签，需传 tokenSymbol 和 tokenAmount
            let sym = token_symbol
                .ok_or_else(|| anyhow::anyhow!("x402 模式必须指定 --token-symbol"))?;
            let amt = token_amount
                .ok_or_else(|| anyhow::anyhow!("x402 模式必须指定 --token-amount"))?;

            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
                "tokenSymbol": sym,
                "tokenAmount": amt,
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
