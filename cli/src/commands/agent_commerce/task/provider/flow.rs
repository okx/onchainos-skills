//! Provider 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 目的：把散落在 provider.md 里的 Scene 步骤集中到代码里，让 agent 只需
//! `exec onchainos agent next-action ...` 拿提示词直接执行，不用推理整份文档。

/// 根据 jobStatus 生成 provider 下一步动作的结构化提示词
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    let header_template = format!(
        "jobId:  {job_id}\n来自:   {agent_id} [PROVIDER]\n类型:   REPLY\n会话:   <来源消息的'会话:'行的值>\n----------------------------------------"
    );

    match job_status {
        // ─── Scene 3: 接单申请已上链，生成付款单给买家 ────────────────
        "TASK_APPLIED" => format!(
            "【当前状态】TASK_APPLIED（链上已确认接单申请）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 CLI 生成付款单：**\n\
             ```bash\n\
             onchainos agent payment {job_id}\n\
             ```\n\
             返回字段：amount、tokenSymbol、providerAddress、paymentMode。\n\n\
             **Step 2 — 输出 header 格式回复，把付款单发给买家（纯文本，不加 markdown/代码块）：**\n\n\
             {header_template}\n\
             接单申请已上链确认（TASK_APPLIED）。以下是付款单：\n\
             金额：<amount> <tokenSymbol>\n\
             支付代币：<tokenSymbol>（XLayer）\n\
             收款地址：<providerAddress>\n\
             支付方式：<paymentMode>\n\
             请确认接受并完成付款。\n\n\
             【后续事件】\n\
             - TASK_ACCEPTED → 买家确认，开始执行任务\n\
             - 若 payment 命令不可用，可从 `onchainos agent status {job_id}` 手动组织付款单。\n"
        ),

        // ─── Scene 4: 买家已确认接单，执行任务并交付 ─────────────────
        "TASK_ACCEPTED" => format!(
            "【当前状态】TASK_ACCEPTED（买家已确认接单，资金托管）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序，不得跳步）】\n\n\
             **Step 1 — 必须先调用 notify_main 工具，通知主 session 接单成功：**\n\
             ```\n\
             notify_main(\n\
             \x20\x20jobId=\"{job_id}\",\n\
             \x20\x20conversationId=\"<来源消息的'会话:'行的值>\",\n\
             \x20\x20message=\"[接单成功通知] 任务 {job_id} 已完成接单\\n- 标题：<title>\\n- 描述：<description>\\n- 协商价格：<amount> <tokenSymbol>\\n- 支付方式：<mode>\\n- 卖家 AgentID：{agent_id}\\n\\n资金已托管，开始执行任务。\"\n\
             )\n\
             ```\n\
             字段值从 `onchainos agent common context {job_id} --role seller` 输出中提取。\n\n\
             **Step 2 — 向买家输出 header 格式回复确认：**\n\n\
             {header_template}\n\
             已收到接单确认（TASK_ACCEPTED），开始执行任务。\n\n\
             **Step 3 — 执行任务（mock 环境可直接跳过），完成后调用 CLI 提交交付物：**\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --message \"任务已完成，请验收\"\n\
             ```\n\
             CLI 内部：POST submit API → 签名 uopHash → 广播上链。\n\n\
             【⚠️ 重要】执行 deliver 后不得立即回复买家'请验收'，必须等 TASK_SUBMITTED 通知再回复。\n\n\
             【后续事件】\n\
             - TASK_SUBMITTED → 交付物已上链，再次调用 next-action 获取下一步\n"
        ),

        // ─── Scene 5: 交付物已上链，通知买家验收 ─────────────────────
        "TASK_SUBMITTED" => format!(
            "【当前状态】TASK_SUBMITTED（交付物已上链确认）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             从 TASK_SUBMITTED 通知的 payload 中提取 deliverableUrl（字段 `deliverable`），\n\
             输出 header 格式回复告诉买家验收：\n\n\
             {header_template}\n\
             交付物已上链确认（TASK_SUBMITTED），交付链接：<deliverableUrl>。等待买家验收。\n\n\
             【后续事件】\n\
             - TASK_COMPLETED → 验收通过，调用 next-action 获取收尾步骤\n\
             - TASK_REFUSED   → 买家拒绝，调用 next-action 获取处理步骤\n"
        ),

        // ─── Scene 6: 买家拒绝交付物 ─────────────────────────────────
        "TASK_REFUSED" => format!(
            "【当前状态】TASK_REFUSED（买家拒绝交付物）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 向买家输出 header 格式回复：**\n\n\
             {header_template}\n\
             已收到买家拒绝通知（TASK_REFUSED）。正在确认后续处理方案，请稍候。\n\n\
             **Step 2 — 调用 notify_main 把决策请求推给主 session 用户：**\n\
             ```\n\
             notify_main(\n\
             \x20\x20jobId=\"{job_id}\",\n\
             \x20\x20conversationId=\"<子 session 会话 ID>\",\n\
             \x20\x20message=\"任务 {job_id} 被买家拒绝。请用户选择：\\n1. 发起仲裁 → 回复'发起仲裁，理由是<理由>'\\n2. 同意退款 → 回复'同意退款'\"\n\
             )\n\
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
             **Step 2 — 收到 TASK_DISPUTED 通知后，上传链下证据：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 3 — 向买家输出 header 回复：**\n\n\
             {header_template}\n\
             已发起仲裁（TASK_DISPUTED），等待仲裁员裁决。\n\n\
             【后续事件】\n\
             - TASK_COMPLETED（仲裁胜诉）\n\
             - TASK_REJECTED（仲裁败诉）\n"
        ),

        // ─── Scene 6.2: 用户决定同意退款 ─────────────────────────────
        "AGREE_REFUND" => format!(
            "【当前动作】同意退款\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI（上链）：**\n\
             ```bash\n\
             onchainos agent agree-refund {job_id}\n\
             ```\n\n\
             **Step 2 — 向买家输出 header 回复：**\n\n\
             {header_template}\n\
             已同意退款，等待链上确认（TASK_REJECTED）。\n"
        ),

        // ─── Scene 7: 任务完成（验收通过 / 仲裁胜诉） ────────────────
        "TASK_COMPLETED" => format!(
            "【当前状态】TASK_COMPLETED（任务完成，资金已释放）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家输出 header 格式回复：\n\n\
             {header_template}\n\
             任务已完成（TASK_COMPLETED），资金已释放。感谢合作。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.5: 任务终止（退款 / 仲裁败诉） ──────────────────
        "TASK_REJECTED" => format!(
            "【当前状态】TASK_REJECTED（任务终止，资金退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家输出 header 格式回复：\n\n\
             {header_template}\n\
             任务已终止（TASK_REJECTED），资金已退还买家。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.4: 仲裁进行中，提交证据 ─────────────────────────
        "TASK_DISPUTED" => format!(
            "【当前状态】TASK_DISPUTED（仲裁已发起）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             在 1 小时准备期内上传链下证据（多次可重复）：\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片>\n\
             ```\n\n\
             【后续事件】\n\
             - TASK_COMPLETED → 胜诉\n\
             - TASK_REJECTED  → 败诉\n"
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
