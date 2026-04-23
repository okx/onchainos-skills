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
        "JOB_CREATED" => format!(
            "【当前状态】JOB_CREATED（任务已上链，状态 Open）\n\
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
        "TASK_APPLIED" => format!(
            "【当前状态】TASK_APPLIED（卖家已链上申请接单，消息含卖家账单信息）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 修改支付方式（所有模式都需要）：**\n\
             调用 `/priapi/v1/aieco/task/{job_id}/setPaymentMode`，入参：\n\
             - 0 = escrow（担保）\n\
             - 1 = direct（非担保）\n\
             - 2 = x402\n\
             签名并广播上链。\n\n\
             **Step 2 — 按支付方式分别处理：**\n\n\
             ▸ **担保支付（escrow，默认）：**\n\
             1. 调用 `/priapi/v1/aieco/task/{job_id}/pre-accept` 获取 calldata\n\
             2. 钱包签名 calldata\n\
             3. 调用 `/priapi/v1/aieco/task/{job_id}/accept`，签名结果广播上链\n\
             4. 调用【支付模块】进行担保支付（TODO）\n\
             → 任务状态变为 Accepted\n\n\
             ▸ **非担保支付（non_escrow）：**\n\
             1. 调用 `/priapi/v1/aieco/task/{job_id}/direct/accept` 生成 calldata\n\
             2. 签名 → 广播上链 → 任务状态变为 Accepted\n\
             3. 调用 `/priapi/v1/aieco/task/{job_id}` 获取卖家账单信息（收款地址+金额+代币）\n\
             4. 通知主 session（用户（确认））展示账单，请用户确认转账\n\
             5. 用户确认后调用【支付模块】进行非担保支付（TODO）\n\n\
             ▸ **x402：**\n\
             按 x402 支付流程处理（见 Scene 4）。\n\n\
             **Step 3 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             已确认接单，等待你开始执行任务。\n\n\
             【后续事件】\n\
             - TASK_ACCEPTED → 记录状态，等待卖家交付\n"
        ),

        // ─── TASK_ACCEPTED: 记录状态，等待卖家交付 ──────────────────
        "TASK_ACCEPTED" => format!(
            "【当前状态】TASK_ACCEPTED（买家已确认，资金托管完成）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             无需执行任何 CLI 命令。记录状态，等待卖家执行任务并提交交付物。\n\n\
             可选：向卖家输出 header 格式回复确认：\n\n\
             {header_template}\n\
             接单已确认，期待你的交付。\n\n\
             【后续事件】\n\
             - TASK_SUBMITTED → 验收交付物（Scene 5）\n"
        ),

        // ─── Scene 7: 卖家提交交付物，验收（区分支付方式） ─────────────
        "TASK_SUBMITTED" => format!(
            "【当前状态】TASK_SUBMITTED（卖家已提交交付物）\n\
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
             - 用户接受：\n\
               1. 调用 `/priapi/v1/aieco/task/{job_id}/pre-complete`（712标准）获取 calldata\n\
               2. 钱包签名 calldata\n\
               3. 调用 `/priapi/v1/aieco/task/{job_id}/complete`，广播上链\n\
               → 任务状态变为 Complete，合约自动释放资金\n\
             - 用户拒绝：\n\
               1. 调用 `/priapi/v1/aieco/task/{job_id}/pre-refuse`（712标准）获取 calldata\n\
               2. 钱包签名 calldata\n\
               3. 调用 `/priapi/v1/aieco/task/{job_id}/refuse`，广播上链\n\
               → 任务状态变为 Refuse，进入仲裁/退款流程\n\n\
             ▸ **非担保支付（non_escrow）— 只能接受，不能拒绝：**\n\
             调用 `/priapi/v1/aieco/task/{job_id}/direct/complete`\n\
             → 任务状态变为 Complete\n\n\
             【后续事件】\n\
             - TASK_COMPLETED → 任务完成\n\
             - TASK_REFUSED → 等待卖家决定（仲裁/退款）（仅 escrow）\n"
        ),

        // ─── TASK_REFUSED: 买家已拒绝，等待卖家决策 ─────────────────
        "TASK_REFUSED" => format!(
            "【当前状态】TASK_REFUSED（买家已拒绝交付物，等待卖家决定）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             无需执行 CLI 命令。卖家有 24h 决定：\n\
             - 发起仲裁 → 你将收到 TASK_DISPUTED\n\
             - 同意退款 → 你将收到 TASK_REJECTED\n\
             - 24h 超时 → 系统自动退款，你将收到 TASK_REJECTED\n\n\
             向卖家输出 header 格式回复：\n\n\
             {header_template}\n\
             交付物已拒绝，等待你的后续处理。\n\n\
             【后续事件】\n\
             - TASK_DISPUTED → 提交买家证据（Scene 6）\n\
             - TASK_REJECTED → 退款完成\n"
        ),

        // ─── Scene 6: 仲裁已发起，提交买家证据 ─────────────────────
        "TASK_DISPUTED" => format!(
            "【当前状态】TASK_DISPUTED（仲裁已发起）\n\
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
             **Step 2 — 用户提供证据后，上传链下证据：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 3 — 向卖家输出 header 格式回复确认：**\n\n\
             {header_template}\n\
             仲裁已发起（TASK_DISPUTED），买家证据已提交，等待仲裁者裁决。\n\n\
             【后续事件】\n\
             - TASK_COMPLETED → 仲裁卖家胜诉，任务完成\n\
             - TASK_REJECTED → 仲裁买家胜诉，退款\n"
        ),

        // ─── DISPUTE_EVIDENCE: 用户提供了证据，执行上传 ─────────────
        "DISPUTE_EVIDENCE" => format!(
            "【当前动作】提交买家仲裁证据\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 上传链下证据：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 2 — 向卖家输出 header 回复：**\n\n\
             {header_template}\n\
             买家证据已提交，等待仲裁者裁决。\n"
        ),

        // ─── 任务完成 ─────────────────────────────────────────────────
        "TASK_COMPLETED" => format!(
            "【当前状态】TASK_COMPLETED（任务完成）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             任务已完成（TASK_COMPLETED），感谢合作。\n\n\
             **Step 2 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已验收完成。\n\n\
             **Step 3 — 引导评价卖家（TODO）：**\n\
             收到 TASK_COMPLETED 时，引导买家对卖家进行评价（调用身份模块，待确认）。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 任务终止（退款 / 仲裁买家胜诉） ────────────────────────
        "TASK_REJECTED" => format!(
            "【当前状态】TASK_REJECTED（任务终止，资金退还买家）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 检查是否需要 claim：**\n\
             如果通知中 `arbitration: true`（仲裁买家胜诉），调用：\n\
             ```bash\n\
             onchainos agent claim {job_id}\n\
             ```\n\
             签名 claim calldata → 广播，退款/奖金到账。\n\n\
             **Step 2 — 向卖家输出 header 格式回复：**\n\n\
             {header_template}\n\
             任务已终止（TASK_REJECTED），资金已退还。\n\n\
             **Step 3 — 通知主 session（用户（通知））：**\n\
             任务 {job_id} 已终止，资金已返还至您的钱包。\n\n\
             【流程结束】子 session 可以关闭。\n"
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
