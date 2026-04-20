//! 确认接单 + Fund
//!
//! 买家动作：确认接单（担保双签 / 非担保单签）— onchainos task confirm-accept

use anyhow::Result;

use crate::commands::agent_commerce::task::common::{PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW};
use crate::commands::agent_commerce::task::signing;

/// confirm-accept — 确认接受卖家（担保双签 / 非担保单签）
pub async fn handle_confirm_accept(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
) -> Result<()> {
    let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(http, api, job_id).await?;
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");

    if payment_mode == PAYMENT_MODE_NON_ESCROW {
        // 非担保：标准单签 direct/accept
        let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
        let body = serde_json::json!({
            "providerAddress": provider,
            "providerAgentId": provider,
        });
        let result = signing::task_sign_and_broadcast_with_headers(
            http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
        ).await?;
        println!("✓ 已接受卖家 {provider}（非担保支付），任务状态 → accepted");
        println!("  注意：任务完成后需手动转账给卖家");
        println!("  txHash: {}", result.tx_hash);
    } else {
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
                "paymentMode": PAYMENT_MODE_ESCROW,
                "signature": signature,
            }),
            &broadcast,
            &account_id,
            &address,
            &agent_id,
        ).await?;
        println!("✓ 已接受卖家 {provider}（担保支付），任务状态 → accepted");
        println!("  txHash: {}", result.tx_hash);
    }
    Ok(())
}
