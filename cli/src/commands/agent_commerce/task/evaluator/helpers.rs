use std::path::PathBuf;

use anyhow::Result;

/// Evidence-image on-disk directory: `~/.onchainos/task/<job_id>/dispute/<agent_id>/`.
///
/// In V1 a single jobId has at most one active dispute at a time; evidence is
/// isolated by jobId + agent_id — when multiple evaluator agents run on the
/// same machine under the same OS user, each gets its own directory, so
/// concurrent writes do not race. On round re-draws the local cache is
/// overwritten on demand by the next `evidence-info` run.
pub(super) fn evidence_dir(job_id: &str, agent_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve HOME directory"))?;
    Ok(home
        .join(".onchainos")
        .join("task")
        .join(job_id)
        .join("dispute")
        .join(agent_id))
}