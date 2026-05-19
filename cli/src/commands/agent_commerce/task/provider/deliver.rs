//! 卖家提交交付物
//!
//! 卖家动作：交付 — onchainos agent deliver

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::state_machine::Status;
use crate::commands::agent_commerce::task::signing;

/// deliver — 提交交付物
///
/// 1. 前置检查：任务必须在 accepted 状态（status=1）—— 即买家已 confirm-accept 上链
/// 2. POST submit API（带身份头）→ 获取 uopData
/// 3. 签名 uopData + 广播上链
pub async fn handle_deliver(
    client: &mut TaskApiClient,
    job_id: &str,
    _file: &str,
    _message: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id 必填，传卖家自己的 agentId（beta 后端拒空 agenticId header）");
    }

    // 前置：任务必须 accepted（买家已 confirm-accept、收到链上 job_accepted 通知）才能交付。
    // 防止 agent 在 apply 完没等买家确认就抢跑 deliver——后端会拒，但提前 bail 让错误更清楚。
    let task_resp = client.get_with_identity(&client.task_path(job_id), agent_id).await?;
    let status_int = task_resp["status"]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| anyhow::anyhow!("任务详情缺少 status 字段，无法判定能否交付"))?;
    let status = Status::from_int(status_int);
    if status != Status::Accepted {
        audit::log(
            "cli",
            "provider/deliver_blocked_wrong_status",
            false,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("statusInt={status_int}"),
                format!("status={}", status.as_str()),
            ]),
            Some("status != accepted(1)"),
        );
        bail!(
            "deliver 拒绝执行：任务当前 status = {} ({}), 必须为 accepted (1) 才能交付。\n\
             如果你刚 apply，需要等买家 confirm-accept 上链，收到 `job_accepted` 系统通知后再 deliver。\n\
             不要主动 xmtp_send 催买家——买家 confirm-accept 是用户决策，由对方 user session 推进。",
            status_int,
            status.as_str(),
        );
    }

    let (account_id, address) = signing::resolve_wallet(None, None)?;
    // 后端 spec：submit endpoint 接受 `evidenceHash` 字段，目前传空字符串占位（链下证据由
    // /evidence/upload 多 part 上传，不在 submit 阶段提供 hash）。file/message 仍只作
    // CLI 输入预留位，不上链。
    let body = serde_json::json!({
        "evidenceHash": "",
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "submit"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
    ).await?;

    audit::log(
        "cli",
        "provider/deliver_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ 交付物已提交，等待链上确认（job_submitted）");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  下一步由系统通知驱动，不要主动给买家发消息：");
    println!("    - 禁止立即调 `xmtp_send` 告诉买家 \"交付物已上链，请验收\" 等文字");
    println!("    - 链上确认后会收到 `job_submitted` 系统通知");
    println!("    - 收到通知后再调 `onchainos agent next-action --jobid {job_id} --jobStatus job_submitted --role provider`");
    Ok(())
}
