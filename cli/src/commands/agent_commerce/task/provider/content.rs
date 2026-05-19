//! Provider 端消息模板 — 单一维护点。
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
//! 2. **Peer-facing** (`xmtp_send` content,给买家 sub agent)
//!    agent-to-agent 协议消息。命名后缀 `_to_buyer`。
//!    规则:可以含协议字面量(`[intent:*]` / `fileKey`/`digest` 等);
//!    **禁止指挥对方调 CLI**(对方有自己的 flow.rs,会按链事件自决,你下指令是越权)。
//!
//! 字段值占位符用 `<...>` 包,agent 拿 `common context` / 上下文填充。
//! 添加新文案 → 加新 fn;改文案 → 改 fn 体;flow.rs 永远只调这里、不内嵌字面量。

/// `Event::JobAccepted` Step 1 推给用户的接单成功通知。
///
/// 每行前置 4 空格缩进,跟 flow.rs 里其它 step 的 content 块对齐。
/// (Rust 字符串续行会吞换行后的空白,所以缩进必须用 `\x20` 显式 escape。)
pub fn job_accepted_user_notify(job_id: &str, agent_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[接单成功通知] 任务 {job_id} 已完成接单\n\
         \x20\x20\x20\x20- 标题：<title>\n\
         \x20\x20\x20\x20- 描述：<description>\n\
         \x20\x20\x20\x20- 协商价格：<amount> <tokenSymbol>\n\
         \x20\x20\x20\x20- 支付方式：<填中文「担保」/「x402」,不要写 escrow 原值>\n\
         \x20\x20\x20\x20- 卖家：{agent_id}\n\
         \x20\x20\x20\x20资金已托管，卖家已开始执行任务。"
    )
}

/// `Event::JobRefused` Step 1 给用户看的决策 prompt(`xmtp_prompt_user.userContent`)。
///
/// 短 jobId 前缀让用户在多任务并发时一眼分清是哪个任务。
pub fn job_refused_user_decision_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务 {short_id} 你作为卖家] 任务被买家拒绝。请选择:\n\
         \x20\x20\x20\x201. 发起仲裁 → 回复『发起仲裁，理由是<理由>』\n\
         \x20\x20\x20\x202. 同意退款 → 回复『同意退款』"
    )
}

/// `Event::JobCompleted` Step 2 推给用户的任务完成通知。
///
/// 末尾轻引导评价(用户回复「评价」时由 `okx-agent-identity` skill 接管),不放评分细节 / CLI flag。
pub fn job_completed_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务完成 💰] 任务 {job_id}（<title>）已验收通过，资金已到账。\n\
         \x20\x20\x20\x20  - 收入：<tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - 买家：<buyerAgentId>\n\
         \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
         \x20\x20\x20\x20\n\
         \x20\x20\x20\x20如想给买家打分留言，告诉我「评价」即可。"
    )
}

/// `Event::DisputeResolved` 分支 A 卖家胜诉 — agent 在 A-Step 2 实际 claim 到非 0 奖励时的 user notify。
///
/// 末尾轻引导评价(同 JobCompleted)。
pub fn dispute_won_with_claim_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[仲裁胜诉 ⚖️💰] 任务 {job_id}（<title>）仲裁完成，**卖方胜诉**。\n\
         \x20\x20\x20\x20  - 任务收入：<tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - 已自动领取账户奖励：<claimed amount> <symbol>（txHash=<hash>）\n\
         \x20\x20\x20\x20  - 买家：<buyerAgentId>\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  如想给买家打分留言，告诉我「评价」即可。"
    )
}

/// `Event::DisputeResolved` 分支 A 卖家胜诉 — A-Step 1 `claimable` 输出全 0 (无可领) 时的 user notify。
pub fn dispute_won_no_claim_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[仲裁胜诉 ⚖️💰] 任务 {job_id}（<title>）仲裁完成，**卖方胜诉**。\n\
         \x20\x20\x20\x20  - 任务收入：<tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20  - 账户级待领奖励：无（已检查）\n\
         \x20\x20\x20\x20  - 买家：<buyerAgentId>\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  如想给买家打分留言，告诉我「评价」即可。"
    )
}

/// `Event::JobAutoCompleted` Step 1 code 非 0 (自动完成 tx 失败) 推给用户的失败通知。
pub fn job_auto_completed_failed_user_notify(job_id: &str) -> String {
    format!("[自动完成失败] 任务 {job_id} 自动完成交易失败。")
}

/// `Event::RewardClaimed` Step 1 code 非 0 (奖励领取 tx 失败) 推给用户的失败通知。
pub fn reward_claim_failed_user_notify(job_id: &str) -> String {
    format!("[奖励领取失败] 任务 {job_id} 奖励领取交易失败。")
}

/// `Event::RewardClaimed` Step 2 (奖励到账) 推给用户的成功通知。
pub fn reward_claimed_user_notify(job_id: &str) -> String {
    format!("[奖励已到账] 任务 {job_id} 的奖励已成功领取到您的钱包。")
}

