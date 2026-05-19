//! Buyer 端消息模板 — 单一维护点。
//!
//! 收两类模板:
//!
//! 1. **User-facing** (`xmtp_dispatch_user(content)` / `xmtp_prompt_user(userContent)`)
//!    给用户看的聊天内容。命名后缀 `_user_notify` / `_user_prompt`。
//!    规则:**禁用技术术语** —— tool 名(`xmtp_*`) / 事件名(`provider_applied`/`job_*` 等) /
//!    状态名(`open`/`accepted` 等英文枚举) / CLI flag(`--*`) /
//!    skill 名(`okx-agent-identity` 等) / 状态字段名(`jobStatus`/`paymentMode`)。
//!    **本文件模板的字面量是中文**(担保/x402,验收期超时,任务已完成,等),作为 sub agent
//!    LOCALIZATION_PREFIX 翻译时的 source-of-truth —— 中文用户原样呈现,非中文用户由
//!    sub agent 翻成等价口语化表达(英文用户:「escrowed payment/x402, review window expired,
//!    task completed」等)。禁用技术术语这条对所有语言生效,不止中文。
//!
//! 2. **Peer-facing** (`xmtp_send` content,给服务商 sub agent)
//!    agent-to-agent 协议消息。命名后缀 `_to_seller`。
//!    规则:可以含协议字面量(`[NEGOTIATE_*]` 等);
//!    **禁止指挥对方调 CLI**(对方有自己的 flow.rs,会按链事件自决,你下指令是越权)。
//!
//! 字段值占位符用 `<...>` 包,agent 拿 `common context` / 上下文填充。
//! 添加新文案 → 加新 fn;改文案 → 改 fn 体;flow.rs 永远只调这里、不内嵌字面量。

// ── Event::JobCreated ──────────────────────────────────────────────

/// `Event::JobCreated` Step 0 推给用户的任务上链成功通知。
pub fn job_created_user_notify(job_id: &str, notify_text: &str) -> String {
    format!("任务 {job_id} 已上链成功（待接单），{notify_text}")
}

// ── Event::JobAccepted ─────────────────────────────────────────────

/// `Event::JobAccepted` 分支 A（escrow）推给用户的接单成功通知。
pub fn job_accepted_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20[接单成功] 任务 {job_id} 已确认接单，进入执行阶段。\n\
         \x20\x20任务标题：{title}\n\
         \x20\x20任务描述：<description>\n\
         \x20\x20服务商 AgentID：<providerAgentId>\n\
         \x20\x20支付方式：担保\n\
         \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
         \x20\x20等待服务商执行并提交交付物。"
    )
}

/// `Event::JobAccepted` 分支 B（x402）重放失败时推给用户的通知。
pub fn job_accepted_x402_replay_fail_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 重放失败] 任务 {job_id} 已接单但 endpoint 重放失败。\n\
         \x20\x20HTTP 状态：<replayStatus>\n\
         \x20\x20错误信息：<replayBody>\n\
         \x20\x20任务已进入 accepted 状态，等待进一步处理。"
    )
}

// ── Event::JobRefused ──────────────────────────────────────────────

/// `Event::JobRefused` Step 1 推给用户的拒绝上链成功通知。
pub fn job_refused_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[拒绝已确认] 任务 **{title}**（{job_id}）的交付物已拒绝，等待服务商处理。\n\
         \x20\x20\x20\x20服务商将在 24 小时内选择：发起仲裁 或 同意退款。\n\
         \x20\x20\x20\x20超时未操作将自动退款至您的钱包。"
    )
}

// ── Event::JobDisputed ─────────────────────────────────────────────

/// `Event::JobDisputed` Step 1 给用户看的证据收集 prompt(`xmtp_prompt_user.userContent`)。
pub fn job_disputed_user_evidence_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务 {short_id} 你作为用户] 仲裁已上链，需要在 1 小时内提交链下证据。请提供：\n\
         \x20\x20\x20\x20- 文字摘要（必填）：说明交付物不达标的关键证据点\n\
         \x20\x20\x20\x20- 图片路径（可选）：截图、聊天记录等本地文件路径\n\
         \x20\x20\x20\x20回复格式示例：『证据：交付物缺少 X/Y/Z；图片：/path/to/screenshot.png』"
    )
}

// ── Event::JobCompleted ────────────────────────────────────────────

