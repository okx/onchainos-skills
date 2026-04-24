//! Client (买家) 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 对应 provider/flow.rs 的买家版本，让 agent 只需
//! `exec onchainos agent next-action --role buyer ...` 拿提示词直接执行。

/// 根据 jobStatus 生成 client/buyer 下一步动作的结构化提示词
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    let header_template = format!(
        "jobId:  {job_id}\n来自:   {agent_id} [BUYER]\n类型:   REPLY\n会话:   <来源消息的'会话:'行的值>\n----------------------------------------"
    );

    match job_status {
        // ─── Scene 0: 任务上链确认，自动联系推荐卖家 ────────────────
        "job_created" => format!(
            "【当前状态】job_created（任务已上链，状态 Open）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序，不询问用户，全自动执行）】\n\n\
             **Step 1 — 查询推荐卖家：**\n\
             ```bash\n\
             onchainos agent recommend {job_id}\n\
             ```\n\
             缓存完整推荐列表，记录 currentProviderIndex = 0。\n\n\
             **Step 2 — 向第一个推荐卖家发起协商（子 session 自动创建）：**\n\
             直接输出文本：\n\n\
             {header_template}\n\
             你好，我有一个任务（jobId: {job_id}）想请你来完成，请问你感兴趣吗？\n\n\
             **Step 3 — 通知主 session：**\n\
             已自动联系推荐卖家（<providerAgentId>），等待对方回复。\n\n\
             【后续事件】\n\
             - 卖家回复 NEGOTIATE / REPLY → 进入协商（Scene 2）\n\
             - 卖家无回应 → 自动切换下一个推荐卖家\n"
        ),

        // ─── Scene 6: 卖家申请接单，确认接单（区分支付方式） ──────────
        "provider_applied" => format!(
            "【当前状态】provider_applied（卖家已链上申请接单，消息含卖家账单信息）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 通知主 session（用户（确认））：**\n\
             卖家已申请接单，请确认支付方式（escrow/non_escrow/x402）并确认接单。\n\n\
             **Step 2 — 用户确认后，确认接单（命令自动处理 setPaymentMode + accept 签名上链）：**\n\n\
             ▸ **担保支付（escrow，默认）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode escrow\n\
             ```\n\n\
             ▸ **非担保支付（non_escrow）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode non_escrow\n\
             ```\n\n\
             ▸ **x402：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode x402\n\
             ```\n\n\
             **Step 3 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             已确认接单，等待你开始执行任务。\n\n\
             【后续事件】\n\
             - job_accepted → 记录状态，等待卖家交付\n"
        ),

        // ─── job_accepted: 记录状态，等待卖家交付 ──────────────────
        "job_accepted" => format!(
            "【当前状态】job_accepted（买家已确认，资金托管完成）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             无需执行任何 CLI 命令。记录状态，等待卖家执行任务并提交交付物。\n\n\
             可选：向卖家输出 header 格式回复确认：\n\n\
             {header_template}\n\
             接单已确认，期待你的交付。\n\n\
             【后续事件】\n\
             - job_submitted → 验收交付物（Scene 5）\n"
        ),

        // ─── Scene 7: 卖家提交交付物，验收（区分支付方式） ─────────────
        "job_submitted" => format!(
            "【当前状态】job_submitted（卖家已提交交付物）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 查询交付物详情：**\n\
             ```bash\n\
             onchainos agent status {job_id}\n\
             ```\n\
             提取 `deliverableUrl`、`qualityStandards` 和 `paymentMode`。\n\n\
             **Step 2 — 通知主 session 请求用户决策（用户（确认），必须等待回复）：**\n\
             调用 `notify_main` 工具：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<子 session 会话 ID>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[交付物验收] 任务 {job_id} 卖家已提交交付物。\n\
             \x20\x20\x20\x20- 交付物地址：<deliverableUrl>\n\
             \x20\x20\x20\x20- 验收标准：<qualityStandards>\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20请确认：接受（验收通过）还是拒绝（不达标）？\n\
             ```\n\n\
             **Step 3 — 根据用户决策执行（按支付方式分别处理）：**\n\n\
             ▸ **担保支付（escrow）— 可接受或拒绝：**\n\
             - 用户接受（验收通过，释放资金）：\n\
               ```bash\n\
               onchainos agent complete {job_id}\n\
               ```\n\
             - 用户拒绝（不达标，进入仲裁/退款流程）：\n\
               ```bash\n\
               onchainos agent reject {job_id} --reason \"<拒绝原因>\"\n\
               ```\n\n\
             ▸ **非担保支付（non_escrow）— 只能接受，不能拒绝：**\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\n\
             【后续事件】\n\
             - job_completed → 任务完成\n\
             - job_refused → 等待卖家决定（仲裁/退款）（仅 escrow）\n"
        ),

        // ─── job_refused: 买家已拒绝，等待卖家决策 ─────────────────
        "job_refused" => format!(
            "【当前状态】job_refused（买家已拒绝交付物，等待卖家决定）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             无需执行 CLI 命令。卖家有 24h 决定：\n\
             - 发起仲裁 → 你将收到 job_disputed\n\
             - 同意退款 → 你将收到 confirm_refund\n\
             - 24h 超时 → 系统自动退款，你将收到 confirm_refund\n\n\
             向卖家输出 header 格式回复：\n\n\
             {header_template}\n\
             交付物已拒绝，等待你的后续处理。\n\n\
             【后续事件】\n\
             - job_disputed → 提交买家证据（Scene 6）\n\
             - confirm_refund → 退款完成\n"
        ),

        // ─── Scene 6: 仲裁已发起，提交买家证据 ─────────────────────
        "job_disputed" => format!(
            "【当前状态】job_disputed（仲裁已发起）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 通知主 session 请求用户提供证据（用户（确认））：**\n\
             调用 `notify_main` 工具：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<子 session 会话 ID>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[仲裁通知] 任务 {job_id} 卖家已发起仲裁，需要提交证据。\n\
             \x20\x20\x20\x20请提供：\n\
             \x20\x20\x20\x201. 证据摘要（文字描述问题）\n\
             \x20\x20\x20\x202. 证据文件（截图/文档，可选）\n\
             ```\n\n\
             **Step 2 — 用户提供证据后，上传链下证据（买卖双方共用，自动识别角色）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 3 — 向卖家输出 header 格式回复确认：**\n\n\
             {header_template}\n\
             仲裁已发起（job_disputed），买家证据已提交，等待仲裁者裁决。\n\n\
             【后续事件】\n\
             - job_completed → 仲裁卖家胜诉，任务完成\n\
             - confirm_refund → 仲裁买家胜诉，退款\n"
        ),

        // ─── DISPUTE_EVIDENCE: 用户提供了证据，执行上传 ─────────────
        "DISPUTE_EVIDENCE" => format!(
            "【当前动作】提交买家仲裁证据\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 上传链下证据（买卖双方共用，自动识别角色）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 2 — 向卖家输出 header 回复：**\n\n\
             {header_template}\n\
             买家证据已提交，等待仲裁者裁决。\n"
        ),

        // ─── 任务完成 ─────────────────────────────────────────────────
        "job_completed" => format!(
            "【当前状态】job_completed（任务完成）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             任务已完成（job_completed），感谢合作。\n\n\
             **Step 2 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已验收完成。\n\n\
             **Step 3 — 评价卖家：**\n\
             ```bash\n\
             onchainos agent judge {job_id}\n\
             ```\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 仲裁结束（DisputeSettled） ─────────────────────────────
        "dispute_resolved" => format!(
            "【当前状态】dispute_resolved（仲裁已裁决）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 查询可领取代币及奖励：**\n\
             ```bash\n\
             onchainos agent status {job_id}\n\
             ```\n\
             检查 claimable 字段，确认是否有可领取的退款/奖励。\n\n\
             **Step 2 — 领取退款/奖金（如有 claimable）：**\n\
             ```bash\n\
             onchainos agent claim {job_id}\n\
             ```\n\
             签名 claim calldata → 广播，退款/奖金到账。\n\n\
             **Step 3 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），资金已处理。\n\n\
             **Step 4 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 仲裁已结束，请检查钱包余额。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 卖家同意退款（TODO: 后端尚未定义此 event，暂用 confirm_refund）
        "confirm_refund" => format!(
            "【当前状态】confirm_refund（卖家同意退款，任务终止）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             卖家已同意退款（confirm_refund），资金已退还。\n\n\
             **Step 2 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 卖家已同意退款，资金已返还至您的钱包。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 任务超时（OPEN→EXPIRED 或 ACCEPTED→EXPIRED）──────────
        "job_expired" => format!(
            "【当前状态】job_expired（任务超时，无人接单或卖家未提交）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 通知主 session（用户（确认））：**\n\
             任务 {job_id} 已超时（accept 截止前未接单 或 submit 截止前未提交），是否关闭任务回收资金？\n\n\
             **Step 2 — 用户确认后，关闭任务回收资金：**\n\
             ```bash\n\
             onchainos agent close {job_id}\n\
             ```\n\n\
             【后续事件】\n\
             - job_closed → 关闭完成，资金已回收\n"
        ),

        // ─── 任务已关闭（close tx 结果）─────────────────────────────
        "job_closed" => format!(
            "【当前状态】job_closed（任务已关闭）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已关闭，资金已回收。\n\n\
             检查 payload 中 status 字段：\n\
             - success → 任务已关闭\n\
             - failed → 关闭失败，按 errorCode 重试\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 可见性切换结果（setVisibility tx 结果）───────────────────
        "job_visibility_changed" => format!(
            "【当前状态】job_visibility_changed（公开/私有切换已上链）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             检查 payload 中 status 字段：\n\
             - success → 公开/私有切换已生效\n\
             - failed → 切换失败，按 errorCode 重试\n\n\
             **通知主 session（用户（通知））：**\n\
             任务 {job_id} 可见性已更新。\n"
        ),

        // ─── 支付模式切换结果（setPaymentMode tx 结果）────────────────
        "job_payment_mode_changed" => format!(
            "【当前状态】job_payment_mode_changed（支付模式切换已上链）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             检查 payload 中 status 字段：\n\
             - success → 支付模式已切换\n\
             - failed → 切换失败，按 errorCode 重试\n\n\
             **通知主 session（用户（通知））：**\n\
             任务 {job_id} 支付模式已更新。\n"
        ),

        // ─── 关闭任务（仅 Open 状态可用） ───────────────────────────
        "CLOSE" => format!(
            "【当前动作】关闭任务\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 关闭任务（仅 Open 状态有效）：**\n\
             ```bash\n\
             onchainos agent close {job_id}\n\
             ```\n\n\
             **Step 2 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已关闭。\n"
        ),

        // ─── 设为公开任务 ───────────────────────────────────────────
        "SET_PUBLIC" => format!(
            "【当前动作】转为公开任务\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 转为公开任务：**\n\
             ```bash\n\
             onchainos agent set-public {job_id}\n\
             ```\n\n\
             **Step 2 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已转为公开任务，等待卖家主动申请。\n"
        ),

        // ─── 卖家未提交交付物超时 ─────────────────────────────────────
        "submit_expired" | "SUBMIT_EXPIRED" => format!(
            "【系统通知】卖家提交交付物超时\n\
             【角色】买家（Client）\n\n\
             卖家未在规定期限内提交交付物，你可以申请自动退款。\n\n\
             **Step 1 — 通知主 session（用户确认）：**\n\
             任务 {job_id} 的卖家未在截止时间前提交交付物，是否申请自动退款？\n\n\
             **Step 2 — 用户确认后，领取自动退款：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 3 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已申请自动退款，资金将退回你的账户。\n"
        ),

        // ─── 买家拒绝后卖家仲裁超时 ─────────────────────────────────
        "refuse_expired" | "REFUSE_EXPIRED" => format!(
            "【系统通知】卖家仲裁超时\n\
             【角色】买家（Client）\n\n\
             你拒绝交付物后，卖家未在规定期限内发起仲裁，你可以申请自动退款。\n\n\
             **Step 1 — 通知主 session（用户确认）：**\n\
             任务 {job_id} 的卖家在你拒绝交付物后未及时发起仲裁，是否申请自动退款？\n\n\
             **Step 2 — 用户确认后，领取自动退款：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 3 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已申请自动退款，资金将退回你的账户。\n"
        ),

        // ─── 未知类型兜底 ───────────────────────────────────────────
        other => format!(
            "【未知状态】{other}\n\
             【建议】\n\
             1. 调用 `onchainos agent common context {job_id} --role buyer` 查看完整上下文\n\
             2. 如该状态不在预期流程内，等待用户指示\n\
             3. 不要预测/假设其他通知\n"
        ),
    }
}
