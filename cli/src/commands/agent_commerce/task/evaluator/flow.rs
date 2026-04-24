//! Evaluator（仲裁者）端任务流程驱动器
//!
//! **所有事件都在 sub session 收到**（ws-channel 自动路由到 `conv-arb-*`）。
//! 事件命名对齐设计文档（Lark wiki `UumqwSyM5i1AuakBNLClJo9igIb`）。
//!
//! | 分类 | 事件 | 行为 |
//! |---|---|---|
//! | 仲裁自主闭环（不 notify_main） | evaluator_selected / reveal_started / dispute_resolved / round_failed | sub 里执行 CLI 动作 + 静默结束 |
//! | 资金/罚没（notify_main 推用户） | reward_claimed / slashed | sub 提取字段 + notify_main 推人话 |
//! | 质押 tx 回执（notify_main 推用户） | staked / stake_increased / unstake_requested / unstake_claimed / unstake_cancelled | sub 提取字段 + notify_main 推人话 |
//! | 自己的投票 tx 回执 | vote_committed（静默记录）/ vote_revealed（完全忽略） | 都不通知用户 |
//! | 其他方事件 | job_disputed | 完全忽略 |
//!
//! evaluator 在 `evaluator_selected`（VotersSelected 上链）时即介入——此刻 CommitPhase 已开，
//! 在 sub session 里**自主闭环**完成 "拉证据（含看图）→ 按 PRD §3.4/§3.5 判决 → 归约到 vote ∈ {{1,2}} → commit"。
//! 判决过程不通知用户；用户感知由后续 dispute_resolved → reward_claimed / slashed 负责。
//! PRD `Vz0jdNtugoZk9oxHr6mlQENjghg` L2 + §3.7：用户偏好会引入社会压力/贿赂风险，必须隔离。
//! 证据上传是链下操作（doc §7.8：No chain event for evidence），不再等"证据封期"信号。
//
// TODO(backend-config): 本文件生成的文案里包含多处硬编码经济参数（PRD Lark `Vz0jdNtugoZk9oxHr6mlQENjghg` 附录 A）：
//   - evaluator_selected arm: 超时罚 0.5%（PRD TIMEOUT_PENALTY_RATE）
//   - dispute_resolved arm:   少数方罚 1%（PRD MINORITY_PENALTY_RATE）/ 超时罚 0.5%
//   - staked arm:             首次最低 100 OKB、errorCode 1001
//   - unstake_requested arm:  7 天冷却期
// `/staking/config` 后端端点上线后，这些数字应改由注入的配置值替换，模板用
// `{slashTimeoutBps/100}%` / `{firstStakeMinOkb} OKB` / `{unstakeCooldownSeconds/86400} 天` 等。
// 参见 skills/okx-agent-task/evaluator.md §13 完整清单。

