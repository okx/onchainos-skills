//! 任务执行 + 仲裁 + 终态的 prompt 生成函数
//!
//! 从 `flow.rs` 拆分出来的生命周期事件：
//! - provider_applied / job_accepted / job_submitted
//! - job_refused / job_disputed / dispute_evidence / approve_review / reject_review
//! - job_completed / dispute_resolved / job_refunded / job_auto_refunded / job_expired / job_closed
//! - submit_expired / refuse_expired / review_deadline_warn / review_expired / job_auto_completed
//! - reward_claimed / wakeup_notify / create_task
//! - task_token_budget_change / task_provider_change
//! - staked/evaluator lifecycle / unknown fallback

use super::flow::FlowContext;

// ─── 执行阶段 ─────────────────────────────────────────────────────────

pub(super) fn provider_applied(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "【当前状态】provider_applied（服务商已链上申请接单）\n\
     【角色】用户（User Agent）\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 获取任务信息：**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取：providerAgentId、paymentMode、tokenSymbol、tokenAmount。\n\
     ⚠️ paymentMode 此时应为 escrow（1）。\n\n\
     **Step 2 — 执行 confirm-accept（确认接单上链）：**\n\
     ```bash\n\
     onchainos agent confirm-accept {job_id} --provider-agent-id <providerAgentId> --payment-mode escrow --token-symbol <tokenSymbol> --token-amount <tokenAmount>\n\
     ```\n\
     ⚠️ 参数是 `--provider-agent-id`，不是 `--agent-id`。\n\
     🛑 **provider-agent-id 必须与服务商 a2a-agent-chat 消息的 sender.agentId 一致**——优先从本 turn 收到的服务商消息中提取 agentId，其次从 sub session 历史的 [intent:ack] 中提取。不要用 common context 里的值（多任务场景可能串）。\n\
     ⚠️ **不要查询任务 API 验证服务商是否已 apply**——链上索引有延迟，`confirm-accept` 内部会做链上校验。\n\
     ❌ 禁止调 apply（apply 是服务商动作，用户永远不执行）\n\
     ❌ 禁止调 set-payment-mode（已在 negotiate_ack 事件中完成）\n\n\
     → 执行后**结束本轮 turn**，等待 `job_accepted` 系统通知。\n"
    )
}

pub(super) fn job_accepted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;

    let accepted_escrow_notify = super::content::job_accepted_escrow_user_notify(job_id, title_display);
    let accepted_x402_fail = super::content::job_accepted_x402_replay_fail_user_notify(job_id);
    format!(
    "【当前状态】job_accepted（用户已确认接单，任务进入执行阶段）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 获取任务完整信息：**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取：{title_in_extract}description、providerAgentId、paymentMode（int：1=escrow, 3=x402）、tokenAmount、tokenSymbol。\n\n\
     **Step 2 — 按支付方式分流：**\n\n\
     ━━━━━━━━━ 分支 A：escrow（担保）━━━━━━━━━\n\n\
     调用 xmtp_dispatch_user 通知用户接单成功：\n\
     \x20\x20content:\n\
     {accepted_escrow_notify}\n\n\
     【后续事件】\n\
     - job_submitted → 验收交付物\n\n\
     ━━━━━━━━━ 分支 B：x402 ━━━━━━━━━\n\n\
     ⚠️ 回顾本会话上一轮 turn 中 `task-402-pay` 命令的 JSON 输出（该命令在 job_payment_mode_changed 事件处理时执行），\n\
     从中提取 `replaySuccess`、`replayBody`、`replayStatus` 等字段：\n\n\
     **B-分支 1：replaySuccess=true（重放成功，交付物已获取）**\n\n\
     **B-Step 1 — 执行 complete（单签）：**\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     （内部：POST /priapi/v1/aieco/task/{job_id}/direct/complete → 获取 calldata → 签名 uopHash → 广播上链）\n\n\
     ⚠️ **不要通知用户**——交付物已在 task-402-pay 后（A-Step 4）发送过，最终汇总由 job_completed 事件负责。\n\n\
     **B-分支 2：replaySuccess=false（重放失败，未获取交付物）**\n\n\
     ⚠️ **不要执行 complete**——用户未收到交付物，不能完成支付。\n\n\
     **B-Step 1 — 通知用户重放失败：**\n\
     调用 xmtp_dispatch_user：\n\
     \x20\x20content:\n\
     {accepted_x402_fail}\n\n\
     【后续事件】\n\
     - replaySuccess=true: job_completed → 最终确认\n\
     - replaySuccess=false: 等待用户指示（可重试或关闭任务）\n"
    )
}

