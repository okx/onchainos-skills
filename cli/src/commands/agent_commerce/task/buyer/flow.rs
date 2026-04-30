//! Client (买家) 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 对应 provider/flow.rs 的买家版本，让 agent 只需
//! `exec onchainos agent next-action --role buyer ...` 拿提示词直接执行。

use crate::commands::agent_commerce::task::common::state_machine::Status;

/// Buyer 在某 status 下可执行的 CLI 命令清单（用于 `agent common context` 输出末尾的菜单）。
///
/// 每个 status 列出主动作 + 一行索引指回 `next-action` 完整剧本（
/// `generate_next_action` 函数同文件，按 status 对应的 entry event 路由）。
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**下一步必做** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role buyer --agentId <agentId>`（拿当前 status 的完整剧本，**按剧本走**，不要绕过 next-action 直接调下方 CLI）")
    };
    let ref_header = "（参考·剧本里会用到的相关 CLI；不要直接调，先调 next-action 拿剧本）".to_string();
    match status {
        Status::Open => vec![
            next_action("job_created"),
            ref_header,
            format!("  onchainos agent recommend {job_id} --agent-id <agentId>  # 查看推荐卖家"),
            format!("  onchainos agent confirm-accept {job_id} --provider <addr> --payment-mode <escrow|non_escrow|x402> --token-symbol <sym> --token-amount <amt>  # 接受卖家并注资"),
            format!("  onchainos agent close {job_id}          # 关闭任务"),
            format!("  onchainos agent set-public {job_id}     # 转为公开任务"),
        ],
        Status::Accepted => vec![
            next_action("job_accepted"),
            "（被动等待）卖家执行任务中：job_submitted → 进入验收".to_string(),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            ref_header,
            format!("  onchainos agent complete {job_id}       # 验收通过，释放款项"),
            format!("  onchainos agent reject {job_id} --reason <reason>  # 拒绝验收（仅 escrow）"),
        ],
        Status::Refused => vec![
            next_action("job_refused"),
            "（被动等待）卖家 24h 内决策：job_disputed → 进入仲裁举证；confirm_refund → 退款".to_string(),
        ],
        Status::Disputed => vec![
            next_action("job_disputed"),
            ref_header,
            format!("  onchainos agent dispute upload {job_id} --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
        ],
        Status::Completed => vec![
            next_action("job_completed"),
            "（流程结束）任务完成，资金已释放。子 session 可关闭。".to_string(),
        ],
        Status::Refunded => vec![
            next_action("confirm_refund"),
            "（流程结束）退款已到账。子 session 可关闭。".to_string(),
        ],
        Status::Other(s) => vec![
            format!("当前状态 `{s}` 不在标准状态机内 → 先 `onchainos agent status {job_id}` 查最新状态"),
        ],
    }
}

/// 根据 jobStatus 生成 client/buyer 下一步动作的结构化提示词。
///
/// `job_status` 参数同时兼容 event 名（job_created / provider_applied / ...）
/// 和 status 名（open / submitted / ...），由 state_machine 统一解析。
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    eprintln!(
        "[buyer-flow] generate_next_action called: job_id={job_id}, job_status={job_status}, agent_id={agent_id}"
    );

    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md §Session 通信契约。
    // 本文件只告诉 agent **每一步把什么内容发到哪**。
    // ──────────────────────────────────────────────────────────────────────
    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md §Session 通信契约。
    // 本文件只负责告诉 agent **每一步把什么内容发到哪**，不重复解释工具用法。
    //
    // 三种通信工具：
    //   - xmtp_send：发给卖家（peer sub session）
    //   - xmtp_dispatch_user：通知用户（无需确认），参数：content
    //   - xmtp_prompt_user：需要用户交互（需确认/决策），参数：llmContent + userContent
    //     llmContent = 注入 LLM session 的指令（用户不可见）
    //     userContent = 发送给用户的可见消息
    // ──────────────────────────────────────────────────────────────────────
    let send_to_peer = format!(
        "→ 用 xmtp_send 发给卖家（机制见 SKILL.md §Session 通信契约 §1 路径 4）。\n\
         当前 sub session：jobId={job_id}，我方 agentId={agent_id}。\n\
         content（纯自然语言，不要包 markdown / 代码块）："
    );
    let header_template = &send_to_peer;

    let context_preamble = format!(
        "📍 你在 sub session（你看到这段 next-action 输出 = 100% 在 sub）。\n\n\
         🔒 **如果当前 turn 没读过 SKILL.md §Session 通信契约**（envelope 形态白名单 / xmtp_send 两步 / xmtp_dispatch_user·xmtp_prompt_user 推 user session 铁律），\n\
         **先读 `skills/okx-agent-task/SKILL.md`** 再继续——下面步骤会引用它的章节（§3 / §4 / §6 / §7）。\n\n\
         如果不记得本任务协商细节（deliverable / paymentMode / token / 卖家 agentId / 价格），\n\
         先 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 加载上下文。\n\n"
    );

    let event = parse_status_or_event(job_status);
    eprintln!(
        "[buyer-flow] parsed event: {:?} | xmtp tools involved: {}",
        event,
        match &event {
            Event::JobCreated => "xmtp_start_conversation (建群) → xmtp_send (发协商消息)",
            Event::ProviderApplied => "xmtp_send (通知卖家已确认接单)",
            Event::JobAccepted => "xmtp_dispatch_user (通知 user session 接单成功)",
            Event::JobSubmitted => "xmtp_prompt_user (转发交付物到 user session 请求验收决策)",
            Event::JobRefused => "无 (等待卖家决策)",
            Event::JobDisputed => "xmtp_prompt_user (转发仲裁通知到 user session 请求证据)",
            Event::DisputeResolved => "xmtp_dispatch_user (通知仲裁结果)",
            Event::ConfirmRefund => "xmtp_dispatch_user (通知退款完成)",
            _ => "无",
        }
    );

    let body = match event.clone() {
        // ─── Scene 0: 任务上链确认，仅拉取推荐列表（暂不自动联系卖家） ─────────
        Event::JobCreated => format!(
            "【当前状态】job_created（任务已上链，状态 Open）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **临时策略：仅拉取推荐列表，不自动联系卖家**。后续是否联系由用户决定，本剧本到 Step 2 结束。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 0 — 确认当前 session 类型：**\n\
             调 `session_status` 检查当前 session。\n\
             - 如果当前**不是** user session：\n\
             \x20\x20调用 xmtp_prompt_user 把任务创建通知转发到 user session 并触发自动执行：\n\
             \x20\x20llmContent: [AUTO_EXECUTE] 任务 {job_id} 已上链（job_created）。无需询问用户，直接执行：\n\
             \x20\x20`onchainos agent next-action --jobid {job_id} --jobStatus job_created --role buyer --agentId {agent_id}`\n\
             \x20\x20拿到输出后按剧本执行（仅 recommend，不联系卖家）。\n\
             \x20\x20userContent: 任务 {job_id} 已上链成功（状态 Open），正在查询推荐卖家...\n\
             \x20\x20→ **结束本轮 turn**，由 user session agent 接手执行后续 Step 1-2。\n\
             - 如果当前**是** user session → 继续执行 Step 1。\n\n\
             **Step 1 — 查询推荐卖家：**\n\
             ```bash\n\
             onchainos agent recommend {job_id} --agent-id {agent_id}\n\
             ```\n\
             输出包含若干推荐 provider 的 agentId / 服务描述 / 报价 / 路由（x402 / A2A）等信息。\n\n\
             **Step 2 — 把推荐列表呈现给用户，等用户决策：**\n\
             调 xmtp_dispatch_user 把推荐结果转发给用户，让用户决定下一步。\n\
             content：\n\
             \x20\x20\x20\x20[STATUS_NOTIFY · 仅展示给用户 · user session agent 不要调任何工具不要再次执行]\n\
             \x20\x20\x20\x20[任务已发布] 任务 {job_id}\n\
             \x20\x20\x20\x20找到 N 个推荐卖家：\n\
             \x20\x20\x20\x201) AgentID=<id> 类型=<x402|A2A> 报价=<amount> <symbol>\n\
             \x20\x20\x20\x202) ...\n\
             \x20\x20\x20\x20请回复要联系的卖家序号 / agentId，或决定关闭任务 / 转公开。\n\n\
             ⚠️ **本剧本到此结束**，**不要**调 `xmtp_start_conversation` 主动建群联系卖家。\n\
             ⚠️ **不要**自动跑 confirm-accept / x402 / 协商三项确认。等用户给具体指令再做。\n\n\
             【后续事件】\n\
             - 用户回复要联系的卖家 → 那时再启动联系流程（暂未在本剧本内实现）\n\
             - 用户决定关闭任务 → `onchainos agent close {job_id}`\n\
             - 用户决定转为公开 → `onchainos agent set-public {job_id}`\n"
        ),

        // ─── Scene 6: 卖家申请接单，确认接单（A2A 路径，支付方式已在协商中确定） ──────────
        Event::ProviderApplied => format!(
            "【当前状态】provider_applied（卖家已链上申请接单）\n\
             【角色】买家（Client）\n\n\
             【前置】协商阶段已确定支付方式（escrow / non_escrow）、金额、代币，从协商上下文获取：\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取协商结果：providerAgentId、paymentMode、tokenAmount、tokenSymbol。\n\
             ⚠️ tokenAmount 和 tokenSymbol 必须从协商结果获取，不是任务详情。\n\
             ⚠️ non_escrow 还需要 paymentId（卖家通过 XMTP 传递的 a2a_xxx 格式 ID）。\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 确认接单（按协商确定的支付方式，无需再询问用户）：**\n\n\
             ▸ **担保支付（escrow）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode escrow --token-symbol <tokenSymbol> --token-amount <tokenAmount>\n\
             ```\n\
             （内部：setPaymentMode(1) → providerConfirmStatus → sign_escrow TEE 签名 → accept 获取 calldata → 签名 → 广播，资金托管）\n\n\
             ▸ **非担保支付（non_escrow）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode non_escrow --payment-id <paymentId>\n\
             ```\n\
             （内部：setPaymentMode(2) → a2a_pay EIP-3009 签名 → direct/accept 获取 calldata → 签名 → 广播）\n\n\
             **Step 2 — 调用 xmtp_send 工具向卖家发送：**\n\n\
             {header_template}\n\
             已确认接单，支付方式：<paymentMode>。等待你开始执行任务。\n\n\
             【后续事件】\n\
             - job_accepted → 通知 user session 接单成功\n"
        ),

        // ─── job_accepted: 通知 user session 接单成功，等待卖家交付 ──────────────────
        Event::JobAccepted => format!(
            "【当前状态】job_accepted（买家已确认接单，任务进入执行阶段）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 获取任务完整信息：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取：title、description、deliverable、providerAgentId、paymentMode、tokenAmount、tokenSymbol。\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户接单成功：**\n\
             \x20\x20content:\n\
             \x20\x20[接单成功] 任务 {job_id} 已确认接单，进入执行阶段。\n\
             \x20\x20任务标题：<title>\n\
             \x20\x20任务描述：<description>\n\
             \x20\x20交付物：<deliverable>\n\
             \x20\x20卖家 AgentID：<providerAgentId>\n\
             \x20\x20支付方式：<paymentMode>\n\
             \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
             \x20\x20等待卖家执行并提交交付物。\n\n\
             **Step 3 — 可选：调用 xmtp_send 工具向卖家发送确认：**\n\n\
             {header_template}\n\
             接单已确认，期待你的交付。\n\n\
             【后续事件】\n\
             - job_submitted → 验收交付物\n"
        ),

        // ─── Scene 7: 卖家提交交付物，下载 + 验收（区分支付方式） ─────────
        Event::JobSubmitted => format!(
            "【当前状态】job_submitted（卖家已提交交付物）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 查询任务详情，提取交付物和支付方式：**\n\
             ```bash\n\
             onchainos agent status {job_id}\n\
             ```\n\
             提取 `deliverableUrl`、`qualityStandards` 和 `paymentMode`（int：1=escrow, 2=non_escrow, 3=x402）。\n\n\
             **Step 2 — 下载交付物文件（xmtp_file_download）：**\n\
             从卖家在 sub session 中发送的交付物消息里提取加密元数据，调用 xmtp_file_download 工具：\n\
             \x20\x20参数：\n\
             \x20\x20- fileKey：卖家上传时返回的 fileKey\n\
             \x20\x20- agentId：{agent_id}（买家 agentId）\n\
             \x20\x20- digest：SHA-256 digest（hex）\n\
             \x20\x20- salt：加密 salt（base64）\n\
             \x20\x20- nonce：加密 nonce（base64）\n\
             \x20\x20- secret：加密 secret（base64）\n\
             \x20\x20- filename：（可选）保存文件名\n\
             ⚠️ 调用前输出：`[buyer-xmtp] xmtp_file_download: fileKey=<fileKey>, agentId={agent_id}`\n\
             ⚠️ 调用后输出：`[buyer-xmtp] xmtp_file_download result: localPath=<返回的本地路径>`\n\n\
             下载成功后记录 localPath，后续展示给用户。\n\
             如果下载失败 → 用 deliverableUrl 作为备用展示信息。\n\n\
             **Step 3 — 调用 xmtp_prompt_user 把交付物和验收决策请求推到 user session：**\n\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey。\n\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}] \
             用户回复「验收通过」→ relay 回 sub session 执行 onchainos agent complete；\
             回复「拒绝，原因是...」→ relay 回 sub session 执行 onchainos agent reject（仅 escrow 可拒绝）。\
             禁止 user session agent 自己执行 task CLI。\n\
             \x20\x20\x20\x20userContent:\n\
             \x20\x20\x20\x20任务 {job_id} 卖家已提交交付物，已下载到本地。\n\
             \x20\x20\x20\x20交付物本地路径：<localPath>（如下载失败则显示 deliverableUrl）\n\
             \x20\x20\x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20\x20\x20验收标准：<qualityStandards>\n\
             \x20\x20\x20\x20支付方式：<paymentMode>（escrow=担保 / non_escrow=非担保）\n\
             \x20\x20\x20\x20请选择：\n\
             \x20\x20\x20\x201. 验收通过 → 回复「验收通过」\n\
             \x20\x20\x20\x202. 拒绝（仅担保支付 escrow 可拒绝）→ 回复「拒绝，原因是<原因>」\n\n\
             **Step 4 — 等用户回复 relay 回来**，按用户决策 + 支付方式分支执行：\n\
             收到 `[USER_DECISION_RELAY] 用户决策：...` 后，按关键词执行：\n\n\
             ━━━━━━━━━ 分支 A：用户验收通过 ━━━━━━━━━\n\n\
             ▸ **担保支付（escrow, paymentMode=1）— 双签流程：**\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             内部流程：\n\
             \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-complete（712 标准，非 uop）→ 获取 digest\n\
             \x20\x202. ED25519 签名 digest → signature\n\
             \x20\x203. POST /priapi/v1/aieco/task/{job_id}/complete（body: {{\"signature\": \"<sig>\"}}）→ 获取 uopData\n\
             \x20\x204. 签名 uopHash → 广播上链\n\
             \x20\x20→ 任务状态变为 Complete，资金从合约释放给卖家。\n\n\
             ▸ **非担保支付（non_escrow, paymentMode=2）— 单签流程：**\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             内部流程：\n\
             \x20\x201. POST /priapi/v1/aieco/task/{job_id}/direct/complete → 获取 uopData\n\
             \x20\x202. 签名 uopHash → 广播上链\n\
             \x20\x20→ 任务状态变为 Complete（买家后续需手动转账给卖家）。\n\n\
             ━━━━━━━━━ 分支 B：用户拒绝交付物（仅 escrow 担保支付可拒绝）━━━━━━━━━\n\n\
             ⚠️ 非担保支付（non_escrow）不支持拒绝，如果用户要求拒绝 non_escrow 任务，\n\
             通知用户：「非担保支付任务不支持拒绝交付物，只能验收通过。」\n\n\
             ▸ **担保支付（escrow）拒绝 — 双签流程：**\n\
             ```bash\n\
             onchainos agent reject {job_id} --reason \"<用户提供的拒绝原因>\"\n\
             ```\n\
             内部流程：\n\
             \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-refuse（712 标准，非 uop）→ 获取 digest\n\
             \x20\x202. ED25519 签名 digest → signature\n\
             \x20\x203. POST /priapi/v1/aieco/task/{job_id}/refuse（body: {{\"signature\": \"<sig>\", \"reason\": \"<reason>\"}}）→ 获取 uopData\n\
             \x20\x204. 签名 uopHash → 广播上链\n\
             \x20\x20→ 任务状态变为 Refused，卖家 24h 内可发起仲裁。\n\n\
             跑完 Step 4 → **结束本轮 turn**，等系统通知。\n\n\
             【后续事件】\n\
             - job_completed → 任务完成（Scene 8）\n\
             - job_refused → 等待卖家决定：仲裁（job_disputed）或退款（confirm_refund）（仅 escrow）\n"
        ),

        // ─── job_refused: 买家已拒绝，等待卖家决策 ─────────────────
        Event::JobRefused => format!(
            "【当前状态】job_refused（买家已拒绝交付物，等待卖家决定）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             无需执行 CLI 命令。卖家有 24h 决定：\n\
             - 发起仲裁 → 你将收到 job_disputed\n\
             - 同意退款 → 你将收到 confirm_refund\n\
             - 24h 超时 → 系统自动退款，你将收到 confirm_refund\n\n\
             调用 xmtp_send 工具向卖家发送：\n\n\
             {header_template}\n\
             交付物已拒绝，等待你的后续处理。\n\n\
             【后续事件】\n\
             - job_disputed → 提交买家证据（Scene 6）\n\
             - confirm_refund → 退款完成\n"
        ),

        // ─── Scene 6: 仲裁已发起，提交买家证据 ─────────────────────
        Event::JobDisputed => format!(
            "【当前状态】job_disputed（仲裁已发起，进入 1 小时证据准备期）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **证据内容必须由用户决策**——sub agent 不知道用户手上有什么证据（截图、聊天记录、交付物链接等），\n\
             不要凭空编造证据摘要直接调 `dispute upload`。**先把决策请求推到 user session 让用户拍板**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 向卖家发一条状态告知（用 `xmtp_send` 工具）：**\n\n\
             {header_template}\n\
             仲裁已上链（job_disputed），正在准备证据材料。\n\n\
             **Step 2 — 调用 xmtp_prompt_user 把证据决策请求推到 user session 让用户提供内容：**\n\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey。\n\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}] \
             用户回复证据后，relay 回 sub session 执行 onchainos agent dispute upload。禁止 user session agent 自己执行 task CLI。1 小时内必须提交。\n\
             \x20\x20\x20\x20userContent:\n\
             \x20\x20\x20\x20任务 {job_id} 仲裁已上链，需要在 1 小时内提交链下证据。请提供：\n\
             \x20\x20\x20\x20- 文字摘要（必填）：说明交付物不达标的关键证据点\n\
             \x20\x20\x20\x20- 图片路径（可选）：截图、聊天记录等本地文件路径\n\
             \x20\x20\x20\x20回复格式示例：『证据：交付物缺少 X/Y/Z；图片：/path/to/screenshot.png』\n\n\
             **Step 3 — 等用户回复 relay 回来**：收到 `[USER_DECISION_RELAY] 用户证据：...` 后，调 `next-action --jobStatus dispute_evidence` 拿上传剧本。\n\n\
             ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
             跑完 Step 1-2 → **结束本轮 turn**，等用户回复。\n"
        ),

        // ─── dispute_evidence: 用户提供了证据，执行上传（伪 event）─────
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】上传仲裁证据\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 从 relay 进来的用户消息中提取证据内容：**\n\
             - 文字摘要 → `--text` 参数\n\
             - 图片路径（如果用户提供了）→ `--image` 参数\n\
             text 和 image **至少一项**。\n\n\
             **Step 2 — 调用 CLI 上传证据（上链）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<用户提供的文字摘要>\" --image <用户提供的图片路径或省略>\n\
             ```\n\
             text 和 image **至少一项**；图片可省略整个 `--image` 段，不要给空字符串。\n\n\
             **Step 3 — 调用 `xmtp_send` 工具向卖家发送：**\n\n\
             {header_template}\n\
             证据已提交，等待仲裁员裁决。\n\n\
             【后续事件】\n\
             - job_completed → 仲裁卖家胜诉，任务完成\n\
             - confirm_refund → 仲裁买家胜诉，退款\n\n\
             跑完 Step 1-3 → **结束本轮 turn，不要 xmtp_dispatch_user / xmtp_prompt_user 推 main**。\n"
        ),

        // ─── 任务完成 ─────────────────────────────────────────────────
        Event::JobCompleted => format!(
            "【当前状态】job_completed（任务完成，资金已释放给卖家）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 给卖家发完成致谢**：\n\n\
             {header_template}\n\
             任务已完成（job_completed），感谢合作。\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 告知用户任务完成：**\n\n\
             从 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             content：\n\
             \x20\x20\x20\x20[任务完成] 任务 {job_id}（<title>）已验收通过，资金已释放给卖家。\n\
             \x20\x20\x20\x20  - 支出：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 3 — 评价卖家：**\n\
             ```bash\n\
             onchainos agent judge {job_id}\n\
             ```\n\n\
             **Step 4 — 关闭 sub session**（终态收尾，机制见 SKILL.md §Session 通信契约 §6 路径 5）：\n\
             1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段\n\
             2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串\n\
             删除后本 sub session 不再接收任何消息——任务完整结束。\n"
        ),

        // ─── 仲裁结束（DisputeSettled） ─────────────────────────────
        Event::DisputeResolved => format!(
            "【当前状态】dispute_resolved（仲裁已裁决）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **判定胜负**：从你刚收到的系统通知 envelope 里读 `message.jobStatus` 字段：\n\
             - `jobStatus = \"rejected\"` → **你（buyer）胜诉**，资金已退还给你\n\
             - `jobStatus = \"complete\"` → **你（buyer）败诉**，资金已释放给卖家\n\
             （另有 `message.winner` 字段冗余可对照：`buyer`=你赢；`provider`=对方赢）\n\n\
             【你的下一步动作（按胜负分流）】\n\n\
             ━━━━━━━━━━━━━ 分支 A：jobStatus=rejected（买家胜诉）━━━━━━━━━━━━━\n\n\
             **A-Step 1 — 领取退款：**\n\
             ```bash\n\
             onchainos agent claim {job_id}\n\
             ```\n\
             签名 claim calldata → 广播，退款到账。\n\n\
             **A-Step 2 — 给卖家发结果**（用 `xmtp_send`）：\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），裁决支持买方。资金已退还。\n\n\
             **A-Step 3 — 调用 xmtp_dispatch_user 通知用户仲裁结果：**\n\n\
             从 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             content：\n\
             \x20\x20\x20\x20[仲裁胜诉] 任务 {job_id}（<title>）仲裁完成，买方胜诉。\n\
             \x20\x20\x20\x20  - 退款：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=rejected）\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             ━━━━━━━━━━━━━ 分支 B：jobStatus=complete（买家败诉）━━━━━━━━━━━━━\n\n\
             **B-Step 1 — 给卖家发结果**（用 `xmtp_send`）：\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），裁决支持卖方。资金已释放给卖家。\n\n\
             **B-Step 2 — 调用 xmtp_dispatch_user 通知用户仲裁结果：**\n\n\
             从 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             content：\n\
             \x20\x20\x20\x20[仲裁败诉] 任务 {job_id}（<title>）仲裁完成，卖方胜诉。\n\
             \x20\x20\x20\x20  - 损失：<tokenAmount> <tokenSymbol>（资金已释放给卖家）\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=complete）\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
             **Step 3（两个分支都要做）— 关闭 sub session**（终态收尾，机制见 SKILL.md §Session 通信契约 §6 路径 5）：\n\
             1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段\n\
             2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串\n\
             删除后本 sub session 不再接收任何消息——仲裁流程完整结束。\n"
        ),

        // ─── 卖家同意退款（TODO: 后端尚未定义此 event，暂用 confirm_refund）
        Event::ConfirmRefund => format!(
            "【当前状态】confirm_refund（卖家同意退款，资金退还买家）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 给卖家发收尾**：\n\n\
             {header_template}\n\
             卖家已同意退款（confirm_refund），资金已退还。\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户退款完成：**\n\n\
             content：\n\
             \x20\x20\x20\x20[退款完成] 任务 {job_id} 卖家已同意退款，资金已返还至您的钱包。\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 3 — 关闭 sub session**（终态收尾，机制见 SKILL.md §Session 通信契约 §6 路径 5）：\n\
             1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段\n\
             2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串\n\
             删除后本 sub session 不再接收任何消息——退款流程完整结束。\n"
        ),

        // ─── 任务超时（OPEN→EXPIRED 或 ACCEPTED→EXPIRED）──────────
        Event::JobExpired => format!(
            "【当前状态】job_expired（任务超时，无人接单或卖家未提交）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_prompt_user 请求用户确认是否关闭：**\n\
             \x20\x20llmContent: 用户确认后执行 onchainos agent close {job_id} 关闭任务回收资金；用户拒绝则不操作。\n\
             \x20\x20userContent: 任务 {job_id} 已超时（accept 截止前未接单 或 submit 截止前未提交），是否关闭任务回收资金？\n\n\
             **Step 2 — 用户确认后，关闭任务回收资金：**\n\
             ```bash\n\
             onchainos agent close {job_id}\n\
             ```\n\n\
             【后续事件】\n\
             - job_closed → 关闭完成，资金已回收\n"
        ),

        // ─── 任务已关闭（close tx 结果）─────────────────────────────
        Event::JobClosed => format!(
            "【当前状态】job_closed（任务已关闭）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户：**\n\
             \x20\x20content: 任务 {job_id} 已关闭，资金已回收。\n\n\
             检查 payload 中 status 字段：\n\
             - success → 任务已关闭\n\
             - failed → 关闭失败，按 errorCode 重试\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 卖家主动联系买家（public 任务，卖家找到任务后发起会话）─────
        // 触发方式：user session 收到自然语言消息（如"有N个卖家待沟通"），
        // 不再依赖 provider_conversation 系统通知。
        Event::Other(ref s) if s == "provider_conversation" => format!(
            "【触发】收到「有卖家待沟通」类消息（user session 侧）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序，循环遍历 pending list）】\n\n\
             **Step 1 — 获取待沟通卖家列表：**\n\
             调用 xmtp_get_pending_list 工具获取待沟通卖家列表。\n\
             ⚠️ 调用前输出：`[buyer-xmtp] xmtp_get_pending_list`\n\
             ⚠️ 调用后输出：`[buyer-xmtp] xmtp_get_pending_list result: <返回值>`\n\n\
             如果返回空列表 → 通知用户「当前没有待沟通的卖家」，结束。\n\n\
             **Step 2 — 调用 xmtp_prompt_user 请求用户确认是否与第一个卖家协商：**\n\
             从 pending list 第一个卖家提取信息，展示给用户：\n\
             \x20\x20llmContent: 用户接受 → 执行 xmtp_start_conversation 建群并开始协商（A 分支）；用户拒绝 → 调用 xmtp_deny_pending_conversation 拒绝后尝试下一个卖家（B 分支）。\n\
             \x20\x20userContent:\n\
             \x20\x20有卖家申请做你的任务，是否接受协商？\n\
             \x20\x20- 任务 JobId：{job_id}\n\
             \x20\x20- 任务标题：<pending list 中的 job title>\n\
             \x20\x20- 卖家 AgentID：<第一个卖家的 agentId>\n\
             \x20\x20- 卖家名称：<第一个卖家的 name>\n\
             \x20\x20- 信用分：<第一个卖家的 creditScore>\n\
             \x20\x20- 完成任务数：<第一个卖家的 completedTaskCount>（如有）\n\
             \x20\x20请回复「接受」开始协商，或「拒绝」跳过此卖家。\n\n\
             **Step 3 — 等待用户回复，按用户决策分支：**\n\n\
             ━━━━━━━━━ 分支 A：用户接受 → 与卖家协商 ━━━━━━━━━\n\n\
             A-Step 1：调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<当前卖家的 agentId>，jobId={job_id}\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             A-Step 2：建群后已进入 sub session，调用 xmtp_send 向卖家发起协商（参照 buyer.md §2.1 协商剧本三步确认）：\n\
             \x20\x20⚠️ **禁止**用 xmtp_dispatch_user / xmtp_dispatch_session，建群后统一用 xmtp_send。\n\
             \x20\x20content: 你好，我有一个任务（jobId: {job_id}）想请你来完成，请问你感兴趣吗？\n\n\
             A-Step 3：协商成功 → 卖家 apply 上链 → 等待 provider_applied 事件（进入场景 6）\n\n\
             A-Step 4：协商失败（卖家拒绝 / 超时 / 条件不一致）→ 跳到 B 分支。\n\n\
             ━━━━━━━━━ 分支 B：用户拒绝 / 协商失败 → 拒绝当前卖家，尝试下一个 ━━━━━━━━━\n\n\
             B-Step 1：调用 xmtp_deny_pending_conversation 拒绝当前卖家：\n\
             \x20\x20参数：agentId=<当前卖家的 agentId>，jobId={job_id}\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_deny_pending_conversation: agentId=<agentId>, jobId={job_id}`\n\n\
             B-Step 2：重新调用 xmtp_get_pending_list 获取最新待沟通列表。\n\n\
             B-Step 3：如果列表不为空 → 回到 Step 2，用列表第一个卖家提示用户确认。\n\n\
             B-Step 4：如果列表为空 → 调用 xmtp_dispatch_user 通知用户：\n\
             \x20\x20content: 任务 {job_id} 当前没有更多待沟通卖家，建议等待新卖家联系或调整任务描述。\n\n\
             【循环结束条件】xmtp_get_pending_list 返回空列表 或 协商成功进入场景 6。\n"
        ),

        // ─── 可见性切换结果（setVisibility tx 结果）───────────────────
        Event::JobVisibilityChanged => format!(
            "【当前状态】job_visibility_changed（公开/私有切换已上链）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             检查 payload 中 status 字段：\n\
             - success → 公开/私有切换已生效\n\
             - failed → 切换失败，按 errorCode 重试\n\n\
             **通知用户：**\n\
             调用 xmtp_dispatch_user：\n\
             content: \"任务 {job_id} 可见性已更新。\"\n"
        ),

        // ─── 支付模式切换结果（setPaymentMode tx 结果）────────────────
        Event::JobPaymentModeChanged => format!(
            "【当前状态】job_payment_mode_changed（支付模式切换已上链）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             检查 payload 中 status 字段：\n\
             - success → 支付模式已切换\n\
             - failed → 切换失败，按 errorCode 重试\n\n\
             **通知用户：**\n\
             调用 xmtp_dispatch_user：\n\
             content: \"任务 {job_id} 支付模式已更新。\"\n"
        ),

        // ─── 关闭任务（仅 Open 状态可用，user-instruction 伪 event）─────
        Event::Other(ref s) if s == "close" => format!(
            "【当前动作】关闭任务\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 关闭任务（仅 Open 状态有效）：**\n\
             ```bash\n\
             onchainos agent close {job_id}\n\
             ```\n\n\
             **Step 2 — 通知用户：**\n\
             调用 xmtp_dispatch_user：\n\
             content: \"任务 {job_id} 已关闭。\"\n"
        ),

        // ─── 设为公开任务（user-instruction 伪 event）────────────────
        Event::Other(ref s) if s == "set_public" => format!(
            "【当前动作】转为公开任务\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 转为公开任务：**\n\
             ```bash\n\
             onchainos agent set-public {job_id}\n\
             ```\n\n\
             **Step 2 — 通知用户：**\n\
             调用 xmtp_dispatch_user：\n\
             content: \"任务 {job_id} 已转为公开任务，等待卖家主动申请。\"\n"
        ),

        // ─── 卖家未提交交付物超时 ─────────────────────────────────────
        Event::SubmitExpired => format!(
            "【系统通知】卖家提交交付物超时\n\
             【角色】买家（Client）\n\n\
             卖家未在规定期限内提交交付物，你可以申请自动退款。\n\n\
             **Step 1 — 调用 xmtp_prompt_user 请求用户确认：**\n\
             \x20\x20llmContent: 用户确认后执行 onchainos agent claim-auto-refund {job_id}；用户拒绝则不操作。\n\
             \x20\x20userContent: 任务 {job_id} 的卖家未在截止时间前提交交付物，是否申请自动退款？\n\n\
             **Step 2 — 用户确认后，领取自动退款：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 3 — 调用 xmtp_dispatch_user 通知用户：**\n\
             content: \"任务 {job_id} 已申请自动退款，资金将退回你的账户。\"\n"
        ),

        // ─── 买家拒绝后卖家仲裁超时 ─────────────────────────────────
        Event::RefuseExpired => format!(
            "【系统通知】卖家仲裁超时\n\
             【角色】买家（Client）\n\n\
             你拒绝交付物后，卖家未在规定期限内发起仲裁，你可以申请自动退款。\n\n\
             **Step 1 — 调用 xmtp_prompt_user 请求用户确认：**\n\
             \x20\x20llmContent: 用户确认后执行 onchainos agent claim-auto-refund {job_id}；用户拒绝则不操作。\n\
             \x20\x20userContent: 任务 {job_id} 的卖家在你拒绝交付物后未及时发起仲裁，是否申请自动退款？\n\n\
             **Step 2 — 用户确认后，领取自动退款：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 3 — 调用 xmtp_dispatch_user 通知用户：**\n\
             content: \"任务 {job_id} 已申请自动退款，资金将退回你的账户。\"\n"
        ),

        // ─── buyer 自己的截止提醒 ─────────────────────────────────────
        Event::ReviewDeadlineWarn => format!(
            "【系统通知】review_deadline_warn（验收截止时间快到了）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             如果对交付物满意，立即调：\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             如果不满意，调：\n\
             ```bash\n\
             onchainos agent reject {job_id} --reason \"<原因>\"\n\
             ```\n\
             超时后 provider 可调 claimAutoComplete 自动通过。\n"
        ),

        // ─── review_expired: 买家未在期限内验收，provider 已 claimAutoComplete ─────
        Event::ReviewExpired => "【系统通知】review_expired（验收超时，provider 已自动 complete）\n\
             【角色】买家（Client）\n\n\
             【建议】task 已自动进入 completed 状态，资金已释放给 provider。\n\
             子 session 可关闭。\n".to_string(),

        // ─── provider 的截止提醒 — buyer 端无关 ────────────────────────
        Event::SubmitDeadlineWarn => "【系统通知】submit_deadline_warn（provider 端截止提醒）\n\
             【角色】买家（Client）\n\n\
             【建议】静默观察即可，等 provider 提交交付物（job_submitted 通知）后再处理。\n".to_string(),

        // ─── 仲裁子状态机事件 — buyer 在 disputed 状态下关心 dispute_resolved（已有专门 arm）─────
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed => format!(
            "【系统通知】{event}（仲裁内部事件，evaluator 处理）\n\
             【角色】买家（Client）\n\n\
             【建议】静默观察即可。等 `dispute_resolved` 通知到达后再 next-action 处理收尾。\n",
            event = event.as_str()
        ),

        // ─── dispute_approved — provider 仲裁阶段 1，buyer 无关 ─────
        Event::DisputeApproved => format!(
            "【系统通知】dispute_approved（provider 已上链仲裁阶段 1 approve，buyer 无关）\n\
             【建议】静默观察即可。等 `job_disputed` 通知到达再 next-action 进入证据准备期。\n"
        ),

        // ─── 质押 / 罚没 lifecycle — buyer 不是 evaluator 时无关 ─────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed => format!(
            "【系统通知】{event}（evaluator 质押 lifecycle，buyer 无关）\n\
             【建议】忽略即可。\n",
            event = event.as_str()
        ),

        // ─── reward_claimed: buyer 自己的 claim tx 回执（仲裁胜诉退款等） ─────
        Event::RewardClaimed => format!(
            "【系统通知】reward_claimed（claimRewards tx 回执）\n\
             【角色】买家（Client）\n\n\
             【建议】从 payload 提取 status / amount / txHash。\n\
             - success → 退款/奖励已到账\n\
             - failed → 按 errorCode 重试 `onchainos agent claim {job_id}`\n"
        ),

        // ─── 未知类型兜底 ───────────────────────────────────────────
        Event::Other(other) => format!(
            "【未知状态】{other}\n\
             【建议】\n\
             1. 调用 `onchainos agent common context {job_id} --role buyer` 查看完整上下文\n\
             2. 如该状态不在预期流程内，等待用户指示\n\
             3. 不要预测/假设其他通知\n"
        ),
    };

    let result = format!("{context_preamble}{body}");
    eprintln!(
        "[buyer-flow] output length: {} chars | first 200: {}",
        result.len(),
        &result[..result.len().min(200)]
    );
    result
}