/// 根据 jobStatus 生成 evaluator 下一步动作的结构化提示词
pub fn generate_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> String {
    match job_status {
        // ─── 入口：本轮陪审选出，CommitPhase 已开（sub session 侧，agent 自主闭环） ──
        // 判决方法论严格对齐 PRD『A2A Evaluator Skill』(Lark docx Vz0jdNtugoZk9oxHr6mlQENjghg)
        // Module 1 L1-L5 + Module 3 §3.3/§3.4/§3.5/§3.6。V1 合约只接受 vote ∈ {1, 2}，
        // PRD 的 3 选项按 Step 4.5 归约表压到 1/2。结果不推给用户（不 notify_main）。
        "evaluator_selected" => format!(
            "【当前状态】evaluator_selected（VotersSelected 上链，你是本轮陪审，CommitPhase 已开，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户，**结果不通知用户**。评估证据 → 直接 commit → 结束。\n\
             【判决权威】PRD『A2A Evaluator Skill』(Lark `Vz0jdNtugoZk9oxHr6mlQENjghg`) Module 1 L1-L5 + §3.3/§3.4/§3.5/§3.6。冲突以 PRD 为准。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从入站消息提取 `disputeId` 和 `disputeType`（质量/超时/恶意）。**\n\
             ⚠️ `disputeId` 缺省时直接中止本轮处理，输出 `missing disputeId in payload; abort` 日志结束——真后端 `disputeId = keccak256(jobId, roundNumber)`，第 2+ 轮重选时 `d-{job_id}-r1` 一定对不上合约。\n\
             `disputeType` 缺省时按质量争议处理（最常见）。\n\n\
             **Step 2 — 拉取当前证据：**\n\
             ```bash\n\
             onchainos agent evaluator info <disputeId>\n\
             ```\n\
             返回真后端结构 `evidences: {{ provider: {{texts[], images[]}}, client: {{texts[], images[]}} }}`。\n\
             每张 `images[].fileKey` 已由 CLI 下载到本地，`localPath` 是绝对路径。\n\n\
             **⚠️ Step 2.5 — 必须实际打开每张图片阅读（最重要，禁止跳过）：**\n\
             - 遍历 `evidences.provider.images[].localPath` 和 `evidences.client.images[].localPath`\n\
             - **逐张调用多模态 read / view 能力读图**——截图里写了什么、展示了什么交付物、时间戳、对话内容，全要实际看过\n\
             - **禁止**只凭 `texts[]` 或 fileKey 名称猜测图片内容；不看图 = 放弃双方可能最关键的证据 = 违反 PRD L3 义务 #1『必须完整阅读双方提交的所有材料』\n\
             - 下载失败（有 `downloadError`）的图片记为缺失，按 PRD §2.5『一方未提交视为放弃举证』处理\n\n\
             **Step 3 — 按 PRD §3.2 材料读取流程构建证据清单：**\n\
             - ① 完整性：双方各提交了什么文本/图片？缺失什么？\n\
             - ② 任务基线：从 qualityStandards / description 建立\"任务应该是什么样\"\n\
             - ③ 分歧点：对比 clientReason / providerReason 标记双方说法不同的地方\n\
             - ④ 证据关联：每个分歧点对应哪些证据（文本 + 图片），按 PRD §3.3 打等级 S/A/B/C/D\n\
             - ⑤ 链上验证：若证据引用链上记录，做交叉验证（S/A 级直接采信；C/D 级需对方承认或交叉佐证）\n\n\
             **Step 4 — 按 `disputeType` 选对应 Rubric 打分（PRD §3.5），再按 §3.4 决策原则（优先级从高到低：证据为王 > 规格至上 > 举证责任 > 比例原则 > 模糊不利于起草方 > 沟通义务 > 善意推定 > 时间戳权威）收敛到 PRD 原生选项：**\n\
             \n\
             | disputeType | Rubric 权重（满分 100） | PRD 原生选项 |\n\
             |---|---|---|\n\
             | 质量争议 | 规格匹配 40 + 验收达标 30 + 功能正确 20 + 专业标准 10 | 完成 / 部分完成 / 未完成 |\n\
             | 超时争议 | 时间线 35 + 沟通响应 25 + 阻塞依赖 25 + 外部因素 15 | 责任在 Client / 责任在 Provider / 不可抗力 |\n\
             | 恶意行为 | 行为性质 + 证据强度 + 行为模式 + 损害程度（汉隆剃刀：先排除能力不足） | 成立 / 不成立 |\n\
             \n\
             **Step 4.5 — 归约到 V1 合约的 vote ∈ {{1, 2}}（V1 二元投票强制约束，PRD 3 选项不能直接上链）：**\n\
             \n\
             | disputeType | PRD 原生选项 | vote | 语义 |\n\
             |---|---|---|---|\n\
             | 质量 | 完成（总分 ≥ 80） | **1** | Provider 胜，资金全额释放 |\n\
             | 质量 | 部分完成（40-79）/ 未完成（< 40） | **2** | Client 胜，资金退回——V1 无部分结算通道；按 §3.4 原则 #3『举证责任』质量争议由 Client 证明未完成 |\n\
             | 超时 | 责任在 Client / 不可抗力 | **1** | Provider 不背锅 |\n\
             | 超时 | 责任在 Provider | **2** | Provider 超时违约 |\n\
             | 恶意 | 不成立 | **1** | 被举报方无责 |\n\
             | 恶意 | 成立 | **2** | 被举报方违约 |\n\
             \n\
             ⚠️ 归约规则是硬约束——不得为了\"平衡\"或\"避免争议\"反向归约。PRD §3.4 原则优先于对结果的担忧。\n\n\
             **Step 5 — 写裁决书（PRD §3.6 + L3 义务 #4『必须在投票前写下完整推理链』）：**\n\
             \n\
             在 session 记忆里生成结构化文本（不入链、不推用户，但必须实际产出；用于 L4 递归自检）：\n\
             ```\n\
             争议 ID: <disputeId>\n\
             争议类型: <质量/超时/恶意>\n\
             Rubric 打分: <规格 X/40 + 验收 Y/30 + 功能 Z/20 + 专业 W/10 = 总分 N/100>\n\
             PRD 原生选项: <完成 | 部分完成 | 未完成 | ...>\n\
             V1 vote: <1 | 2>\n\
             事实认定:\n\
             \x20\x201. <基于证据认定的事实>\n\
             \x20\x202. <...>\n\
             证据引用（必须包含图片内容，不能只引用 texts[]）:\n\
             \x20\x20- 事实 1 ← provider/client 的 <图片 localPath 或 texts[i]> (Level <S|A|B|C|D>)\n\
             \x20\x20- ...\n\
             推理（引用 PRD §3.4 原则编号）:\n\
             \x20\x20按原则 #<N>，<推理过程>\n\
             归约: PRD 原生『<...>』→ V1 vote=<1|2>，依据 Step 4.5 归约表\n\
             ```\n\n\
             **Step 6 — L4 递归自检（PRD Module 1）：出发 commit 前逐项确认，任一未通过回 Step 4 重审：**\n\
             - □ 我是否完整阅读了双方全部材料（含每张图片）？\n\
             - □ 结论是否由证据推导出来的（而非先有结论再找证据）？\n\
             - □ 把 Client 和 Provider 角色互换，我会做出同样裁决吗？\n\
             - □ 我是否受到了材料包外的信息影响？\n\
             - □ 我是否在猜测其他 Evaluator 怎么投？\n\n\
             **Step 7 — 执行 commit：**\n\
             ```bash\n\
             onchainos agent evaluator commit <disputeId> --side <1|2>\n\
             ```\n\
             ⚠️ **只能是 1 或 2，禁止 skip**（V1 无弃权；拖到超时罚 0.5% 比错投 1% 更亏）。\n\
             失败最多重试 3 次（CRITICAL，commit 窗口关闭即罚 0.5%）。返回 `voter has already committed` 视为成功进入 Step 8。\n\
             body 只带 `vote`；裁决书（Step 5）仅保留在 session 记忆，**不写入后端，不推主 session**。\n\n\
             **Step 8 — 输出一行 sub session 日志后结束本回合。不调用 notify_main，不通知用户：**\n\n\
             > Committed dispute=<disputeId> side=<1|2> autonomously per PRD §3.4-§3.6.\n\n\
             【原则】\n\
             - **完全静默**：本 arm 不 escalate_to_main、不 notify_main；用户只会在后续结算/罚没/奖励事件被通知\n\
             - **判决权威**：所有打分规则、决策原则、裁决书格式以 PRD `Vz0jdNtugoZk9oxHr6mlQENjghg` 为准\n\
             - **图片必读**：不读图即违反 L3 义务 #1 + §3.1 举证对称；这是本 arm 最重要的执行要求\n\n\
             【后续事件】\n\
             - vote_committed → sub 里仅记录\n\
             - reveal_started → sub 里自动 reveal\n\
             - dispute_resolved → sub 里自动 claim（若赢）+ forget + notify_main\n\
             - round_failed → sub 里 forget + notify_main\n"
        ),

        // ─── reveal 窗口开启（sub session，完全静默） ──────────────────
        "reveal_started" =>
            "【当前状态】reveal_started（RevealStarted 上链，reveal 窗口开启，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。**agent 自主 reveal，不通知用户**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `disputeId`，执行 reveal：**\n\
             ```bash\n\
             onchainos agent evaluator reveal <disputeId>\n\
             ```\n\
             ⚠️ 不要传 `--side`：CLI 会从 `~/.onchainos/evaluator-commits.jsonl`（commit 时自动写入）查出当时的 side，再发给后端。\n\
             \x20只有当你明确知道 commit 时投的 side 且 local store 被清空时，才显式传 `--side <1|2>` 覆盖。\n\
             \x20传错会让链上 commitHash 验签失败 → 合约 revert。\n\n\
             **Step 2 — 输出一行 sub session 日志后结束。禁止调用 notify_main：**\n\n\
             > Revealed dispute=<disputeId> side=<1|2> autonomously.\n\n\
             【错误映射】\n\
             - `canReveal=false` → CLI 已预检拒绝，无需重试；等下一个事件（若本轮已结算，会收到 dispute_resolved / round_failed）\n\
             - `already resolved` → 视为成功（本轮已裁决）\n\
             - `voter has not committed` → 本轮未 commit，跳过 reveal 是正常的\n\
             - 其他失败最多重试 3 次（未 reveal 罚 0.5%，PRD 附录 A TIMEOUT_PENALTY_RATE）\n\n\
             【后续事件】dispute_resolved / round_failed / reward_claimed / slashed 会继续在同一 sub session 到达。仅 reward_claimed 和 slashed 会转发到主 session。\n"
                .to_string(),

        // ─── 结算完成（sub 静默处理；入账/罚没通过后续 reward_claimed / slashed 事件再推主 session） ─
        "dispute_resolved" =>
            "【当前状态】dispute_resolved（DisputeSettled 上链，仲裁结算完成，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。**agent 自主 claim + 清理，不通知用户**。用户侧的入账/罚没通知由后续 reward_claimed / slashed arm 负责。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `winningSide` / `yourVote`。**\n\n\
             **Step 2 — 若 `yourVote` 与 `winningSide` 一致（多数方），立即领取奖励：**\n\
             ```bash\n\
             onchainos agent evaluator claim\n\
             ```\n\
             ⚠️ 无参命令：account 级 pull 模式，一次把所有已结算 dispute 的待领奖励一起领出来（后端 `POST /task/claim`，空 body）。\n\
             失败最多重试 3 次。真正的入账确认会通过稍后到达的 `reward_claimed` 事件告知用户（那个 arm 会 notify_main）。\n\
             若 `yourVote` 与多数不一致 / 为空，跳过 claim（不会有奖励，可能会收到 slashed 事件）。\n\n\
             **Step 3 — 清理本地 commit 存档（dispute 已终结，{vote, salt} 不再需要）：**\n\
             ```bash\n\
             onchainos agent evaluator forget <disputeId>\n\
             ```\n\
             幂等——若无记录也只会报 \"already clean\"，不会失败。\n\n\
             **Step 4 — 输出一行 sub session 日志后结束。禁止调用 notify_main：**\n\n\
             > Settled dispute=<disputeId> winningSide=<1|2> yourVote=<1|2> claim_submitted={true|false} store_cleaned.\n\n\
             【后续事件】\n\
             - reward_claimed（claim tx 回执）→ 另一个 arm，会 notify_main 推入账/失败给用户\n\
             - slashed（被罚通知）→ 另一个 arm，会 notify_main 推罚没金额+原因给用户\n\
             本 arm 到这里结束，**不抢这两个 arm 的通知职责**。\n"
                .to_string(),

        // ─── 本轮失效（sub 静默处理；若被罚会通过 slashed arm 再推主 session） ──
        "round_failed" =>
            "【当前状态】round_failed（DisputeInvalidated 上链，本轮无效：票数不足 / 无人揭示 / 全员弃票）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。**agent 自主清理，不通知用户**。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `disputeId`，清理本地 commit 存档（本轮作废，salt 不再可用）：**\n\
             ```bash\n\
             onchainos agent evaluator forget <disputeId>\n\
             ```\n\
             幂等——若无记录也只会报 \"already clean\"，不会失败。\n\n\
             **Step 2 — 输出一行 sub session 日志后结束。禁止调用 notify_main：**\n\n\
             > round_failed disputeId=<disputeId> store cleaned; awaiting next round.\n\n\
             【后续事件】\n\
             - 若被罚（未 commit / 未 reveal / 弃票）→ slashed arm 会 notify_main 告知用户\n\
             - 若再次被选中 → evaluator_selected 会在新 disputeId 上到达（roundNumber++）\n\
             否则本流程对你终止。\n"
                .to_string(),

        // ─── 被罚没（VoterStaking.Slashed） ─────────────────────────────
        "slashed" => format!(
            "【当前状态】slashed（VoterStaking.Slashed 上链，你的 stake 被罚没，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 无用户。此事件被动触发，无需额外链上操作。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `amount`（被罚金额）、`reason`（commit/reveal 超时 / 少数方 / 弃权）、`disputeId`。**\n\n\
             **Step 2 — 拉任务上下文为通知加标题：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\n\
             **Step 3 — 调 `notify_main`：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[Stake 罚没] 任务『<title>』(jobId={job_id})\n\
             \x20\x20\x20\x20  - 金额：<amount> OKB\n\
             \x20\x20\x20\x20  - 原因：<reason>\n\
             \x20\x20\x20\x20  - disputeId：<disputeId>\n\
             ```\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > Slashed amount=<amount> reason=<reason> relayed.\n"
        ),

        // ─── 奖励到账（claimRewards tx 上链结果） ──────────────────────
        "reward_claimed" => format!(
            "【当前状态】reward_claimed（claimRewards tx 上链完成，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `status`（success / failed）、`txHash`、`rewardAmount`、`errorCode`（若 failed）。**\n\n\
             **Step 2 — 拉任务上下文为通知加标题：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\n\
             **Step 3 — 调 `notify_main`：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20<按 status 二选一：>\n\
             \x20\x20\x20\x20  - success → [仲裁奖励] 任务『<title>』(jobId={job_id}) 奖励已到账 <rewardAmount> OKB，txHash=<txHash>。\n\
             \x20\x20\x20\x20  - failed  → [仲裁奖励失败] 任务『<title>』(jobId={job_id}) claim 失败 (errorCode=<errorCode>, txHash=<txHash>)，请按错误码重试。\n\
             ```\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > reward_claimed status=<status> amount=<rewardAmount> relayed.\n\n\
             【流程结束】此 disputeId 的 evaluator 生命周期完成；后续事件无需响应。\n"
        ),

        // ─── 质押生命周期：sub 收到 → notify_main 推人话给用户 ─────────────────
        "staked" => "【当前状态】staked（VoterStaking.Staked 上链，首次质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 从 payload 提取字段 → 调 `notify_main` 推人话给用户。禁止 sessions_send / 直接输出。\n\n\
             【Step 1】从 payload 提取 `status`（success / failed）、`amount`、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】调 `notify_main`：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20<按 status 二选一：>\n\
             \x20\x20\x20\x20  - success → [质押] 质押已生效：+<amount> OKB，txHash=<txHash>。你现在是活跃仲裁者候选，被选入陪审时会收到通知。\n\
             \x20\x20\x20\x20  - failed  → [质押失败] errorCode=<errorCode>, txHash=<txHash>。常见错误：4000 agentId 无效 / 2004 无 evaluator 身份 / 1001 累计质押 < 100 OKB（PRD 3.1.1，合约按 `已有余额 + 本次 >= 100` 校验）。请按错误码修正后重试 `onchainos agent evaluator stake --amount <N>`。\n\
             ```\n\n\
             【Step 3】输出日志结束：`> staked status=<status> amount=<amount> relayed.`\n".to_string(),

        "stake_increased" => "【当前状态】stake_increased（VoterStaking.IncreaseStake 上链，补充质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 调 `notify_main` 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `status`、`amount`、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】调 `notify_main`：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20<按 status 二选一：>\n\
             \x20\x20\x20\x20  - success → [质押] 追加质押已入账：+<amount> OKB，txHash=<txHash>。你的选中权重相应提升。\n\
             \x20\x20\x20\x20  - failed  → [质押失败] 追加质押失败（errorCode=<errorCode>, txHash=<txHash>），请按错误码重试 `onchainos agent evaluator increase-stake --amount <N>`。\n\
             ```\n\n\
             【Step 3】输出日志结束：`> stake_increased status=<status> amount=<amount> relayed.`\n".to_string(),

        "unstake_requested" => "【当前状态】unstake_requested（VoterStaking.UnstakeRequested 上链，申请解质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 调 `notify_main` 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `status`、`amount`、`availableAt`（冷却结束毫秒时间戳）、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】调 `notify_main`（availableAt 转本地时间后展示）：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20<按 status 二选一：>\n\
             \x20\x20\x20\x20  - success → [解质押] 申请已受理：-<amount> OKB 进入 7 天冷却期，可领取时间 <availableAt 本地时间>。到点跑 `onchainos agent evaluator claim-unstake` 提走；想撤销跑 `onchainos agent evaluator cancel-unstake`（仅冷却期内有效）。\n\
             \x20\x20\x20\x20  - failed  → [解质押失败] errorCode=<errorCode>, txHash=<txHash>。常见原因：活跃仲裁期间不可解质押 / 已在冷却期 / 余额不足。\n\
             ```\n\n\
             【Step 3】输出日志结束：`> unstake_requested status=<status> amount=<amount> relayed.`\n".to_string(),

        "unstake_claimed" => "【当前状态】unstake_claimed（VoterStaking.UnstakeClaimed 上链，领取解质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 调 `notify_main` 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `status`、`amount`、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】调 `notify_main`：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20<按 status 二选一：>\n\
             \x20\x20\x20\x20  - success → [解质押] 已提走 <amount> OKB，已入钱包，txHash=<txHash>。\n\
             \x20\x20\x20\x20  - failed  → [解质押失败] 领取失败（errorCode=<errorCode>, txHash=<txHash>），请按错误码重试。常见原因：锁定期未满 / 无待解质押。\n\
             ```\n\n\
             【Step 3】输出日志结束：`> unstake_claimed status=<status> amount=<amount> relayed.`\n".to_string(),

        "unstake_cancelled" => "【当前状态】unstake_cancelled（VoterStaking.UnstakeCancelled 上链，取消解质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 调 `notify_main` 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `status`、`amount`、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】调 `notify_main`：\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20<按 status 二选一：>\n\
             \x20\x20\x20\x20  - success → [解质押] 已取消：<amount> OKB 回到质押状态，txHash=<txHash>。\n\
             \x20\x20\x20\x20  - failed  → [解质押失败] 取消失败（errorCode=<errorCode>, txHash=<txHash>）。常见原因：冷却期已过 / 无待解质押。\n\
             ```\n\n\
             【Step 3】输出日志结束：`> unstake_cancelled status=<status> amount=<amount> relayed.`\n".to_string(),

        // ─── 自己的投票 tx 回执 ──────────────────────────────────────────
        "vote_committed" => "【当前状态】vote_committed（你自己的 commit tx 上链 success，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 无用户。这是**确认通知**，不是动作触发点。\n\n\
             【动作】仅记录 tx 成功状态；禁止重复 commit（后端会返回 `voter has already committed`）。**不调用 notify_main，不通知用户**——commit 是 agent 内部决策过程，用户感知由后续 dispute_resolved → reward_claimed / slashed 负责。\n\n\
             【输出】一行日志后结束：`> vote_committed recorded (silent).`\n\n\
             【后续事件】等 `reveal_started`（开启 reveal 窗口）→ sub 里跑 `evaluator reveal`。\n".to_string(),

        "vote_revealed" => "【当前状态】vote_revealed（你自己的 reveal tx 上链 success，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — **完全忽略**，不记录不通知。\n\n\
             【动作】无——输出一行日志 `> vote_revealed ignored.` 后结束。禁止 notify_main。reveal 成功的用户感知由后续 dispute_resolved → reward_claimed / slashed 负责。\n\n\
             【后续事件】等 `dispute_resolved` / `round_failed`（结算/失效）→ sub 里跑对应 arm。\n".to_string(),

        // ─── 兜底（含 job_disputed 等对 evaluator 无意义的事件） ────────
        other => format!(
            "【未知或无需动作的状态】{other}\n\
             【说明（所有事件都在 sub session 收到）】\n\
             - 仲裁自主闭环（sub 处理，不 notify_main）：evaluator_selected / reveal_started / dispute_resolved / round_failed\n\
             - 资金/罚没相关（sub 处理 + notify_main 推用户）：reward_claimed / slashed\n\
             - 质押 tx 回执（sub 处理 + notify_main 推用户）：staked / stake_increased / unstake_requested / unstake_claimed / unstake_cancelled\n\
             - 自己的 tx 回执：vote_committed（静默记录）/ vote_revealed（完全忽略）\n\
             - 其他方的事件（完全忽略）：job_disputed\n\n\
             【若确有异常】\n\
             1. 调用 `onchainos agent common context {job_id} --role evaluator` 查看上下文\n\
             2. 不要预测/假设其他通知\n"
        ),
    }
}
