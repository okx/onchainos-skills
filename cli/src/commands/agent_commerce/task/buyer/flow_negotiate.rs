//! 协商/匹配阶段的 prompt 生成函数
//!
//! 从 `flow.rs` 拆分出来的协商相关事件：
//! - `job_created`（任务上链 → 推荐/指定服务商路由）
//! - `switch_provider`（用户换服务商后立即启动新流程）
//! - `provider_conversation`（公开任务服务商主动联系）
//! - `job_visibility_changed`（可见性切换）
//! - `job_payment_mode_changed`（支付模式切换上链）
//! - `negotiate_reply` / `negotiate_ack` / `negotiate_counter`（协商中继）

use super::flow::FlowContext;

/// 指定服务商 D-Step 路由（service-list 查询 → x402 或 A2A 分支入口）
pub(super) fn designated_provider_d_steps(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    format!("\
             🎯 **指定服务商**: {dp_id}\n\
             ⚠️ 指定服务商的持久化文件已由 CLI 在生成本提示词时自动删除（consume-on-read），无需手动清理。\n\n\
             **D-Step 1 — 查询服务商 service-list：**\n\
             ```bash\n\
             onchainos agent service-list --agent-id {dp_id}\n\
             ```\n\
             检查返回结果中是否有服务（services 数组非空）以及服务中的 endpoint、feeAmount、feeTokenSymbol 字段。\n\n\
             **D-Step 2 — 按 service-list 结果路由：**\n\
             - **有服务且含 endpoint（支持 x402）** → 提取 services[0] 的 feeAmount、feeTokenSymbol、endpoint。\n\
             \x20\x20⚠️ **feeAmount 是服务商注册时手动填写的，不一定等于链上实际价格**，须经 DX-Step 1 `x402-check` 验证。展示给用户时注明「注册费用」。\n\
             \x20\x20执行以下指定服务商 x402 流程（不跳到 A-Step 1）：\n\n\
             \x20\x20**DX-Step 1 — 验证 endpoint：**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id}\n\
             \x20\x20```\n\
             \x20\x20- `valid=false` → 调用 xmtp_prompt_user 通知用户 endpoint 不合法，引导用户选择下一步（需要用户决策）：\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] 用户语义「选 A / 指定服务商」并提供 agentId → 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<用户提供的agentId>] 用户原话：<用户回复原文>\") relay 回 sub session；用户语义「选 B / 公开」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] 用户原话：<用户回复原文>\") relay；用户语义「选 C / 关闭」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] 用户原话：<用户回复原文>\") relay。⚠️ 路由 tag 协议：intent 名完全大写 ASCII 原样塞入；禁止翻译/改写。⚠️ relay 必须使用 xmtp_dispatch_session。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: [任务 {short_id} 你作为用户] 指定服务商（AgentID={dp_id}）的 x402 endpoint 不合法，无法使用。请选择下一步：\n\
             \x20\x20\x20\x20A. 指定其他服务商 — 请提供服务商 agentId\n\
             \x20\x20\x20\x20B. 转为公开任务 — 让更多服务商看到任务\n\
             \x20\x20\x20\x20C. 关闭任务\n\
             \x20\x20\x20\x20→ **结束本轮 turn**，等用户回复。\n\n\
             \x20\x20**DX-Step 2 — 金额校验：**\n\
             \x20\x20比较 x402-check 的 `amountHuman` 与 services[0] 的 `feeAmount`：\n\
             \x20\x20- 不一致（差异 > 1%）→ 调用 xmtp_prompt_user 询问用户是否接受实际价格：\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] 用户语义「肯定/接受/accept/OK/同意 等」→ 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:ACCEPT_X402_PRICE] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session 继续 DX-Step 3；用户语义「否定/拒绝/reject/decline/no 等」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:REJECT_X402_PRICE] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session 引导换服务商。⚠️ **路由 tag 协议**：`[intent:ACCEPT_X402_PRICE]` / `[intent:REJECT_X402_PRICE]` 必须**完全大写 ASCII** 原样塞入，禁止翻译/改写——sub 按 intent tag 分支，不读用户原话做匹配。⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: 任务 {job_id} 指定服务商（AgentID={dp_id}）实际收费 <amountHuman> <tokenSymbol>，与注册费用 <feeAmount> <feeTokenSymbol> 不一致，是否接受？\n\
             \x20\x20- 一致 → 继续 DX-Step 3。\n\n\
             \x20\x20**DX-Step 3 — 预算检查：**\n\
             \x20\x20先调 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}`，提取 `paymentMostTokenAmount`（最高预算）和任务的 `tokenSymbol`。\n\
             \x20\x20⚠️ **币种校验**：比较 x402-check 返回的 `tokenSymbol` 与任务的 `tokenSymbol`——\n\
             \x20\x20- 不一致（如任务 USDG、x402 收 USDT）→ 因 USDT/USDG 均为 USD 稳定币（≈1:1），仍按数值比较预算。\n\
             \x20\x20\x20\x20`set-payment-mode` 会将链上支付代币**切换为 x402 endpoint 的代币**（不再是任务创建时的币种）。\n\
             \x20\x20- 一致 → 直接比较。\n\
             \x20\x20比较 `amountHuman` 与 `paymentMostTokenAmount`（**不是 tokenAmount，tokenAmount 是基准预算**）：\n\
             \x20\x20- 超出 → 调用 xmtp_prompt_user 通知用户费用超出最高预算，引导用户选择下一步（需要用户决策）：\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] 用户语义「选 A / 指定服务商」并提供 agentId → 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<用户提供的agentId>] 用户原话：<用户回复原文>\") relay 回 sub session；用户语义「选 B / 公开」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] 用户原话：<用户回复原文>\") relay；用户语义「选 C / 关闭」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] 用户原话：<用户回复原文>\") relay。⚠️ 路由 tag 协议：intent 名完全大写 ASCII 原样塞入；禁止翻译/改写。⚠️ relay 必须使用 xmtp_dispatch_session。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: [任务 {short_id} 你作为用户] 指定服务商（AgentID={dp_id}）的 x402 实际费用 <amountHuman> <tokenSymbol> 超出你的最高预算，无法使用。请选择下一步：\n\
             \x20\x20\x20\x20A. 指定其他服务商 — 请提供服务商 agentId\n\
             \x20\x20\x20\x20B. 转为公开任务 — 让更多服务商看到任务\n\
             \x20\x20\x20\x20C. 关闭任务\n\
             \x20\x20\x20\x20→ **结束本轮 turn**，等用户回复。\n\
             \x20\x20- 未超出 → 执行下方 **A-Step 3**。\n\n\
             \x20\x20**A-Step 3 — set-payment-mode（x402 上链）：**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent set-payment-mode {job_id} --payment-mode x402 --token-symbol <x402-check 返回的 tokenSymbol> --token-amount <x402-check 返回的 amountHuman> --endpoint <endpoint>\n\
             \x20\x20```\n\
             \x20\x20⚠️ tokenSymbol 和 tokenAmount 使用 **x402-check 返回的实际值**（不是任务创建时的原始预算）。\n\
             \x20\x20→ **结束本轮 turn**，等待 `job_payment_mode_changed` 系统通知（届时按 Activation 铁律处理，剧本会引导执行 task-402-pay）。\n\n\
             - **无服务或无 endpoint（不支持 x402）** → 进入 **B-Step 1** 建群协商。",
             CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

/// 指定服务商 B-Step 协商协议（三步握手 + 建群 + 多轮协商 + 落盘 + fallback）
pub(super) fn designated_provider_negotiate(job_id: &str, agent_id: &str, short_id: &str, dp_id: &str) -> String {
    let fallback_cmd = format!("onchainos agent mark-failed {job_id} --provider {dp_id} && onchainos agent recommend {job_id} --agent-id {agent_id}");
    let fallback_lines = format!("先执行 `onchainos agent mark-failed {job_id} --provider {dp_id}` 标记失败，再执行 `onchainos agent recommend {job_id} --agent-id {agent_id}` 获取新推荐列表。\n\
             \x20\x20如果列表非空 → 按 xmtp_prompt_user 模板展示给用户选择（格式同非指定服务商的 Step 2：列出服务商信息 + 选择/翻页/公开/关闭选项）。\n\
             \x20\x20如果列表为空 → 按下方引导用户选择 A/B/C");
    format!("\
             🛑 **硬约束 — 三步握手是让服务商 apply 的唯一合法路径**\n\n\
             你想让服务商进入 apply 阶段（escrow），**必须**完整发完三步握手：\n\
             \x20\x201) `[intent:propose]`（你 → 服务商，结构化提案）\n\
             \x20\x202) 等服务商回 `[intent:ack]`（字段全等）或 `[intent:counter]`（继续谈）或 `[intent:reject]`（服务商拒绝）\n\
             \x20\x203) 你回 `[intent:confirm]`（原样回传 ACK 字段，服务商见到这个标记才会 apply）\n\
             \x20\x20⚡ 任一方可随时发 `[intent:reject]` 终止协商（含 jobId + reason），收到后**不再回复**，立即切换下一个服务商。\n\n\
             ❌ **禁止用自然语言绕过握手**——不要发以下这种消息：\n\
             \x20\x20• 「协商条款已锁定 / 条款已敲定 / 无需额外提案 / 请你直接 apply / 请直接接单」\n\
             \x20\x20• 「最终确认：任务/价格/支付方式 ...」之类的纯文字总结，没带 [intent:propose] / [intent:confirm] 标记\n\
             \x20\x20• 任何形式的「替代握手」短路——服务商 flow 里把 `[intent:confirm]` 字面量当作 apply 唯一触发器，你发自然语言『请 apply』根本不会被识别，服务商只能继续等 [intent:propose]\n\n\
             正确做法：协商达成一致后，**严格用** `[intent:propose]` 模板（见下方 B-Step 2 Step 4），让握手机器解析跑通。**协商再短也要走完三步**——哪怕是「能做、原价 OK、escrow OK」三连答，也要把它变成 [intent:propose] 发出去，绝不省略。\n\n\
             ━━━━━━━━━ 分支 B：supportA2MCP=false → A2A（需协商）━━━━━━━━━\n\n\
             **B-Step 0 — 防重复检查（🛑 硬门禁）：**\n\
             调 `session_status` 检查当前 job 是否已有 sub session（即是否已建群）。\n\
             如果**已存在** sub session → 说明首轮询盘已发过。**立即结束本轮 turn**——不建群、不发消息、不发询盘、不执行后续任何 B-Step。\n\
             如果**不存在** → 继续 B-Step 1。\n\n\
             **B-Step 1 — 建群：**\n\
             调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
             \x20\x20参数：myAgentId={agent_id}，toAgentId=<{dp_id}>，jobId={job_id}\n\
             \x20\x20成功返回 sessionKey + xmtpGroupId。\n\
             \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<providerAgentId>, jobId={job_id}`\n\
             \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
             **B-Step 2 — 自动协商（用户 Agent ↔ 服务商 Agent 在 sub session 中多轮交互）：**\n\
             🛑 **建群后同一 turn 内必须调 `xmtp_send` 发首条询盘消息**——建群只是创建通道，不发消息 = 服务商收不到任何信号 = 流程卡死。\n\
             ❌ 绝对禁止建群后结束 turn 不发消息\n\
             ❌ 绝对禁止用 xmtp_dispatch_user / xmtp_dispatch_session 代替 xmtp_send——建群后统一用 xmtp_send\n\n\
             协商目标：就以下结构化字段达成一致（其他字段按用户发布任务时为准，不协商）——\n\
             \x20\x20- paymentMode：支付方式（**A2A 协商会话中固定为 escrow**——x402 走 recommend 自动路由，不进协商）\n\
             \x20\x20- tokenSymbol：支付代币\n\
             \x20\x20- tokenAmount：支付金额\n\n\
             ⏱ 超时规则：每轮等待服务商回复最多 5 分钟。超时未回复 → 先 xmtp_send 发 `[intent:reject]`（reason: 协商超时，5 分钟未回复）给服务商，再 `{fallback_cmd}` 切换下一个服务商（**禁止 xmtp_delete_conversation 删群**）。超时后若再收到该服务商的 a2a-agent-chat 消息，**不回复、不处理**，直接忽略。\n\n\
             ⚠️ **协商消息格式铁律**：所有协商阶段的结构化消息（PROPOSE / CONFIRM / REJECT）**必须以对应 `[intent:*]` 后缀标记结尾**，\n\
             content 最后一行必须是 `[intent:propose]` / `[intent:confirm]` / `[intent:reject]`，**严禁用自然语言替代**。\n\
             服务商 Agent 通过后缀做机器解析，缺少后缀会导致协商流程卡死。\n\n\
             📌 **你有完整的协商权 —— 不要机械接受服务商任何报价**。看 context 里的【任务详情】+【服务商 profile / service-list / 历史 securityRate / feedback】，自己判断：\n\
             \x20\x20• 服务商给的价格相对任务工作量是否合理；超过你预算上限就不要勉强答应\n\
             \x20\x20• 服务商 profile / service-list 同类服务单价 vs 当前报价（服务商自己挂的价就是参考锚）\n\
             \x20\x20• A2A 协商路径 paymentMode 固定为 escrow（资金有担保保障）\n\
             \x20\x20• 多个推荐服务商的话，不要勉强跟某一个谈拢；不合适直接 5 分钟超时切下一个\n\n\
             🛑🛑🛑 **ABSOLUTE PROHIBITION — 铁律：协商全程禁止向服务商透露最高预算（max_budget / paymentMostTokenAmount）。**\n\
             任何发给服务商的消息（自然语言、[intent:propose]、[intent:confirm]）中都**绝对不能**包含 max_budget 数值。\n\
             泄露最高预算 = 服务商直接报上限价 = 用户丧失全部议价能力。\n\
             ❌ 绝对禁止在 xmtp_send 中提及「最高预算」「上限」「max budget」「最多能出」等字眼或对应数值\n\
             ❌ 绝对禁止把 paymentMostTokenAmount 字段值写入任何发给服务商的消息\n\n\
             协商步骤：\n\
             1. 调用 xmtp_send 发送第一条询盘消息（自然语言，让服务商先给报价，你再判断）：\n\
             \x20\x20content=<任务描述 + 期望交付物 + paymentMode 倾向 + budget（基准预算），**禁止暴露 max_budget**>\n\
             \x20\x20→ 等待服务商回复（5 分钟超时）\n\
             2. （sub session 内）服务商回复报价（金额、代币、支付方式偏好、预计交付时间）\n\n\
             🛑 **评估前置（强制）— 收到服务商回复后，必须先完成以下步骤再发送任何 xmtp_send**：\n\
             \x20\x20a) `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 获取 budget / max_budget\n\
             \x20\x20b) 从服务商回复中提取报价、能力信息\n\
             \x20\x20c) 按下方 Step 2.5 决策矩阵评估\n\
             \x20\x20❌ 禁止在 a-c 未完成前发送任何 xmtp_send（包括拒绝）——跳过评估直接回复 = 决策无依据\n\n\
             🔴 **Step 2.5 — 服务商首次报价评估（全自动，禁止问用户）**：\n\
             收到服务商自然语言报价后，**立即**从报价中提取最低价格，与任务 budget / max_budget 做对比。\n\
             max_budget 从 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 的 `paymentMostTokenAmount` 字段获取。\n\n\
             \x20\x20| 服务商报价 | 动作 | 说明 |\n\
             \x20\x20|---|---|---|\n\
             \x20\x20| ≤ budget | → 价格可接受；继续确认 paymentMode 等条款，全部明确后进 Step 4 | 价格在预算内，但其他条款仍需协商 |\n\
             \x20\x20| budget < 报价 ≤ max_budget | → 进 Step 3 自然语言还价 | 有谈判空间，自主砍价 |\n\
             \x20\x20| > max_budget | → **自动 REJECT + 切换**（见下方） | 超出硬上限，不可接受 |\n\n\
             \x20\x20**报价 > max_budget 的强制动作（全自动执行，不询问用户，不 xmtp_dispatch_user）**：\n\
             \x20\x20a) xmtp_send 发送 `[intent:reject]`：\n\
             \x20\x20\x20\x20content=\n\
             \x20\x20\x20\x20jobId: {job_id}\n\
             \x20\x20\x20\x20reason: 报价超出最高预算\n\
             \x20\x20\x20\x20[intent:reject]\n\
             \x20\x20b) `{fallback_cmd}` 切换下一个服务商\n\
             \x20\x20c) 回到 Step 2 路由判断\n\n\
             3. （sub session 内）双方就价格/条件进行自然语言调整（可能多轮，每轮 5 分钟超时，服务商 COUNTER 上限 3 次）\n\
             \x20\x20每轮调用 xmtp_send，参数：sessionKey=<同上>，content=<协商内容>\n\
             \x20\x20⚠️ **不要机械接受服务商加价**：以**任务的 max_budget（最高预算）为绝对上限**——超过 max_budget 一律拒绝，不论差多少。`budget < 服务商价 ≤ max_budget` 区间内可谈，可以原价接受或继续还价；服务商价 ≤ budget 直接接受。\n\
             ⚠️ **币种可协商**：tokenSymbol 允许双方协商变更（如 USDT ↔ USDG），但须双方明确同意。协商初始币种从 `onchainos agent common context` 获取。\n\n\
             ⚠️ 任一步骤服务商 5 分钟未回复 → 视为协商超时，先 xmtp_send 发 `[intent:reject]`（reason: 协商超时）给服务商，再 `{fallback_cmd}` 切换下一个服务商（**不删群**）。超时后再收到该服务商消息一律忽略、不回复。\n\n\
             4. 达成初步一致后，调用 xmtp_send 发送 **[intent:propose]** 结构化提案（必须严格使用此格式，服务商 Agent 会机器解析）：\n\
             \n\
             📋 **填字段前必做的口头记录自检（防止『记忆穿越』）**：\n\
             \x20\x20在写 [intent:propose] 任何字段前，**逐字段从最近一条往前回看本 sub session 的所有 xmtp_send 内容**，找到**最后一次双方明确同意的值**：\n\
             \x20\x20- tokenAmount：以**最后一次自然语言达成的价格**为准（不是任务原始预算、不是 recommend 列表里的标价、不是中间任意一轮的报价）\n\
             \x20\x20- paymentMode：同样取最后一次共识\n\
             \x20\x20- 任一字段在对话里没有明确共识 → **不要发 [intent:propose]**，先 xmtp_send 自然语言再确认一次\n\
             \x20\x20⚠️ 不要凭印象直接填——你的训练数据里没有本次会话的记忆，唯一可靠来源是回看本 sub session 的消息历史。\n\n\
             \x20\x20content=\n\
             jobId: {job_id}\n\
             paymentMode: escrow\n\
             tokenSymbol: <USDT|USDG>\n\
             tokenAmount: <金额>\n\
             [intent:propose]\n\n\
             5. **等待服务商回复 [intent:ack] 或 [intent:counter]**（5 分钟超时）：\n\n\
             \x20\x20▸ 收到 **[intent:ack]** → 逐字段校验服务商回传的值与你发送的 PROPOSE 完全一致：\n\
             \x20\x20\x20\x20- 全部一致 → ✅ **立即执行 Step 6**（不发任何消息，直接跑 bash 命令）：\n\
             \x20\x20\x20\x20\x20\x20🚫 **此处禁止 xmtp_send**——不发 [intent:confirm]、不发自然语言、不发任何消息。\n\
             \x20\x20\x20\x20\x20\x20[intent:confirm] 必须等 Step 6 的 set-payment-mode 上链确认（`job_payment_mode_changed` 事件）后才发。\n\
             \x20\x20\x20\x20\x20\x20→ **现在**跳到下方 Step 6，执行 save-agreed + set-payment-mode。\n\
             \x20\x20\x20\x20- 任一字段不一致 → 视为篡改，调 xmtp_send 告知服务商字段不一致并重新发送 [intent:propose]\n\n\
             \x20\x20▸ 收到 **[intent:counter]** → **先计数**：回看本 sub session 历史，统计服务商已发送的 `[intent:counter]` 总次数（含本次）。\n\
             \x20\x20\x20\x20🔢 **COUNTER 轮次上限 = 3 次**：如果本次是第 3 次（含）以上 COUNTER，**不处理 COUNTER 内容**，直接 xmtp_send 发 `[intent:reject]`（reason: 协商轮次超限，已达 3 次 COUNTER），然后 `{fallback_cmd}` 切换下一个服务商。\n\
             \x20\x20\x20\x20未超限 → 继续下方价值判断：\n\n\
             \x20\x20\x20\x20服务商提出反提案，**带价值判断决定接不接，不要机械接受**：\n\
             \x20\x20\x20\x20⚠️ **第 0 步：先回看 sub session 历史，确认你刚才发的 [intent:propose] 是否填错了**：\n\
             \x20\x20\x20\x20\x20\x20· 回看自然语言协商最后一次明确同意的金额 / paymentMode\n\
             \x20\x20\x20\x20\x20\x20· 如果 COUNTER 的金额**等于**自然语言里你最后同意的那个数 → **是你 PROPOSE 写错了，不是服务商加价**：直接用 COUNTER 的金额重发新 [intent:propose]，**不要再讨价还价**也不要嘴硬说『我们之前是 X』，直接修正即可\n\
             \x20\x20\x20\x20\x20\x20· 如果 COUNTER 的金额**高于**自然语言里你最后同意的数 → 才是服务商加价，按下方决策矩阵处理\n\n\
             \x20\x20\x20\x20- 检查 tokenSymbol 改动：服务商提出不同币种时评估是否可接受（须双方明确同意）\n\
             \x20\x20\x20\x20- 评估 tokenAmount（**max_budget 优先，不是百分比**）：\n\
             \x20\x20\x20\x20\x20\x20· COUNTER 价 ≤ 任务 budget（原预算）→ 可接受，用 COUNTER 值发新 [intent:propose]\n\
             \x20\x20\x20\x20\x20\x20· budget < COUNTER 价 ≤ max_budget（最高预算）→ 可接受，或继续还价取折中（带理由发新 [intent:propose]）\n\
             \x20\x20\x20\x20\x20\x20· COUNTER 价 > max_budget → 调 xmtp_send 发送 `[intent:reject]` 结束协商，然后**立即** `{fallback_cmd}` 切换下一个服务商：\n\
             \x20\x20\x20\x20\x20\x20\x20\x20content=\n\
             \x20\x20\x20\x20\x20\x20\x20\x20jobId: {job_id}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20reason: 报价超出最高预算\n\
             \x20\x20\x20\x20\x20\x20\x20\x20[intent:reject]\n\
             \x20\x20\x20\x20\x20\x20· max_budget 不知道 → 调 `onchainos agent common context {job_id} --role buyer --agent-id {agent_id}` 取 `paymentMostTokenAmount` 字段\n\
             \x20\x20\x20\x20- paymentMode 固定为 escrow，不接受其他支付方式\n\
             \x20\x20\x20\x20- 全部可接受 → 用 COUNTER 中的值发新的 [intent:propose]，回到 Step 5 等 ACK\n\n\
             \x20\x20▸ 收到 **[intent:reject]** → 服务商主动拒绝协商。**不再回复**，立即 `{fallback_cmd}` 切换下一个服务商。\n\n\
             \x20\x20▸ 收到的回复**不含** [intent:ack] / [intent:counter] / [intent:reject] 标记 → 视为自然语言讨论，继续协商，重新回到 Step 4\n\n\
             6. **收到 [intent:ack] 全等 → 落盘 + setPaymentMode → 最后才发 [intent:confirm]**：\n\n\
             🛑 **顺序铁律（[intent:confirm] 是服务商 apply 的唯一触发器，必须 paymentMode 在链上就位后才发，否则服务商 apply 会基于错的支付状态）**：\n\n\
             **Step 6.1 — save-agreed 落盘**（无条件第一步）：\n\
             ```bash\n\
             onchainos agent save-agreed {job_id} --provider <当前协商的providerAgentId> --token-symbol <协商币种> --token-amount <协商价格> --agent-id {agent_id}\n\
             ```\n\
             不保存会导致后续 confirm-accept 使用错误的币种/金额。\n\n\
             **Step 6.2 — 执行 setPaymentMode（无条件，不判断当前链上值）**：\n\
             ⚠️ **不论链上 paymentType 当前是什么值（0 / 1 / 2 / 3），都必须执行 set-payment-mode。** 不要查 common context 比较——直接调：\n\
             ⚠️ **A2A 协商会话中固定 escrow**：无论服务商是否有 endpoint，协商会话中只用 escrow。此处 set-payment-mode 会覆盖链上值。\n\n\
             ```bash\n\
             onchainos agent set-payment-mode {job_id} --payment-mode escrow --token-symbol <协商币种> --token-amount <协商价格>\n\
             ```\n\
             此命令执行 setPaymentMode → 签名 → 广播，然后返回 exit code 2 (confirming)。\n\
             ⚠️ **绝对不要**在此 turn 内 xmtp_send [intent:confirm]——服务商见 [intent:confirm] 会立刻 apply，但链上 paymentMode 还在 mempool / 没确认，apply 会失败或行为错位。[intent:confirm] 必须等 `job_payment_mode_changed` 事件确认 paymentMode 上链后再发。\n\n\
             **Step 6.3 — 结束本轮 turn**，等待 `job_payment_mode_changed` 系统通知。\n\n\
             （新一 turn）收到 `job_payment_mode_changed` → 调 next-action --jobStatus job_payment_mode_changed → 按剧本 xmtp_send [intent:confirm] 给服务商。服务商此时见 CONFIRM → apply（escrow），链上 paymentMode 已就位。\n\n\
             ━━━━━━━━━ 协商失败 / 切换服务商 ━━━━━━━━━\n\n\
             当前服务商超时未回复（5 分钟）/ COUNTER 轮次超限（≥3 次）/ 收到 `[intent:reject]` / 协商失败 → 先 xmtp_send 发 `[intent:reject]`（reason 填超时/超限/失败原因）给服务商，再切换：\n\
             \x20\x20{fallback_lines}\n\
             ⚠️ **切换时必须先发 [intent:reject] 再切走**（让服务商有明确终止信号），但**禁止 xmtp_delete_conversation 删群**。切走后再收到该服务商消息一律忽略、不回复。\n\
             当前页无剩余服务商且翻页也无结果 → 先调 `session_status` 拿 sessionKey；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7);再调用 xmtp_prompt_user 引导用户选择：\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] \
             用户语义「选 A / 指定服务商」并提供 agentId → 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<用户提供的agentId>] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session，sub agent 查 service-list 后路由（x402 或建群协商）；\
             用户语义「选 B / 公开 / public」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session 执行 set-public；\
             用户语义「选 C / 关闭 / close」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] 用户原话：<用户回复原文，不解读、不翻译>\") relay 回 sub session 执行 close。\
             ⚠️ **路由 tag 协议**：`[intent:PICK_PROVIDER agentId=<...>]` / `[intent:SET_PUBLIC]` / `[intent:CLOSE_TASK]` 必须**完全大写 ASCII** 原样塞入；禁止翻译/改写——sub 按 intent tag 分支，不读用户原话做匹配。\
             ⚠️ relay 必须使用 xmtp_dispatch_session（不要用 sessions_send）。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\
             \x20\x20userContent: [任务 {short_id} 你作为用户] 推荐服务商均不合适。请选择下一步：\n\
             \x20\x20A. 指定服务商 — 请提供服务商 agentId\n\
             \x20\x20B. 转为公开任务 — 让更多服务商看到任务\n\
             \x20\x20C. 关闭任务 — 取消并退款\n\
             \x20\x20→ **结束本轮 turn**，等用户回复 relay 回来后继续执行。\n\n\
             【后续事件】\n\
             - x402 → set-payment-mode → job_payment_mode_changed → task-402-pay（签名 + direct/accept + endpoint 重放）→ job_accepted → complete\n\
             - escrow → set-payment-mode → job_payment_mode_changed → 通知服务商 apply → 服务商 apply 上链 → 服务商 xmtp_send 通知用户 → 用户收到 a2a-agent-chat → confirm-accept → job_accepted\n",
             CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

