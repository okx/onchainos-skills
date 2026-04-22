//! 查询争议详情（只读）— onchainos agent dispute info <disputeId>

use anyhow::Result;
use serde_json::Value;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

pub async fn handle_dispute_info(client: &TaskApiClient, dispute_id: &str) -> Result<()> {
    let url = format!("{}/priapi/v1/aieco/task/dispute/{}", client.base_url(), dispute_id);
    let resp = client.get(&url).await?;
    print_dispute_info(dispute_id, &resp["data"]);
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
