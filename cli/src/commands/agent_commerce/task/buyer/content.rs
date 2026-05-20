//! Buyer 端消息模板 — 单一维护点。
//!
//! 收两类模板:
//!
//! 1. **User-facing** (`xmtp_dispatch_user(content)` / `xmtp_prompt_user(userContent)`)
//!    给用户看的聊天内容。命名后缀 `_user_notify` / `_user_prompt`。
//!    规则:**禁用技术术语** —— tool 名(`xmtp_*`) / 事件名(`provider_applied`/`job_*` 等) /
//!    状态名(`Open`/`accepted` 等英文枚举除外,这些是文档明确保留的字面量) / CLI flag(`--*`) /
//!    skill 名(`okx-agent-identity` 等) / 后端方法名(`claimAutoComplete` 等)。
//!    **本文件模板的字面量是英文**(按 PM Review 翻译基线落地,源:
//!    `https://okg-block.sg.larksuite.com/docx/YSHcdZaWmo2KofxaHRuloeBYgme` §一),
//!    作为 sub agent LOCALIZATION_PREFIX 翻译时的 source-of-truth —— 英文用户原样呈现,
//!    非英文用户由 sub agent 翻成等价口语化表达。
//!    术语约定:任务→Job,用户→User Agent,服务商→ASP,agentId 驼峰,escrow/non-escrow/x402 小写,
//!    用户回复指令用 plain `"..."` 双引号。
//!
//! 2. **Peer-facing** (`xmtp_send` content,给服务商 sub agent)
//!    agent-to-agent 协议消息。命名后缀 `_to_seller`。
//!    规则:可以含协议字面量(`[intent:*]` 等);
//!    **禁止指挥对方调 CLI**(对方有自己的 flow.rs,会按链事件自决,你下指令是越权)。
//!
//! 字段值占位符用 `<...>` 包,agent 拿 `common context` / 上下文填充。
//! 添加新文案 → 加新 fn;改文案 → 改 fn 体;flow.rs 永远只调这里、不内嵌字面量。

// ── Event::JobCreated ──────────────────────────────────────────────

/// `Event::JobCreated` Step 0 推给用户的任务上链成功通知。
pub fn job_created_user_notify(job_id: &str, notify_text: &str) -> String {
    format!("Job `{job_id}` confirmed on-chain (status: Open). {notify_text}")
}

/// 指定服务商离线时推给用户的提示（D-Step 1.5）。
pub fn provider_offline_user_prompt(job_id: &str, short_id: &str, dp_id: &str) -> String {
    format!(
        "[Job {short_id} — you are the User Agent] The designated ASP (agentId={dp_id}) for job {job_id} \
         is currently offline. Negotiation requires the ASP to be online. \
         Please choose:\n\
         A. Designate another ASP — please provide the agentId\n\
         B. Make the job public — let more ASPs discover it\n\
         C. Close the job"
    )
}

// ── Event::JobAccepted ─────────────────────────────────────────────

/// `Event::JobAccepted` 分支 A（escrow）推给用户的接单成功通知。
pub fn job_accepted_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20[Job Accepted] Job `{job_id}` has been accepted; execution begins.\n\
         \x20\x20Title: {title}\n\
         \x20\x20Description: <description>\n\
         \x20\x20Deliverable: <deliverable>\n\
         \x20\x20ASP agentId: <providerAgentId>\n\
         \x20\x20Payment: escrow\n\
         \x20\x20Amount: <tokenAmount> <tokenSymbol>\n\
         \x20\x20Waiting for the ASP to execute and submit the deliverable."
    )
}

/// `Event::JobAccepted` 分支 B（x402）重放失败时推给用户的通知。
pub fn job_accepted_x402_replay_fail_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 Replay Failed] Job `{job_id}` was accepted but the endpoint replay failed.\n\
         \x20\x20HTTP status: <replayStatus>\n\
         \x20\x20Error: <replayBody>\n\
         \x20\x20The job is now in `accepted` status. Please give a new instruction; the agent will not auto-retry."
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
        "\x20\x20\x20\x20[Job {short_id} — you are the User Agent] The dispute is confirmed on-chain. You must submit off-chain evidence within 1 hour. Please provide:\n\
         \x20\x20\x20\x20- Text summary (required): key evidence that the deliverable failed the quality standards\n\
         \x20\x20\x20\x20- Image path (optional): local file path to screenshots, chat logs, etc.\n\
         \x20\x20\x20\x20Reply format example: \"Evidence: the deliverable is missing X/Y/Z; image: /path/to/screenshot.png\""
    )
}

// ── Event::JobCompleted ────────────────────────────────────────────

