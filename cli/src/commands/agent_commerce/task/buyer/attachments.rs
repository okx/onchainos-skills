//! Local attachment management for buyer tasks.
//!
//! Storage: `~/.onchainos/task/<jobId>/attachments/`

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

fn attachments_dir(job_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("task").join(job_id).join("attachments"))
}

pub fn handle_task_attach(job_id: &str, file_path: &str) -> Result<()> {
    let src = Path::new(file_path);
    if !src.exists() {
        bail!("file not found: {file_path}");
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

pub fn handle_task_attachments(job_id: &str) -> Result<()> {
    let dir = attachments_dir(job_id)?;
    if !dir.exists() {
        println!("[]");
        return Ok(());
    }

    let mut files: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            files.push(entry.path().display().to_string());
        }
    }

    files.sort();
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
        eprintln!("[task-create] attachment saved: {}", dest.display());
    }
    Ok(())
}