// ─── Event handler functions ─────────────────────────────────────────────

pub(super) fn job_created(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let designated_provider = super::negotiate::take_designated_provider(job_id).ok().flatten();

    let notify_text = match &designated_provider {
        Some(dp_id) => format!("正在对接指定服务商 {dp_id}..."),
        None => "正在自动查询推荐服务商...".to_string(),
    };

    let created_notify = super::content::job_created_user_notify(job_id, &notify_text);

    let routing_section = if let Some(dp_id) = &designated_provider {
        designated_provider_d_steps(job_id, agent_id, short_id, dp_id)
    } else {
        format!("\
             **Step 0 — 幂等检查：查询是否已有此任务的待决事项：**\n\
             ```bash\n\
             onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
             ```\n\
             如果返回列表中已存在 jobId={job_id} 且 role=buyer 的条目 → **说明已经通知过用户,本次是重复事件,直接结束 turn,不再通知。**\n\
             如果不存在 → 继续 Step 1。\n\n\
             **Step 1 — 查询推荐服务商列表：**\n\
             ```bash\n\
             onchainos agent recommend {job_id} --agent-id {agent_id}\n\
             ```\n\
             输出服务商列表（Agent Name / 服务描述 / 信用分 / 支付方式），已自动过滤协商失败的服务商。\n\n\
             🛑🛑🛑 **ABSOLUTE PROHIBITION — 铁律：禁止在当前 session（sub/backup）中直接展示推荐列表或任何文字回复。**\n\
             你现在所在的是 sub session 或 backup session——**用户看不到这里的任何输出**。\n\
             你必须调用 `xmtp_prompt_user` 工具把列表推到 user session，这是**唯一**让用户看到列表的方法。\n\
             🔴 真实事故：Minimax 模型在 backup session 拿到 recommend 结果后直接输出列表文字，用户完全没看到，任务卡死。\n\
             ❌ 绝对禁止用文字回复代替 xmtp_prompt_user 工具调用——文字回复 = 用户不可见 = 任务卡死\n\
             ❌ 绝对禁止用 xmtp_dispatch_user 代替 xmtp_prompt_user——dispatch_user 无法 relay，用户选择无法路由回 sub\n\
             ❌ 绝对禁止先输出文字「给用户看」再调工具——sub session 的文字输出永远不会到达用户\n\n\
             **Step 2 — 展示列表给用户，让用户选择：**\n\
             调 `session_status` 拿 sessionKey；调 `pending-decisions add`（见硬规则 7）；再调 `xmtp_prompt_user`：\n\n\
             \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer]\n\
             \x20\x20序号→AgentID 映射：<从 recommend 输出提取，格式如 1→798, 2→806, 3→866, 4→864, 5→865, 6→916, 7→810>\n\
             \x20\x20用户回复数字（如 \"2\"\"4\"）→ 按上方映射表转为 AgentID；用户回复 3 位数 AgentID（如 \"864\"）→ 直接使用。\n\
             \x20\x20路由规则：\n\
             \x20\x20- 用户选服务商（数字序号或 AgentID）→ xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<映射后的AgentID>] 用户原话：<用户回复原文>\")\n\
             \x20\x20- 用户说「下一页/more/next」→ xmtp_dispatch_session(sessionKey=\"<同上>\", content=\"[USER_DECISION_RELAY][intent:NEXT_PAGE] 用户原话：<用户回复原文>\")\n\
             \x20\x20- 用户说「公开/public」→ xmtp_dispatch_session(sessionKey=\"<同上>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] 用户原话：<用户回复原文>\")\n\
             \x20\x20- 用户说「关闭/取消/close」→ xmtp_dispatch_session(sessionKey=\"<同上>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] 用户原话：<用户回复原文>\")\n\
             \x20\x20⚠️ intent tag 必须完全大写 ASCII 原样塞入，禁止翻译/改写。relay 必须使用 xmtp_dispatch_session。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\n\
             \x20\x20⚠️ llmContent 中的「序号→AgentID 映射」**必须**从 recommend 输出逐行提取并内联——user session agent 看不到 userContent 列表，没有映射表就无法把用户的序号转为 AgentID，导致路由失败。\n\n\
             \x20\x20userContent: [任务 {short_id} 你作为用户] 以下是推荐服务商列表：\n\
             \x20\x20<将 recommend 输出的服务商列表完整粘贴，每个服务商一段：序号 / Agent Name / AgentID / 服务名称与描述 / 信用分 / 费用 / 支付方式>\n\
             \x20\x20---\n\
             \x20\x20请选择：回复序号（如 1、2、3）或 AgentID（如 864）选择服务商 | 回复「下一页」查看更多 | 回复「公开」转为公开任务 | 回复「关闭」关闭任务\n\n\
             → **结束本轮 turn**，等用户回复 relay 回来。\n\n\
             **Step 3 — 收到用户 relay 后处理：**\n\n\
             ▸ 用户选择了某个服务商（agentId=X）→ 调用 `next-action --provider X` 进入指定服务商流程：\n\
             ```bash\n\
             onchainos agent next-action --jobid {job_id} --jobStatus job_created --role buyer --agentId {agent_id} --provider <用户选中的agentId>\n\
             ```\n\
             按输出的剧本执行（内部自动查 service-list 路由 x402 或 A2A）。\n\n\
             ▸ 用户要求翻页 → 执行：\n\
             ```bash\n\
             onchainos agent recommend {job_id} --next-page\n\
             ```\n\
             如果有结果 → 回到 Step 2 展示新列表给用户。\n\
             如果为空 → 调用 xmtp_prompt_user 通知用户无更多服务商，引导选择（需要用户决策，不能用 xmtp_dispatch_user）：\n\
             \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer] 用户语义「选 A / 指定服务商」并提供 agentId → 调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<用户提供的agentId>] 用户原话：<用户回复原文>\") relay 回 sub session；用户语义「选 B / 公开」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:SET_PUBLIC] 用户原话：<用户回复原文>\") relay；用户语义「选 C / 关闭」→ 调用 xmtp_dispatch_session(sessionKey=\"<同上 sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] 用户原话：<用户回复原文>\") relay。⚠️ 路由 tag 协议同上。⚠️ relay 必须使用 xmtp_dispatch_session。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\
             \x20\x20\x20\x20userContent: [任务 {short_id} 你作为用户] 没有更多推荐服务商了。请选择下一步：\n\
             \x20\x20\x20\x20A. 指定服务商 — 请提供服务商 agentId\n\
             \x20\x20\x20\x20B. 转为公开任务 — 让更多服务商看到任务\n\
             \x20\x20\x20\x20C. 关闭任务\n\n\
             ▸ 用户选择转为公开任务 → `onchainos agent set-public {job_id}`\n\n\
             ▸ 用户选择关闭任务 → `onchainos agent close {job_id}`",
             CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
    };

    let mut output = format!(
        "🛑🛑🛑 **IDENTITY CHECK — 你就是执行者，禁止委托**\n\
         你现在所在的是 sub session 或 backup session。**你自己**就是负责执行本剧本的 agent。\n\
         ❌ **绝对禁止** `sessions_spawn`——不要派生子 agent 来「帮你」处理本事件\n\
         ❌ **绝对禁止** `sessions_yield`——不要交出控制权\n\
         🔴 真实事故：backup 收到 job_created 后调 sessions_spawn 委托给子 agent，导致 designated-provider 消费上下文断裂、协商流程不可控。\n\
         **正确做法**：你自己按下方步骤逐步执行 CLI 命令和 xmtp 工具调用。\n\n\
         【当前状态】job_created（任务已上链，状态：待接单）\n\
         【角色】用户（User Agent）\n\n\
         ⚠️ **Open ≠ 公开**：Open 是任务生命周期状态（待接单），不是可见性（公开/私有）。任务可见性由 visibility 字段决定（0=公开，1=私有），与 Open 状态无关。禁止在通知中把 Open 翻译为「公开」。\n\n\
         🛑 **本事件禁止调用的 CLI**：save-agreed / set-payment-mode / confirm-accept / apply / complete / reject——此时尚未选定服务商，协商未开始，这些命令全部非法。\n\n\
         【你的下一步动作（严格顺序）】\n\n\
         **Step 0 — 通知 user session + 在当前 sub/backup session 继续执行：**\n\
         调用 xmtp_dispatch_user 通知用户任务已上链（纯通知，不触发 LLM 思考）：\n\
         \x20\x20content: {created_notify}\n\n\
         ⚠️ 后续路由 → 协商/接单 全部在**当前 session** 中执行，不要转到 user session，不要 sessions_spawn。\n\n\
         {routing_section}\n\n"
    );

    if let Some(ref dp_id) = designated_provider {
        output.push_str("\n━━━━━━━━━ 以下 B-Step 仅在 D-Step 判定「无服务或无 endpoint」时执行 ━━━━━━━━━\n\
                         🛑 如果 D-Step 已路由到 x402（service-list 有 endpoint），则下方 B-Step **全部跳过，绝对禁止执行**。\n\
                         x402 完整路径：DX-Step 1→2→3 → A-Step 3（set-payment-mode）→ 等 job_payment_mode_changed → task-402-pay。\n\
                         x402 路径中**绝对不涉及** xmtp_start_conversation / 建群 / 三步握手 / xmtp_send 协商消息。\n\n");
        output.push_str(&designated_provider_negotiate(job_id, agent_id, short_id, dp_id));
    }

    output
}

pub(super) fn switch_provider(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let designated_provider = super::negotiate::take_designated_provider(job_id).ok().flatten();
    let dp_id = match &designated_provider {
        Some(id) => id.clone(),
        None => {
            return format!("【错误】switch_provider 缺少 --provider 参数。\n\
                 请重新调用：onchainos agent next-action --jobid {job_id} --jobStatus switch_provider --role buyer --agentId {agent_id} --provider <新服务商agentId>\n");
        }
    };

    let d_steps = designated_provider_d_steps(job_id, agent_id, short_id, &dp_id);
    let negotiate = designated_provider_negotiate(job_id, agent_id, short_id, &dp_id);
    format!("\
         【服务商变更】set-provider 已提交，立即启动新服务商流程（不等 task_provider_change 上链确认）\n\
         【角色】用户（User Agent） | 【执行环境】user session\n\n\
         🛑 **本事件禁止调用的 CLI**：save-agreed / set-payment-mode / confirm-accept / apply / complete / reject——此时尚未与新服务商协商，这些命令全部非法。\n\n\
         ⚠️ 旧服务商的 sub session 会在收到 `task_provider_change` 上链事件后自动发 [intent:reject]，无需你干预。\n\n\
         【你的下一步动作（严格顺序）】\n\n\
         {d_steps}\n\n\
         ━━━━━━━━━ 以下 B-Step 仅在 D-Step 判定「无服务或无 endpoint」时执行 ━━━━━━━━━\n\
         🛑 如果 D-Step 已路由到 x402（service-list 有 endpoint），则下方 B-Step **全部跳过，绝对禁止执行**。\n\
         x402 完整路径：DX-Step 1→2→3 → A-Step 3（set-payment-mode）→ 等 job_payment_mode_changed → task-402-pay。\n\
         x402 路径中**绝对不涉及** xmtp_start_conversation / 建群 / 三步握手 / xmtp_send 协商消息。\n\n\
         {negotiate}\n")
}

pub(super) fn provider_conversation(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;

    let no_sellers = super::content::no_more_sellers_user_notify(job_id);
    format!(
    "【触发】收到「有服务商待沟通」类消息（user session 侧）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **禁止自动建群**：收到 pending_list 通知后，**绝对不能**主动调用 xmtp_start_conversation。\n\
     必须先展示列表让用户自己选择服务商，用户明确指定后才能建群。\n\n\
     🛑 **CRITICAL — 本事件必须使用 `xmtp_prompt_user` 推送服务商列表到 user session，禁止在 sub session 中直接输出文字回复。**\n\
     ❌ 禁止用文字回复代替 xmtp_prompt_user 工具调用（sub session 输出用户看不到）\n\
     ❌ 禁止用 xmtp_dispatch_user 代替 xmtp_prompt_user（用户需要做服务商选择决策，dispatch_user 无法 relay）\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 0 — 幂等检查：查询是否已有此任务的待决事项：**\n\
     ```bash\n\
     onchainos agent pending-decisions list --format json --agent-id {agent_id}\n\
     ```\n\
     如果返回列表中已存在 jobId={job_id} 且 role=buyer 的条目 → **说明已经通知过用户,本次是重复事件,直接结束 turn,不再通知。**\n\
     如果不存在 → 继续 Step 1。\n\n\
     **Step 1 — 获取待沟通服务商列表：**\n\
     调用 xmtp_get_pending_list 工具获取待沟通服务商列表。\n\
     ⚠️ 调用前输出：`[buyer-xmtp] xmtp_get_pending_list`\n\
     ⚠️ 调用后输出：`[buyer-xmtp] xmtp_get_pending_list result: <返回值>`\n\n\
     如果返回空列表 → 调用 xmtp_dispatch_user 通知用户「当前没有待沟通的服务商」，结束。\n\n\
     **Step 2 — 调用 xmtp_prompt_user 展示所有待沟通服务商，让用户选择：**\n\
     🛑 **必须等用户选择**，不能替用户做决定。\n\
     先调 `session_status` 拿到本 sub session 的 sessionKey；调 `xmtp_prompt_user` **之前**先调 `pending-decisions add`(见硬规则 7)。\n\
     将 pending list 中**所有服务商**逐一列出，让用户挑选：\n\
     \x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <session_status 拿到的 sessionKey 整串>][job: {job_id}][role: buyer]\n\
     \x20\x20序号→AgentID 映射：<从 pending list 提取，格式如 1→798, 2→806, 3→866>\n\
     \x20\x20路由规则：\n\
     \x20\x20- 用户选服务商（数字序号或 AgentID）→ 按映射表转为 AgentID，调用 xmtp_dispatch_session(sessionKey=\"<session_status 拿到的 sessionKey 整串>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER index=<N> agentId=<映射后的AgentID>] 用户原话：<用户回复原文>\")\n\
     \x20\x20- 用户说「全部跳过/都不要/skip all/none」→ xmtp_dispatch_session(sessionKey=\"<同上>\", content=\"[USER_DECISION_RELAY][intent:SKIP_ALL_PROVIDERS] 用户原话：<用户回复原文>\")\n\
     \x20\x20⚠️ intent tag 必须完全大写 ASCII 原样塞入，禁止翻译/改写。relay 必须使用 xmtp_dispatch_session。禁止 user session agent 自己执行建群或 task CLI。{CONSTRAINT}\n\
     \x20\x20⚠️ 映射表必须从 pending list 逐行提取并内联到 llmContent——user session agent 看不到 userContent，没有映射表就无法把序号转为 AgentID。\n\
     \x20\x20userContent:\n\
     \x20\x20[任务 {short_id} 你作为用户] 有以下服务商主动联系你，请选择一个开始协商：\n\
     \x20\x20\n\
     \x20\x20[遍历 pending list 每个服务商，格式：]\n\
     \x20\x20<序号>. 服务商 AgentID：<agentId> | 名称：<name> | 信用分：<creditScore> | 完成任务数：<completedTaskCount>\n\
     \x20\x20\n\
     \x20\x20请回复服务商序号开始协商，或回复「全部跳过」。\n\n\
     **Step 3 — 收到 `[USER_DECISION_RELAY][intent:CODE] 用户原话：...` 后按 intent code 路由：**\n\n\
     ━━━━━━━━━ 分支 A：`[intent:PICK_PROVIDER index=<N> agentId=<X>]` → 建立 session 后协商 ━━━━━━━━━\n\n\
     A-Step 1：从 intent tag 里直接读 `agentId=<X>`，调 xmtp_start_conversation 工具建群 + 创建 sub session：\n\
     \x20\x20参数：myAgentId={agent_id}，toAgentId=<tag 里的 agentId>，jobId={job_id}\n\
     \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_start_conversation: myAgentId={agent_id}, toAgentId=<agentId>, jobId={job_id}`\n\
     \x20\x20⚠️ 调用后输出：`[buyer-xmtp] xmtp_start_conversation result: sessionKey=<返回值>, xmtpGroupId=<返回值>`\n\n\
     🛑 **建群后同一 turn 内必须调 `xmtp_send` 发首条消息**——建群只是创建通道，不发消息 = 服务商收不到任何信号 = 流程卡死。\n\
     ❌ 绝对禁止建群后结束 turn 不发消息\n\n\
     A-Step 2：建群后已进入 sub session，调用 xmtp_send 向服务商发起协商（参照 buyer.md 3.2 协商阶段三步确认）：\n\
     \x20\x20⚠️ **禁止**用 xmtp_dispatch_user / xmtp_dispatch_session，建群后统一用 xmtp_send。\n\
     \x20\x20content: 你好，我有一个任务（jobId: {job_id}）想请你来完成，请问你感兴趣吗？\n\n\
     A-Step 3：协商成功 → 服务商 apply 上链 → 等待服务商 XMTP 消息告知已 apply（buyer.md 路由 #2 触发 confirm-accept）\n\n\
     A-Step 4：协商失败（服务商拒绝 / 超时 / 条件不一致）→ 跳到 B 分支。\n\n\
     ━━━━━━━━━ 分支 B：用户拒绝当前服务商 / 协商失败 → 拒绝并回到列表 ━━━━━━━━━\n\n\
     B-Step 1：调用 xmtp_deny_pending_conversation 拒绝该服务商：\n\
     \x20\x20参数：agentId=<被拒绝服务商的 agentId>，jobId={job_id}\n\
     \x20\x20⚠️ 调用前输出：`[buyer-xmtp] xmtp_deny_pending_conversation: agentId=<agentId>, jobId={job_id}`\n\n\
     B-Step 2：重新调用 xmtp_get_pending_list 获取最新待沟通列表。\n\n\
     B-Step 3：如果列表不为空 → 回到 Step 2，展示剩余服务商让用户选择。\n\n\
     B-Step 4：如果列表为空 → 调用 xmtp_dispatch_user 通知用户：\n\
     \x20\x20content: {no_sellers}\n\n\
     【循环结束条件】xmtp_get_pending_list 返回空列表 或 协商成功进入场景 6。\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)

}

pub(super) fn job_visibility_changed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let visibility_public = super::content::visibility_public_user_notify(job_id, title_display);
    let visibility_private = super::content::visibility_private_user_notify(job_id, title_display);
    format!(
    "【当前状态】job_visibility_changed（公开/私有切换已上链）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **这不是辅助事件，必须通知用户。**\n\n\
     【你的下一步动作（严格顺序）】\n\n\
     {title_query_hint}\
     **Step 1 — 从系统通知 envelope 中读取 `visibility` 字段：**\n\
     - `visibility=0` → 公开（public）\n\
     - `visibility=1` → 私有（private）\n\n\
     **Step 2 — 调用 xmtp_dispatch_user 通知用户可见性已变更：**\n\
     content：\n\
     \x20\x20- visibility=0 → {visibility_public}\n\
     \x20\x20- visibility=1 → {visibility_private}\n\n\
     ⚠️ 切换为 public 后，**不要**请求推荐服务商列表（recommend），用户只需等待服务商主动找过来。\n\
     → **结束本轮 turn**。\n"
    )
}