/// `Event::JobCompleted` 分支 A（escrow）推给用户的任务完成通知。
pub fn job_completed_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Job Completed] {title} (`{job_id}`) — approved by the User Agent; funds released to the ASP.\n\
         \x20\x20\x20\x20  - Spent: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Payment: escrow\n\
         \x20\x20\x20\x20  - txHash: <txHash>\n\
         \x20\x20\x20\x20  - Settled at: <timestamp>\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20This job is complete."
    )
}

/// `Event::JobCompleted` 分支 B（x402）推给用户的最终汇总通知。
pub fn job_completed_x402_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[x402 Job Completed] {title} (`{job_id}`) — all steps complete.\n\
         \x20\x20\x20\x20  - Spent: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Payment: x402\n\
         \x20\x20\x20\x20  - Settled at: <timestamp>\n\
         \x20\x20\x20\x20To rate the ASP, reply \"rate\"."
    )
}

// ── Event::DisputeResolved ─────────────────────────────────────────

/// `Event::DisputeResolved` 用户胜诉推给用户的通知。
pub fn dispute_won_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Dispute Won] {title} (`{job_id}`) — dispute resolved; User Agent wins.\n\
         \x20\x20\x20\x20  - Refund: <tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - Outcome: ClientWins\n\
         \x20\x20\x20\x20This job is complete. To rate the ASP, reply \"rate\"."
    )
}

/// `Event::DisputeResolved` 用户败诉推给用户的通知。
pub fn dispute_lost_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Dispute Lost] {title} (`{job_id}`) — dispute resolved; ASP wins.\n\
         \x20\x20\x20\x20  - Loss: <tokenAmount> <tokenSymbol> (funds released to the ASP)\n\
         \x20\x20\x20\x20  - Outcome: ProviderWins\n\
         \x20\x20\x20\x20This job is complete. To rate the ASP, reply \"rate\"."
    )
}

// ── Event::JobRefunded ─────────────────────────────────────────────

/// `Event::JobRefunded` 推给用户的退款完成通知。
pub fn job_refunded_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Refund Settled] Job `{job_id}` — refund confirmed on-chain; funds returned to your wallet. This job is complete."
    )
}

// ── Event::JobAutoRefunded ─────────────────────────────────────────

/// `Event::JobAutoRefunded` 推给用户的自动退款成功通知。
pub fn job_auto_refunded_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Auto-Refund Settled] {title} (`{job_id}`) — escrowed funds returned to your wallet. This job is complete."
    )
}

// ── Event::JobExpired ──────────────────────────────────────────────

/// `Event::JobExpired` 推给用户的任务超时通知。
pub fn job_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` has expired (no ASP accepted before the accept deadline, or no deliverable submitted before the submit deadline). The job is now closed."
    )
}

// ── Event::JobClosed ───────────────────────────────────────────────

/// `Event::JobClosed` 推给用户的任务关闭通知。
pub fn job_closed_user_notify(job_id: &str, title: &str) -> String {
    format!("{title} (`{job_id}`) has been closed; funds have been returned.")
}

// ── Event::JobVisibilityChanged ────────────────────────────────────

/// `Event::JobVisibilityChanged` visibility=0 → 公开通知。
pub fn visibility_public_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now public. Waiting for ASPs to reach out.")
}

/// `Event::JobVisibilityChanged` visibility=1 → 私有通知。
pub fn visibility_private_user_notify(job_id: &str, title: &str) -> String {
    format!("[Visibility Changed] {title} (`{job_id}`) is now private.")
}

// ── Event::JobPaymentModeChanged ───────────────────────────────────

/// `Event::JobPaymentModeChanged` escrow 分支 Step 4 推给用户的通知。
pub fn payment_mode_escrow_user_notify(job_id: &str, title: &str) -> String {
    format!("{title} (`{job_id}`) — payment mode updated successfully; ASP <providerName> (`<providerAgentId>`) is accepting...")
}

/// x402 set-payment-mode 上链成功后、task-402-pay 之前推给用户的过渡通知。
pub fn x402_paying_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "[x402 支付中] 任务 **{title}**（{job_id}）已与服务商（<providerAgentId>）达成 x402 协议，\
         费用 <tokenAmount> <tokenSymbol>，正在支付并获取交付物..."
    )
}

/// `Event::JobPaymentModeChanged` x402 分支 — 重放成功时推给用户的交付物通知。
pub fn x402_deliverable_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 Deliverable Received] Job `{job_id}` endpoint replayed successfully.\n\
         \x20\x20ASP agentId: <providerAgentId>\n\
         \x20\x20Amount: <tokenAmount> <tokenSymbol>\n\
         \x20\x20---Deliverable---\n\
         \x20\x20<replayBody full content, formatted if JSON>\n\
         \x20\x20---End of deliverable---\n\
         \x20\x20Waiting for on-chain confirmation. The job will auto-complete once confirmed."
    )
}

/// `Event::JobPaymentModeChanged` x402 分支 — 重放失败时推给用户的通知。
pub fn x402_replay_fail_payment_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[x402 Replay Failed] Job `{job_id}` was accepted but the endpoint replay failed.\n\
         \x20\x20HTTP status: <replayStatus>\n\
         \x20\x20Error: <replayBody>\n\
         \x20\x20Auto-complete will not run after `job_accepted`. Please give a new instruction; the agent will not auto-retry."
    )
}

// ── Event::NegotiateReply (over budget) ────────────────────────────

/// `Event::NegotiateReply` 报价超出 max_budget 时推给用户的决策 prompt。
pub fn over_budget_user_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[Task {short_id}] 服务商报价超出最高预算，协商已终止。请选择下一步：\n\
         \x20\x20\x20\x20\x20\x20A. 查看推荐服务商列表\n\
         \x20\x20\x20\x20\x20\x20B. 指定其他服务商（请提供 agentId）\n\
         \x20\x20\x20\x20\x20\x20C. 关闭任务"
    )
}

// ── Pseudo events (close / set_public) ─────────────────────────────

/// 关闭任务后推给用户的通知。
pub fn close_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` has been closed.")
}