/// `Event::JobCompleted` 分支 A（escrow）推给用户的任务完成通知。
pub fn job_completed_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务完成] **{title}**（{job_id}）已验收通过，资金已释放给服务商。\n\
         \x20\x20\x20\x20  - 支出：**<tokenAmount> <tokenSymbol>**\n\
         \x20\x20\x20\x20  - 支付方式：**担保**\n\
         \x20\x20\x20\x20  - 链上凭证：<txHash>（来自 complete CLI 输出）\n\
         \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20本任务流程结束。"
    )
}

/// `Event::JobCompleted` 分支 B（x402）推给用户的最终汇总通知。
pub fn job_completed_x402_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[x402 任务完成] **{title}**（{job_id}）全部流程已完成。\n\
         \x20\x20\x20\x20  - 支出：**<tokenAmount> <tokenSymbol>**\n\
         \x20\x20\x20\x20  - 支付方式：**x402**\n\
         \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
         \x20\x20\x20\x20如需评价服务商，请回复「评价」。"
    )
}

// ── Event::DisputeResolved ─────────────────────────────────────────

/// `Event::DisputeResolved` 用户胜诉推给用户的通知。
pub fn dispute_won_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[仲裁胜诉] **{title}**（{job_id}）仲裁完成，**用户方胜诉**。\n\
         \x20\x20\x20\x20  - 退款：**<tokenAmount> <tokenSymbol>**\n\
         \x20\x20\x20\x20本任务流程结束。如需评价服务商，请回复「评价」。"
    )
}

/// `Event::DisputeResolved` 用户败诉推给用户的通知。
pub fn dispute_lost_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[仲裁败诉] **{title}**（{job_id}）仲裁完成，**服务商方胜诉**。\n\
         \x20\x20\x20\x20  - 损失：**<tokenAmount> <tokenSymbol>**（资金已释放给服务商）\n\
         \x20\x20\x20\x20本任务流程结束。如需评价服务商，请回复「评价」。"
    )
}

// ── Event::JobRefunded ─────────────────────────────────────────────

/// `Event::JobRefunded` 推给用户的退款完成通知。
pub fn job_refunded_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[退款完成] 任务 {job_id} 退款已上链，**资金已返还**至您的钱包。\n\
         \x20\x20\x20\x20本任务流程结束。"
    )
}

// ── Event::JobAutoRefunded ─────────────────────────────────────────

/// `Event::JobAutoRefunded` 推给用户的自动退款成功通知。
pub fn job_auto_refunded_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[自动退款成功] **{title}**（{job_id}）的担保资金已退还至您的钱包。\n\
         \x20\x20\x20\x20本任务流程结束。"
    )
}

// ── Event::JobExpired ──────────────────────────────────────────────

/// `Event::JobExpired` 推给用户的任务超时通知。
pub fn job_expired_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} **已超时**（accept 截止前未接单 或 submit 截止前未提交），任务已结束。")
}

// ── Event::JobClosed ───────────────────────────────────────────────

/// `Event::JobClosed` 推给用户的任务关闭通知。
pub fn job_closed_user_notify(job_id: &str, title: &str) -> String {
    format!("**{title}**（{job_id}）**已关闭**，资金已回收。")
}

// ── Event::JobVisibilityChanged ────────────────────────────────────

/// `Event::JobVisibilityChanged` visibility=0 → 公开通知。
pub fn visibility_public_user_notify(job_id: &str, title: &str) -> String {
    format!("[可见性变更] **{title}**（{job_id}）已切换为**公开（public）**，等待服务商主动联系。")
}

/// `Event::JobVisibilityChanged` visibility=1 → 私有通知。
pub fn visibility_private_user_notify(job_id: &str, title: &str) -> String {
    format!("[可见性变更] **{title}**（{job_id}）已切换为**私有（private）**。")
}

// ── Event::JobPaymentModeChanged ───────────────────────────────────

/// `Event::JobPaymentModeChanged` escrow 分支 Step 4 推给用户的通知。
pub fn payment_mode_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!("**{title}**（{job_id}）更新支付方式成功，设置服务商 **<providerName>**（<providerAgentId>）接单中...")
}

/// `Event::JobPaymentModeChanged` x402 分支 — 重放成功时推给用户的交付物通知。
pub fn x402_deliverable_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 交付物已获取] 任务 {job_id} endpoint 重放成功。\n\
         \x20\x20服务商 AgentID：<providerAgentId>\n\
         \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
         \x20\x20---交付物内容---\n\
         \x20\x20<replayBody 完整内容，JSON 则格式化输出>\n\
         \x20\x20---交付物结束---\n\
         \x20\x20正在等待链上确认，确认后将自动完成任务。"
    )
}

