//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保双签 / 非担保单签 / x402）— onchainos task confirm-accept
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
pub async fn handle_confirm_accept(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
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
            // 非担保：展示账单 → 用户确认（skill 层控制）→ direct/accept 单签
            print_invoice(http, api, job_id).await?;

            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
            });
            let result = signing::task_sign_and_broadcast_with_headers(
                http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            )
            .await?;
            println!("✓ 已接受卖家 {provider}（非担保支付）");
            println!("  注意：任务完成后需手动转账给卖家（onchainos agent pay {job_id}）");
            println!("  txHash: {}", result.tx_hash);
        }
        PAYMENT_MODE_X402 | "2" => {
            // x402：调用支付模块 + direct/accept 单签
            // [TODO] 支付模块集成 — 当前先走 direct/accept，待 x402 支付模块就绪后补充
            println!("[TODO] x402 支付模块尚未集成，当前走 direct/accept 流程");

            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
            });
            let result = signing::task_sign_and_broadcast_with_headers(
                http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            )
            .await?;
            println!("✓ 已接受卖家 {provider}（x402 支付）");
            println!("  txHash: {}", result.tx_hash);
        }
        other => {
            bail!("不支持的支付方式: {other}，可选: escrow / non_escrow / x402");
        }
    }

    Ok(())
}

/// 从任务详情 API 获取账单信息并展示
async fn print_invoice(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let resp: serde_json::Value = http
        .get(format!("{api}/priapi/v1/aieco/task/{job_id}"))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法查询任务详情: {e}"))?
        .json()
        .await?;

    if resp["code"] != 0 {
        bail!(
            "查询任务失败: {}",
            resp["msg"].as_str().unwrap_or("unknown")
        );
    }

    let task = &resp["data"]["task"];
    let amount = task["tokenAmount"].as_str().unwrap_or("?");
    let token_symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT");
    let provider_addr = task["providerAgentAddress"].as_str().unwrap_or("?");

    println!("── 卖家账单 ──────────────────────────");
    println!("  收款地址: {provider_addr}");
    println!("  金额:     {amount} {token_symbol}");
    println!("  链:       XLayer (chainId=196)");
    println!("──────────────────────────────────────");

    Ok(())
}
