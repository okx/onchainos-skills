//! Client (买家) 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 对应 provider/flow.rs 的买家版本，让 agent 只需
//! `exec onchainos agent next-action --role buyer ...` 拿提示词直接执行。

use crate::commands::agent_commerce::task::common::pending::short_job_id;
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
            format!("  onchainos agent set-payment-mode {job_id} --payment-mode <escrow|non_escrow|x402> --token-symbol <sym> --token-amount <amt> [--endpoint <url>]  # 设置支付方式"),
            format!("  onchainos agent confirm-accept {job_id} --provider-agent-id <agentId> --payment-mode <escrow|non_escrow> --token-symbol <sym> --token-amount <amt>  # 确认接单（setPaymentMode 后执行）"),
            format!("  onchainos agent direct-accept {job_id} --provider-agent-id <agentId> --token-symbol <sym> --token-amount <amt>  # x402 阶段 2b: endpoint 交互后调用"),
            format!("  onchainos agent close {job_id}          # 关闭任务"),
            format!("  onchainos agent set-public {job_id}     # 转为公开任务"),
        ],
        Status::Accepted => vec![
            next_action("job_accepted"),
            ref_header.clone(),
            format!("  onchainos agent complete {job_id} --payment-id <paymentId> --token-symbol <sym> --token-amount <amt>  # 非担保：收到交付物+paymentId后支付并完成"),
            "（escrow 被动等待）卖家执行任务中：job_submitted → 进入验收".to_string(),
            "（non_escrow 被动等待）卖家交付 + 发送 paymentId → 买家支付 + complete".to_string(),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            ref_header,
            format!("  onchainos agent complete {job_id}       # escrow：验收通过，释放款项"),
            format!("  onchainos agent reject {job_id} --reason <reason>  # 拒绝验收（仅 escrow）"),
            format!("  onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id <buyerAgentId> --score <score> --task-id {job_id}  # 评价卖家（用户回复「评价」后再收集评分和内容）"),
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
            "（终态）任务已 COMPLETE — **资金已释放给卖家**".to_string(),
            "  ▸ escrow 验收通过 → 释放担保款给卖家".to_string(),
            "  ▸ 仲裁卖家胜（dispute_resolved seller-wins）→ 释放担保款给卖家".to_string(),
            "  ▸ non_escrow 买家收到交付物后支付 + complete".to_string(),
            "⚠️ 保留 sub session（不关闭），便于事后查阅历史。".to_string(),
        ],
        Status::Rejected => vec![
            next_action("job_refunded"),
            "（终态）任务已 REJECTED — **资金已退还买家**".to_string(),
            "  ▸ 卖家同意退款（agree-refund）/ 自动退款 → 资金原路返回".to_string(),
            "  ▸ 仲裁买家胜（dispute_resolved buyer-wins）→ 退款".to_string(),
            "⚠️ 保留 sub session（不关闭），便于事后查阅历史。".to_string(),
        ],
        Status::Close => vec![
            "任务已关闭（Close）。⚠️ 保留 sub session（不关闭），便于事后查阅历史。".to_string(),
        ],
        Status::Expired => vec![
            "任务已过期（Expired）。".to_string(),
            format!("  onchainos agent claim-auto-refund {job_id}  # 领取自动退款"),
        ],
        Status::AdminStopped => vec![
            "任务已被管理员停止（AdminStopped）。请联系平台客服了解原因。".to_string(),
        ],
        Status::Init => vec![
            "任务初始化中（等待上链确认）→ 等待 job_created 事件".to_string(),
        ],
        Status::Other(s) => vec![
            format!("当前任务 status=`{s}` 不在 buyer 关心的状态集（open / accepted / submitted / refused / disputed / completed / rejected / close / expired / admin_stopped）内"),
            "→ 本角色无需任何任务级动作，等下一个相关链事件 / 用户决策再处理".to_string(),
            "→ **不要**重复跑 `agent status` / `agent common context`（结果会一样），结束本轮 turn".to_string(),
        ],
    }
}

