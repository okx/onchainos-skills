//! Provider 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 目的：把散落在 provider.md 里的 Scene 步骤集中到代码里，让 agent 只需
//! `exec onchainos agent next-action ...` 拿提示词直接执行，不用推理整份文档。

use crate::commands::agent_commerce::task::common::state_machine::Status;

/// Provider 在某 status 下可执行的 CLI 命令清单（用于 `agent common context` 输出末尾的菜单）。
///
/// 每个 status 列出主动作 + 一行索引指回 `next-action` 完整剧本（
/// `generate_next_action` 函数同文件，按 status 对应的 entry event 路由）。
/// 这样 menu 跟剧本不会再脱节——两份从同一个状态机视图衍生。
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action_hint = |evt: &str| {
        format!("onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role provider --agentId <agentId>  # 完整剧本")
    };
    match status {
        Status::Open => vec![
            format!("onchainos agent apply {job_id} --token-amount <price> --token-symbol USDT --agent-id <agentId>  # 申请接单（协商完成后才上链）"),
            next_action_hint("job_created"),
        ],
        Status::Accepted => vec![
            format!("onchainos agent deliver {job_id} --file <deliverable> --message <msg>  # 提交交付"),
            next_action_hint("job_accepted"),
        ],
        Status::Submitted => vec![
            "（被动等待）等待买家验收：job_completed → 任务完成；job_refused → 进入仲裁/退款决策".to_string(),
            next_action_hint("job_submitted"),
        ],
        Status::Refused => vec![
            format!("onchainos agent dispute raise {job_id} --reason <reason>  # 发起仲裁"),
            format!("onchainos agent agree-refund {job_id}  # 同意退款"),
            next_action_hint("job_refused"),
        ],
        Status::Disputed => vec![
            format!("onchainos agent dispute upload {job_id} --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
            next_action_hint("job_disputed"),
        ],
        Status::Completed => vec![
            "（流程结束）任务完成，资金已释放。子 session 可关闭。".to_string(),
            next_action_hint("job_completed"),
        ],
        Status::Refunded => vec![
            "（流程结束）资金已退还买家。子 session 可关闭。".to_string(),
            next_action_hint("confirm_refund"),
        ],
        Status::Other(s) => vec![
            format!("onchainos agent status {job_id}         # 当前状态 `{s}` 不在标准状态机内，先查最新状态"),
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

    // 通用前置：sub session 可能是 fresh state（直接跳转到中间状态、刚 replay 进入、
    // 或 main session 重启后 sub 还没 hydrate 协商记忆）。所有 chain-event 类剧本都先
    // 加载任务上下文，避免 agent 在不知道 deliverable / paymentMode / token 等字段
    // 的情况下盲跑后续 step。
    let context_preamble = format!(
        "🚨 **session 自检铁律（看到这段输出 = 你在 sub session，不要再判断）**：\n\
         `next-action` 命令**只在 sub session 里被调用**——main session agent 收到 \
         `[STATUS_NOTIFY ...]` / `[USER_DECISION_REQUEST ...]` 类消息是直接展示给用户，\n\
         **不会调** `next-action`。所以你看到这段 next-action 输出 ⇒ 100% 在 sub session。\n\n\
         **sessionKey 命名规则（关键，避免误判）**：\n\
         - main session 的 sessionKey **字面就是** `agent:main:main`（三段，无 xmtp / group / & 字段）\n\
         - sub session 的 sessionKey 形如 `agent:main:xmtp:group:okx-xmtp:my=0x...&to=0x...&job=<jobId>&gid=<groupId>`\n\
         - **`agent:main:` 只是命名空间**（在 main agent 这套会话集合里），**不代表你在 main session**\n\
         - **判断标准**：看 sessionKey 是否含 `xmtp:group:` 子串或 `&job=` 字段——含 = sub，不含 = main\n\
         - 你当前的 sessionKey 含 `&job={job_id}` ⇒ 你**在 sub session**，**不是 main**\n\n\
         **行为规则**：\n\
         - **不要**相信自己 thinking 里『我在 main session』『session_key starts with agent:main』的判断（那是命名空间误读）\n\
         - 任何『推到 main session』的指令必须用 `xmtp_dispatch_session` 工具**省略 sessionKey 参数**\n\
         - **禁止**把决策选项 / 状态通知直接以 assistant TEXT 输出（用户在 main 看不到，等于没推）\n\n\
         🔄 **前置上下文加载（如果是 fresh sub session 必做）**：\n\
         如果你不记得本 jobId 的协商细节（deliverable / paymentMode / token / 买家 agentId / 价格 等），\n\
         先调 CLI 加载完整上下文，再继续下面的 Step 1：\n\
         ```bash\n\
         onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
         ```\n\
         如果当前 turn 已经包含了任务上下文（紧跟首次询问的同一 sub session、或上一轮已经调过），\n\
         可以跳过这步，直接进 Step 1。\n\n\
         ─────────────────────────────────────────────────────────────\n\n"
    );

    let event = parse_status_or_event(job_status);
    let body = match event {
        // ─── Scene 3: 接单申请已上链，生成付款单给买家 ────────────────
        Event::ProviderApplied => format!(
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
        Event::JobAccepted => format!(
            "【当前状态】job_accepted（买家已确认接单，资金托管）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序，不得跳步）】\n\n\
             **Step 1 — 把接单成功通知推到 main session（用户那边）**：\n\n\
             ⚠️ 你**当前在 sub session**（agent:main:xmtp:group:...&job={job_id}&gid=...），**不是 main session**。\n\
             必须显式调 `xmtp_dispatch_session` 工具，**省略 sessionKey 参数**（这正是该工具向 main 派发的语义；\n\
             看工具 description：『可通过 sessionKey 指定目标 session，省略 sessionKey 则发送到 main session』）。\n\n\
             调用形式：\n\
             ```\n\
             tool: xmtp_dispatch_session\n\
             arguments:\n\
             \x20\x20content: |\n\
             \x20\x20\x20\x20[STATUS_NOTIFY · 仅展示给用户 · main agent 不要调任何工具不要再次执行]\n\
             \x20\x20\x20\x20[接单成功通知] 任务 {job_id} 已完成接单\n\
             \x20\x20\x20\x20- 标题：<title>\n\
             \x20\x20\x20\x20- 描述：<description>\n\
             \x20\x20\x20\x20- 协商价格：<amount> <tokenSymbol>\n\
             \x20\x20\x20\x20- 支付方式：<mode>\n\
             \x20\x20\x20\x20- 卖家 AgentID：{agent_id}\n\
             \x20\x20\x20\x20\n\
             \x20\x20\x20\x20资金已托管，sub session 卖家已开始执行任务。本通知由 sub session 推送，仅作状态同步，main agent 直接展示原文给用户。\n\
             ```\n\
             字段值从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 输出中提取。\n\n\
             ⚠️ **常见错误**：\n\
             - 把当前会话误判为 main session 而跳过这一步。当前 sessionKey 含 `&job=` 字段就一定是 sub session，不是 main。\n\
             - 不加 `[STATUS_NOTIFY · 仅展示给用户...]` 前缀 → main agent 会把通知当任务再执行一遍（重复调 deliver / xmtp_send / 等等）。**前缀是必填**。\n\n\
             **Step 2 — 向买家发 P2P 消息确认（走 xmtp_send 流程）：**\n\n\
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
             **Step 2 — 把决策请求推到 main session（让用户拍板，要把 sub session 的 sessionKey 嵌入消息让 main agent 知道往哪 relay 回）**：\n\n\
             先调 `session_status` 工具拿到当前 sub session 的 `sessionKey` 字段，再调 `xmtp_dispatch_session` **省略 sessionKey 参数**（工具描述：『省略 sessionKey 则发送到 main session』），把 sessionKey 嵌入 content 的 metadata 行：\n\n\
             ```\n\
             tool: xmtp_dispatch_session\n\
             arguments:\n\
             \x20\x20content: |\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST · 仅询问用户 · main agent 等用户回复后用 sub_key 反向 dispatch 回 sub，禁止自己执行 task CLI]\n\
             \x20\x20\x20\x20[sub_key: <粘贴你刚才 session_status 拿到的 sessionKey 整串>]\n\
             \x20\x20\x20\x20[job: {job_id}]\n\
             \x20\x20\x20\x20任务 {job_id} 被买家拒绝。请选择：\n\
             \x20\x20\x20\x201. 发起仲裁 → 回复'发起仲裁，理由是<理由>'\n\
             \x20\x20\x20\x202. 同意退款 → 回复'同意退款'\n\
             ```\n\n\
             **Step 3 — 等待主 session 用户决策**：\n\
             用户在 main session 回复后，main agent 会**用 `sub_key` 反向 `xmtp_dispatch_session` 把决策 relay 回 sub**（你这里）。\n\
             收到 relay 进来的用户决策（含『发起仲裁』或『同意退款』关键词）后，再次调 next-action：\n\
             - 用户『发起仲裁，理由是<X>』 → `--jobStatus dispute_raise`\n\
             - 用户『同意退款』 → `--jobStatus agree_refund`\n\n\
             ⚠️ 24h 内必须决策，否则资金自动退还买家。\n"
        ),

        // ─── Scene 6.3: 用户决定发起仲裁（user-instruction 伪 event）───
        Event::Other(ref s) if s == "dispute_raise" => format!(
            "【当前动作】发起仲裁\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **不要主动推 STATUS_NOTIFY 到 main session**——用户刚刚自己回复『发起仲裁』，\n\
             已经知道这个动作。tx broadcast 后/等链上确认是过场状态，对用户没信息量。\n\
             等收到链上 `job_disputed` 系统通知后，再 next-action 拿剧本（那时才推有意义的『仲裁已上链』通知）。\n\n\
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
             ⚠️ **不要主动推 STATUS_NOTIFY 到 main session**——用户刚刚自己回复『同意退款』，\n\
             已经知道这个动作。等收到链上 `confirm_refund` 通知后，再 next-action 拿收尾剧本。\n\n\
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
            "【当前状态】job_completed（任务完成，资金已释放）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家调用 `xmtp_send` 工具发送消息：\n\n\
             {header_template}\n\
             任务已完成（job_completed），资金已释放。感谢合作。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.5a: 仲裁败诉（资金退还买家） ────────────────────
        Event::DisputeResolved => format!(
            "【当前状态】dispute_resolved（仲裁已裁决，资金退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家调用 `xmtp_send` 工具发送消息：\n\n\
             {header_template}\n\
             仲裁已裁决（dispute_resolved），资金已退还买家。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.5b: 卖家同意退款（TODO: 后端尚未定义此 event）───
        Event::ConfirmRefund => format!(
            "【当前状态】confirm_refund（卖家已同意退款，资金退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             向买家调用 `xmtp_send` 工具发送消息：\n\n\
             {header_template}\n\
             已同意退款（confirm_refund），资金已退还买家。\n\n\
             【流程结束】子 session 可以关闭。\n"
        ),

        // ─── Scene 6.4: 仲裁已上链，需用户提供证据 ───────────────────
        Event::JobDisputed => format!(
            "【当前状态】job_disputed（仲裁已上链，进入 1 小时证据准备期）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **证据内容必须由用户决策**——sub agent 不知道用户手上有什么证据（截图、聊天记录、交付物链接等），\n\
             不要凭空编造证据摘要直接调 `dispute upload`。**先把决策请求推到 main session 让用户拍板**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 向买家发一条状态告知（用 `xmtp_send` 工具）：**\n\n\
             {header_template}\n\
             仲裁已上链（job_disputed），正在准备证据材料。\n\n\
             **Step 2 — 把证据决策请求推到 main session（让用户提供证据内容，要把 sub session 的 sessionKey 嵌入消息让 main agent 知道往哪 relay 回）：**\n\n\
             先调 `session_status` 工具拿到当前 sub session 的 `sessionKey` 字段，再调 `xmtp_dispatch_session` **省略 sessionKey 参数**（『省略 sessionKey 则发送到 main session』），把 sessionKey 嵌入 content 的 metadata 行：\n\n\
             ```\n\
             tool: xmtp_dispatch_session\n\
             arguments:\n\
             \x20\x20content: |\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST · 仅询问用户 · main agent 等用户回复后用 sub_key 反向 dispatch 回 sub，禁止自己执行 task CLI]\n\
             \x20\x20\x20\x20[sub_key: <粘贴你刚才 session_status 拿到的 sessionKey 整串>]\n\
             \x20\x20\x20\x20[job: {job_id}]\n\
             \x20\x20\x20\x20任务 {job_id} 仲裁已上链，需要在 1 小时内提交链下证据。请提供：\n\
             \x20\x20\x20\x20- 文字摘要（必填）：说明你已按验收标准完成的关键证据点\n\
             \x20\x20\x20\x20- 图片路径（可选）：截图、设计稿、聊天记录等本地文件路径\n\
             \x20\x20\x20\x20回复格式示例：『证据：已按需求完成 X/Y/Z；图片：/path/to/screenshot.png』\n\
             ```\n\n\
             **Step 3 — 等待主 session 用户回复**：\n\
             用户在 main session 回复后，main agent 会**用 `sub_key` 反向 `xmtp_dispatch_session` 把证据内容 relay 回 sub**（你这里）。\n\
             收到 relay 进来的证据内容后，再次调 next-action：\n\
             - `--jobStatus dispute_evidence` → 拿到上传证据的剧本\n\n\
             ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
             跑完 Step 1-2 → **结束本轮 turn**，等用户回复 relay 回来再继续。\n"
        ),

        // ─── Scene 6.4b: 用户已提供证据内容（user-instruction 伪 event）──
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】上传仲裁证据\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **不要主动推 STATUS_NOTIFY 到 main session**——用户刚刚自己提供了证据内容，\n\
             已经知道要上传。tx 上链是过场状态，对用户没信息量。\n\
             等仲裁裁决（job_completed 或 dispute_resolved）的系统通知到达后，再 next-action 拿收尾剧本。\n\n\
             **Step 1 — 从 relay 进来的用户消息中提取证据内容：**\n\
             - 文字摘要 → `--text` 参数\n\
             - 图片路径（如果用户提供了）→ `--image` 参数\n\
             text 和 image **至少一项**。\n\n\
             **Step 2 — 调用 CLI 上传证据（上链）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --text \"<用户提供的文字摘要>\" --image <用户提供的图片路径或省略>\n\
             ```\n\
             **🛑 该命令只接受这三个参数 `<JOB_ID>` / `--text` / `--image`，禁止添加任何其他参数**：\n\
             - ❌ 不要加 `--agent-id` / `--agentId`（这是 apply / accept 等命令的参数，不属于 upload）\n\
             - ❌ 不要加 `--provider` / `--role` / `--from` 等任何角色标记（CLI 自动从钱包地址识别）\n\
             - ❌ 不要在 image 路径前后加引号包裹其他参数；如果用户没提供图片，直接省略整个 `--image` 段，不要给空字符串\n\
             一旦 CLI 报 `unexpected argument` 错，**不要换名重试**（比如 `--agent-id` 失败就换 `--agentId`）——签名就是这三个参数，重试只会再失败。直接按上面命令重发。\n\n\
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
            "【当前状态】job_created（任务上链 / 买家首次询问 a2a-agent-chat）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **协商阶段，禁止直接调 `onchainos agent apply`**：apply 是链上动作（需 gas、签名上链），\n\
             协商失败无法撤销。必须先走完下面 Step 1 / 2 / 3，三项全部确认后再 apply。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             返回里包含【你的身份】（name、profileDescription）+【任务详情】+「专业匹配检查」区块。\n\n\
             **Step 2 — 专业匹配判断（按 context 输出的「专业匹配检查」区块严格执行）：**\n\
             - 领域匹配（任务关键词落在你的 profileDescription 范围内）→ 进入 Step 3\n\
             - 领域不匹配 → 按区块给出的拒绝模板调 `xmtp_send`（纯自然语言，不写 text-header），结束\n\n\
             **Step 3 — 协商三项确认（一条 xmtp_send 回复内尽量一次问完）：**\n\
             1) 任务内容和验收标准是否在能力范围内\n\
             2) 价格可接受（币种必须是 XLayer 的 USDT 或 USDG，看任务详情里的 token 字段）\n\
             3) 支付方式可接受（escrow / non_escrow，由买家在 confirm-accept 时定）\n\n\
             两步必做（不能跳第 1 步）：\n\
             1) 先调 `session_status` 工具拿到当前子 session 的 `sessionKey`\n\
             2) 再调 `xmtp_send` 工具，参数 `sessionKey`=上面拿到的值，`content`=三项确认的纯自然语言提问\n\n\
             **Step 4 — 三项全部确认后（且仅在此时）才调 apply：**\n\
             ```bash\n\
             onchainos agent apply {job_id} --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id {agent_id}\n\
             ```\n\
             apply 是上链签名动作，CLI 内部完成 unsigned info → sign → broadcast，等链上 provider_applied 通知。\n\n\
             **任一项未达成** → 调 `xmtp_send` 回复\"很抱歉，无法接受当前条件\"（纯自然语言），结束。\n\n\
             【时限】整个协商在 5 分钟内完成；不要反复追问已经知道的信息。\n\n\
             【后续事件】\n\
             - apply 上链成功 → 收到 `provider_applied` 系统通知 → 再次调 next-action 拿 Scene 3 剧本\n"
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