pub(super) fn job_payment_mode_changed(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_display = ctx.title_display;
    let title_query_hint = ctx.title_query_hint;

    let payment_escrow_notify = super::content::payment_mode_escrow_user_notify(job_id, title_display);
    let x402_deliverable = super::content::x402_deliverable_user_notify(job_id);
    let x402_replay_fail = super::content::x402_replay_fail_payment_user_notify(job_id);
    format!(
    "【当前状态】job_payment_mode_changed（支付模式切换已上链）\n\
     【角色】用户（User Agent）\n\n\
     🛑 **必须通知用户支付模式变更结果。**\n\n\
     🛑 **本事件允许的动作白名单**：escrow 路径仅 xmtp_send [intent:confirm] + xmtp_dispatch_user 通知用户；x402 路径仅 x402-check + task-402-pay + xmtp_dispatch_user。\n\
     ❌ 禁止再调 set-payment-mode（paymentMode 已在链上就位，重复调用会导致状态污染）\n\
     ❌ 禁止调 save-agreed（已在 negotiate_ack 事件中完成）\n\
     ❌ 禁止调 apply（apply 是服务商动作，用户永远不执行）\n\
     ❌ 禁止调 confirm-accept（服务商尚未 apply，必须等服务商收到 CONFIRM 后 apply 再执行）\n\n\
     【你的下一步动作】\n\n\
     {title_query_hint}\
     **Step 1 — 从系统通知 envelope 中读取 `paymentMode` 字段：**\n\
     paymentMode 值映射：1=escrow, 3=x402。\n\
     ⚠️ 直接使用 envelope 中的 paymentMode，不需要额外查询 API。\n\n\
     ━━━━━━━━━ escrow（paymentMode=1）— 发 [intent:confirm] 触发服务商 apply ━━━━━━━━━\n\n\
     **Step 3 — 发 [intent:confirm]（服务商 apply 的唯一合法触发器）**：\n\
     链上 paymentMode 已就位，现在可以安全发 [intent:confirm] 让服务商 apply。\n\
     从你之前发的 [intent:propose] / 收到的 [intent:ack] **原样取所有字段**（paymentMode / tokenSymbol / tokenAmount）回看 sub session 历史复制即可：\n\n\
     调用 xmtp_send：\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <与 [intent:ack] 完全相同>\n\
     \x20\x20tokenAmount: <与 [intent:ack] 完全相同>\n\
     \x20\x20[intent:confirm]\n\n\
     ⚠️ **严禁**用自然语言「请你 apply / 请接单」绕过——服务商 flow.rs 把 `[intent:confirm]` 字面量当 apply 唯一触发器，自然语言指令**根本不会被识别**。\n\
     ⚠️ apply 是服务商动作，用户不执行 apply。\n\n\
     **Step 4 — 通知用户：**\n\
     调用 xmtp_dispatch_user：\n\
     \x20\x20content: {payment_escrow_notify}\n\n\
     → **结束本轮 turn**，等待服务商 XMTP 消息告知已 apply（buyer.md 路由优先级 #2 处理）。\n\n\
     ━━━━━━━━━ x402（paymentMode=3）━━━━━━━━━\n\n\
     从上一步 set-payment-mode / x402-check 的输出中提取 endpoint、acceptsJson、feeTokenSymbol、feeAmount、providerAgentId。\n\n\
     ⚠️ **参数丢失兜底**（上下文压缩可能导致上一 turn 输出丢失）：\n\
     如果上下文中缺少 providerAgentId 或 endpoint → 先调：\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取 `providerAgentId`；endpoint 从 `onchainos agent service-list --agent-id <providerAgentId>` 的 services[0].endpoint 获取。\n\n\
     如果上下文中缺少 acceptsJson / feeTokenSymbol / feeAmount → 用上面拿到的 endpoint 重新验证：\n\
     ```bash\n\
     onchainos agent x402-check --endpoint <endpoint> --agent-id {agent_id}\n\
     ```\n\
     提取 `acceptsJson`、`tokenSymbol`（= feeTokenSymbol）、`amountHuman`（= feeAmount）。\n\n\
     **x402 阶段 2 — 签名 + direct/accept + 重放 endpoint（原子命令）：**\n\
     ```bash\n\
     onchainos agent task-402-pay {job_id} --provider-agent-id <providerAgentId> --accepts '<acceptsJson>' --endpoint <endpoint URL> --token-symbol <feeTokenSymbol> --token-amount <feeAmount>\n\
     ```\n\
     内部执行：x402_pay 签名 → direct/accept 上链 → 组装 payment header → 重放 endpoint\n\
     输出：{{ replaySuccess, replayStatus, replayBody, signature, authorization, sessionCert, txHash }}\n\n\
     **x402 阶段 2 Step 3 — 检查重放结果并通知用户：**\n\
     - replaySuccess=true → 交付物在 replayBody 中。**立即**调用 xmtp_dispatch_user 将交付物发送给用户：\n\
     \x20\x20content:\n\
     {x402_deliverable}\n\n\
     - replaySuccess=false → 调用 xmtp_dispatch_user 通知用户重放失败：\n\
     \x20\x20content:\n\
     {x402_replay_fail}\n\n\
     → **结束本轮 turn**，等待 `job_accepted` 系统通知。\n\n\
     🛑🛑🛑 **收到 `job_accepted` 后的铁律（MANDATORY）**：\n\
     收到 `job_accepted` 系统事件后，**必须**调用：\n\
     ```bash\n\
     onchainos agent next-action --jobid {job_id} --jobStatus job_accepted --role buyer --agentId {agent_id}\n\
     ```\n\
     按返回的剧本执行（剧本会引导执行 `onchainos agent complete`）。\n\
     ❌ **绝对禁止**重跑本 turn 的 `x402-check` / `task-402-pay` / `xmtp_dispatch_user`——这些已在本 turn 完成，重跑会导致重复支付或重复通知。\n\
     ❌ **绝对禁止**跳过 `next-action` 自行决定下一步——`job_accepted` 剧本包含 `complete` 步骤，跳过 = 任务永久卡在 accepted 状态。\n"
    )
}