pub(super) fn job_submitted(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let terminal_session_hint = ctx.terminal_session_hint;

    format!(
    "【当前状态】job_submitted（服务商已提交交付物）\n\
     【角色】用户（User Agent）\n\n\
     🛑🛑🛑 **ABSOLUTE REQUIREMENT — escrow 模式必须用 `xmtp_prompt_user`（不是 `xmtp_dispatch_user`）推送验收决策到 user session**。\n\
     `xmtp_dispatch_user` 是纯通知，用户回复无法 relay 回 sub session → 验收流程卡死。\n\
     `xmtp_prompt_user` 才能携带 llmContent + userContent，让 user session 把用户验收决策 relay 回来。\n\
     🔴 真实事故：Minimax 模型收到 job_submitted 后调了 xmtp_dispatch_user 发了一句「卖家已提交交付物，等待你的验收」——用户看不到交付物内容、无法 relay 验收决策，任务卡死。\n\n\
     🛑🛑🛑 **即使本 turn 内已处理过服务商的 a2a-agent-chat 交付消息（如调了 xmtp_file_download 下载文件），收到 job_submitted 仍必须完整执行下方所有 Step**。\n\
     a2a-agent-chat 处理（下载文件）≠ 验收流程——验收必须由 job_submitted 剧本驱动，交付物内容（文件路径/文字）必须放进 userContent 给用户看。\n\n\
     🛑 **escrow 严禁自动验收**：必须等用户通过 relay 做出决策，Agent 不得替用户决定——无论交付物质量如何、无论是否超时临近。\n\
     ⚠️ x402 模式：资金已支付，只需通知用户交付物内容，用户不能拒绝。\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 0 — 幂等检查：查询是否已有此任务的待决事项：**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     如果返回列表中已存在 jobId={job_id} 且 role=buyer 的条目 → **说明已经通知过用户,本次是重复事件,直接结束 turn,不再通知。**\n\
     如果不存在 → 继续 Step 1。\n\n\
     **Step 1 — 查询任务详情，提取交付物和支付方式：**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     提取 `paymentMode`（int：1=escrow, 3=x402）。\n\
     ⚠️ status 接口不返回 deliverableUrl，该字段从 Step 2 聊天记录中提取。qualityStandards 从 `onchainos agent common context` 获取（按任务发布时为准）。\n\n\
     **Step 2 — 获取交付物内容（区分文字 vs 文件）：**\n\
     ⚠️ **交付物内容必须在本步骤提取并完整放入 Step 3 的 userContent**——之前收到服务商消息时只发了简短通知（「等待链上确认」），用户尚未看到交付物正文。**禁止省略、概括、或只写「已发送给你」**。\n\
     先调 `session_status` 拿到本 sub session 的 sessionKey（后续 Step 3 复用，同 turn 不再重复调）。\n\
     再调 `xmtp_get_conversation_history`（sessionKey = 上一步拿到的 sessionKey）拉取与服务商的聊天记录，完成两件事：\n\
     \x20\x20a) 从 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 提取 `qualityStandards`（验收标准，按任务发布时为准）；如果字段为空则后续展示时省略该行。\n\
     \x20\x20b) 找到服务商发送的**包含 `[intent:deliver]` 后缀标记的消息**（从最新往前找，首个命中即为交付物消息），根据 `deliverableType` 字段判断类型：\n\n\
     ━━━ 情况 A：deliverableType=file（消息包含 fileKey / digest / salt / nonce / secret 解密字段）━━━\n\n\
     调用 xmtp_file_download 工具下载文件：\n\
     \x20\x20参数：\n\
     \x20\x20- fileKey：服务商上传时返回的 fileKey\n\
     \x20\x20- agentId：{agent_id}（用户 agentId）\n\
     \x20\x20- digest：SHA-256 digest（hex）\n\
     \x20\x20- salt：加密 salt（base64）\n\
     \x20\x20- nonce：加密 nonce（base64）\n\
     \x20\x20- secret：加密 secret（base64）\n\
     \x20\x20- filename：（可选）保存文件名\n\
     ⚠️ 调用前输出：`[buyer-xmtp] xmtp_file_download: fileKey=<fileKey>, agentId={agent_id}`\n\
     ⚠️ 调用后输出：`[buyer-xmtp] xmtp_file_download result: localPath=<返回的本地路径>`\n\n\
     下载成功后记录 localPath，**必须是完整绝对路径**（如 /Users/xxx/Downloads/task预发.png）。\n\
     ⚠️ **严禁只显示文件名**（如 cat-picture.png），用户无法定位文件。后续所有展示给用户的内容必须包含完整路径。\n\
     如果下载失败 → 在展示中注明「文件下载失败，请联系服务商重新发送」。\n\
     ⚠️ 如果服务商消息除文件外还包含文字说明（如「这是交付物，请查收」），一并记录到 deliverableText。\n\
     交付物展示变量：deliverableType=file, localPath=<完整路径>, deliverableText=<文字说明，无则留空>\n\n\
     ━━━ 情况 B：deliverableType=text（`---` 分隔符之间的正文内容）━━━\n\n\
     提取 `[intent:deliver]` 消息中 `---` 分隔符之间的文字内容，**完整保留原文**，不要截断或概括。\n\
     交付物展示变量：deliverableType=text, deliverableText=<服务商发送的完整文字内容>\n\n\
     **Step 3 — 按支付方式分流：**\n\n\
     ━━━━━━━━━ 分支 A：escrow（担保）— 需要用户验收决策 ━━━━━━━━━\n\n\
     调用 xmtp_prompt_user 把交付物和验收决策请求推给用户（sessionKey 复用 Step 2 已获取的值;调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`,见硬规则 7）：\n\n\
     \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
     🛑 展示 userContent 后**必须结束本 turn 等用户真实输入**——[USER_DECISION_REQUEST] 是**问题**不是**答案**，禁止同 turn 内编造用户决策。\
     🛑 **禁止执行** onchainos agent 命令（complete/reject/status 等一切 task CLI）——你只负责展示和 relay，不负责执行链上动作。\
     用户**真实回复到达后**（下一 turn）：\
     用户语义「肯定/通过/approve/OK/同意/yes 等」→ **仅调** xmtp_dispatch_session(sessionKey=\"<Step 2 session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:APPROVE_REVIEW] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session，**到此为止**（sub session 收到后自己跑 approve_review 流程，你不要做其它事）；\
     用户语义「否定/拒绝/reject/decline/no 等 + 给出原因」→ **仅调** xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:REJECT_REVIEW] 用户原话：<用户回复原文，包含原因>\") relay 回 sub session，**到此为止**（sub session 收到后自己跑 reject_review 流程，你不要做其它事）。\
     ⚠️ **路由 tag 协议**：`[intent:APPROVE_REVIEW]` / `[intent:REJECT_REVIEW]` 必须**完全大写 ASCII** 原样塞入，**禁止翻译 / 改写 / 省略 / 拆开**——sub 按 intent tag 分支，不再按文字匹配，避免多语言失配。\n\
     ⚠️ relay 必须使用 xmtp_dispatch_session 工具（不要用 sessions_send，它有 session tree 限制）。⚠️ xmtp_dispatch_session 只调用**一次**。{CONSTRAINT}\n\
     \x20\x20\x20\x20userContent（按 deliverableType 分,首行务必带 `[任务 {short_id} 你作为用户]` 前缀）：\n\n\
     \x20\x20\x20\x20▸ deliverableType=file：\n\
     \x20\x20\x20\x20[任务 {short_id} 你作为用户] 服务商已提交交付物（文件），已下载到本地。\n\
     \x20\x20\x20\x20📁 交付物文件路径：<localPath>（⚠️ 必须是完整绝对路径，如 /Users/xxx/Downloads/task预发.png，严禁只写文件名）\n\
     \x20\x20\x20\x20<如果 deliverableText 非空，追加：服务商说明：<deliverableText>>\n\
     \x20\x20\x20\x20<如果 qualityStandards 非空，追加：验收标准：<qualityStandards>>\n\
     \x20\x20\x20\x20支付方式：担保\n\
     \x20\x20\x20\x20请选择：\n\
     \x20\x20\x20\x201. 验收通过 → 回复「验收通过」\n\
     \x20\x20\x20\x202. 拒绝 → 回复「拒绝，原因是<原因>」\n\n\
     \x20\x20\x20\x20▸ deliverableType=text：\n\
     \x20\x20\x20\x20[任务 {short_id} 你作为用户] 服务商已提交交付物（文字）。\n\
     \x20\x20\x20\x20---交付物内容---\n\
     \x20\x20\x20\x20<deliverableText 完整原文，不截断不概括>\n\
     \x20\x20\x20\x20---交付物结束---\n\
     \x20\x20\x20\x20<如果 qualityStandards 非空，追加：验收标准：<qualityStandards>>\n\
     \x20\x20\x20\x20支付方式：担保\n\
     \x20\x20\x20\x20请选择：\n\
     \x20\x20\x20\x201. 验收通过 → 回复「验收通过」\n\
     \x20\x20\x20\x202. 拒绝 → 回复「拒绝，原因是<原因>」\n\n\
     ═══════════════════════════════════════════════════════════════\n\
     🛑🛑🛑 STOP — Step 3 xmtp_prompt_user 调完后 **必须结束本 turn**\n\
     ═══════════════════════════════════════════════════════════════\n\
     本剧本到此结束。后续 turn 收到 `[USER_DECISION_RELAY]` 后，\n\
     按 intent 调 `next-action` 拿对应剧本：\n\
     ▸ `[intent:APPROVE_REVIEW]` → `onchainos agent next-action --jobid {job_id} --jobStatus approve_review --role buyer --agentId {agent_id}`\n\
     ▸ `[intent:REJECT_REVIEW]` → `onchainos agent next-action --jobid {job_id} --jobStatus reject_review --role buyer --agentId {agent_id}`\n\
     ❌ 本 turn 内禁止调 `onchainos agent complete` / `onchainos agent reject`——这两个命令不在本剧本中。\n\
     ═══════════════════════════════════════════════════════════════\n\n\
     ━━━━━━━━━ 分支 B：x402 — 通知用户交付物内容（不可拒绝） ━━━━━━━━━\n\n\
     ⚠️ x402 流程中资金已在 job_accepted 阶段支付，用户**不能拒绝交付物**，只需通知。\n\
     \n\
     **B-Step 1 — 调用 xmtp_dispatch_user 通知用户收到交付物（按 deliverableType 分）：**\n\n\
     \x20\x20▸ deliverableType=file：\n\
     \x20\x20content:\n\
     \x20\x20[交付物已收到] 任务 {job_id} 服务商已提交交付物（x402 模式，资金已支付）。\n\
     \x20\x20📁 交付物文件路径：<localPath>（⚠️ 必须是完整绝对路径，如 /Users/xxx/Downloads/task预发.png，严禁只写文件名）\n\
     \x20\x20<如果 deliverableText 非空，追加：服务商说明：<deliverableText>>\n\
     \x20\x20<如果 qualityStandards 非空，追加：验收标准：<qualityStandards>>\n\n\
     \x20\x20▸ deliverableType=text：\n\
     \x20\x20content:\n\
     \x20\x20[交付物已收到] 任务 {job_id} 服务商已提交交付物（x402 模式，资金已支付）。\n\
     \x20\x20---交付物内容---\n\
     \x20\x20<deliverableText 完整原文，不截断不概括>\n\
     \x20\x20---交付物结束---\n\
     \x20\x20<如果 qualityStandards 非空，追加：验收标准：<qualityStandards>>\n\n\
     **B-Step 2 — 终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     ⚠️ **不要自动评价**——在通知末尾引导用户自行评价：「如需评价服务商，请回复「评价」。」\n\
     任务完整结束。\n\n\
     【后续事件】\n\
     - escrow: job_completed → 任务完成 / job_refused → 等待服务商决定仲裁或退款\n\
     - x402: 流程已结束\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)

}

// ─── 拒绝 / 仲裁 ──────────────────────────────────────────────────────

pub(super) fn job_refused(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let refused_notify = super::content::job_refused_user_notify(job_id, title_display);
    format!(
    "【当前状态】job_refused（用户拒绝交付物已上链，等待服务商决定）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户拒绝已上链，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     {title_query_hint}\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户拒绝已确认：**\n\n\
     content：\n\
     {refused_notify}\n\n\
     **Step 2 — 静默等待服务商决策：**\n\n\
     ⚠️ **不要通过 xmtp_send 向服务商发送任何消息**。服务商有 24h 决定：\n\
     - 发起仲裁 → 你将收到 job_disputed\n\
     - 同意退款 → 你将收到 job_refunded\n\
     - 24h 超时 → 系统自动退款，你将收到 job_refunded\n\n\
     跑完 Step 1 → **结束本轮 turn**，等待下一个系统事件。\n\n\
     【后续事件】\n\
     - job_disputed → 提交用户证据（Scene 6）\n\
     - job_refunded → 退款完成\n"
    )
}

pub(super) fn job_disputed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let evidence_prompt = super::content::job_disputed_user_evidence_prompt(short_id);
    format!(
    "【当前状态】job_disputed（仲裁已发起，进入 1 小时证据准备期）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **CRITICAL — 本事件必须使用 `xmtp_prompt_user` 推送到 user session，禁止在 sub session 中直接输出文字回复。**\n\
     sub session 不面向用户——在 sub session 中直接生成文字回复（哪怕内容正确）= 用户看不到 + relay 通道断裂 + 证据无法提交。\n\
     唯一正确做法：调 `xmtp_prompt_user(llmContent=..., userContent=...)` 工具，把证据收集请求推到 user session。\n\
     ❌ 禁止用文字回复代替 xmtp_prompt_user 工具调用\n\
     ❌ 禁止用 xmtp_dispatch_user 代替 xmtp_prompt_user（dispatch_user 是纯通知无法 relay，用户回复无法路由回 sub）\n\
     ❌ 禁止凭空编造证据摘要直接调 `dispute upload`——sub agent 不知道用户手上有什么证据\n\
     ❌ 禁止通过 xmtp_send 向服务商发送任何消息——仲裁期间双方通过链上证据交互\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 0 — 幂等检查：查询是否已有此任务的待决事项：**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     如果返回列表中已存在 jobId={job_id} 且 role=buyer 的条目 → **说明已经通知过用户,本次是重复事件,直接结束 turn,不再通知。**\n\
     如果不存在 → 继续 Step 1。\n\n\
     **Step 1 — 调用 xmtp_prompt_user 把证据决策请求推给用户：**\n\n\
     先调 `session_status` 拿到本 sub session 的 sessionKey；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\n\
     \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
     🛑 展示 userContent 后**必须结束本 turn 等用户真实输入**——[USER_DECISION_REQUEST] 是**问题**不是**答案**，禁止同 turn 内编造用户决策。\
     🛑 **禁止执行** onchainos agent 命令（complete/reject/dispute 等一切 task CLI）——你只负责展示和 relay，不负责执行链上动作。\
     用户**真实回复到达后**（下一 turn）：\
     用户提供证据后，调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:SUBMIT_EVIDENCE] 用户证据：<用户提供的完整原文，文字 + 图片路径，不解读、不翻译>\") relay 回 sub session 执行 dispute upload。⚠️ **路由 tag 协议**：`[intent:SUBMIT_EVIDENCE]` 必须**完全大写 ASCII** 原样塞入，禁止翻译/改写/省略。⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。⚠️ xmtp_dispatch_session 只调用**一次**。1 小时内必须提交。{CONSTRAINT}\n\
     \x20\x20\x20\x20userContent:\n\
     {evidence_prompt}\n\n\
     **Step 2 — 等用户回复 relay 回来**：收到 `[USER_DECISION_RELAY][intent:SUBMIT_EVIDENCE] 用户证据：...` 后，调 `next-action --jobStatus dispute_evidence` 拿上传剧本（intent tag 已是路由确认；用户证据原文从 `用户证据：` 后面读）。\n\n\
     ⚠️ 1 小时内必须提交证据，过期后失效。\n\n\
     跑完 Step 1-2 → **结束本轮 turn**，等用户回复。\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

pub(super) fn dispute_evidence(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "【当前动作】上传仲裁证据\n\
     【角色】用户（User Agent）\n\n\
     **Step 0 — 清除 pending-decisions：**\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
     **Step 1 — 从 relay 提取证据内容：**\n\
     已通过 `[USER_DECISION_RELAY][intent:SUBMIT_EVIDENCE]` 路由进来，从 `用户证据：` 后面提取：\n\
     - 文字摘要 → 用户提供的文字部分\n\
     - 图片路径（如果用户提供了）→ `--image` 参数\n\
     text 和 image **至少一项**。\n\n\
     **Step 2 — 拉本 sub session 协商 / 交付聊天记录，作为客观证据附在 text 头部：**\n\
     调 `xmtp_get_conversation_history`（sessionKey = 本 sub session 的 sessionKey），拿到与服务商的全部 a2a-agent-chat 历史。\n\
     把历史按下面这种**结构化分段**拼到 `--text` 字段最前面（仲裁员是 LLM，会通读 text 字段判断），后面再贴用户摘要：\n\n\
     ```\n\
     ==== 协商 / 交付聊天记录（从 xmtp_get_conversation_history 拉取） ====\n\
     [时间] 服务商(<agentId>): ...\n\
     [时间] 用户(<agentId>): ...\n\
     ...（按时间顺序，关键节点：报价 / [intent:propose] / [intent:ack] / [intent:confirm] / 交付物消息）\n\n\
     ==== 用户证据摘要 ====\n\
     <用户原话摘要>\n\
     ```\n\n\
     ⚠️ **`--text` 上限 16 KB**——聊天记录过长就**只保留**关键节点（PROPOSE / ACK / CONFIRM / 交付物 / 双方关键争议点），开头标注「（已截取关键节点）」；不要随便丢前 N 条机械截断。\n\n\
     **Step 3 — 调用 CLI 上传证据（链下 multipart）：**\n\
     ```bash\n\
     onchainos agent dispute upload {job_id} --agent-id {agent_id} --text \"<聊天记录 + 用户摘要 拼接后的完整 text>\" --image <用户提供的图片路径或省略>\n\
     ```\n\
     text 和 image **至少一项**；图片可省略整个 `--image` 段，不要给空字符串。\n\n\
     ⚠️ **不要通过 xmtp_send 向服务商发送任何消息**（如「证据已提交」），服务商通过链上事件得知。\n\n\
     【后续事件】\n\
     - job_completed → 仲裁服务商胜诉，任务完成\n\
     - job_refunded → 仲裁用户胜诉，退款\n\n\
     跑完 Step 1-3 → **结束本轮 turn，不要 xmtp_dispatch_user / xmtp_prompt_user 推 main**。\n"
    )
}

pub(super) fn approve_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "【当前动作】验收通过 — 执行 complete 释放款项\n\
     【角色】用户（User Agent）\n\n\
     已通过 `[USER_DECISION_RELAY][intent:APPROVE_REVIEW]` 路由进来，用户已确认验收通过。\n\n\
     **Step 1 — 清除 pending-decisions：**\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
     **Step 2 — 双签验收，释放款项：**\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     内部流程：\n\
     \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-complete（712 标准，非 uop）→ 获取 digest\n\
     \x20\x202. ED25519 签名 digest → signature\n\
     \x20\x203. POST /priapi/v1/aieco/task/{job_id}/complete（body: {{\"signature\": \"<sig>\"}}）→ 获取 uopData\n\
     \x20\x204. 签名 uopHash → 广播上链\n\
     \x20\x20→ 任务状态变为 Complete，资金从合约释放给服务商。\n\n\
     🛑 **complete CLI 成功 ≠ 任务结束**——`complete` 仅提交链上交易，**用户尚未被通知任务完成**。\n\
     此处禁止 xmtp_dispatch_user / xmtp_prompt_user——链上确认后你会收到 `job_completed` 系统事件（`source:\"system\"`），\n\
     由该事件的剧本统一通过 xmtp_dispatch_user 通知用户，此处提前发 = 重复卡片。\n\
     记住 CLI 输出中的 txHash，`job_completed` 剧本会用到。\n\n\
     跑完 Step 1-2 → **结束本轮 turn**。\n\
     ⚠️ **你的工作没有完成**——`job_completed` 系统事件（`source:\"system\"`）到达后，你必须按 SKILL.md Activation 铁律处理，\n\
     否则用户永远收不到「任务已完成」通知、不知道资金已释放。\n"
    )
}

pub(super) fn reject_review(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    format!(
    "【当前动作】拒绝验收 — 执行 reject\n\
     【角色】用户（User Agent）\n\n\
     已通过 `[USER_DECISION_RELAY][intent:REJECT_REVIEW]` 路由进来，用户已拒绝交付物。\n\
     从 relay 消息的 `用户原话：` 后面提取拒绝理由。\n\n\
     **Step 1 — 清除 pending-decisions：**\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\n\
     **Step 2 — 双签拒绝：**\n\
     ```bash\n\
     onchainos agent reject {job_id} --reason \"<用户原话里的拒绝理由>\"\n\
     ```\n\
     内部流程：\n\
     \x20\x201. POST /priapi/v1/aieco/task/{job_id}/pre-refuse（712 标准，非 uop）→ 获取 digest\n\
     \x20\x202. ED25519 签名 digest → signature\n\
     \x20\x203. POST /priapi/v1/aieco/task/{job_id}/refuse（body: {{\"signature\": \"<sig>\", \"reason\": \"<reason>\"}}）→ 获取 uopData\n\
     \x20\x204. 签名 uopHash → 广播上链\n\
     \x20\x20→ 任务状态变为 Refused，服务商 24h 内可发起仲裁。\n\n\
     ⚠️ **不要通过 xmtp_send 向服务商发送任何消息**（如「已拒绝」），服务商通过链上事件得知。\n\n\
     跑完 Step 1-2 → **结束本轮 turn**，等待 `job_refused` 系统通知。\n"
    )
}

// ─── 终态 ─────────────────────────────────────────────────────────────

pub(super) fn job_completed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = ctx.terminal_session_hint;

    let completed_escrow_notify = super::content::job_completed_escrow_user_notify(job_id, title_display);
    let completed_x402_notify = super::content::job_completed_x402_user_notify(job_id, title_display);
    format!(
    "【当前状态】job_completed（任务支付链路完成）\n\
     【角色】用户（User Agent）\n\n\
     🛑🛑🛑 **ABSOLUTE REQUIREMENT — buyer 收到 job_completed 后必须调 `xmtp_dispatch_user` 通知用户**。\n\
     job_completed 是**双方收**事件（buyer + provider 都收到），buyer 必须处理。\n\
     禁止在 sub session 中直接输出文字回复（见硬规则 10）——文字回复 = 用户看不到 = 任务完成了但用户不知道。\n\
     🔴 真实事故：模型误以为 job_completed 只发给 provider，跳过了 xmtp_dispatch_user，用户未收到任务完成通知。\n\n\
     **Step 1 — 获取任务信息和支付方式：**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取：{title_in_extract}tokenAmount、tokenSymbol、paymentMode（int：1=escrow, 3=x402）。\n\n\
     **Step 2 — 按支付方式分流：**\n\n\
     ━━━━━━━━━ 分支 A：escrow（担保）— 流程结束 ━━━━━━━━━\n\n\
     担保模式下 job_completed 意味着服务商已交付且用户已验收，资金从合约释放给服务商。\n\n\
     **A-Step 1 — 调用 xmtp_dispatch_user 告知用户任务完成：**\n\
     ⚠️ txHash：从本 sub session 上下文中找到之前 `onchainos agent complete` CLI 输出的 txHash（格式 0x...）。\n\
     如果上下文中没有（如 auto-complete 等非主动验收场景），省略链上凭证行即可。\n\
     content：\n\
     {completed_escrow_notify}\n\n\
     **A-Step 2 — 终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     ⚠️ **不要自动评价**——在通知末尾引导用户自行评价：「如需评价服务商，请回复「评价」。」\n\
     任务完整结束。\n\n\
     ━━━━━━━━━ 分支 B：x402 — 最终汇总 ━━━━━━━━━\n\n\
     ⚠️ x402 模式下 job_completed 意味着支付链路（accept + complete）已完成上链。\n\
     交付物已在 task-402-pay 阶段（A-Step 4）发送给用户，此处只做最终汇总。\n\n\
     **B-Step 1 — 调用 xmtp_dispatch_user 发送最终汇总：**\n\
     content：\n\
     {completed_x402_notify}\n\n\
     **B-Step 2 — 终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     任务完整结束。\n"
    )
}