/// `Event::WakeupNotify` 网络重启后,该 jobId 已有 pending 条目时推给用户的恢复通知。
pub fn wakeup_resume_user_notify(job_id: &str) -> String {
    format!("任务 {job_id} 已恢复,请继续之前的决策")
}

/// preamble 异常升级硬规则 1) 协议理解错位 — content 模板。
pub fn escalation_protocol_misread_notify(job_id: &str) -> String {
    format!("[⚠️ 协议理解错位] 任务 {job_id} 多次澄清同一流程对方仍重复，已停回复，请介入或给新指令。")
}

/// preamble 异常升级硬规则 2) 执行报错 — content 模板。
pub fn escalation_cli_failed_notify(job_id: &str) -> String {
    format!("[⚠️ 执行报错] 任务 {job_id} <动作简述,如「提交交付物」/「申请接单」/「拉付款单」>失败,请查看后给新指令。")
}

/// `Event::JobAutoCompleted` Step 2 推给用户的自动完成到账通知 (验收期超时,卖家通过 claimAutoComplete 领回款项)。
pub fn job_auto_completed_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务自动完成] 任务 {job_id}（<title>）买家验收期超时，资金已自动到账。\n\
         \x20\x20\x20\x20  - 收入：<tokenAmount> <tokenSymbol>\n\
         \x20\x20\x20\x20本任务流程结束。"
    )
}

/// `Event::SubmitDeadlineWarn` 给用户看的决策 prompt(`xmtp_prompt_user.userContent`)。
///
/// 短 jobId 前缀让用户在多任务并发时一眼分清是哪个任务(同 `job_refused_user_decision_prompt`)。
/// 用户回 `立即提交` → user-session 把决策 relay 回 sub,sub 跑交付流程;不回 → sub 等 submit_expired 触发退款。
pub fn submit_deadline_warn_user_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[⏰ 截止警告 任务 {short_id} 你作为卖家] 提交交付物时限快到了。\n\
         \x20\x20\x20\x20如果交付物已准备好,请回复『立即提交』,我会马上跑交付流程;\n\
         \x20\x20\x20\x20如果还没准备好,可以不回复——超时后买家可领取自动退款,担保资金原路返回买家,本任务作废。"
    )
}

/// `Event::DisputeResolved` 分支 B 卖家败诉 — B-Step 1 user notify。
pub fn dispute_lost_user_notify(job_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[仲裁败诉 ⚖️⚠️] 任务 {job_id}（<title>）仲裁完成，**买方胜诉**。\n\
         \x20\x20\x20\x20  - 损失：<tokenAmount> <tokenSymbol>（资金已退还买家）\n\
         \x20\x20\x20\x20  - 买家：<buyerAgentId>\n\
         \x20\x20\x20\x20  \n\
         \x20\x20\x20\x20  如想给买家打分留言，告诉我「评价」即可。"
    )
}

/// `Event::JobDisputed` Step 1 给用户看的证据收集 prompt(`xmtp_prompt_user.userContent`)。
pub fn job_disputed_user_evidence_prompt(short_id: &str) -> String {
    format!(
        "\x20\x20\x20\x20[任务 {short_id} 你作为卖家] 仲裁已上链，需要在 1 小时内提交链下证据。请提供:\n\
         \x20\x20\x20\x20- 文字摘要(必填):说明你已按验收标准完成的关键证据点\n\
         \x20\x20\x20\x20- 图片路径(可选):截图、设计稿、聊天记录等本地文件路径\n\
         \x20\x20\x20\x20回复格式示例:『证据：已按需求完成 X/Y/Z；图片：/path/to/screenshot.png』"
    )
}

/// `Event::JobAccepted` Step 3 分支 A (escrow 文本交付物) 给买家的 xmtp_send content。
///
/// **不指挥**对方 CLI——买家 sub agent 收到后会自己按 `Event::JobSubmitted` 剧本走。
pub fn deliver_text_to_buyer(job_id: &str) -> String {
    format!(
        "jobId: {job_id}\n\
         deliverableType: text\n\
         ---\n\
         <这里贴交付内容文本>\n\
         ---\n\
         [intent:deliver]"
    )
}

/// `Event::JobAccepted` Step 3 分支 A (escrow 文件交付物) 给买家的 xmtp_send content。
///
/// 5 个解密元数据字段(`fileKey`/`digest`/`salt`/`nonce`/`secret`/`filename`)是协议字面量,
/// 买家 sub agent 解析这些字段调 `xmtp_file_download` 取本地文件。
/// **不指挥**对方 CLI。
pub fn deliver_file_to_buyer(job_id: &str) -> String {
    format!(
        "jobId: {job_id}\n\
         deliverableType: file\n\
         fileKey: <A-Step 1 返回的 fileKey 完整字符串>\n\
         digest: <A-Step 1 返回的 digest>\n\
         salt: <A-Step 1 返回的 salt>\n\
         nonce: <A-Step 1 返回的 nonce>\n\
         secret: <A-Step 1 返回的 secret>\n\
         filename: <A-Step 1 返回的 filename>\n\
         [intent:deliver]"
    )
}

