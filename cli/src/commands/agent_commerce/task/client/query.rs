//! 只读查询命令（无链上签名）
//!
//! status, list, recommend, pay

use anyhow::{bail, Result};

use crate::commands::agent_commerce::task::common::XLAYER_CHAIN_ID;

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

/// 查询任务状态
pub async fn handle_status(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let resp: serde_json::Value = http
        .get(format!("{api}/priapi/v1/aieco/task/{job_id}"))
        .send().await
        .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
        .json().await?;

    if resp["code"] != 0 {
        bail!("任务不存在: {job_id}");
    }
    let t = &resp["data"]["task"];
    println!("任务状态: {}", t["statusStr"].as_str().unwrap_or("?"));
    println!("  jobId:    {job_id}");
    println!("  标题:     {}", t["title"].as_str().unwrap_or("?"));
    println!("  预算:     {} USDT", t["tokenAmount"].as_str().unwrap_or("?"));
    println!("  买家:     {}", t["buyerAgentId"].as_str().unwrap_or("?"));
    if let Some(pid) = t["providerAgentId"].as_str() {
        println!("  卖家:     {pid}");
    }
    println!("  更新时间: {}", t["updateTime"].as_str().unwrap_or("?"));
    Ok(())
}

/// 任务列表
pub async fn handle_list(
    http: &reqwest::Client,
    api: &str,
    role: Option<&str>,
    status: Option<&str>,
    page: u32,
    limit: u32,
) -> Result<()> {
    let url = if role == Some("provider") || role == Some("client") {
        let r = role.unwrap_or("client");
        format!("{api}/priapi/v1/aieco/task/my?role={r}&page={page}&page_size={limit}")
    } else {
        let mut u = format!("{api}/priapi/v1/aieco/task/list?page={page}&page_size={limit}");
        if let Some(s) = status { u.push_str(&format!("&status={s}")); }
        u
    };
    let resp: serde_json::Value = http.get(&url).send().await
        .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
        .json().await?;
    let tasks = resp["data"]["list"].as_array().cloned().unwrap_or_default();
    let total = resp["data"]["total"].as_u64().unwrap_or(0);
    println!("任务列表（共 {total} 个，第 {page} 页）：");
    for t in &tasks {
        println!("  [{}] {} — {} USDT",
            t["statusStr"].as_str().unwrap_or("?"),
            t["jobId"].as_str().unwrap_or("?"),
            t["tokenAmount"].as_str().unwrap_or("?"),
        );
        println!("       {}", t["title"].as_str().unwrap_or("?"));
    }
    Ok(())
}

/// 非担保模式手动转账（展示转账命令）
pub async fn handle_pay(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let resp: serde_json::Value = http
        .get(format!("{api}/priapi/v1/aieco/task/{job_id}"))
        .send().await
        .map_err(|e| anyhow::anyhow!("无法查询任务详情: {e}"))?
        .json().await?;

    if resp["code"] != 0 {
        bail!("查询任务失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
    }

    let task = &resp["data"]["task"];
    let status = task["statusStr"].as_str().unwrap_or("");
    if status != "complete" {
        bail!("任务状态为 {status}，仅 complete 状态可执行 pay");
    }

    let provider_addr = task["providerAgentAddress"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 providerAgentAddress"))?;
    let amount = task["tokenAmount"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 tokenAmount"))?;
    let token_symbol = task["paymentTokenSymbol"]
        .as_str()
        .unwrap_or("USDT");
    let token_address = task["tokenAddress"]
        .as_str()
        .unwrap_or("");

    println!("非担保任务付款信息：");
    println!("  Provider: {provider_addr}");
    println!("  金额:     {amount} {token_symbol}");
    println!("  链:       xlayer (chainId={})", XLAYER_CHAIN_ID);
    println!();
    println!("请执行以下命令完成转账：");
    if token_address.is_empty() {
        println!("  onchainos wallet send --readable-amount {amount} --recipient {provider_addr} --chain xlayer");
    } else {
        println!("  onchainos wallet send --readable-amount {amount} --recipient {provider_addr} --chain xlayer --contract-token {token_address}");
    }
    Ok(())
}