pub(super) fn dispute_resolved(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_in_extract = ctx.title_in_extract;
    let terminal_session_hint = ctx.terminal_session_hint;

    let dispute_won = super::content::dispute_won_user_notify(job_id, title_display);
    let dispute_lost = super::content::dispute_lost_user_notify(job_id, title_display);
    format!(
    "【当前状态】dispute_resolved（仲裁已裁决）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户仲裁结果，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     **Step 1 — 判定胜负**：从系统通知 envelope 里读 `message.jobStatus` 字段：\n\
     - `jobStatus = \"rejected\"` → **用户胜诉**\n\
     - `jobStatus = \"complete\"` → **用户败诉**\n\
     - 其他值（如 `disputed`）→ 无法直接判定，执行 Step 1.5 查询任务详情\n\n\
     **Step 1.5（仅 jobStatus 非 rejected/complete 时）— 查询任务详情获取实际状态：**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     从返回的 `jobStatus` 字段判断：`rejected` = 用户胜诉，`complete` = 用户败诉。\n\n\
     **Step 2 — 获取任务信息：**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取 {title_in_extract}tokenAmount、tokenSymbol。\n\n\
     **Step 3 — 调用 xmtp_dispatch_user 通知用户仲裁结果（按胜负分流）：**\n\n\
     ━━━━━━━━━━━━━ 用户胜诉（jobStatus=rejected）━━━━━━━━━━━━━\n\
     content：\n\
     {dispute_won}\n\n\
     ━━━━━━━━━━━━━ 用户败诉（jobStatus=complete）━━━━━━━━━━━━━\n\
     content：\n\
     {dispute_lost}\n\n\
     **Step 4 — 终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     ⚠️ **不要自动评价**。\n\
     仲裁流程完整结束。\n"
    )
}

