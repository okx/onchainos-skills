use std::path::PathBuf;

use anyhow::Result;

/// 证据图片落盘目录：`~/.onchainos/task/<job_id>/dispute/<agent_id>/`。
///
/// V1 一个 jobId 同时只有一条 active dispute，证据按 jobId + agent_id 隔离——
/// 同机器同 OS 用户跑多个 evaluator agent 时各自独立目录，避免并发写入竞争；
/// 多轮重抽时本地缓存按需被新一轮 evidence-info 覆盖。
pub(super) fn evidence_dir(job_id: &str, agent_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?;
    Ok(home
        .join(".onchainos")
        .join("task")
        .join(job_id)
        .join("dispute")
        .join(agent_id))
}