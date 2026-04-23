//! 获取支付预信息（prePayTaskInfo）
//!
//! 卖家在 TASK_APPLIED 后调用，拿到链上支付参数（recipient / evaluator / hook / windows 等），
//! 组装付款单发给买家。对应后端 `POST /priapi/v1/aieco/task/{jobId}/prePayTaskInfo`。

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_get_payment(
    client: &TaskApiClient,
    job_id: &str,
    token_symbol: &str,
) -> Result<()> {
    let (_, address, agent_id) =
        signing::resolve_wallet_and_agent_for_provider(client, job_id).await?;

    let body = serde_json::json!({ "tokenSymbol": token_symbol });
    let resp = client
        .post_with_identity(
            &client.endpoint(job_id, "prePayTaskInfo"),
            &body,
            &agent_id,
            &address,
        )
        .await?;

    let d = &resp["data"];
    println!("支付预信息（prePayTaskInfo）：");
    println!("  jobId:            {job_id}");
    println!("  tokenSymbol:      {token_symbol}");
    println!("  currency:         {}", d["currency"].as_str().unwrap_or("?"));
    println!("  recipient:        {}", d["recipient"].as_str().unwrap_or("?"));
    println!("  receiver:         {}", d["receiver"].as_str().unwrap_or("?"));
    println!("  evaluator:        {}", d["evaluator"].as_str().unwrap_or("?"));
    println!("  submitWindow:     {}", d["submitWindow"].as_str().unwrap_or("?"));
    println!("  disputeWindow:    {}", d["disputeWindow"].as_str().unwrap_or("?"));
    println!("  evaluateWindow:   {}", d["evaluateWindow"].as_str().unwrap_or("?"));
    println!("  completedWindow:  {}", d["completedWindow"].as_str().unwrap_or("?"));
    println!("  hook:             {}", d["hook"].as_str().unwrap_or("?"));
    println!("  hookData:         {}", d["hookData"].as_str().unwrap_or("?"));
    println!("  salt:             {}", d["salt"].as_str().unwrap_or("?"));
    println!("  expiredAt:        {}", d["expiredAt"].as_str().unwrap_or("?"));

    Ok(())
}