pub(super) fn job_refunded(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let terminal_session_hint = ctx.terminal_session_hint;

    let refunded_notify = super::content::job_refunded_user_notify(job_id);
    format!(
    "【当前状态】job_refunded（资金已退还用户）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户退款完成，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户退款完成：**\n\n\
     content：\n\
     {refunded_notify}\n\n\
     **Step 2 — 终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     退款流程完整结束。\n"
    )
}

pub(super) fn job_auto_refunded(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = ctx.terminal_session_hint;

    let auto_refunded_notify = super::content::job_auto_refunded_user_notify(job_id, title_display);
    format!(
    "【系统通知】job_auto_refunded（claimAutoRefund tx 回执）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户退款到账，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     {title_query_hint}\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户退款到账：**\n\n\
     content：\n\
     {auto_refunded_notify}\n\n\
     **Step 2 — 终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     退款流程完整结束。\n"
    )
}

pub(super) fn job_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let expired_notify = super::content::job_expired_user_notify(job_id);
    format!(
    "【当前状态】job_expired（任务超时，无人接单或服务商未提交）\n\
     【角色】用户（User Agent）\n\n\
     【你的下一步动作】\n\n\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户任务已超时：**\n\
     \x20\x20content: {expired_notify}\n\n\
     本任务已到达终态，流程结束。\n"
    )
}

