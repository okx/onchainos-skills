//! Evaluator 证据查询
//!
//! disputeId 格式: `d-<jobId>-r<round>` — 解析 jobId，GET /evidence，下载图片到本地，
//! 供多模态 agent 直接读取。统一走 TaskApiClient（和 provider/dispute_info 一致的风格）。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// E1: fetch dispute evidence (text + images) for the evaluator.
pub async fn handle_info(client: &mut TaskApiClient, dispute_id: &str) -> Result<()> {
    let job_id = parse_job_id(dispute_id)?;

    // GET /priapi/v1/aieco/task/{jobId}/evidence — 返回 data 层已 unwrap 的 evidence 对象。
    let path = client.endpoint(&job_id, "evidence");
    let mut data = client.get(&path).await?;

    let tmp_dir = std::env::temp_dir()
        .join("onchainos-dispute")
        .join(dispute_id);
    fs::create_dir_all(&tmp_dir)?;

    if let Some(evs) = data["evidences"].as_array().cloned() {
        let mut enriched = Vec::with_capacity(evs.len());
        for ev in evs {
            let mut ev2 = ev.clone();
            if ev["kind"].as_str() == Some("image") {
                if let Some(name) = ev["name"].as_str() {
                    match download_image(client, &job_id, name, &tmp_dir).await {
                        Ok(p) => {
                            ev2["localPath"] = serde_json::Value::String(p.to_string_lossy().into());
                        }
                        Err(e) => {
                            ev2["downloadError"] = serde_json::Value::String(e.to_string());
                        }
                    }
                }
            }
            enriched.push(ev2);
        }
        data["evidences"] = serde_json::Value::Array(enriched);
    }

    println!("{}", serde_json::to_string_pretty(&data)?);
    Ok(())
}

/// 图片下载走裸 reqwest（二进制流，不能经 handle_response 的 JSON 解析）。
/// 路径对齐真后端：`GET /priapi/v1/aieco/task/{jobId}/evidence/download?name=<...>`。
async fn download_image(
    client: &TaskApiClient,
    job_id: &str,
    name: &str,
    tmp_dir: &Path,
) -> Result<PathBuf> {
    let url = format!(
        "{}{}/evidence/download",
        client.base_url().trim_end_matches('/'),
        client.endpoint(job_id, "").trim_end_matches('/'),
    );
    let resp = client
        .http()
        .get(&url)
        .query(&[("name", name)])
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("evidence download failed ({}): {}", resp.status(), url);
    }
    let bytes = resp.bytes().await?;
    let path = tmp_dir.join(name);
    fs::write(&path, &bytes)?;
    Ok(path)
}
