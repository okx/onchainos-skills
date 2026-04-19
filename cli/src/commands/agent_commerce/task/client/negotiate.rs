//! 协商消息命令（走 messaging 层，非链上操作）

use anyhow::Result;

use crate::commands::agent_commerce::task::messaging::{self, MessageSender};
use crate::commands::Context;

use super::NegotiateCommand;

pub async fn run_negotiate(cmd: NegotiateCommand, _ctx: &Context) -> Result<()> {
    let sender = messaging::create_sender();
    let now = chrono::Utc::now().to_rfc3339();

    match cmd {
        NegotiateCommand::Start { to, job_id, message } => {
            let msg = serde_json::json!({
                "type": "negotiate:start",
                "jobId": job_id,
                "to": to,
                "message": message,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Quote { to, job_id, price, currency, delivery_hours, skill_id, message } => {
            let msg = serde_json::json!({
                "type": "negotiate:quote",
                "jobId": job_id,
                "to": to,
                "price": price,
                "currency": currency.to_uppercase(),
                "deliveryHours": delivery_hours,
                "skillId": skill_id,
                "message": message,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Counter { to, job_id, price, reason } => {
            let msg = serde_json::json!({
                "type": "negotiate:counter",
                "jobId": job_id,
                "to": to,
                "price": price,
                "reason": reason,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Accept { to, job_id, price, delivery_hours, payment_mode } => {
            let msg = serde_json::json!({
                "type": "negotiate:accept",
                "jobId": job_id,
                "to": to,
                "price": price,
                "deliveryHours": delivery_hours,
                "paymentMode": payment_mode,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Reject { to, job_id, reason } => {
            let msg = serde_json::json!({
                "type": "negotiate:reject",
                "jobId": job_id,
                "to": to,
                "reason": reason,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
    }
    Ok(())
}
