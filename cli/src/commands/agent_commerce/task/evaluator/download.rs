//! Evaluator 单文件证据下载 — `onchainos agent evaluator download <jobId> <fileKey>`
//!
//! 与 `evaluator info` 不同：info 拉一次 evidence 列表后批量下载所有图片；
//! download 直接按 (jobId, fileKey) 拉一份字节落盘，用于失败重试或外部脚本场景。
//!
//! 后端 endpoint: `GET /priapi/v1/aieco/task/{jobId}/evidence/download?fileKey=<...>`，
//! 强制 JWT + agenticId 鉴权（同 evaluator info 走的是同一鉴权链路）。

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::info::fetch_evidence_bytes;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_download(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    output: Option<&str>,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id_hint).await?;
    let bytes = fetch_evidence_bytes(client, job_id, file_key, &agent_id).await?;

    let path = match output {
        Some(p) => PathBuf::from(p),
        None => {
            let dir = std::env::temp_dir()
                .join("onchainos-dispute")
                .join(job_id);
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create dir {}", dir.display()))?;
            let filename = file_key.rsplit('/').next().unwrap_or(file_key);
            dir.join(filename)
        }
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }
    }
    fs::write(&path, &bytes)
        .with_context(|| format!("failed to write {}", path.display()))?;

    println!(
        "{}",
        serde_json::json!({
            "fileKey": file_key,
            "jobId": job_id,
            "localPath": path.to_string_lossy(),
            "fileSize": bytes.len(),
        })
    );
    Ok(())
}
