//! 仲裁相关命令
//!
//! Client: info（已实现）, evidence（已实现）
//! Provider: raise（已实现）
//! Evaluator: vote, appeal（TODO）

use anyhow::{bail, Result};

use crate::commands::Context;
use crate::commands::agent_commerce::task::signing;

use super::{task_api_url, DisputeCommand};

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        // ── dispute raise（Provider 发起仲裁，单签）─────────────────────
        DisputeCommand::Raise { job_id, reason } => {
            let api = task_api_url();
            let http = reqwest::Client::new();
            let (account_id, address, agent_id) = signing::resolve_wallet_and_agent_for_task(&http, &api, &job_id).await?;
            let endpoint  = format!("{api}/priapi/v1/aieco/task/{job_id}/dispute");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({ "reason": reason });

            let result = signing::task_sign_and_broadcast_with_headers(
                &http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
            ).await?;

            println!("✓ 仲裁已发起，任务状态 → disputed");
            println!("  jobId:  {job_id}");
            println!("  原因:   {reason}");
            println!("  txHash: {}", result.tx_hash);
        }
        // ── dispute evidence（提交证据，简单 POST，无签名）──────────────
        DisputeCommand::Evidence { job_id, summary, .. } => {
            let api = task_api_url();
            let http = reqwest::Client::new();
            let body = serde_json::json!({ "text": summary });

            let resp: serde_json::Value = http
                .post(format!("{api}/priapi/v1/aieco/task/{job_id}/evidence"))
                .json(&body)
                .send().await
                .map_err(|e| anyhow::anyhow!("无法提交证据: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("提交证据失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
            }

            println!("✓ 证据已提交");
            println!("  jobId:  {job_id}");
            println!("  摘要:   {summary}");
        }
        // ── dispute info（GET 只读查询）────────────────────────────────
        DisputeCommand::Info { dispute_id } => {
            let api = task_api_url();
            let http = reqwest::Client::new();
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
        // TODO(evaluator): Commit-Reveal 投票第一步
        DisputeCommand::Vote { .. } => todo!("dispute vote"),
        // TODO(provider): Provider 上诉
        DisputeCommand::Appeal { .. } => todo!("dispute appeal"),
    }
    Ok(())
}
