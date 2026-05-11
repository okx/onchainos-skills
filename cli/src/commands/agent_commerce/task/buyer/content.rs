//! Buyer 端消息模板 — 单一维护点。
//!
//! 收两类模板:
//!
//! 1. **User-facing** (`xmtp_dispatch_user(content)` / `xmtp_prompt_user(userContent)`)
//!    给用户看的聊天内容。命名后缀 `_user_notify` / `_user_prompt`。
//!    规则:**禁用** tool 名(`xmtp_*`) / 事件名(`provider_applied`/`job_*` 等) /
//!    状态名(`open`/`accepted` 等英文枚举) / CLI flag(`--*`) /
//!    skill 名(`okx-agent-identity` 等) / 状态字段名(`jobStatus`/`paymentMode`)。
//!    用自然中文(担保/非担保/x402,验收期超时,任务已完成,等)。
//!
//! 2. **Peer-facing** (`xmtp_send` content,给卖家 sub agent)
//!    agent-to-agent 协议消息。命名后缀 `_to_seller`。
//!    规则:可以含协议字面量(`[NEGOTIATE_*]` / `paymentId` 等);
//!    **禁止指挥对方调 CLI**(对方有自己的 flow.rs,会按链事件自决,你下指令是越权)。
//!
//! 字段值占位符用 `<...>` 包,agent 拿 `common context` / 上下文填充。
//! 添加新文案 → 加新 fn;改文案 → 改 fn 体;flow.rs 永远只调这里、不内嵌字面量。

/// preamble 异常升级硬规则 1) 协议理解错位 — content 模板。
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ 协议理解错位] 任务 {job_id} 多次澄清同一流程对方仍重复，已停回复，请介入或给新指令。")
}

/// preamble 异常升级硬规则 2) 执行报错 — content 模板。
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!("[⚠️ 执行报错] 任务 {job_id} <动作简述,如「确认接单」/「验收交付物」/「提交证据」>失败,请查看后给新指令。")
}
