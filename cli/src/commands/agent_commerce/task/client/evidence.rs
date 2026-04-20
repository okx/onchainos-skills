//! 买家仲裁证据提交
//!
//! - evidence: 提交证据（/evidence/upload → calldata → 签名 → 广播）
//! - info: 查询争议详情（GET 只读）

use anyhow::{bail, Result};

use super::BuyerDisputeCommand;
use crate::commands::agent_commerce::task::signing;

fn task_api_url() -> String {
    std::env::var("TASK_API_URL").unwrap_or_else(|_| "http://127.0.0.1:9001".to_string())
}

pub async fn run_buyer_dispute(cmd: BuyerDisputeCommand) -> Result<()> {
    let api = task_api_url();
    let http = reqwest::Client::new();

    match cmd {
        // ── evidence（提交证据：/evidence/upload → calldata → 签名 → 广播）
        BuyerDisputeCommand::Evidence {
            job_id, summary, ..
        } => {
            let (account_id, address, agent_id) =
                signing::resolve_wallet_and_agent_for_task(&http, &api, &job_id).await?;
            let endpoint =
                format!("{api}/priapi/v1/aieco/task/{job_id}/evidence/upload");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({ "text": summary });

            let result = signing::task_sign_and_broadcast_with_headers(
                &http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            )
            .await?;

            println!("✓ 证据已提交");
            println!("  jobId:  {job_id}");
            println!("  摘要:   {summary}");
            println!("  txHash: {}", result.tx_hash);
        }
        // ── info（GET 只读查询）
        BuyerDisputeCommand::Info { dispute_id } => {
            let resp: serde_json::Value = http
                .get(format!(
                    "{api}/priapi/v1/aieco/task/dispute/{dispute_id}"
                ))
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("无法查询争议详情: {e}"))?
                .json()
                .await?;

            if resp["code"] != 0 {
                bail!(
                    "查询争议失败: {}",
                    resp["msg"].as_str().unwrap_or("unknown")
                );
            }

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
