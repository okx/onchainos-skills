//! 评价 Agent（任务完成后对对方 Agent 打分）
//!
//! 买卖双方通用。内部调用 `onchainos agent feedback-submit` 子进程，
//! 参数透传到 identity 模块的 FeedbackSubmitArgs。

use anyhow::{bail, Result};
use tokio::process::Command;

/// 提交评价 / 评分
///
/// - `agent_id`：被评价的 Agent ID（必填）
/// - `creator_id`：评价发起方 Agent ID（必填）
/// - `score`：0-100 分（必填）
/// - `description`：文字评价（可选）
/// - `task_id`：关联任务 ID（可选）
pub async fn handle_rate_agent(
    agent_id: &str,
    creator_id: &str,
    score: &str,
    description: Option<&str>,
    task_id: Option<&str>,
) -> Result<()> {
    // 前置校验
    if agent_id.is_empty() {
        bail!("--agent-id 必填（被评价的 Agent ID）");
    }
    if creator_id.is_empty() {
        bail!("--creator-id 必填（评价发起方的 Agent ID）");
    }
    let score_val: i32 = score
        .parse()
        .map_err(|_| anyhow::anyhow!("--score 必须是 0-100 之间的整数（当前: {score}）"))?;
    if !(0..=100).contains(&score_val) {
        bail!("--score 必须在 0-100 之间（当前: {score_val}）");
    }

    // 组装子进程参数
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("无法获取当前可执行文件路径: {e}"))?;
    let mut args: Vec<String> = vec![
        "agent".into(),
        "feedback-submit".into(),
        "--agent-id".into(), agent_id.into(),
        "--creator-id".into(), creator_id.into(),
        "--score".into(), score.into(),
    ];
    if let Some(d) = description {
        if !d.is_empty() {
            args.push("--description".into());
            args.push(d.into());
        }
    }
    if let Some(t) = task_id {
        if !t.is_empty() {
            args.push("--task-id".into());
            args.push(t.into());
        }
    }

    println!("📝 提交评价：");
    println!("  被评价:     {agent_id}");
    println!("  发起方:     {creator_id}");
    println!("  分数:       {score}");
    if let Some(d) = description.filter(|s| !s.is_empty()) {
        println!("  描述:       {d}");
    }
    if let Some(t) = task_id.filter(|s| !s.is_empty()) {
        println!("  taskId:     {t}");
    }
    println!();

    // 调子进程（继承 stdin/stdout/stderr 让用户看到原始输出）
    let status = Command::new(&exe).args(&args).status().await
        .map_err(|e| anyhow::anyhow!("调用 `onchainos agent feedback-submit` 失败: {e}"))?;

    if !status.success() {
        bail!("`onchainos agent feedback-submit` 退出码: {status}");
    }
    Ok(())
}
