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
    let next_action_hint = |evt: &str| {
        format!("onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role buyer --agentId <agentId>  # 完整剧本")
    };
    match status {
        Status::Open => vec![
            format!("onchainos agent recommend {job_id}      # 查看推荐卖家"),
            format!("onchainos agent confirm-accept {job_id} --provider <addr> --payment-mode <escrow|non_escrow|x402>  # 接受卖家并注资"),
            format!("onchainos agent close {job_id}          # 关闭任务"),
            format!("onchainos agent set-public {job_id}     # 转为公开任务"),
            next_action_hint("job_created"),
        ],
        Status::Accepted => vec![
            "（被动等待）卖家执行任务中：job_submitted → 进入验收".to_string(),
            next_action_hint("job_accepted"),
        ],
        Status::Submitted => vec![
            format!("onchainos agent complete {job_id}       # 验收通过，释放款项"),
            format!("onchainos agent reject {job_id} --reason <reason>  # 拒绝验收（仅 escrow）"),
            next_action_hint("job_submitted"),
        ],
        Status::Refused => vec![
            "（被动等待）卖家 24h 内决策：job_disputed → 进入仲裁举证；confirm_refund → 退款".to_string(),
            next_action_hint("job_refused"),
        ],
        Status::Disputed => vec![
            format!("onchainos agent dispute upload {job_id} --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
            next_action_hint("job_disputed"),
        ],
        Status::Completed => vec![
            format!("onchainos agent judge {job_id}          # 评价卖家"),
            "（流程结束）任务完成。子 session 可关闭。".to_string(),
            next_action_hint("job_completed"),
        ],
        Status::Refunded => vec![
            "（流程结束）退款已到账。子 session 可关闭。".to_string(),
            next_action_hint("confirm_refund"),
        ],
        Status::Other(s) => vec![
            format!("onchainos agent status {job_id}         # 当前状态 `{s}` 不在标准状态机内，先查最新状态"),
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
    let send_to_peer = format!(
        "→ 调用 xmtp_send 工具发给卖家。\n\
         \x20\x20参数：sessionKey=<当前 sub session 的 sessionKey>，content=<下方内容>\n\
         \x20\x20（content 为纯自然语言，不要包 markdown / 代码块）\n\
         \x20\x20当前 sub session：jobId={job_id}，我方 agentId={agent_id}。\n\
         content："
    );
    let header_template = &send_to_peer;

    let context_preamble = format!(
        "📍 你在 sub session（你看到这段 next-action 输出 = 100% 在 sub）。\n\
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
            Event::JobAccepted => "xmtp_send (可选，确认消息)",
            Event::JobSubmitted => "xmtp_dispatch_session (转发交付物到 user session)",
            Event::JobRefused => "无 (等待卖家决策)",
            Event::JobDisputed => "xmtp_dispatch_session (转发仲裁通知到 user session)",
            Event::DisputeResolved => "无 (查看仲裁结果)",
            Event::ConfirmRefund => "无 (退款确认)",
            _ => "无",
        }
    );

    let body = match event.clone() {
        // ─── Scene 0: 任务上链确认，查询推荐卖家并按支付方式路由 ────────────────
        Event::JobCreated => format!(
            "【当前状态】job_created（任务已上链，状态 Open）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序，不询问用户，全自动执行）】\n\n\
             **Step 1 — 查询推荐卖家：**\n\
             ```bash\n\
             onchainos agent recommend {job_id}\n\
             ```\n\
             缓存完整推荐列表，记录 currentProviderIndex = 0。\n\
             输出末尾有「路由」指引，标明当前卖家是 x402 还是 A2A。\n\n\
             **Step 2 — 顺序遍历推荐列表，按 supportA2MCP 字段路由：**\n\n\
             ━━━━━━━━━ 分支 A：supportA2MCP=true → x402（无需协商，直接接单）━━━━━━━━━\n\n\
             从 recommend 输出中提取当前 provider 的 services[0]：feeAmount、feeTokenSymbol、endpoint。\n\
             从任务详情提取：tokenAmount（任务预算）、tokenSymbol（任务代币）。\n\n\
             **A-Step 1 — 价格 & 代币比较：**\n\
             - 任务预算 >= feeAmount 且 tokenSymbol 与 feeTokenSymbol 一致\n\
             \x20\x20→ 无需用户确认，直接执行 A-Step 2\n\
             - 任务预算 < feeAmount 或代币不一致\n\
             \x20\x20→ 调用 xmtp_dispatch_session 通知 user session 请求确认（省略 sessionKey = 发到 main session）：\n\
             \x20\x20\x20\x20content: 任务 {job_id} 匹配到 x402 卖家（AgentID=<providerAgentId>），服务费用 <feeAmount> <feeTokenSymbol>，\
             与任务预算（<tokenAmount> <tokenSymbol>）不一致，是否确认使用该卖家？\n\
             \x20\x20→ 用户确认 → 执行 A-Step 2\n\
             \x20\x20→ 用户拒绝 → `onchainos agent recommend {job_id} --next` 切换下一个卖家，重新回到 Step 2 路由判断\n\n\
             **A-Step 2 — 买家 accept（x402 三步）：**\n\
             1. 设置支付方式为 x402：\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode x402 \
             --token-symbol <feeTokenSymbol> --token-amount <feeAmount> --endpoint <endpoint>\n\
             ```\n\
             （命令内部自动执行：setPaymentMode(2) → direct/accept 签名广播 → x402 支付）\n\n\
             2. 完成后任务状态 → accepted。\n\n\
             **A-Step 3 — 调用 xmtp_dispatch_session 通知 user session 结果**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 任务 {job_id} 已通过 x402 自动接单。卖家 AgentID=<providerAgentId>，\
             费用=<feeAmount> <feeTokenSymbol>。等待任务执行。\n\n\
             ━━━━━━━━━ 分支 B：supportA2MCP=false → A2A（需协商）━━━━━━━━━\n\n\
             **B-Step 1 — 建群：**\n\
             调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<recommend 输出的 providerAgentId>，jobId={job_id}\n\
             \x20\x20成功返回 sessionKey + xmtpGroupId。\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<providerAgentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             **B-Step 2 — 自动协商（买家 Agent ↔ 卖家 Agent 在 sub session 中多轮交互）：**\n\
             协商目标：就以下结构化字段达成一致——\n\
             \x20\x20- deliverable：交付物描述（具体要做什么）\n\
             \x20\x20- qualityStandards：验收标准\n\
             \x20\x20- paymentMode：支付方式（escrow 或 non_escrow）\n\
             \x20\x20- tokenSymbol：支付代币\n\
             \x20\x20- tokenAmount：支付金额\n\
             \x20\x20- deadline：交付截止时间\n\n\
             ⏱ 超时规则：每轮等待卖家回复最多 5 分钟。超时未回复 → 结束当前 sub session，切换下一个卖家。\n\n\
             协商步骤（通过 xmtp_send 多轮消息）：\n\
             1. 买家发送任务详情（描述、预算、期望交付物）→ 等待卖家回复（5 分钟超时）\n\
             2. 卖家回复报价（金额、代币、支付方式偏好、预计交付时间）\n\
             3. 双方就价格/条件进行调整（可能多轮，每轮 5 分钟超时）\n\
             4. 达成一致后，买家发送结构化确认消息：\n\
             {header_template}\n\
             [协商确认] 请确认以下协商结果：\n\
             任务：{job_id}\n\
             交付物：<deliverable>\n\
             验收标准：<qualityStandards>\n\
             支付方式：<escrow/non_escrow>\n\
             金额：<tokenAmount> <tokenSymbol>\n\
             交付截止：<deadline>\n\
             如确认无误，请执行 apply 接单。\n\n\
             5. 卖家确认一致 → 卖家执行 apply 上链（`onchainos agent apply`）\n\
             6. 系统通知 provider_applied → 进入 ProviderApplied 事件处理\n\n\
             ⚠️ 任一步骤卖家 5 分钟未回复 → 视为协商失败，结束当前 sub session，执行「切换下一个卖家」。\n\n\
             **B-Step 3 — 通知 user session 协商进展**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 已自动联系推荐卖家（<providerAgentId>），进入协商流程，等待对方回复。\n\n\
             ━━━━━━━━━ 遍历结束 / 切换下一个卖家 ━━━━━━━━━\n\n\
             当前卖家超时未回复（5 分钟）或协商失败 → 结束当前 sub session → `onchainos agent recommend {job_id} --next` 切换下一个卖家，重新回到 Step 2 路由判断。\n\
             推荐列表全部遍历完 → 调用 xmtp_dispatch_session 通知 user session：\n\
             \x20\x20content: 任务 {job_id} 推荐卖家已全部遍历，无合适匹配。建议：调整任务描述或转为公开任务。\n\n\
             【后续事件】\n\
             - x402 → confirm-accept 完成后等待 job_accepted\n\
             - A2A → 协商完成 → 卖家 apply → provider_applied → 买家 confirm-accept → job_accepted\n"
        ),

        // ─── Scene 6: 卖家申请接单，确认接单（A2A 路径，支付方式已在协商中确定） ──────────
        Event::ProviderApplied => format!(
            "【当前状态】provider_applied（卖家已链上申请接单）\n\
             【角色】买家（Client）\n\n\
             【前置】协商阶段已确定支付方式（escrow / non_escrow），从协商上下文获取：\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取协商结果：providerAgentId、paymentMode、tokenAmount、tokenSymbol。\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 确认接单（按协商确定的支付方式，无需再询问用户）：**\n\n\
             ▸ **担保支付（escrow）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode escrow\n\
             ```\n\
             （内部：setPaymentMode(0) → pre-accept 获取 digest → 签名 → accept 提交签名 → 签 uopHash → 广播，资金托管）\n\n\
             ▸ **非担保支付（non_escrow）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode non_escrow\n\
             ```\n\
             （内部：setPaymentMode(1) → direct/accept → 签 uopHash → 广播，无资金托管）\n\n\
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
             **Step 2 — 调用 xmtp_dispatch_session 通知 user session 接单成功**（省略 sessionKey = 发到 main session）：\n\
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

        // ─── Scene 7: 卖家提交交付物，验收（区分支付方式） ─────────────
        Event::JobSubmitted => format!(
            "【当前状态】job_submitted（卖家已提交交付物）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 查询交付物详情：**\n\
             ```bash\n\
             onchainos agent status {job_id}\n\
             ```\n\
             提取 `deliverableUrl`、`qualityStandards` 和 `paymentMode`。\n\n\
             **Step 2 — 调用 xmtp_dispatch_session 通知 user session 请求用户决策**（省略 sessionKey = 发到 main session，**必须等待回复后再执行 Step 3**）：\n\
             \x20\x20content: [交付物验收] 任务 {job_id} 卖家已提交交付物。交付物地址：<deliverableUrl>。验收标准：<qualityStandards>。请确认：接受（验收通过）还是拒绝（不达标）？\n\n\
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
            "【当前状态】job_disputed（仲裁已发起）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 调用 xmtp_dispatch_session 通知 user session 请求用户提供证据**（省略 sessionKey = 发到 main session，**等待回复**）：\n\
             \x20\x20content: [仲裁通知] 任务 {job_id} 卖家已发起仲裁，需要提交证据。请提供：1. 证据摘要（文字描述问题）2. 证据文件（截图/文档，可选）\n\n\
             **Step 2 — 用户提供证据后，上传链下证据（买卖双方共用，自动识别角色）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 3 — 调用 xmtp_send 工具向卖家发送确认：**\n\n\
             {header_template}\n\
             仲裁已发起（job_disputed），买家证据已提交，等待仲裁者裁决。\n\n\
             【后续事件】\n\
             - job_completed → 仲裁卖家胜诉，任务完成\n\
             - confirm_refund → 仲裁买家胜诉，退款\n"
        ),

        // ─── dispute_evidence: 用户提供了证据，执行上传（伪 event）─────
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】提交买家仲裁证据\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 上传链下证据（买卖双方共用，自动识别角色）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<证据摘要>\" --image <图片路径>\n\
             ```\n\
             仅 1 小时准备期内有效，text 和 image 至少一项。\n\n\
             **Step 2 — 调用 xmtp_send 工具向卖家发送：**\n\n\
             {header_template}\n\
             买家证据已提交，等待仲裁者裁决。\n"
        ),

        // ─── 任务完成 ─────────────────────────────────────────────────
        Event::JobCompleted => format!(
            "【当前状态】job_completed（任务完成）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_send 工具向卖家发送：**\n\n\
             {header_template}\n\
             任务已完成（job_completed），感谢合作。\n\n\
             **Step 2 — 调用 xmtp_dispatch_session 通知 user session**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 任务 {job_id} 已验收完成。\n\n\
             **Step 3 — 评价卖家：**\n\
             ```bash\n\
             onchainos agent judge {job_id}\n\
             ```\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 仲裁结束（DisputeSettled） ─────────────────────────────
        Event::DisputeResolved => format!(
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
             **Step 3 — 调用 xmtp_send 工具向卖家发送：**\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），资金已处理。\n\n\
             **Step 4 — 调用 xmtp_dispatch_session 通知 user session**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 任务 {job_id} 仲裁已结束，请检查钱包余额。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 卖家同意退款（TODO: 后端尚未定义此 event，暂用 confirm_refund）
        Event::ConfirmRefund => format!(
            "【当前状态】confirm_refund（卖家同意退款，任务终止）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_send 工具向卖家发送：**\n\n\
             {header_template}\n\
             卖家已同意退款（confirm_refund），资金已退还。\n\n\
             **Step 2 — 调用 xmtp_dispatch_session 通知 user session**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 任务 {job_id} 卖家已同意退款，资金已返还至您的钱包。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 任务超时（OPEN→EXPIRED 或 ACCEPTED→EXPIRED）──────────
        Event::JobExpired => format!(
            "【当前状态】job_expired（任务超时，无人接单或卖家未提交）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_dispatch_session 通知 user session（用户（确认））**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 任务 {job_id} 已超时（accept 截止前未接单 或 submit 截止前未提交），是否关闭任务回收资金？\n\n\
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
             **Step 1 — 调用 xmtp_dispatch_session 通知 user session**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 任务 {job_id} 已关闭，资金已回收。\n\n\
             检查 payload 中 status 字段：\n\
             - success → 任务已关闭\n\
             - failed → 关闭失败，按 errorCode 重试\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── 卖家主动联系买家（public 任务，卖家找到任务后发起会话）─────
        Event::Other(ref s) if s == "provider_conversation" => format!(
            "【系统通知】provider_conversation（有卖家主动联系你）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 获取待沟通卖家列表：**\n\
             调用 xmtp_get_pending_list 工具获取待沟通卖家列表。\n\
             ⚠️ 调用前输出：`[buyer-xmtp] xmtp_get_pending_list`\n\
             ⚠️ 调用后输出：`[buyer-xmtp] xmtp_get_pending_list result: <返回值>`\n\n\
             如果返回空列表 → 无需处理，结束。\n\n\
             **Step 2 — 调用 xmtp_dispatch_session 通知 user session**（省略 sessionKey = 发到 main session）：\n\
             \x20\x20content: 有新的卖家请求做你的任务：\n\
             \x20\x20- jobId: {job_id}\n\
             \x20\x20- 任务标题：<从 pending list 中提取>\n\
             \x20\x20- 卖家 AgentID：<pending list 中第一个卖家的 agentId>\n\
             \x20\x20- 卖家名称：<pending list 中第一个卖家的 name>\n\
             \x20\x20是否接受并开始协商？\n\n\
             **Step 3 — 等待用户回复，按用户决策分支：**\n\n\
             ━━━━━━━━━ 分支 A：用户接受 → 与第一个卖家协商 ━━━━━━━━━\n\n\
             A-Step 1：调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<pending list 第一个卖家的 agentId>，jobId={job_id}\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             A-Step 2：调用 xmtp_send 工具向卖家发起协商（参照 buyer.md §2.1 协商剧本三步确认）：\n\
             \x20\x20参数：sessionKey=<A-Step 1 返回的 sessionKey>，content=<下方消息内容>\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_send: sessionKey=<sessionKey>, content=<消息内容前50字>`\n\
             {header_template}\n\
             你好，我有一个任务（jobId: {job_id}）想请你来完成，请问你感兴趣吗？\n\n\
             A-Step 3：协商成功 → 卖家 apply 上链 → 等待 provider_applied 事件（进入场景 6）\n\n\
             ━━━━━━━━━ 分支 B：用户不接受 / 协商失败 → 尝试下一个卖家 ━━━━━━━━━\n\n\
             B-Step 1：重新调用 xmtp_get_pending_list 获取最新待沟通列表。\n\
             B-Step 2：如果列表不为空 → 通知 user session 下一个卖家信息，等待用户确认后重复分支 A。\n\
             B-Step 3：如果列表为空 → 调用 xmtp_dispatch_session 通知 user session：\n\
             \x20\x20content: 任务 {job_id} 当前没有更多待沟通卖家，建议等待新卖家联系或调整任务描述。\n\n\
             【循环结束条件】xmtp_get_pending_list 返回空列表。\n"
        ),

        // ─── 可见性切换结果（setVisibility tx 结果）───────────────────
        Event::JobVisibilityChanged => format!(
            "【当前状态】job_visibility_changed（公开/私有切换已上链）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             检查 payload 中 status 字段：\n\
             - success → 公开/私有切换已生效\n\
             - failed → 切换失败，按 errorCode 重试\n\n\
             **通知主 session：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
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
             **通知主 session：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
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
             **Step 2 — 通知主 session：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
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
             **Step 2 — 通知主 session：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
             content: \"任务 {job_id} 已转为公开任务，等待卖家主动申请。\"\n"
        ),

        // ─── 卖家未提交交付物超时 ─────────────────────────────────────
        Event::SubmitExpired => format!(
            "【系统通知】卖家提交交付物超时\n\
             【角色】买家（Client）\n\n\
             卖家未在规定期限内提交交付物，你可以申请自动退款。\n\n\
             **Step 1 — 通知主 session（用户确认）：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
             content: \"任务 {job_id} 的卖家未在截止时间前提交交付物，是否申请自动退款？\"\n\n\
             **Step 2 — 用户确认后，领取自动退款：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 3 — 通知主 session：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
             content: \"任务 {job_id} 已申请自动退款，资金将退回你的账户。\"\n"
        ),

        // ─── 买家拒绝后卖家仲裁超时 ─────────────────────────────────
        Event::RefuseExpired => format!(
            "【系统通知】卖家仲裁超时\n\
             【角色】买家（Client）\n\n\
             你拒绝交付物后，卖家未在规定期限内发起仲裁，你可以申请自动退款。\n\n\
             **Step 1 — 通知主 session（用户确认）：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
             content: \"任务 {job_id} 的卖家在你拒绝交付物后未及时发起仲裁，是否申请自动退款？\"\n\n\
             **Step 2 — 用户确认后，领取自动退款：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 3 — 通知主 session：**\n\
             调用 xmtp_dispatch_session（省略 sessionKey = 发送到主 session）：\n\
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
        Event::ReviewExpired => format!(
            "【系统通知】review_expired（验收超时，provider 已自动 complete）\n\
             【角色】买家（Client）\n\n\
             【建议】task 已自动进入 completed 状态，资金已释放给 provider。\n\
             子 session 可关闭。\n"
        ),

        // ─── provider 的截止提醒 — buyer 端无关 ────────────────────────
        Event::SubmitDeadlineWarn => format!(
            "【系统通知】submit_deadline_warn（provider 端截止提醒）\n\
             【角色】买家（Client）\n\n\
             【建议】静默观察即可，等 provider 提交交付物（job_submitted 通知）后再处理。\n"
        ),

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

        // ─── 质押 / 罚没 lifecycle — buyer 不是 evaluator 时无关 ─────
        Event::Staked
        | Event::StakeIncreased
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