/// 根据 jobStatus 生成 client/buyer 下一步动作的结构化提示词。
///
/// `job_status` 参数同时兼容 event 名（job_created / provider_applied / ...）
/// 和 status 名（open / submitted / ...），由 state_machine 统一解析。
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str, job_title: Option<&str>, seller: Option<&str>) -> String {
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    // 短 jobId,用在 xmtp_prompt_user 的 userContent 第一行 `[任务 <短ID> 你作为买家]` 前缀,
    // 多 prompt 并发时给用户和 user agent 双重消歧锚。详见 SKILL.md Session 通信契约 5.
    let short_id = short_job_id(job_id);

    // envelope 携带的 jobTitle — 有值时直接内联到剧本，省掉 agent 额外查询 API 取 title。
    let title_display = job_title.unwrap_or("<title>");
    let title_query_hint = if job_title.is_some() {
        String::new()
    } else {
        format!(
            "⚠️ 通知用户时使用 `<title>（{job_id}）` 格式。\
             title 从上下文取；如不记得，先 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 查询。\n\n"
        )
    };
    // Group B 事件仍需调 API 取 tokenAmount 等字段——"提取"列表里是否包含 title 取决于入参。
    let title_in_extract = if job_title.is_some() { "" } else { "title、" };

    // ──────────────────────────────────────────────────────────────────────
    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md Session 通信契约。
    // 本文件只负责告诉 agent **每一步把什么内容发到哪**，不重复解释工具用法。
    //
    // 三种通信工具：
    //   - xmtp_send：发给卖家（peer sub session），参数 sessionKey + content
    //   - xmtp_dispatch_user：通知用户（无需用户决策），参数：content
    //   - xmtp_prompt_user：需要用户交互（确认 / 决策），参数：llmContent + userContent
    //     llmContent = 注入 user session LLM 的指令（用户不可见，含 sub_key 让 user agent
    //                  把决策 relay 回 sub）
    //     userContent = 发送给用户的可见消息
    //
    // 老的 `xmtp_dispatch_session` 省略 sessionKey + `[STATUS_NOTIFY]` 包裹形态已被
    // `xmtp_dispatch_user` / `xmtp_prompt_user` 替代——本文件不再用 dispatch_session 推用户。
    // ──────────────────────────────────────────────────────────────────────
    let terminal_session_hint = if crate::commands::agent_commerce::task::common::config::KEEP_CONVERSATION_ON_TERMINAL {
        "⚠️ **不要 `xmtp_delete_conversation`**——保留会话历史便于事后查阅。"
    } else {
        "ℹ️ 任务终态,可调 `xmtp_delete_conversation` 释放会话资源(已无后续事件)。"
    };

    let escalation_protocol_misread = super::content::escalation_protocol_misread_notify(job_id);
    let escalation_cli_failed = super::content::escalation_cli_failed_notify(job_id);

    let context_preamble = format!(
        "🔒 当前 turn 未读 `skills/okx-agent-task/SKILL.md Session 通信契约` → 先读再继续(envelope 白名单 / xmtp_send 两步 / xmtp_dispatch_user·xmtp_prompt_user 推用户 铁律)。下面步骤会引用它的章节(3 / 4 / 5 / 6)。\n\n\
         ⚠️ **异常升级硬规则**（任何场景都适用，详见 _shared/exception-escalation.md + buyer.md）：\n\
         \x20\x201) 协议理解错位(同一流程澄清 ≥1 次对方仍重复) → **停回复对方**，调 `xmtp_dispatch_user`，content=`{escalation_protocol_misread}`，结束 turn\n\
         \x20\x202) 执行报错(`onchainos agent <cmd>` 失败) → **不重试**，调 `xmtp_dispatch_user`，content=`{escalation_cli_failed}`，等用户新指令。**例外**:JWT 失效（msg 含 `JWT verification failed`/`unauthorized`）自动重登一次；网络 timeout 同样推用户,不盲重\n\
         \x20\x203) ❌ **绝对禁止把技术错误细节广播给对方**：CLI 命令名 / 后端字段名 / stderr 摘要 / `bug`/`命令：`/`错误：` 一律不能进 xmtp_send 给对方。最多发一句『稍等，正在确认细节』或干脆不通知对方。\n\
         \x20\x204) ❌ **同 turn 不重复 xmtp_send**：剧本说『发一条』→ 调过一次工具返回『已发送』就**算成功**，**当前 turn 内不再对同一对方调 xmtp_send 第二次**。不要因为消息可能不够清晰就重发——重发 = 刷屏 + 触发对方循环。下一条 inbound 进来再说。\n\
         \x20\x205) ❌ **apply 是卖家动作**：escrow 路径中 `apply` 由卖家执行，买家绝不能调 `onchainos agent apply`。买家先调 `set-payment-mode`，再在收到卖家申请通知后执行 `confirm-accept`。non_escrow 路径需从卖家消息中提取 paymentId 再 confirm-accept。\n\
         \x20\x206) ❌ **同 turn 只调一次 `session_status`**:sessionKey 在同 turn 内稳定,调过一次结果复用。重复调 = 死循环征兆,立即停。\n\
         \x20\x207) ❌ **`xmtp_prompt_user` 必前后配对 `pending-decisions`**(唯一键 = jobId+role+agentId 三元组,规则源 `SKILL.md §通信契约 5`):\n\
         \x20\x20\x20\x20• 调 `xmtp_prompt_user` 前: `onchainos agent pending-decisions add --sub-key <sessionKey> --job-id {job_id} --role buyer --agent-id {agent_id} --summary \"<userContent 首行后简述>\" --user-content \"<userContent 完整原文>\"`\n\
         \x20\x20\x20\x20• 解析 `[USER_DECISION_RELAY]` 后、调 next-action 前: `onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}`\n\
         \x20\x20\x20\x20漏 `add` → 用户回复时反查不到本条决策,无法 relay 回本会话;\n\
         \x20\x20\x20\x20漏 `remove` → 旧条目残留成僵尸,下次再调 `xmtp_prompt_user` 时被误命中,用户回复派给错的会话。\n\
         \x20\x208) ❌ **用户可见内容禁用技术术语**:`xmtp_dispatch_user` 的 content 和 `xmtp_prompt_user` 的 userContent 都直接给用户看,**禁写** tool 名(`xmtp_*`) / 事件名(`provider_applied`/`job_*`/`dispute_resolved` 等) / 状态名(`open`/`accepted`/`disputed` 等英文枚举) / CLI flag(`--*`) / skill 名(`okx-agent-identity` / `§Feedback Submit` 等) / 状态字段名(`jobStatus`/`paymentMode` 等)——一律用自然中文(担保/非担保/x402,验收期超时,任务已完成,等)。同 turn 内的 `xmtp_send` 给卖家也按此规则。\n\
         \x20\x209) ❌ **禁止给卖家发过场消息**：除协商阶段的结构化消息（[NEGOTIATE_PROPOSE]、[NEGOTIATE_CONFIRM]、协商自然语言对话）外，**任何事件处理中都不要 xmtp_send 给卖家**。包括但不限于「已确认接单」「资金已托管」「已验收」「证据已提交」「任务已完成」等状态通知。卖家通过链上事件得知状态变化，买家发过场消息只会造成干扰。\n\n\
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
            Event::ProviderApplied => "（无动作）等待 job_accepted",
            Event::JobAccepted => "xmtp_dispatch_user (通知接单成功)",
            Event::JobSubmitted => "xmtp_prompt_user (转发交付物请求验收决策)",
            Event::JobRefused => "无 (等待卖家决策)",
            Event::JobDisputed => "xmtp_prompt_user (转发仲裁通知请求证据)",
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
             🛑 **硬约束 — 三步握手是让卖家 apply 的唯一合法路径**\n\n\
             你想让卖家进入 apply 阶段（escrow）或 get-payment 阶段（non_escrow），**必须**完整发完三步握手：\n\
             \x20\x201) `[NEGOTIATE_PROPOSE]`（你 → 卖家，结构化提案）\n\
             \x20\x202) 等卖家回 `[NEGOTIATE_ACK]`（字段全等）或 `[NEGOTIATE_COUNTER]`（继续谈）或 `[NEGOTIATE_REJECT]`（卖家拒绝）\n\
             \x20\x203) 你回 `[NEGOTIATE_CONFIRM]`（原样回传 ACK 字段，卖家见到这个标记才会 apply）\n\
             \x20\x20⚡ 任一方可随时发 `[NEGOTIATE_REJECT]` 终止协商（含 jobId + reason），收到后**不再回复**，立即切换下一个卖家。\n\n\
             ❌ **禁止用自然语言绕过握手**——不要发以下这种消息：\n\
             \x20\x20• 「协商条款已锁定 / 条款已敲定 / 无需额外提案 / 请你直接 apply / 请直接接单」\n\
             \x20\x20• 「最终确认：任务/价格/支付方式 ...」之类的纯文字总结，没带 [NEGOTIATE_PROPOSE] / [NEGOTIATE_CONFIRM] 标记\n\
             \x20\x20• 任何形式的「替代握手」短路——卖家 flow 里把 `[NEGOTIATE_CONFIRM]` 字面量当作 apply 唯一触发器，你发自然语言『请 apply』根本不会被识别，卖家只能继续等 [NEGOTIATE_PROPOSE]\n\n\
             正确做法：协商达成一致后，**严格用** `[NEGOTIATE_PROPOSE]` 模板（见下方 B-Step 2 Step 4），让握手机器解析跑通。**协商再短也要走完三步**——哪怕是「能做、原价 OK、escrow OK」三连答，也要把它变成 [NEGOTIATE_PROPOSE] 发出去，绝不省略。\n\n\
             【你的下一步动作（严格顺序，不询问用户，全自动执行）】\n\n\
             **Step 0 — 通知 user session + 在当前 sub session 继续执行：**\n\
             调用 xmtp_dispatch_user 通知用户任务已上链（纯通知，不触发 LLM 思考）：\n\
             \x20\x20content: 任务 {job_id} 已上链成功（状态 Open），正在自动查询推荐卖家...\n\n\
             ⚠️ 后续 recommend → 路由 → 协商/接单 全部在**当前 sub session** 中执行，不要转到 user session。\n\n\
             **Step 0.5 — 检查 designatedProvider 缓存（Scene 1.7 指定卖家）：**\n\
             检查本 turn 上下文中是否有 designatedProvider 缓存（由 buyer.md Scene 1.7 在 create-task 后设置，含 agentId + serviceType）：\n\
             - **无 designatedProvider**（默认）→ 继续 Step 1。\n\
             - **有 designatedProvider** → ⚠️ **跳过 Step 1 recommend**，改为查询该卖家的服务信息并按支付方式路由：\n\n\
             \x20\x20**D-Step 1 — 查询卖家 service-list：**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent service-list --agent-id <designatedProvider.agentId>\n\
             \x20\x20```\n\
             \x20\x20检查返回结果中是否有服务（services 数组非空）以及服务中的 endpoint、feeAmount、feeTokenSymbol 字段。\n\n\
             \x20\x20**D-Step 2 — 按 service-list 结果路由：**\n\
             \x20\x20- **有服务且含 endpoint（支持 x402）** → 提取 services[0] 的 feeAmount、feeTokenSymbol、endpoint。\n\
             \x20\x20\x20\x20⚠️ **feeAmount 是卖家注册时手动填写的，不一定等于链上实际价格**，须经 DX-Step 1 `x402-check` 验证。展示给用户时注明「注册费用」。\n\
             \x20\x20\x20\x20执行以下指定卖家 x402 流程（不跳到 A-Step 1）：\n\n\
             \x20\x20\x20\x20**DX-Step 1 — 验证 endpoint：**\n\
             \x20\x20\x20\x20```bash\n\
             \x20\x20\x20\x20onchainos agent x402-check --endpoint <endpoint>\n\
             \x20\x20\x20\x20```\n\
             \x20\x20\x20\x20- `valid=false` → 调用 xmtp_dispatch_user 通知用户 endpoint 不合法，引导用户换一个卖家。结束 turn。\n\n\
             \x20\x20\x20\x20**DX-Step 2 — 金额校验：**\n\
             \x20\x20\x20\x20比较 x402-check 的 `amountHuman` 与 services[0] 的 `feeAmount`：\n\
             \x20\x20\x20\x20- 不一致（差异 > 1%）→ 调用 xmtp_prompt_user 询问用户是否接受实际价格：\n\
             \x20\x20\x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] 用户回复「接受」→ 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY] 用户决策：接受\") relay 回 sub session 继续 DX-Step 3；回复「拒绝」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY] 用户决策：拒绝\") relay 回 sub session 引导换卖家。⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行 task CLI。\n\
             \x20\x20\x20\x20\x20\x20userContent: 任务 {job_id} 指定卖家（AgentID=<agentId>）实际收费 <amountHuman> <tokenSymbol>，与注册费用 <feeAmount> <feeTokenSymbol> 不一致，是否接受？\n\
             \x20\x20\x20\x20- 一致 → 继续 DX-Step 3。\n\n\
             \x20\x20\x20\x20**DX-Step 3 — 预算检查：**\n\
             \x20\x20\x20\x20比较 `amountHuman` 与任务最高预算（tokenAmount）：\n\
             \x20\x20\x20\x20- 超出 → 调用 xmtp_dispatch_user 通知用户费用超额，引导换卖家。结束 turn。\n\
             \x20\x20\x20\x20- 未超出 → 进入 **A-Step 3**（set-payment-mode + task-402-pay）。\n\n\
             \x20\x20- **无服务或无 endpoint（不支持 x402）** → 进入 **B-Step 1** 建群协商。\n\n\
             \x20\x20清除 designatedProvider 缓存。\n\n\
             **Step 1 — 查询推荐卖家：**\n\
             ```bash\n\
             onchainos agent recommend {job_id} --agent-id {agent_id}\n\
             ```\n\
             缓存完整推荐列表，记录 currentProviderIndex = 0。\n\
             输出末尾有「路由」指引，标明当前卖家是 x402 还是 A2A。\n\n\
             **Step 2 — 顺序遍历推荐列表，按 supportA2MCP 字段路由：**\n\n\
             ━━━━━━━━━ 分支 A：supportA2MCP=true → x402（无需协商，直接接单）━━━━━━━━━\n\n\
             🛑 **x402 全自动铁律（以下两条绝对禁止）：**\n\
             \x20\x201) ❌ **禁止停顿征求用户确认**：x402 路径从 A-Step 1 到 A-Step 3 必须一气呵成自动执行。不要展示卖家信息后等用户说「执行」「确认」再继续——recommend 输出只是 agent 内部决策数据，不是给用户看的确认表单。\n\
             \x20\x202) ❌ **禁止调 `confirm-accept`**：x402 接单的唯一合法路径是 `set-payment-mode` → 等 `job_payment_mode_changed` → `task-402-pay`。`confirm-accept` 是 escrow/non_escrow 专用命令，x402 调它会导致支付签名缺失、endpoint 重放缺失。\n\n\
             从 recommend 输出中提取当前 provider 的 services[0]：feeAmount、feeTokenSymbol、endpoint。\n\
             ⚠️ **feeAmount / feeTokenSymbol 是卖家注册身份时手动填写的，不一定等于链上实际最新价格。** 展示给用户时须注明「注册费用」，以 A-Step 1 `x402-check` 返回的 `amountHuman` 为链上实际费用。\n\
             从任务详情提取：tokenAmount（任务最高预算）、tokenSymbol（任务代币）。\n\n\
             **A-Step 1 — 验证 endpoint 是否是合法的 x402 服务：**\n\
             ```bash\n\
             onchainos agent x402-check --endpoint <endpoint>\n\
             ```\n\
             - `valid=false` → 跳过该卖家，执行 `recommend --next` 切换下一个。\n\
             - `valid=true` → 继续 A-Step 2。\n\n\
             **A-Step 2 — 金额 & 代币校验（三重检查）：**\n\n\
             从 x402-check 输出提取 `amountHuman`（实际服务金额）、`tokenSymbol`（实际代币）。\n\n\
             **检查 1 — 402 金额 vs 卖家注册金额：**\n\
             比较 x402-check 返回的 `amountHuman` 与 recommend 中该卖家 services[0] 的 `feeAmount`（注意单位，两者都是人类可读金额）。\n\
             - 不一致（差异 > 1%）→ 跳过该卖家，`recommend --next`。\n\n\
             **检查 2 — 代币一致性：**\n\
             比较 x402-check 的 `tokenSymbol` 与 services[0] 的 `feeTokenSymbol`。\n\
             - 不一致 → 跳过该卖家，`recommend --next`。\n\n\
             **检查 3 — 预算限额：**\n\
             比较 `amountHuman` 与任务最高预算（tokenAmount）。\n\
             - 超出预算 → 跳过该卖家，`recommend --next`。\n\n\
             三项检查全部通过 → 进入 A-Step 3。\n\n\
             **A-Step 3 — setPaymentMode（x402 阶段 1）：**\n\
             ```bash\n\
             onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <feeTokenSymbol> --token-amount <amountHuman> --endpoint <endpoint>\n\
             ```\n\
             → **结束本轮 turn**，等待 `job_payment_mode_changed` 系统通知。\n\n\
             **A-Step 3b — 支付重放（x402 阶段 2，收到 job_payment_mode_changed 后执行）：**\n\
             ```bash\n\
             onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<x402-check 输出的 acceptsJson>' --endpoint <endpoint> --token-symbol <feeTokenSymbol> --token-amount <amountHuman>\n\
             ```\n\
             输出：{{ replaySuccess, replayStatus, replayBody, signature, authorization, txHash }}\n\n\
             **A-Step 4 — 处理重放结果：**\n\
             - replaySuccess=true → 调用 xmtp_dispatch_user 将交付物发送给用户：\n\
             \x20\x20content:\n\
             \x20\x20[x402 交付物预览] 任务 {job_id} endpoint 重放成功，交付物已获取。\n\
             \x20\x20卖家 AgentID：<providerAgentId>\n\
             \x20\x20---交付物内容---\n\
             \x20\x20<replayBody 完整内容，JSON 则格式化输出>\n\
             \x20\x20---交付物结束---\n\
             \x20\x20正在等待链上确认，确认后将自动完成任务。\n\n\
             - replaySuccess=false → 调用 xmtp_dispatch_user 通知用户重放失败，等待用户指示。\n\n\
             → **结束本轮 turn**，等待 `job_accepted` 系统通知。\n\n\
             ━━━━━━━━━ 分支 B：supportA2MCP=false → A2A（需协商）━━━━━━━━━\n\n\
             **B-Step 0 — 防重复检查：**\n\
             调 `session_status` 检查当前 job 是否已有 sub session（即是否已建群）。\n\
             如果**已存在** sub session → 说明 job_created 被重复处理，**跳过建群和发消息，直接结束本轮 turn**。\n\
             如果**不存在** → 继续 B-Step 1。\n\n\
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
             \x20\x20- paymentMode：支付方式（**仅 escrow 或 non_escrow**——A2A 协商会话中禁止 x402，无论卖家是否有 endpoint）\n\
             \x20\x20- tokenSymbol：支付代币\n\
             \x20\x20- tokenAmount：支付金额\n\
             \x20\x20- deadline：交付截止时间\n\n\
             ⏱ **超时规则（negotiate-tick CLI 强制执行）**：**每次 xmtp_send 给卖家之后**，立即调：\n\
             ```bash\n\
             onchainos agent negotiate-tick {job_id} --agent-id {agent_id} --seller <卖家agentId> --event sent\n\
             ```\n\
             这会记录发送时间戳并返回 `timeoutSecs: 300` 和 `sellerAgentId`。设置 300 秒定时器；若到期卖家未回复，调 `onchainos agent next-action --jobid {job_id} --jobStatus negotiate_timeout --role buyer --agentId {agent_id} --seller <卖家agentId>` 触发超时处理（**--seller 必传**，让超时脚本精确定位是哪个卖家超时）。\n\
             超时未回复 → 发 [NEGOTIATE_REJECT] 后切换下一个卖家（**禁止 xmtp_delete_conversation**）。超时后若再收到该卖家的 a2a-agent-chat 消息，**不回复、不处理**，直接忽略。\n\
             🔢 **COUNTER 轮次限制（negotiate-tick CLI 强制执行）**：卖家最多发送 3 次 [NEGOTIATE_COUNTER]。收到 COUNTER 时先调 `negotiate-tick --event counter` 检查，超限后自动发 [NEGOTIATE_REJECT] 并切换下一个卖家。\n\n\
             ⚠️ **协商消息格式铁律**：所有协商阶段的结构化消息（PROPOSE / CONFIRM / REJECT）**必须以对应前缀标记开头**，\n\
             content 第一行必须是 `[NEGOTIATE_PROPOSE]` / `[NEGOTIATE_CONFIRM]` / `[NEGOTIATE_REJECT]`，**严禁用自然语言替代**。\n\
             卖家 Agent 通过前缀做机器解析，缺少前缀会导致协商流程卡死。\n\n\
             📌 **你有完整的协商权 —— 不要机械接受卖家任何报价**。看 context 里的【任务详情】+【卖家 profile / service-list / 历史 securityRate / feedback】，自己判断：\n\
             \x20\x20• 卖家给的价格相对任务工作量是否合理；超过你预算上限就不要勉强答应\n\
             \x20\x20• 卖家 profile / service-list 同类服务单价 vs 当前报价（卖家自己挂的价就是参考锚）\n\
             \x20\x20• 卖家 paymentMode 偏好（escrow / non_escrow）跟你需求是否匹配（金额大 / 不熟卖家 → 坚持 escrow；熟悉/小额 → 可让步 non_escrow）\n\
             \x20\x20• 多个推荐卖家的话，不要勉强跟某一个谈拢；不合适直接切下一个（超时 / COUNTER 超限 / 主动 REJECT 都可以）\n\n\
             协商步骤：\n\
             1. 调用 xmtp_send 发送第一条询盘消息（自然语言，不要把 budget 数字直接抛给卖家——让卖家先给报价，你再判断）：\n\
             \x20\x20content=<任务描述 + 期望交付物 + paymentMode 倾向 + deadline，**先不暴露上限价**>\n\
             \x20\x20→ 等待卖家回复（300 秒超时，由 negotiate-tick 管控）\n\
             2. （sub session 内）卖家回复报价（金额、代币、支付方式偏好、预计交付时间）\n\
             3. （sub session 内）双方就价格/条件进行调整（可能多轮，每轮 300 秒超时，最多 3 次 COUNTER）\n\
             \x20\x20每轮调用 xmtp_send，参数：sessionKey=<同上>，content=<协商内容>\n\
             \x20\x20⚠️ **不要机械接受卖家加价**：以**任务的 max_budget（最高预算）为绝对上限**——超过 max_budget 一律拒绝，不论差多少。max_budget 从 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 的 `paymentMostTokenAmount` 字段获取。`budget < 卖家价 ≤ max_budget` 区间内可谈，可以原价接受或继续还价；卖家价 ≤ budget 直接接受。\n\
             ⚠️ **币种铁律**：协商只允许改**金额**，不允许改**币种**。任务发布时的币种（从 `onchainos agent common context` 获取）\n\
             是链上合约绑定的。如果卖家提出不同币种，必须纠正：「本任务使用 <任务币种>，请用 <任务币种> 报价。」\n\n\
             ⚠️ 任一步骤卖家 300 秒未回复 → negotiate-tick 判定超时，发 [NEGOTIATE_REJECT] 后切换下一个卖家（**不删群**）。超时后再收到该卖家消息一律忽略、不回复。\n\n\
             4. 达成初步一致后，调用 xmtp_send 发送 **[NEGOTIATE_PROPOSE]** 结构化提案（必须严格使用此格式，卖家 Agent 会机器解析）：\n\
             \n\
             📋 **填字段前必做的口头记录自检（防止『记忆穿越』）**：\n\
             \x20\x20在写 [NEGOTIATE_PROPOSE] 任何字段前，**逐字段从最近一条往前回看本 sub session 的所有 xmtp_send 内容**，找到**最后一次双方明确同意的值**：\n\
             \x20\x20- tokenAmount：以**最后一次自然语言达成的价格**为准（不是任务原始预算、不是 recommend 列表里的标价、不是中间任意一轮的报价）\n\
             \x20\x20- paymentMode / deadline / deliverable / qualityStandards：同样取最后一次共识\n\
             \x20\x20- 任一字段在对话里没有明确共识 → **不要发 [NEGOTIATE_PROPOSE]**，先 xmtp_send 自然语言再确认一次\n\
             \x20\x20⚠️ 不要凭印象直接填——你的训练数据里没有本次会话的记忆，唯一可靠来源是回看本 sub session 的消息历史。\n\n\
             \x20\x20content=\n\
             [NEGOTIATE_PROPOSE]\n\
             jobId: {job_id}\n\
             deliverable: <交付物描述>\n\
             qualityStandards: <验收标准>\n\
             paymentMode: <escrow|non_escrow>\n\
             tokenSymbol: <USDT|USDG>\n\
             tokenAmount: <金额>\n\
             deadline: <交付截止时间>\n\n\
             ⚠️ 发完 PROPOSE 后别忘了调 `negotiate-tick --event sent`（上面的超时规则），然后等待卖家回复。\n\n\
             5. **等待卖家回复 [NEGOTIATE_ACK] 或 [NEGOTIATE_COUNTER]**（300 秒超时，由 negotiate-tick 定时器管控）：\n\n\
             \x20\x20▸ 收到 **[NEGOTIATE_ACK]** → 逐字段校验卖家回传的值与你发送的 PROPOSE 完全一致：\n\
             \x20\x20\x20\x20- 全部一致 → **先做完 Step 6 落盘 + setPaymentMode 后**才发 [NEGOTIATE_CONFIRM]（卖家见 [NEGOTIATE_CONFIRM] 立刻 apply，所以 paymentMode 必须先在链上就位）。模板（**先按此格式准备好 content，但暂不发送**）：\n\
             \x20\x20\x20\x20\x20\x20content=\n\
             \x20\x20\x20\x20\x20\x20[NEGOTIATE_CONFIRM]\n\
             \x20\x20\x20\x20\x20\x20jobId: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20deliverable: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20qualityStandards: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20paymentMode: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20tokenSymbol: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20tokenAmount: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20deadline: <与 ACK 完全相同>\n\
             \x20\x20\x20\x20\x20\x20→ 立即转 Step 6（落盘 + 视情况 setPaymentMode），按 Step 6 分支决定**何时**发 [NEGOTIATE_CONFIRM]\n\
             \x20\x20\x20\x20- 任一字段不一致 → 视为篡改，调 xmtp_send 告知卖家字段不一致并重新发送 [NEGOTIATE_PROPOSE]\n\n\
             \x20\x20▸ 收到 **[NEGOTIATE_COUNTER]** → **先调 negotiate-tick 检查计数器**：\n\
             \x20\x20\x20\x20```bash\n\
             \x20\x20\x20\x20onchainos agent negotiate-tick {job_id} --agent-id {agent_id} --seller <卖家agentId> --event counter\n\
             \x20\x20\x20\x20```\n\
             \x20\x20\x20\x20检查输出 `action` 字段：\n\
             \x20\x20\x20\x20- `action: \"counter_exceeded\"` → **不处理 COUNTER 内容**，直接 xmtp_send 发送 `[NEGOTIATE_REJECT]`（reason: 协商轮次超限，已达 3 次 COUNTER），调 `recommend --next` 切换\n\
             \x20\x20\x20\x20- `action: \"timeout\"` → **不处理 COUNTER 内容**，直接 xmtp_send 发送 `[NEGOTIATE_REJECT]`（reason: 协商超时），调 `recommend --next` 切换\n\
             \x20\x20\x20\x20- `action: \"continue\"` → 正常处理 COUNTER（`remaining` 字段显示剩余轮次）\n\n\
             \x20\x20\x20\x20卖家提出反提案，**带价值判断决定接不接，不要机械接受**：\n\
             \x20\x20\x20\x20⚠️ **第 0 步：先回看 sub session 历史，确认你刚才发的 [NEGOTIATE_PROPOSE] 是否填错了**：\n\
             \x20\x20\x20\x20\x20\x20· 回看自然语言协商最后一次明确同意的金额 / paymentMode / deadline\n\
             \x20\x20\x20\x20\x20\x20· 如果 COUNTER 的金额**等于**自然语言里你最后同意的那个数 → **是你 PROPOSE 写错了，不是卖家加价**：直接用 COUNTER 的金额重发新 [NEGOTIATE_PROPOSE]，**不要再讨价还价**也不要嘴硬说『我们之前是 X』，直接修正即可\n\
             \x20\x20\x20\x20\x20\x20· 如果 COUNTER 的金额**高于**自然语言里你最后同意的数 → 才是卖家加价，按下方决策矩阵处理\n\n\
             \x20\x20\x20\x20- 检查 tokenSymbol 是否被改动（禁止改币种）→ 如被改动，拒绝并纠正\n\
             \x20\x20\x20\x20- 评估 tokenAmount（**max_budget 优先，不是百分比**）：\n\
             \x20\x20\x20\x20\x20\x20· COUNTER 价 ≤ 任务 budget（原预算）→ 可接受，用 COUNTER 值发新 [NEGOTIATE_PROPOSE]\n\
             \x20\x20\x20\x20\x20\x20· budget < COUNTER 价 ≤ max_budget（最高预算）→ 可接受，或继续还价取折中（带理由发新 [NEGOTIATE_PROPOSE]）\n\
             \x20\x20\x20\x20\x20\x20· COUNTER 价 > max_budget → 调 xmtp_send 发送 `[NEGOTIATE_REJECT]` 结束协商，然后**立即** `recommend --next` 切换下一个卖家：\n\
             \x20\x20\x20\x20\x20\x20\x20\x20content=\n\
             \x20\x20\x20\x20\x20\x20\x20\x20[NEGOTIATE_REJECT]\n\
             \x20\x20\x20\x20\x20\x20\x20\x20jobId: {job_id}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20reason: 报价超出最高预算\n\
             \x20\x20\x20\x20\x20\x20· max_budget 不知道 → 调 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 取 `paymentMostTokenAmount` 字段\n\
             \x20\x20\x20\x20- 评估 paymentMode 改动：卖家把 escrow 改成 non_escrow 且任务金额不小 → 拒绝，坚持 escrow\n\
             \x20\x20\x20\x20- 评估 deadline 改动：卖家拉长是否影响你交付计划 → 不可接受就还价或切换\n\
             \x20\x20\x20\x20- 全部可接受 → 用 COUNTER 中的值发新的 [NEGOTIATE_PROPOSE]，回到 Step 5 等 ACK\n\n\
             \x20\x20▸ 收到 **[NEGOTIATE_REJECT]** → 卖家主动拒绝协商。**不再回复**，立即 `recommend --next` 切换下一个卖家。\n\n\
             \x20\x20▸ 收到的回复**不含** [NEGOTIATE_ACK] / [NEGOTIATE_COUNTER] / [NEGOTIATE_REJECT] 标记 → 视为自然语言讨论，继续协商，重新回到 Step 4\n\n\
             6. **收到 [NEGOTIATE_ACK] 全等 → 落盘 + setPaymentMode → 最后才发 [NEGOTIATE_CONFIRM]**：\n\n\
             🛑 **顺序铁律（[NEGOTIATE_CONFIRM] 是卖家 apply 的唯一触发器，必须 paymentMode 在链上就位后才发，否则卖家 apply 会基于错的支付状态）**：\n\n\
             **Step 6.1 — save-agreed 落盘**（无条件第一步）：\n\
             ```bash\n\
             onchainos agent save-agreed {job_id} --provider <当前协商的providerAgentId> --token-symbol <协商币种> --token-amount <协商价格>\n\
             ```\n\
             不保存会导致后续 confirm-accept 使用错误的币种/金额。\n\n\
             **Step 6.2 — 执行 setPaymentMode（无条件，不判断当前链上值）**：\n\
             ⚠️ **不论链上 paymentType 当前是什么值（0 / 1 / 2 / 3），都必须执行 set-payment-mode。** 不要查 common context 比较——直接调：\n\
             ⚠️ **A2A 协商会话中禁止 x402**：无论卖家是否有 endpoint，协商会话中只能选 escrow 或 non_escrow。此处 set-payment-mode 会覆盖链上值。\n\n\
             ```bash\n\
             onchainos agent set-payment-mode {job_id} --payment-mode <escrow|non_escrow> --token-symbol <协商币种> --token-amount <协商价格>\n\
             ```\n\
             此命令执行 setPaymentMode → 签名 → 广播，然后返回 exit code 2 (confirming)。\n\
             ⚠️ **绝对不要**在此 turn 内 xmtp_send [NEGOTIATE_CONFIRM]——卖家见 [NEGOTIATE_CONFIRM] 会立刻 apply，但链上 paymentMode 还在 mempool / 没确认，apply 会失败或行为错位。[NEGOTIATE_CONFIRM] 必须等 `job_payment_mode_changed` 事件确认 paymentMode 上链后再发。\n\n\
             **Step 6.3 — 结束本轮 turn**，等待 `job_payment_mode_changed` 系统通知。\n\n\
             （新一 turn）收到 `job_payment_mode_changed` → 调 next-action --jobStatus job_payment_mode_changed → 按剧本 xmtp_send [NEGOTIATE_CONFIRM] 给卖家。卖家此时见 CONFIRM → apply（escrow）或 create_payment_charge（non_escrow），链上 paymentMode 已就位。\n\n\
             ━━━━━━━━━ 遍历结束 / 切换下一个卖家 ━━━━━━━━━\n\n\
             当前卖家 negotiate-tick 判定超时 / COUNTER 超限 / 收到 `[NEGOTIATE_REJECT]` / 协商失败 → 先调 `negotiate-tick --event reject` 记录终止状态，再调 `onchainos agent recommend {job_id} --next` 切换下一个卖家，重新回到 Step 2 路由判断。\n\
             ⚠️ **超时/超限切换时先发 [NEGOTIATE_REJECT] 给卖家**（reason 填超时/超限原因），然后不再发任何消息。不要 xmtp_delete_conversation 删群。超时后再收到该卖家消息一律忽略、不回复。\n\
             推荐列表全部遍历完（或初始推荐列表为空）→ 先调 `session_status` 拿 sessionKey；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7);再调用 xmtp_prompt_user 引导用户选择：\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
             用户选择 A 并提供 agentId → 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY] 用户决策：指定卖家 agentId=<用户提供的agentId>\") relay 回 sub session，sub agent 查 service-list 后路由（x402 或建群协商）；\
             用户选择 B → 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY] 用户决策：转为公开任务\") relay 回 sub session 执行 set-public；\
             用户选择 C → 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY] 用户决策：关闭任务\") relay 回 sub session 执行 close。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行 task CLI。\n\
             \x20\x20userContent: [任务 {short_id} 你作为买家] 推荐卖家已全部遍历，无合适匹配。请选择下一步：\n\
             \x20\x20A. 指定卖家 — 请提供卖家 agentId\n\
             \x20\x20B. 转为公开任务 — 让更多卖家看到任务\n\
             \x20\x20C. 关闭任务 — 取消并退款\n\
             \x20\x20→ **结束本轮 turn**，等用户回复 relay 回来后继续执行。\n\n\
             【后续事件】\n\
             - x402 → set-payment-mode → job_payment_mode_changed → task-402-pay（签名 + direct/accept + endpoint 重放）→ job_accepted → complete\n\
             - escrow → set-payment-mode → job_payment_mode_changed → 通知卖家 apply → 卖家 apply 上链 → 卖家 xmtp_send 通知买家 → 买家收到 a2a-agent-chat → confirm-accept → job_accepted\n\
             - non_escrow → set-payment-mode → job_payment_mode_changed → [NEGOTIATE_CONFIRM] + confirm-accept（此步只接单不支付）→ job_accepted → 等卖家交付 + paymentId → 支付 + complete → job_completed\n"
        ),

        // ─── provider_applied（卖家已 apply）──────────────────
        // 触发来源：buyer.md 路由优先级 #2（卖家 XMTP 消息告知已 apply）调 next-action --jobStatus provider_applied。
        // ⚠️ 买家不会收到 provider_applied 系统通知，此剧本仅由 a2a-agent-chat 路由触发。
        Event::ProviderApplied => format!(
            "【当前状态】provider_applied（卖家已链上申请接单）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 获取任务信息：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取：providerAgentId、paymentMode、tokenSymbol、tokenAmount。\n\
             ⚠️ paymentMode 此时应为 escrow（1）——non_escrow 不走 apply 流程。\n\n\
             **Step 2 — 执行 confirm-accept（确认接单上链）：**\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider-agent-id <providerAgentId> --payment-mode escrow --token-symbol <tokenSymbol> --token-amount <tokenAmount>\n\
             ```\n\
             ⚠️ 参数是 `--provider-agent-id`，不是 `--agent-id`。\n\
             ⚠️ **不要查询任务 API 验证卖家是否已 apply**——链上索引有延迟，`confirm-accept` 内部会做链上校验。\n\n\
             → 执行后**结束本轮 turn**，等待 `job_accepted` 系统通知。\n"
        ),

        // ─── job_accepted: 按支付方式分流（非担保立即 complete，担保等交付）──────────────────
        Event::JobAccepted => format!(
            "【当前状态】job_accepted（买家已确认接单，任务进入执行阶段）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**（如「已确认接单」「资金已托管」等），卖家通过链上事件得知状态变化。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 获取任务完整信息：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取：{title_in_extract}description、deliverable、providerAgentId、paymentMode（int：1=escrow, 2=non_escrow, 3=x402）、tokenAmount、tokenSymbol。\n\n\
             **Step 2 — 按支付方式分流：**\n\n\
             ━━━━━━━━━ 分支 A：escrow（担保）━━━━━━━━━\n\n\
             调用 xmtp_dispatch_user 通知用户接单成功：\n\
             \x20\x20content:\n\
             \x20\x20[接单成功] 任务 {job_id} 已确认接单，进入执行阶段。\n\
             \x20\x20任务标题：{title_display}\n\
             \x20\x20任务描述：<description>\n\
             \x20\x20交付物：<deliverable>\n\
             \x20\x20卖家 AgentID：<providerAgentId>\n\
             \x20\x20支付方式：escrow（担保）\n\
             \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
             \x20\x20等待卖家执行并提交交付物。\n\n\
             【后续事件】\n\
             - job_submitted → 验收交付物\n\n\
             ━━━━━━━━━ 分支 B：non_escrow（非担保 — 先交付后支付）━━━━━━━━━\n\n\
             ⚠️ 非担保流程：接单后**等待卖家交付并发送 paymentId**，买家收到交付物 + paymentId 后才支付。\n\n\
             **B-Step 1 — 通知用户接单成功，等待卖家交付：**\n\
             调用 xmtp_dispatch_user：\n\
             \x20\x20content:\n\
             \x20\x20[接单成功] 任务 {job_id} 已确认接单（非担保），等待卖家交付。\n\
             \x20\x20任务标题：{title_display}\n\
             \x20\x20交付物：<deliverable>\n\
             \x20\x20卖家 AgentID：<providerAgentId>\n\
             \x20\x20支付方式：non_escrow（先交付后支付）\n\
             \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
             \x20\x20卖家交付后会发送 paymentId，届时自动完成支付。\n\n\
             ⚠️ **不要在此 turn 执行 complete**——买家尚未收到交付物和 paymentId。\n\
             → **结束本轮 turn**，等待卖家 XMTP 消息。\n\n\
             **B-Step 2 — （下一 turn）收到卖家 XMTP 交付物 + paymentId 后执行：**\n\
             卖家会通过 sub session 发送消息，包含 paymentId（格式：`paymentId: a2a_xxx` 或消息中含 `a2a_` 前缀的 ID）+ 交付物内容。\n\
             收到后：\n\
             \x20\x201. 提取 paymentId 和交付物内容\n\
             \x20\x202. 执行支付 + complete：\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent complete {job_id} --payment-id <paymentId> --token-symbol <tokenSymbol> --token-amount <tokenAmount>\n\
             \x20\x20```\n\
             \x20\x20（内部：a2a_pay 支付 → direct/complete → 签名 → 广播）\n\
             \x20\x203. 支付成功后调用 xmtp_dispatch_user 通知用户：\n\
             \x20\x20content:\n\
             \x20\x20[支付完成] 任务 {job_id} 已收到交付物并完成支付。\n\
             \x20\x20---交付物内容---\n\
             \x20\x20<交付物完整内容>\n\
             \x20\x20---交付物结束---\n\
             \x20\x20支出：<tokenAmount> <tokenSymbol>\n\
             \x20\x20本任务流程结束。如需评价卖家，请回复「评价」。\n\n\
             【后续事件】\n\
             - 卖家发送交付物 + paymentId（XMTP 消息）→ 买家支付 + complete → job_completed\n\n\
             ━━━━━━━━━ 分支 C：x402 ━━━━━━━━━\n\n\
             ⚠️ 回顾本会话上一轮 turn 中 `task-402-pay` 命令的 JSON 输出（该命令在 job_payment_mode_changed 事件处理时执行），\n\
             从中提取 `replaySuccess`、`replayBody`、`replayStatus` 等字段：\n\n\
             **C-分支 1：replaySuccess=true（重放成功，交付物已获取）**\n\n\
             **C-Step 1 — 执行 complete（单签）：**\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             （内部：POST /priapi/v1/aieco/task/{job_id}/direct/complete → 获取 calldata → 签名 uopHash → 广播上链）\n\n\
             ⚠️ **不要通知用户**——交付物已在 task-402-pay 后（A-Step 4）发送过，最终汇总由 job_completed 事件负责。\n\n\
             **C-分支 2：replaySuccess=false（重放失败，未获取交付物）**\n\n\
             ⚠️ **不要执行 complete**——买家未收到交付物，不能完成支付。\n\n\
             **C-Step 1 — 通知用户重放失败：**\n\
             调用 xmtp_dispatch_user：\n\
             \x20\x20content:\n\
             \x20\x20[x402 重放失败] 任务 {job_id} 已接单但 endpoint 重放失败。\n\
             \x20\x20HTTP 状态：<replayStatus>\n\
             \x20\x20错误信息：<replayBody>\n\
             \x20\x20任务已进入 accepted 状态，等待进一步处理。\n\n\
             【后续事件】\n\
             - replaySuccess=true: job_completed → 最终确认\n\
             - replaySuccess=false: 等待用户指示（可重试或关闭任务）\n"
        ),

        // ─── Scene 7: 卖家提交交付物，下载 + 验收（区分支付方式） ─────────
        Event::JobSubmitted => format!(
            "【当前状态】job_submitted（卖家已提交交付物）\n\
             【角色】买家（Client）\n\n\
             🚫 **担保模式严禁自动验收**：escrow 模式下收到交付物后**必须通知 user session，由用户决定验收通过还是拒绝**。\n\
             Agent 不得替用户做验收决策，即使交付物看起来符合验收标准。\n\
             ⚠️ non_escrow / x402 模式：资金已支付，只需通知用户交付物内容，用户不能拒绝。\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**（如「收到交付物」「正在验收」等过场话），直接执行下述步骤即可。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 查询任务详情，提取交付物和支付方式：**\n\
             ```bash\n\
             onchainos agent status {job_id}\n\
             ```\n\
             提取 `deliverableUrl`、`qualityStandards` 和 `paymentMode`（int：1=escrow, 2=non_escrow, 3=x402）。\n\n\
             **Step 2 — 获取交付物内容（区分文字 vs 文件）：**\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey（后续 Step 3 复用，同 turn 不再重复调）。\n\
             再调 `xmtp_get_conversation_history`（sessionKey = 上一步拿到的 sessionKey）拉取与卖家的聊天记录，\n\
             找到卖家发送的**最近一条交付物消息**（通常是最后一条或倒数几条中包含文件元数据或交付说明的消息），判断交付物类型：\n\n\
             ━━━ 情况 A：交付物是文件（消息包含 fileKey / digest / salt / nonce / secret 等加密元数据）━━━\n\n\
             调用 xmtp_file_download 工具下载文件：\n\
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
             下载成功后记录 localPath，**必须是完整绝对路径**（如 /Users/xxx/Downloads/task预发.png）。\n\
             ⚠️ **严禁只显示文件名**（如 cat-picture.png），用户无法定位文件。后续所有展示给用户的内容必须包含完整路径。\n\
             如果下载失败 → 用 deliverableUrl 作为备用展示信息。\n\
             ⚠️ 如果卖家消息除文件外还包含文字说明（如「这是交付物，请查收」），一并记录到 deliverableText。\n\
             交付物展示变量：deliverableType=file, localPath=<完整路径>, deliverableText=<文字说明，无则留空>\n\n\
             ━━━ 情况 B：交付物是纯文字（消息不含加密元数据，直接是文本内容）━━━\n\n\
             直接提取卖家消息中的文字内容，**完整保留原文**，不要截断或概括。\n\
             交付物展示变量：deliverableType=text, deliverableText=<卖家发送的完整文字内容>\n\n\
             **Step 3 — 按支付方式分流：**\n\n\
             ━━━━━━━━━ 分支 A：escrow（担保）— 需要用户验收决策 ━━━━━━━━━\n\n\
             调用 xmtp_prompt_user 把交付物和验收决策请求推给用户（sessionKey 复用 Step 2 已获取的值;调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`,见硬规则 7）：\n\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
             如果用户回复「验收通过」→ 调用 xmtp_dispatch_session(sessionKey=\"<Step 2 session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY] 用户决策：验收通过\") relay 回 sub session 执行 complete；\
             如果用户回复「拒绝，原因是...」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY] 用户决策：拒绝，原因是<用户原话>\") relay 回 sub session 执行 reject。\
             ⚠️ relay 必须使用 xmtp_dispatch_session 工具（不要用 sessions_send，它有 session tree 限制）。禁止 user session agent 自己执行 task CLI。\n\
             \x20\x20\x20\x20userContent（按 deliverableType 分,首行务必带 `[任务 {short_id} 你作为买家]` 前缀）：\n\n\
             \x20\x20\x20\x20▸ deliverableType=file：\n\
             \x20\x20\x20\x20[任务 {short_id} 你作为买家] 卖家已提交交付物（文件），已下载到本地。\n\
             \x20\x20\x20\x20📁 交付物文件路径：<localPath>（⚠️ 必须是完整绝对路径，如 /Users/xxx/Downloads/task预发.png，严禁只写文件名）\n\
             \x20\x20\x20\x20<如果 deliverableText 非空，追加：卖家说明：<deliverableText>>\n\
             \x20\x20\x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20\x20\x20验收标准：<qualityStandards>\n\
             \x20\x20\x20\x20支付方式：escrow（担保）\n\
             \x20\x20\x20\x20请选择：\n\
             \x20\x20\x20\x201. 验收通过 → 回复「验收通过」\n\
             \x20\x20\x20\x202. 拒绝 → 回复「拒绝，原因是<原因>」\n\n\
             \x20\x20\x20\x20▸ deliverableType=text：\n\
             \x20\x20\x20\x20[任务 {short_id} 你作为买家] 卖家已提交交付物（文字）。\n\
             \x20\x20\x20\x20---交付物内容---\n\
             \x20\x20\x20\x20<deliverableText 完整原文，不截断不概括>\n\
             \x20\x20\x20\x20---交付物结束---\n\
             \x20\x20\x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20\x20\x20验收标准：<qualityStandards>\n\
             \x20\x20\x20\x20支付方式：escrow（担保）\n\
             \x20\x20\x20\x20请选择：\n\
             \x20\x20\x20\x201. 验收通过 → 回复「验收通过」\n\
             \x20\x20\x20\x202. 拒绝 → 回复「拒绝，原因是<原因>」\n\n\
             **Step 4（escrow）— 等用户回复 relay 回来**，按用户决策执行：\n\
             收到 `[USER_DECISION_RELAY] 用户决策：...` 后（由 user session 通过 xmtp_dispatch_session 发回），按关键词执行：\n\n\
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
             🛑 **complete CLI 成功后禁止 xmtp_dispatch_user / xmtp_prompt_user 通知用户**——\n\
             链上确认后会收到 `job_completed` 系统事件，由该事件的剧本统一发完成通知，\n\
             此处提前发会导致用户收到重复卡片。记住 CLI 输出中的 txHash，后续 `job_completed` 剧本会用到。\n\n\
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
             ━━━━━━━━━ 分支 B：x402 — 通知用户交付物内容（不可拒绝） ━━━━━━━━━\n\n\
             ⚠️ x402 流程中资金已在 job_accepted 阶段支付，用户**不能拒绝交付物**，只需通知。\n\
             ⚠️ **非担保（non_escrow）不会收到 job_submitted 事件**——非担保的交付物 + paymentId 通过 XMTP 消息在 job_accepted 阶段处理。\n\n\
             **B-Step 1 — 调用 xmtp_dispatch_user 通知用户收到交付物（按 deliverableType 分）：**\n\n\
             \x20\x20▸ deliverableType=file：\n\
             \x20\x20content:\n\
             \x20\x20[交付物已收到] 任务 {job_id} 卖家已提交交付物（x402 模式，资金已支付）。\n\
             \x20\x20📁 交付物文件路径：<localPath>（⚠️ 必须是完整绝对路径，如 /Users/xxx/Downloads/task预发.png，严禁只写文件名）\n\
             \x20\x20<如果 deliverableText 非空，追加：卖家说明：<deliverableText>>\n\
             \x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20验收标准：<qualityStandards>\n\n\
             \x20\x20▸ deliverableType=text：\n\
             \x20\x20content:\n\
             \x20\x20[交付物已收到] 任务 {job_id} 卖家已提交交付物（x402 模式，资金已支付）。\n\
             \x20\x20---交付物内容---\n\
             \x20\x20<deliverableText 完整原文，不截断不概括>\n\
             \x20\x20---交付物结束---\n\
             \x20\x20交付物地址：<deliverableUrl>\n\
             \x20\x20验收标准：<qualityStandards>\n\n\
             **B-Step 2 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             ⚠️ **不要自动评价**——在通知末尾引导用户自行评价：「如需评价卖家，请回复「评价」。」\n\
             任务完整结束。\n\n\
             【后续事件】\n\
             - escrow: job_completed → 任务完成 / job_refused → 等待卖家决定仲裁或退款\n\
             - x402: 流程已结束\n"
        ),

        // ─── job_refused: 买家已拒绝，等待卖家决策 ─────────────────
        Event::JobRefused =>
            "【当前状态】job_refused（买家已拒绝交付物，等待卖家决定）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**，静默等待即可。\n\
             无需执行 CLI 命令。卖家有 24h 决定：\n\
             - 发起仲裁 → 你将收到 job_disputed\n\
             - 同意退款 → 你将收到 job_refunded\n\
             - 24h 超时 → 系统自动退款，你将收到 job_refunded\n\n\
             【后续事件】\n\
             - job_disputed → 提交买家证据（Scene 6）\n\
             - job_refunded → 退款完成\n".to_string()
        ,

        // ─── Scene 6: 仲裁已发起，提交买家证据 ─────────────────────
        Event::JobDisputed => format!(
            "【当前状态】job_disputed（仲裁已发起，进入 1 小时证据准备期）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**，仲裁期间双方通过链上证据交互，不通过消息。\n\
             ⚠️ **证据内容必须由用户决策**——sub agent 不知道用户手上有什么证据（截图、聊天记录、交付物链接等），\n\
             不要凭空编造证据摘要直接调 `dispute upload`。**先把决策请求推到 user session 让用户拍板**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 调用 xmtp_prompt_user 把证据决策请求推给用户：**\n\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
             用户回复证据后，调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY] 用户证据：<用户提供的证据内容>\") relay 回 sub session 执行 dispute upload。⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行 task CLI。1 小时内必须提交。\n\
             \x20\x20\x20\x20userContent:\n\
             \x20\x20\x20\x20[任务 {short_id} 你作为买家] 仲裁已上链，需要在 1 小时内提交链下证据。请提供：\n\
             \x20\x20\x20\x20- 文字摘要（必填）：说明交付物不达标的关键证据点\n\
             \x20\x20\x20\x20- 图片路径（可选）：截图、聊天记录等本地文件路径\n\
             \x20\x20\x20\x20回复格式示例：『证据：交付物缺少 X/Y/Z；图片：/path/to/screenshot.png』\n\n\
             **Step 2 — 等用户回复 relay 回来**：收到 `[USER_DECISION_RELAY] 用户证据：...` 后，调 `next-action --jobStatus dispute_evidence` 拿上传剧本。\n\n\
             ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
             跑完 Step 1-2 → **结束本轮 turn**，等用户回复。\n"
        ),

        // ─── dispute_evidence: 用户提供了证据，执行上传（伪 event）─────
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】上传仲裁证据\n\
             【角色】买家（Client）\n\n\
             **Step 1 — 从 relay 进来的用户消息中提取证据内容：**\n\
             - 文字摘要 → 用户提供的部分\n\
             - 图片路径（如果用户提供了）→ `--image` 参数\n\
             text 和 image **至少一项**。\n\n\
             **Step 2 — 拉本 sub session 协商 / 交付聊天记录，作为客观证据附在 text 头部：**\n\
             调 `xmtp_get_conversation_history`（sessionKey = 本 sub session 的 sessionKey），拿到与卖家的全部 a2a-agent-chat 历史。\n\
             把历史按下面这种**结构化分段**拼到 `--text` 字段最前面（仲裁员是 LLM，会通读 text 字段判断），后面再贴用户摘要：\n\n\
             ```\n\
             ==== 协商 / 交付聊天记录（从 xmtp_get_conversation_history 拉取） ====\n\
             [时间] 卖家(<agentId>): ...\n\
             [时间] 买家(<agentId>): ...\n\
             ...（按时间顺序，关键节点：报价 / NEGOTIATE_PROPOSE / NEGOTIATE_ACK / NEGOTIATE_CONFIRM / 交付物消息）\n\n\
             ==== 用户证据摘要 ====\n\
             <用户原话摘要>\n\
             ```\n\n\
             ⚠️ **`--text` 上限 16 KB**——聊天记录过长就**只保留**关键节点（PROPOSE / ACK / CONFIRM / 交付物 / 双方关键争议点），开头标注「（已截取关键节点）」；不要随便丢前 N 条机械截断。\n\n\
             **Step 3 — 调用 CLI 上传证据（链下 multipart）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<聊天记录 + 用户摘要 拼接后的完整 text>\" --image <用户提供的图片路径或省略>\n\
             ```\n\
             text 和 image **至少一项**；图片可省略整个 `--image` 段，不要给空字符串。\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**（如「证据已提交」），卖家通过链上事件得知。\n\n\
             【后续事件】\n\
             - job_completed → 仲裁卖家胜诉，任务完成\n\
             - job_refunded → 仲裁买家胜诉，退款\n\n\
             跑完 Step 1-3 → **结束本轮 turn，不要 xmtp_dispatch_user / xmtp_prompt_user 推 main**。\n"
        ),

        // ─── 任务完成（按支付方式分流） ─────────────────────────────────
        Event::JobCompleted => format!(
            "【当前状态】job_completed（任务支付链路完成）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**，只需通知 user session。\n\n\
             **Step 1 — 获取任务信息和支付方式：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取：{title_in_extract}tokenAmount、tokenSymbol、paymentMode（int：1=escrow, 2=non_escrow, 3=x402）。\n\n\
             **Step 2 — 按支付方式分流：**\n\n\
             ━━━━━━━━━ 分支 A：escrow（担保）— 流程结束 ━━━━━━━━━\n\n\
             担保模式下 job_completed 意味着卖家已交付且买家已验收，资金从合约释放给卖家。\n\n\
             **A-Step 1 — 调用 xmtp_dispatch_user 告知用户任务完成：**\n\
             ⚠️ txHash：从本 sub session 上下文中找到之前 `onchainos agent complete` CLI 输出的 txHash（格式 0x...）。\n\
             如果上下文中没有（如 auto-complete 等非主动验收场景），省略链上凭证行即可。\n\
             content：\n\
             \x20\x20\x20\x20[任务完成] **{title_display}**（{job_id}）已验收通过，资金已释放给卖家。\n\
             \x20\x20\x20\x20  - 支出：**<tokenAmount> <tokenSymbol>**\n\
             \x20\x20\x20\x20  - 支付方式：**escrow（担保支付）**\n\
             \x20\x20\x20\x20  - 链上凭证：<txHash>（来自 complete CLI 输出）\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **A-Step 2 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             ⚠️ **不要自动评价**——在通知末尾引导用户自行评价：「如需评价卖家，请回复「评价」。」\n\
             任务完整结束。\n\n\
             ━━━━━━━━━ 分支 B：non_escrow（非担保）— 流程结束 ━━━━━━━━━\n\n\
             ⚠️ 非担保模式下 job_completed 意味着买家已收到交付物并完成支付，任务**已到达终态**。\n\n\
             **B-Step 1 — 调用 xmtp_dispatch_user 告知用户任务完成：**\n\
             content：\n\
             \x20\x20\x20\x20[任务完成] **{title_display}**（{job_id}）已完成，交付物已收到，支付已完成。\n\
             \x20\x20\x20\x20  - 支出：**<tokenAmount> <tokenSymbol>**\n\
             \x20\x20\x20\x20  - 支付方式：**非担保（non_escrow）**\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **B-Step 2 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             ⚠️ **不要自动评价**——在通知末尾引导用户自行评价：「如需评价卖家，请回复「评价」。」\n\
             任务完整结束。\n\n\
             ━━━━━━━━━ 分支 C：x402 — 最终汇总 ━━━━━━━━━\n\n\
             ⚠️ x402 模式下 job_completed 意味着支付链路（accept + complete）已完成上链。\n\
             交付物已在 task-402-pay 阶段（A-Step 4）发送给用户，此处只做最终汇总。\n\n\
             **C-Step 1 — 调用 xmtp_dispatch_user 发送最终汇总：**\n\
             content：\n\
             \x20\x20\x20\x20[x402 任务完成] **{title_display}**（{job_id}）全部流程已完成。\n\
             \x20\x20\x20\x20  - 支出：**<tokenAmount> <tokenSymbol>**\n\
             \x20\x20\x20\x20  - 支付方式：**x402**\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20如需评价卖家，请回复「评价」。\n\n\
             **C-Step 2 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             任务完整结束。\n"
        ),

        // ─── 仲裁结束（DisputeSettled） ─────────────────────────────
        Event::DisputeResolved => format!(
            "【当前状态】dispute_resolved（仲裁已裁决）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**，只需通知 user session。\n\n\
             **Step 1 — 判定胜负**：从系统通知 envelope 里读 `message.jobStatus` 字段：\n\
             - `jobStatus = \"rejected\"` → **买家胜诉**\n\
             - `jobStatus = \"complete\"` → **买家败诉**\n\
             - 其他值（如 `disputed`）→ 无法直接判定，执行 Step 1.5 查询任务详情\n\n\
             **Step 1.5（仅 jobStatus 非 rejected/complete 时）— 查询任务详情获取实际状态：**\n\
             ```bash\n\
             onchainos agent status {job_id}\n\
             ```\n\
             从返回的 `jobStatus` 字段判断：`rejected` = 买家胜诉，`complete` = 买家败诉。\n\n\
             **Step 2 — 获取任务信息：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
             ```\n\
             提取 {title_in_extract}tokenAmount、tokenSymbol。\n\n\
             **Step 3 — 调用 xmtp_dispatch_user 通知用户仲裁结果（按胜负分流）：**\n\n\
             ━━━━━━━━━━━━━ 买家胜诉（jobStatus=rejected）━━━━━━━━━━━━━\n\
             content：\n\
             \x20\x20\x20\x20[仲裁胜诉] **{title_display}**（{job_id}）仲裁完成，**买方胜诉**。\n\
             \x20\x20\x20\x20  - 退款：**<tokenAmount> <tokenSymbol>**\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（买家胜诉）\n\
             \x20\x20\x20\x20本任务流程结束。如需评价卖家，请回复「评价」。\n\n\
             ━━━━━━━━━━━━━ 买家败诉（jobStatus=complete）━━━━━━━━━━━━━\n\
             content：\n\
             \x20\x20\x20\x20[仲裁败诉] **{title_display}**（{job_id}）仲裁完成，**卖方胜诉**。\n\
             \x20\x20\x20\x20  - 损失：**<tokenAmount> <tokenSymbol>**（资金已释放给卖家）\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（买家败诉）\n\
             \x20\x20\x20\x20本任务流程结束。如需评价卖家，请回复「评价」。\n\n\
             **Step 4 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             ⚠️ **不要自动评价**。\n\
             仲裁流程完整结束。\n"
        ),

        // ─── 卖家同意退款 / 仲裁退款上链 ─────────────────────────────
        Event::JobRefunded => format!(
            "【当前状态】job_refunded（资金已退还买家）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**，只需通知 user session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户退款完成：**\n\n\
             content：\n\
             \x20\x20\x20\x20[退款完成] 任务 {job_id} 退款已上链，**资金已返还**至您的钱包。\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 2 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             退款流程完整结束。\n"
        ),

        // ─── claimAutoRefund tx 回执（submit/refuse 超时后 buyer 主动领回资金）──
        Event::JobAutoRefunded => format!(
            "【系统通知】job_auto_refunded（claimAutoRefund tx 回执）\n\
             【角色】买家（Client）\n\n\
             ⚠️ **不要通过 xmtp_send 向卖家发送任何消息**，只需通知 user session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {title_query_hint}\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户退款到账：**\n\n\
             content：\n\
             \x20\x20\x20\x20[自动退款成功] **{title_display}**（{job_id}）的担保资金已退还至您的钱包。\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 2 — 终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             退款流程完整结束。\n"
        ),

        // ─── 任务超时（OPEN→EXPIRED 或 ACCEPTED→EXPIRED）──────────
        Event::JobExpired => format!(
            "【当前状态】job_expired（任务超时，无人接单或卖家未提交）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户任务已超时：**\n\
             \x20\x20content: 任务 {job_id} **已超时**（accept 截止前未接单 或 submit 截止前未提交），任务已结束。\n\n\
             本任务已到达终态，流程结束。\n"
        ),

        // ─── 任务已关闭（close tx 结果）─────────────────────────────
        Event::JobClosed => format!(
            "【当前状态】job_closed（close tx 结果通知）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             {title_query_hint}\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户：**\n\
             \x20\x20content: **{title_display}**（{job_id}）**已关闭**，资金已回收。\n\n\
             **终态收尾（保留 sub session）：**\n\
             {terminal_session_hint}\n\
             任务关闭流程结束。\n"
        ),

        // ─── 卖家主动联系买家（public 任务，卖家找到任务后发起会话）─────
        // 触发方式：user session 收到自然语言消息（如"有N个卖家待沟通"），
        // 不再依赖 provider_conversation 系统通知。
        Event::Other(ref s) if s == "provider_conversation" => format!(
            "【触发】收到「有卖家待沟通」类消息（user session 侧）\n\
             【角色】买家（Client）\n\n\
             🛑 **禁止自动建群**：收到 pending_list 通知后，**绝对不能**主动调用 xmtp_start_conversation。\n\
             必须先展示列表让用户自己选择卖家，用户明确指定后才能建群。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 获取待沟通卖家列表：**\n\
             调用 xmtp_get_pending_list 工具获取待沟通卖家列表。\n\
             ⚠️ 调用前输出：`[buyer-xmtp] xmtp_get_pending_list`\n\
             ⚠️ 调用后输出：`[buyer-xmtp] xmtp_get_pending_list result: <返回值>`\n\n\
             如果返回空列表 → 调用 xmtp_dispatch_user 通知用户「当前没有待沟通的卖家」，结束。\n\n\
             **Step 2 — 调用 xmtp_prompt_user 展示所有待沟通卖家，让用户选择：**\n\
             🛑 **必须等用户选择**，不能替用户做决定。\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\
             将 pending list 中**所有卖家**逐一列出，让用户挑选：\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
             用户选择某个卖家（回复序号）→ 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY] 用户决策：选择卖家序号 <N>，agentId=<对应agentId>\") relay 回 sub session，sub agent 用选中的 agentId 执行 xmtp_start_conversation 建群；\
             用户回复「全部跳过」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY] 用户决策：全部跳过\") relay 回 sub session，结束。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行建群或 task CLI。\n\
             \x20\x20userContent:\n\
             \x20\x20[任务 {short_id} 你作为买家] 有以下卖家主动联系你，请选择一个开始协商：\n\
             \x20\x20\n\
             \x20\x20[遍历 pending list 每个卖家，格式：]\n\
             \x20\x20<序号>. 卖家 AgentID：<agentId> | 名称：<name> | 信用分：<creditScore> | 完成任务数：<completedTaskCount>\n\
             \x20\x20\n\
             \x20\x20请回复卖家序号开始协商，或回复「全部跳过」。\n\n\
             **Step 3 — 等待用户回复，按用户决策分支：**\n\n\
             ━━━━━━━━━ 分支 A：用户选择了某个卖家 → 建立 session 后协商 ━━━━━━━━━\n\n\
             A-Step 1：调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<用户选中的卖家 agentId>，jobId={job_id}\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             A-Step 2：建群后已进入 sub session，调用 xmtp_send 向卖家发起协商（参照 buyer.md 3.2 协商阶段三步确认）：\n\
             \x20\x20⚠️ **禁止**用 xmtp_dispatch_user / xmtp_dispatch_session，建群后统一用 xmtp_send。\n\
             \x20\x20content: 你好，我有一个任务（jobId: {job_id}）想请你来完成，请问你感兴趣吗？\n\n\
             A-Step 3：协商成功 → 卖家 apply 上链 → 等待卖家 XMTP 消息告知已 apply（buyer.md 路由 #2 触发 confirm-accept）\n\n\
             A-Step 4：协商失败（卖家拒绝 / 超时 / 条件不一致）→ 跳到 B 分支。\n\n\
             ━━━━━━━━━ 分支 B：用户拒绝当前卖家 / 协商失败 → 拒绝并回到列表 ━━━━━━━━━\n\n\
             B-Step 1：调用 xmtp_deny_pending_conversation 拒绝该卖家：\n\
             \x20\x20参数：agentId=<被拒绝卖家的 agentId>，jobId={job_id}\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_deny_pending_conversation: agentId=<agentId>, jobId={job_id}`\n\n\
             B-Step 2：重新调用 xmtp_get_pending_list 获取最新待沟通列表。\n\n\
             B-Step 3：如果列表不为空 → 回到 Step 2，展示剩余卖家让用户选择。\n\n\
             B-Step 4：如果列表为空 → 调用 xmtp_dispatch_user 通知用户：\n\
             \x20\x20content: 任务 {job_id} 当前没有更多待沟通卖家，建议等待新卖家联系或调整任务描述。\n\n\
             【循环结束条件】xmtp_get_pending_list 返回空列表 或 协商成功进入场景 6。\n"
        ),

        // ─── 可见性切换结果（setVisibility tx 结果）───────────────────
        Event::JobVisibilityChanged => format!(
            "【当前状态】job_visibility_changed（公开/私有切换已上链）\n\
             【角色】买家（Client）\n\n\
             🛑 **这不是辅助事件，必须通知用户。**\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {title_query_hint}\
             **Step 1 — 从系统通知 envelope 中读取 `visibility` 字段：**\n\
             - `visibility=0` → 公开（public）\n\
             - `visibility=1` → 私有（private）\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户可见性已变更：**\n\
             content：\n\
             \x20\x20- visibility=0 → [可见性变更] **{title_display}**（{job_id}）已切换为**公开（public）**，等待卖家主动联系。\n\
             \x20\x20- visibility=1 → [可见性变更] **{title_display}**（{job_id}）已切换为**私有（private）**。\n\n\
             ⚠️ 切换为 public 后，**不要**请求推荐卖家列表（recommend），买家只需等待卖家主动找过来。\n\
             → **结束本轮 turn**。\n"
        ),

        // ─── 支付模式切换结果（setPaymentMode tx 结果）────────────────
        Event::JobPaymentModeChanged => format!(
            "【当前状态】job_payment_mode_changed（支付模式切换已上链）\n\
             【角色】买家（Client）\n\n\
             🛑 **必须通知用户支付模式变更结果。**\n\n\
             【你的下一步动作】\n\n\
             {title_query_hint}\
             **Step 1 — 从系统通知 envelope 中读取 `paymentMode` 字段：**\n\
             paymentMode 值映射：1=escrow, 2=non_escrow, 3=x402。\n\
             ⚠️ 直接使用 envelope 中的 paymentMode，不需要额外查询 API。\n\n\
             ━━━━━━━━━ escrow（paymentMode=1）— 发 [NEGOTIATE_CONFIRM] 触发卖家 apply ━━━━━━━━━\n\n\
             **Step 3 — 发 [NEGOTIATE_CONFIRM]（卖家 apply 的唯一合法触发器）**：\n\
             链上 paymentMode 已就位，现在可以安全发 [NEGOTIATE_CONFIRM] 让卖家 apply。\n\
             从你之前发的 [NEGOTIATE_PROPOSE] / 收到的 [NEGOTIATE_ACK] **原样取所有字段**（deliverable / qualityStandards / paymentMode / tokenSymbol / tokenAmount / deadline）回看 sub session 历史复制即可：\n\n\
             调用 xmtp_send：\n\
             \x20\x20content=\n\
             \x20\x20[NEGOTIATE_CONFIRM]\n\
             \x20\x20jobId: {job_id}\n\
             \x20\x20deliverable: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20qualityStandards: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20paymentMode: escrow\n\
             \x20\x20tokenSymbol: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20tokenAmount: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20deadline: <与 [NEGOTIATE_ACK] 完全相同>\n\n\
             ⚠️ **严禁**用自然语言「请你 apply / 请接单」绕过——卖家 flow.rs 把 `[NEGOTIATE_CONFIRM]` 字面量当 apply 唯一触发器，自然语言指令**根本不会被识别**。\n\
             ⚠️ apply 是卖家动作，买家不执行 apply。\n\n\
             **Step 3b — 标记协商完成：**\n\
             ```bash\n\
             onchainos agent negotiate-tick {job_id} --agent-id {agent_id} --seller <providerAgentId> --event confirm\n\
             ```\n\n\
             **Step 4 — 通知用户：**\n\
             调用 xmtp_dispatch_user：\n\
             \x20\x20content: **{title_display}**（{job_id}）更新支付方式成功，设置卖家 **<providerName>**（<providerAgentId>）接单中...\n\n\
             → **结束本轮 turn**，等待卖家 XMTP 消息告知已 apply（buyer.md 路由优先级 #2 处理）。\n\n\
             ━━━━━━━━━ non_escrow（paymentMode=2）— 发 [NEGOTIATE_CONFIRM] + confirm-accept ━━━━━━━━━\n\n\
             🛑 **non_escrow 以下 Step 3 → Step 4 → Step 5 必须在同一 turn 内连续执行，中间不结束 turn。**\n\
             卖家收到 [NEGOTIATE_CONFIRM] 后**不需要先执行链上操作**（escrow 需要 apply），只是静默等 job_accepted。\n\
             因此不存在竞争窗口。\n\n\
             **Step 3 — 发 [NEGOTIATE_CONFIRM] 通知卖家协商已锁定**：\n\
             调用 xmtp_send：\n\
             \x20\x20content=\n\
             \x20\x20[NEGOTIATE_CONFIRM]\n\
             \x20\x20jobId: {job_id}\n\
             \x20\x20deliverable: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20qualityStandards: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20paymentMode: non_escrow\n\
             \x20\x20tokenSymbol: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20tokenAmount: <与 [NEGOTIATE_ACK] 完全相同>\n\
             \x20\x20deadline: <与 [NEGOTIATE_ACK] 完全相同>\n\n\
             ⚠️ **严禁**用自然语言绕过——卖家 flow 只识别 [NEGOTIATE_CONFIRM] 字面量。\n\n\
             **Step 3b — 标记协商完成：**\n\
             ```bash\n\
             onchainos agent negotiate-tick {job_id} --agent-id {agent_id} --seller <providerAgentId> --event confirm\n\
             ```\n\n\
             **Step 4 — 紧接着执行 confirm-accept 上链（不结束 turn，不等卖家回应）**：\n\
             ```bash\n\
             onchainos agent confirm-accept {job_id} --provider-agent-id <providerAgentId> --payment-mode non_escrow --token-symbol <sym> --token-amount <amt>\n\
             ```\n\
             ⚠️ 此步 confirm-accept **只做接单上链**，支付在 complete 阶段执行（先交付后支付）。\n\n\
             **Step 5 — 通知用户：**\n\
             调用 xmtp_dispatch_user：\n\
             \x20\x20content: **{title_display}**（{job_id}）更新支付方式成功，设置卖家 **<providerName>**（<providerAgentId>）接单中...\n\n\
             → **结束本轮 turn**，等待 `job_accepted` 系统通知。\n\n\
             ━━━━━━━━━ x402（paymentMode=3）━━━━━━━━━\n\n\
             从上一步 set-payment-mode / x402-check 的输出中提取 endpoint、acceptsJson、feeTokenSymbol、feeAmount、provider。\n\
             如果上下文中没有 acceptsJson，重新验证：\n\
             ```bash\n\
             onchainos agent x402-check --endpoint <endpoint>\n\
             ```\n\
             提取 `acceptsJson`。\n\n\
             **x402 阶段 2 — 签名 + direct/accept + 重放 endpoint（原子命令）：**\n\
             ```bash\n\
             onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint URL> --token-symbol <feeTokenSymbol> --token-amount <feeAmount>\n\
             ```\n\
             内部执行：x402_pay 签名 → direct/accept 上链 → 组装 payment header → 重放 endpoint\n\
             输出：{{ replaySuccess, replayStatus, replayBody, signature, authorization, sessionCert, txHash }}\n\n\
             **x402 阶段 2 Step 3 — 检查重放结果并通知用户：**\n\
             - replaySuccess=true → 交付物在 replayBody 中。**立即**调用 xmtp_dispatch_user 将交付物发送给用户：\n\
             \x20\x20content:\n\
             \x20\x20[x402 交付物已获取] 任务 {job_id} endpoint 重放成功。\n\
             \x20\x20卖家 AgentID：<providerAgentId>\n\
             \x20\x20金额：<tokenAmount> <tokenSymbol>\n\
             \x20\x20---交付物内容---\n\
             \x20\x20<replayBody 完整内容，JSON 则格式化输出>\n\
             \x20\x20---交付物结束---\n\
             \x20\x20正在等待链上确认（job_accepted），确认后将自动完成任务。\n\n\
             - replaySuccess=false → 调用 xmtp_dispatch_user 通知用户重放失败：\n\
             \x20\x20content:\n\
             \x20\x20[x402 重放失败] 任务 {job_id} 已接单但 endpoint 重放失败。\n\
             \x20\x20HTTP 状态：<replayStatus>\n\
             \x20\x20错误信息：<replayBody>\n\
             \x20\x20等待 `job_accepted` 后**不会自动执行 complete**，需要用户指示。\n\n\
             → **结束本轮 turn**，等待 `job_accepted` 系统通知。\n"
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
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 调用 xmtp_prompt_user 通知用户验收截止时间即将到期，请求决策：**\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
             用户回复「通过」→ 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY] 用户决策：验收通过\") relay 回 sub session 执行 complete；\
             用户回复「拒绝」+ 原因 → 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY] 用户决策：拒绝，原因是<用户原话>\") relay 回 sub session 执行 reject。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行 task CLI。\n\
             \x20\x20userContent:\n\
             \x20\x20[验收截止提醒] 任务 {job_id} 的验收截止时间即将到期。\n\
             \x20\x20超时后卖家可自动领取资金（claimAutoComplete）。\n\
             \x20\x20请尽快决定：\n\
             \x20\x20A. 通过验收 — 回复「通过」\n\
             \x20\x20B. 拒绝交付物 — 回复「拒绝」并说明原因\n\n\
             **Step 2 — 等待用户回复后执行对应命令：**\n\
             - 用户选择通过：\n\
             ```bash\n\
             onchainos agent complete {job_id}\n\
             ```\n\
             - 用户选择拒绝：\n\
             ```bash\n\
             onchainos agent reject {job_id} --reason \"<用户提供的原因>\"\n\
             ```\n"
        ),

        // ─── review_expired: review 窗口超时，等 provider 调 claimAutoComplete ─────
        Event::ReviewExpired => format!(
            "【系统通知】review_expired（review 窗口超时，task 仍是 submitted）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户验收窗口已过期：**\n\
             \x20\x20content:\n\
             \x20\x20[验收超时] 任务 {job_id} 的验收窗口已过期，你未在截止时间前做出验收决定。\n\
             \x20\x20卖家现在可以调用 claimAutoComplete 自动领取资金。\n\
             \x20\x20等待卖家操作中...\n\n\
             **Step 2** — 等待 `job_auto_completed` 系统通知到达后做收尾。\n"
        ),

        // ─── job_auto_completed: provider 的 claim 回执，buyer 端只需观察 ─────
        Event::JobAutoCompleted => format!(
            "【系统通知】job_auto_completed（claimAutoComplete tx 回执）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             {title_query_hint}\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户任务已自动完成：**\n\
             \x20\x20content:\n\
             \x20\x20[任务自动完成] **{title_display}**（{job_id}）因**验收超时**，卖家已通过 claimAutoComplete 领取资金。\n\
             \x20\x20任务状态：completed\n\
             \x20\x20本任务流程结束。\n\n\
             {terminal_session_hint}\n"
        ),

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

        // ─── reward_claimed: buyer 自己的 claim tx 回执（仲裁胜诉退款等） ─────
        Event::RewardClaimed => format!(
            "【系统通知】reward_claimed（claimRewards tx 回执）\n\
             【角色】买家（Client）\n\n\
             【你的下一步动作】\n\n\
             {title_query_hint}\
             **Step 1 — 调用 xmtp_dispatch_user 通知用户奖励已到账：**\n\
             \x20\x20content: [奖励已到账] **{title_display}**（{job_id}）的**奖励/退款已成功领取**到您的钱包。\n"
        ),

        // ─── 网络/重启唤醒 ──────────────────────────────────────────
        Event::WakeupNotify => format!(
            "【系统通知】wakeup_notify（网络/电脑重启后任务唤醒）\n\
             【角色】买家（Client）\n\n\
             ⚠️ 这是 wake-up 心跳事件,**不是**业务驱动事件。真实业务状态在 envelope.message.jobStatus 字段。\n\
             你不应该用 `wakeup_notify` 作为 --jobStatus 跑剧本——本剧本只是引导。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 envelope 读真实 status**:\n\
             从触发本 turn 的 wakeup_notify envelope 里读 `message.jobStatus` 字段（如 `accepted` / `submitted` / `refused` / `disputed` / `completed` / `rejected` 等真实 status string）。\n\n\
             **Step 2 — 用真实 status 重调 next-action 拿当前剧本**:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --jobStatus <message.jobStatus 字段值> --role buyer --agentId {agent_id}\n\
             ```\n\
             按返回剧本走当前 status 应做动作。\n\n\
             **Step 3 — 幂等性自查（避免重复 prompt 用户）**:\n\
             如果 Step 2 拿到的剧本含 `xmtp_prompt_user` 步骤,**先**调:\n\
             ```bash\n\
             onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
             ```\n\
             - 该 jobId 已有 pending 条目（断线前已 prompt 过）→ **跳过本次 xmtp_prompt_user 重发**,改成 `xmtp_dispatch_user` 通知「任务 {job_id} 已恢复,请继续在 user session 处理决策」\n\
             - 无 pending 条目（首次或之前已 RELAY 关闭）→ 按 Step 2 剧本正常执行(包括 pending-decisions add + xmtp_prompt_user)\n\n\
             ⚠️ **不要** xmtp_send 给卖家「我重新上线了」之类的过场——对方不关心你的连接状态。\n\
             ⚠️ Step 2 拿到的剧本如果是被动等待类（如 status=accepted 等卖家交付）,只输出「任务恢复」通知后结束 turn,不主动跑业务动作。\n"
        ),

        // ─── 发布任务（user session 主动操作，非链事件）────────────────
        Event::Other(ref s) if s == "create_task" => "\
【当前操作】发布任务（create_task）
【角色】买家（Client）
【会话类型】user session（直接与用户对话）

🛑 **禁止跳步**：必须完成全部字段收集 → 展示确认表单 → 用户明确确认后，才能调 CLI。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 1 — 字段收集（通过对话逐步收集，**全部就绪才进 Step 2**）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

| 字段 | CLI 参数 | 约束 | 收集方式 |
|---|---|---|---|
| 描述 | --description | 10–2000 字符 | 整合用户原话。<10 → 「描述越详细，匹配到的 Provider 越准确。能补充一下具体需求吗？」 |
| 标题 | --title | ≤30 字符 | Agent 总结，生成后**必须计数**，>30 缩短 |
| 摘要 | --description-summary | ≤200 字符 | Agent 总结，生成后**必须计数**，>200 缩短 |
| 支付代币 | --currency | 仅 USDT / USDG | ⚠️ 见下方代币规则 |
| 预算 | --budget | 数字; ≤5 位小数; max 10,000,000 | 提取数字 |
| 最高预算 | --max-budget | **Required**; ≥ budget; ≤5 位小数; max 10,000,000 | ⚠️ **必须明确询问用户**，不可自动填充或猜测。这是协商价格上限，卖家报价不得超过此值 |
| 接单时限 | --deadline-open | 10 min – 6 months; 格式 `<n>h` / `<n>m` | **必须询问用户**。发布后多久无人接单则自动关闭 |
| 交付时限 | --deadline-submit | 1 min – 6 months; 格式 `<n>h` / `<n>m` | **必须询问用户**。接单后多久内须完成交付 |

🛑 **代币规则（最高优先级）**：
- 用户明确写 \"USDT\" 或 \"USDG\" → 直接用，无需确认
- 用户使用模糊表达（\"U\" / \"u\" / \"刀\" / \"美元\" / \"美金\" / \"dollar\" / \"USD\" / \"100U\" / \"50u\"）→ **必须先问「请确认支付代币：USDT 还是 USDG？」**，等用户明确回复后才填入
- **禁止默认 USDT**，展示 \"100 USDT\" 当用户只说 \"100U\" 是违规

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 2 — 校验（字段全部收集后、展示表单前）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. 代币 ≠ USDT 且 ≠ USDG → 「目前只支持 USDT 和 USDG，请选择其中一个。」
2. 描述 < 10 字符 → 引导补充
3. max_budget < budget → 「最高预算不能小于预算。」
4. max_budget 未填 → 「请设置最高预算（协商价格上限），卖家报价不得超过此值。」
5. budget > 10,000,000 或小数位 > 5 → 提示限制

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 3 — 身份 & 余额检查
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. `onchainos agent get` 检查当前账户是否有 buyer 身份（role=1）
2. 有 buyer → 告知用户使用哪个账户
3. 无 buyer → 引导注册 `onchainos agent register`
4. 余额不足 → 警告但不阻断创建
5. 执行 `skills/okx-agent-chat/after-agent-list-changed.md` 检查通信服务可用性

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 4 — 展示确认表单（格式见 `skills/okx-agent-task/references/display-formats.md` §3）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

| 字段 | 值 |
|---|---|
| 标题 | <agent 总结> |
| 摘要 | <agent 总结，≤200 字符> |
| 描述 | <完整内容>（≤200 字符时放表格内；>200 字符时表格写 `见下方`，在表格下方用 prose 展示完整内容） |
| 支付代币 | <USDT 或 USDG> |
| 预算 | <数字> |
| 最高预算 | <数字>（协商价格上限） |
| 接单时限 | <Nh>（发布后 N 小时无人接单自动关闭） |
| 交付时限 | <Nh>（接单后 N 小时内须完成交付） |

> 确认无误？确认后我立即上链创建任务。

⚠️ 中文对话用中文字段标签，英文对话用英文。

→ **结束本轮 turn**，展示表单后必须停止，等待用户对**本表单**的明确确认回复。
🛑 之前对话中用户对子问题（如代币确认）的「确认」不算对表单的确认，必须是用户看到表单后的新回复。

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 5 — 用户对表单确认后调 CLI（🛑 禁止与 Step 4 同一轮执行）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

```bash
onchainos agent create-task \\
  --description \"<description>\" \\
  --description-summary \"<summary>\" \\
  --title \"<title>\" \\
  --budget <budget> --max-budget <max_budget> \\
  --currency <USDT|USDG> \\
  --deadline-open <deadline_open> --deadline-submit <deadline_submit>
```

🚫 **create-task 只接受以上参数。没有 --content / --period / --visibility / --amount / --token / --payment-mode 参数。**
⚠️ **支付方式不在创建阶段设置**——paymentMode 由后续与卖家协商时根据卖家支持的方式决定（escrow / non_escrow），或指定卖家时由其服务类型决定（x402）。如果用户在发布任务时提到了支付方式偏好（如「用担保支付」「用 escrow」），**不要传 --payment-mode**，告知用户：「支付方式将在与卖家协商时确定，届时会根据卖家支持的方式和你的偏好来选择。」

成功后告知用户：
> 任务已提交，jobId: <jobId>，等待上链确认（约数秒）。确认后系统将自动联系推荐卖家开始协商。

⚠️ 不要说「发布成功」——此时尚未上链确认。上链确认由 job_created 消息触发。
⚠️ 不要调 recommend——推荐在 job_created 收到后自动执行。
".to_string(),

        // ─── negotiate_timeout: 协商超时，自动 REJECT + 切换下一个卖家 ────
        Event::NegotiateTimeout => {
            let seller_hint = if let Some(sid) = seller {
                format!(
                    "**seller 已由 --seller 参数传入：`{sid}`**（下面用 `<sellerAgentId>` 表示，值为 `{sid}`）。\n\n"
                )
            } else {
                format!(
                    "**Step 0 — 获取当前协商卖家信息（--seller 未传入，需手动获取）：**\n\
                     ```bash\n\
                     onchainos agent recommend {job_id} --current\n\
                     ```\n\
                     从输出提取 `providerAgentId`（下面用 `<sellerAgentId>` 表示）。\n\n"
                )
            };
            format!(
                "【当前状态】negotiate_timeout（协商超时 / COUNTER 轮次超限）\n\
                 【角色】买家（Client）\n\n\
                 【你的下一步动作（严格顺序，全自动执行，不询问用户）】\n\n\
                 {seller_hint}\
                 **Step 1 — 调 negotiate-tick 确认超时状态：**\n\
                 ```bash\n\
                 onchainos agent negotiate-tick {job_id} --agent-id {agent_id} --seller <sellerAgentId> --event timeout_check\n\
                 ```\n\
                 检查输出 `action` 字段：\n\
                 - `action: \"timeout\"` → 确认超时，继续 Step 2\n\
                 - `action: \"continue\"` → 尚未真正超时（时钟偏差），**不要 REJECT**，结束 turn 继续等待\n\
                 - `action: \"already_terminated\"` → 该卖家已被处理（rejected/completed），结束 turn\n\n\
                 **Step 2 — 获取 session 状态并发送 [NEGOTIATE_REJECT]：**\n\
                 先调 `onchainos agent session-status {job_id} --agent-id {agent_id} --peer <sellerAgentId>` 获取 `sessionKey`。\n\
                 再调 xmtp_send（需要 sessionKey）发送：\n\
                 \x20\x20content=\n\
                 \x20\x20[NEGOTIATE_REJECT]\n\
                 \x20\x20jobId: {job_id}\n\
                 \x20\x20reason: 协商超时（300秒未回复）\n\n\
                 **Step 3 — 记录 reject：**\n\
                 ```bash\n\
                 onchainos agent negotiate-tick {job_id} --agent-id {agent_id} --seller <sellerAgentId> --event reject\n\
                 ```\n\n\
                 **Step 4 — 切换下一个卖家：**\n\
                 ```bash\n\
                 onchainos agent recommend {job_id} --next\n\
                 ```\n\
                 回到 job_created 剧本的 Step 2 路由判断。\n\
                 推荐列表遍历完 → 按 job_created 剧本的「遍历结束」流程引导用户选择。\n\n\
                 ⚠️ **超时后再收到该卖家消息一律忽略、不回复。**\n"
            )
        }

        // ─── 买家不会收到的事件（evaluator 质押 lifecycle）──────────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed
        | Event::StakeStopped
        | Event::CooldownEntered
        | Event::DisputeApproved
        // ─── 未知类型兜底 ───────────────────────────────────────────
        | Event::Other(_) => format!(
            "【未知状态】{event}\n\
             【建议】\n\
             1. 调用 `onchainos agent common context {job_id} --role buyer` 查看完整上下文\n\
             2. 如该状态不在预期流程内，等待用户指示\n\
             3. 不要预测/假设其他通知\n",
            event = event.as_str()
        ),
    };

    let result = if job_status == "create_task" {
        body
    } else {
        format!("{context_preamble}{body}")
    };
    eprintln!(
        "[buyer-flow] output length: {} chars | first 200: {}",
        result.len(),
        &result[..result.len().min(200)]
    );
    result
}
