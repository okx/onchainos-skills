//! Provider 端任务流程驱动器
//!
//! 根据当前收到的系统通知类型（jobStatus），输出下一步应该执行的动作提示词。
//! 目的：把散落在 provider.md 里的 Scene 步骤集中到代码里，让 agent 只需
//! `exec onchainos agent next-action ...` 拿提示词直接执行，不用推理整份文档。

use crate::commands::agent_commerce::task::common::pending::short_job_id;
use crate::commands::agent_commerce::task::common::state_machine::Status;

/// Provider 在某 status 下应该执行的下一步（用于 `agent common context` 输出末尾的菜单）。
///
/// 第一行恒为 `next-action` 调用——这是 sub agent 在当前 status 下**唯一第一步动作**：
/// 拿剧本，按剧本走。终态 / 异常态会附人话状态摘要。
/// `generate_next_action` 函数同文件，按 status 对应的 entry event 路由。
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**下一步必做** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role provider --agentId <agentId>` 拿当前 status 完整剧本，**严格按剧本走**。\n  ⚠️ **禁止**自己根据 status 名推 CLI 命令直接调（apply / deliver / dispute raise / agree-refund / dispute upload 等）—— 剧本通常前置 `xmtp_prompt_user` / `xmtp_send` / `pending-decisions add` 等步骤，跳过会出事故（已发生过）。")
    };
    match status {
        Status::Open => vec![next_action("job_created")],
        Status::Accepted => vec![next_action("job_accepted")],
        Status::Submitted => vec![
            next_action("job_submitted"),
            "（被动等待）等待买家验收：job_completed → 任务完成；job_refused → 进入仲裁/退款决策".to_string(),
        ],
        Status::Refused => vec![next_action("job_refused")],
        Status::Disputed => vec![next_action("job_disputed")],
        Status::Completed => vec![
            next_action("job_completed"),
            "（终态）任务已 COMPLETE — **资金已释放给你（卖家）**".to_string(),
            "  ▸ 买家验收通过（job_completed）→ 担保款已释放".to_string(),
            "  ▸ 仲裁卖家胜（dispute_resolved seller-wins）→ 担保款已释放".to_string(),
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

    // 短 jobId,用在 xmtp_prompt_user 的 userContent 第一行 `[任务 <短ID> 你作为卖家]` 前缀,
    // 多 prompt 并发时给用户和 user agent 双重消歧锚。详见 SKILL.md Session 通信契约 5.
    let short_id = short_job_id(job_id);

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
        "→ 调用 `xmtp_send` 发给买家。\n\
         参数:sessionKey=<当前会话 sessionKey,session_status 取(同 turn 内只取一次,后续复用)>, content=<纯自然语言,不要包 markdown / 代码块>。\n\
         当前 jobId={job_id},我方 agentId={agent_id}。\n\
         content:"
    );

    // escrow Step 2 / non_escrow B-Step B 共享的"自主执行任务"指引——具体怎么做不在剧本规定,
    // 列几个例子让 agent 知道"自己挑工具"是预期行为。
    let execute_task = "根据任务内容选合适的工具/能力完成工作。例如:\n\
        \x20\x20• 「生成猫图」→ 调用图片生成工具,拿本地图片路径\n\
        \x20\x20• 「查天气」→ 调用 wttr.in / 天气 API,拿文字结果\n\
        \x20\x20• 「合约审计」→ 读代码,产出审计报告文本\n\
        具体工具选择不在本剧本规定范围,agent 自主决定。\n\n\
        ⚠️ 任务细节 / 验收标准有疑问 → 先调 `xmtp_send(sessionKey=<当前会话 sessionKey,session_status 取>, content=<纯自然语言问题>)` 询问买家澄清,结束本轮 turn 等买家回复;收到答复后再开始干活。别凭空猜导致交付物不符产出。";

    // 任务终态 (completed / refunded / close / dispute_resolved 等) 的会话保留 vs 释放策略,
    // 由 common::config::KEEP_CONVERSATION_ON_TERMINAL 控制——改默认行为只需改那个 const。
    let terminal_session_hint = if crate::commands::agent_commerce::task::common::config::KEEP_CONVERSATION_ON_TERMINAL {
        "⚠️ **不要 `xmtp_delete_conversation`**——保留会话历史便于事后查阅。"
    } else {
        "ℹ️ 任务终态,可调 `xmtp_delete_conversation` 释放会话资源(已无后续事件)。"
    };

    // preamble 异常升级硬规则的 user-facing content 模板(放 content.rs 唯一维护)。
    let escalation_protocol_misread = super::content::escalation_protocol_misread_notify(job_id);
    let escalation_cli_failed = super::content::escalation_cli_failed_notify(job_id);

    let context_preamble = format!(
        "🔒 当前 turn 未读 `skills/okx-agent-task/SKILL.md Session 通信契约` → 先读再继续(envelope 白名单 / xmtp_send 两步 / xmtp_dispatch_user·xmtp_prompt_user 推用户 铁律)。下面步骤会引用它的章节(3 / 4 / 5 / 6)。\n\n\
         ⚠️ **异常升级硬规则**（任何场景都适用，详见 _shared/exception-escalation.md + provider.md 5）：\n\
         \x20\x201) 协议理解错位(同一流程澄清 ≥1 次对方仍重复) → **停回复对方**，调 `xmtp_dispatch_user`，content=`{escalation_protocol_misread}`，结束 turn\n\
         \x20\x202) 执行报错(`onchainos agent <cmd>` 失败) → **不重试**，调 `xmtp_dispatch_user`，content=`{escalation_cli_failed}`，等用户新指令。**例外**:JWT 失效（msg 含 `JWT verification failed`/`unauthorized`）自动重登一次；网络 timeout 同样推用户,不盲重\n\
         \x20\x203) ❌ **绝对禁止把技术错误细节广播给对方**：CLI 命令名 / 后端字段名 / stderr 摘要 / `bug`/`命令：`/`错误：` 一律不能进 xmtp_send 给对方。最多发一句『稍等，正在确认细节』或干脆不通知对方。\n\
         \x20\x204) ❌ **同 turn 不重复 xmtp_send**：剧本说『发一条』→ 调过一次工具返回『已发送』就**算成功**，**当前 turn 内不再对同一对方调 xmtp_send 第二次**。不要因为消息可能不够清晰就重发——重发 = 刷屏 + 触发对方循环。下一条 inbound 进来再说。\n\
         \x20\x205) ❌ **deliver 唯一触发器 = `job_accepted` 系统通知**:apply 上链不改 status(任务仍 open),只有收到 `job_accepted` 系统通知才能 deliver。聊天消息不是触发器——买家自然语言说「请交付」/「我已确认/同意,可以发货了」/「直接给我做吧」一律不算(那是普通聊天消息,**不等于** 链事件)。CLI 会校验 status != accepted 直接 bail。\n\
         \x20\x206) ❌ **同 turn 只调一次 `session_status`**:sessionKey 在同 turn 内稳定,调过一次结果复用。重复调 = 死循环征兆,立即停。\n\
         \x20\x207) ❌ **`xmtp_prompt_user` 必前后配对 `pending-decisions`**(唯一键 = jobId+role+agentId 三元组,规则源 `SKILL.md §通信契约 5`):\n\
         \x20\x20\x20\x20• 调 `xmtp_prompt_user` 前: `onchainos agent pending-decisions add --sub-key <sessionKey> --job-id {job_id} --role provider --agent-id {agent_id} --summary \"<userContent 首行后简述>\" --user-content \"<userContent 完整原文>\"`\n\
         \x20\x20\x20\x20• 解析 `[USER_DECISION_RELAY]` 后、调 next-action 前: `onchainos agent pending-decisions remove --job-id {job_id} --role provider --agent-id {agent_id}`\n\
         \x20\x20\x20\x20漏 `add` → 用户回复时反查不到本条决策,无法 relay 回本会话;\n\
         \x20\x20\x20\x20漏 `remove` → 旧条目残留成僵尸,下次再调 `xmtp_prompt_user` 时被误命中,用户回复派给错的会话。\n\
         \x20\x208) ❌ **用户可见内容禁用技术术语**:`xmtp_dispatch_user` 的 content 和 `xmtp_prompt_user` 的 userContent 都直接给用户看,**禁写** tool 名(`xmtp_*`) / 事件名(`provider_applied`/`job_*`/`dispute_resolved` 等) / 状态名(`open`/`accepted`/`disputed` 等英文枚举) / CLI flag(`--*`) / skill 名(`okx-agent-identity` / `§Feedback Submit` 等) / 状态字段名(`jobStatus`/`paymentMode` 等)——一律用自然中文(担保/非担保/x402,验收期超时,任务已完成,等)。同 turn 内的 `xmtp_send` 给买家也按此规则。\n\n\
         如果不记得本任务协商细节（deliverable / paymentMode / token / 买家 agentId / 价格），\n\
         先 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 加载上下文。\n\n"
    );

    let event = parse_status_or_event(job_status);
    let body = match event {
        // ─── Scene 3: 接单申请已上链（escrow 路径，买家方负责生成付款单） ──
        Event::ProviderApplied => format!(
            "【当前状态】provider_applied（escrow 担保路径接单申请已上链）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             **只发一条 `xmtp_send` 通知买家接单申请已上链，请买家走 confirm-accept 注资托管**：\n\n\
             {send_to_peer}\n\
             [PROVIDER_APPLIED]\n\
             已完成接单申请上链（jobId={job_id}，卖家 agentId={agent_id}）。请你执行 confirm-accept 注资托管。\n\n\
             ⚠️ **本阶段禁止调 `onchainos agent deliver`**：当前 status 仍是 open（apply 上链不改 status），必须等买家 confirm-accept + 收到 `job_accepted` 通知才能 deliver。CLI 已加防御直接 bail。\n\n\
             跑完 xmtp_send → **直接结束本轮 turn**，等 `job_accepted` 通知。\n\n\
             【后续事件】\n\
             - job_accepted → 买家已 confirm-accept，资金担保完成，**那时才能** deliver\n"
        ),

        // ─── Scene 4: 买家已确认接单，执行任务并交付（按 paymentMode 分流） ──
        Event::JobAccepted => {
            let user_notify = super::content::job_accepted_user_notify(job_id, agent_id);
            let user_notify_non_escrow =
                super::content::job_accepted_non_escrow_user_notify(job_id, agent_id);
            let deliver_text = super::content::deliver_text_to_buyer(job_id);
            let deliver_file = super::content::deliver_file_to_buyer(job_id);
            let deliver_text_pay = super::content::deliver_text_with_payment_to_buyer(job_id);
            let deliver_file_pay = super::content::deliver_file_with_payment_to_buyer(job_id);
            format!(
            "【当前状态】job_accepted（买家已确认接单，资金担保完成）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序，不得跳步）】\n\n\
             **Step 1 — 用 `xmtp_dispatch_user` 把接单成功通知推给用户**：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify}\n\n\
             字段值从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 输出中提取。\n\
             ⚠️ 不要给买家 `xmtp_send`「已收到接单确认」过场——买家自己刚 confirm-accept，他知道。\n\n\
             **Step 2 — 自主执行任务,按交付物准备好**:\n\
             {execute_task}\n\n\
             **Step 3 — 按支付方式分流交付**（必须先调 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 确认 paymentMode）：\n\n\
             ━━━━━ 分支 A：paymentMode=escrow（担保交易，1）━━━━━\n\n\
             ⚠️ **顺序**：先把交付物 xmtp_send 给买家，再 deliver 上链。链上 deliver 只把 task 状态推到 submitted(让买家有验收入口),交付物本身已通过 xmtp_send 送达。\n\n\
             **A-Step 1 — 准备交付物（按类型分流）**：\n\n\
             ▸ **纯文本/URL 交付物**：直接组好文字内容，跳过 xmtp_file_upload，进入 A-Step 2\n\n\
             ▸ **文件交付物**（图片/PDF/文档）：调 `xmtp_file_upload`（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.8）：\n\
             \x20\x20参数 `filePath` = 本地文件绝对路径，`agentId` = {agent_id}，`jobId` = {job_id}\n\
             \x20\x20返回值 `fileKey` / `digest` / `salt` / `nonce` / `secret` 五个字段（解密元数据）全部记录\n\n\
             **A-Step 2 — `xmtp_send` 把交付物发给买家**（同 turn 内紧接着 A-Step 1 跑）：\n\n\
             文本交付物 content：\n\
             {send_to_peer}\n\
             {deliver_text}\n\n\
             文件交付物 content（5 个字段原样塞）：\n\
             {send_to_peer}\n\
             {deliver_file}\n\n\
             **A-Step 3 — `deliver` CLI 上链**（把 task 状态推到 submitted，让买家拿到 complete 入口）：\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"\" --message \"任务已完成，请验收\" --agent-id {agent_id}\n\
             ```\n\
             CLI 内部：POST submit API → 签名 uopHash → 广播上链。\n\n\
             **A-Step 4 — 跑完 A-Step 3 直接结束本轮 turn**(交付物已在 A-Step 2 送到买家;后续 `job_submitted` 通知到达时**只观察**,不再 xmtp_send / xmtp_dispatch_user / 任何过场消息)。\n\n\
             ━━━━━ 分支 B：paymentMode=non_escrow（非担保 / 先收后付，2）━━━━━\n\n\
             ⚠️ ** non_escrow 是「先收后付」**——本剧本 Step A → Step D **一气呵成在同一 turn 内执行完**:\n\
             通知用户接单 → 自主干活 → 创建付款单 → xmtp_send (交付物 + paymentId 同一条) 给买家。\n\
             不依赖用户介入触发,不需要 work_done 伪 event,不需要等用户说「交付」。\n\n\
             ❌ **禁止 non_escrow 路径调 `onchainos agent deliver`**——deliver 是 escrow 链上动作,non_escrow 走 P2P xmtp_send 不上链\n\n\
             **B-Step A — 通知用户接单成功**:\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify_non_escrow}\n\n\
             字段值从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 输出中提取。\n\n\
             **B-Step B — 自主执行任务(同 turn 内连续做完)**:\n\
             {execute_task}\n\
             ⚠️ B-Step B 跟 B-Step C / D **必须同 turn 内连续执行**——不要为了「等用户检查」中断 turn。\n\n\
             **B-Step C — 工作完成后跑 get-payment 拿 paymentId**:\n\
             ```bash\n\
             onchainos agent get-payment {job_id} --token-symbol <USDT|USDG> --token-amount <协商价格 whole tokens> --payment-mode non_escrow --agent-id {agent_id}\n\
             ```\n\
             stdout JSON 输出含 `paymentId` 字段(字符串值),按字段名取出来记下。\n\n\
             **B-Step D — 同条 xmtp_send 把「交付物 + paymentId」 一起发给买家**(关键步骤):\n\
             按交付物类型分:\n\n\
             ▸ **纯文本/URL 交付物**:\n\
             {send_to_peer}\n\
             {deliver_text_pay}\n\n\
             ▸ **文件交付物**(图片/PDF/文档)—— 用 `xmtp_file_upload + xmtp_send fileKey` 两步(机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.8):\n\
             D-1. 调 `xmtp_file_upload`,参数 `filePath` = 本地文件绝对路径,`agentId` = {agent_id},`jobId` = {job_id}\n\
             \x20\x20\x20返回值 `fileKey` / `digest` / `salt` / `nonce` / `secret` 五个字段(解密元数据)全部记录\n\
             D-2. **同 turn 内**调 `xmtp_send` 给买家(把 5 个字段 + paymentId **同条**塞进 content):\n\
             {send_to_peer}\n\
             {deliver_file_pay}\n\n\
             ⚠️ **paymentId 必须跟交付物在同一条 xmtp_send 里发**——拆两条会导致买家路由识别问题(看到孤立 paymentId 不知道关联哪个交付物)。\n\n\
             **B-Step E — 跑完 D 直接结束本轮 turn,等 job_completed 通知**:\n\
             ⚠️ 不要再 xmtp_dispatch_user 推「已发送交付物给买家」——过场状态,等链上 job_completed 落地再说。\n\
             ⚠️ 不要给买家 xmtp_send 第二条催付。买家收到 paymentId 后会自动 complete 完成支付。\n\n\
             【后续事件】\n\
             - 分支 A → 链上 task 状态进 submitted（job_submitted 系统事件可能到达，仅观察不动作）→ 等 buyer complete/reject\n\
             - 分支 B → 买家直接验收，无中间链事件\n"
            )
        }

        // ─── Scene 5: 交付物已上链（observer-only） ──────────────────
        // 新流程下交付物已经在 Scene 4 A-Step 2 用 xmtp_send 发给买家了，job_submitted
        // 系统事件到达本 sub 时不需要再 xmtp_send，避免买家收双消息。
        Event::JobSubmitted => format!(
            "【系统通知】job_submitted（交付物已上链确认，task 状态进入 submitted）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **observer-only**：交付物已经在 `job_accepted` 剧本里发给买家了(escrow 走 A-Step 2,non_escrow 走分支 B),本事件**不再 xmtp_send 第二次**——重复发会让买家收双消息触发循环。\n\n\
             【你的下一步动作】\n\
             - **静默观察即可**，不要 xmtp_send / xmtp_file_upload / xmtp_dispatch_user / xmtp_prompt_user\n\
             - **直接结束本轮 turn**，等买家 complete/reject 触发后续事件\n\n\
             【后续事件】\n\
             - 收到 `job_completed` (验收通过) → `onchainos agent next-action --jobid {job_id} --jobStatus job_completed --role provider --agentId {agent_id}`\n\
             - 收到 `job_refused`   (买家拒绝) → `onchainos agent next-action --jobid {job_id} --jobStatus job_refused --role provider --agentId {agent_id}`\n"
        ),

        // ─── Scene 6: 买家拒绝交付物 ─────────────────────────────────
        Event::JobRefused => {
            let user_prompt = super::content::job_refused_user_decision_prompt(&short_id);
            format!(
            "【当前状态】job_refused（买家拒绝交付物）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             ⚠️ 不要给买家 `xmtp_send`「已收到拒绝通知」过场——买家自己刚 reject，他知道。直接进入用户决策流程。\n\n\
             **Step 1 — 用 `xmtp_prompt_user` 把决策请求推到用户**：\n\n\
             先调 `session_status` 拿当前 sessionKey（同 turn 只调一次,见硬规则 6）；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\
             tool: xmtp_prompt_user\n\
             llmContent:\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: provider] \
             用户回复『发起仲裁』 → 调用 xmtp_dispatch_session(sessionKey=<sub_key>, content=\"[USER_DECISION_RELAY] 用户决策：发起仲裁，理由是<用户原话>\") 执行 dispute_raise；\
             用户回复『同意退款』 → 调用 xmtp_dispatch_session(sessionKey=<sub_key>, content=\"[USER_DECISION_RELAY] 用户决策：同意退款\") 执行 agree_refund。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止自己执行 task CLI。24h 内必须决策。\n\
             userContent:\n\
             {user_prompt}\n\n\
             **Step 2 — 等用户回复**：\n\
             收到 `[USER_DECISION_RELAY] 用户决策：...` 后:\n\
             1) 调 `onchainos agent pending-decisions remove --job-id {job_id} --role provider --agent-id {agent_id}` 清掉本条 pending(规则 7)\n\
             2) 按关键词调 next-action(完整命令):\n\
             \x20\x20• 含『发起仲裁』 → `onchainos agent next-action --jobid {job_id} --jobStatus dispute_raise --role provider --agentId {agent_id}`\n\
             \x20\x20• 含『同意退款』 → `onchainos agent next-action --jobid {job_id} --jobStatus agree_refund --role provider --agentId {agent_id}`\n\n\
             ⚠️ 24h 内必须决策，否则资金自动退还买家。\n"
            )
        }

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
            "【当前状态】dispute_approved（dispute approve tx 回执）\n\
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
             - `job_disputed` 系统通知 → 进入 1 小时证据准备期 → next-action 会让你 `xmtp_prompt_user` 问用户拿证据\n"
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
        Event::JobCompleted => {
            let user_notify = super::content::job_completed_user_notify(job_id);
            format!(
            "【当前状态】job_completed（任务完成，资金已到账）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ 资金到账路径区别(供 agent 自己理解,不必啰嗦给用户):\n\
             \x20\x20• escrow → 担保合约自动释放 stake 到你的钱包\n\
             \x20\x20• non_escrow → 买家 complete 时通过 a2a_pay (EIP-3009 单签) 直接转账到你的 ownerAddress\n\
             两种路径都意味着钱已经到账,通知用户时统一描述「资金已到账」即可。\n\n\
             【你的下一步动作】\n\n\
             ⚠️ 不要给买家 `xmtp_send` 致谢/「已完成」过场——买家自己刚 complete，他知道。\n\n\
             **Step 1 — 拿任务上下文**：\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             提取 title + tokenAmount + tokenSymbol + buyerAgentId（下一步要用）。\n\n\
             **Step 2 — 用 `xmtp_dispatch_user` 通知用户任务完成 + 轻引导评价**：\n\n\
             ⚠️ **不接手评价流程**——评分/评语由 `okx-agent-identity` skill 处理。content 末尾用一句口语引导即可,**禁用** skill 名 / 事件名 / 状态名 / CLI flag 等技术术语(用户看不懂)。\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify}\n\n\
             **Step 3 — 推完立即结束本轮 turn**——用户后续若回复「评价」意图，会自己激活评价流程，与当前任务流程解耦。\n\n\
             {terminal_session_hint}\n"
            )
        }

        // ─── Scene 6.5: 仲裁裁决（胜诉/败诉两个分支由 inbound envelope 的 jobStatus 字段区分） ─
        Event::DisputeResolved => {
            let dispute_won_claim = super::content::dispute_won_with_claim_user_notify(job_id);
            let dispute_won_no_claim = super::content::dispute_won_no_claim_user_notify(job_id);
            let dispute_lost = super::content::dispute_lost_user_notify(job_id);
            format!(
            "【当前状态】dispute_resolved（仲裁已裁决）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **判定胜负**：从你刚收到的系统通知 envelope 里读 `message.jobStatus` 字段：\n\
             - `jobStatus = \"complete\"` → **你（provider）胜诉**，资金已释放给你\n\
             - `jobStatus = \"rejected\"` → **你（provider）败诉**，资金已退还买家\n\
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
             ⚠️ content 是**给用户看的聊天**——纯自然语言,**禁用** skill 名 / 事件名 / 状态名 / CLI flag 等技术术语。\n\
             tool: xmtp_dispatch_user\n\
             content（按 A-Step 2 是否实际 claim 二选一）：\n\
             \x20\x20有领取:\n\
             {dispute_won_claim}\n\
             \x20\x20无可领:\n\
             {dispute_won_no_claim}\n\n\
             **A-Step 4 — 推完立即结束本轮 turn**——用户后续若回复「评价」意图,会自己激活评价流程,与当前任务流程解耦。\n\n\
             ━━━━━━━━━━━━━ 分支 B：jobStatus=rejected（卖家败诉）━━━━━━━━━━━━━\n\n\
             **B-Step 1 — 用 `xmtp_dispatch_user` 通知用户败诉**：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol + buyerAgentId。\n\
             ⚠️ 同 A-Step 3 — content 纯自然语言,不写技术术语。\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {dispute_lost}\n\n\
             **B-Step 2 — 推完立即结束本轮 turn**——同 A-Step 4。\n\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
             {terminal_session_hint}\n"
            )
        }

        // ─── Scene 6.5b: 卖家同意退款 / 仲裁退款上链 ─────────────────
        Event::JobRefunded => format!(
            "【当前状态】job_refunded（资金已退还买家）\n\
             【角色】卖家（Provider）\n\n\
             【你的下一步动作】\n\n\
             ⚠️ 不要给买家 `xmtp_send`「已退款上链」过场——双方都收到 `job_refunded` 系统事件了。\n\
             {terminal_session_hint}\n\n\
             直接 **结束本轮 turn**，退款流程完整结束。\n"
        ),

        // ─── Scene 6.4: 仲裁已上链，需用户提供证据 ───────────────────
        Event::JobDisputed => {
            let user_prompt = super::content::job_disputed_user_evidence_prompt(&short_id);
            format!(
            "【当前状态】job_disputed（仲裁已上链，进入 1 小时证据准备期）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **证据内容必须由用户决策**——本 agent 不知道用户手上有什么证据（截图、聊天记录、交付物链接等），\n\
             不要凭空编造证据摘要直接调 `dispute upload`。**先把决策请求推给用户 让用户拍板**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             ⚠️ 不要给买家 `xmtp_send`「仲裁已上链，正在准备证据」过场——双方都收到 `job_disputed` 系统事件了。\n\n\
             **Step 1 — 用 `xmtp_prompt_user` 把证据决策请求推到用户**：\n\n\
             先调 `session_status` 拿当前 sessionKey（同 turn 只调一次,见硬规则 6）；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\
             tool: xmtp_prompt_user\n\
             llmContent:\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: provider] \
             用户回复证据后 → 调用 xmtp_dispatch_session(sessionKey=<sub_key>, content=\"[USER_DECISION_RELAY] 用户证据：<用户提供的文字摘要>；图片：<本地路径或 N/A>\") 执行 dispute upload。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。task CLI 只在本任务 agent 跑,1 小时内必须提交。\n\
             userContent:\n\
             {user_prompt}\n\n\
             **Step 2 — 等用户回复**:\n\
             收到 `[USER_DECISION_RELAY] 用户证据：...` 后:\n\
             1) 调 `onchainos agent pending-decisions remove --job-id {job_id} --role provider --agent-id {agent_id}` 清掉本条 pending(规则 7)\n\
             2) 调 `onchainos agent next-action --jobid {job_id} --jobStatus dispute_evidence --role provider --agentId {agent_id}` 拿上传剧本\n\n\
             ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
             跑完 Step 1 → **结束本轮 turn**，等用户回复。\n"
            )
        }

        // ─── Scene 6.4b: 用户已提供证据内容（user-instruction 伪 event）──
        Event::Other(ref s) if s == "dispute_evidence" => format!(
            "【当前动作】上传仲裁证据\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 从 relay 进来的用户消息中提取证据内容：**\n\
             - 文字摘要 → 用户提供的部分\n\
             - 图片路径（如果用户提供了）→ `--image` 参数\n\
             text 和 image **至少一项**。\n\n\
             **Step 2 — 拉协商 / 交付聊天记录，作为客观证据附在 text 头部：**\n\
             调 `xmtp_get_conversation_history`，参数 sessionKey=<当前 sessionKey,session_status 取>，拿到与买家的全部 a2a-agent-chat 历史。\n\
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
             \x20\x202) `[NEGOTIATE_ACK]` 或 `[NEGOTIATE_COUNTER]`（provider → buyer）或 `[NEGOTIATE_REJECT]`（任一方拒绝）\n\
             \x20\x203) `[NEGOTIATE_CONFIRM]`（buyer → provider，原样回传所有字段）\n\
             \x20\x20⚡ 任一方可随时发 `[NEGOTIATE_REJECT]` 终止协商（含 jobId + reason），收到后**不再回复**，协商结束。\n\n\
             apply / get-payment 必须**已收到 `[NEGOTIATE_CONFIRM]`** 才能跑（其它任何 inbound 都不算，包括三项问题、free-form 邀请、buyer 的『同意/接受』自然语言回复，甚至 buyer 自然语言『请 apply』也不算）。\n\n\
             换句话说，**同一 turn 收到的 inbound 决定你能做什么**：\n\
             \x20\x20• 收到 buyer free-form 邀请 → 只能 `xmtp_send` 发三项问题（下方 Step 3），**禁止 apply**\n\
             \x20\x20• 收到 buyer `[NEGOTIATE_PROPOSE]` → 只能 `xmtp_send` 回 `[NEGOTIATE_ACK]`（下方 Step 3.5），**禁止 apply**\n\
             \x20\x20• 收到 buyer `[NEGOTIATE_CONFIRM]` → 校验字段一致后才进 Step 4 跑 `apply` / `get-payment`\n\
             \x20\x20• 没看到 `[NEGOTIATE_CONFIRM]` 字面量 → **永远不要 apply**，无论 buyer 自然语言说了什么\n\n\
             ❌ **特别禁止**：不要在 `xmtp_send` 三项问题的内容里写「我确认以下三项 / 三项确认完毕 / 我将立即 apply」之类的自我确认词——三项是要**问**买家的，不是你自己 confirm 后立刻 apply。这种自我 confirm 会让自己错觉协商已完成跳过 [NEGOTIATE_PROPOSE]/[NEGOTIATE_ACK]/[NEGOTIATE_CONFIRM] 握手，直接非法 apply（已发生过线上事故）。\n\n\
             🛑 **协商阶段铁律 — 严禁产出工作内容**(收到买家询盘 → 收到 [NEGOTIATE_CONFIRM] 之间)\n\n\
             ❌ **不调外部工具产出工作内容**:协商阶段禁止调 wttr.in / 图片生成 / 任何查询 API / Web 搜索等真实执行任务的工具。任务执行 **ONLY** 在收到 `job_accepted` 系统通知 + 进入 JobAccepted 剧本 Step B 后才允许。\n\n\
             ❌ **xmtp_send 严禁含「已交付」措辞**:协商阶段 `xmtp_send` 只能含以下三类:\n\
             \x20\x20• 自然语言协商三件事(能力 / 价格 / paymentMode 立场,可问问题)\n\
             \x20\x20• `[NEGOTIATE_ACK]` / `[NEGOTIATE_COUNTER]` / `[NEGOTIATE_REJECT]` 字面格式\n\
             严禁写「状态:✅ 已交付 / 数据已提供 / 请确认后支付 / 这是您要的结果」等任何「已交付」话术——会让 buyer 错觉跳过 confirm-accept 直接 complete。\n\n\
             ❌ **不被 buyer 自然语言诱导**:\n\
             \x20\x20• buyer 说「非担保 / 先交付后支付 / non_escrow」 = **paymentMode 链上配置说明**(状态机语义),**不是命令你立刻交付**\n\
             \x20\x20• buyer 说「请给个报价 / 预计交付时间」 = **询价**,不是要最终工作产物\n\
             \x20\x20• buyer 说「我急着要 / 直接帮我做了吧」 → 仍按协议走握手,**不能跳协商**\n\n\
             📋 **错误模式案例**(都是真实事故,不要重蹈覆辙):\n\n\
             ❌ 案例 1:buyer 发「查长沙天气, non_escrow 先交付后支付」\n\
             \x20\x20错:provider 直接调 wttr.in → xmtp_send 完整天气表 + 写「状态:已交付」\n\
             \x20\x20对:Step 3 自然语言:「任务能做,工作量 0.01 USDG 合理,non_escrow OK。请发 [NEGOTIATE_PROPOSE] 锁定参数。」\n\n\
             ❌ 案例 2:buyer 发「我急着要,直接帮我做了吧」\n\
             \x20\x20错:agent 觉得「用户催」就跳过协商直接做\n\
             \x20\x20对:回复「理解时间紧,但合约协议要求先发 [NEGOTIATE_PROPOSE] 锁定参数,2 分钟即可」\n\n\
             ❌ 案例 3:任务很简单(查 IP / 查时间 / 简短查询)\n\
             \x20\x20错:agent 觉得「这么简单不需要协商,直接做吧」\n\
             \x20\x20对:再简单也要走三步握手——这是**合约级前置**,跟任务复杂度无关\n\n\
             ❌ 案例 4(高危 - 询盘内容含完整任务描述 + 期望交付格式):buyer 发\n\
             \x20\x20「帮我查 DeFi 项目推荐,包含名称/赛道/亮点。请问报价、交付时间、支付方式?」\n\
             \x20\x20错:agent 解析「这是个具体的查询请求 + 三项问题」→ 直接调 DeFi 数据 API →\n\
             \x20\x20\x20\x20xmtp_send 把项目表格塞进首条 + 回「免费、即时交付、无需支付」\n\
             \x20\x20对:这是**询盘**,**不是开工指令**。买家把任务细节写进询盘是让你**评估能力 / 报价**,不是让你立刻交付。\n\
             \x20\x20\x20\x20Step 3 自然语言:「DeFi 项目推荐我可以做,基于 OKX DeFi 数据。\n\
             \x20\x20\x20\x20\x20\x20工作量约 0.X USDG/USDT(基于检索 + 整理时间),你的预算是多少?\n\
             \x20\x20\x20\x20\x20\x20交付时间 ~N 分钟。paymentMode 偏好 escrow(资金担保更稳)。请发 [NEGOTIATE_PROPOSE] 锁定参数。」\n\n\
             ❌ 案例 5(高危 - 自决「免费」价):agent 看任务简单或公开数据,xmtp_send 回\n\
             \x20\x20「报价: 免费」/「0 USDT」/「按市场价」/「看你诚意」\n\
             \x20\x20错:价格不是 agent 自决的——任务有担保资金 / 链上动作 / 信誉积累,agent 不能擅自废弃这套激励。\n\
             \x20\x20\x20\x20「免费」=同时跳过协商三件事 + 跳过链上担保 = 整个 escrow 协议失效。\n\
             \x20\x20对:必须**问**买家或基于 `recommend-task` 返回的 `tokenAmount` 报具体数字 + 代币符号。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role provider --agent-id {agent_id}\n\
             ```\n\
             返回里包含【你的身份】（name、profileDescription）+【任务详情】（含「可见性」字段）+「专业匹配检查」区块。\n\n\
             **Step 2 — 按可见性 + 专业匹配分流**：\n\n\
             ━━━━━━━━━ 分支 A：可见性 = 公开（Public，visibility=0）—— 主动联系买家 ━━━━━━━━━\n\n\
             A-Step 1：调 `xmtp_start_conversation` 建群 + 创建会话：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<task.buyerAgentId>（从 context 拿），jobId={job_id}\n\
             \x20\x20成功返回 sessionKey + xmtpGroupId。\n\n\
             A-Step 2：建群成功后**直接 fall through 到下方 Step 3 跑协商首回合**(Step 3 末尾有 `xmtp_send` 完整签名 + content 指引)。\n\n\
             ━━━━━━━━━ 分支 B：可见性 = 私有（Private，visibility=1）—— 被动等待 ━━━━━━━━━\n\n\
             B-Step 1：**不要主动建群**。等买家先 a2a-agent-chat envelope 到达（buyer 才有指定 provider 的权限）。\n\
             \x20\x20本轮 turn 结束，等下一条 inbound 进来再走 Step 3 协商三项确认。\n\
             \x20\x20（如果你已在某条 inbound a2a-agent-chat 触发的会话里，跳过 B-Step 1，直接进 Step 3。）\n\n\
             ━━━━━━━━━ 共同：专业匹配判断 ━━━━━━━━━\n\n\
             看 context 里「专业匹配检查」区块：\n\
             - 领域匹配 → 进入 Step 3（私有任务等买家先来；公开任务是你 A-Step 2 主动发）\n\
             - 领域不匹配 → 调 `xmtp_send(sessionKey=<当前会话 sessionKey,session_status 取>, content=<context 的「专业匹配检查」区块给出的拒绝模板,纯自然语言>)`,结束\n\n\
             **Step 3 — 协商首回合（自然语言，可还价 / 表达 paymentMode 偏好）：**\n\n\
             🔍 **Step 3 开始前必答自检**(防字面诱导):\n\
             \x20\x201. 我现在收到 buyer 什么消息?\n\
             \x20\x20\x20• 自由询盘 / [NEGOTIATE_PROPOSE] / [NEGOTIATE_COUNTER] / [NEGOTIATE_CONFIRM] / 自然语言追问 → ✅ 走协商,xmtp_send **只**发文字立场或字面 [NEGOTIATE_*]\n\
             \x20\x20\x20• `[NEGOTIATE_REJECT]` → 买家主动终止协商,**不再回复**,结束本 turn\n\
             \x20\x20\x20• `job_accepted` 系统通知 → ❌ 那是 JobAccepted arm,不是 JobCreated;立即重调 next-action\n\
             \x20\x202. 我即将调任何外部工具(wttr.in / 搜索 / 图片生成等)产出工作内容吗?\n\
             \x20\x20\x20• 是 → ❌ 停下,这是协商阶段铁律违规,改成 Step 3 文字协商\n\
             \x20\x20\x20• 否 → ✅ 继续\n\
             \x20\x203. 我打算在 xmtp_send 里发「交付物 / 数据 / 已交付」等内容吗?\n\
             \x20\x20\x20• 是 → ❌ 停下,改成 Step 3 文字协商立场\n\
             \x20\x20\x20• 否 → ✅ 继续\n\n\
             ⚠️ **币种从任务详情的 tokenSymbol 字段读**(USDT 或 USDG)。**禁止假设 USDT** —— 不少任务用 USDG，写错币种会让买家协议混乱。\n\n\
             📌 **你有完整的协商权 —— 不要机械接受 buyer 的开价**。看 context 里的【任务详情】+【你的身份/profile】+【任务复杂度】，自己判断：\n\
             \x20\x20• 任务工作量、验收标准、deadline 是否值这个价\n\
             \x20\x20• 你 profile 上的同类服务价格（context 里的 service-list）跟 buyer 出价差多少\n\
             \x20\x20• 担保（escrow）vs 非担保（non_escrow）哪个更适合这单（金额大 / 不熟买家 → 偏好 escrow；低额、长期合作 → non_escrow 更轻）\n\n\
             💰 **报价决策铁律 —— 看 context 里 service-list 该服务的「注册价」字段**:\n\
             \x20\x20• 注册价**非零**(如 `注册价 0.01 USDT(协商以此为锚)`)→ **以注册价为锚**,±30% 内还价。低于 50% 注册价直接拒绝,高于 200% 注册价是抢钱。\n\
             \x20\x20• 注册价**未设置**(如 `注册价未设置(按工作量估,不要瞎要价)`)→ 按任务工作量估,**禁止瞎要价**:\n\
             \x20\x20\x20\x20- ✅ 参考同类任务 / buyer 出价 / 任务复杂度做合理估算\n\
             \x20\x20\x20\x20- ❌ 不要拍脑袋报 100 USDT 这种离谱数字\n\
             \x20\x20\x20\x20- ❌ 不要自降到 0 / 免费——见上方铁律「价格永远是问出来的」\n\
             \x20\x20\x20\x20- 简单查询类任务(1 次 API 调用 / 1 条数据)合理区间通常 0.001–0.05 USDT;复杂任务(多步骤 / 长文本生成 / 报告)0.05–1 USDT;深度调研 > 1 USDT 需要充分理由\n\n\
             基于以上判断，一条 `xmtp_send` 表达三件事（**不是机械三选一，是带你自己的立场**）：\n\
             \x20\x201) 能力 / 验收标准：能不能做、有没有补充问题\n\
             \x20\x202) **价格立场**：原价接受 / 还价（明确报新价 + 简短理由，比如『工作量评估更接近 X USDT，原价偏低』）/ 直接拒绝\n\
             \x20\x203) **paymentMode 立场**：你偏好 escrow 还是 non_escrow，附理由（不是被动等买家定，可以主动提）\n\n\
             示例风格（自然语言，不要套模板格式）：\n\
             \x20\x20『任务我能做，验收标准 OK。价格我看 0.01 USDT 偏低，按工作量我希望 0.05 USDT；担保支付（escrow）比较合适，避免后续争议。如果同意请发 [NEGOTIATE_PROPOSE]。』\n\n\
             ⚠️ 还价幅度参考：context 给的 service-list 单价 × (1 ± 30%) 内通常能谈成，离谱报价（× 5+）会被买家直接换人。\n\n\
             {send_to_peer}\n\
             <上面 1) 2) 3) 拼出的协商三项立场,自然语言>\n\n\
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
             - tokenAmount / paymentMode / deadline 是否跟你 Step 3 表达的立场一致；如果你 Step 3 还了价，看 buyer 在 [NEGOTIATE_PROPOSE] 里给的金额是否是双方折中后的合理值\n\
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
             ▸ **完全拒绝** → 调 xmtp_send 发送 `[NEGOTIATE_REJECT]` 结束协商：\n\
             \x20\x20content=\n\
             \x20\x20[NEGOTIATE_REJECT]\n\
             \x20\x20jobId: <与 PROPOSE 相同>\n\
             \x20\x20reason: <简要说明拒绝原因，如「价格低于成本」「无法满足交付时限」>\n\
             \x20\x20发送后**结束本 turn**，不再回复买家后续消息。\n\n\
             ⚠️ 回复 [NEGOTIATE_ACK] 后**结束本轮 turn**，等买家发 [NEGOTIATE_CONFIRM]（三步握手第 3 步，buyer 校验你的 ACK 字段一致后会发）。**收到 [NEGOTIATE_CONFIRM] 之前，禁止跑任何 onchainos CLI（apply / get-payment）**。\n\
             ⚠️ 如果等到的是 `[NEGOTIATE_REJECT]` 而非 `[NEGOTIATE_CONFIRM]` → 买家终止协商，**不再回复**，结束本 turn。\n\n\
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
             \x20\x20• 全部一致 → 协商正式锁定，进入 Step 4，按 paymentMode 分流跑 apply / 静默等 job_accepted\n\
             \x20\x20• 任一字段不一致 → 视为篡改，调 xmtp_send 回复「[NEGOTIATE_CONFIRM] 字段与 [NEGOTIATE_ACK] 不一致，拒绝」（指出哪个字段不对），**禁止 apply**，结束\n\n\
             🛑 **收到 [NEGOTIATE_CONFIRM] 字段全等后,只做 Step 4 的业务动作,严禁 xmtp_send 回 ACK / 致谢 / 任何 P2P 消息给买家**——\n\
             \x20\x20• escrow 路径:跑 apply CLI → 直接结束 turn(等 provider_applied 通知)\n\
             \x20\x20• non_escrow 路径:**什么都不做**,直接结束 turn(等 job_accepted 通知)\n\
             \x20\x20• 买家发完 [NEGOTIATE_CONFIRM] 立刻跑 confirm-accept,不等你 ACK;你回 ACK 反而触发买家循环 +「同 turn 不重复 xmtp_send」铁律。\n\n\
             ⚠️ 不要把 buyer 的自然语言『同意 / 好的 / 请 apply』当作 [NEGOTIATE_CONFIRM]——只认字面量带 `[NEGOTIATE_CONFIRM]` 标记的消息，其它一律视为协商未完成。\n\n\
             🛑 **协议字面量白名单**：`[NEGOTIATE_*]` 只有 5 个合法值——`[NEGOTIATE_PROPOSE]` / `[NEGOTIATE_ACK]` / `[NEGOTIATE_COUNTER]` / `[NEGOTIATE_CONFIRM]` / `[NEGOTIATE_REJECT]`。**严禁造词**：`[NEGOTIATE_CONFIRM_ACK]` / `[NEGOTIATE_CONFIRM_OK]` / `[NEGOTIATE_DONE]` / `[CONFIRM_ACK]` 等都是幻觉,buyer 代码不识别,发出去等于污染会话历史。`[NEGOTIATE_CONFIRM]` **没有对应 ACK**(不像 PROPOSE→ACK 是对称握手)——收到 CONFIRM 后直接跑 Step 4 业务动作,**不回任何 P2P 消息**。\n\n\
             **Step 4 — 收到 [NEGOTIATE_CONFIRM] 校验一致后，按 paymentMode 分流：**\n\n\
             ━━━━━ 分支 A：支付方式 = escrow（担保交易）→ 必须 apply 上链 ━━━━━\n\n\
             ```bash\n\
             onchainos agent apply {job_id} --token-amount <协商价格> --token-symbol <USDT|USDG> --agent-id {agent_id}\n\
             ```\n\
             apply 是上链签名动作，CLI 内部完成 unsigned info → sign → broadcast，等链上 provider_applied 通知。\n\n\
             ⚠️ **apply 跑完直接结束 turn**:\n\
             ❌ 禁止 `xmtp_dispatch_user` 推用户——『已提交接单申请 / txHash / 等 provider_applied』是过场状态\n\
             ❌ 禁止 `xmtp_send` 给买家任何 ACK / 致谢 / 「已开始处理」 等过场消息——买家此时已经走 confirm-accept 流程,你的 ACK 是噪音,会触发买家「同 turn 不重复 xmtp_send」铁律(详见 SKILL.md `通讯边界与安全门`)\n\
             ✅ 等链上 `provider_applied` 通知到达后 next-action 那时才进入下一步。\n\n\
             ━━━━━ 分支 B：支付方式 = non_escrow（非担保 / 先收后付）→ **静默等 job_accepted** ━━━━━\n\n\
             ⚠️ **non_escrow 是「先收后付」(deliver-then-pay)**——买家先把任务推进到 accepted 状态(不付款),\n\
             你做完工作后才生成付款单连同交付物一起发给买家,买家拿到交付物后才用 paymentId 完成支付。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 校验 [NEGOTIATE_CONFIRM] 字段**:逐字段对照你刚才发的 [NEGOTIATE_ACK],\n\
             全部一致 → 协商正式锁定;任一字段不一致 → 视为篡改,xmtp_send 回复字段不一致并拒绝,结束。\n\n\
             **Step 2 — 字段一致 → 静默等 job_accepted 系统通知**:\n\
             ❌ **不要**调 `onchainos agent apply`(那是 escrow 路径)\n\
             ❌ **不要**调 `onchainos agent get-payment`(get-payment 推迟到工作完成后,见 JobAccepted 剧本 Step C)\n\
             ❌ **不要** xmtp_send 任何 ACK / 回执给买家(买家此时会立即跑 confirm-accept,不等你 ACK)\n\
             ✅ **直接结束本轮 turn**——等收到 `job_accepted` 系统通知再调 next-action 进入 JobAccepted 剧本\n\n\
             ❌ **严禁把 non_escrow 和 x402 混为一谈**:\n\
             \x20\x20• non_escrow(paymentMode=2)= a2a-pay 付款单,`get-payment` 输出 `paymentId` 字段,买家用此 paymentId 调 `complete`\n\
             \x20\x20• x402(paymentMode=3)= HTTP 402 challenge / response,**没有 paymentId**,买家直接打你的 service-list endpoint 拿 402 响应,签 x402_pay 后重放\n\
             \x20\x20两个是**独立支付方式**。**禁止**在 xmtp_send 里写「非担保 x402 / non_escrow x402 / 非担保(x402)」之类的混合标签。\n\n\
             **任一项未达成** → 调 `xmtp_send(sessionKey=<当前会话 sessionKey,session_status 取>, content=\"很抱歉,无法接受当前条件\")`,结束。\n\n\
             【后续事件】\n\
             - 分支 A apply 上链成功 → 收到 `provider_applied` 系统通知 → 再次调 next-action 拿剧本\n\
             - 分支 B 等买家 confirm-accept → 收到 `job_accepted` 系统通知 → 进入 JobAccepted 剧本 Step A-D 完成「接单 → 干活 → 创建付款单 → 同条 xmtp_send 交付物+paymentId」 一气呵成\n"
        ),
        // ─── buyer 主导的 tx 结果通知，provider 端无需动作 ─────
        Event::JobClosed
        | Event::JobVisibilityChanged
        | Event::JobPaymentModeChanged => format!(
            "【系统通知】{event}（buyer 端 tx 回执，provider 无关）\n\
             【角色】卖家（Provider）\n\n\
             静默忽略,结束本轮 turn。如需详情可调 `onchainos agent common context {job_id} --role provider`。\n",
            event = event.as_str()
        ),

        // ─── buyer 主导的超时事件，provider 端无需动作 ─────
        Event::JobExpired
        | Event::SubmitExpired
        | Event::RefuseExpired
        | Event::ReviewDeadlineWarn => format!(
            "【系统通知】{event}（buyer 端超时事件，provider 无关）\n\
             【角色】卖家（Provider）\n\n\
             静默忽略,结束本轮 turn。如需详情可调 `onchainos agent common context {job_id} --role provider`。\n",
            event = event.as_str()
        ),

        // ─── review_expired: review 窗口超时，卖家主动领货款 ─────────────
        Event::ReviewExpired => format!(
            "【系统通知】review_expired（review 窗口超时，买家未在期限内验收）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ **review_expired 只是窗口超时事件，task 状态仍是 submitted，资金未自动释放**。\n\
             需要你主动调 claimAutoComplete 把资金从担保合约领回，链上确认后才进 completed。\n\n\
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
             - `job_auto_completed`（status=success） → next-action 拿到账剧本（推用户通知，会话保留）\n\
             - `job_auto_completed`（status=failed）  → 按 errorCode 重试 claim-auto-complete\n"
        ),

        // ─── job_auto_completed: claimAutoComplete tx 回执 ────────────────
        Event::JobAutoCompleted => {
            let user_notify = super::content::job_auto_completed_user_notify(job_id);
            let failed_notify = super::content::job_auto_completed_failed_user_notify(job_id);
            format!(
            "【系统通知】job_auto_completed（claimAutoComplete tx 回执）\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 检查 envelope `message.code` 字段：**\n\
             - `code` 非 0（失败）→ 调用 xmtp_dispatch_user 通知用户:\n\
             \x20\x20content: {failed_notify}\n\
             \x20\x20→ 结束 turn。\n\n\
             - `code` = 0（成功）→ 继续 Step 2。\n\n\
             **Step 2 — 用 `xmtp_dispatch_user` 通知用户到账**：\n\n\
             从 `onchainos agent common context {job_id} --role provider --agent-id {agent_id}` 拿任务 title + tokenAmount + tokenSymbol。\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             {user_notify}\n\n\
             ⚠️ 不要给买家 `xmtp_send` 过场——双方都收到 `job_auto_completed` 系统事件了。\n\
             {terminal_session_hint}\n"
            )
        }

        // ─── provider 自己的截止提醒 ─────────────────────────────────────
        Event::SubmitDeadlineWarn => {
            let user_prompt = super::content::submit_deadline_warn_user_prompt(&short_id);
            format!(
            "【系统通知】submit_deadline_warn（提交交付物截止时间快到了）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ 用 `xmtp_prompt_user`等用户决策(立即提交 vs 让超时退款)。\n\n\
             【你的下一步动作】\n\n\
             **Step 1 — 用 `xmtp_prompt_user` 把截止警告 + 决策请求推给用户**:\n\n\
             先调 `session_status` 拿当前 sessionKey(同 turn 只调一次,见硬规则 6);调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\
             tool: xmtp_prompt_user\n\
             llmContent:\n\
             \x20\x20\x20\x20[USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: provider] \
             用户回复『立即提交』/『提交交付物』/类似 → 调用 xmtp_dispatch_session(sessionKey=<sub_key>, content=\"[USER_DECISION_RELAY] 用户决策：立即提交\") 触发当前任务跑交付流程；用户不回复或回复别的 → 不 relay,等 submit_expired 自动退款。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。\n\
             userContent:\n\
             {user_prompt}\n\n\
             **Step 2 — 等用户回复**:\n\
             收到 `[USER_DECISION_RELAY] 用户决策：立即提交` 后:\n\
             1) 调 `onchainos agent pending-decisions remove --job-id {job_id} --role provider --agent-id {agent_id}` 清掉本条 pending(规则 7)\n\
             2) 走交付流程(同 JobAccepted Step 2-3)：自主完成工作 → `xmtp_send` 把交付物发给买家(`{{send_to_peer}}` 模板) → 跑 `onchainos agent deliver` 上链\n\
             \x20\x20(如果想拿完整剧本,可调 `onchainos agent next-action --jobid {job_id} --jobStatus job_accepted --role provider --agentId {agent_id}`,但跳过其中的 Step 1 接单通知——用户已经知道接过单了)\n\n\
             ⚠️ **不要在本 turn 自动跑 `onchainos agent deliver`**——是否准备好交付物只有用户知道,agent 不能替用户决定『交付物已就绪』。\n\
             ⚠️ **不要给买家 `xmtp_send`**——截止警告是 provider 跟用户之间的事,跟买家无关。\n\n\
             跑完 Step 1 → **结束本轮 turn**,等用户回复或等 submit_expired。\n"
            )
        }

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

        // ─── 质押 / 奖励 / 罚没 lifecycle tx 回执 — provider 不是 evaluator 时无关 ─────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::Slashed
        | Event::StakeStopped
        | Event::CooldownEntered => format!(
            "【系统通知】{event}（evaluator 质押 lifecycle tx 回执，provider 无关）\n\
             【角色】卖家（Provider）\n\n\
             静默忽略,结束本轮 turn。\n",
            event = event.as_str()
        ),

        // reward_claimed —— 自己的 claim tx 回执（provider 也可能 claim 仲裁奖励）
        Event::RewardClaimed => {
            let failed_notify = super::content::reward_claim_failed_user_notify(job_id);
            let claimed_notify = super::content::reward_claimed_user_notify(job_id);
            format!(
            "【系统通知】reward_claimed（claimRewards tx 回执）\n\
             【角色】卖家（Provider）\n\n\
             **Step 1 — 检查 envelope `message.code` 字段：**\n\
             - `code` 非 0（失败）→ 调用 xmtp_dispatch_user 通知用户:\n\
             \x20\x20content: {failed_notify}\n\
             \x20\x20→ 结束 turn。\n\n\
             - `code` = 0（成功）→ 继续 Step 2。\n\n\
             **Step 2 — 调用 xmtp_dispatch_user 通知用户奖励已到账:**\n\
             \x20\x20content: {claimed_notify}\n"
            )
        }

        // job_auto_refunded —— buyer 端 tx 回执，provider 无关
        Event::JobAutoRefunded => "【系统通知】job_auto_refunded（buyer 端 claimAutoRefund tx 回执，provider 无关）\n\
             【角色】卖家（Provider）\n\n\
             静默忽略,结束本轮 turn。\n".to_string(),

        Event::WakeupNotify => {
            let wakeup_resume = super::content::wakeup_resume_user_notify(job_id);
            format!(
            "【系统通知】wakeup_notify（网络/电脑重启后任务唤醒）\n\
             【角色】卖家（Provider）\n\n\
             ⚠️ 这是 wake-up 心跳事件,**不是**业务驱动事件。真实业务状态在 envelope.message.jobStatus 字段。\n\
             你不应该用 `wakeup_notify` 作为 --jobStatus 跑剧本——本剧本只是引导。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 envelope 读真实 status**:\n\
             从触发本 turn 的 wakeup_notify envelope 里读 `message.jobStatus` 字段（如 `accepted` / `submitted` / `refused` / `disputed` / `completed` / `rejected` 等真实 status string）。\n\n\
             **Step 2 — 用真实 status 重调 next-action 拿当前剧本**:\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --jobStatus <message.jobStatus 字段值> --role provider --agentId {agent_id}\n\
             ```\n\
             按返回剧本走当前 status 应做动作。\n\n\
             **Step 3 — 幂等性自查（避免重复 prompt 用户）**:\n\
             如果 Step 2 拿到的剧本含 `xmtp_prompt_user` 步骤,**先**调:\n\
             ```bash\n\
             onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
             ```\n\
             - 该 jobId 已有 pending 条目（断线前已 prompt 过）→ **跳过本次 xmtp_prompt_user 重发**,改成 `xmtp_dispatch_user` content=`{wakeup_resume}`\n\
             - 无 pending 条目 → 按 Step 2 剧本正常执行(包括 pending-decisions add + xmtp_prompt_user)\n\n\
             ⚠️ **不要** xmtp_send 给买家「我重新上线了」之类的过场——对方不关心你的连接状态。\n\
             ⚠️ Step 2 拿到的剧本如果是被动等待类（如 status=accepted 卖家正在做事 / status=submitted 等买家验收）,只输出「任务恢复」通知后结束 turn,不主动跑业务动作。\n"
            )
        }

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