pub(super) fn negotiate_reply(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let short_id = ctx.short_id;
    let title_query_hint = ctx.title_query_hint;

    let over_budget = super::content::over_budget_user_prompt(short_id);
    format!(
    "【协商中继】negotiate_reply（服务商自然语言回复，无结构化标记）\n\
     【角色】用户（User Agent）\n\n\
     服务商在协商过程中发了自然语言消息（可能是报价、讨论细节、提问等）。你需要**自主评估并回复**。\n\n\
     🛑 **评估前置（强制）**：Step 1 和 Step 2 是强制步骤——必须完成后才能发送任何 xmtp_send（包括拒绝）。禁止跳过评估直接回复或拒绝。\n\n\
     {title_query_hint}\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 获取任务上下文（如本 turn 未查过则查一次）：**\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取关键字段：budget、paymentMostTokenAmount（max_budget）、tokenSymbol、description。\n\n\
     **Step 2 — 评估服务商回复内容：**\n\n\
     🛑 **铁律：回复服务商的任何消息中绝对禁止透露 max_budget（最高预算）数值**——泄露 = 服务商直接报上限价 = 用户丧失全部议价能力。\n\
     🚫 **协商自治红线**：除下方「报价 > max_budget」自动 REJECT 路径外，**禁止**调 `xmtp_prompt_user` / `pending-decisions add` 让用户做协商决策。协商由 sub session 自主完成——按决策矩阵评估后直接回复服务商（自然语言讨论 / [intent:propose]），不得把报价转发给用户问「是否接受」。\n\n\
     从服务商消息中提取报价信息（如有）：金额、币种、支付方式偏好、交付时间。\n\n\
     🔴 **报价评估决策矩阵**（如服务商给出了明确价格）：\n\
     \x20\x20| 服务商报价 | 动作 |\n\
     \x20\x20|---|---|\n\
     \x20\x20| ≤ budget | 价格可接受，确认其他条款后进入 [intent:propose] |\n\
     \x20\x20| budget < 报价 ≤ max_budget | 有谈判空间，自主还价 |\n\
     \x20\x20| > max_budget | **自动 REJECT + 切换**（见下方） |\n\n\
     **报价 > max_budget 的强制动作**：\n\
     \x20\x20a) xmtp_send 发 `[intent:reject]`：\n\
     \x20\x20\x20\x20content=\n\
     \x20\x20\x20\x20jobId: {job_id}\n\
     \x20\x20\x20\x20reason: 报价超出最高预算\n\
     \x20\x20\x20\x20[intent:reject]\n\
     \x20\x20b) `onchainos agent mark-failed {job_id} --provider <当前服务商agentId>`\n\
     \x20\x20c) 调 `session_status` 拿 sessionKey；调 `pending-decisions add`（见硬规则 7）；调 `xmtp_prompt_user` 让用户决定下一步：\n\
     \x20\x20\x20\x20llmContent: [USER_DECISION_REQUEST][sub_key: <sessionKey>][job: {job_id}][role: buyer] 用户语义「选 A / 查看推荐」→ 调用 xmtp_dispatch_session(sessionKey=\"<sessionKey>\", content=\"[USER_DECISION_RELAY][intent:VIEW_RECOMMEND] 用户原话：<用户回复原文>\") relay；用户语义「选 B / 指定服务商」并提供 agentId → 调用 xmtp_dispatch_session(sessionKey=\"<sessionKey>\", content=\"[USER_DECISION_RELAY][intent:PICK_PROVIDER agentId=<用户提供的agentId>] 用户原话：<用户回复原文>\") relay；用户语义「选 C / 关闭」→ 调用 xmtp_dispatch_session(sessionKey=\"<sessionKey>\", content=\"[USER_DECISION_RELAY][intent:CLOSE_TASK] 用户原话：<用户回复原文>\") relay。⚠️ 路由 tag 协议：intent 名完全大写 ASCII 原样塞入；禁止翻译/改写。⚠️ relay 必须使用 xmtp_dispatch_session。禁止 user session agent 自己执行 task CLI。{CONSTRAINT}\n\
     \x20\x20\x20\x20userContent:\n\
     {over_budget}\n\
     \x20\x20\x20\x20→ **结束本轮 turn**，等用户回复 relay 回来后：A → `recommend`；B → `next-action --provider <agentId>`；C → `close`。\n\n\
     **Step 3 — 回复服务商（取决于 Step 2 评估）：**\n\n\
     ▸ **服务商还在讨论阶段（未给出明确价格或在询问细节）** → xmtp_send 自然语言回复，继续讨论。\n\n\
     ▸ **双方就 tokenAmount / tokenSymbol / paymentMode 达成一致** → 发送 [intent:propose]：\n\
     \x20\x20📋 **填字段前必做自检**：逐字段回看 sub session 历史找**最后一次双方明确同意的值**。\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <USDT|USDG>\n\
     \x20\x20tokenAmount: <金额>\n\
     \x20\x20[intent:propose]\n\n\
     ⚠️ **A2A 协商会话中 paymentMode 固定 escrow**。\n\
     ⚠️ **禁止用自然语言替代 [intent:propose]**——服务商 Agent 只识别结构化标记，自然语言「请 apply / 条款已锁定」不会被解析。\n\
     ⚠️ **同 turn 只发一条 xmtp_send**。\n\
     🚫 🛑 **CRITICAL — 本事件绝对禁止调用 save-agreed / set-payment-mode / confirm-accept**——这些只在后续 negotiate_ack 事件中才能执行。服务商自然语言说「我接受」「同意」「OK」「没问题」**不是** `[intent:ack]`——只有 content 以字面量 `[intent:ack]` 方括号开头才算。在用户发出 [intent:propose] 之前，服务商不可能回 [intent:ack]。违反 = 跳过三步握手 = 任务永久卡死。\n\
     → **结束本轮 turn**，等待服务商回复。\n",
     CONSTRAINT = super::flow::PROMPT_USER_SESSION_CONSTRAINT)
}

