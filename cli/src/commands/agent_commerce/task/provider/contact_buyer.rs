//! 卖家主动联系买家（placeholder）
//!
//! TODO: 调用真实 xmtp_send（通过 openclaw runtime / ws-channel 注册的 XMTP mock）。
//! 当前为占位实现，仅打印意图，便于流程集成测试。

use anyhow::Result;

pub async fn handle_contact_buyer(
    to_agent_id: &str,
    job_id: &str,
    message: Option<&str>,
) -> Result<()> {
    let msg = message.unwrap_or(
        "你好，我看到了你发布的任务，想了解更多细节。如有兴趣协商，请回复。",
    );

    println!("📨 [placeholder] 向买家发起协商会话");
    println!("   目标 agentId: {to_agent_id}");
    println!("   jobId:        {job_id}");
    println!("   消息正文:     {msg}");
    println!();
    println!("ℹ️  当前为占位实现。真实环境将调用 xmtp_send:");
    println!("   toAgentId = {to_agent_id}");
    println!("   taskId    = {job_id}");
    println!("   type      = task_inquire");
    println!("   content   = <上面的消息>");
    Ok(())
}