/// `Event::JobPaymentModeChanged` x402 分支 — 重放失败时推给用户的通知。
pub fn x402_replay_fail_payment_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 重放失败] 任务 {job_id} 已接单但 endpoint 重放失败。\n\
         \x20\x20HTTP 状态：<replayStatus>\n\
         \x20\x20错误信息：<replayBody>\n\
         \x20\x20等待链上确认后**不会自动执行 complete**，需要用户指示。"
    )
}

// ── Event::NegotiateReply (over budget) ────────────────────────────

/// `Event::NegotiateReply` 报价超出 max_budget 时推给用户的决策 prompt。
pub fn over_budget_user_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务 {short_id}] 服务商报价超出最高预算，协商已终止。请选择下一步：\n\
         \x20\x20\x20\x20\x20\x20A. 查看推荐服务商列表\n\
         \x20\x20\x20\x20\x20\x20B. 指定其他服务商（请提供 agentId）\n\
         \x20\x20\x20\x20\x20\x20C. 关闭任务"
    )
}

// ── Pseudo events (close / set_public) ─────────────────────────────

/// 关闭任务后推给用户的通知。
pub fn close_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 已关闭。")
}

/// 转为公开任务后推给用户的通知。
pub fn set_public_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 已转为公开任务，等待服务商主动申请。")
}

// ── Event::SubmitExpired ───────────────────────────────────────────

/// `Event::SubmitExpired` 推给用户的服务商提交超时通知。
pub fn submit_expired_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 的服务商未在截止时间前提交交付物，已自动申请退款，资金将退回你的账户。")
}

// ── Event::RefuseExpired ───────────────────────────────────────────

/// `Event::RefuseExpired` 推给用户的服务商仲裁超时通知。
pub fn refuse_expired_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 的服务商在你拒绝交付物后未及时发起仲裁，已自动申请退款，资金将退回你的账户。")
}

// ── Event::ReviewDeadlineWarn ──────────────────────────────────────

/// `Event::ReviewDeadlineWarn` 给用户看的验收截止 prompt(`xmtp_prompt_user.userContent`)。
pub fn review_deadline_warn_user_prompt(job_id: &str) -> String {
    format!(
        "\x20\x20[验收截止提醒] 任务 {job_id} 的验收截止时间即将到期。\n\
         \x20\x20超时后服务商可自动领取资金。\n\
         \x20\x20请尽快决定：\n\
         \x20\x20A. 通过验收 — 回复「通过」\n\
         \x20\x20B. 拒绝交付物 — 回复「拒绝」并说明原因"
    )
}

// ── Event::ReviewExpired ───────────────────────────────────────────

/// `Event::ReviewExpired` 推给用户的验收超时通知。
pub fn review_expired_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[验收超时] 任务 {job_id} 的验收窗口已过期，你未在截止时间前做出验收决定。\n\
         \x20\x20服务商现在可自动领取资金。\n\
         \x20\x20等待服务商操作中..."
    )
}

// ── Event::JobAutoCompleted ────────────────────────────────────────

/// `Event::JobAutoCompleted` 推给用户的自动完成通知。
pub fn job_auto_completed_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20[任务自动完成] **{title}**（{job_id}）因**验收超时**，资金已自动释放给服务商。\n\
         \x20\x20本任务流程结束。"
    )
}

// ── Event::RewardClaimed ───────────────────────────────────────────

/// `Event::RewardClaimed` 推给用户的奖励到账通知。
pub fn reward_claimed_user_notify(job_id: &str, title: &str) -> String {
    format!("[奖励已到账] **{title}**（{job_id}）的**奖励/退款已成功领取**到您的钱包。")
}

// ── Event::WakeupNotify ────────────────────────────────────────────

/// `Event::WakeupNotify` 已有 pending 条目时推给用户的恢复通知。
pub fn wakeup_resume_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 已恢复，请继续之前的决策")
}

// ── provider_conversation 无更多服务商 ──────────────────────────────

/// `provider_conversation` B-Step 4 无更多待沟通服务商时推给用户的通知。
pub fn no_more_sellers_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 当前没有更多待沟通服务商，建议等待新服务商联系或调整任务描述。")
}

// ── Escalation（preamble 异常升级）─────────────────────────────────

/// preamble 异常升级硬规则 1) 协议理解错位 — content 模板。
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ 协议理解错位] 任务 {job_id} 多次澄清同一流程对方仍重复，已停回复，请介入或给新指令。")
}

/// preamble 异常升级硬规则 2) 执行报错 — content 模板。
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!("[⚠️ 执行报错] 任务 {job_id} <动作简述,如「确认接单」/「验收交付物」/「提交证据」>失败,请查看后给新指令。")
}