pub(super) fn negotiate_ack(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_query_hint = ctx.title_query_hint;

    format!(
    "【协商中继】negotiate_ack（服务商接受 PROPOSE，回 [intent:ack]）\n\
     【角色】用户（User Agent）\n\n\
     服务商回复了 [intent:ack]——表示接受你的 [intent:propose] 条款。\n\n\
     {title_query_hint}\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 逐字段校验 ACK 与你发的 PROPOSE 一致：**\n\
     回看 sub session 历史，对比服务商 ACK 中的 paymentMode / tokenSymbol / tokenAmount 与你最近的 PROPOSE。\n\
     - **任一字段不一致** → 视为篡改，xmtp_send 告知服务商字段不一致并重发 [intent:propose]，结束 turn。\n\
     - **全部一致** → 继续 Step 2。\n\n\
     🛑 **本事件允许的 CLI 命令白名单**：save-agreed → set-payment-mode，**仅此两个、顺序固定**。\n\
     ❌ 禁止调 confirm-accept（服务商尚未 apply）\n\
     ❌ 禁止调 complete / reject（任务尚未进入执行阶段）\n\
     ❌ 禁止调 apply（apply 是服务商动作，用户永远不执行）\n\n\
     **Step 2 — save-agreed 落盘（🛑 不可跳过）：**\n\
     ```bash\n\
     onchainos agent save-agreed {job_id} --provider <当前协商的providerAgentId> --token-symbol <ACK中的tokenSymbol> --token-amount <ACK中的tokenAmount> --agent-id {agent_id}\n\
     ```\n\
     🛑 save-agreed **必须在 set-payment-mode 之前执行**——它将协商结果落盘，后续 confirm-accept 依赖此数据。跳过 save-agreed 直接调 set-payment-mode → confirm-accept 会使用错误参数。\n\n\
     **Step 3 — set-payment-mode（A2A 协商固定 escrow）：**\n\
     ⚠️ **不论链上 paymentType 当前是什么值，都必须执行**，不要查 common context 比较。\n\
     ```bash\n\
     onchainos agent set-payment-mode {job_id} --payment-mode escrow --token-symbol <ACK中的tokenSymbol> --token-amount <ACK中的tokenAmount>\n\
     ```\n\
     此命令返回 exit code 2 (confirming)。\n\n\
     🛑 **铁律：本 turn 绝对禁止 xmtp_send [intent:confirm]**——这是最常见的死锁触发器。\n\
     链上 paymentMode 还在 mempool，服务商见 CONFIRM 会立刻 apply，但 paymentMode 没确认，apply 会失败。\n\
     [intent:confirm] **只能**在收到 `job_payment_mode_changed` 系统事件后才能发送，没有任何例外。\n\n\
     → **结束本轮 turn**，等待 `job_payment_mode_changed` 系统通知。\n"
    )
}

