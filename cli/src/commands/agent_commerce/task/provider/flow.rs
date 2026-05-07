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
            format!("  onchainos agent apply {job_id} --token-amount <price> --token-symbol <USDT|USDG，看任务详情> --agent-id <agentId>  # 申请接单（**仅 escrow 担保交易**才需要；non_escrow 不要 apply）"),
            format!("  onchainos agent get-payment {job_id} --token-symbol <USDT|USDG，看任务详情> --token-amount <price> --payment-mode <escrow|non_escrow> --agent-id <agentId>  # 拉 prePayTaskInfo + 创建 a2a-pay 付款单，输出 paymentId"),
        ],
        Status::Accepted => vec![
            next_action("job_accepted"),
            ref_header,
            format!("  onchainos agent deliver {job_id} --file <deliverable> --message <msg> --agent-id <agentId>  # 提交交付（**仅 status=accepted 才允许**，CLI 会强制校验，apply 后立即 deliver 会被拒）"),
        ],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "（被动等待）等待买家验收：job_completed → 任务完成；job_refused → 进入仲裁/退款决策".to_string(),
        ],
        Status::Refused => vec![
            next_action("job_refused"),
            ref_header,
            format!("  onchainos agent dispute raise {job_id} --reason <reason> --agent-id <agentId>  # 发起仲裁"),
            format!("  onchainos agent agree-refund {job_id} --agent-id <agentId>  # 同意退款"),
        ],
        Status::Disputed => vec![
            next_action("job_disputed"),
            ref_header,
            format!("  onchainos agent dispute upload {job_id} --agent-id <你的agentId> --text \"<摘要>\" --image <图片>  # 1h 准备期内提交证据"),
        ],
        Status::Completed => vec![
            next_action("job_completed"),
            "（终态）任务已 COMPLETE — **资金已释放给你（卖家）**".to_string(),
            "  ▸ 买家验收通过（job_completed）→ 托管款已释放".to_string(),
            "  ▸ 仲裁卖家胜（dispute_resolved seller-wins）→ 托管款已释放".to_string(),
            "子 session 可关闭。".to_string(),
        ],
        Status::Rejected => vec![
            next_action("job_refunded"),
            "（终态）任务已 REJECTED — **资金已退还买家**".to_string(),
            "  ▸ 你同意退款（agree-refund）/ 自动退款 → 资金原路返回买家".to_string(),
            "  ▸ 仲裁买家胜（dispute_resolved buyer-wins）→ 退款".to_string(),
            "子 session 可关闭。".to_string(),
        ],
        Status::Close => vec![
            "任务已被买家关闭（Close）。子 session 可关闭。".to_string(),
        ],
        Status::Expired => vec![
            "任务已过期（Expired）。子 session 可关闭。".to_string(),
        ],
        Status::AdminStopped => vec![
            "任务已被管理员停止（AdminStopped）。请联系平台客服了解原因。".to_string(),
        ],
        Status::Init => vec![
            "任务初始化中（等待上链确认）→ 等待 job_created 事件".to_string(),
        ],
        Status::Other(s) => vec![
            format!("当前任务 status=`{s}` 不在 provider 关心的状态集（open / accepted / submitted / refused / disputed / completed / rejected / close / expired / admin_stopped）内"),
            "→ 本角色无需任何任务级动作，等下一个相关链事件 / 用户决策再处理".to_string(),
            "→ **不要**重复跑 `agent status` / `agent common context`（结果会一样），结束本轮 turn".to_string(),
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
    // 通信机制（怎么发、能不能发、形态白名单）— 一律见 SKILL.md Session 通信契约。
    // 本文件只负责告诉 agent **每一步把什么内容发到哪**，不重复解释工具用法。
    //
    // 三种通信工具：
    //   - xmtp_send：发给买家（peer sub session），参数 sessionKey + content
    //   - xmtp_dispatch_user：通知用户（无需用户决策），参数：content
    //   - xmtp_prompt_user：需要用户交互（确认 / 决策），参数：llmContent + userContent
    //     llmContent = 注入 user session LLM 的指令（用户不可见，含 sub_key 让 user agent
    //                  把决策 relay 回 sub）
    //     userContent = 发送给用户的可见消息
    //
    // 老的 `xmtp_dispatch_session` 省略 sessionKey + `[STATUS_NOTIFY]` 包裹形态已被
    // `xmtp_dispatch_user` / `xmtp_prompt_user` 替代——本文件不再用 dispatch_session 推用户。
    // 注：`[USER_DECISION_REQUEST]` 标记仍出现在 `xmtp_prompt_user` 的 llmContent 里，
    // 这是给 user agent 识别"待用户决策"用的内联 tag，不是老的 envelope wrapper——
    // user agent 拿 sub_key 后通过 path 3 (`xmtp_dispatch_session(sessionKey=<sub>, [USER_DECISION_RELAY] ...)`) 反推回 sub。
    // ──────────────────────────────────────────────────────────────────────
    let send_to_peer = format!(
        "→ 用 `xmtp_send` 发给买家（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 1.4）。\n\
         当前 sub session：jobId={job_id}，我方 agentId={agent_id}。\n\
         content（纯自然语言，不要包 markdown / 代码块）："
    );
    // 兼容旧变量名
    let header_template = &send_to_peer;

    let context_preamble = format!(
        "📍 你在 sub session（你看到这段 next-action 输出 = 100% 在 sub）。\n\n\
         🔒 **如果当前 turn 没读过 skills/okx-agent-task/SKILL.md Session 通信契约**（envelope 形态白名单 / xmtp_send 两步 / xmtp_dispatch_user·xmtp_prompt_user 推 user session 铁律），\n\
         **先读 `skills/okx-agent-task/SKILL.md`** 再继续——下面步骤会引用它的章节（3 / 4 / 5 / 6）。\n\n\
         ⚠️ **异常升级硬规则**（任何场景都适用，详见 _shared/exception-escalation.md + provider.md 5）：\n\
         \x20\x201) 协议理解错位：你已澄清同一条流程 ≥1 次，对方下一条还在重复错误诉求 → **不再回复对方**，调 `xmtp_dispatch_user` 推 `[⚠️ 协议理解错位] ...`，结束 turn\n\
         \x20\x202) CLI 错误：`onchainos agent <cmd>` 报错 → **不要重试**，直接调 `xmtp_dispatch_user` 推 `[⚠️ CLI 报错] ...`，等用户新指令。**唯一例外**：JWT 过期（msg 含 `JWT verification failed` / `unauthorized`）刷新登录态后自动重试一次；网络 timeout 也按业务错处理推用户，不在 sub 里盲重\n\
         \x20\x203) ❌ **绝对禁止把技术错误细节广播给对方**：CLI 命令名 / 后端字段名 / stderr 摘要 / `bug`/`命令：`/`错误：` 一律不能进 xmtp_send 给对方。最多发一句『稍等，正在确认细节』或干脆不通知对方。\n\
         \x20\x204) ❌ **同 turn 不重复 xmtp_send**：剧本说『发一条』→ 调过一次工具返回『已发送』就**算成功**，**当前 turn 内不再对同一对方调 xmtp_send 第二次**。不要因为消息可能不够清晰就重发——重发 = 刷屏 + 触发对方循环。下一条 inbound 进来再说。\n\
         \x20\x205) ❌ **deliver 必须等 `job_accepted` 通知**：apply 上链不改 status，任务仍是 open；只有买家 confirm-accept 触发的 `job_accepted` 链事件到达后才能 deliver。**绝对禁止在 ProviderApplied 剧本里抢跑 deliver**，CLI 会校验 status != accepted 直接 bail。\n\
         \x20\x206) ❌ **同 turn 不重复 `session_status`**：sub session 的 sessionKey 在同一 turn 内是稳定的——**调过一次就把结果存住，后续 step 直接复用**。即使剧本多个 step 都提到 sessionKey，也只调一次 session_status。重复调 = 死循环征兆，必须立即停。\n\n\
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
             ⚠️ 不要再调 `onchainos agent get-payment`——那是 non_escrow 路径才用的。\n\
             ⚠️ **本阶段绝对禁止调 `onchainos agent deliver`**：当前 status 仍是 open（apply 上链不改 status），\n\
             必须等买家 confirm-accept 上链 + 你收到 `job_accepted` 通知后才能 deliver。\n\
             CLI 已加防御：deliver 在 status != accepted 时会直接报错——但你应该一开始就不要尝试。\n\n\
             跑完 xmtp_send → **直接结束本轮 turn**，等买家 confirm-accept 触发的 `job_accepted` 通知再进入 Scene 4。\n\n\
             【后续事件】\n\
             - job_accepted → 买家已 confirm-accept，资金托管完成，**那时才能** deliver\n"
        ),

        // ─── Scene 4: 买家已确认接单，执行任务并交付（按 paymentMode 分流） ──
        Event::JobAccepted => format!(
            "【当前状态】job_accepted（买家已确认接单，资金托管）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序，不得跳步）】\n\n\
             **Step 1 — 用 `xmtp_dispatch_user` 把接单成功通知推给用户**：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[接单成功通知] 任务 {job_id} 已完成接单\n\
             \x20\x20\x20\x20- 标题：<title>\n\
             \x20\x20\x20\x20- 描述：<description>\n\
             \x20\x20\x20\x20- 协商价格：<amount> <tokenSymbol>\n\
             \x20\x20\x20\x20- 支付方式：<mode>\n\
             \x20\x20\x20\x20- 卖家 AgentID：{agent_id}\n\
             \x20\x20\x20\x20资金已托管，sub session 卖家已开始执行任务。\n\n\
             字段值从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 输出中提取。\n\
             ⚠️ 不要给买家 `xmtp_send`「已收到接单确认」过场——买家自己刚 confirm-accept，他知道。\n\n\
             **Step 2 — 执行任务**，按交付物准备好。\n\n\
             **Step 3 — 按支付方式分流交付**（必须先调 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 确认 paymentMode）：\n\n\
             ━━━━━ 分支 A：paymentMode=escrow（担保交易，1）━━━━━\n\n\
             ⚠️ **顺序很重要**：先把交付物 xmtp_send 给买家，再 deliver 上链。\n\
             之前的设计是 deliver 先上链等 `job_submitted` 通知再 xmtp_send，但 `job_submitted` 系统事件不是 100% 可达；\n\
             买家拿不到交付物会直接 reject，浪费仲裁押金。所以现在改成「**先发交付物 → 再上链**」，\n\
             链上确认只是把 task 状态推到 submitted（让买家有 complete/reject 入口），交付物本身已经送到了。\n\n\
             **A-Step 1 — 准备交付物（按类型分流）**：\n\n\
             ▸ **纯文本/URL 交付物**：直接组好文字内容，跳过 xmtp_file_upload，进入 A-Step 2\n\n\
             ▸ **文件交付物**（图片/PDF/文档）：调 `xmtp_file_upload`（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.8）：\n\
             \x20\x20参数 `filePath` = 本地文件绝对路径，`agentId` = {agent_id}，`jobId` = {job_id}\n\
             \x20\x20返回值 `fileKey` / `digest` / `salt` / `nonce` / `secret` 五个字段（解密元数据）全部记录\n\n\
             **A-Step 2 — `xmtp_send` 把交付物发给买家**（同 turn 内紧接着 A-Step 1 跑）：\n\n\
             文本交付物 content：\n\
             {header_template}\n\
             任务 {job_id} 已完成。交付物：\n\
             <这里贴交付内容文本>\n\
             请你验收并调 `onchainos agent complete {job_id}` 释放款项；如有问题调 `onchainos agent reject` 反馈。\n\n\
             文件交付物 content（5 个字段原样塞）：\n\
             {header_template}\n\
             任务 {job_id} 已完成。以下是交付信息：\n\
             - fileKey: <A-Step 1 返回的 fileKey 完整字符串>\n\
             - digest: <A-Step 1 返回的 digest>\n\
             - salt: <A-Step 1 返回的 salt>\n\
             - nonce: <A-Step 1 返回的 nonce>\n\
             - secret: <A-Step 1 返回的 secret>\n\
             - filename: <A-Step 1 返回的 filename>\n\
             请用 xmtp_file_download 下载查看，确认无误后调 `onchainos agent complete {job_id}` 释放款项。\n\n\
             **A-Step 3 — `deliver` CLI 上链**（把 task 状态推到 submitted，让买家拿到 complete 入口）：\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --message \"任务已完成，请验收\" --agent-id {agent_id}\n\
             ```\n\
             CLI 内部：POST submit API → 签名 uopHash → 广播上链。\n\n\
             **A-Step 4 — 跑完 A-Step 3 直接结束本轮 turn**：\n\
             ⚠️ 不需要等 `job_submitted` 通知——交付物已经在 A-Step 2 送到买家了。\n\
             ⚠️ 禁止此时再给买家 xmtp_send 任何过场（『已上链请验收』之类）。\n\
             ⚠️ 禁止 `xmtp_dispatch_user` 推用户。\n\
             ⚠️ Scene 5 (`job_submitted`) 收到时只观察、不再 xmtp_send（避免给买家发双消息）。\n\n\
             ━━━━━ 分支 B：paymentMode=non_escrow（非担保交易，2）━━━━━\n\n\
             非担保不走链上 submit，**直接 xmtp_send 把交付物发给买家**。**按交付物类型分流**：\n\n\
             ▸ **纯文本交付物**（一段话、一段查询结果、一段 URL 链接）：\n\
             {header_template}\n\
             任务 {job_id} 已完成。交付物：\n\
             <这里贴交付内容文本>\n\
             请你验收并调 `onchainos agent complete {job_id}` 释放款项；如有问题调 `onchainos agent reject` 反馈。\n\n\
             ▸ **文件交付物**（图片/PDF/文档）—— 用 `xmtp_file_upload + xmtp_send fileKey` 两步（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.8）：\n\
             ⚠️ **B-1 和 B-2 必须同 turn 内连续执行**——`xmtp_file_upload` 调完拿到返回值后**立即**接着调 `xmtp_send`，不要把 turn 切断。上传完不发 fileKey 给买家 = 买家完全收不到交付物。\n\
             B-1. 调 `xmtp_file_upload`，参数 `filePath` = 本地文件绝对路径，`agentId` = {agent_id}，`jobId` = {job_id}\n\
             \x20\x20\x20返回值 `fileKey` / `digest` / `salt` / `nonce` / `secret` 五个字段（解密元数据）全部记录\n\
             B-2. **同 turn 内**继续调 `xmtp_send` 给买家（必须把 B-1 拿到的 5 个字段原样塞进 content）：\n\
             {header_template}\n\
             任务 {job_id} 已完成。以下是交付信息：\n\
             - fileKey: <B-1 返回的 fileKey 完整字符串>\n\
             - digest: <B-1 返回的 digest>\n\
             - salt: <B-1 返回的 salt>\n\
             - nonce: <B-1 返回的 nonce>\n\
             - secret: <B-1 返回的 secret>\n\
             - filename: <B-1 返回的 filename，例如 task预发.png>\n\
             请用 xmtp_file_download 下载查看，确认无误后调 `onchainos agent complete {job_id}` 释放款项；如有问题调 `onchainos agent reject` 反馈。\n\n\
             B-Step 后续：等买家 user session 决策 → 若买家完成验收会触发后续事件；non_escrow 卖家这条 turn 跑完一条 xmtp_send 即结束。\n\
             ⚠️ **禁止 non_escrow 路径调 `onchainos agent deliver`**——deliver 是 escrow 链上动作，non_escrow 调会被后端拒。\n\n\
             【后续事件】\n\
             - 分支 A → 链上 task 状态进 submitted（job_submitted 系统事件可能到达，仅观察不动作）→ 等 buyer complete/reject\n\
             - 分支 B → 买家直接验收，无中间链事件\n"
        ),

        // ─── Scene 5: 交付物已上链（observer-only） ──────────────────
        // 新流程下交付物已经在 Scene 4 A-Step 2 用 xmtp_send 发给买家了，job_submitted
        // 系统事件到达本 sub 时不需要再 xmtp_send，避免买家收双消息。
        Event::JobSubmitted => format!(
            "【系统通知】job_submitted（交付物已上链确认，task 状态进入 submitted）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **observer-only**：交付物已经在 Scene 4 A-Step 2（escrow 路径）或 Scene 4 分支 B（non_escrow 路径）发给买家了，本事件**不需要再 xmtp_send 第二次**——重复发会让买家 sub 收到双消息触发循环。\n\n\
             【你的下一步动作】\n\
             - **静默观察即可**，不要 xmtp_send / xmtp_file_upload / xmtp_dispatch_user / xmtp_prompt_user\n\
             - **直接结束本轮 turn**，等买家 complete/reject 触发后续事件\n\n\
             【后续事件】\n\
             - job_completed → 验收通过，调用 next-action 进 Scene 7 收尾\n\
             - job_refused   → 买家拒绝，调用 next-action 进 Scene 6 决策\n"
        ),

        // ─── Scene 6: 买家拒绝交付物 ─────────────────────────────────
        Event::JobRefused => format!(
            "【当前状态】job_refused（买家拒绝交付物）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             ⚠️ 不要给买家 `xmtp_send`「已收到拒绝通知」过场——买家自己刚 reject，他知道。直接进入用户决策流程。\n\n\
             **Step 1 — 用 `xmtp_prompt_user` 把决策请求推到用户**：\n\n\
             先调 `session_status` 拿当前 sub session 的 sessionKey（同 turn 内调一次即可），\n\
             user session agent 拿到 llmContent 后会按 `sub_key` 把用户决策反向 dispatch 回本 sub。\n\n\
             tool: xmtp_prompt_user\n\
             llmContent:\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}] \
             用户回复决策后，relay 回 sub session 执行 next-action。禁止 user session agent 自己执行 task CLI。24h 内必须决策。\n\
             userContent:\n\
             \x20\x20\x20\x20任务 {job_id} 被买家拒绝。请选择：\n\
             \x20\x20\x20\x201. 发起仲裁 → 回复『发起仲裁，理由是<理由>』\n\
             \x20\x20\x20\x202. 同意退款 → 回复『同意退款』\n\n\
             **Step 2 — 等用户回复 relay 回来**：\n\
             收到 `[USER_DECISION_RELAY] 用户决策：...` 后，按关键词调 next-action：\n\
             - 含『发起仲裁』 → `--jobStatus dispute_raise`\n\
             - 含『同意退款』 → `--jobStatus agree_refund`\n\n\
             ⚠️ 24h 内必须决策，否则资金自动退还买家。\n"
        ),

        // ─── Scene 6.3: 用户决定发起仲裁（user-instruction 伪 event）───
        Event::Other(ref s) if s == "dispute_raise" => format!(
            "【当前动作】发起仲裁 — 阶段 1（approve）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **仲裁是两阶段链上流程**：阶段 1 approve → 等 `dispute_approved` 通知 → 阶段 2 dispute → 等 `job_disputed` 通知。本轮只跑阶段 1。\n\n\
             **Step 1 — 调用 CLI 跑阶段 1 approve（上链）：**\n\
             ```bash\n\
             onchainos agent dispute raise {job_id} --reason \"<用户提供的理由或默认：已按验收标准完成>\" --agent-id {agent_id}\n\
             ```\n\
             CLI 内部：POST /dispute/approve → uopData → sign uopHash → broadcast。等链上 `dispute_approved` 通知。\n\n\
             ⚠️ **跑完 dispute raise 直接结束 turn**：\n\
             - 禁止给买家发任何 xmtp_send（『已发起仲裁』之类是过场状态，等阶段 2 完成再说）\n\
             - 禁止在同一 turn 内调 `dispute confirm`（必须等链上 dispute_approved 通知到达）\n\n\
             【后续事件】\n\
             - `dispute_approved` 系统通知 → 调 next-action 拿阶段 2 剧本（dispute confirm）\n\
             - 之后才会进入 `job_disputed` → 证据准备期\n"
        ),

        // ─── Scene 6.3.5: 仲裁阶段 1 approve 上链确认 → 跑阶段 2 dispute ─
        Event::DisputeApproved => format!(
            "【当前状态】dispute_approved（仲裁阶段 1 approve 已上链，进入阶段 2）\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI 跑阶段 2 dispute（上链）：**\n\
             ```bash\n\
             onchainos agent dispute confirm {job_id} --agent-id {agent_id}\n\
             ```\n\
             CLI 内部：POST /dispute → uopData → sign uopHash → broadcast。等链上 `job_disputed` 通知。\n\n\
             ⚠️ **跑完 dispute confirm 直接结束 turn**：\n\
             - 禁止给买家 xmtp_send（仍是过场状态）\n\
             - 禁止在同一 turn 内提交证据（证据走 dispute upload，要等 `job_disputed` 通知 + 用户提供内容）\n\n\
             【后续事件】\n\
             - `job_disputed` 系统通知 → 进入 1 小时证据准备期 → next-action 会让你向 user session 询问证据内容\n"
        ),

        // ─── Scene 6.2: 用户决定同意退款（user-instruction 伪 event）───
        Event::Other(ref s) if s == "agree_refund" => format!(
            "【当前动作】同意退款\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 调用 CLI（上链）：**\n\
             ```bash\n\
             onchainos agent agree-refund {job_id} --agent-id {agent_id}\n\
             ```\n\n\
             跑完 Step 1 → **结束本轮 turn**。\n\
             ⚠️ 不要给买家 `xmtp_send`「已同意退款」过场——双方都会收到 `job_refunded` 系统事件。\n\
             ⚠️ 不要 `xmtp_dispatch_user` 推用户。\n"
        ),

        // ─── Scene 7: 任务完成（验收通过 / 仲裁胜诉） ────────────────
        Event::JobCompleted => format!(
            "【当前状态】job_completed（任务完成，资金已释放给你）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             ⚠️ 不要给买家 `xmtp_send` 致谢/「已完成」过场——买家自己刚 complete，他知道。\n\n\
             **Step 1 — 拿任务上下文**：\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             提取 title + tokenAmount + tokenSymbol + buyerAgentId（下一步要用）。\n\n\
             **Step 2 — 用 `xmtp_dispatch_user` 推用户：通知任务完成 + 触发 user session 走 `okx-agent-identity` §Feedback Submit 评价买家**：\n\n\
             ⚠️ sub agent **不直接调** `feedback-submit` CLI——评分 / 评语需要用户拍板，sub 没有用户交互通道。改为推一条带评价指令的通知到 user session，由 user session main agent 激活 `okx-agent-identity` 的 §Feedback Submit workflow 完整接管（拉评分 + 评语 + 确认卡片 + 上链）。\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[任务完成 💰⭐] 任务 {job_id}（<title>）已验收通过，资金已释放到你的钱包。\n\
             \x20\x20\x20\x20  - 收入：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\n\
             \x20\x20\x20\x20⭐ 请给买家 <buyerAgentId> 打个分（0-100）+ 一句简评——我即将调 `okx-agent-identity` §Feedback Submit 接管：\n\
             \x20\x20\x20\x20\x20\x20- 被评价方 (--agent-id) = <buyerAgentId>\n\
             \x20\x20\x20\x20\x20\x20- 评价发起方 (--creator-id) = {agent_id}\n\
             \x20\x20\x20\x20\x20\x20- 任务 ID (--task-id) = {job_id}\n\
             \x20\x20\x20\x20\x20\x20- score / description：等用户拍板\n\
             \x20\x20\x20\x20score 参考：100 = 验收爽快无纠纷 / 80 = 顺利但有小磨合 / 60 = 一般 / 40 以下 = 体验差。\n\n\
             **Step 3 — 推完 user session 立即结束本轮 turn**：\n\
             - 不要等用户回复再做什么——user session main agent 拿到上面 content 后会自己激活 identity skill 走完 feedback-submit 流程，跟 sub 解耦。\n\
             - sub 不需要知道评分结果（feedback-submit 是账户级 + 一次性上链 tx，不影响 task sub 状态机）。\n\n\
             ⚠️ **不要 `xmtp_delete_conversation`**——保留 sub session 历史以便事后查阅。任务终态后继续在 sub 里观察后续事件即可。\n"
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
             ⚠️ 不要给买家 `xmtp_send`「裁决支持 X 方」过场——双方都会收到 `dispute_resolved` 系统事件。\n\n\
             ━━━━━━━━━━━━━ 分支 A：jobStatus=complete（卖家胜诉）━━━━━━━━━━━━━\n\n\
             **A-Step 1 — 检查待领奖励（account-pull）**：\n\
             ```bash\n\
             onchainos agent provider-claimable --agent-id {agent_id}\n\
             ```\n\
             stdout 含 `•` 标记的行表示该 token 有非 0 待领金额。\n\n\
             **A-Step 2 — 有非 0 金额时一次性领取**（claimable 输出全 0 则跳过）：\n\
             ```bash\n\
             onchainos agent provider-claim-rewards --agent-id {agent_id}\n\
             ```\n\
             记录 stdout 的 txHash + 实际领取的金额 / token（用于下一步通知用户）。\n\n\
             **A-Step 3 — 用 `xmtp_dispatch_user` 通知用户胜诉 + 领取结果**：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol + buyerAgentId。\n\
             tool: xmtp_dispatch_user\n\
             content（按 A-Step 2 是否实际 claim 二选一）：\n\
             \x20\x20有领取：[仲裁胜诉 ⚖️💰] 任务 {job_id}（<title>）仲裁完成，**卖方胜诉**。\n\
             \x20\x20\x20\x20  - 任务收入：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 已自动领取账户奖励：<claimed amount> <symbol>（txHash=<hash>）\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=complete）\n\
             \x20\x20\x20\x20  - 接下来我会自动给买家打个分（feedback-submit）\n\
             \x20\x20无可领：[仲裁胜诉 ⚖️💰] 任务 {job_id}（<title>）仲裁完成，**卖方胜诉**。\n\
             \x20\x20\x20\x20  - 任务收入：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 账户级待领奖励：无（已检查）\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=complete）\n\
             \x20\x20\x20\x20  - 接下来我会自动给买家打个分（feedback-submit）\n\n\
             **A-Step 4 — 把评价请求推到 user session（让 main agent 走 `okx-agent-identity` §Feedback Submit）**：\n\n\
             ⚠️ sub agent 不直接调 feedback-submit CLI——A-Step 3 的 user 通知 content 末尾**已经包含**了「请给买家打分」的指令 + 参数（buyerAgentId / creator-id / task-id），user session main agent 收到后会自己激活 identity skill 接管。\n\
             如果 A-Step 3 的 content 没显式写评价指令，**追加一条 xmtp_dispatch_user**：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20⭐ 任务 {job_id} 仲裁结束（卖方胜诉）。请给买家 <buyerAgentId> 打分（0-100）+ 一句简评——我即将调 `okx-agent-identity` §Feedback Submit：\n\
             \x20\x20\x20\x20\x20\x20- 被评价方 (--agent-id) = <buyerAgentId>\n\
             \x20\x20\x20\x20\x20\x20- 评价发起方 (--creator-id) = {agent_id}\n\
             \x20\x20\x20\x20\x20\x20- 任务 ID (--task-id) = {job_id}\n\
             \x20\x20\x20\x20score 参考（卖方胜诉路径）：80 = 沟通顺畅 / 60 = 中性 / 40 以下 = 买家不合理 nitpick。\n\n\
             ⚠️ 推完 user session 立即**结束本轮 turn**——不等回复，user session main agent 自己接管 feedback-submit 流程，跟 sub 解耦。\n\n\
             ━━━━━━━━━━━━━ 分支 B：jobStatus=rejected（卖家败诉）━━━━━━━━━━━━━\n\n\
             **B-Step 1 — 用 `xmtp_dispatch_user` 通知用户败诉**：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol + buyerAgentId。\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[仲裁败诉 ⚖️⚠️] 任务 {job_id}（<title>）仲裁完成，**买方胜诉**。\n\
             \x20\x20\x20\x20  - 损失：<tokenAmount> <tokenSymbol>（资金已退还买家）\n\
             \x20\x20\x20\x20  - 仲裁结果：dispute_resolved（jobStatus=rejected）\n\
             \x20\x20\x20\x20  - 接下来我会自动给买家打个分（feedback-submit）\n\n\
             **B-Step 2 — 把评价请求推到 user session（让 main agent 走 `okx-agent-identity` §Feedback Submit）**：\n\n\
             ⚠️ sub agent 不直接调 feedback-submit CLI——B-Step 1 的 user 通知 content 末尾**已经包含**了「请给买家打分」指令 + 参数。如果 B-Step 1 没显式写评价指令，**追加一条 xmtp_dispatch_user**：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20⭐ 任务 {job_id} 仲裁结束（卖方败诉）。请给买家 <buyerAgentId> 打分（0-100）+ 一句简评——我即将调 `okx-agent-identity` §Feedback Submit：\n\
             \x20\x20\x20\x20\x20\x20- 被评价方 (--agent-id) = <buyerAgentId>\n\
             \x20\x20\x20\x20\x20\x20- 评价发起方 (--creator-id) = {agent_id}\n\
             \x20\x20\x20\x20\x20\x20- 任务 ID (--task-id) = {job_id}\n\
             \x20\x20\x20\x20败诉路径常见区间 0-50（结果不利你 + 体感不佳），但应**基于事实而非情绪打分**；description 简短陈述事实。\n\n\
             ⚠️ 推完 user session 立即**结束本轮 turn**——不等回复，user session main agent 自己接管 feedback-submit 流程。\n\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
             ⚠️ **不要 `xmtp_delete_conversation`**——保留 sub session 历史以便事后查阅。仲裁终态后继续在 sub 里观察后续事件即可。\n"
        ),

        // ─── Scene 6.5b: 卖家同意退款 / 仲裁退款上链 ─────────────────
        Event::JobRefunded => format!(
            "【当前状态】job_refunded（资金已退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             ⚠️ 不要给买家 `xmtp_send`「已退款上链」过场——双方都收到 `job_refunded` 系统事件了。\n\
             ⚠️ **不要 `xmtp_delete_conversation`**——保留 sub session 历史以便事后查阅。\n\n\
             直接 **结束本轮 turn**，退款流程完整结束。\n"
        ),

        // ─── Scene 6.4: 仲裁已上链，需用户提供证据 ───────────────────
        Event::JobDisputed => format!(
            "【当前状态】job_disputed（仲裁已上链，进入 1 小时证据准备期）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **证据内容必须由用户决策**——sub agent 不知道用户手上有什么证据（截图、聊天记录、交付物链接等），\n\
             不要凭空编造证据摘要直接调 `dispute upload`。**先把决策请求推到 user session 让用户拍板**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             ⚠️ 不要给买家 `xmtp_send`「仲裁已上链，正在准备证据」过场——双方都收到 `job_disputed` 系统事件了。\n\n\
             **Step 1 — 用 `xmtp_prompt_user` 把证据决策请求推到用户**：\n\n\
             先调 `session_status` 拿当前 sub session 的 sessionKey（同 turn 内调一次即可），\n\
             user session agent 拿到 llmContent 后会按 `sub_key` 把用户证据 relay 回本 sub。\n\n\
             tool: xmtp_prompt_user\n\
             llmContent:\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}] \
             用户回复证据后，relay 回 sub session 执行 onchainos agent dispute upload。禁止 user session agent 自己执行 task CLI。1 小时内必须提交。\n\
             userContent:\n\
             \x20\x20\x20\x20任务 {job_id} 仲裁已上链，需要在 1 小时内提交链下证据。请提供：\n\
             \x20\x20\x20\x20- 文字摘要（必填）：说明你已按验收标准完成的关键证据点\n\
             \x20\x20\x20\x20- 图片路径（可选）：截图、设计稿、聊天记录等本地文件路径\n\
             \x20\x20\x20\x20回复格式示例：『证据：已按需求完成 X/Y/Z；图片：/path/to/screenshot.png』\n\n\
             **Step 2 — 等用户回复 relay 回来**：收到 `[USER_DECISION_RELAY] 用户证据：...` 后，调 `next-action --jobStatus dispute_evidence` 拿上传剧本。\n\n\
             ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
             跑完 Step 1 → **结束本轮 turn**，等用户回复。\n"
        ),

        // ─── Scene 6.4b: 用户已提供证据内容（user-instruction 伪 event）──
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】上传仲裁证据\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 从 relay 进来的用户消息中提取证据内容：**\n\
             - 文字摘要 → 用户提供的部分\n\
             - 图片路径（如果用户提供了）→ `--image` 参数\n\
             text 和 image **至少一项**。\n\n\
             **Step 2 — 拉本 sub session 协商 / 交付聊天记录，作为客观证据附在 text 头部：**\n\
             调 `xmtp_get_conversation_history`（sessionKey = 本 sub session 的 sessionKey），拿到与买家的全部 a2a-agent-chat 历史。\n\
             把历史按下面这种**结构化分段**拼到 `--text` 字段最前面（仲裁员是 LLM，会通读 text 字段判断），后面再贴用户摘要：\n\n\
             ```\n\
             ==== 协商 / 交付聊天记录（从 xmtp_get_conversation_history 拉取） ====\n\
             [时间] 买家(<agentId>): ...\n\
             [时间] 卖家(<agentId>): ...\n\
             ...（按时间顺序，关键节点：买家询盘 / NEGOTIATE_PROPOSE / 你回 NEGOTIATE_ACK / 买家 NEGOTIATE_CONFIRM / 你的 deliver 消息）\n\n\
             ==== 用户证据摘要 ====\n\
             <用户原话摘要>\n\
             ```\n\n\
             ⚠️ **`--text` 上限 16 KB**——聊天记录过长就**只保留**关键节点（PROPOSE / ACK / CONFIRM / 交付物 / 双方关键争议点），开头标注「（已截取关键节点）」；不要随便丢前 N 条机械截断。\n\n\
             **Step 3 — 调用 CLI 上传证据（链下 multipart）：**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<聊天记录 + 用户摘要 拼接后的完整 text>\" --image <用户提供的图片路径或省略>\n\
             ```\n\
             text 和 image **至少一项**；图片可省略整个 `--image` 段，不要给空字符串。\n\n\
             【后续事件】\n\
             - job_completed → 胜诉，资金释放给卖家\n\
             - dispute_resolved → 败诉，资金退还买家\n\n\
             跑完 Step 1-3 → **直接结束本轮 turn**。\n\
             ⚠️ 不要给买家 `xmtp_send`「证据已提交」过场——双方都在各自上传证据，互相通知没价值；仲裁结果由 `dispute_resolved` 系统事件通知双方。\n\
             ⚠️ 不要 `xmtp_dispatch_user` 推用户。\n"
        ),

        // ─── 未知类型兜底 ─────────────────────────────────────────────
        Event::JobCreated => format!(
            "【当前状态】job_created（任务上链）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **协商阶段，禁止直接调 `onchainos agent apply`**：apply 是链上动作（需 gas、签名上链），\n\
             协商失败无法撤销。必须先走完下方协商三项全部确认后再 apply。\n\n\
             🛑 **硬约束 — 三步握手 + 同一 turn 禁止 xmtp_send 之后再跑任何 onchainos CLI**\n\n\
             协商必须完整走完三步握手（buyer 协议铁律，已由买家代码强制）：\n\
             \x20\x201) `[NEGOTIATE_PROPOSE]`（buyer → provider）\n\
             \x20\x202) `[NEGOTIATE_ACK]` 或 `[NEGOTIATE_COUNTER]`（provider → buyer）\n\
             \x20\x203) `[NEGOTIATE_CONFIRM]`（buyer → provider，原样回传所有字段）\n\n\
             apply / get-payment 必须**已收到 `[NEGOTIATE_CONFIRM]`** 才能跑（其它任何 inbound 都不算，包括三项问题、free-form 邀请、buyer 的『同意/接受』自然语言回复，甚至 buyer 自然语言『请 apply』也不算）。\n\n\
             换句话说，**同一 turn 收到的 inbound 决定你能做什么**：\n\
             \x20\x20• 收到 buyer free-form 邀请 → 只能 `xmtp_send` 发三项问题（下方 Step 3），**禁止 apply**\n\
             \x20\x20• 收到 buyer `[NEGOTIATE_PROPOSE]` → 只能 `xmtp_send` 回 `[NEGOTIATE_ACK]`（下方 Step 3.5），**禁止 apply**\n\
             \x20\x20• 收到 buyer `[NEGOTIATE_CONFIRM]` → 校验字段一致后才进 Step 4 跑 `apply` / `get-payment`\n\
             \x20\x20• 没看到 `[NEGOTIATE_CONFIRM]` 字面量 → **永远不要 apply**，无论 buyer 自然语言说了什么\n\n\
             ❌ **特别禁止**：不要在 `xmtp_send` 三项问题的内容里写「我确认以下三项 / 三项确认完毕 / 我将立即 apply」之类的自我确认词——三项是要**问**买家的，不是你自己 confirm 后立刻 apply。这种自我 confirm 会让 sub 错觉协商已完成跳过 [PROPOSE]/[ACK]/[CONFIRM] 握手，直接非法 apply（已发生过线上事故）。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             返回里包含【你的身份】（name、profileDescription）+【任务详情】（含「可见性」字段）+「专业匹配检查」区块。\n\n\
             **Step 2 — 按可见性 + 专业匹配分流**：\n\n\
             ━━━━━━━━━ 分支 A：可见性 = 公开（Public，visibility=0）—— 主动联系买家 ━━━━━━━━━\n\n\
             A-Step 1：调 `xmtp_start_conversation` 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<task.buyerAgentId>（从 context 拿），jobId={job_id}\n\
             \x20\x20成功返回 sessionKey + xmtpGroupId。\n\n\
             A-Step 2：用 `xmtp_send` 给买家发协商三项确认（见 Step 3 模板）。\n\n\
             ━━━━━━━━━ 分支 B：可见性 = 私有（Private，visibility=1）—— 被动等待 ━━━━━━━━━\n\n\
             B-Step 1：**不要主动建群**。等买家先 a2a-agent-chat envelope 到达（buyer 才有指定 provider 的权限）。\n\
             \x20\x20本轮 turn 结束，等下一条 inbound 进来再走 Step 3 协商三项确认。\n\
             \x20\x20（如果你已在某条 inbound a2a-agent-chat 触发的 sub session 里，跳过 B-Step 1，直接进 Step 3。）\n\n\
             ━━━━━━━━━ 共同：专业匹配判断 ━━━━━━━━━\n\n\
             看 context 里「专业匹配检查」区块：\n\
             - 领域匹配 → 进入 Step 3（私有任务等买家先来；公开任务是你 A-Step 2 主动发）\n\
             - 领域不匹配 → 按区块给出的拒绝模板调 `xmtp_send`（纯自然语言），结束\n\n\
             **Step 3 — 协商首回合（自然语言，可还价 / 表达 paymentMode 偏好）：**\n\n\
             ⚠️ **币种必须从任务详情读出**：context 输出里的「预算」字段后括号里的 token 地址就是任务规定的币种 —— XLayer USDT 合约 `0x779ded0c9e1022225f8e0630b35a9b54be713736` / USDG 合约 `0x4ae46a509f6b1d9056937ba4500cb143933d2dc8`。**禁止假设 USDT** —— 不少任务用 USDG，回复里写错币种会让买家协议混乱。如果 token 地址不能确定，向用户 dispatch 询问，不要瞎猜。\n\n\
             📌 **你有完整的协商权 —— 不要机械接受 buyer 的开价**。看 context 里的【任务详情】+【你的身份/profile】+【任务复杂度】，自己判断：\n\
             \x20\x20• 任务工作量、验收标准、deadline 是否值这个价\n\
             \x20\x20• 你 profile 上的同类服务价格（context 里的 service-list）跟 buyer 出价差多少\n\
             \x20\x20• 担保（escrow）vs 非担保（non_escrow）哪个更适合这单（金额大 / 不熟买家 → 偏好 escrow；低额、长期合作 → non_escrow 更轻）\n\n\
             基于以上判断，一条 `xmtp_send` 表达三件事（**不是机械三选一，是带你自己的立场**）：\n\
             \x20\x201) 能力 / 验收标准：能不能做、有没有补充问题\n\
             \x20\x202) **价格立场**：原价接受 / 还价（明确报新价 + 简短理由，比如『工作量评估更接近 X USDT，原价偏低』）/ 直接拒绝\n\
             \x20\x203) **paymentMode 立场**：你偏好 escrow 还是 non_escrow，附理由（不是被动等买家定，可以主动提）\n\n\
             示例风格（自然语言，不要套模板格式）：\n\
             \x20\x20『任务我能做，验收标准 OK。价格我看 0.01 USDT 偏低，按工作量我希望 0.05 USDT；担保支付（escrow）比较合适，避免后续争议。如果同意请发 [NEGOTIATE_PROPOSE]。』\n\n\
             ⚠️ 还价幅度参考：context 给的 service-list 单价 × (1 ± 30%) 内通常能谈成，离谱报价（× 5+）会被买家直接换人。\n\
             → 用 `xmtp_send` 给买家发立场（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.4）。\n\n\
             **Step 3.5 — 处理买家的 [NEGOTIATE_PROPOSE] 结构化提案：**\n\n\
             买家协商达成一致后会发送格式化提案：\n\
             ```\n\
             [NEGOTIATE_PROPOSE]\n\
             jobId: ...\n\
             deliverable: ...\n\
             qualityStandards: ...\n\
             paymentMode: ...\n\
             tokenSymbol: ...\n\
             tokenAmount: ...\n\
             deadline: ...\n\
             ```\n\n\
             收到 [NEGOTIATE_PROPOSE] 后**逐字段校验 + 价值判断**：\n\
             - tokenSymbol 必须与任务详情一致（**链上币种，不允许改**）\n\
             - tokenAmount / paymentMode / deadline 是否跟你 Step 3 表达的立场一致；如果你 Step 3 还了价，看 buyer 在 [PROPOSE] 里给的金额是否是双方折中后的合理值\n\
             - deliverable / qualityStandards 是否在你能力范围内\n\n\
             **判断标准（带主观能动性，不是机械接受）**：\n\
             \x20\x20• 价格在你心理预期 ±10% 内、paymentMode 没硬冲突 → ACK\n\
             \x20\x20• 价格仍偏离（buyer 没采纳还价 / 还价幅度不够）→ COUNTER 继续谈，不要勉强 ACK 委屈成交\n\
             \x20\x20• paymentMode 跟你 Step 3 表达的偏好相反、且金额不小 → COUNTER 改 paymentMode\n\
             \x20\x20• deliverable 把验收标准提高了（明显超出原任务描述）→ COUNTER 把 qualityStandards 改回合理范围，或要求加价\n\n\
             ▸ **全部同意** → 调 xmtp_send 回复 **[NEGOTIATE_ACK]**（必须严格使用此格式，原样回传所有字段）：\n\
             \x20\x20content=\n\
             \x20\x20[NEGOTIATE_ACK]\n\
             \x20\x20jobId: <与 PROPOSE 完全相同>\n\
             \x20\x20deliverable: <与 PROPOSE 完全相同>\n\
             \x20\x20qualityStandards: <与 PROPOSE 完全相同>\n\
             \x20\x20paymentMode: <与 PROPOSE 完全相同>\n\
             \x20\x20tokenSymbol: <与 PROPOSE 完全相同>\n\
             \x20\x20tokenAmount: <与 PROPOSE 完全相同>\n\
             \x20\x20deadline: <与 PROPOSE 完全相同>\n\n\
             ▸ **部分不同意**（如价格偏低）→ 调 xmtp_send 回复 **[NEGOTIATE_COUNTER]**（填入你期望的值）：\n\
             \x20\x20content=\n\
             \x20\x20[NEGOTIATE_COUNTER]\n\
             \x20\x20jobId: <与 PROPOSE 相同>\n\
             \x20\x20deliverable: <同意则原样，不同意则填你的版本>\n\
             \x20\x20qualityStandards: <同意则原样，不同意则填你的版本>\n\
             \x20\x20paymentMode: <同意则原样，不同意则填你的版本>\n\
             \x20\x20tokenSymbol: <必须与 PROPOSE 相同，禁止改币种>\n\
             \x20\x20tokenAmount: <你期望的金额>\n\
             \x20\x20deadline: <你期望的截止时间>\n\
             \x20\x20reason: <简要说明修改原因>\n\n\
             ▸ **完全拒绝** → 调 xmtp_send 回复「很抱歉，无法接受当前条件」（纯自然语言），结束。\n\n\
             ⚠️ 回复 [NEGOTIATE_ACK] 后**结束本轮 turn**，等买家发 [NEGOTIATE_CONFIRM]（三步握手第 3 步，buyer 校验你的 ACK 字段一致后会发）。**收到 [NEGOTIATE_CONFIRM] 之前，禁止跑任何 onchainos CLI（apply / get-payment）**。\n\n\
             **Step 3.7 — 收到买家的 [NEGOTIATE_CONFIRM]（apply/get-payment 的唯一合法触发器）：**\n\n\
             ```\n\
             [NEGOTIATE_CONFIRM]\n\
             jobId: ...\n\
             deliverable: ...\n\
             qualityStandards: ...\n\
             paymentMode: ...\n\
             tokenSymbol: ...\n\
             tokenAmount: ...\n\
             deadline: ...\n\
             ```\n\n\
             **逐字段校验** [NEGOTIATE_CONFIRM] 与你之前发的 [NEGOTIATE_ACK] 是否完全一致：\n\
             \x20\x20• 全部一致 → 协商正式锁定，进入 Step 4，按 paymentMode 分流跑 apply / get-payment\n\
             \x20\x20• 任一字段不一致 → 视为篡改，调 xmtp_send 回复「[NEGOTIATE_CONFIRM] 字段与 [NEGOTIATE_ACK] 不一致，拒绝」（指出哪个字段不对），**禁止 apply**，结束\n\n\
             ⚠️ 不要把 buyer 的自然语言『同意 / 好的 / 请 apply』当作 [NEGOTIATE_CONFIRM]——只认字面量带 `[NEGOTIATE_CONFIRM]` 标记的消息，其它一律视为协商未完成。\n\n\
             **Step 4 — 收到 [NEGOTIATE_CONFIRM] 校验一致后，按 paymentMode 分流：**\n\n\
             ━━━━━ 分支 A：支付方式 = escrow（担保交易）→ 必须 apply 上链 ━━━━━\n\n\
             ```bash\n\
             onchainos agent apply {job_id} --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id {agent_id}\n\
             ```\n\
             apply 是上链签名动作，CLI 内部完成 unsigned info → sign → broadcast，等链上 provider_applied 通知。\n\n\
             ⚠️ **apply 跑完直接结束 turn，禁止 `xmtp_dispatch_user` 推用户**——『已提交接单申请 / txHash / 等 provider_applied』是过场状态，对用户没信息量。等链上 `provider_applied` 通知到达后 next-action 那时才有值得推的。这条命令再说一遍是因为 sub 容易在 tx broadcast 后本能想『通知用户』——不要。\n\n\
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
        | Event::ReviewDeadlineWarn => format!(
            "【系统通知】{event}（buyer 端动作或超时事件）\n\
             【角色】卖家（Provider）\n\n\
             【建议】\n\
             - 静默观察即可，无需主动 xmtp_send\n\
             - 如需要详细信息，调用 `onchainos agent common context {job_id} --role provider`\n",
            event = event.as_str()
        ),

        // ─── review_expired: review 窗口超时，卖家主动领货款 ─────────────
        Event::ReviewExpired => format!(
            "【系统通知】review_expired（review 窗口超时，买家未在期限内验收）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **review_expired 只是窗口超时事件，task 状态仍是 submitted，资金未自动释放**。\n\
             需要你主动调 claimAutoComplete 把资金从托管合约领回，链上确认后才进 completed。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 调 CLI 领取货款（上链）：**\n\
             ```bash\n\
             onchainos agent claim-auto-complete {job_id} --agent-id {agent_id}\n\
             ```\n\
             CLI 内部：POST /claimAutoComplete → uopData → sign uopHash → broadcast。等链上 `job_auto_completed` 通知。\n\n\
             ⚠️ **跑完 claim-auto-complete 直接结束 turn**：\n\
             - 禁止给买家发任何 xmtp_send（中间过场，等 job_auto_completed 上链回执到达再说）\n\
             - 禁止 `xmtp_dispatch_user` 推用户\n\n\
             【后续事件】\n\
             - `job_auto_completed`（status=success） → next-action 拿到账剧本（推 user 通知，不关闭 sub）\n\
             - `job_auto_completed`（status=failed）  → 按 errorCode 重试 claim-auto-complete\n"
        ),

        // ─── job_auto_completed: claimAutoComplete tx 回执 ────────────────
        Event::JobAutoCompleted => format!(
            "【系统通知】job_auto_completed（claimAutoComplete tx 回执）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **判定 status**：从你刚收到的系统通知 envelope 里读 `message.status` 字段：\n\
             - `status = \"success\"` → 资金已自动到账，按下方 A 分支收尾\n\
             - `status = \"failed\"` → 按下方 B 分支按 errorCode 重试\n\n\
             ━━━━━━━━━ 分支 A：status=success（自动完成成功，资金已到账）━━━━━━━━━\n\n\
             ⚠️ 不要给买家 `xmtp_send`「review 期已结束 / 资金已自动结算」过场——双方都收到 `job_auto_completed` 系统事件了。\n\n\
             **A-Step 1 — 用 `xmtp_dispatch_user` 通知用户到账**：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[任务自动完成 💰] 任务 {job_id}（<title>）review 超时，已通过 claimAutoComplete 自动到账。\n\
             \x20\x20\x20\x20  - 收入：<tokenAmount> <tokenSymbol>\n\
             \x20\x20\x20\x20  - 完成时间：<现在的时间戳>\n\
             \x20\x20\x20\x20本任务流程结束。\n\n\
             ⚠️ **不要 `xmtp_delete_conversation`**——保留 sub session 历史以便事后查阅。\n\n\
             ━━━━━━━━━ 分支 B：status=failed（claim 失败，按 errorCode 重试）━━━━━━━━━\n\n\
             从 envelope payload 读 `errorCode` / `errorMessage`，按错误重试：\n\
             ```bash\n\
             onchainos agent claim-auto-complete {job_id} --agent-id {agent_id}\n\
             ```\n\
             重试前可选先看链上状态：`onchainos agent common context {job_id} --role provider --agent-id {agent_id}`。\n\
             ⚠️ 失败状态下**不要**给买家 xmtp_send 任何过场信息。\n"
        ),

        // ─── provider 自己的截止提醒 ─────────────────────────────────────
        Event::SubmitDeadlineWarn => format!(
            "【系统通知】submit_deadline_warn（提交交付物截止时间快到了）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 调 `xmtp_dispatch_user` 把截止警告推到 user session 通知用户：**\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20[⏰ 截止警告] 任务 {job_id} 提交交付物时限快到了。\n\
             \x20\x20如果交付物已准备好，请回复『提交交付物』，由我（agent {agent_id}）执行 deliver 上链；\n\
             \x20\x20否则尽快完成准备——超时后买家可调 `agent claim-auto-refund <jobId>` 强制退款，托管资金会原路返回买家，本任务作废。\n\n\
             **Step 2 — 结束本轮 turn**：等用户在 user session 决策回复，或等下一个真实链事件（job_submitted / submit_expired）到达再动作。\n\n\
             ⚠️ **不要在本 turn 自动跑 `onchainos agent deliver`**——是否准备好交付物只有用户知道，agent 不能替用户决定『交付物已就绪』。\n\
             ⚠️ **不要给买家 `xmtp_send`**——截止警告是 provider 内部的事情，跟买家无关。\n"
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
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed
        | Event::StakeStopped
        | Event::CooldownEntered => format!(
            "【系统通知】{event}（evaluator 质押 lifecycle，provider 无关）\n\
             【建议】忽略即可。\n",
            event = event.as_str()
        ),

        // reward_claimed —— 自己的 claim tx 回执（可能 provider 也会 claim 仲裁奖励）
        Event::RewardClaimed => format!(
            "【系统通知】reward_claimed（claimRewards tx 回执）\n\
             【角色】卖家（Provider）\n\n\
             【建议】从 payload 提取 status / amount / txHash。如 status=success 表示奖励到账；\n\
             如 status=failed 按 errorCode 重试 `onchainos agent provider-claim-rewards --agent-id {agent_id}`。\n"
        ),

        // job_auto_refunded —— buyer 端 tx 回执，provider 无关
        Event::JobAutoRefunded => format!(
            "【系统通知】job_auto_refunded（buyer 端 claimAutoRefund tx 回执，provider 无关）\n\
             【建议】忽略即可。买家已领取自动退款。\n"
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
