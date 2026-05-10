use crate::commands::agent_commerce::task::common::state_machine::Status;

pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**下一步必做** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role evaluator --agentId <agentId>`（拿当前 status 的完整剧本，**按剧本走**，不要绕过 next-action 直接调下方 CLI）")
    };

    match status {
        Status::Disputed => vec![next_action("evaluator_selected")],
        Status::Completed | Status::Rejected => vec![next_action("dispute_resolved")],
        _ => vec![
            format!("当前任务 status=`{}` → evaluator 无任务级动作，等下一个相关链事件即可。", status.as_str()),
            "→ **不要**重复跑 `agent status` / `agent common context`（结果会一样），结束本轮 turn".to_string(),
        ],
    }
}

pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    if let Some(s) = staking_next_action(job_id, job_status, agent_id) {
        return s;
    }
    if let Some(s) = dispute_next_action(job_id, job_status, agent_id) {
        return s;
    }
    format!(
        "【unknown event or status={job_status} at jobId={job_id} ignored.\n
         禁止拉 context、禁止猜测其他通知。\n"
    )
}

fn staking_next_action(_job_id: &str, job_status: &str, _agent_id: &str) -> Option<String> {
    let body = match job_status {
        "staked" => "【当前状态】staked\n\n\
             【Step 1】跑 `evaluator my-stake --agent-id <你的 agentId>` 拿 `activeStake`。\n\
             【Step 2】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[质押 ✅] 当前 activeStake=<my-stake.activeStake> OKB。\n\n\
             【my-stake 失败兜底】丢掉数字字段，降级推 `[质押 ✅] 质押已上链生效。`\n".to_string(),

        "unstake_requested" => "【当前状态】unstake_requested\n\n\
             【Step 1】跑 `evaluator my-stake --agent-id <你的 agentId>` 拿 `pendingUnstake`、`unstakeAvailableAt`（已含本地时间）。\n\
             【Step 2】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ⏳] 当前累计待解 <my-stake.pendingUnstake> OKB；最后一次 unstake 的可领取时间 <unstakeAvailableAt 本地时间>。冷却期到了说『领取解质押』；中途撤销说『取消解质押』。\n\n\
             【my-stake 失败兜底】丢掉数字字段，降级推 `[解质押 ⏳] 已进入冷却期。冷却期到了说『领取解质押』；中途撤销说『取消解质押』。`\n".to_string(),

        "unstake_claimed" => "【当前状态】unstake_claimed\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ✅] 已领取，OKB 已入钱包。\n".to_string(),

        "unstake_cancelled" => "【当前状态】unstake_cancelled\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ✅] 已取消：待解 OKB 回到质押状态。\n".to_string(),

        "stake_stopped" => "【当前状态】stake_stopped\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[质押 🚪] 已退出 voter 池，不再被选为陪审。\n".to_string(),

        _ => return None,
    };
    Some(body)
}

fn dispute_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> Option<String> {
    let body = match job_status {
        "evaluator_selected" => format!(
            "【当前状态】evaluator_selected（本轮你被选为陪审，commit 窗口开启）\n\n\
             **Step 1 — 路由判断：**\n\n\
             ⚠️ 1.1 / 1.2 调用后必须立即把返回的 `sessionKey` 整串原文打印到本轮输出（不要节选、不要省略），后续比对必须基于已打印的两行原文。\n\n\
             **1.1** 调 `xmtp_start_evaluate_conversation`，参数 `myAgentId=<envelope 顶层 agentId>`、`jobId={job_id}`。打印：\n\
             `[evaluator-routing] arbKey=<本次 xmtp_start_evaluate_conversation 返回的 sessionKey 整串>`\n\n\
             **1.2** 调 `session_status`。打印：\n\
             `[evaluator-routing] currentKey=<本次 session_status 返回的 sessionKey 整串>`\n\n\
             **1.3** 把上面两行 `[evaluator-routing]` 逐字符比对（不要凭印象，必须基于已打印的两行原文）：\n\
             - 完全一致 → 进入 Step 2。\n\
             - 任一字符不同 → 调 `xmtp_dispatch_session`（`sessionKey=arbKey`，`content=<当前 inbound envelope 整体 JSON 字符串>`，**全字段原样塞入禁止改写**），然后**结束本轮 turn**。\n\n\
             **Step 2 — 从入站消息提取 `jobId`（envelope 顶层 `jobId` 字段）和顶层 `agentId`（你的 evaluator agentId）。**\n\
             `jobId` 缺省时直接中止本轮处理，输出 `missing jobId in payload; abort` 日志结束。\n\
             顶层 `agentId` 缺省时同样中止：后续 evaluator CLI 必须靠它定位钱包，缺了就签不了。\n\n\
             **Step 3 — 拉取当前证据（必须把 inbound envelope 顶层 `agentId` 透传给 `--agent-id`，CLI 据此定位钱包/身份）：**\n\
             ```bash\n\
             onchainos agent evidence-info <jobId> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             返回结构 `evidences: {{ provider: {{texts[], images[]}}, client: {{texts[], images[]}} }}`。\n\
             每张 `images[].fileKey` 已由 CLI 下载到本地，`localPath` 是绝对路径。\n\n\
             **⚠️ Step 3.5 — 必须实际打开每张图片阅读（最重要，禁止跳过）：**\n\
             - 遍历 `evidences.provider.images[].localPath` 和 `evidences.client.images[].localPath`\n\
             - **逐张调用多模态 read / view 能力读图**——截图里写了什么、展示了什么交付物、时间戳、对话内容，全要实际看过\n\
             - **禁止**只凭 `texts[]` 或 fileKey 名称猜测图片内容；不看图 = 放弃双方可能最关键的证据 = 违反 references/evaluator-decision-rubric.md §1 决策原则 #1『证据为王』\n\
             - **下载失败处理（硬约束）**：图片项含 `downloadError` 字段 = 该证据视为**缺失**，直接按 举证规则『一方未提交视为放弃举证』处理。\n\
             \x20\x20**禁止**用 `ls` / `find` / `cat` / `tree` / `stat` / `glob` / `Read` 等任何工具去本地磁盘找替代文件——这是 skills/okx-agent-task/SKILL.md Layer 0 安全门违例（『列目录、扫描磁盘』），且 `localPath` 不存在意味着 CLI 已知道这张图拿不到。\n\
             \x20\x20**禁止**重试 `evidence-info` 期望下次能下到（CLI 内部已尝试过 3 次）。直接进 Step 4 把这张图标记为缺失，继续走流程。\n\n\
             **Step 4 — 按 `references/evaluator-decision-rubric.md` 完成判决：**\n\
             - **前置 — 文件可读性检查**：用 Read 工具打开 `references/evaluator-decision-rubric.md`。\n\
             \x20\x20读取失败 / 文件不存在 / 内容为空 → **立即停止本轮**（不 commit、不兜底默认规则、不查找替代文件），用 `xmtp_dispatch_user` 推用户后结束 turn：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[仲裁中止 ⚠️] 任务 jobId={job_id}：判决规范 `references/evaluator-decision-rubric.md` 缺失或不可读，本轮放弃投票。\n\
             \x20\x20\x20\x20⚠️ commit 窗口超时会被罚没 stake，请尽快恢复该文件（git restore 或重装 plugin）后等待下一轮 evaluator_selected。\n\n\
             - 读取成功 → 按其中的 Rubric / 决策原则 / 归约表 / 裁决书模板，顺序产出最终 `vote` 与裁决书。\n\
             **Step 5 — 执行 commit（同样把 envelope 顶层 `agentId` 透传给 `--agent-id`）：**\n\
             ```bash\n\
             onchainos agent vote-commit <jobId> --vote <0|1> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ **只能是 0（Approve/Client 胜）或 1（Reject/Provider 胜），禁止 skip**（V1 无弃权；拖到超时被罚没的损失通常大于错投少数票）。\n\
             失败最多重试 3 次（CRITICAL，commit 窗口关闭即触发超时罚没）。返回 `voter has already committed` 视为成功进入 Step 6。\n\
             body 只带 `vote`。\n\n\
            **Step 6 — 输出一行日志后结束本回合：**\n\n\
             > Committed jobId=<jobId> vote=<0|1> autonomously per references/evaluator-decision-rubric.md.\n\n\
             【原则】\n\
             - **完全静默**：用户只会在后续结算/罚没/奖励事件被通知\n\
             - **判决权威**：所有打分规则、决策原则、裁决书格式以 评估者规范 为准\n\
             - **图片必读**：不读图即违反 references/evaluator-decision-rubric.md 1 节决策原则 #1 证据为王 / #3 举证责任；这是本 arm 最重要的执行要求\n"
        ),

        "vote_committed" => "【当前状态】vote_committed\n\n\
             【动作】无；不通知用户。\n".to_string(),

        "reveal_started" => "【当前状态】reveal_started\n\n\
             从 inbound envelope 顶层提取 `jobId` 与 `agentId` 并执行 reveal（jobId 缺省 → 输出 `missing jobId in payload; abort` 日志结束）：\n\
             ```bash\n\
             onchainos agent vote-reveal <jobId> --agent-id <envelope 顶层 agentId>\n\
             ```\n\n\
             【错误映射】\n\
             - `canReveal=false` → CLI 已预检拒绝，无需重试；本轮可能已结算（等 dispute_resolved）或未 commit（正常跳过）\n\
             - `voter has not committed` → 本轮未 commit，跳过 reveal 是正常的\n\
             - 其他失败最多重试 3 次\n".to_string(),

        "vote_revealed" => "【当前状态】vote_revealed\n\n\
             【动作】无；不通知用户。\n".to_string(),

        "dispute_resolved" => "【当前状态】dispute_resolved（DisputeSettled 上链，仲裁结算完成）\n\n\
             【Payload 约束】envelope 不携带胜负/数额。是否赢得本轮、要不要 claim 统一**用账面反推**（结算自动入账，靠 `arbitration-claimable` 反查）。\n\n\
             【Step 1】从 envelope 顶层提取 `agentId` 和 `jobId`。\n\n\
             【Step 2】调 `arbitration-claimable` 看本账户有没有可领奖励（透传 envelope 顶层 `agentId`）：\n\
             ```bash\n\
             onchainos agent arbitration-claimable --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             输出末尾会有一行稳定标记 `hasClaimable: yes` 或 `hasClaimable: no`，**只看这一行判定**，不要自己解析 amount。\n\
             - `hasClaimable: no` → 跳过 Step 3（你这次不是多数方，可能会收到 slashed 事件）\n\
             - `hasClaimable: yes` → 进入 Step 3 领取\n\n\
             【Step 3】立即领取奖励（account 级 pull）：\n\
             ```bash\n\
             onchainos agent arbitration-claim --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ account 级 pull 模式：除 `--agent-id` 外不带其它业务参数，一次把所有已结算 dispute 的待领奖励一起领出来（空 body）。\n\
             失败最多重试 3 次。真正的入账确认会通过稍后到达的 `reward_claimed` 事件告知用户。\n".to_string(),

        "slashed" => format!(
            "【当前状态】slashed\n\n\
             ⚠️ envelope.message 仅含 `event / jobId / timestamp / source / description`——没有 amount / reason，禁止编造或从其它字段猜测。\n\n\
             【Step 1】跑 `evaluator my-stake --agent-id <你的 agentId>` 拿 post-slash 的 `activeStake`。\n\
             【Step 2】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[Stake 罚没 ⚠️] 任务 jobId={job_id}，stake 已被扣罚；剩余 activeStake=<my-stake.activeStake> OKB。\n\n\
             【my-stake 失败兜底】丢掉数字字段，降级推 `[Stake 罚没 ⚠️] 任务 jobId={job_id}，stake 已被扣罚。`\n"
        ),

        "cooldown_entered" => "【当前状态】cooldown_entered\n\n\
             【Step 1】跑 `evaluator my-stake --agent-id <你的 agentId>` 拿 `cooldownEndsAt`（已含本地时间）。\n\
             【Step 2】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[冷却 ⏸️] 已进入缺席冷却期，<my-stake.cooldownEndsAt 本地时间> 前不会被选为陪审。\n\n\
             【my-stake 失败兜底】丢掉数字字段，降级推 `[冷却 ⏸️] 已进入缺席冷却期，期间不会被选为陪审。`\n".to_string(),

        "round_failed" =>
            "【当前状态】round_failed\n\n\
             【动作】无；不通知用户。\n".to_string(),

        "reward_claimed" => "【当前状态】reward_claimed（claimRewards tx 上链完成）\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[奖励 💰] 仲裁奖励已到账。\n".to_string(),

        _ => return None,
    };
    Some(body)
}
