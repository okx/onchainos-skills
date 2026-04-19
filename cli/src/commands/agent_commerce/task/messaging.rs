//! Task system messaging abstraction.
//!
//! Provides a [`MessageSender`] trait with two implementations:
//! - [`MockSender`] (default) — prints structured JSON to stdout
//! - [`XmtpSender`] — calls `onchainos msg send` CLI (feature `xmtp`)
//!
//! All negotiate commands and status notifications go through this layer.

use anyhow::Result;
use serde_json::Value;

// ─── Trait ────────────────────────────────────────────────────────────────

/// Abstraction for sending messages to counterparties.
pub trait MessageSender: Send + Sync {
    /// Send a direct message to a specific address.
    fn send_dm(
        &self,
        to: &str,
        msg: &Value,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Send a message to a group conversation.
    fn send_group(
        &self,
        group_id: &str,
        msg: &Value,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

// ─── MockSender (default) ─────────────────────────────────────────────────

/// Prints structured JSON to stdout, simulating message delivery.
pub struct MockSender;

impl MessageSender for MockSender {
    async fn send_dm(&self, to: &str, msg: &Value) -> Result<()> {
        println!("[mock] XMTP DM \u{2192} {to}");
        println!("{}", serde_json::to_string_pretty(msg)?);
        Ok(())
    }

    async fn send_group(&self, group_id: &str, msg: &Value) -> Result<()> {
        println!("[mock] XMTP Group({group_id}) \u{2192}");
        println!("{}", serde_json::to_string_pretty(msg)?);
        Ok(())
    }
}

// ─── XmtpSender (feature = "xmtp") ───────────────────────────────────────

/// Sends messages via `onchainos msg send` subprocess.
#[cfg(feature = "xmtp")]
pub struct XmtpSender;

#[cfg(feature = "xmtp")]
impl MessageSender for XmtpSender {
    async fn send_dm(&self, to: &str, msg: &Value) -> Result<()> {
        let content = serde_json::to_string(msg)?;
        let output = tokio::process::Command::new("onchainos")
            .args(["msg", "send", "--to", to, "--content", &content])
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("无法执行 onchainos msg send: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("XMTP DM 发送失败: {stderr}");
        }
        Ok(())
    }

    async fn send_group(&self, group_id: &str, msg: &Value) -> Result<()> {
        let content = serde_json::to_string(msg)?;
        let output = tokio::process::Command::new("onchainos")
            .args(["msg", "send", "--group", group_id, "--content", &content])
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("无法执行 onchainos msg send: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("XMTP Group 发送失败: {stderr}");
        }
        Ok(())
    }
}

// ─── Factory ──────────────────────────────────────────────────────────────

/// Create the active message sender based on feature flags.
///
/// - `xmtp` feature enabled → [`XmtpSender`]
/// - default → [`MockSender`]
#[cfg(feature = "xmtp")]
pub fn create_sender() -> XmtpSender {
    XmtpSender
}

#[cfg(not(feature = "xmtp"))]
pub fn create_sender() -> MockSender {
    MockSender
}