pub(super) fn job_closed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = ctx.terminal_session_hint;

    let closed_notify = super::content::job_closed_user_notify(job_id, title_display);
    format!(
    "【当前状态】job_closed（close tx 结果通知）\n\
     【角色】用户（User Agent）\n\n\
     【你的下一步动作】\n\n\
     {title_query_hint}\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户：**\n\
     \x20\x20content: {closed_notify}\n\n\
     **终态收尾（保留 sub session）：**\n\
     {terminal_session_hint}\n\
     任务关闭流程结束。\n"
    )
}

// ─── 超时 / 自动完成 ──────────────────────────────────────────────────

pub(super) fn submit_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let submit_expired = super::content::submit_expired_user_notify(job_id);
    format!(
    "【系统通知】服务商提交交付物超时\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\
     服务商未在规定期限内提交交付物，自动执行退款。\n\n\
     **Step 1 — 立即领取自动退款（无需用户确认）：**\n\
     ```bash\n\
     onchainos agent claim-auto-refund {job_id}\n\
     ```\n\n\
     **Step 2 — 调用 xmtp_dispatch_user 通知用户：**\n\
     content: \"{submit_expired}\"\n"
    )
}

pub(super) fn refuse_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let refuse_expired = super::content::refuse_expired_user_notify(job_id);
    format!(
    "【系统通知】服务商仲裁超时\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\
     你拒绝交付物后，服务商未在规定期限内发起仲裁，自动执行退款。\n\n\
     **Step 1 — 立即领取自动退款（无需用户确认）：**\n\
     ```bash\n\
     onchainos agent claim-auto-refund {job_id}\n\
     ```\n\n\
     **Step 2 — 调用 xmtp_dispatch_user 通知用户：**\n\
     content: \"{refuse_expired}\"\n"
    )
}

