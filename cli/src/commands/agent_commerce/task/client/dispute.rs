//! 仲裁相关命令
//!
//! Client: info（已实现）, evidence（TODO）
//! Provider/Evaluator: raise, vote, appeal（TODO）

use anyhow::{bail, Result};

use crate::commands::Context;

use super::{task_api_url, DisputeCommand};

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        // TODO(provider): Provider 发起仲裁，捆绑签名 approve(DisputeManager, 5%) + createDispute(jobId)
        DisputeCommand::Raise { .. } => todo!("dispute raise"),
        // TODO(client): Phase 4 实现 — multipart 文件上传（jpg/jpeg/png/gif/webp），无链上签名
        DisputeCommand::Evidence { .. } => todo!("dispute evidence"),
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