/// 转为公开任务后推给用户的通知。
pub fn set_public_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` is now public. Waiting for ASPs to apply.")
}

// ── Event::SubmitExpired ───────────────────────────────────────────

/// `Event::SubmitExpired` 推给用户的服务商提交超时通知。
pub fn submit_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not submit the deliverable before the deadline. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::RefuseExpired ───────────────────────────────────────────

/// `Event::RefuseExpired` 推给用户的服务商仲裁超时通知。
pub fn refuse_expired_user_notify(job_id: &str) -> String {
    format!(
        "Job `{job_id}` — the ASP did not file a dispute in time after you rejected the deliverable. An auto-refund has been requested; funds will return to your wallet."
    )
}

// ── Event::ReviewDeadlineWarn ──────────────────────────────────────

/// `Event::ReviewDeadlineWarn` 给用户看的验收截止 prompt(`xmtp_prompt_user.userContent`)。
pub fn review_deadline_warn_user_prompt(job_id: &str) -> String {
    format!(
        "\x20\x20[⏰ Review Deadline Warning] Job `{job_id}` — the review deadline is approaching.\n\
         \x20\x20After expiry, the ASP can auto-claim the funds.\n\
         \x20\x20Please decide soon:\n\
         \x20\x20A. Approve → reply \"approve\"\n\
         \x20\x20B. Reject → reply \"reject\" and provide  {{reason}}"
    )
}

// ── Event::ReviewExpired ───────────────────────────────────────────

/// `Event::ReviewExpired` 推给用户的验收超时通知。
pub fn review_expired_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20[Review Expired] Job `{job_id}` — the review window has expired; you did not decide before the deadline.\n\
         \x20\x20The ASP can now claim the funds automatically. Waiting for the ASP's action..."
    )
}

// ── Event::JobAutoCompleted ────────────────────────────────────────

/// `Event::JobAutoCompleted` 推给用户的自动完成通知。
pub fn job_auto_completed_user_notify(job_id: &str, title: &str) -> String {
    format!(
        "\x20\x20[Job Auto-Completed] {title} (`{job_id}`) — the review window expired and the ASP has claimed the funds.\n\
         \x20\x20Status: completed. This job is complete."
    )
}

// ── Event::RewardClaimed ───────────────────────────────────────────

/// `Event::RewardClaimed` 推给用户的奖励到账通知。
pub fn reward_claimed_user_notify(job_id: &str, title: &str) -> String {
    format!("[Reward Claimed] {title} (`{job_id}`) — reward / refund successfully claimed to your wallet.")
}

// ── Event::WakeupNotify ────────────────────────────────────────────

/// `Event::WakeupNotify` 已有 pending 条目时推给用户的恢复通知。
pub fn wakeup_resume_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` is back online. Please continue your decision in the user session.")
}

// ── provider_conversation 无更多服务商 ──────────────────────────────

/// `provider_conversation` B-Step 4 无更多待沟通服务商时推给用户的通知。
pub fn no_more_sellers_user_notify(job_id: &str) -> String {
    format!("Job `{job_id}` — no more pending ASPs. Wait for new ASPs to reach out, or adjust the job description.")
}

// ── Escalation（preamble 异常升级）─────────────────────────────────

/// preamble 异常升级硬规则 1) 协议理解错位 — content 模板。
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ Protocol Misalignment] Job `{job_id}` — the remote agent repeatedly sends messages that do not match the current flow. Replies have stopped. Please intervene manually to continue.")
}

/// preamble 异常升级硬规则 2) 执行报错 — content 模板。
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!("[⚠️ CLI Error] Job `{job_id}` <action summary, e.g. \"confirm accept\" / \"approve deliverable\" / \"submit evidence\"> failed. Please review and give a new instruction; the agent will not auto-retry.")
}
