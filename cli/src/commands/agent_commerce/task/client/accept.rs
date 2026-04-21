//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保双签 / 非担保单签 / x402 单签）— onchainos task confirm-accept
//!
//! 流程：
//! 1. setPaymentMode（单签上链）
//! 2. 按支付方式分支处理 accept

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::{
    self, PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW, PAYMENT_MODE_X402,
};
use crate::commands::agent_commerce::task::signing;

/// confirm-accept — 确认接受卖家
#[allow(clippy::too_many_arguments)]
pub async fn handle_confirm_accept(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
    token_symbol: Option<&str>,
    token_amount: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");

    // ── Step 1: setPaymentMode（单签 + 广播上链）──────────────────────
    let mode_int = common::payment_mode_to_int(payment_mode);
    let set_mode_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/setPaymentMode");
    let set_mode_body = serde_json::json!({ "paymentMode": mode_int });

    let _mode_result = signing::task_sign_and_broadcast_with_headers(
        http,
        &set_mode_endpoint,
        &set_mode_body,
        &broadcast,
        &account_id,
        &address,
        &agent_id,
    )
    .await?;
    println!("✓ 支付方式已设置: {payment_mode} ({mode_int})");

    // ── Step 2: 按支付方式分支处理 ──────────────────────────────────
    match payment_mode {
        PAYMENT_MODE_ESCROW | "0" => {
            // 担保：双签 pre-accept → 签 digest → accept → 签 uopHash → broadcast
            let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-accept");
            let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/accept");
            let pre_body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
            });
            let provider_owned = provider.to_string();
            let result = signing::task_dual_sign_and_broadcast(
                http,
                &pre_endpoint,
                &pre_body,
                &main_endpoint,
                move |signature| serde_json::json!({
                    "providerAddress": provider_owned,
                    "providerAgentId": provider_owned,
                    "signature": signature,
                }),
                &broadcast,
                &account_id,
                &address,
                &agent_id,
            )
            .await?;
            println!("✓ 已接受卖家 {provider}（担保支付），资金已托管");
            println!("  txHash: {}", result.tx_hash);
        }
        PAYMENT_MODE_NON_ESCROW | "direct" | "1" => {
            // 非担保：direct/accept 单签（账单在交付完成阶段通过 /direct/complete 展示）
            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
            });
            let result = signing::task_sign_and_broadcast_with_headers(
                http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            )
            .await?;
            println!("✓ 已接受卖家 {provider}（非担保支付），状态 → accepted");
            println!("  txHash: {}", result.tx_hash);
        }
        PAYMENT_MODE_X402 | "2" => {
            // x402：direct/accept 单签，需传 tokenSymbol 和 tokenAmount
            let sym = token_symbol
                .ok_or_else(|| anyhow::anyhow!("x402 模式必须指定 --token-symbol"))?;
            let amt = token_amount
                .ok_or_else(|| anyhow::anyhow!("x402 模式必须指定 --token-amount"))?;

            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
                "tokenSymbol": sym,
                "tokenAmount": amt,
            });
            let result = signing::task_sign_and_broadcast_with_headers(
                http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            )
            .await?;
            println!("✓ 已接受卖家 {provider}（x402 支付），金额: {amt} {sym}");
            println!("  txHash: {}", result.tx_hash);
        }
        other => {
            bail!("不支持的支付方式: {other}，可选: escrow / non_escrow / x402");
        }
    }

    Ok(())
}
