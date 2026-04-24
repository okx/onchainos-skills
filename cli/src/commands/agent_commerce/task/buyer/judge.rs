//! 评价卖家
//!
//! 买家动作：评价卖家 — 身份系统 CLI
//! 任务结束后，买家对卖家进行评分。
//!
//! 当前为占位模块，评价功能依赖身份系统 CLI 实现。

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// 评价卖家（TODO: 对接身份系统 CLI）
pub async fn handle_judge(
    _client: &mut TaskApiClient,
    _job_id: &str,
) -> Result<()> {
    // TODO(identity): 对接身份系统 CLI 的评价接口
    println!("[TODO] 评价卖家功能待对接身份系统 CLI");
    Ok(())
}
