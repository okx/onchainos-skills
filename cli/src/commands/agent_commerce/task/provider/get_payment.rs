//! 创建 a2a-pay charge 付款单（仅 non_escrow 路径）
//!
//! 卖家在协商完成后调用。**只服务非担保（non_escrow / charge）**：
//!   1. POST /priapi/v1/aieco/task/{jobId}/prePayTaskInfo 拿链上付款参数
//!   2. 取 `recipient` + 协商价格调 a2a-pay `create_payment_charge` → `payment_id`
//!   3. stdout 输出 `paymentId` —— 卖家 sub agent 通过 `xmtp_send` 把它发给买家，
//!      买家用 `payment_id` 调 `pay()` 完成 EIP-3009 签名 + credential 提交
//!
//! escrow 路径**不再走本命令**：买家在 `confirm-accept` 时自行生成付款单，
//! 卖家收到 provider_applied 通知只需 xmtp 通知"已接单，请 confirm-accept"即可。
//!
//! 对应文档：A2A Pay Seller / Buyer 接入示例（larksuite docx CwWbd6eCOopgq6x6VwTlWEivgrc）

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PaymentMode;
use crate::commands::payment::a2a_pay::{
    create_payment_charge, ChargeParams, CreatePaymentOutput,
};

pub async fn handle_get_payment(
    client: &mut TaskApiClient,
    job_id: &str,
    token_symbol: &str,
    token_amount: &str,
    payment_mode: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id 必填，传卖家自己的 agentId");
    }

    // escrow 路径已经下线本命令——买家在 confirm-accept 时自己生成付款单
    let mode = PaymentMode::from_str(payment_mode);
    if mode == PaymentMode::Escrow {
        bail!(
            "escrow 担保路径不需要 get-payment：买家在 confirm-accept 时自己生成付款单。\n\
             provider_applied 通知到达后，只需 xmtp_send 一条「已接单，请 confirm-accept」消息即可。"
        );
    }
    if mode != PaymentMode::NonEscrow {
        bail!(
            "unsupported payment mode '{payment_mode}', expected 'non_escrow'"
        );
    }

    // ── Step 1: 拉链上付款预信息（取 recipient）─────────────────────
    // 注意 non_escrow 路径在 status=open 阶段就调，task.providerAgentId 还没写入；
    // 必须用调用方显式传的 agent_id 走 agenticId header，不能从任务详情反查。
    // tokenAmount/amount 都不带 decimals 精度，传 whole tokens（"10" 表示 10 USDG）。
    // 后端 Java 端字段叫 amount，spec 里写的是 tokenAmount，两个都发不会冲突。
    let body = serde_json::json!({
        "tokenSymbol": token_symbol,
        "tokenAmount": token_amount,
        "amount": token_amount,
    });
    let pre = client
        .post_with_identity(
            &client.endpoint(job_id, "prePayTaskInfo"),
            &body,
            agent_id,
        )
        .await?;

    // ── Step 2: 调 a2a-pay /payment/create (charge) ────────────────────
    let recipient = require_str(&pre, "recipient")?;
    let out: CreatePaymentOutput = create_payment_charge(ChargeParams {
        amount: token_amount.to_string(),
        symbol: token_symbol.to_string(),
        recipient,
        description: None,
        external_id: Some(format!("task-{job_id}")),
        expires_in: None,
        realm: None,
    })
    .await?;

    // ── Step 3: 输出 payment_id（卖家 sub agent 走 xmtp_send 发给买家）─
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "paymentId": out.payment_id,
            "deliveries": out.deliveries,
        }))?
    );

    Ok(())
}

fn require_str(v: &Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("prePayTaskInfo response missing '{key}'"))
}
