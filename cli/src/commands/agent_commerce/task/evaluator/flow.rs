//! Evaluator（仲裁者）端任务流程驱动器
//!
//! 事件命名对齐设计文档（Lark wiki `UumqwSyM5i1AuakBNLClJo9igIb`）的 event 枚举：
//! evaluator_selected / reveal_started / dispute_resolved / round_failed / slashed / reward_claimed。
//! evaluator 在 `evaluator_selected`（VotersSelected 上链）时即介入——此刻 CommitPhase 已开，
//! 一次性完成"拉证据 → 分析 → escalate_to_main → 用户回 1/2/skip → 立即 commit"。
//! 证据上传是链下操作（doc §7.8：No chain event for evidence），不再等"证据封期"信号。
//! job_disputed / vote_committed / vote_revealed 对 evaluator 不是动作触发点。

/// 根据 jobStatus 生成 evaluator 下一步动作的结构化提示词
pub fn generate_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> String {
    match job_status {
        // ─── 入口：本轮陪审选出，CommitPhase 已开（sub session 侧） ────────
        "evaluator_selected" => format!(
            "【当前状态】evaluator_selected（VotersSelected 上链，你是本轮陪审，CommitPhase 已开，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。禁止输出提示/询问；全部步骤静默完成后调 `escalate_to_main` 结束回合。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从入站消息提取 `disputeId`（字段名 `disputeId`；缺省用 `d-{job_id}-r1`）。**\n\n\
             **Step 2 — 拉取当前证据（证据上传是链下操作，随时可追加；你只看当下可见的版本）：**\n\
             ```bash\n\
             onchainos agent evaluator info <disputeId>\n\
             ```\n\
             返回 qualityStandards / clientReason / providerReason / deliverableUrl / evidences[]。\n\
             evidences[] 中 kind=image 的条目 CLI 已把图片下载到本地 `localPath`（多模态 session 可直接读取）。\n\n\
             **Step 3 — 对照 Evidence Credibility Levels（S>A>B>C>D）评估双方证据，按每条 qualityStandards 打分。产出三项：**\n\
             - `recommendedSide`：1 = Provider 胜 / 2 = Client 胜\n\
             - `rationale`：具体指向哪条标准 + 哪级证据（不可泛泛而谈）\n\
             - `alternativeReading`：翻盘条件；若无，写 `none`\n\n\
             **Step 4 — 调用工具名为 `escalate_to_main` 的自定义工具（⚠️ 禁止使用 `sessions_send` / `xmtp_send` / 任何其他消息工具）：**\n\n\
             ```\n\
             tool: escalate_to_main     ← 必须是这个名字\n\
             arguments:\n\
             \x20\x20topic: \"dispute\"\n\
             \x20\x20context: {{ \"disputeId\": \"<disputeId>\", \"jobId\": \"{job_id}\" }}\n\
             \x20\x20userMessage: |\n\
             \x20\x20\x20\x20[仲裁决策请求] dispute <disputeId> (任务 {job_id})\n\
             \x20\x20\x20\x20建议投: side <1|2>（Provider wins | Client wins）\n\
             \x20\x20\x20\x20理由: <一句话 rationale，含标准 + 证据等级>\n\
             \x20\x20\x20\x20证据:\n\
             \x20\x20\x20\x20  - Client (Level <S|A|B|C|D>): <one-line>\n\
             \x20\x20\x20\x20  - Provider (Level <S|A|B|C|D>): <one-line>\n\
             \x20\x20\x20\x20请回复：\n\
             \x20\x20\x20\x20  1       投 Provider 胜\n\
             \x20\x20\x20\x20  2       投 Client 胜\n\
             \x20\x20\x20\x20  skip    弃权（超时罚 0.5% 质押）\n\
             \x20\x20agentInstructions: |\n\
             \x20\x20\x20\x20You are the Evaluator agent on disputeId=<disputeId> jobId={job_id}.\n\
             \x20\x20\x20\x20recommended side=<1|2>  reason=<Step 3 rationale verbatim>\n\
             \x20\x20\x20\x20CommitPhase is OPEN NOW — commit immediately once user decides. Deadline: 18h from selection.\n\
             \x20\x20\x20\x20- User reply 1/2 → capture `side` into main-session memory (REQUIRED for reveal), then run ONCE:\n\
             \x20\x20\x20\x20    onchainos agent evaluator commit <disputeId> --side <1|2>\n\
             \x20\x20\x20\x20  (body 只带 vote；agent rationale 不写入后端，仅保留在 session 记忆里)\n\
             \x20\x20\x20\x20  On success: tell user `已承诺 (committed)，disputeId=<id>，等待 reveal 窗口。`\n\
             \x20\x20\x20\x20  On `voter has already committed`: `本轮已承诺过，跳过重复 commit。`\n\
             \x20\x20\x20\x20- User reply skip/abstain/弃权 → do NOT commit; reply `已跳过投票。Commit/Reveal 超时会罚 0.5% 质押。`\n\
             \x20\x20\x20\x20- User question → fetch silently:\n\
             \x20\x20\x20\x20    任务详情/验收标准 → onchainos agent status {job_id}\n\
             \x20\x20\x20\x20    证据细节         → onchainos agent evaluator info <disputeId>\n\
             \x20\x20\x20\x20  Translate to natural Chinese; end with `想好怎么投了请回复 1 / 2 / skip。`\n\
             \x20\x20\x20\x20- On reveal_started / dispute_resolved / round_failed / slashed / reward_claimed arrival → call\n\
             \x20\x20\x20\x20    onchainos agent next-action --jobid {job_id} --jobStatus <type> --agentId <me> --role evaluator\n\
             \x20\x20\x20\x20  and follow its output.\n\
             \x20\x20\x20\x20- Never expose raw CLI to user; never act on a different disputeId.\n\
             ```\n\n\
             **Step 5 — 输出一行 sub session 日志后结束本回合：**\n\n\
             > Escalated dispute=<disputeId> to main session.\n\n\
             【严禁】sub session 内直接跑 `evaluator commit` / `evaluator reveal`；commit 在主 session 的对话闭环里跑。\n\n\
             【后续事件】\n\
             - SUB_DECISION_REQUEST topic=dispute → 主 session 与用户对话 → 立即 commit\n\
             - reveal_started → 主 session 再调 next-action 执行 reveal\n\
             - round_failed → 本轮无效，等下一轮 evaluator_selected\n"
        ),

        // ─── reveal 窗口开启（sub session） ───────────────────────────────
        "reveal_started" => format!(
            "【当前状态】reveal_started（RevealStarted 上链，reveal 窗口开启，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。拉 context / 跑 CLI 都在 sub 里完成，\n\
             \x20最后用 `notify_main` 把结果推一条干净文案到主 session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文（为通知加任务标题）：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\
             提取 `title`。\n\n\
             **Step 2 — 从 payload 提取 `disputeId`，执行 reveal：**\n\
             ```bash\n\
             onchainos agent evaluator reveal <disputeId>\n\
             ```\n\
             ⚠️ 不要传 `--side`：CLI 会从 `~/.onchainos/evaluator-commits.jsonl`（commit 时自动写入）查出当时的 side，再发给后端。\n\
             \x20只有当你明确知道 commit 时投的 side 且 local store 被清空时，才显式传 `--side <1|2>` 覆盖。\n\
             \x20传错会让链上 commitHash 验签失败 → 合约 revert。\n\n\
             **Step 3 — 调用 `notify_main` 工具（⚠️ 禁止 `sessions_send` / 直接输出给用户）：**\n\n\
             ```\n\
             tool: notify_main     ← 必须是这个名字\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[仲裁进展] 任务『<title>』(jobId={job_id})\n\
             \x20\x20\x20\x20<按结果二选一：>\n\
             \x20\x20\x20\x20  - 成功 → 已披露 (side=<1|2>)，等待最终裁决。\n\
             \x20\x20\x20\x20  - already resolved → 仲裁已被裁决，无需重复 reveal。\n\
             \x20\x20\x20\x20  - voter has not committed → 本轮未 commit，跳过 reveal（skip 场景）。\n\
             ```\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > Revealed dispute=<disputeId> side=<1|2>.\n\n\
             【错误映射】其他 reveal 失败最多重试 3 次（未 reveal 罚 0.3%）。\n\n\
             【后续事件】dispute_resolved / round_failed / reward_claimed 会继续在同一 sub session 到达。\n"
        ),

        // ─── 结算完成（合并原 TASK_RESOLVED + REWARD_CLAIMABLE：claim 并入此分支） ─
        "dispute_resolved" => format!(
            "【当前状态】dispute_resolved（DisputeSettled 上链，仲裁结算完成，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。拉 context + claim 奖励都在 sub 里完成，\n\
             \x20最后 `notify_main` 推结构化通知到主 session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文（为通知加标题 + 原争议对照）：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\
             提取 `title`、`clientReason` / `providerReason`（如有）。\n\n\
             **Step 2 — 从 payload 提取 `winningSide` / `settlement` / `yourVote`。**\n\n\
             **Step 3 — 若 `yourVote` 与 `winningSide` 一致（多数方），立即领取奖励：**\n\
             ```bash\n\
             onchainos agent evaluator claim {job_id}\n\
             ```\n\
             失败最多重试 3 次。真正的入账确认会通过稍后到达的 `reward_claimed` 事件告知。\n\
             若 `yourVote=skip` 或与多数不一致，跳过 claim。\n\n\
             **Step 4 — 清理本地 commit 存档（dispute 已终结，{{vote, salt}} 不再需要）：**\n\
             ```bash\n\
             onchainos agent evaluator forget <disputeId>\n\
             ```\n\
             幂等——若无记录也只会报 \"already clean\"，不会失败。\n\n\
             **Step 5 — 调用 `notify_main` 工具：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[仲裁结算] 任务『<title>』(jobId={job_id}) 仲裁结算完成：\n\
             \x20\x20\x20\x20- 裁决结果：<Provider 胜诉 | Client 胜诉>（winningSide=<1|2>）\n\
             \x20\x20\x20\x20- 资金处理：<资金已释放给 Provider | 资金已退还 Client>\n\
             \x20\x20\x20\x20- 您本轮投票：side=<1|2|skip>，<与多数一致（已提交 claim，等待 reward_claimed 确认）| 与多数不一致（被罚 1%）| 弃权（被罚 0.5%）>\n\
             ```\n\n\
             **Step 6 — 输出一行 sub session 日志后结束：**\n\n\
             > Relayed dispute_resolved to main, winningSide=<1|2>, claim submitted={{true|false}}, store cleaned.\n\n\
             【后续事件】\n\
             - reward_claimed（tx 上链结果）→ 推一条 claim 入账确认到主 session\n\
             - slashed（被罚通知）→ 若你是少数方或超时方，此事件稍后到达\n"
        ),

        // ─── 本轮失效（DisputeInvalidated） ──────────────────────────────
        "round_failed" => format!(
            "【当前状态】round_failed（DisputeInvalidated 上链，本轮无效：票数不足 / 无人揭示 / 全员弃票）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\
             提取 `title`。\n\n\
             **Step 2 — 清理本地 commit 存档（本轮作废，salt 不再可用）：**\n\
             ```bash\n\
             onchainos agent evaluator forget <disputeId>\n\
             ```\n\n\
             **Step 3 — 调 `notify_main` 推一条通知到主 session：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[仲裁进展] 任务『<title>』(jobId={job_id}) 本轮仲裁无效（票数不足 / 未揭示 / 全员弃票），\n\
             \x20\x20\x20\x20roundNumber++ 后等待下一轮陪审选出（若你再次被选中会收到 evaluator_selected）。\n\
             ```\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > round_failed disputeId=<disputeId> relayed; awaiting next round.\n\n\
             【后续事件】新一轮 evaluator_selected 会在 roundNumber++ 的新 disputeId 上到达（若再次被选中）；否则本流程对你终止。\n"
        ),

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
             \x20\x20\x20\x20若认为判决有误，可在申诉窗口内提交申诉。\n\
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

        // ─── 兜底（含 job_disputed / vote_committed / vote_revealed 等无需动作的事件） ─
        other => format!(
            "【未知或无需动作的状态】{other}\n\
             【说明】\n\
             - evaluator 仅在 evaluator_selected / reveal_started / dispute_resolved / round_failed / slashed / reward_claimed 介入\n\
             - job_disputed / vote_committed / vote_revealed 等事件无需动作，仅记录即可\n\n\
             【若确有异常】\n\
             1. 调用 `onchainos agent common context {job_id} --role evaluator` 查看上下文\n\
             2. 不要预测/假设其他通知\n"
        ),
    }
}
