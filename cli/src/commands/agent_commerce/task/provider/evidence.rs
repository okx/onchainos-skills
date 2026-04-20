//! 仲裁相关命令
//!
//! - dispute raise: 发起仲裁（卖家）
//! - dispute evidence: 提交证据（双方）
//! - dispute info: 查询争议详情

use anyhow::{bail, Result};

use super::DisputeCommand;
use crate::commands::agent_commerce::task::signing;

pub async fn run_evidence(cmd: DisputeCommand) -> Result<()> {
    let api = super::task_api_url();
    let http = reqwest::Client::new();

    match cmd {
        // ── dispute raise（发起仲裁：dispute API → calldata → 签名 → 广播）──
        DisputeCommand::Raise { job_id, reason } => {
            let (account_id, address, agent_id) =
                signing::resolve_wallet_and_agent_for_provider(&http, &api, &job_id).await?;
            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/dispute");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({ "reason": reason });

            let result = signing::task_sign_and_broadcast_with_headers(
                &http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            ).await?;

            println!("✓ 已发起仲裁，等待链上确认（TASK_DISPUTED）");
            println!("  原因: {reason}");
            println!("  txHash: {}", result.tx_hash);
        }
        // ── dispute evidence（提交证据：evidence API → calldata → 签名 → 广播）
        DisputeCommand::Evidence { job_id, summary, .. } => {
            let (account_id, address, agent_id) =
                signing::resolve_wallet_and_agent_for_provider(&http, &api, &job_id).await?;
            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/evidence");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({ "text": summary });

            let result = signing::task_sign_and_broadcast_with_headers(
                &http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            ).await?;

            println!("✓ 证据已提交");
            println!("  jobId:  {job_id}");
            println!("  摘要:   {summary}");
            println!("  txHash: {}", result.tx_hash);
        }
        // ── dispute info（GET 只读查询）────────────────────────────────
        DisputeCommand::Info { dispute_id } => {
            let resp: serde_json::Value = http
                .get(format!("{api}/priapi/v1/aieco/task/dispute/{dispute_id}"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法查询争议详情: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("查询争议失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
            }

            let d = &resp["data"];
            println!("争议详情：");
            println!("  disputeId: {dispute_id}");
            println!("  jobId:     {}", d["jobId"].as_str().unwrap_or("?"));
            println!("  状态:      {}", d["statusStr"].as_str().unwrap_or("?"));
            println!("  发起方:    {}", d["raiserAddress"].as_str().unwrap_or("?"));
            println!("  发起原因:  {}", d["reason"].as_str().unwrap_or("?"));
            println!("  创建时间:  {}", d["createTime"].as_str().unwrap_or("?"));

            let evidences = d["evidences"].as_array();
            if let Some(evs) = evidences {
                println!("\n证据列表（共 {} 条）：", evs.len());
                for (i, ev) in evs.iter().enumerate() {
                    println!("  {}. 提交方: {}  类型: {}",
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
