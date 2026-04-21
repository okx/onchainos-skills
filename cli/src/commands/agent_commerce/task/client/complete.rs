//! 确认完成
//!
//! 买家动作：确认完成（验收通过，释放付款）— onchainos task complete
//!
//! 根据支付方式分流：
//! - escrow: pre-complete(712) → 签 digest → complete → 签 uopHash → broadcast
//! - non-escrow: 展示账单 → /direct/complete 单签 → broadcast

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::PAYMENT_MODE_INT_ESCROW;
use crate::commands::agent_commerce::task::signing;

/// complete — 验收通过
pub async fn handle_complete(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");

    // 查询任务详情获取 paymentMode
    let task = query_task(http, api, job_id).await?;
    let payment_mode = task["paymentType"].as_i64().unwrap_or(0) as i32;

    if payment_mode == PAYMENT_MODE_INT_ESCROW {
        // ── 担保：双签 pre-complete → complete ──────────────────────
        let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-complete");
        let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/complete");
        let pre_body = serde_json::json!({});

        let result = signing::task_dual_sign_and_broadcast(
            http,
            &pre_endpoint,
            &pre_body,
            &main_endpoint,
            |signature| serde_json::json!({ "signature": signature }),
            &broadcast,
            &account_id,
            &address,
            &agent_id,
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

        let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/complete");
        let body = serde_json::json!({});

        let result = signing::task_sign_and_broadcast_with_headers(
            http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
        )
        .await?;

        println!("✓ 任务验收通过（非担保），状态 → complete");
        println!("  请完成转账: onchainos agent pay {job_id}");
        println!("  txHash: {}", result.tx_hash);
    }

    Ok(())
}

/// 查询任务详情，返回 task 对象
async fn query_task(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<serde_json::Value> {
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

    Ok(resp["data"]["task"].clone())
}