pub(super) fn review_deadline_warn(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let review_deadline_prompt = super::content::review_deadline_warn_user_prompt(job_id);
    format!(
    "【系统通知】review_deadline_warn（验收截止时间快到了）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **CRITICAL — 本事件必须使用 `xmtp_prompt_user` 推送到 user session，禁止在 sub session 中直接输出文字回复。**\n\
     验收截止 = 用户资金安全红线——如果用户没收到此通知，超时后资金自动释放给服务商，不可逆。\n\
     ❌ 禁止用文字回复代替 xmtp_prompt_user 工具调用\n\
     ❌ 禁止用 xmtp_dispatch_user 代替 xmtp_prompt_user（用户需要做验收决策，dispatch_user 无法 relay）\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 幂等检查：查询是否已有此任务的待决事项：**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     如果返回列表中已存在 jobId={job_id} 且 role=buyer 的条目 → **说明已经通知过用户,本次是重复事件,直接结束 turn,不再通知。**\n\
     如果不存在 → 继续 Step 2。\n\n\
     **Step 2 — 获取 sessionKey 并注册 pending-decision（硬规则 7）：**\n\
     先调 `session_status` 拿到 sessionKey，然后：\n\
     ```bash\n\
     onchainos agent pending-decisions add --sub-key <sessionKey> --job-id {job_id} --role buyer --agent-id {agent_id} --summary \"验收截止时间即将到期\" --user-content \"[验收截止提醒] 任务 {job_id} 的验收截止时间即将到期。超时后服务商可自动领取资金。请尽快决定：A. 通过验收 B. 拒绝交付物\"\n\
     ```\n\n\
     **Step 3 — 调用 xmtp_prompt_user 通知用户验收截止时间即将到期，请求决策：**\n\
     \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
     🛑 展示 userContent 后**必须结束本 turn 等用户真实输入**——[USER_DECISION_REQUEST] 是**问题**不是**答案**，禁止同 turn 内编造用户决策。\
     🛑 **禁止执行** onchainos agent 命令（complete/reject/status 等一切 task CLI）——你只负责展示和 relay，不负责执行链上动作。\
     用户**真实回复到达后**（下一 turn）：\
     用户语义「肯定/通过/approve/OK/同意/yes 等」→ 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:APPROVE_REVIEW] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session 执行 complete；\
     用户语义「否定/拒绝/reject/decline/no 等 + 给出原因」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:REJECT_REVIEW] 用户原话：<用户回复原文，包含原因>\") relay 回 sub session 执行 reject。\
     ⚠️ **路由 tag 协议**：`[intent:APPROVE_REVIEW]` / `[intent:REJECT_REVIEW]` 必须**完全大写 ASCII** 原样塞入，**禁止翻译 / 改写 / 省略**——sub 按 intent tag 分支，不按文字匹配。\n\
     ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。⚠️ xmtp_dispatch_session 只调用**一次**。{CONSTRAINT}\n\
     \x20\x20userContent:\n\
     {review_deadline_prompt}\n\n\
     **Step 4 — 收到 `[USER_DECISION_RELAY][intent:CODE] 用户原话：...` 后按 intent code 路由：**\n\
     先调 `pending-decisions remove`（硬规则 7）：\n\
     ```bash\n\
     onchainos agent pending-decisions remove --job-id {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     然后按 intent code 执行：\n\
     - `[intent:APPROVE_REVIEW]`：\n\
     ```bash\n\
     onchainos agent complete {job_id}\n\
     ```\n\
     - `[intent:REJECT_REVIEW]`（reason 从 `用户原话：` 后面抽取）：\n\
     ```bash\n\
     onchainos agent reject {job_id} --reason \"<用户原话里的拒绝理由>\"\n\
     ```\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

pub(super) fn review_expired(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let review_expired = super::content::review_expired_user_notify(job_id);
    format!(
    "【系统通知】review_expired（review 窗口超时，task 仍是 submitted）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户验收超时，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     【你的下一步动作】\n\n\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户验收窗口已过期：**\n\
     \x20\x20content:\n\
     {review_expired}\n\n\
     **Step 2** — 等待 `job_auto_completed` 系统通知到达后做收尾。\n"
    )
}

pub(super) fn job_auto_completed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;
    let terminal_session_hint = ctx.terminal_session_hint;

    let auto_completed_notify = super::content::job_auto_completed_user_notify(job_id, title_display);
    format!(
    "【系统通知】job_auto_completed（claimAutoComplete tx 回执）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须调用 `xmtp_dispatch_user` 通知用户任务已自动完成，禁止在 sub session 中直接输出文字回复**（见硬规则 10）。\n\n\
     【你的下一步动作】\n\n\
     {title_query_hint}\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户任务已自动完成：**\n\
     \x20\x20content:\n\
     {auto_completed_notify}\n\n\
     {terminal_session_hint}\n"
    )
}

// ─── 用户操作伪事件 ───────────────────────────────────────────────────

pub(super) fn close_task(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let close_notify = super::content::close_user_notify(job_id);
    format!(
    "【当前动作】关闭任务\n\
     【角色】用户（User Agent）\n\n\
     **Step 1 — 关闭任务（仅 Open 状态有效）：**\n\
     ```bash\n\
     onchainos agent close {job_id}\n\
     ```\n\n\
     **Step 2 — 通知用户：**\n\
     调用 xmtp_dispatch_user：\n\
     content: \"{close_notify}\"\n"
    )
}

pub(super) fn set_public(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    let set_public_notify = super::content::set_public_user_notify(job_id);
    format!(
    "【当前动作】转为公开任务\n\
     【角色】用户（User Agent）\n\n\
     **Step 1 — 转为公开任务：**\n\
     ```bash\n\
     onchainos agent set-public {job_id}\n\
     ```\n\n\
     **Step 2 — 通知用户：**\n\
     调用 xmtp_dispatch_user：\n\
     content: \"{set_public_notify}\"\n"
    )
}

// ─── 其他事件 ─────────────────────────────────────────────────────────

pub(super) fn submit_deadline_warn() -> String {
    "【系统通知】submit_deadline_warn（provider 端截止提醒）\n\
     【角色】用户（User Agent）\n\n\
     【建议】静默观察即可，等 provider 提交交付物（job_submitted 通知）后再处理。\n".to_string()
}

pub(super) fn evaluator_events(event_str: &str) -> String {
    format!(
    "【系统通知】{event_str}（仲裁内部事件，evaluator 处理）\n\
     【角色】用户（User Agent）\n\n\
     【建议】静默观察即可。等 `dispute_resolved` 通知到达后再 next-action 处理收尾。\n"
    )
}

