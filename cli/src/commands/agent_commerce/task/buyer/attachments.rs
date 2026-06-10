//! Local attachment management for buyer tasks.
//!
//! Storage: `~/.onchainos/task/<jobId>/attachments/`

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::query::{self as common_query, status_name};
use crate::commands::agent_commerce::task::common::{AGENT_ROLE_BUYER, DEBUG_LOG};

fn attachments_dir(job_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("task").join(job_id).join("attachments"))
}

pub async fn handle_task_attach(client: &mut TaskApiClient, job_id: &str, file_path: &str) -> Result<()> {
    let agent_id = common_query::resolve_agent_id("", AGENT_ROLE_BUYER).await;
    let resp = client.get_with_agent_id(&client.task_path(job_id), &agent_id).await?;
    let status = resp["status"].as_i64().unwrap_or(-1);
    if status >= 2 {
        let status_str = status_name(status);
        bail!(
            "task status is \"{status_str}\" (status={status}); \
             attachments can only be added when the task is in created or accepted state"
        );
    }

    let src = Path::new(file_path);
    if !src.exists() {
        bail!("file not found: {file_path}");
    }

    const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100 MB
    let file_size = std::fs::metadata(src)?.len();
    if file_size > MAX_FILE_SIZE {
        let size_mb = file_size as f64 / (1024.0 * 1024.0);
        bail!(
            "file too large: {size_mb:.1} MB (max 100 MB). \
             Please compress or resize the file before adding it as an attachment."
        );
    }

    let file_name = src.file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid file path: {file_path}"))?;

    let dir = attachments_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;

    let dest = dir.join(file_name);
    std::fs::copy(src, &dest)?;

    println!("✓ Attachment saved");
    println!("  jobId: {job_id}");
    println!("  file:  {}", dest.display());
    println!();
    println!("🛑 NEXT STEP (MUST NOT SKIP): the file is saved LOCALLY only — it has NOT been sent to the provider yet.");
    println!("   If a sub session exists for this job (task already has a matched provider),");
    println!("   you MUST call xmtp_dispatch_session to notify the sub session:");
    println!();
    println!("   1. xmtp_sessions_query (myAgentId, jobId={job_id}) → find the sub session key", );
    println!("   2. xmtp_dispatch_session(sessionKey=<sub_key>, content=\"[ATTACHMENT_ADDED] {}\")  ← exact prefix, do NOT change", dest.display());
    println!();
    println!("   If NO sub session exists yet (task not matched with a provider), skip the dispatch —");
    println!("   the sub session will pick up the file automatically via list-attachments when it starts.");
    Ok(())
}

pub fn list_attachment_paths(job_id: &str) -> Vec<String> {
    let dir = match attachments_dir(job_id) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                files.push(entry.path().display().to_string());
            }
        }
    }
    files.sort();
    files
}

pub fn handle_task_attachments(job_id: &str) -> Result<()> {
    let files = list_attachment_paths(job_id);
    let json = serde_json::to_string_pretty(&files)?;
    println!("{json}");
    Ok(())
}

pub fn copy_attachments_to_job(job_id: &str, sources: &[String]) -> Result<()> {
    let dir = attachments_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;

    for src_path in sources {
        let src = Path::new(src_path);
        if !src.exists() {
            bail!("attachment file not found: {src_path}");
        }
        let file_name = src.file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid file path: {src_path}"))?;
        let dest = dir.join(file_name);
        std::fs::copy(src, &dest)?;
        if DEBUG_LOG {
            eprintln!("[task-create] attachment saved: {}", dest.display());
        }
    }
    Ok(())
}
