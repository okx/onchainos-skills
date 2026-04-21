//! 仲裁相关命令
//!
//! - dispute raise: 发起仲裁（卖家）
//! - dispute evidence: 提交证据（双方）
//! - dispute info: 查询争议详情

use anyhow::Result;
use serde_json::Value;

use super::DisputeCommand;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn run_evidence(cmd: DisputeCommand, client: &TaskApiClient) -> Result<()> {
    match cmd {
        DisputeCommand::Raise { job_id, reason } => {
            let (account_id, address, agent_id) =
                signing::resolve_wallet_and_agent_for_provider(client.http(), client.base_url(), &job_id).await?;
            let body = serde_json::json!({ "reason": reason });

            let resp = client.post_with_identity(
                &client.endpoint(&job_id, "dispute"), &body, &agent_id, &address,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client.http(), &client.broadcast_url(), &resp["data"]["uopData"], &account_id, &address,
            ).await?;

            println!("✓ 已发起仲裁，等待链上确认（TASK_DISPUTED）");
            println!("  原因: {reason}");
            println!("  txHash: {tx_hash}");
        }
        DisputeCommand::Evidence { job_id, summary, .. } => {
            let (account_id, address, agent_id) =
                signing::resolve_wallet_and_agent_for_provider(client.http(), client.base_url(), &job_id).await?;
            let body = serde_json::json!({ "text": summary });

            let resp = client.post_with_identity(
                &client.endpoint(&job_id, "evidence"), &body, &agent_id, &address,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client.http(), &client.broadcast_url(), &resp["data"]["uopData"], &account_id, &address,
            ).await?;

            println!("✓ 证据已提交");
            println!("  jobId:  {job_id}");
            println!("  摘要:   {summary}");
            println!("  txHash: {tx_hash}");
        }
        DisputeCommand::Info { dispute_id } => {
            let url = format!("{}/priapi/v1/aieco/task/dispute/{}", client.base_url(), dispute_id);
            let resp = client.get(&url).await?;
            print_dispute_info(&dispute_id, &resp["data"]);
        }
    }
    Ok(())
}

fn print_dispute_info(dispute_id: &str, data: &Value) {
    println!("争议详情：");
    println!("  disputeId: {dispute_id}");
    println!("  jobId:     {}", data["jobId"].as_str().unwrap_or("?"));
    println!("  状态:      {}", data["statusStr"].as_str().unwrap_or("?"));
    println!("  发起方:    {}", data["raiserAddress"].as_str().unwrap_or("?"));
    println!("  发起原因:  {}", data["reason"].as_str().unwrap_or("?"));
    println!("  创建时间:  {}", data["createTime"].as_str().unwrap_or("?"));

    if let Some(evs) = data["evidences"].as_array() {
        println!("\n证据列表（共 {} 条）：", evs.len());
        for (i, ev) in evs.iter().enumerate() {
            println!(
                "  {}. 提交方: {}  类型: {}",
                i + 1,
                ev["submitter"].as_str().unwrap_or("?"),
                ev["type"].as_str().unwrap_or("?"),
            );
            println!("     摘要: {}", ev["summary"].as_str().unwrap_or("?"));
            if let Some(url) = ev["fileUrl"].as_str() {
                println!("     文件: {url}");
            }
        }
    } else {
        println!("\n暂无证据提交");
    }
}
