//! Review gate.
//!
//! Prevents the agent from skipping the user's review decision and calling `complete` directly
//! in escrow mode.
//!
//! Write points (automatic at the code level, not driven by prompts):
//! - `next-action --jobStatus job_submitted --role buyer` → writes `pending`
//! - `next-action --jobStatus approve_review --role buyer` → `pending` → `approved`
//!
//! Check points:
//! - `complete.rs` escrow path: `approved` lets the call through and clears the gate; everything else is rejected.

use anyhow::{bail, Result};
use std::path::PathBuf;

fn gate_path(job_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?;
    let dir = home.join(".onchainos").join("task").join(job_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("review-gate"))
}

pub fn mark_pending(job_id: &str) -> Result<()> {
    let path = gate_path(job_id)?;
    std::fs::write(&path, "pending")?;
    eprintln!("[review-gate] mark_pending: {}", path.display());
    Ok(())
}

pub fn mark_approved(job_id: &str) -> Result<()> {
    let path = gate_path(job_id)?;
    match std::fs::read_to_string(&path) {
        Ok(content) if content.trim() == "pending" => {
            std::fs::write(&path, "approved")?;
            eprintln!("[review-gate] mark_approved: {}", path.display());
            Ok(())
        }
        Ok(content) => {
            bail!(
                "review-gate 状态异常：期望 pending，实际 '{}'。\
                 请先走 next-action --jobStatus job_submitted 流程。",
                content.trim()
            );
        }
        Err(_) => {
            bail!(
                "review-gate 文件不存在（未经过 job_submitted 流程）。\
                 请先调 next-action --jobStatus job_submitted --role buyer。"
            );
        }
    }
}

pub fn check_and_consume(job_id: &str) -> Result<()> {
    let path = gate_path(job_id)?;
    match std::fs::read_to_string(&path) {
        Ok(content) if content.trim() == "approved" => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[review-gate] check_and_consume: approved, gate cleared");
            Ok(())
        }
        Ok(content) if content.trim() == "pending" => {
            bail!(
                "用户尚未做验收决策（review-gate = pending）。\
                 请先通过 `onchainos agent pending-decisions-v2 request` 入队验收决策并等待用户回复，\
                 收到 `[USER_DECISION_RELAY] decision: <verbatim>` 后再调 next-action --jobStatus approve_review 拿验收剧本。"
            );
        }
        Ok(content) => {
            bail!("review-gate 状态异常：'{}'", content.trim());
        }
        Err(_) => {
            bail!(
                "review-gate 文件不存在，escrow 模式下必须先走 \
                 next-action --jobStatus job_submitted 验收流程。\
                 禁止直接调用 complete。"
            );
        }
    }
}
