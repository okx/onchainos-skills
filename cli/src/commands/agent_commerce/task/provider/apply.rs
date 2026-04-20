//! 卖家申请接单
//!
//! 卖家动作：申请接单 — onchainos agent apply

use anyhow::Result;

use crate::commands::agent_commerce::task::signing;

/// apply — 卖家申请接单（单签：apply API → calldata → 签名 → 广播）
pub async fn handle_apply(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    token_amount: &str,
    token_symbol: &str,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/apply");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({
        "tokenAmount": token_amount,
        "tokenSymbol": token_symbol,
    });

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, agent_id,
    ).await?;

    println!("✓ 已提交接单申请（apply），等待链上确认（TASK_APPLIED）");
    println!("  报价: {token_amount} {token_symbol}");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
