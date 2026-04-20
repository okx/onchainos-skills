//! 获取推荐卖家
//!
//! 买家动作：获取推荐卖家 — onchainos task recommend

use anyhow::{bail, Result};

/// 查询推荐卖家
pub async fn handle_recommend(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let resp: serde_json::Value = http
        .post(format!("{api}/priapi/v1/aieco/task/{job_id}/match"))
        .send().await
        .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
        .json().await?;

    if resp["code"] != 0 {
        bail!("{}", resp["msg"].as_str().unwrap_or("error"));
    }
    let recs = resp["data"]["recommendations"].as_array()
        .cloned().unwrap_or_default();
    println!("推荐卖家列表（共 {} 个）：", recs.len());
    for (i, r) in recs.iter().enumerate() {
        println!("  {}. AgentID: {}  匹配分: {}  信用分: {}",
            i + 1,
            r["providerAgentId"].as_str().unwrap_or("?"),
            r["matchScore"].as_f64().unwrap_or(0.0),
            r["creditScore"].as_i64().unwrap_or(0),
        );
        println!("     能力: {}", r["capabilitySummary"].as_str().unwrap_or(""));
        println!("     地址: {}", r["providerAddress"].as_str().unwrap_or("?"));
    }
    Ok(())
}
