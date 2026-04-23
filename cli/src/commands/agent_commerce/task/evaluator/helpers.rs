use anyhow::{bail, Result};

pub(super) fn task_api_url() -> String {
    std::env::var("TASK_API_URL").unwrap_or_else(|_| "http://127.0.0.1:9001".to_string())
}

pub(super) fn evaluator_addr() -> String {
    std::env::var("EVALUATOR_COMM_ADDR")
        .unwrap_or_else(|_| "0xEvaluator00000000000000000000000000001".to_string())
}

pub(super) fn evaluator_agent_id() -> String {
    std::env::var("EVALUATOR_AGENT_ID")
        .unwrap_or_else(|_| "mock-evaluator-agent-001".to_string())
}

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
