//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保 / 非担保 / x402）— onchainos task confirm-accept
//!
//! 流程：
//! 1. setPaymentMode（单签上链）
//! 2. 调用 a2a_pay::pay() 完成 EIP-3009 支付签名（escrow / non_escrow）
//! 3. 按支付方式调用 accept API 获取 calldata → 签名 → 广播上链
//!
//! 支付方式差异：
//! - escrow:      a2a_pay::pay(→signature) → accept(signatureData) → sign uop → broadcast
//! - non_escrow:  a2a_pay::pay() → direct/accept → sign uop → broadcast
//! - x402:        direct/accept → sign uop → broadcast → x402_request_sign_replay
//!
//! 接口文档：https://okg-block.sg.larksuite.com/wiki/UumqwSyM5i1AuakBNLClJo9igIb
//! 支付设计：https://okg-block.sg.larksuite.com/docx/CwWbd6eCOopgq6x6VwTlWEivgrc

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    self, PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW, PAYMENT_MODE_X402,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay::{self, PayParams};
use super::x402_flow;

/// confirm-accept — 确认接受卖家
#[allow(clippy::too_many_lines)]
pub async fn handle_confirm_accept(
    client: &mut TaskApiClient,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
    payment_id: Option<&str>,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id).await?;

    // ── Step 1: setPaymentMode（单签 + 广播上链）──────────────────────
    let mode_int = common::payment_mode_to_int(payment_mode);
    let resp = client.post_with_identity(
        &client.endpoint(job_id, "setPaymentMode"),
        &serde_json::json!({ "paymentMode": mode_int }),
        &agent_id,
    ).await?;

    signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::BizContext::JobSetPaymentMode,
    ).await?;
    println!("✓ 支付方式已设置: {payment_mode} ({mode_int})");

    // ── Step 2: 按支付方式分支处理 ──────────────────────────────────
    match payment_mode {
        PAYMENT_MODE_ESCROW | "0" => {
            // ── 担保支付 (Escrow) ───────────────────────────────────
            let pid = payment_id.ok_or_else(|| {
                anyhow::anyhow!("担保支付需要 --payment-id（由卖家通过 XMTP 传递）")
            })?;

            // 从任务详情获取金额和币种
            let task_resp = client.get(&client.task_path(job_id)).await?;
            let task = &task_resp["task"];
            let amount = task["tokenAmount"].as_str().unwrap_or("0").to_string();
            let symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT").to_string();

            // Escrow 模式下 EIP-3009 的 recipient 是 escrow 合约地址（非 provider），
            // 需通过 prePayTaskInfo 获取正确的 escrowContract 地址。
            // a2a_pay::pay() 内部会校验 recipient_address 与 challenge 中的 recipient 一致。
            let pre_pay_body = serde_json::json!({ "tokenSymbol": &symbol });
            let pre_pay_info = client
                .post_with_identity(
                    &client.endpoint(job_id, "prePayTaskInfo"),
                    &pre_pay_body,
                    &agent_id,
                )
                .await?;
            let recipient = pre_pay_info["escrowContract"]
                .as_str()
                .or_else(|| pre_pay_info["recipient"].as_str())
                .unwrap_or(provider)
                .to_string();

            // Step 2a: a2a_pay::pay() — EIP-3009 支付签名
            // PayParams.amount 应与卖家 create_payment 时传入的 amount 一致（整数单位，
            // 如 "50" 表示 50 USDT，后端按 token decimals 换算）。
            let pay_result = a2a_pay::pay(PayParams {
                payment_id: pid.to_string(),
                amount: amount.clone(),
                symbol: symbol.clone(),
                recipient_address: recipient,
            }).await?;
            println!("✓ a2a_pay 支付完成: payment_id={}, status={}", pay_result.payment_id, pay_result.status);
            if let Some(ref tx) = pay_result.tx_hash {
                println!("  pay txHash: {tx}");
            }

            // Step 2b: accept — 直接用 pay_result 中的 EIP-3009 签名
            // pay() 返回的 signature / valid_after / valid_before 即 ERC-3009 授权签名，
            // 无需再走 preAccept → sign digest 双签流程。
            // accept 入参:
            //   providerAddress:  必填, hex 地址
            //   providerAgentId:  必填
            //   signatureData:    必填 { signature, validAfter, validBefore }
            //   tokenSymbol:      可选
            //   tokenAmount:      可选
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
                "signatureData": {
                    "signature": pay_result.signature,
                    "validAfter": pay_result.valid_after,
                    "validBefore": pay_result.valid_before,
                },
                "tokenSymbol": symbol,
                "tokenAmount": amount,
            });
            let resp = client.post_with_identity(
                &client.endpoint(job_id, "accept"),
                &body,
                &agent_id,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client, &resp["uopData"], &account_id, &address,
                job_id, signing::BizContext::JobAccept,
            ).await?;
            println!("✓ 已接受卖家 {provider}（担保支付），资金已托管");
            println!("  txHash: {tx_hash}");
        }
        PAYMENT_MODE_NON_ESCROW | "direct" | "1" => {
            // ── 非担保支付 (Charge / Direct) ────────────────────────
            let pid = payment_id.ok_or_else(|| {
                anyhow::anyhow!("非担保支付需要 --payment-id（由卖家通过 XMTP 传递）")
            })?;

            // 从任务详情获取金额、币种和卖家地址
            let task_resp = client.get(&client.task_path(job_id)).await?;
            let task = &task_resp["task"];
            let amount = task["tokenAmount"].as_str().unwrap_or("0").to_string();
            let symbol = task["paymentTokenSymbol"].as_str().unwrap_or("USDT").to_string();
            // Charge 模式 EIP-3009 recipient = 卖家钱包地址（seller 创建 payment 时传入）
            let recipient = task["providerAddress"]
                .as_str()
                .unwrap_or(provider)
                .to_string();

            // Step 2a: a2a_pay::pay() — EIP-3009 支付签名
            // PayParams.amount 应与卖家 create_payment_charge 时传入的 amount 一致。
            let pay_result = a2a_pay::pay(PayParams {
                payment_id: pid.to_string(),
                amount: amount.clone(),
                symbol: symbol.clone(),
                recipient_address: recipient,
            }).await?;
            println!("✓ a2a_pay 支付完成: payment_id={}, status={}", pay_result.payment_id, pay_result.status);
            if let Some(ref tx) = pay_result.tx_hash {
                println!("  pay txHash: {tx}");
            }

            // Step 2b: direct/accept → calldata(uopData) → 签名 → 广播
            // direct/accept 入参:
            //   providerAddress:  必填
            //   providerAgentId:  必填
            //   tokenSymbol:      可选
            //   tokenAmount:      可选 (decimal string, 无小数点)
            let body = serde_json::json!({
                "providerAddress": provider,
                "providerAgentId": provider,
                "tokenSymbol": symbol,
                "tokenAmount": amount,
            });
            let resp = client.post_with_identity(
                &client.endpoint(job_id, "direct/accept"),
                &body,
                &agent_id,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client, &resp["uopData"], &account_id, &address,
                job_id, signing::BizContext::JobAccept,
            ).await?;
            println!("✓ 已接受卖家 {provider}（非担保支付），状态 → accepted");
            println!("  txHash: {tx_hash}");
        }
        PAYMENT_MODE_X402 | "2" => {
            // ── x402 支付：参数从缓存（/match 接口返回）获取 ────────
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
            let task_resp = client.get(&client.task_path(job_id)).await?;
            let task_currency = task_resp["task"]["paymentTokenSymbol"]
                .as_str().unwrap_or("");
            if !task_currency.is_empty() && !task_currency.eq_ignore_ascii_case(sym) {
                println!("⚠ 注意：Provider 要求的支付币种 ({sym}) 与任务发布时的币种 ({task_currency}) 不同");
                println!("  将以 Provider 要求的 {sym} 进行支付，金额: {amt} {sym}");
                println!("  如需取消，请 Ctrl+C 终止");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }

            // x402 Step 1-2：direct/accept 单签（paymentMode=2 时走 direct/accept）
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
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client, &resp["uopData"], &account_id, &address,
                job_id, signing::BizContext::JobAccept,
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