pub(super) fn negotiate_counter(ctx: &FlowContext<'_>) -> String {
    let job_id = ctx.job_id;
    let agent_id = ctx.agent_id;
    let title_query_hint = ctx.title_query_hint;

    format!(
    "【协商中继】negotiate_counter（服务商发送反提案 [intent:counter]）\n\
     【角色】用户（User Agent）\n\n\
     服务商不接受你的 PROPOSE，发了 [intent:counter] 反提案。\n\n\
     🛑 **本事件禁止调用 save-agreed / set-payment-mode / confirm-accept / apply**——COUNTER 意味着条款未达成一致，只能发新 [intent:propose] 或 [intent:reject]。\n\
     🛑 **铁律：回复服务商的任何消息中绝对禁止透露 max_budget（最高预算）数值**——泄露 = 服务商直接报上限价 = 用户丧失全部议价能力。\n\n\
     {title_query_hint}\
     【你的下一步动作（严格顺序）】\n\n\
     **Step 1 — 轮次计数：**\n\
     回看 sub session 历史，统计服务商已发送的 `[intent:counter]` 总次数（含本次）。\n\
     🔢 **COUNTER 轮次上限 = 3 次**：\n\
     - 本次是第 3 次（含）以上 COUNTER → **不处理 COUNTER 内容**，直接 xmtp_send 发：\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20reason: 协商轮次超限，已达 3 次 COUNTER\n\
     \x20\x20[intent:reject]\n\
     \x20\x20然后 `onchainos agent mark-failed {job_id} --provider <当前服务商agentId>`，\n\
     \x20\x20调 xmtp_prompt_user 让用户决定下一步（同 negotiate_reply 超预算处理：A.查看推荐 / B.指定服务商 / C.关闭任务）。\n\
     \x20\x20→ **结束本轮 turn**，等用户 relay。\n\n\
     - 未超限 → 继续 Step 2。\n\n\
     **Step 2 — PROPOSE 笔误自检（优先级最高）：**\n\
     ⚠️ **先回看 sub session 历史，确认你上次发的 [intent:propose] 是否填错了**：\n\
     \x20\x20- COUNTER 金额 **等于** 自然语言里你最后同意的数 → **是你 PROPOSE 写错了**：直接用 COUNTER 值重发 [intent:propose]，不要再讨价还价。\n\
     \x20\x20- COUNTER 金额 **高于** 自然语言里你最后同意的数 → 才是服务商加价，继续 Step 3。\n\n\
     **Step 3 — 评估 COUNTER 条款：**\n\
     获取 max_budget：\n\
     ```bash\n\
     onchainos agent common context {job_id} --role buyer --agent-id {agent_id}\n\
     ```\n\
     提取 `paymentMostTokenAmount`。\n\n\
     \x20\x20| COUNTER 报价 | 动作 |\n\
     \x20\x20|---|---|\n\
     \x20\x20| ≤ budget | 可接受，用 COUNTER 值发新 [intent:propose] |\n\
     \x20\x20| budget < 报价 ≤ max_budget | 可接受或继续还价，发新 [intent:propose] |\n\
     \x20\x20| > max_budget | xmtp_send `[intent:reject]`，mark-failed，xmtp_prompt_user 让用户决定下一步（同 negotiate_reply 的超预算处理） |\n\n\
     - 检查 tokenSymbol 改动：服务商提出不同币种时评估是否可接受\n\
     - paymentMode 固定 escrow，不接受其他支付方式\n\n\
     **Step 4 — 发送新 [intent:propose]（如决定接受或还价）：**\n\
     \x20\x20content=\n\
     \x20\x20jobId: {job_id}\n\
     \x20\x20paymentMode: escrow\n\
     \x20\x20tokenSymbol: <USDT|USDG>\n\
     \x20\x20tokenAmount: <金额>\n\
     \x20\x20[intent:propose]\n\n\
     ⚠️ **禁止用自然语言替代 [intent:propose]**——服务商 Agent 只识别结构化标记。\n\
     → **结束本轮 turn**，等待服务商回复 [intent:ack] / [intent:counter] / [intent:reject]。\n"
    )
}
