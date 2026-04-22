//! 买家仲裁证据提交
//!
//! - evidence: 提交证据（/evidence/upload → calldata → 签名 → 广播）
//! - info: 查询争议详情（GET 只读）

use anyhow::Result;

use super::BuyerDisputeCommand;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn run_buyer_dispute(cmd: BuyerDisputeCommand, client: &TaskApiClient) -> Result<()> {
    match cmd {
        // ── evidence（提交证据：/evidence/upload → calldata → 签名 → 广播）
        BuyerDisputeCommand::Evidence {
            job_id, summary, ..
        } => {
            let (account_id, address, agent_id) =
                signing::resolve_wallet_and_agent_for_task(client.http(), client.base_url(), &job_id).await?;

            let resp = client.post_with_identity(
                &client.endpoint(&job_id, "evidence/upload"),
                &serde_json::json!({ "text": summary }),
                &agent_id,
                &address,
            ).await?;

            let tx_hash = signing::sign_uop_and_broadcast(
                client, &resp["data"]["uopData"], &account_id, &address,
                signing::BizContext::DisputeCreate,
            ).await?;

            println!("✓ 证据已提交");
            println!("  jobId:  {job_id}");
            println!("  摘要:   {summary}");
            println!("  txHash: {tx_hash}");
        }
        // ── info（GET 只读查询）
        BuyerDisputeCommand::Info { dispute_id } => {
            let url = format!("{}/priapi/v1/aieco/task/dispute/{dispute_id}", client.base_url());
            let resp = client.get(&url).await?;

            let d = &resp["data"];
            println!("争议详情：");
            println!("  disputeId: {dispute_id}");
            println!("  jobId:     {}", d["jobId"].as_str().unwrap_or("?"));
            println!("  状态:      {}", d["statusStr"].as_str().unwrap_or("?"));
            println!("  发起方:    {}", d["raiserAddress"].as_str().unwrap_or("?"));
            println!("  发起原因:  {}", d["reason"].as_str().unwrap_or("?"));
            println!("  创建时间:  {}", d["createTime"].as_str().unwrap_or("?"));

            if let Some(evs) = d["evidences"].as_array() {
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
    }
    Ok(())
}
