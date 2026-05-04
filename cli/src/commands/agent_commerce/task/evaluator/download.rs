
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::helpers::evidence_dir;
use super::info::fetch_evidence_bytes;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

//todo zhangxin 测试完成后删除
pub async fn handle_download(
    client: &TaskApiClient,
    job_id: &str,
    file_key: &str,
    output: Option<&str>,
    agent_id: &str,
) -> Result<()> {
    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;
    let bytes = fetch_evidence_bytes(client, job_id, file_key, &agent_id).await?;

    let path = match output {
        Some(p) => PathBuf::from(p),
        None => {
            let dir = evidence_dir(job_id, None)?;
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
