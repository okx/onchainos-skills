//! 协商
//!
//! 买家动作：协商（包括还价、接受报价、协商支付方式、拒绝报价）
//! — onchainos negotiate start
//!
//! 协商在子 session 中由 Agent 自然语言完成，
//! 通信模块自动转发，不需要独立 CLI 命令。
//! 本模块为占位，预留未来扩展。

use anyhow::Result;

/// 协商入口（当前由 Agent 子 session 自然语言完成）
pub async fn handle_negotiate_start(
    _http: &reqwest::Client,
    _api: &str,
    _job_id: &str,
    _provider: &str,
) -> Result<()> {
    println!("[stub] negotiate 由 Agent 子 session 自然语言完成，无需手动调用");
    Ok(())
}
