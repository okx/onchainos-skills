//! Evaluator（仲裁者）端任务流程驱动器
//!
//! 合并后的状态机：evaluator 只在 EVIDENCE_CLOSED 时介入——此时证据已定版、
//! commit 窗口已开，一次性完成"拉证据 → 分析 → escalate_to_main → 用户回 1/2/skip →
//! 立即 commit"。TASK_DISPUTED / DISPUTE_ASSIGNED 对 evaluator 不再是动作触发点。

/// 根据 jobStatus 生成 evaluator 下一步动作的结构化提示词
pub fn generate_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> String {
    match job_status {
        // ─── 入口：证据封期结束，commit 窗口已开（sub session 侧） ────────
        "EVIDENCE_CLOSED" => format!(
            "【当前状态】EVIDENCE_CLOSED（证据封期结束，commit 窗口已开，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。禁止输出提示/询问；全部步骤静默完成后调 `escalate_to_main` 结束回合。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从入站消息提取 `disputeId`（字段名 `disputeId`；缺省用 `d-{job_id}-r1`）。**\n\n\
             **Step 2 — 拉取最终证据（此时已定版，不会再变）：**\n\
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
             \x20\x20\x20\x20Commit window is OPEN NOW — commit immediately once user decides.\n\
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
             \x20\x20\x20\x20- On REVEAL_WINDOW_OPEN / TASK_RESOLVED / REWARD_CLAIMABLE arrival → call\n\
             \x20\x20\x20\x20    onchainos agent next-action --jobid {job_id} --jobStatus <type> --agentId <me> --role evaluator\n\
             \x20\x20\x20\x20  and follow its output.\n\
             \x20\x20\x20\x20- Never expose raw CLI to user; never act on a different disputeId.\n\
             ```\n\n\
             **Step 5 — 输出一行 sub session 日志后结束本回合：**\n\n\
             > Escalated dispute=<disputeId> to main session.\n\n\
             【严禁】sub session 内直接跑 `evaluator commit` / `evaluator reveal`；commit 在主 session 的对话闭环里跑。\n\n\
             【后续事件】\n\
             - SUB_DECISION_REQUEST topic=dispute → 主 session 与用户对话 → 立即 commit\n\
             - REVEAL_WINDOW_OPEN → 主 session 再调 next-action 执行 reveal\n"
        ),

        // ─── reveal 窗口开启（sub session） ───────────────────────────────
        "REVEAL_WINDOW_OPEN" => format!(
            "【当前状态】REVEAL_WINDOW_OPEN（reveal 窗口开启，sub session 侧）\n\
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
             【后续事件】TASK_RESOLVED / REWARD_CLAIMABLE 会继续在同一 sub session 到达。\n"
        ),

        // ─── 结算完成（sub session） ─────────────────────────────────────
        "TASK_RESOLVED" => format!(
            "【当前状态】TASK_RESOLVED（仲裁结算完成，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。拉 context 在 sub 里完成，\n\
             \x20最后 `notify_main` 推结构化通知到主 session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文（为通知加标题 + 原争议对照）：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\
             提取 `title`（任务标题）、`clientReason` / `providerReason`（如有）。\n\n\
             **Step 2 — 从 payload 提取 `winningSide` / `settlement` / `yourVote`（不再跑任何 CLI）。**\n\n\
             **Step 3 — 调用 `notify_main` 工具：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[仲裁结算] 任务『<title>』(jobId={job_id}) 仲裁结算完成：\n\
             \x20\x20\x20\x20- 裁决结果：<Provider 胜诉 | Client 胜诉>（winningSide=<1|2>）\n\
             \x20\x20\x20\x20- 资金处理：<资金已释放给 Provider | 资金已退还 Client>\n\
             \x20\x20\x20\x20- 您本轮投票：side=<1|2|skip>，<与多数一致（获奖励）| 与多数不一致（被罚 1%）| 弃权（被罚 0.5%）>\n\
             ```\n\n\
             **Step 4 — 清理本地 commit 存档（dispute 已终结，{{vote, salt}} 不再需要）：**\n\
             ```bash\n\
             onchainos agent evaluator forget <disputeId>\n\
             ```\n\
             幂等——若无记录也只会报 \"already clean\"，不会失败。\n\n\
             **Step 5 — 输出一行 sub session 日志后结束：**\n\n\
             > Relayed TASK_RESOLVED to main, winningSide=<1|2>, store cleaned.\n\n\
             【后续事件】REWARD_CLAIMABLE 会继续在同一 sub session 到达。\n"
        ),

        // ─── 奖励可领取（sub session） ───────────────────────────────────
        "REWARD_CLAIMABLE" => format!(
            "【当前状态】REWARD_CLAIMABLE（奖励可领取，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。拉 context / 跑 claim 都在 sub 里完成，\n\
             \x20最后 `notify_main` 把领取结果推到主 session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 拉任务上下文：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\
             提取 `title`。payload 若带 `rewardAmount` 字段也一并记录。\n\n\
             **Step 2 — 执行 claim：**\n\
             ```bash\n\
             onchainos agent evaluator claim {job_id}\n\
             ```\n\n\
             **Step 3 — 调用 `notify_main` 工具：**\n\n\
             ```\n\
             tool: notify_main\n\
             arguments:\n\
             \x20\x20jobId: \"{job_id}\"\n\
             \x20\x20conversationId: \"<来源消息'会话:'行的值>\"\n\
             \x20\x20message: |\n\
             \x20\x20\x20\x20[仲裁奖励] 任务『<title>』(jobId={job_id}) 的仲裁奖励已领取 <rewardAmount OKB / 若无则省略>。\n\
             ```\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > Reward claimed jobId={job_id}.\n\n\
             【流程结束】此 disputeId 的 evaluator 生命周期完成；后续事件无需响应。\n"
        ),

        // ─── 未知类型兜底（含 TASK_DISPUTED / VOTE_COMMITTED / VOTE_REVEALED） ─
        other => format!(
            "【未知或无需动作的状态】{other}\n\
             【说明】\n\
             - evaluator 合并后只在 EVIDENCE_CLOSED / REVEAL_WINDOW_OPEN / TASK_RESOLVED / REWARD_CLAIMABLE 介入\n\
             - TASK_DISPUTED / DISPUTE_ASSIGNED / VOTE_COMMITTED / VOTE_REVEALED 等事件无需动作，仅记录即可\n\n\
             【若确有异常】\n\
             1. 调用 `onchainos agent common context {job_id} --role evaluator` 查看上下文\n\
             2. 不要预测/假设其他通知\n"
        ),
    }
}
