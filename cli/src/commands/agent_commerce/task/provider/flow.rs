//! Provider 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 目的：把散落在 provider.md 里的 Scene 步骤集中到代码里，让 agent 只需
//! `exec onchainos agent next-action ...` 拿提示词直接执行，不用推理整份文档。

/// 根据 jobStatus 生成 provider 下一步动作的结构化提示词
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    // P2P 消息走 `xmtp_send` 工具（真实 XMTP 插件提供）。
    // 会话信息（sessionKey / toXmtpAddress / groupId / jobId）由当前 XMTP 子 session 自动解析，
    // agent 只需填 `content` 字段。旧的 text-header 格式（`jobId: / 来自: [PROVIDER] / 类型: REPLY / 会话: / ----`）
    // 已废弃，不要再输出。
    let xmtp_hint = format!(
        "⚠️ 两步必做（不能跳第 1 步）：\n\
         1) 先调 `session_status` 工具拿到当前子 session 的 `sessionKey` 字段，等 tool_result 返回\n\
         2) 再调 `xmtp_send` 工具，参数 `sessionKey`=上面拿到的值，`content`=下面这段纯自然语言正文（不要写 text header，不要包代码块）。\n\
         当前 session 关联 jobId={job_id}，我方 agentId={agent_id}。content 如下："
    );
    // 兼容变量名 —— 仍叫 header_template 让下方插值点少改一轮
    let header_template = &xmtp_hint;

    match job_status {
        // ─── Scene 3: 接单申请已上链，生成付款单给买家 ────────────────
        "provider_applied" => format!(
            "【当前状态】provider_applied（链上已确认接单申请）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 CLI 拉取链上支付预信息（从任务详情确定 tokenSymbol：USDT 或 USDG）：**\n\
             ```bash\n\
             onchainos agent get-payment {job_id} --token-symbol <USDT|USDG>\n\
             ```\n\
             返回字段（节选）：currency（token 地址）、recipient（你的钱包地址）、evaluator、submitWindow、disputeWindow、hook、salt、expiredAt。\n\n\
             **Step 2 — 调用 `xmtp_send` 工具发送消息，把付款单发给买家（纯文本，不加 markdown/代码块）：**\n\n\
             {header_template}\n\
             接单申请已上链确认（provider_applied）。以下是付款单：\n\
             金额：<tokenAmount> <tokenSymbol>（从 common context 或任务详情获取）\n\
             支付代币合约：<currency>\n\
             收款地址：<recipient>\n\
             仲裁者：<evaluator>\n\
             提交期：<submitWindow>s  仲裁期：<disputeWindow>s\n\
             salt：<salt>  有效期至：<expiredAt>\n\
             请确认接受并完成付款。\n\n\
             【后续事件】\n\
             - job_accepted → 买家已确认，开始执行任务\n\
             - 若 get-payment 命令不可用，可从 `onchainos agent status {job_id}` 手动组织付款单（退化模式）。\n"
        ),

        // ─── Scene 4: 买家已确认接单，执行任务并交付 ─────────────────
        "job_accepted" => format!(
            "【当前状态】job_accepted（买家已确认接单，资金托管）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序，不得跳步）】\n\n\
             **Step 1 — 调用工具名为 `notify_main` 的自定义工具（⚠️ 禁止使用 `sessions_send` / `xmtp_send` / 任何其他消息工具），通知主 session 接单成功：**\n\n\
             工具调用：\n\
             ```\n\
             tool: notify_main      ← 必须是这个名字，不是其他\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[接单成功通知] 任务 {job_id} 已完成接单\n\
             \x20\x20\x20\x20- 标题：<title>\n\
             \x20\x20\x20\x20- 描述：<description>\n\
             \x20\x20\x20\x20- 协商价格：<amount> <tokenSymbol>\n\
             \x20\x20\x20\x20- 支付方式：<mode>\n\
             \x20\x20\x20\x20- 卖家 AgentID：{agent_id}\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20资金已托管，开始执行任务。\n\
             ```\n\
             字段值从 `onchainos agent common context {job_id} --role seller` 输出中提取。\n\n\
             ⚠️ **如果找不到 `notify_main` 工具，直接跳到 Step 2**（不要用其他工具顶替）。`sessions_send` 不是本项目的工具，调它没用。\n\n\
             **Step 2 — 向买家调用 `xmtp_send` 工具发送消息确认：**\n\n\
             {header_template}\n\
             已收到接单确认（job_accepted），开始执行任务。\n\n\
             **Step 3 — 执行任务（mock 环境可直接跳过），完成后调用 CLI 提交交付物：**\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --message \"任务已完成，请验收\"\n\
             ```\n\
             CLI 内部：POST submit API → 签名 uopHash → 广播上链。\n\n\
             【⚠️ 重要】执行 deliver 后不得立即回复买家'请验收'，必须等 job_submitted 通知再回复。\n\n\
             【后续事件】\n\
             - job_submitted → 交付物已上链，再次调用 next-action 获取下一步\n"
        ),

        // ─── Scene 5: 交付物已上链，通知买家验收 ─────────────────────
        "job_submitted" => format!(
            "【当前状态】job_submitted（交付物已上链确认）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             从 job_submitted 通知的 payload 中提取 deliverableUrl（字段 `deliverable`），\n\
             调用 `xmtp_send` 工具发送消息告诉买家验收：\n\n\
             {header_template}\n\
             交付物已上链确认（job_submitted），交付链接：<deliverableUrl>。等待买家验收。\n\n\
             【后续事件】\n\
             - job_completed → 验收通过，调用 next-action 获取收尾步骤\n\
             - job_refused   → 买家拒绝，调用 next-action 获取处理步骤\n"
        ),

        // ─── Scene 6: 买家拒绝交付物 ─────────────────────────────────
        "job_refused" => format!(
            "【当前状态】job_refused（买家拒绝交付物）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 向买家调用 `xmtp_send` 工具发送消息：**\n\n\
             {header_template}\n\
             已收到买家拒绝通知（job_refused）。正在确认后续处理方案，请稍候。\n\n\
             **Step 2 — 调用工具名为 `notify_main` 的自定义工具（⚠️ 禁止 `sessions_send` 等其他名字），把决策请求推给主 session 用户：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<子 session 会话 ID>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20任务 {job_id} 被买家拒绝。请用户选择：\n\
             \x20\x20\x20\x201. 发起仲裁 → 回复'发起仲裁，理由是<理由>'\n\
             \x20\x20\x20\x202. 同意退款 → 回复'同意退款'\n\
             ```\n\n\
             **Step 3 — 等待主 session 用户决策（ws-channel 自动 relay 回子 session）。**\n\n\
             收到 USER_INSTRUCTION 后再次调用 next-action（传入用户决定的 DISPUTE_RAISE / AGREE_REFUND）。\n\n\
             ⚠️ 24h 内必须决策，否则资金自动退还买家。\n"
        ),

        // ─── Scene 6.3: 用户决定发起仲裁 ─────────────────────────────
        "DISPUTE_RAISE" => format!(
            "【当前动作】发起仲裁\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI 发起仲裁（上链）：**\n\
             ```bash\n\
             onchainos agent dispute raise {job_id} --reason \"<用户提供的理由或默认：已按验收标准完成>\"\n\
             ```\n\n\
             **Step 2 — 收到 job_disputed 通知后，上传链下证据：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 3 — 调用 `xmtp_send` 工具向买家发送：**\n\n\
             {header_template}\n\
             已发起仲裁（job_disputed），等待仲裁员裁决。\n\n\
             【后续事件】\n\
             - job_completed（仲裁胜诉）\n\
             - dispute_resolved（仲裁败诉）\n"
        ),

        // ─── Scene 6.2: 用户决定同意退款 ─────────────────────────────
        "AGREE_REFUND" => format!(
            "【当前动作】同意退款\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI（上链）：**\n\
             ```bash\n\
             onchainos agent agree-refund {job_id}\n\
             ```\n\n\
             **Step 2 — 调用 `xmtp_send` 工具向买家发送：**\n\n\
             {header_template}\n\
             已同意退款，等待链上确认（confirm_refund）。\n"
        ),

        // ─── Scene 7: 任务完成（验收通过 / 仲裁胜诉） ────────────────
        "job_completed" => format!(
            "【当前状态】job_completed（任务完成，资金已释放）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家调用 `xmtp_send` 工具发送消息：\n\n\
             {header_template}\n\
             任务已完成（job_completed），资金已释放。感谢合作。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.5a: 仲裁败诉（资金退还买家） ────────────────────
        "dispute_resolved" => format!(
            "【当前状态】dispute_resolved（仲裁已裁决，资金退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家调用 `xmtp_send` 工具发送消息：\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），资金已退还买家。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.5b: 卖家同意退款（TODO: 后端尚未定义此 event）───
        "confirm_refund" => format!(
            "【当前状态】confirm_refund（卖家已同意退款，资金退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家调用 `xmtp_send` 工具发送消息：\n\n\
             {header_template}\n\
             已同意退款（confirm_refund），资金已退还买家。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.4: 仲裁进行中，提交证据 ─────────────────────────
        "job_disputed" => format!(
            "【当前状态】job_disputed（仲裁已发起）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             在 1 小时准备期内上传链下证据（多次可重复）：\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片>\n\
             ```\n\n\
             【后续事件】\n\
             - job_completed → 胜诉\n\
             - dispute_resolved → 败诉\n"
        ),

        // ─── 未知类型兜底 ─────────────────────────────────────────────
        other => format!(
            "【未知状态】{other}\n\
             【建议】\n\
             1. 调用 `onchainos agent common context {job_id} --role seller` 查看完整上下文\n\
             2. 如该状态不在预期流程内，等待用户指示\n\
             3. 不要预测/假设其他通知\n"
        ),
    }
}
