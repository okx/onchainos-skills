//! User Agent (用户) 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 对应 provider/flow.rs 的用户版本，让 agent 只需
//! `exec onchainos agent next-action --role buyer ...` 拿提示词直接执行。
//!
//! 实际 prompt 生成逻辑按职责拆分到：
//! - `flow_negotiate.rs` — 协商/匹配阶段
//! - `flow_lifecycle.rs` — 任务执行 + 仲裁 + 终态

use crate::commands::agent_commerce::task::common::pending::short_job_id;
use crate::commands::agent_commerce::task::common::state_machine::Status;

const LOCALIZATION_PREFIX: &str = "[Localization] All `content:` / `userContent:` templates below are samples — translate to the user's language before `xmtp_dispatch_user` / `xmtp_prompt_user`.\n\n";

/// 跨所有事件处理函数共享的上下文参数包。
pub(super) struct FlowContext<'a> {
    pub job_id: &'a str,
    pub agent_id: &'a str,
    pub short_id: &'a str,
    pub title_display: &'a str,
    pub title_query_hint: &'a str,
    pub title_in_extract: &'a str,
    pub terminal_session_hint: &'a str,
}

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
        Status::Created => vec![
            next_action("job_created"),
            ref_header,
            format!("  onchainos agent recommend {job_id} --agent-id <agentId>  # 查看推荐服务商"),
            format!("  onchainos agent set-payment-mode {job_id} --payment-mode <escrow|x402> --token-symbol <sym> --token-amount <amt> [--endpoint <url>]  # 设置支付方式"),
            format!("  onchainos agent confirm-accept {job_id} --provider-agent-id <agentId> --payment-mode escrow --token-symbol <sym> --token-amount <amt>  # 确认接单（setPaymentMode 后执行，仅 escrow）"),
            format!("  onchainos agent direct-accept {job_id} --provider-agent-id <agentId> --token-symbol <sym> --token-amount <amt>  # x402 阶段 2b: endpoint 交互后调用"),
            format!("  onchainos agent close {job_id}          # 关闭任务"),
            format!("  onchainos agent set-public {job_id}     # 转为公开任务"),
            format!("  onchainos agent set-token-and-budget {job_id} --token-symbol <USDT|USDG> --budget <amount>  # 修改支付代币及金额（上链）"),
            format!("  onchainos agent set-provider {job_id} --provider-agent-id <agentId>  # 修改服务商（上链）"),
            format!("  onchainos agent set-max-budget {job_id} --max-budget <amount>  # 修改最高预算（不上链）"),
        ],
        Status::Accepted => vec![
            "（escrow）服务商执行任务中，等待 job_submitted 进入验收".to_string(),
            "（x402）服务商交付已在 accept 阶段完成".to_string(),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "⚠️ complete/reject 不在 job_submitted 剧本中——收到用户验收决策后调 next-action 拿对应伪事件剧本：".to_string(),
            format!("  onchainos agent next-action --jobid {job_id} --jobStatus approve_review --role buyer --agentId <agentId>  # 用户验收通过后"),
            format!("  onchainos agent next-action --jobid {job_id} --jobStatus reject_review --role buyer --agentId <agentId>  # 用户拒绝验收后"),
            format!("  onchainos agent feedback-submit --agent-id <providerAgentId> --creator-id <buyerAgentId> --score <score> --task-id {job_id}  # 评价服务商（用户回复「评价」后再收集评分和内容）"),
        ],
        Status::Refused => vec![
            next_action("job_refused"),
            "（被动等待）服务商 24h 内决策：job_disputed → 进入仲裁举证；job_refunded → 退款".to_string(),
        ],
        Status::Disputed => vec![
            next_action("job_disputed"),
            ref_header,
            format!("  onchainos agent dispute upload {job_id} --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
        ],
        Status::Completed => vec![
            next_action("job_completed"),
            "（终态）任务已 COMPLETE — **资金已释放给服务商**".to_string(),
            "  ▸ escrow 验收通过 → 释放担保款给服务商".to_string(),
            "  ▸ 仲裁服务商胜（dispute_resolved seller-wins）→ 释放担保款给服务商".to_string(),
            "  ▸ x402 资金在 accept 阶段已支付".to_string(),
            "⚠️ 保留 sub session（不关闭），便于事后查阅历史。".to_string(),
        ],
        Status::Rejected => vec![
            next_action("job_refunded"),
            "（终态）任务已 REJECTED — **资金已退还用户**".to_string(),
            "  ▸ 服务商同意退款（agree-refund）/ 自动退款 → 资金原路返回".to_string(),
            "  ▸ 仲裁用户胜（dispute_resolved buyer-wins）→ 退款".to_string(),
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
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str, job_title: Option<&str>) -> String {
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    // 短 jobId,用在 xmtp_prompt_user 的 userContent 第一行 `[任务 <短ID> 你作为用户]` 前缀,
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
    //   - xmtp_send：发给服务商（peer sub session），参数 sessionKey + content
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
         \x20\x205) ❌ **apply 是服务商动作**：escrow 路径中 `apply` 由服务商执行，用户绝不能调 `onchainos agent apply`。用户先调 `set-payment-mode`，再在收到服务商申请通知后执行 `confirm-accept`。\n\
         \x20\x206) ❌ **同 turn 只调一次 `session_status`**:sessionKey 在同 turn 内稳定,调过一次结果复用。重复调 = 死循环征兆,立即停。\n\
         \x20\x207) ❌ **`xmtp_prompt_user` 必前后配对 `pending-decisions`**(唯一键 = jobId+role+agentId 三元组,规则源 `SKILL.md §通信契约 5`):\n\
         \x20\x20\x20\x20• 调 `xmtp_prompt_user` 前: `onchainos agent pending-decisions add --sub-key <sessionKey> --job-id {job_id} --role buyer --agent-id {agent_id} --summary \"<userContent 首行后简述>\" --user-content \"<userContent 完整原文>\"`\n\
         \x20\x20\x20\x20• 解析 `[USER_DECISION_RELAY]` 后、调 next-action 前: `onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}`\n\
         \x20\x20\x20\x20漏 `add` → 用户回复时反查不到本条决策,无法 relay 回本会话;\n\
         \x20\x20\x20\x20漏 `remove` → 旧条目残留成僵尸,下次再调 `xmtp_prompt_user` 时被误命中,用户回复派给错的会话。\n\
         \x20\x208) ❌ **用户可见内容禁用技术术语**:`xmtp_dispatch_user` 的 content 和 `xmtp_prompt_user` 的 userContent 都直接给用户看,**禁写** tool 名(`xmtp_*`) / 事件名(`provider_applied`/`job_*`/`dispute_resolved` 等) / 状态名(`open`/`accepted`/`disputed` 等英文枚举) / CLI flag(`--*`) / skill 名(`okx-agent-identity` / `§Feedback Submit` 等) / 状态字段名(`jobStatus`/`paymentMode` 等)——一律用**用户语言**的自然表达(中文用户看到「担保/x402, 验收期超时, 任务已完成」, 英文用户看到等价口语化措辞如「escrowed payment/x402, review window expired, task completed」, 由 sub agent 按 LOCALIZATION_PREFIX 翻译时一并替换)。同 turn 内的 `xmtp_send` 给服务商也按此规则。\n\
         \x20\x209) ❌ **禁止给服务商发过场消息**：除协商阶段的结构化消息（[intent:propose]、[intent:confirm]、协商自然语言对话）外，**任何事件处理中都不要 xmtp_send 给服务商**。包括但不限于「已确认接单」「资金已托管」「已验收」「证据已提交」「任务已完成」等状态通知。服务商通过链上事件得知状态变化，用户发过场消息只会造成干扰。\n\
         \x20\x2010) 🛑🛑🛑 **ABSOLUTE PROHIBITION — sub session / backup session 禁止直接生成文字回复**——你在 sub/backup session 中输出的任何文字用户**完全、绝对、100% 看不到**。所有面向用户的内容**必须且只能**通过 `xmtp_dispatch_user`（纯通知）或 `xmtp_prompt_user`（需要用户决策）工具推送。直接输出文字 = 信息丢失 + 用户无感知 + 流程卡死。🔴 真实事故：模型在 backup session 拿到推荐列表后直接输出文字展示，用户完全没收到，任务卡死。\n\
         \x20\x2012) 🛑🛑🛑 **ABSOLUTE PROHIBITION — 禁止 `sessions_spawn` / `sessions_yield`**——你（sub session / backup session）**自己就是**负责执行剧本的 agent。**绝对禁止**调 `sessions_spawn` 派生子 agent 再委托、**绝对禁止**调 `sessions_yield` 交出控制权。backup session 也是 sub，收到 `source:\"system\"` 事件后必须**自己调 `next-action`、自己执行剧本**。🔴 真实事故：backup 收到 `job_created` 后调 `sessions_spawn` 派生子 agent，虽然结果碰巧正确但执行路径错误——designated-provider 可能未被正确消费、协商上下文断裂。\n\
         \x20\x2013) 🛑🛑🛑 **job_submitted 验收硬门禁 — 禁止自动 complete/reject**：`job_submitted` 剧本中**不包含** `onchainos agent complete` / `onchainos agent reject` 命令——它们被拆分到独立伪事件 `approve_review` / `reject_review` 中。收到 `[USER_DECISION_RELAY]` 后，**必须调 `next-action --jobStatus approve_review` 或 `reject_review` 拿剧本**，不要自己拼 complete/reject 命令。🔴 真实事故：模型收到 job_submitted 后跳过 xmtp_prompt_user，直接调 `onchainos agent complete` 自动验收释放资金——用户完全没看到交付物、没做验收决策，资金不可逆地转给服务商。\n\
         \x20\x2014) 🛑 **协商评估前置 — 禁止跳过评估直接拒绝**：收到服务商回复后，**必须先完成评估**（`common context` 获取 budget/max_budget → 提取报价/能力信息 → 按决策矩阵判断）**再**发送任何 `xmtp_send`。跳过评估直接回复或拒绝 = 决策无依据。🔴 真实事故：模型收到服务商首条报价后跳过评估，1 秒内自动发送「技能不匹配」拒绝——服务商的报价在预算内、技能完全匹配，但模型没有读取回复内容就做了判断。\n\
         \x20\x2015) 🛑🛑🛑 **ABSOLUTE PROHIBITION — 收到 `[USER_DECISION_RELAY]` 必须就地执行，禁止转发**：当你（sub/backup session）收到以 `[USER_DECISION_RELAY][intent:...]` 开头的消息时，这是**用户决策由 user session relay 给你执行的**——你就是目标 session、你就是执行者。**必须**：先 `pending-decisions remove`（规则 7），再解析 intent tag 并执行对应动作：\n\
         \x20\x20\x20\x20▸ `[intent:PICK_PROVIDER agentId=X]` → `onchainos agent next-action --jobid {job_id} --jobStatus job_created --role buyer --agentId {agent_id} --provider X`\n\
         \x20\x20\x20\x20▸ `[intent:NEXT_PAGE]` → 翻页（recommend 下一页）\n\
         \x20\x20\x20\x20▸ `[intent:SET_PUBLIC]` → `onchainos agent set-public {job_id}`\n\
         \x20\x20\x20\x20▸ `[intent:CLOSE_TASK]` → `onchainos agent close {job_id}`\n\
         \x20\x20\x20\x20▸ `[intent:VIEW_RECOMMEND]` → `onchainos agent recommend {job_id} --agent-id {agent_id}`\n\
         \x20\x20\x20\x20▸ `[intent:APPROVE_REVIEW]` → `onchainos agent next-action --jobid {job_id} --jobStatus approve_review --role buyer --agentId {agent_id}`\n\
         \x20\x20\x20\x20▸ `[intent:REJECT_REVIEW]` → `onchainos agent next-action --jobid {job_id} --jobStatus reject_review --role buyer --agentId {agent_id}`\n\
         \x20\x20\x20\x20▸ `[intent:SUBMIT_EVIDENCE]` → `onchainos agent next-action --jobid {job_id} --jobStatus dispute_evidence --role buyer --agentId {agent_id}`\n\
         \x20\x20\x20\x20▸ `[intent:ACCEPT_X402_PRICE]` → 继续 x402 支付流程（DX-Step 3）\n\
         \x20\x20\x20\x20▸ `[intent:REJECT_X402_PRICE]` → 引导换服务商\n\
         \x20\x20\x20\x20▸ `[intent:SKIP_ALL_PROVIDERS]` → 结束换服务商流程\n\
         \x20\x20\x20\x20**绝对禁止**调 `xmtp_dispatch_session` 把 `[USER_DECISION_RELAY]` 内容再转发给任何 session（包括自己）——你就是最终接收方，转发 = 死循环。🔴 真实事故：backup session（Minimax）收到 `[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=806]` 后没有执行 next-action，反而调 xmtp_dispatch_session 把同一消息转发给自己（agent:main:okx-a2a:group:backup），形成无限循环，任务卡死。\n\n\
         如果不记得本任务协商细节（paymentMode / token / 服务商 agentId / 价格），\n\
         先 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 加载上下文。\n\n"
    );

    let ctx = FlowContext {
        job_id,
        agent_id,
        short_id: &short_id,
        title_display,
        title_query_hint: &title_query_hint,
        title_in_extract,
        terminal_session_hint,
    };

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
            Event::JobRefused => "xmtp_dispatch_user (通知拒绝已上链) → 等待服务商决策",
            Event::JobDisputed => "xmtp_prompt_user (转发仲裁通知请求证据)",
            Event::DisputeResolved => "xmtp_dispatch_user (通知仲裁结果)",
            Event::JobRefunded => "xmtp_dispatch_user (通知退款完成)",
            Event::JobAutoRefunded => "xmtp_dispatch_user (claimAutoRefund tx 回执)",
            Event::NegotiateReply => "xmtp_send (评估服务商自然语言回复)",
            Event::NegotiateAck => "save-agreed → set-payment-mode (ACK 校验 → 落盘)",
            Event::NegotiateCounter => "xmtp_send (评估 COUNTER → 新 PROPOSE 或 REJECT)",
            _ => "无",
        }
    );

    let body = match event {
        // ─── 协商/匹配阶段 → flow_negotiate ──────────────────────────
        Event::JobCreated => super::flow_negotiate::job_created(&ctx),
        Event::SwitchProvider => super::flow_negotiate::switch_provider(&ctx),
        Event::Other(ref s) if s == "provider_conversation" => super::flow_negotiate::provider_conversation(&ctx),
        Event::JobVisibilityChanged => super::flow_negotiate::job_visibility_changed(&ctx),
        Event::JobPaymentModeChanged => super::flow_negotiate::job_payment_mode_changed(&ctx),
        Event::NegotiateReply => super::flow_negotiate::negotiate_reply(&ctx),
        Event::NegotiateAck => super::flow_negotiate::negotiate_ack(&ctx),
        Event::NegotiateCounter => super::flow_negotiate::negotiate_counter(&ctx),

        // ─── 任务执行 + 仲裁 + 终态 → flow_lifecycle ─────────────────
        Event::ProviderApplied => super::flow_lifecycle::provider_applied(&ctx),
        Event::JobAccepted => super::flow_lifecycle::job_accepted(&ctx),
        Event::JobSubmitted => super::flow_lifecycle::job_submitted(&ctx),
        Event::JobRefused => super::flow_lifecycle::job_refused(&ctx),
        Event::JobDisputed => super::flow_lifecycle::job_disputed(&ctx),
        Event::Other(ref s) if s == "dispute_evidence" => super::flow_lifecycle::dispute_evidence(&ctx),
        Event::Other(ref s) if s == "approve_review" => super::flow_lifecycle::approve_review(&ctx),
        Event::Other(ref s) if s == "reject_review" => super::flow_lifecycle::reject_review(&ctx),
        Event::JobCompleted => super::flow_lifecycle::job_completed(&ctx),
        Event::DisputeResolved => super::flow_lifecycle::dispute_resolved(&ctx),
        Event::JobRefunded => super::flow_lifecycle::job_refunded(&ctx),
        Event::JobAutoRefunded => super::flow_lifecycle::job_auto_refunded(&ctx),
        Event::JobExpired => super::flow_lifecycle::job_expired(&ctx),
        Event::JobClosed => super::flow_lifecycle::job_closed(&ctx),
        Event::SubmitExpired => super::flow_lifecycle::submit_expired(&ctx),
        Event::RefuseExpired => super::flow_lifecycle::refuse_expired(&ctx),
        Event::ReviewDeadlineWarn => super::flow_lifecycle::review_deadline_warn(&ctx),
        Event::ReviewExpired => super::flow_lifecycle::review_expired(&ctx),
        Event::JobAutoCompleted => super::flow_lifecycle::job_auto_completed(&ctx),
        Event::SubmitDeadlineWarn => super::flow_lifecycle::submit_deadline_warn(),
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed => super::flow_lifecycle::evaluator_events(event.as_str()),
        Event::RewardClaimed => super::flow_lifecycle::reward_claimed(&ctx),
        Event::WakeupNotify => super::flow_lifecycle::wakeup_notify(&ctx),
        Event::Other(ref s) if s == "create_task" => super::flow_lifecycle::create_task(),
        Event::Other(ref s) if s == "close" => super::flow_lifecycle::close_task(&ctx),
        Event::Other(ref s) if s == "set_public" => super::flow_lifecycle::set_public(&ctx),
        Event::TaskTokenBudgetChange => super::flow_lifecycle::task_token_budget_change(&ctx),
        Event::TaskProviderChange => super::flow_lifecycle::task_provider_change(&ctx),

        // ─── 用户不会收到的事件 + 未知兜底 ──────────────────────────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed
        | Event::StakeStopped
        | Event::CooldownEntered
        | Event::DisputeApproved
        | Event::Other(_) => super::flow_lifecycle::staked_and_unknown(event.as_str(), job_id),
    };

    let core = if job_status == "create_task" || job_status == "switch_provider" {
        body
    } else {
        format!("{context_preamble}{body}")
    };
    let result = format!("{LOCALIZATION_PREFIX}{core}");
    let preview: String = result.chars().take(200).collect();
    eprintln!(
        "[buyer-flow] output length: {} chars | first 200: {}",
        result.len(),
        preview
    );
    result
}
