use anyhow::Result;
use std::fs;
use std::time::Duration;

use super::helpers::evidence_dir;
use crate::audit;

/// 用户自定义 rubric 删除了 §3 裁决书模板、或 LLM 未按模板产出时落盘的占位符。
/// 留下 jobId / agentId 让审计槽位永不为空。
fn placeholder(job_id: &str, agent_id: &str) -> String {
    format!(
        "# Verdict not generated\n\
         \n\
         jobId: {job_id}\n\
         agentId: {agent_id}\n\
         \n\
         vote was committed on-chain, but this round did not produce a verdict per the `references/evaluator-decision-rubric.md` §3 template\n\
         (possible causes: user-customized rubric removed §3, or the evaluator did not follow the template).\n"
    )
}

/// 把 evaluator 产出的裁决书 markdown 落盘到 `<evidence_dir>/verdict.md`。
///
/// commit 后调用，作为本地审计冗余（vote 已上链，落盘仅供事后人工/复议核对）。
/// `verdict` 为 None → 写入占位符；失败由 flow.rs 决定如何处理（默认不重试、不阻塞）。
pub async fn handle_record(
    job_id: &str,
    agent_id: &str,
    verdict: Option<&str>,
) -> Result<()> {
    let dir = evidence_dir(job_id, agent_id)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("verdict.md");

    let is_placeholder;
    let content_owned;
    let content: &str = match verdict {
        Some(v) if !v.is_empty() => {
            is_placeholder = false;
            v
        }
        _ => {
            is_placeholder = true;
            content_owned = placeholder(job_id, agent_id);
            &content_owned
        }
    };
    fs::write(&path, content)?;

    let event = if is_placeholder {
        "evaluator/verdict_placeholder_written"
    } else {
        "evaluator/verdict_written"
    };
    audit::log(
        "cli",
        event,
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("path={}", path.display()),
        ]),
        None,
    );

    println!("verdict written (jobId={job_id})");
    println!("  path: {}", path.display());
    Ok(())
}