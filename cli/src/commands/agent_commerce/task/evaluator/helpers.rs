use std::path::PathBuf;

use anyhow::{bail, Result};

/// Parse jobId from disputeId like "d-<jobId>-r<round>".
pub(super) fn parse_job_id(dispute_id: &str) -> Result<String> {
    let s = dispute_id
        .strip_prefix("d-")
        .ok_or_else(|| anyhow::anyhow!("disputeId must start with 'd-'"))?;
    let idx = s
        .rfind("-r")
        .ok_or_else(|| anyhow::anyhow!("disputeId must contain '-r<round>'"))?;
    let (job, rest) = s.split_at(idx);
    let round = &rest[2..];
    if round.is_empty() || !round.chars().all(|c| c.is_ascii_digit()) {
        bail!("disputeId round part must be digits");
    }
    if job.is_empty() {
        bail!("disputeId jobId part empty");
    }
    Ok(job.to_string())
}

/// 证据图片落盘目录：
/// - `~/.onchainos/task/<job_id>/dispute/<dispute_id>/`（`evidence-info` 走这条，按
///   round 隔离）
/// 命名对齐 buyer 的 `~/.onchainos/task/<jobId>/`，方便集中清理 + 跨重启保留。
pub(super) fn evidence_dir(job_id: &str, dispute_id: Option<&str>) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?;
    let mut p = home.join(".onchainos").join("task").join(job_id).join("dispute");
    if let Some(d) = dispute_id {
        p = p.join(d);
    }
    Ok(p)
}
