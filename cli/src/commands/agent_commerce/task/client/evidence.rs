//! 提交证据
//!
//! 买家动作：提交证据 — onchainos dispute evidence
//! 附带：查询争议详情 (dispute info)

use anyhow::{bail, Result};

use super::DisputeCommand;

/// 仲裁证据提交 + 争议查询（买家侧）
pub async fn run_evidence(cmd: DisputeCommand) -> Result<()> {
    let api = super::task_api_url();
    let http = reqwest::Client::new();

    match cmd {
        // ── dispute evidence（提交证据，简单 POST，无签名）──────────────
        DisputeCommand::Evidence { job_id, summary, .. } => {
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
