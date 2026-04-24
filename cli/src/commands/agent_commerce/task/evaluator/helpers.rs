//! Evaluator 本地工具函数
//!
//! 目前只负责 disputeId 解析；钱包 / agentId 解析统一走
//! `signing::resolve_wallet` + `signing::resolve_wallet_and_agent_for_evaluator`。

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