pub(super) fn reward_claimed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let reward_claimed = super::content::reward_claimed_user_notify(job_id, title_display);
    format!(
    "【系统通知】reward_claimed（claimRewards tx 回执）\n\
     【角色】用户（User Agent）\n\n\
     【你的下一步动作】\n\n\
     {title_query_hint}\
     **Step 1 — 调用 xmtp_dispatch_user 通知用户奖励已到账：**\n\
     \x20\x20content: {reward_claimed}\n"
    )
}

pub(super) fn wakeup_notify(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;

    let wakeup_resume = super::content::wakeup_resume_user_notify(job_id);
    format!(
    "【系统通知】wakeup_notify（网络/电脑重启后任务唤醒）\n\
     【角色】用户（User Agent）\n\n\
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
     - 该 jobId 已有 pending 条目（断线前已 prompt 过）→ **跳过本次 xmtp_prompt_user 重发**,改成 `xmtp_dispatch_user` 通知「{wakeup_resume}」\n\
     - 无 pending 条目（首次或之前已 RELAY 关闭）→ 按 Step 2 剧本正常执行(包括 pending-decisions add + xmtp_prompt_user)\n\n\
     ⚠️ **不要** xmtp_send 给服务商「我重新上线了」之类的过场——对方不关心你的连接状态。\n\
     ⚠️ Step 2 拿到的剧本如果是被动等待类（如 status=accepted 等服务商交付）,只输出「任务恢复」通知后结束 turn,不主动跑业务动作。\n"
    )
}

