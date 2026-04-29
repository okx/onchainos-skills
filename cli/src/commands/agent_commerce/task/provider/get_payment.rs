//! 获取支付预信息 + 创建 a2a-pay 付款单
//!
//! 卖家在协商完成后调用。担保（escrow）和非担保（non_escrow / charge）两条路径
//! 共用本方法，区别在于把 prePayTaskInfo 的链上参数 + 协商价格喂给 a2a-pay 的
//! 哪个 create 接口：
//!   - escrow      → `create_payment_escrow` （锁仓 + 仲裁 + 工期窗口）
//!   - non_escrow  → `create_payment_charge` （EIP-3009 直转，无托管）
//!
//! 流程：
//!   1. POST /priapi/v1/aieco/task/{jobId}/prePayTaskInfo 拿链上付款参数
//!      （recipient / evaluator / hook / salt / windows / expiredAt 等）
//!   2. 按支付方式调用 a2a-pay `/payment/create`，拿到 `payment_id`
//!   3. stdout 输出 `paymentId` —— 卖家 sub agent 后续通过 `xmtp_send` 把它发给买家，
//!      买家用 `payment_id` 调 `pay()` 完成签名 + credential 提交
//!
//! 对应文档：A2A Pay Seller / Buyer 接入示例（larksuite docx CwWbd6eCOopgq6x6VwTlWEivgrc）

use anyhow::{anyhow, bail, Context, Result};
use chrono::{TimeZone, Utc};
use serde_json::Value;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::{
    PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::payment::a2a_pay::{
    create_payment_charge, create_payment_escrow, ChargeParams, CreatePaymentOutput,
    EscrowDetails, EscrowParams,
};

pub async fn handle_get_payment(
    client: &mut TaskApiClient,
    job_id: &str,
    token_symbol: &str,
    token_amount: &str,
    payment_mode: &str,
) -> Result<()> {
    let (_, _, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;

    // ── Step 1: 拉链上付款预信息 ───────────────────────────────────────
    let body = serde_json::json!({ "tokenSymbol": token_symbol });
    let pre = client
        .post_with_identity(
            &client.endpoint(job_id, "prePayTaskInfo"),
            &body,
            &agent_id,
        )
        .await?;

    // ── Step 2: 按支付方式分流，调 a2a-pay /payment/create ────────────
    let out: CreatePaymentOutput = match payment_mode {
        PAYMENT_MODE_NON_ESCROW => {
            let recipient = require_str(&pre, "recipient")?;
            create_payment_charge(ChargeParams {
                amount: token_amount.to_string(),
                symbol: token_symbol.to_string(),
                recipient,
                description: None,
                external_id: Some(format!("task-{job_id}")),
                expires_in: None,
                realm: None,
            })
            .await?
        }
        PAYMENT_MODE_ESCROW => {
            let escrow_contract = require_str(&pre, "escrowContract")?;
            let provider = require_str(&pre, "recipient")?;
            let receiver = pre["receiver"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| provider.clone());
            let arbitrator = require_str(&pre, "evaluator")?;
            // 后端 windows 字段是字符串数字（如 "86400"）— 解析为 u64
            let submit_window = parse_window(&pre, "submitWindow")?;
            let dispute_window = parse_window(&pre, "disputeWindow")?;
            // 兼容两种命名：arbitrationWindow（规范）/ evaluateWindow（mock 旧名）
            let arbitration_window = parse_window(&pre, "arbitrationWindow")
                .or_else(|_| parse_window(&pre, "evaluateWindow"))?;
            let termination_window = parse_window(&pre, "terminationWindow")
                .or_else(|_| parse_window(&pre, "completedWindow"))?;
            let expired_at = normalize_expired_at(&pre)?;
            let hook = require_str(&pre, "hook")?;
            let hook_data = pre["hookData"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| "0x".to_string());
            let salt = require_str(&pre, "salt")?;

            create_payment_escrow(EscrowParams {
                amount: token_amount.to_string(),
                symbol: token_symbol.to_string(),
                description: None,
                external_id: Some(format!("task-{job_id}")),
                expires_in: None,
                realm: None,
                escrow: EscrowDetails {
                    escrow_contract,
                    provider,
                    receiver,
                    arbitrator,
                    submit_window,
                    dispute_window,
                    arbitration_window,
                    termination_window,
                    expired_at,
                    hook,
                    hook_data,
                    salt,
                },
            })
            .await?
        }
        other => bail!(
            "unsupported payment mode '{other}', expected '{PAYMENT_MODE_ESCROW}' or '{PAYMENT_MODE_NON_ESCROW}'"
        ),
    };

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

fn parse_window(v: &Value, key: &str) -> Result<u64> {
    let raw = v
        .get(key)
        .ok_or_else(|| anyhow!("prePayTaskInfo response missing '{key}'"))?;
    if let Some(n) = raw.as_u64() {
        return Ok(n);
    }
    if let Some(s) = raw.as_str() {
        return s.parse::<u64>().with_context(|| format!("{key} parse u64"));
    }
    bail!("{key} must be a number or numeric string")
}

/// 后端可能返回 unix 秒（字符串或整数）或已经是 RFC 3339 — 统一转成 RFC 3339。
fn normalize_expired_at(v: &Value) -> Result<String> {
    let raw = v
        .get("expiredAt")
        .ok_or_else(|| anyhow!("prePayTaskInfo response missing 'expiredAt'"))?;
    if let Some(n) = raw.as_i64() {
        return unix_to_rfc3339(n);
    }
    if let Some(s) = raw.as_str() {
        if let Ok(n) = s.parse::<i64>() {
            return unix_to_rfc3339(n);
        }
        // 已经是 RFC 3339（含 'T' 或 '-'）— 原样透传
        return Ok(s.to_string());
    }
    bail!("expiredAt must be unix seconds or RFC 3339 string")
}

fn unix_to_rfc3339(secs: i64) -> Result<String> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .map(|t| t.to_rfc3339())
        .ok_or_else(|| anyhow!("expiredAt unix seconds out of range: {secs}"))
}
