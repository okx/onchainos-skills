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
            format!("  onchainos agent confirm-accept {job_id} --provider <addr> --payment-mode <escrow|non_escrow|x402> --token-symbol <sym> --token-amount <amt> [--endpoint <url>]  # 接受卖家并注资"),
            format!("  onchainos agent close {job_id}          # 关闭任务"),
            format!("  onchainos agent set-public {job_id}     # 转为公开任务"),
        ],
        Status::Accepted => vec![
            next_action("job_accepted"),
            ref_header.clone(),
            format!("  onchainos agent complete {job_id}       # 非担保：接单后立即 direct/complete 完成支付链路"),
            "（escrow 被动等待）卖家执行任务中：job_submitted → 进入验收".to_string(),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            ref_header,
            format!("  onchainos agent complete {job_id}       # escrow：验收通过，释放款项（non_escrow 已在 accepted 阶段完成）"),
            format!("  onchainos agent reject {job_id} --reason <reason>  # 拒绝验收（仅 escrow）"),
            format!("  onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id <buyerAgentId> --score <0-100> --task-id {job_id}  # 评价卖家"),
        ],
        Status::Refused => vec![
            next_action("job_refused"),
            "（被动等待）卖家 24h 内决策：job_disputed → 进入仲裁举证；job_refunded → 退款".to_string(),
        ],
        Status::Disputed => vec![
            next_action("job_disputed"),
            ref_header,
            format!("  onchainos agent dispute upload {job_id} --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
        ],
        Status::Completed => vec![
            next_action("job_completed"),
            "（escrow 流程结束）任务完成，资金已释放。子 session 可关闭。".to_string(),
            "（non_escrow）任务支付链路完成，等待卖家提交交付物。".to_string(),
        ],
        Status::Refunded => vec![
            next_action("job_refunded"),
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

    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md Session 通信契约。
    // 本文件只告诉 agent **每一步把什么内容发到哪**。
    // ──────────────────────────────────────────────────────────────────────
    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md Session 通信契约。
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
        "→ 用 xmtp_send 发给卖家（机制见 SKILL.md Session 通信契约 1.4）。\n\
         当前 sub session：jobId={job_id}，我方 agentId={agent_id}。\n\
         content（纯自然语言，不要包 markdown / 代码块）："
    );
    let header_template = &send_to_peer;

    let context_preamble = format!(
        "📍 你在 sub session（你看到这段 next-action 输出 = 100% 在 sub）。\n\n\
         🔒 **如果当前 turn 没读过 SKILL.md Session 通信契约**（envelope 形态白名单 / xmtp_send 两步 / xmtp_dispatch_user·xmtp_prompt_user 推 user session 铁律），\n\
         **先读 `skills/okx-agent-task/SKILL.md`** 再继续——下面步骤会引用它的章节（3 / 4 / 5 / 6）。\n\n\
         ⚠️ **异常升级硬规则**（任何场景都适用，详见 SKILL.md 通讯边界 + buyer.md）：\n\
         \x20\x201) 协议理解错位：你已澄清同一条流程 ≥1 次，对方下一条还在重复错误诉求 → **不再回复对方**，调 `xmtp_dispatch_user` 推 `[⚠️ 协议理解错位] ...`，结束 turn\n\
         \x20\x202) CLI 错误：`onchainos agent <cmd>` 报错 → **不要重试**，直接调 `xmtp_dispatch_user` 推 `[⚠️ CLI 报错] ...`，等用户新指令。**唯一例外**：JWT 过期（msg 含 `JWT verification failed` / `unauthorized`）刷新登录态后自动重试一次；网络 timeout 也按业务错处理推用户，不在 sub 里盲重\n\
         \x20\x203) ❌ **绝对禁止把技术错误细节广播给对方**：CLI 命令名 / 后端字段名 / stderr 摘要 / `bug`/`命令：`/`错误：` 一律不能进 xmtp_send 给对方。最多发一句『稍等，正在确认细节』或干脆不通知对方。\n\
         \x20\x204) ❌ **同 turn 不重复 xmtp_send**：剧本说『发一条』→ 调过一次工具返回『已发送』就**算成功**，**当前 turn 内不再对同一对方调 xmtp_send 第二次**。不要因为消息可能不够清晰就重发——重发 = 刷屏 + 触发对方循环。下一条 inbound 进来再说。\n\
         \x20\x205) ❌ **apply 是卖家动作**：escrow 路径中 `apply` 由卖家执行，买家绝不能调 `onchainos agent apply`。买家只在收到卖家申请通知后执行 `confirm-accept`。non_escrow 路径需从卖家消息中提取 paymentId 再 confirm-accept。\n\
         \x20\x206) ❌ **同 turn 不重复 `session_status`**：sub session 的 sessionKey 在同一 turn 内是稳定的——**调过一次就把结果存住，后续 step 直接复用**。即使剧本多个 step 都提到 sessionKey，也只调一次 session_status。重复调 = 死循环征兆，必须立即停。\n\n\
         如果不记得本任务协商细节（deliverable / paymentMode / token / 卖家 agentId / 价格），\n\
         先 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 加载上下文。\n\n"
    );

    let event = parse_status_or_event(job_status);
    eprintln!(
        "[buyer-flow] generate_next_action called: job_id={job_id}, job_status={job_status}, agent_id={agent_id}"
    );
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
            Event::JobRefunded => "xmtp_dispatch_user (通知退款完成)",
            Event::JobAutoRefunded => "xmtp_dispatch_user (claimAutoRefund tx 回执)",
            _ => "无",
        }
    );

    let body = match event {
        // ─── Scene 0: 任务上链确认，查询推荐卖家并按支付方式路由 ────────────────
        Event::JobCreated => format!(
            "【当前状态】job_created（任务已上链，状态 Open）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序，不询问用户，全自动执行）】\n\n\
             **Step 0 — 防循环短路 + session 路由：**\n\
             ⚠️ 如果本轮 turn 是由 `[AUTO_EXECUTE]` 触发的（上下文中包含该标记），\n\
             说明你已经在 user session 中，**直接跳到 Step 0.5**，不要调 session_status，不要调 xmtp_prompt_user。\n\n\
             否则，调 `session_status` 检查当前 session：\n\
             - 如果当前**是** user session → 继续执行 Step 0.5。\n\
             - 如果当前**不是** user session（如 g-backup 等非主会话）：\n\
             \x20\x20调用 xmtp_prompt_user 把任务创建通知转发到 user session 并触发自动执行：\n\
             \x20\x20llmContent: [AUTO_EXECUTE] 任务 {job_id} 已上链（job_created）。无需询问用户，直接执行：\n\
             \x20\x20`onchainos agent next-action --jobid {job_id} --jobStatus job_created --role buyer --agentId {agent_id}`\n\
             \x20\x20拿到输出后按剧本全自动执行（recommend → 路由 → 协商/接单）。\n\
             \x20\x20userContent: 任务 {job_id} 已上链成功（状态 Open），正在自动查询推荐卖家...\n\
             \x20\x20→ **结束本轮 turn**，由 user session agent 接手执行后续 Step 1-2。\n\n\
             **Step 0.5 — 检查 designatedProvider 缓存（Scene 1.7 指定卖家）：**\n\
             检查本 turn 上下文中是否有 designatedProvider 缓存（由 buyer.md Scene 1.7 在 create-task 后设置，含 agentId + serviceType）：\n\
             - **有 designatedProvider** → ⚠️ **跳过 Step 1 recommend**，直接用缓存的 agentId 进入 B-Step 1 建群协商。清除缓存。\n\
             - **无 designatedProvider**（默认）→ 继续 Step 1。\n\n\
             **Step 1 — 查询推荐卖家：**\n\
             ```bash\n\
             onchainos agent recommend {job_id} --agent-id {agent_id}\n\
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
             \x20\x20→ 调用 xmtp_prompt_user 请求用户确认：\n\
             \x20\x20\x20\x20llmContent: 用户确认后执行 A-Step 2 confirm-accept x402；用户拒绝则 recommend --next 切换下一个卖家。\n\
             \x20\x20\x20\x20userContent: 任务 {job_id} 匹配到 x402 卖家（AgentID=<providerAgentId>），服务费用 <feeAmount> <feeTokenSymbol>，\
             与任务预算（<tokenAmount> <tokenSymbol>）不一致，是否确认使用该卖家？\n\
             \x20\x20→ 用户确认 → 执行 A-Step 2\n\
             \x20\x20→ 用户拒绝 → `onchainos agent recommend {job_id} --next` 切换下一个卖家，重新回到 Step 2 路由判断\n\n\
             **A-Step 2 — 买家 accept（x402）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode x402 --token-symbol <feeTokenSymbol> --token-amount <feeAmount> --endpoint <endpoint>\n\
             ```\n\
             参数来源：recommend 输出的 services[0] 中的 feeTokenSymbol、feeAmount、endpoint。\n\
             ⚠️ CLI 内部有三级 fallback（CLI flag > recommend 缓存 > service-list API），但显式传参最可靠。\n\
             （命令内部自动执行：setPaymentMode(3) → direct/accept → 签名广播 → x402 endpoint 支付 → direct/complete → 签名广播）\n\n\
             2. 完成后任务状态 → complete（x402 不会收到 job_accepted 通知，命令内部直接 complete）。\n\n\
             **A-Step 3 — 调用 xmtp_dispatch_user 通知用户结果：**\n\
             \x20\x20content: 任务 {job_id} 已通过 x402 自动完成。卖家 AgentID=<providerAgentId>，\
             费用=<feeAmount> <feeTokenSymbol>。任务已完成，等待卖家交付。\n\n\
             ━━━━━━━━━ 分支 B：supportA2MCP=false → A2A（需协商）━━━━━━━━━\n\n\
             **B-Step 1 — 建群：**\n\
             调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<recommend 输出的 providerAgentId>，jobId={job_id}\n\
             \x20\x20成功返回 sessionKey + xmtpGroupId。\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<providerAgentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             **B-Step 2 — 自动协商（买家 Agent ↔ 卖家 Agent 在 sub session 中多轮交互）：**\n\
             ⚠️ B-Step 1 建群后，已进入 sub session。直接用 xmtp_send 发送消息。\n\
             ⚠️ **禁止**用 xmtp_dispatch_user / xmtp_dispatch_session，建群后统一用 xmtp_send。\n\n\
             协商目标：就以下结构化字段达成一致——\n\
             \x20\x20- deliverable：交付物描述（具体要做什么）\n\
             \x20\x20- qualityStandards：验收标准\n\
             \x20\x20- paymentMode：支付方式（escrow 或 non_escrow）\n\
             \x20\x20- tokenSymbol：支付代币\n\
             \x20\x20- tokenAmount：支付金额\n\
             \x20\x20- deadline：交付截止时间\n\n\
             ⏱ 超时规则：每轮等待卖家回复最多 5 分钟。超时未回复 → 结束当前 sub session，切换下一个卖家。\n\n\
             协商步骤：\n\
             1. 调用 xmtp_send 发送第一条询盘消息：\n\
             \x20\x20content=<任务详情（描述、预算、期望交付物、支付方式）>\n\
             \x20\x20→ 等待卖家回复（5 分钟超时）\n\
             2. （sub session 内）卖家回复报价（金额、代币、支付方式偏好、预计交付时间）\n\
             3. （sub session 内）双方就价格/条件进行调整（可能多轮，每轮 5 分钟超时）\n\
             \x20\x20每轮调用 xmtp_send，参数：sessionKey=<同上>，content=<协商内容>\n\
             ⚠️ **币种铁律**：协商只允许改**金额**，不允许改**币种**。任务发布时的币种（从 `onchainos agent common context` 获取）\n\
             是链上合约绑定的。如果卖家提出不同币种，必须纠正：「本任务使用 <任务币种>，请用 <任务币种> 报价。」\n\n\
             ⚠️ 任一步骤卖家 5 分钟未回复 → 视为协商失败，结束当前 sub session，执行「切换下一个卖家」。\n\n\
             4. 达成初步一致后，调用 xmtp_send 发送 **[NEGOTIATE_PROPOSE]** 结构化提案（必须严格使用此格式，卖家 Agent 会机器解析）：\n\
             \x20\x20content=\n\
             [NEGOTIATE_PROPOSE]\n\
             jobId: {job_id}\n\
             deliverable: <交付物描述>\n\
             qualityStandards: <验收标准>\n\
             paymentMode: <escrow|non_escrow>\n\
             tokenSymbol: <USDT|USDG>\n\
             tokenAmount: <金额>\n\
             deadline: <交付截止时间>\n\n\
             5. **等待卖家回复 [NEGOTIATE_ACK] 或 [NEGOTIATE_COUNTER]**（5 分钟超时）：\n\n\
             \x20\x20▸ 收到 **[NEGOTIATE_ACK]** → 逐字段校验卖家回传的值与你发送的 PROPOSE 完全一致：\n\
             \x20\x20\x20\x20- 全部一致 → 协商成功，执行 Step 6\n\
             \x20\x20\x20\x20- 任一字段不一致 → 视为篡改，调 xmtp_send 告知卖家字段不一致并重新发送 [NEGOTIATE_PROPOSE]\n\n\
             \x20\x20▸ 收到 **[NEGOTIATE_COUNTER]** → 卖家提出反提案：\n\
             \x20\x20\x20\x20- 检查 tokenSymbol 是否被改动（禁止改币种）→ 如被改动，拒绝并纠正\n\
             \x20\x20\x20\x20- 评估 tokenAmount / deadline 等调整是否可接受\n\
             \x20\x20\x20\x20- 可接受 → 用 COUNTER 中的值发新的 [NEGOTIATE_PROPOSE]，回到 Step 5 等 ACK\n\
             \x20\x20\x20\x20- 不可接受 → 继续协商或终止切换下一个卖家\n\n\
             \x20\x20▸ 收到的回复**不含** [NEGOTIATE_ACK] 也不含 [NEGOTIATE_COUNTER] 标记 → 视为自然语言讨论，继续协商，重新回到 Step 4\n\n\
             6. **协商确认完成 → 保存 + 分流**：\n\n\
             ⚠️ **收到 [NEGOTIATE_ACK] 且校验一致后，立即保存协商结果**：\n\
             ```bash\n\
             onchainos agent save-agreed {job_id} --token-symbol <协商币种> --token-amount <协商价格>\n\
             ```\n\
             不保存会导致后续 confirm-accept 使用错误的币种/金额。\n\n\
             **按协商确定的支付方式分流**：\n\n\
             \x20\x20▸ **escrow（担保）**：\n\
             \x20\x20\x20\x20调 xmtp_send 告知卖家：协商已确认，请你（卖家）执行 apply 接单。\n\
             \x20\x20\x20\x20⚠️ apply 是卖家动作，买家不执行 apply。\n\
             \x20\x20\x20\x20卖家确认 → 卖家执行 apply 上链 → 系统通知 provider_applied → 进入 ProviderApplied 事件处理。\n\n\
             \x20\x20▸ **non_escrow（非担保）**：\n\
             \x20\x20\x20\x20调 xmtp_send 告知卖家：协商已确认，请你（卖家）生成付款单（create_payment_charge）并把 paymentId 发给我。\n\
             \x20\x20\x20\x20⚠️ 非担保不走 apply，卖家调 create_payment_charge 生成账单后通过 XMTP 把 paymentId 发给买家。\n\
             \x20\x20\x20\x20买家收到 paymentId 后直接执行：\n\
             \x20\x20\x20\x20```bash\n\
             \x20\x20\x20\x20onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode non_escrow --payment-id <paymentId>\n\
             \x20\x20\x20\x20```\n\
             \x20\x20\x20\x20（内部：setPaymentMode(2) → a2a_pay EIP-3009 签名 → direct/accept 获取 calldata → 签名 → 广播）\n\
             \x20\x20\x20\x20→ 等待 job_accepted 系统通知。\n\n\
             **B-Step 3 — 调用 xmtp_dispatch_user 通知用户协商进展：**\n\
             \x20\x20content: 已自动联系推荐卖家（<providerAgentId>），进入协商流程，等待对方回复。\n\n\
             ━━━━━━━━━ 遍历结束 / 切换下一个卖家 ━━━━━━━━━\n\n\
             当前卖家超时未回复（5 分钟）或协商失败 → 结束当前 sub session → `onchainos agent recommend {job_id} --next` 切换下一个卖家，重新回到 Step 2 路由判断。\n\
             推荐列表全部遍历完（或初始推荐列表为空）→ 调用 xmtp_prompt_user 引导用户选择：\n\
             \x20\x20userContent: 任务 {job_id} 推荐卖家已全部遍历，无合适匹配。请选择下一步：\n\
             \x20\x20A. 指定卖家 — 请提供卖家 agentId\n\
             \x20\x20B. 转为公开任务 — 让更多卖家看到任务\n\
             \x20\x20C. 关闭任务 — 取消并退款\n\
             \x20\x20llmContent: 用户选择 A → 用提供的 agentId 调 xmtp_start_conversation 建群协商（进入 B-Step 1）；\
             选择 B → `onchainos agent set-public {job_id}`；\
             选择 C → `onchainos agent close {job_id}`。\n\
             \x20\x20⚠️ **不要自动选择，必须等用户回复后再执行。**\n\n\
             【后续事件】\n\
             - x402 → confirm-accept 完成后等待 job_accepted\n\
             - A2A escrow → 协商完成 → 卖家 apply → provider_applied → 买家 confirm-accept → job_accepted\n\
             - A2A non_escrow → 协商完成 → 卖家 create_payment_charge → 发 paymentId → 买家 confirm-accept → job_accepted\n"
        ),

        // ─── Scene 6: 卖家申请接单，确认接单（仅 escrow 路径会收到此事件） ──────────
        // ⚠️ 非担保（non_escrow）不走 apply，不会触发 provider_applied。
        // 非担保的 confirm-accept 在 JobCreated 协商尾部的 non_escrow 分支中直接执行。
        Event::ProviderApplied => format!(
            "【当前状态】provider_applied（卖家已链上申请接单 — 仅 escrow 担保支付）\n\
             【角色】买家（Client）\n\n\
             【前置】协商阶段已确定金额、代币，从协商上下文获取：\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取协商结果：providerAgentId、tokenAmount、tokenSymbol。\n\
             ⚠️ tokenAmount 和 tokenSymbol 必须从协商结果获取，不是任务详情。\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 确认接单（escrow 担保支付）：**\n\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider <providerAgentId> --payment-mode escrow --token-symbol <tokenSymbol> --token-amount <tokenAmount>\n\
             ```\n\
             （内部：setPaymentMode(1) → providerConfirmStatus → sign_escrow TEE 签名 → accept 获取 calldata → 签名 → 广播，资金托管）\n\n\
             **Step 2 — 调用 xmtp_send 工具向卖家发送：**\n\n\
             {header_template}\n\
             已确认接单，支付方式：escrow（担保）。等待你开始执行任务。\n\n\
             【后续事件】\n\
             - job_accepted → 通知 user session 接单成功，等待卖家交付\n"
        ),

        // ─── job_accepted: 按支付方式分流（非担保立即 complete，担保等交付）──────────────────
        Event::JobAccepted => format!(
            "【当前状态】job_accepted（买家已确认接单，任务进入执行阶段）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 获取任务完整信息：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取：title、description、deliverable、providerAgentId、paymentMode（int：1=escrow, 2=non_escrow, 3=x402）、tokenAmount、tokenSymbol。\n\n\
             **Step 2 — 按支付方式分流：**\n\n\
             ━━━━━━━━━ 分支 A：escrow（担保）━━━━━━━━━\n\n\
             调用 xmtp_dispatch_user 通知用户接单成功：\n\
             \x20\x20content:\n\
             \x20\x20[接单成功] 任务 {job_id} 已确认接单，进入执行阶段。\n\
             \x20\x20任务标题：<title>\n\
             \x20\x20任务描述：<description>\n\
             \x20\x20交付物：<deliverable>\n\
             \x20\x20卖家 AgentID：<providerAgentId>\n\
             \x20\x20支付方式：escrow（担保）\n\
             \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
             \x20\x20等待卖家执行并提交交付物。\n\n\
             可选：调用 xmtp_send 工具向卖家发送确认：\n\n\
             {header_template}\n\
             接单已确认，期待你的交付。\n\n\
             【后续事件】\n\
             - job_submitted → 验收交付物\n\n\
             ━━━━━━━━━ 分支 B：non_escrow（非担保）━━━━━━━━━\n\n\
             ⚠️ 非担保流程：接单后需**立即执行 complete** 完成支付链路，然后等卖家交付。\n\n\
             **B-Step 1 — 执行 complete（单签）：**\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             （内部：POST /priapi/v1/aieco/task/{job_id}/direct/complete → 获取 calldata → 签名 uopHash → 广播上链）\n\n\
             **B-Step 2 — 等待 job_completed 系统通知**，不要在此 turn 做更多动作。\n\n\
             【后续事件】\n\
             - job_completed → 通知 user session，等待卖家提交交付物\n"
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
             下载成功后记录 localPath（完整绝对路径，如 /Users/.../task预发.png），后续展示给用户时必须显示完整路径。\n\
             如果下载失败 → 用 deliverableUrl 作为备用展示信息。\n\n\
             **Step 3 — 按支付方式分流：**\n\n\
             ━━━━━━━━━ 分支 A：escrow（担保）— 需要用户验收决策 ━━━━━━━━━\n\n\
             调用 xmtp_prompt_user 把交付物和验收决策请求推到 user session：\n\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey。\n\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}] \
             用户回复「验收通过」→ relay 回 sub session 执行 onchainos agent complete；\
             回复「拒绝，原因是...」→ relay 回 sub session 执行 onchainos agent reject。\
             禁止 user session agent 自己执行 task CLI。\n\
             \x20\x20\x20\x20userContent:\n\
             \x20\x20\x20\x20任务 {job_id} 卖家已提交交付物，已下载到本地。\n\
             \x20\x20\x20\x20交付物本地路径：<localPath 完整绝对路径>（如下载失败则显示 deliverableUrl）\n\
             \x20\x20\x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20\x20\x20验收标准：<qualityStandards>\n\
             \x20\x20\x20\x20支付方式：escrow（担保）\n\
             \x20\x20\x20\x20请选择：\n\
             \x20\x20\x20\x201. 验收通过 → 回复「验收通过」\n\
             \x20\x20\x20\x202. 拒绝 → 回复「拒绝，原因是<原因>」\n\n\
             **Step 4（escrow）— 等用户回复 relay 回来**，按用户决策执行：\n\
             收到 `[USER_DECISION_RELAY] 用户决策：...` 后，按关键词执行：\n\n\
             ▸ 用户验收通过 — 双签流程：\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             内部流程：\n\
             \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-complete（712 标准，非 uop）→ 获取 digest\n\
             \x20\x202. ED25519 签名 digest → signature\n\
             \x20\x203. POST /priapi/v1/aieco/task/{job_id}/complete（body: {{\"signature\": \"<sig>\"}}）→ 获取 uopData\n\
             \x20\x204. 签名 uopHash → 广播上链\n\
             \x20\x20→ 任务状态变为 Complete，资金从合约释放给卖家。\n\n\
             ▸ 用户拒绝 — 双签流程：\n\
             ```bash\n\
             onchainos agent reject {job_id} --reason \"<用户提供的拒绝原因>\"\n\
             ```\n\
             内部流程：\n\
             \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-refuse（712 标准，非 uop）→ 获取 digest\n\
             \x20\x202. ED25519 签名 digest → signature\n\
             \x20\x203. POST /priapi/v1/aieco/task/{job_id}/refuse（body: {{\"signature\": \"<sig>\", \"reason\": \"<reason>\"}}）→ 获取 uopData\n\
             \x20\x204. 签名 uopHash → 广播上链\n\
             \x20\x20→ 任务状态变为 Refused，卖家 24h 内可发起仲裁。\n\n\
             ━━━━━━━━━ 分支 B：non_escrow（非担保）— 交付物通知 + 终态收尾 ━━━━━━━━━\n\n\
             ⚠️ 非担保流程中 complete 已在 job_accepted 阶段完成，此时收到交付物即为任务真正终态。\n\n\
             **B-Step 1 — 调用 xmtp_dispatch_user 通知用户收到交付物：**\n\
             \x20\x20content:\n\
             \x20\x20[交付物已收到] 任务 {job_id} 卖家已提交交付物。\n\
             \x20\x20交付物本地路径：<localPath 完整绝对路径>（如下载失败则显示 deliverableUrl）\n\
             \x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20验收标准：<qualityStandards>\n\
             \x20\x20\n\
             \x20\x20本任务流程结束。\n\n\
             **B-Step 2 — 给卖家发完成致谢：**\n\n\
             {header_template}\n\
             交付物已收到，任务完成，感谢合作。\n\n\
             **B-Step 3 — 评价卖家（通过身份系统）：**\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <0-100> --task-id {job_id} --description \"<评价内容>\"\n\
             ```\n\n\
             **B-Step 4 — 关闭 sub session**（终态收尾，机制见 SKILL.md Session 通信契约 4.5）：\n\
             （debug 模式：暂不关闭 sub session，保留历史信息）\n\
             <!-- 1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段 -->\n\
             <!-- 2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串 -->\n\
             <!-- 删除后本 sub session 不再接收任何消息—— -->任务完整结束。\n\n\
             【后续事件】\n\
             - escrow: job_completed → 任务完成 / job_refused → 等待卖家决定仲裁或退款\n\
             - non_escrow: 流程已结束（本分支已执行评价 + 关闭 session）\n"
        ),

        // ─── job_refused: 买家已拒绝，等待卖家决策 ─────────────────
        Event::JobRefused => format!(
            "【当前状态】job_refused（买家已拒绝交付物，等待卖家决定）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             无需执行 CLI 命令。卖家有 24h 决定：\n\
             - 发起仲裁 → 你将收到 job_disputed\n\
             - 同意退款 → 你将收到 job_refunded\n\
             - 24h 超时 → 系统自动退款，你将收到 job_refunded\n\n\
             调用 xmtp_send 工具向卖家发送：\n\n\
             {header_template}\n\
             交付物已拒绝，等待你的后续处理。\n\n\
             【后续事件】\n\
             - job_disputed → 提交买家证据（Scene 6）\n\
             - job_refunded → 退款完成\n"
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
             - job_refunded → 仲裁买家胜诉，退款\n\n\
             跑完 Step 1-3 → **结束本轮 turn，不要 xmtp_dispatch_user / xmtp_prompt_user 推 main**。\n"
        ),

        // ─── 任务完成（按支付方式分流） ─────────────────────────────────
        Event::JobCompleted => format!(
            "【当前状态】job_completed（任务支付链路完成）\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 获取任务信息和支付方式：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取：title、tokenAmount、tokenSymbol、paymentMode（int：1=escrow, 2=non_escrow, 3=x402）。\n\n\
             **Step 2 — 按支付方式分流：**\n\n\
             ━━━━━━━━━ 分支 A：escrow（担保）— 流程结束 ━━━━━━━━━\n\n\
             担保模式下 job_completed 意味着卖家已交付且买家已验收，资金从合约释放给卖家。\n\n\
             **A-Step 1 — 给卖家发完成致谢**：\n\n\
             {header_template}\n\
             任务已完成（job_completed），感谢合作。\n\n\
             **A-Step 2 — 调用 xmtp_dispatch_user 告知用户任务完成：**\n\
             content：\n\
             \x20\x20\x20\x20[任务完成] 任务 {job_id}（<title>）已验收通过，资金已释放给卖家。\n\
             \x20\x20\x20\x20  - 支出：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **A-Step 3 — 评价卖家（通过身份系统）：**\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id {agent_id} --score <0-100> --task-id {job_id} --description \"<评价内容>\"\n\
             ```\n\n\
             **A-Step 4 — 关闭 sub session**（终态收尾，机制见 SKILL.md Session 通信契约 4.5）：\n\
             （debug 模式：暂不关闭 sub session，保留历史信息）\n\
             <!-- 1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段 -->\n\
             <!-- 2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串 -->\n\
             <!-- 删除后本 sub session 不再接收任何消息—— -->任务完整结束。\n\n\
             ━━━━━━━━━ 分支 B：non_escrow（非担保）— 支付链路完成，等待卖家交付 ━━━━━━━━━\n\n\
             ⚠️ 非担保模式下 job_completed 意味着支付链路（accept + complete）已完成上链，\n\
             但**卖家尚未提交交付物**。不要关闭 sub session，不要评价。\n\n\
             **B-Step 1 — 调用 xmtp_dispatch_user 通知用户：**\n\
             content：\n\
             \x20\x20\x20\x20[支付完成] 任务 {job_id}（<title>）支付链路已完成上链。\n\
             \x20\x20\x20\x20  - 支出：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 支付方式：非担保（non_escrow）\n\
             \x20\x20\x20\x20等待卖家提交交付物。\n\n\
             **B-Step 2 — 可选：调用 xmtp_send 向卖家确认：**\n\n\
             {header_template}\n\
             支付已完成上链，请开始执行任务并提交交付物。\n\n\
             【后续事件】\n\
             - job_submitted → 卖家提交交付物，通知用户\n"
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
             **Step 3（两个分支都要做）— 关闭 sub session**（终态收尾，机制见 SKILL.md Session 通信契约 4.5）：\n\
             （debug 模式：暂不关闭 sub session，保留历史信息）\n\
             <!-- 1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段 -->\n\
             <!-- 2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串 -->\n\
             <!-- 删除后本 sub session 不再接收任何消息—— -->仲裁流程完整结束。\n"
        ),

        // ─── 卖家同意退款 / 仲裁退款上链 ─────────────────────────────
        Event::JobRefunded => format!(
            "【当前状态】job_refunded（资金已退还买家）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 给卖家发收尾**：\n\n\
             {header_template}\n\
             退款已上链（job_refunded），资金已返还至我方钱包。\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户退款完成：**\n\n\
             content：\n\
             \x20\x20\x20\x20[退款完成] 任务 {job_id} 退款已上链，资金已返还至您的钱包。\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 3 — 关闭 sub session**（终态收尾，机制见 SKILL.md Session 通信契约 4.5）：\n\
             （debug 模式：暂不关闭 sub session，保留历史信息）\n\
             <!-- 1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段 -->\n\
             <!-- 2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串 -->\n\
             <!-- 删除后本 sub session 不再接收任何消息—— -->退款流程完整结束。\n"
        ),

        // ─── claimAutoRefund tx 回执（submit/refuse 超时后 buyer 主动领回资金）──
        Event::JobAutoRefunded => format!(
            "【系统通知】job_auto_refunded（claimAutoRefund tx 回执）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 判断 payload 中的 status：**\n\
             - `success` → 自动退款成功，资金已到账。执行 Step 2。\n\
             - `failed` → 按 errorCode 重试：\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户退款到账：**\n\n\
             content：\n\
             \x20\x20\x20\x20[自动退款成功 💰] 任务 {job_id} 的托管资金已退还至您的钱包。\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 3 — 关闭 sub session**（终态收尾）：\n\
             （debug 模式：暂不关闭 sub session，保留历史信息）\n\
             <!-- 1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段 -->\n\
             <!-- 2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串 -->\n\
             <!-- 删除后本 sub session 不再接收任何消息—— -->退款流程完整结束。\n"
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
             **Step 2 — 调用 xmtp_prompt_user 请求用户确认是否与第一个卖家建立协商：**\n\
             ⚠️ 此步用户只能选择「开始协商」或「拒绝」，不能在此步直接讨论价格、条件等协商内容。具体协商必须在建立 sub session 之后进行。\n\
             从 pending list 第一个卖家提取信息，展示给用户：\n\
             \x20\x20llmContent: 用户接受 → 仅执行 xmtp_start_conversation 建群（A 分支），协商在 sub session 中进行；用户拒绝 → 调用 xmtp_deny_pending_conversation 拒绝后尝试下一个卖家（B 分支）。\n\
             \x20\x20userContent:\n\
             \x20\x20有卖家申请做你的任务，是否开始协商？\n\
             \x20\x20- 任务 JobId：{job_id}\n\
             \x20\x20- 任务标题：<pending list 中的 job title>\n\
             \x20\x20- 卖家 AgentID：<第一个卖家的 agentId>\n\
             \x20\x20- 卖家名称：<第一个卖家的 name>\n\
             \x20\x20- 信用分：<第一个卖家的 creditScore>\n\
             \x20\x20- 完成任务数：<第一个卖家的 completedTaskCount>（如有）\n\
             \x20\x20请回复「开始协商」建立会话，或「拒绝」跳过此卖家。\n\n\
             **Step 3 — 等待用户回复，按用户决策分支：**\n\n\
             ━━━━━━━━━ 分支 A：用户接受 → 建立 session 后协商 ━━━━━━━━━\n\n\
             A-Step 1：调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<当前卖家的 agentId>，jobId={job_id}\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             A-Step 2：建群后已进入 sub session，调用 xmtp_send 向卖家发起协商（参照 buyer.md 3.2 协商阶段三步确认）：\n\
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
             卖家未在规定期限内提交交付物，自动执行退款。\n\n\
             **Step 1 — 立即领取自动退款（无需用户确认）：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户：**\n\
             content: \"任务 {job_id} 的卖家未在截止时间前提交交付物，已自动申请退款，资金将退回你的账户。\"\n"
        ),

        // ─── 买家拒绝后卖家仲裁超时 ─────────────────────────────────
        Event::RefuseExpired => format!(
            "【系统通知】卖家仲裁超时\n\
             【角色】买家（Client）\n\n\
             你拒绝交付物后，卖家未在规定期限内发起仲裁，自动执行退款。\n\n\
             **Step 1 — 立即领取自动退款（无需用户确认）：**\n\
             ```bash\n\
             onchainos agent claim-auto-refund {job_id}\n\
             ```\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户：**\n\
             content: \"任务 {job_id} 的卖家在你拒绝交付物后未及时发起仲裁，已自动申请退款，资金将退回你的账户。\"\n"
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

        // ─── review_expired: review 窗口超时，等 provider 调 claimAutoComplete ─────
        Event::ReviewExpired => "【系统通知】review_expired（review 窗口超时，task 仍是 submitted）\n\
             【角色】买家（Client）\n\n\
             【建议】review 期已结束，资金尚未自动释放——需要等 provider 主动调 claimAutoComplete\n\
             才会进入 completed。本端无需动作，等 `job_auto_completed`（success）通知到达后再做 sub session 收尾。\n".to_string(),

        // ─── job_auto_completed: provider 的 claim 回执，buyer 端只需观察 ─────
        Event::JobAutoCompleted => "【系统通知】job_auto_completed（provider 已通过 claimAutoComplete 领走资金）\n\
             【角色】买家（Client）\n\n\
             【建议】task 已进入 completed 状态，资金已释放给 provider。子 session 可关闭。\n".to_string(),

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
        Event::DisputeApproved => "【系统通知】dispute_approved（provider 已上链仲裁阶段 1 approve，buyer 无关）\n\
             【建议】静默观察即可。等 `job_disputed` 通知到达再 next-action 进入证据准备期。\n".to_string(),

        // ─── 质押 / 罚没 lifecycle — buyer 不是 evaluator 时无关 ─────
        Event::Staked
        | Event::StakeIncreased
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed
        | Event::StakeStopped
        | Event::CooldownEntered => format!(
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
        Event::Other(ref other) => format!(
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