pub(super) fn create_task() -> String {
    "\
🔒 **前置检查**：你是否已读过 `skills/okx-agent-task/SKILL.md` 和 `skills/okx-agent-task/buyer.md`？\n\
如果没有 → **立即停止执行本剧本**，先按 CLAUDE.md 路由规则加载 SKILL.md → 确认角色为 buyer → 读 buyer.md → 再回到此处。\n\
跳过 skill 加载 = 不了解工具白名单/通信协议/安全门 = 后续流程（job_created 事件处理、协商、接单）必然出错。\n\n\
【当前操作】发布任务（create_task）
【角色】用户（User Agent）
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
| 最高预算 | --max-budget | **Required**; ≥ budget; ≤5 位小数; max 10,000,000 | ⚠️ **必须明确询问用户**，不可自动填充或猜测。这是协商价格上限，服务商报价不得超过此值 |
| 接单时限 | --deadline-open | 10 min – 6 months; 格式 `<n>h` / `<n>m` | **必须询问用户**。发布后多久无人接单则自动关闭 |
| 交付时限 | --deadline-submit | 1 min – 6 months; 格式 `<n>h` / `<n>m` | **必须询问用户**。接单后多久内须完成交付 |
| 指定服务商 | --provider | 可选；服务商 agentId | 用户主动提到指定服务商时提取 agentId。**不主动询问**——用户没提就不传 |

🛑 **代币规则（最高优先级）**：
- 用户明确写 \"USDT\" 或 \"USDG\" → 直接用，无需确认
- 用户使用模糊表达（\"U\" / \"u\" / \"刀\" / \"美元\" / \"美金\" / \"dollar\" / \"USD\" / \"100U\" / \"50u\"）→ **必须先问「请确认支付代币：USDT 还是 USDG？」**，等用户明确回复后才填入
- **禁止默认 USDT**，展示 \"100 USDT\" 当用户只说 \"100U\" 是违规

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 2 — 校验（字段全部收集后、展示表单前）
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1. 代币 ≠ USDT 且 ≠ USDG → 「目前只支持 USDT 和 USDG，请选择其中一个。」
2. **预算与最高预算币种一致性**：用户在描述预算和最高预算时如果提到了不同币种（如「预算 10 USDT，最高 20 USDG」）→ **阻断**，「预算和最高预算必须使用同一种代币，请确认你要使用 USDT 还是 USDG？」。任务只有一个 --currency 参数，两者必须统一。
3. 描述 < 10 字符 → 引导补充
4. max_budget < budget → 「最高预算不能小于预算。」
5. max_budget 未填 → 「请设置最高预算（协商价格上限），服务商报价不得超过此值。」
6. budget > 10,000,000 或小数位 > 5 → 提示限制

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
| 指定服务商 | <agentId>（🛑 仅用户主动指定时才展示此行；**未指定则整行不展示**——禁止写「无」「无（公开任务）」等占位。任务默认私有，未指定服务商 ≠ 公开任务） |

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
  --deadline-open <deadline_open> --deadline-submit <deadline_submit> \\
  [--provider <服务商agentId>]
```

⚠️ `--provider`（可选）：指定服务商 agentId。指定后 job_created 将跳过 recommend，直接查询该服务商的 service-list 按支付方式路由（x402 或 A2A 协商）。用户明确要求指定服务商时才传。

🚫 **create-task 只接受以上参数。没有 --content / --period / --visibility / --amount / --token / --payment-mode 参数。** `--provider` 传入时 CLI 自动设置 visibility=1（PRIVATE）和 providerAgentId，无需额外参数。
⚠️ **支付方式不在创建阶段设置**——paymentMode 由后续流程决定：A2A 协商路径固定 escrow，指定服务商且有 endpoint 时走 x402。如果用户在发布任务时提到了支付方式偏好，**不要传 --payment-mode**，告知用户：「支付方式将在与服务商对接时自动确定。」

成功后调 `xmtp_dispatch_user` 通知用户：
- 未指定 --provider → content: 「任务已提交，jobId: <jobId>，等待上链确认（约数秒）。确认后系统将自动联系推荐服务商开始协商。」
- 指定了 --provider → content: 「任务已提交，jobId: <jobId>，等待上链确认（约数秒）。确认后将直接与指定服务商 <agentId> 对接。」

═══════════════════════════════════════════════════════════════
🛑🛑🛑 STOP — create-task 调完后 **必须立即结束本 turn**
═══════════════════════════════════════════════════════════════
❌ **禁止说「任务已发布」「发布成功」**——create-task 只是提交交易，尚未上链确认。
❌ **禁止调 `recommend`**——推荐服务商列表由 backup session 收到 `job_created` 系统通知后自动触发，不在本 turn 执行。
❌ **禁止调任何 onchainos agent 命令**——本 turn 到此结束，后续一切动作等链上事件驱动。
═══════════════════════════════════════════════════════════════
".to_string()
}

// ─── 条款变更事件 ─────────────────────────────────────────────────────

pub(super) fn task_token_budget_change(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    format!(
    "【系统通知】task_token_budget_change（支付代币/金额变更已上链）\n\
     【角色】用户（User Agent）\n\n\
     ⚠️ 本事件由 user session 调用 `set-token-and-budget` 触发。条款已在链上更新。\n\n\
     【接收场景判断——🛑 MANDATORY，判断错误 = 流程卡死】\n\
     本事件会广播到所有用户侧子 session。\n\
     - 如果你是 **backup session** → **忽略本事件，立即结束 turn，不执行任何工具调用**\n\
     - 如果你是 **sub session（与某服务商的协商会话）**→ 先执行 Step 0 活跃检查，再执行后续步骤\n\n\
     【sub session 动作（🛑 四步严格顺序，每步 MUST 等上一步 tool_result 返回后再执行下一步）】\n\n\
     **Step 0 — 🛑 MUST 检查本 session 是否仍活跃（跳过 = 向已终止的服务商发无效消息）：**\n\
     回顾本 session 上下文：如果满足以下**任一条件**，本 session 已终止，**忽略本事件，结束 turn**：\n\
     \x20\x20- 你曾发送或收到 `[intent:reject]`（协商已终止）\n\
     \x20\x20- 你曾调用 `mark-failed` 标记过当前服务商（服务商已被标记失败）\n\
     \x20\x20- 服务商已超过 24h 未回复（协商已冷却）\n\
     如果上下文不足以判断 → 调 `xmtp_get_conversation_history` 检查最近消息，含 [intent:reject] 则终止。\n\
     ⚠️ 只有确认本 session 仍活跃（协商进行中）才继续 Step 1。\n\n\
     **Step 1 — 🛑 MUST 查询最新任务详情（禁止用缓存/旧值）：**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     从返回中提取最新的 tokenSymbol、tokenAmount（budget）。\n\
     ❌ 跳过此步 = PROPOSE 发送旧金额 = 服务商收到过期条款 = 协商基于错误数据\n\n\
     **Step 2 — 🛑 MUST 获取 sessionKey（路径 4 两步必做之一）：**\n\
     调用 `session_status` 工具拿当前 sub session 的 `sessionKey`。\n\
     ❌ 跳过此步 = xmtp_send 缺 sessionKey = 消息发不出去 = 服务商永远收不到新条款\n\n\
     **Step 3 — 🛑 MUST 向服务商发送新一轮 [intent:propose]（不可跳过、不可延迟）：**\n\
     使用 Step 1 拿到的最新 tokenSymbol 和 tokenAmount 构造新的 PROPOSE 消息。\n\
     paymentMode 固定为 escrow（条款变更仅适用于担保支付场景）。\n\n\
     调用 xmtp_send（sessionKey = Step 2 拿到的值）：\n\
     \x20\x20content:\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <Step 1 最新 tokenSymbol>\n\
     \x20\x20tokenAmount: <Step 1 最新 tokenAmount>\n\
     \x20\x20[intent:propose]\n\n\
     ⚠️ 这是新一轮协商，COUNTER 计数器归零。\n\
     ❌ 跳过 Step 3 = 服务商不知道条款已变 = 协商基于旧条款继续 = 最终 accept 参数不一致\n\
     ❌ 禁止 xmtp_dispatch_user（用户在 user session 已知晓变更）\n\
     ❌ 禁止调用 set-token-and-budget / set-provider / set-max-budget（user session 已执行）\n\n\
     → **结束本轮 turn**，等待服务商回复（[intent:ack] / [intent:counter] / [intent:reject]）。\n"
    )
}

pub(super) fn task_provider_change(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;

    format!(
    "【系统通知】task_provider_change（服务商变更已上链）\n\
     【角色】用户（User Agent）\n\n\
     ⚠️ 本事件由 user session 调用 `set-provider` 触发。provider 已在链上更新。\n\n\
     【接收场景判断——🛑 MANDATORY，判断错误 = 流程卡死】\n\
     本事件会广播到所有用户侧子 session。\n\
     - 如果你是 **backup session** → **忽略本事件，立即结束 turn，不执行任何工具调用**\n\
     - 如果你是 **sub session（与某服务商的协商会话）**→ 先执行 Step 0 活跃检查，再执行后续步骤\n\n\
     【sub session 动作（🛑 四步严格顺序，MUST 全部执行）】\n\n\
     **Step 0 — 🛑 MUST 检查本 session 是否仍活跃：**\n\
     回顾本 session 上下文：如果你在本 session 中已发送或收到含 `[intent:reject]` 的消息（协商已终止），\n\
     **忽略本事件，结束 turn**——已终止的协商不需要再发 REJECT。\n\
     只有确认本 session 仍活跃（协商进行中）才继续 Step 1。\n\n\
     **Step 1 — 🛑 MUST 查询任务详情，比对 provider 是否变更（跳过 = 可能误关新服务商的 session）：**\n\
     ```bash\n\
     onchainos agent status {job_id}\n\
     ```\n\
     从返回中提取 `providerAgentId`（链上当前服务商），与**本 session 正在协商的服务商 agentId** 比对：\n\
     \x20\x20- **一致**（本 session 的服务商就是链上最新 provider）→ 本 session 是新服务商的会话，**忽略本事件，结束 turn**，不发 REJECT\n\
     \x20\x20- **不一致**（本 session 的服务商已被替换）→ 继续 Step 2 发 REJECT\n\
     \x20\x20- **providerAgentId 为空或不存在** → 继续 Step 2 发 REJECT（保守处理）\n\
     ❌ 跳过此步 = 无差别对所有 sub session 发 REJECT = 新服务商的 session 也被误关 = 协商中断\n\n\
     **Step 2 — 🛑 MUST 获取 sessionKey（路径 4 两步必做之一）：**\n\
     调用 `session_status` 工具拿当前 sub session 的 `sessionKey`。\n\
     ❌ 跳过此步 = xmtp_send 缺 sessionKey = REJECT 发不出去\n\n\
     **Step 3 — 🛑 MUST 向当前 session 的服务商发送 [intent:reject]（不可跳过）：**\n\
     本任务的 provider 已在链上变更为其他服务商，当前会话的协商即刻终止。\n\
     ❌ 不发 REJECT = 旧服务商不知道被换掉 = 继续等待/发消息 = 协商永远挂起\n\n\
     调用 xmtp_send（sessionKey = Step 2 拿到的值）：\n\
     \x20\x20content:\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20reason: 用户已更换服务商\n\
     \x20\x20[intent:reject]\n\n\
     ❌ 禁止 xmtp_dispatch_user（用户在 user session 已知晓变更）\n\
     ❌ 禁止调用 set-token-and-budget / set-provider / set-max-budget（user session 已执行）\n\
     ❌ 禁止调用 mark-failed（仅终止协商，不排除该服务商）\n\
     ❌ 禁止在 REJECT 后继续与该服务商对话（协商已终止，本 sub session 使命结束）\n\n\
     → **结束本轮 turn**。新服务商的协商由 user session 发起，与本 sub session 无关。\n"
    )
}

// ─── 兜底 ─────────────────────────────────────────────────────────────

pub(super) fn staked_and_unknown(event_str: &str, job_id: &str) -> String {
    format!(
    "【未知状态】{event_str}\n\
     【建议】\n\
     1. 调用 `onchainos agent common context {job_id} --role buyer` 查看完整上下文\n\
     2. 如该状态不在预期流程内，等待用户指示\n\
     3. 不要预测/假设其他通知\n"
    )
}
