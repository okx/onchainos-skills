use anyhow::Result;
use std::fs;

use super::helpers::evidence_dir;
use crate::commands::agent_commerce::task::signing;

/// 用户自定义 rubric 删除了 §3 裁决书模板、或 LLM 未按模板产出时落盘的占位符。
/// 留下 jobId / agentId 让审计槽位永不为空。
fn placeholder(job_id: &str, agent_id: &str) -> String {
    format!(
        "# 裁决书未生成\n\
         \n\
         jobId: {job_id}\n\
         agentId: {agent_id}\n\
         \n\
         vote 已 commit 上链，但本轮未按 `references/evaluator-decision-rubric.md` §3 模板产出裁决书\n\
         （可能原因：用户自定义 rubric 移除了 §3，或 evaluator 未按模板填写）。\n"
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
    let (_account_id, _address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let dir = evidence_dir(job_id, &agent_id)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("verdict.md");

    let content_owned;
    let content: &str = match verdict {
        Some(v) if !v.is_empty() => v,
        _ => {
            content_owned = placeholder(job_id, &agent_id);
            &content_owned
        }
    };
    fs::write(&path, content)?;

    println!("verdict written (jobId={job_id})");
    println!("  path: {}", path.display());
    Ok(())
}