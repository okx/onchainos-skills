//! Provider 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 目的：把散落在 provider.md 里的 Scene 步骤集中到代码里，让 agent 只需
//! `exec onchainos agent next-action ...` 拿提示词直接执行，不用推理整份文档。

use crate::commands::agent_commerce::task::common::state_machine::Status;

/// Provider 在某 status 下应该执行的下一步（用于 `agent common context` 输出末尾的菜单）。
///
/// 第一行恒为 `next-action` 调用——这是 sub agent 在当前 status 下**唯一第一步动作**：
/// 拿剧本，按剧本走。后面的 CLI 命令是"剧本里会用到的相关命令"参考清单，**不要直接调**。
/// `generate_next_action` 函数同文件，按 status 对应的 entry event 路由。
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**下一步必做** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role provider --agentId <agentId>`（拿当前 status 的完整剧本，**按剧本走**，不要绕过 next-action 直接调下方 CLI）")
    };
    let ref_header = "（参考·剧本里会用到的相关 CLI；不要直接调，先调 next-action 拿剧本）".to_string();
    match status {
        Status::Open => vec![
            next_action("job_created"),
            ref_header,
            format!("  onchainos agent apply {job_id} --token-amount <price> --token-symbol USDT --agent-id <agentId>  # 申请接单（**仅 escrow 担保交易**才需要；non_escrow 不要 apply）"),
            format!("  onchainos agent get-payment {job_id} --token-symbol <USDT|USDG> --token-amount <price> --payment-mode <escrow|non_escrow> --agent-id <agentId>  # 拉 prePayTaskInfo + 创建 a2a-pay 付款单，输出 paymentId"),
        ],
        Status::Accepted => vec![
            next_action("job_accepted"),
            ref_header,
            format!("  onchainos agent deliver {job_id} --file <deliverable> --message <msg>  # 提交交付"),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "（被动等待）等待买家验收：job_completed → 任务完成；job_refused → 进入仲裁/退款决策".to_string(),
        ],
        Status::Refused => vec![
            next_action("job_refused"),
            ref_header,
            format!("  onchainos agent dispute raise {job_id} --reason <reason>  # 发起仲裁"),
            format!("  onchainos agent agree-refund {job_id}  # 同意退款"),
        ],
        Status::Disputed => vec![
            next_action("job_disputed"),
            ref_header,
            format!("  onchainos agent dispute upload {job_id} --agent-id <你的agentId> --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
        ],
        Status::Completed => vec![
            next_action("job_completed"),
            "（流程结束）任务完成，资金已释放。子 session 可关闭。".to_string(),
        ],
        Status::Refunded => vec![
            next_action("confirm_refund"),
            "（流程结束）资金已退还买家。子 session 可关闭。".to_string(),
        ],
        Status::Other(s) => vec![
            format!("当前状态 `{s}` 不在标准状态机内 → 先 `onchainos agent status {job_id}` 查最新状态"),
        ],
    }
}

/// 根据 jobStatus 生成 provider 下一步动作的结构化提示词。
///
/// `job_status` 既能接 event 名（provider_applied / job_accepted / ...），
/// 也能接 status 名（open / accepted / ...）—— 内部统一通过 state_machine
/// 解析成 `Event`，看不认识的字符串保留 `Event::Other(s)` 兜底。
pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    use crate::commands::agent_commerce::task::common::state_machine::{parse_status_or_event, Event};

    // ──────────────────────────────────────────────────────────────────────
    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md §Session 通信契约。
    // 本文件只负责告诉 agent **每一步把什么内容发到哪**，不重复解释 xmtp_send /
    // xmtp_dispatch_session 的用法。下面两个变量是给剧本里的占位符：
    //   - send_to_peer：表示"用 xmtp_send 发给买家（peer sub session）"
    //   - send_to_user：表示"用 xmtp_dispatch_session 推到 user session"
    // ──────────────────────────────────────────────────────────────────────
    let send_to_peer = format!(
        "→ 用 `xmtp_send` 发给买家（机制见 SKILL.md §Session 通信契约 §1 路径 4）。\n\
         当前 sub session：jobId={job_id}，我方 agentId={agent_id}。\n\
         content（纯自然语言，不要包 markdown / 代码块）："
    );
    // 兼容旧变量名
    let header_template = &send_to_peer;

    let context_preamble = format!(
        "📍 你在 sub session（你看到这段 next-action 输出 = 100% 在 sub）。\n\n\
         🔒 **如果当前 turn 没读过 SKILL.md §Session 通信契约**（envelope 形态白名单 / xmtp_send 两步 / xmtp_dispatch_session 推 user session opt-in 铁律），\n\
         **先读 `skills/okx-agent-task/SKILL.md`** 再继续——下面步骤会引用它的章节（§3 / §4 / §6 / §7）。\n\n\
         如果不记得本任务协商细节（deliverable / paymentMode / token / 买家 agentId / 价格），\n\
         先 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 加载上下文。\n\n"
    );

    let event = parse_status_or_event(job_status);
    let body = match event {
        // ─── Scene 3: 接单申请已上链（escrow 路径，买家方负责生成付款单） ──
        Event::ProviderApplied => format!(
            "【当前状态】provider_applied（链上已确认接单申请，escrow 担保路径）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             **只发一条 `xmtp_send` 通知买家接单申请已上链，请买家走 confirm-accept**——\n\
             escrow 路径的付款单由买家在 confirm-accept 时自行生成，**卖家不需要**调 `get-payment`。\n\n\
             {send_to_peer}\n\
             已完成接单申请上链（jobId={job_id}，卖家 agentId={agent_id}）。请你执行 confirm-accept 注资托管。\n\n\
             ⚠️ 不要再调 `onchainos agent get-payment`——那是 non_escrow 路径才用的。\n\n\
             跑完 xmtp_send → **直接结束本轮 turn**，等买家 confirm-accept 触发的 `job_accepted` 通知再进入 Scene 4。\n\n\
             【后续事件】\n\
             - job_accepted → 买家已 confirm-accept，资金托管完成，开始执行任务\n"
        ),

        // ─── Scene 4: 买家已确认接单，执行任务并交付 ─────────────────
        Event::JobAccepted => format!(
            "【当前状态】job_accepted（买家已确认接单，资金托管）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序，不得跳步）】\n\n\
             **Step 1 — 把接单成功通知推到 user session**（机制见 SKILL.md §Session 通信契约 §1 路径 2 + §2 STATUS_NOTIFY 形态）：\n\n\
             content：\n\
             \x20\x20\x20\x20[STATUS_NOTIFY · 仅展示给用户 · user session agent 不要调任何工具不要再次执行]\n\
             \x20\x20\x20\x20[接单成功通知] 任务 {job_id} 已完成接单\n\
             \x20\x20\x20\x20- 标题：<title>\n\
             \x20\x20\x20\x20- 描述：<description>\n\
             \x20\x20\x20\x20- 协商价格：<amount> <tokenSymbol>\n\
             \x20\x20\x20\x20- 支付方式：<mode>\n\
             \x20\x20\x20\x20- 卖家 AgentID：{agent_id}\n\
             \x20\x20\x20\x20资金已托管，sub session 卖家已开始执行任务。\n\n\
             字段值从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 输出中提取。\n\n\
             **Step 2 — 给买家发 P2P 消息确认**：\n\n\
             {header_template}\n\
             已收到接单确认（job_accepted），开始执行任务。\n\n\
             **Step 3 — 执行任务（mock 环境可直接跳过），完成后调用 CLI 提交交付物：**\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --message \"任务已完成，请验收\"\n\
             ```\n\
             CLI 内部：POST submit API → 签名 uopHash → 广播上链。\n\n\
             ⚠️ **跑完 deliver 直接结束 turn，禁止 `xmtp_dispatch_session` 推 STATUS_NOTIFY 到 user session**——『已提交交付物 / 等待 job_submitted』是过场状态。等 `job_submitted` 通知到达再回复买家『请验收』。\n\n\
             【后续事件】\n\
             - job_submitted → 交付物已上链，再次调用 next-action 获取下一步\n"
        ),

        // ─── Scene 5: 交付物已上链，通知买家验收 ─────────────────────
        Event::JobSubmitted => format!(
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
        Event::JobRefused => format!(
            "【当前状态】job_refused（买家拒绝交付物）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 向买家调用 `xmtp_send` 工具发送消息：**\n\n\
             {header_template}\n\
             已收到买家拒绝通知（job_refused）。正在确认后续处理方案，请稍候。\n\n\
             **Step 2 — 把决策请求推到 user session 让用户拍板**（机制见 SKILL.md §Session 通信契约 §2 USER_DECISION_REQUEST 形态）：\n\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey，嵌入 `[sub_key: ...]` 行（user session agent 会用它反向 relay 决策回来）。\n\n\
             content：\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST · 仅询问用户 · user session agent 等用户回复后用 sub_key 反向 dispatch 回 sub，禁止自己执行 task CLI]\n\
             \x20\x20\x20\x20[sub_key: <session_status 拿到的 sessionKey 整串>]\n\
             \x20\x20\x20\x20[job: {job_id}]\n\
             \x20\x20\x20\x20任务 {job_id} 被买家拒绝。请选择：\n\
             \x20\x20\x20\x201. 发起仲裁 → 回复'发起仲裁，理由是<理由>'\n\
             \x20\x20\x20\x202. 同意退款 → 回复'同意退款'\n\n\
             **Step 3 — 等用户回复 relay 回来**：\n\
             收到 `[USER_DECISION_RELAY] 用户决策：...` 后，按关键词调 next-action：\n\
             - 含『发起仲裁』 → `--jobStatus dispute_raise`\n\
             - 含『同意退款』 → `--jobStatus agree_refund`\n\n\
             ⚠️ 24h 内必须决策，否则资金自动退还买家。\n"
        ),

        // ─── Scene 6.3: 用户决定发起仲裁（user-instruction 伪 event）───
        Event::Other(ref s) if s == "dispute_raise" => format!(
            "【当前动作】发起仲裁\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI 发起仲裁（上链）：**\n\
             ```bash\n\
             onchainos agent dispute raise {job_id} --reason \"<用户提供的理由或默认：已按验收标准完成>\"\n\
             ```\n\n\
             **Step 2 — 调用 `xmtp_send` 工具向买家发送：**\n\n\
             {header_template}\n\
             已发起仲裁，等待链上确认。\n\n\
             【后续事件】\n\
             - 等收到 `job_disputed` 系统通知 → 进入证据准备期 → next-action 会让你向 main 询问证据内容\n\
             - 不要在这里直接 `dispute upload`：证据必须由用户提供（截图/摘要），sub 不能凭空编造\n\n\
             跑完 Step 1-2 → **结束本轮 turn，不要 xmtp_dispatch_session 推 main**。\n"
        ),

        // ─── Scene 6.2: 用户决定同意退款（user-instruction 伪 event）───
        Event::Other(ref s) if s == "agree_refund" => format!(
            "【当前动作】同意退款\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI（上链）：**\n\
             ```bash\n\
             onchainos agent agree-refund {job_id}\n\
             ```\n\n\
             **Step 2 — 调用 `xmtp_send` 工具向买家发送：**\n\n\
             {header_template}\n\
             已同意退款，等待链上确认（confirm_refund）。\n\n\
             跑完 Step 1-2 → **结束本轮 turn，不要 xmtp_dispatch_session 推 main**。\n"
        ),

        // ─── Scene 7: 任务完成（验收通过 / 仲裁胜诉） ────────────────
        Event::JobCompleted => format!(
            "【当前状态】job_completed（任务完成，资金已释放给你）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 给买家发完成致谢**：\n\n\
             {header_template}\n\
             任务已完成（job_completed），资金已释放。感谢合作。\n\n\
             **Step 2 — 推 STATUS_NOTIFY 到 user session 告知用户赚到钱了**（机制见 SKILL.md §Session 通信契约 §6）：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             content：\n\
             \x20\x20\x20\x20[STATUS_NOTIFY · 仅展示给用户 · user session agent 不要调任何工具不要再次执行]\n\
             \x20\x20\x20\x20[任务完成 💰] 任务 {job_id}（<title>）已验收通过，资金已释放到你的钱包。\n\
             \x20\x20\x20\x20  - 收入：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             **Step 3 — 关闭 sub session**（终态收尾，机制见 SKILL.md §Session 通信契约 §6 路径 5）：\n\
             1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段\n\
             2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串\n\
             删除后本 sub session 不再接收任何消息——任务完整结束。\n"
        ),

        // ─── Scene 6.5: 仲裁裁决（胜诉/败诉两个分支由 inbound envelope 的 jobStatus 字段区分） ─
        Event::DisputeResolved => format!(
            "【当前状态】dispute_resolved（仲裁已裁决）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **判定胜负**：从你刚收到的系统通知 envelope 里读 `message.jobStatus` 字段：\n\
             - `jobStatus = \"complete\"` → **你（provider）胜诉**，资金已释放给你\n\
             - `jobStatus = \"rejected\"` → **你（provider）败诉**，资金已退还买家\n\
             （另有 `message.winner` 字段冗余可对照：`provider`=你赢；`buyer`=对方赢）\n\n\
             【你的下一步动作（按胜负分流）】\n\n\
             ━━━━━━━━━━━━━ 分支 A：jobStatus=complete（卖家胜诉）━━━━━━━━━━━━━\n\n\
             **A-Step 1 — 给买家发结果**（用 `xmtp_send`）：\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），裁决支持卖方。资金已释放。\n\n\
             **A-Step 2 — 推 STATUS_NOTIFY 到 user session**（机制见 SKILL.md §Session 通信契约 §6）：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             content：\n\
             \x20\x20\x20\x20[STATUS_NOTIFY · 仅展示给用户 · user session agent 不要调任何工具不要再次执行]\n\
             \x20\x20\x20\x20[仲裁胜诉 ⚖️💰] 任务 {job_id}（<title>）仲裁完成，**卖方胜诉**。\n\
             \x20\x20\x20\x20  - 收入：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=complete）\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             ━━━━━━━━━━━━━ 分支 B：jobStatus=rejected（卖家败诉）━━━━━━━━━━━━━\n\n\
             **B-Step 1 — 给买家发结果**（用 `xmtp_send`）：\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），裁决支持买方。资金已退还买家。\n\n\
             **B-Step 2 — 推 STATUS_NOTIFY 到 user session**：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             content：\n\
             \x20\x20\x20\x20[STATUS_NOTIFY · 仅展示给用户 · user session agent 不要调任何工具不要再次执行]\n\
             \x20\x20\x20\x20[仲裁败诉 ⚖️⚠️] 任务 {job_id}（<title>）仲裁完成，**买方胜诉**。\n\
             \x20\x20\x20\x20  - 损失：<tokenAmount> <tokenSymbol>（资金已退还买家）\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=rejected）\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
             **Step 3（两个分支都要做）— 关闭 sub session**（终态收尾，机制见 SKILL.md §Session 通信契约 §6 路径 5）：\n\
             1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段\n\
             2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串\n\
             删除后本 sub session 不再接收任何消息——仲裁流程完整结束。\n"
        ),

        // ─── Scene 6.5b: 卖家同意退款（TODO: 后端尚未定义此 event）───
        Event::ConfirmRefund => format!(
            "【当前状态】confirm_refund（卖家已同意退款，资金退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 给买家发收尾**：\n\n\
             {header_template}\n\
             已同意退款（confirm_refund），资金已退还买家。\n\n\
             **Step 2 — 关闭 sub session**（终态收尾，机制见 SKILL.md §Session 通信契约 §6 路径 5）：\n\
             1. 调 `session_status` 拿当前 sub session 的 `sessionKey` 字段\n\
             2. 调 `xmtp_delete_conversation`，参数 `sessionKey` = 第 1 步那串\n\
             删除后本 sub session 不再接收任何消息——退款流程完整结束。\n"
        ),

        // ─── Scene 6.4: 仲裁已上链，需用户提供证据 ───────────────────
        Event::JobDisputed => format!(
            "【当前状态】job_disputed（仲裁已上链，进入 1 小时证据准备期）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **证据内容必须由用户决策**——sub agent 不知道用户手上有什么证据（截图、聊天记录、交付物链接等），\n\
             不要凭空编造证据摘要直接调 `dispute upload`。**先把决策请求推到 user session 让用户拍板**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 向买家发一条状态告知（用 `xmtp_send` 工具）：**\n\n\
             {header_template}\n\
             仲裁已上链（job_disputed），正在准备证据材料。\n\n\
             **Step 2 — 把证据决策请求推到 user session 让用户提供内容**（机制见 SKILL.md §Session 通信契约 §2 USER_DECISION_REQUEST 形态）：\n\n\
             先调 `session_status` 拿到本 sub session 的 sessionKey，嵌入 `[sub_key: ...]` 行。\n\n\
             content：\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST · 仅询问用户 · user session agent 等用户回复后用 sub_key 反向 dispatch 回 sub，禁止自己执行 task CLI]\n\
             \x20\x20\x20\x20[sub_key: <session_status 拿到的 sessionKey 整串>]\n\
             \x20\x20\x20\x20[job: {job_id}]\n\
             \x20\x20\x20\x20任务 {job_id} 仲裁已上链，需要在 1 小时内提交链下证据。请提供：\n\
             \x20\x20\x20\x20- 文字摘要（必填）：说明你已按验收标准完成的关键证据点\n\
             \x20\x20\x20\x20- 图片路径（可选）：截图、设计稿、聊天记录等本地文件路径\n\
             \x20\x20\x20\x20回复格式示例：『证据：已按需求完成 X/Y/Z；图片：/path/to/screenshot.png』\n\n\
             **Step 3 — 等用户回复 relay 回来**：收到 `[USER_DECISION_RELAY] 用户证据：...` 后，调 `next-action --jobStatus dispute_evidence` 拿上传剧本。\n\n\
             ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
             跑完 Step 1-2 → **结束本轮 turn**，等用户回复。\n"
        ),

        // ─── Scene 6.4b: 用户已提供证据内容（user-instruction 伪 event）──
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】上传仲裁证据\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 从 relay 进来的用户消息中提取证据内容：**\n\
             - 文字摘要 → `--text` 参数\n\
             - 图片路径（如果用户提供了）→ `--image` 参数\n\
             text 和 image **至少一项**。\n\n\
             **Step 2 — 调用 CLI 上传证据（上链）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<用户提供的文字摘要>\" --image <用户提供的图片路径或省略>\n\
             ```\n\
             text 和 image **至少一项**；图片可省略整个 `--image` 段，不要给空字符串。\n\n\
             **Step 3 — 调用 `xmtp_send` 工具向买家发送：**\n\n\
             {header_template}\n\
             证据已提交，等待仲裁员裁决。\n\n\
             【后续事件】\n\
             - job_completed → 胜诉，资金释放给卖家\n\
             - dispute_resolved → 败诉，资金退还买家\n\n\
             跑完 Step 1-3 → **结束本轮 turn，不要 xmtp_dispatch_session 推 main**。\n"
        ),

        // ─── 未知类型兜底 ─────────────────────────────────────────────
        Event::JobCreated => format!(
            "【当前状态】job_created（任务上链）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **协商阶段，禁止直接调 `onchainos agent apply`**：apply 是链上动作（需 gas、签名上链），\n\
             协商失败无法撤销。必须先走完下方协商三项全部确认后再 apply。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             返回里包含【你的身份】（name、profileDescription）+【任务详情】（含「可见性」字段）+「专业匹配检查」区块。\n\n\
             **Step 2 — 按可见性 + 专业匹配分流**：\n\n\
             ━━━━━━━━━ 分支 A：可见性 = 公开（Public，openType=1）—— 主动联系买家 ━━━━━━━━━\n\n\
             A-Step 1：调 `xmtp_start_conversation` 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<task.buyerAgentId>（从 context 拿），jobId={job_id}\n\
             \x20\x20成功返回 sessionKey + xmtpGroupId。\n\n\
             A-Step 2：用 `xmtp_send` 给买家发协商三项确认（见 Step 3 模板）。\n\n\
             ━━━━━━━━━ 分支 B：可见性 = 私有（Private，openType=0）—— 被动等待 ━━━━━━━━━\n\n\
             B-Step 1：**不要主动建群**。等买家先 a2a-agent-chat envelope 到达（buyer 才有指定 provider 的权限）。\n\
             \x20\x20本轮 turn 结束，等下一条 inbound 进来再走 Step 3 协商三项确认。\n\
             \x20\x20（如果你已在某条 inbound a2a-agent-chat 触发的 sub session 里，跳过 B-Step 1，直接进 Step 3。）\n\n\
             ━━━━━━━━━ 共同：专业匹配判断 ━━━━━━━━━\n\n\
             看 context 里「专业匹配检查」区块：\n\
             - 领域匹配 → 进入 Step 3（私有任务等买家先来；公开任务是你 A-Step 2 主动发）\n\
             - 领域不匹配 → 按区块给出的拒绝模板调 `xmtp_send`（纯自然语言），结束\n\n\
             **Step 3 — 协商三项确认（一条 `xmtp_send` 内尽量一次问完）：**\n\
             1) 任务内容和验收标准是否在能力范围内\n\
             2) 价格可接受（币种必须是 XLayer 的 USDT 或 USDG，看任务详情里的 token 字段）\n\
             3) 支付方式可接受（escrow / non_escrow，由买家在 confirm-accept 时定）\n\
             → 用 `xmtp_send` 给买家发提问（机制见 SKILL.md §Session 通信契约 §6 路径 4）。\n\n\
             **Step 4 — 三项全部确认后，按双方约定的支付方式分流：**\n\n\
             ━━━━━ 分支 A：支付方式 = escrow（担保交易）→ 必须 apply 上链 ━━━━━\n\n\
             ```bash\n\
             onchainos agent apply {job_id} --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id {agent_id}\n\
             ```\n\
             apply 是上链签名动作，CLI 内部完成 unsigned info → sign → broadcast，等链上 provider_applied 通知。\n\n\
             ⚠️ **apply 跑完直接结束 turn，禁止 `xmtp_dispatch_session` 推 STATUS_NOTIFY 到 user session**——『已提交接单申请 / txHash / 等 provider_applied』是过场状态，对用户没信息量。等链上 `provider_applied` 通知到达后 next-action 那时才有值得推的。这条命令再说一遍是因为 sub 容易在 tx broadcast 后本能想『通知用户』——不要。\n\n\
             ━━━━━ 分支 B：支付方式 = non_escrow（非担保交易）→ **不要** apply，但要建 a2a-pay 付款单 ━━━━━\n\n\
             非担保交易不在链上托管资金，provider 端**禁止**调 `onchainos agent apply`：\n\
             - non_escrow 的链上 provider_applied 不会触发，调了 apply 会在 escrow 合约里多一笔无用上链\n\n\
             但卖家仍要为买家创建一张 a2a-pay charge 付款单：\n\n\
             ```bash\n\
             onchainos agent get-payment {job_id} --token-symbol <USDT|USDG> --token-amount <协商价格 whole tokens> --payment-mode non_escrow --agent-id {agent_id}\n\
             ```\n\
             stdout 输出 `{{ \"paymentId\": \"a2a_xxx\", \"deliveries\": ... }}`。\n\n\
             跑完后 `xmtp_send` 把 paymentId 发给买家（content 纯自然语言，不要贴整段 JSON）：\n\
             \x20\x20```\n\
             \x20\x20协商达成（非担保）。请用此 paymentId 完成支付：<a2a_xxx>\n\
             \x20\x20```\n\
             买家拿到 paymentId 后会调 `pay()` 完成 EIP-3009 单签 + credential 提交，然后再 confirm-accept 走 direct/accept 进入 accepted 状态。\n\n\
             跑完 get-payment + xmtp_send 后**直接结束本轮 turn**，等下一条系统通知（如 `job_accepted`）再调 next-action。\n\n\
             **任一项未达成** → 调 `xmtp_send` 回复\"很抱歉，无法接受当前条件\"（纯自然语言），结束。\n\n\
             【时限】整个协商在 5 分钟内完成；不要反复追问已经知道的信息。\n\n\
             【后续事件】\n\
             - 分支 A apply 上链成功 → 收到 `provider_applied` 系统通知 → 再次调 next-action 拿 Scene 3 剧本\n\
             - 分支 B 等买家 confirm-accept → 收到 `job_accepted` 系统通知 → 再次调 next-action\n"
        ),
        // ─── buyer 主导的 housekeeping 事件，provider 端基本无需动作 ─────
        Event::JobExpired
        | Event::JobClosed
        | Event::JobVisibilityChanged
        | Event::JobPaymentModeChanged
        | Event::SubmitExpired
        | Event::RefuseExpired
        | Event::ReviewExpired
        | Event::ReviewDeadlineWarn => format!(
            "【系统通知】{event}（buyer 端动作或超时事件）\n\
             【角色】卖家（Provider）\n\n\
             【建议】\n\
             - 静默观察即可，无需主动 xmtp_send\n\
             - 如需要详细信息，调用 `onchainos agent common context {job_id} --role provider`\n",
            event = event.as_str()
        ),

        // ─── provider 自己的截止提醒 ─────────────────────────────────────
        Event::SubmitDeadlineWarn => format!(
            "【系统通知】submit_deadline_warn（提交交付物截止时间快到了）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             如果交付物已准备好，立即调：\n\
             ```bash\n\
             onchainos agent deliver {job_id} --message \"<交付内容>\"\n\
             ```\n\
             否则在剩余时间内尽快完成交付，避免被 buyer 调 claimAutoRefund 退款。\n"
        ),

        // ─── 仲裁子状态机事件 — provider 关心 dispute_resolved（已有专门 arm），其他 evaluator 内部事件 provider 静默观察 ─────
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed => format!(
            "【系统通知】{event}（仲裁内部事件，evaluator 处理）\n\
             【角色】卖家（Provider）\n\n\
             【建议】静默观察即可。等 `dispute_resolved` 通知到达后再 next-action 处理收尾。\n",
            event = event.as_str()
        ),

        // ─── 质押 / 奖励 / 罚没 lifecycle — provider 不是 evaluator 时无关 ─────
        Event::Staked
        | Event::StakeIncreased
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed => format!(
            "【系统通知】{event}（evaluator 质押 lifecycle，provider 无关）\n\
             【建议】忽略即可。\n",
            event = event.as_str()
        ),

        // reward_claimed —— 自己的 claim tx 回执（可能 provider 也会 claim 仲裁奖励）
        Event::RewardClaimed => format!(
            "【系统通知】reward_claimed（claimRewards tx 回执）\n\
             【角色】卖家（Provider）\n\n\
             【建议】从 payload 提取 status / amount / txHash。如 status=success 表示奖励到账；\n\
             如 status=failed 按 errorCode 重试 `onchainos agent claim {job_id}`。\n"
        ),

        Event::Other(ref other) => format!(
            "【未知状态】{other}\n\
             【建议】\n\
             1. 调用 `onchainos agent common context {job_id} --role provider` 查看完整上下文\n\
             2. 如该状态不在预期流程内，等待用户指示\n\
             3. 不要预测/假设其他通知\n"
        ),
    };
    format!("{context_preamble}{body}")
}
