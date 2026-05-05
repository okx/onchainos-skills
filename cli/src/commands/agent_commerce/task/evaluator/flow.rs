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

pub fn generate_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> String {
    let step_zero = format!(
        "**Step 0 — 路由到本任务的仲裁子session（必须最先做，先于其它工具调用）：**\n\n\
         仲裁全流程必须在专属仲裁子session 内执行（结果不通知用户）。系统事件首次到达可能落在其他 session，需手动转发后再继续。\n\n\
         **0.1** 调 `xmtp_start_evaluate_conversation`，参数 `myAgentId=<envelope 顶层 agentId>`、`jobId={job_id}` —— 返回值 `sessionKey` 即仲裁子session 的目标 key（下称 `arbKey`）。\n\n\
         **0.2** 调 `session_status` 拿当前所在 session 的 `sessionKey`（下称 `currentKey`）。\n\n\
         **0.3** 比较：\n\
         - `currentKey == arbKey` → 已在仲裁子session 内，进入 Step 1。\n\
         - `currentKey != arbKey` → 调 `xmtp_dispatch_session`（`sessionKey=arbKey`，`content=<当前 inbound envelope 整体 JSON 字符串>`，**全字段原样塞入禁止改写**），然后**结束本轮 turn**。子session agent 收到后会按标准系统通知流程重新触发 next-action。\n\n\
         ⚠️ **同 turn 内 `xmtp_start_evaluate_conversation` / `session_status` 各最多调一次**：结果稳定可缓存复用；重复调 = 死循环征兆，立即停止。\n\n"
    );
    match job_status {
        "evaluator_selected" => format!(
            "【当前状态】evaluator_selected（本轮你被选为陪审，commit 窗口开启）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 仲裁 子session。\n\
             【判决权威】评估者规范（誓约 L1-L5 + 决策原则 / Rubric / 证据等级 / 裁决书规范）。冲突以本规范为准。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从入站消息提取 `disputeId` 和顶层 `agentId`（你的 evaluator agentId）。**\n\
             ⚠️ `disputeId` 缺省时直接中止本轮处理，输出 `missing disputeId in payload; abort` 日志结束——`disputeId` 由 `(jobId, roundNumber)` 派生，第 2+ 轮重选时旧 id 一定对不上合约，不要 fallback 编造。\n\
             顶层 `agentId` 缺省时同样中止：后续 evaluator CLI 必须靠它定位钱包，缺了就签不了。\n\
             ⚠️ 争议类型在 Step 4 由 agent 从 task 详情 + 双方 `clientReason` / `providerReason` 自行判断（关键词：质量/规格/验收 → 质量；超时/逾期/拖延 → 超时；欺诈/恶意/串谋 → 恶意；判不出按\"质量\"兜底）。\n\n\
             **Step 2 — 拉取当前证据（必须把 inbound envelope 顶层 `agentId` 透传给 `--agent-id`，CLI 据此定位钱包/身份）：**\n\
             ```bash\n\
             onchainos agent evidence-info <disputeId> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             返回结构 `evidences: {{ provider: {{texts[], images[]}}, client: {{texts[], images[]}} }}`。\n\
             每张 `images[].fileKey` 已由 CLI 下载到本地，`localPath` 是绝对路径。\n\n\
             **⚠️ Step 2.5 — 必须实际打开每张图片阅读（最重要，禁止跳过）：**\n\
             - 遍历 `evidences.provider.images[].localPath` 和 `evidences.client.images[].localPath`\n\
             - **逐张调用多模态 read / view 能力读图**——截图里写了什么、展示了什么交付物、时间戳、对话内容，全要实际看过\n\
             - **禁止**只凭 `texts[]` 或 fileKey 名称猜测图片内容；不看图 = 放弃双方可能最关键的证据 = 违反 L3 义务 #1『必须完整阅读双方提交的所有材料』\n\
             - **下载失败处理（硬约束）**：图片项含 `downloadError` 字段 = 该证据视为**缺失**，直接按 举证规则『一方未提交视为放弃举证』处理。\n\
             \x20\x20**禁止**用 `ls` / `find` / `cat` / `tree` / `stat` / `glob` / `Read` 等任何工具去本地磁盘找替代文件——这是 skills/okx-agent-task/SKILL.md Layer 0 安全门违例（『列目录、扫描磁盘』），且 `localPath` 不存在意味着 CLI 已知道这张图拿不到。\n\
             \x20\x20**禁止**重试 `evidence-info` 期望下次能下到（CLI 内部已尝试过 3 次）。直接进 Step 3 把这张图标记为缺失，继续走流程。\n\n\
             **Step 3 — 按 证据流程 材料读取流程构建证据清单：**\n\
             - ① 完整性：双方各提交了什么文本/图片？缺失什么？\n\
             - ② 任务基线：从 qualityStandards / description 建立\"任务应该是什么样\"\n\
             - ③ 分歧点：对比 clientReason / providerReason 标记双方说法不同的地方\n\
             - ④ 证据关联：每个分歧点对应哪些证据（文本 + 图片），按 证据等级 打等级 S/A/B/C/D\n\
             - ⑤ 链上验证：若证据引用链上记录，做交叉验证（S/A 级直接采信；C/D 级需对方承认或交叉佐证）\n\n\
             **Step 4 — 自行判定争议类型后选对应 Rubric 打分，再按 references/evaluator-decision-rubric.md 2 决策原则（优先级从高到低：证据为王 > 规格至上 > 举证责任 > 比例原则 > 模糊不利于起草方 > 沟通义务 > 善意推定 > 时间戳权威）收敛到 原生选项：**\n\
             \n\
             | 争议类型 | Rubric 权重（满分 100） | 原生选项 |\n\
             |---|---|---|\n\
             | 质量争议 | 规格匹配 40 + 验收达标 30 + 功能正确 20 + 专业标准 10 | 完成 / 部分完成 / 未完成 |\n\
             | 超时争议 | 时间线 35 + 沟通响应 25 + 阻塞依赖 25 + 外部因素 15 | 责任在 Client / 责任在 Provider / 不可抗力 |\n\
             | 恶意行为 | 行为性质 + 证据强度 + 行为模式 + 损害程度（汉隆剃刀：先排除能力不足） | 成立 / 不成立 |\n\
             \n\
             **Step 4.5 — 归约到 V1 合约的 vote ∈ {{0, 1}}（V1 二元投票强制约束，原生 3 选项不能直接上链）：**\n\
             \n\
             | 争议类型 | 原生选项 | vote | 语义 |\n\
             |---|---|---|---|\n\
             | 质量 | 完成（总分 ≥ 80） | **1** | Reject 仲裁，Provider 胜，资金全额释放 |\n\
             | 质量 | 部分完成（40-79）/ 未完成（< 40） | **0** | Approve 仲裁，Client 胜，资金退回——V1 无部分结算通道；按 references/evaluator-decision-rubric.md 2 决策原则 #3『举证责任』质量争议由 Client 证明未完成 |\n\
             | 超时 | 责任在 Client / 不可抗力 | **1** | Reject 仲裁，Provider 不背锅 |\n\
             | 超时 | 责任在 Provider | **0** | Approve 仲裁，Provider 超时违约 |\n\
             | 恶意 | 不成立 | **1** | Reject 仲裁，被举报方无责 |\n\
             | 恶意 | 成立 | **0** | Approve 仲裁，被举报方违约 |\n\
             \n\
             ⚠️ 归约规则是硬约束——不得为了\"平衡\"或\"避免争议\"反向归约。决策原则 原则优先于对结果的担忧。\n\n\
             **Step 5 — 写裁决书（裁决书规范 + L3 义务 #4『必须在投票前写下完整推理链』）：**\n\
             \n\
             ⚠️ **『session 记忆』= 你 thinking 块里的推理过程，不是磁盘文件**。\n\
             - **禁止**调用 `write` / `edit` / `Write` / `Edit` / `NotebookEdit` 等任何文件写入工具落盘（\n\
             \x20\x20违反 L3 义务 #4 + L1 任务边界——裁决书不入链不推用户也不落盘，仅在 thinking 里推理）。\n\
             - **禁止**用 `exec` 跑 `tee` / `cat > file` / `echo > file` / 重定向 / `printf > file` 等方式间接写文件。\n\
             - 正确做法：把下面这段结构化文本**作为 thinking 内容**整理出来给自己看（用于 Step 6 递归自检），不要任何工具调用。\n\
             ```\n\
             争议 ID: <disputeId>\n\
             争议类型: <质量/超时/恶意>\n\
             Rubric 打分: <规格 X/40 + 验收 Y/30 + 功能 Z/20 + 专业 W/10 = 总分 N/100>\n\
             原生选项: <完成 | 部分完成 | 未完成 | ...>\n\
             V1 vote: <0 | 1>  // 0=Approve(Client 胜) / 1=Reject(Provider 胜)\n\
             事实认定:\n\
             \x20\x201. <基于证据认定的事实>\n\
             \x20\x202. <...>\n\
             证据引用（必须包含图片内容，不能只引用 texts[]）:\n\
             \x20\x20- 事实 1 ← provider/client 的 <图片 localPath 或 texts[i]> (Level <S|A|B|C|D>)\n\
             \x20\x20- ...\n\
             推理（引用 决策原则 原则编号）:\n\
             \x20\x20按原则 #<N>，<推理过程>\n\
             归约: 原生『<...>』→ V1 vote=<0|1>，依据 Step 4.5 归约表\n\
             ```\n\n\
             **Step 6 — L4 递归自检（誓约）：出发 commit 前逐项确认，任一未通过回 Step 4 重审：**\n\
             - □ 我是否完整阅读了双方全部材料（含每张图片）？\n\
             - □ 结论是否由证据推导出来的（而非先有结论再找证据）？\n\
             - □ 把 Client 和 Provider 角色互换，我会做出同样裁决吗？\n\
             - □ 我是否受到了材料包外的信息影响？\n\
             - □ 我是否在猜测其他 Evaluator 怎么投？\n\n\
             **Step 7 — 执行 commit（同样把 envelope 顶层 `agentId` 透传给 `--agent-id`）：**\n\
             ```bash\n\
             onchainos agent vote-commit <disputeId> --vote <0|1> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ **只能是 0（Approve/Client 胜）或 1（Reject/Provider 胜），禁止 skip**（V1 无弃权；拖到超时被罚没的损失通常大于错投少数票）。\n\
             失败最多重试 3 次（CRITICAL，commit 窗口关闭即触发超时罚没）。返回 `voter has already committed` 视为成功进入 Step 8。\n\
             body 只带 `vote`；裁决书（Step 5）仅保留在 session 记忆，**不写本地、不推 user session**。\n\n\
             ⚠️ **错误兜底硬约束（agent 失控反例）**：commit 报 `当前账户没有 evaluator（仲裁者） 身份，请先注册` / `code=2004` 时——\n\
             - **禁止**调 `onchainos agent create` / `agent register` / `identity_register` 任何注册类命令（链上写入、烧 gas、修改全局状态——evaluator 身份注册是用户主动决定的事，不是 子session 自作主张能干的）\n\
             - **禁止**fallback 到查 identity / 找钱包 / 改 config 之类的迂回操作\n\
             - 直接：输出一行 `> commit aborted: evaluator identity not registered for this wallet; report to user via xmtp_dispatch_user`，**不**继续 Step 8，**不**自己跑识别流程，结束 turn 等用户处理\n\n\
             **Step 8 — 输出一行 子session 日志后结束本回合。不调用 通知user session，不通知用户：**\n\n\
             > Committed dispute=<disputeId> vote=<0|1> autonomously per references/evaluator-decision-rubric.md 6 commit 执行.\n\n\
             【原则】\n\
             - **完全静默**：本 arm 不 通知user session；用户只会在后续结算/罚没/奖励事件被通知\n\
             - **判决权威**：所有打分规则、决策原则、裁决书格式以 评估者规范 为准\n\
             - **图片必读**：不读图即违反 L3 义务 #1 + references/evaluator-decision-rubric.md 2 决策原则 #3 举证责任；这是本 arm 最重要的执行要求\n"
        ),

        // ─── reveal 窗口开启（子session，完全静默） ──────────────────
        "reveal_started" => format!(
            "【当前状态】reveal_started（RevealStarted 上链，reveal 窗口开启）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 仲裁 子session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从 inbound envelope 提取 `disputeId` 与顶层 `agentId`，执行 reveal：**\n\
             ```bash\n\
             onchainos agent vote-reveal <disputeId> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ `disputeId` 缺省 → 输出 `missing disputeId in payload; abort` 日志结束，不要 fallback 编造（`disputeId` 由 `(jobId, roundNumber)` 派生，第 2+ 轮重选时旧 id 一定对不上合约）。\n\
             **Step 2 — 输出一行 子session 日志后结束。禁止调用 通知user session：**\n\n\
             > Revealed dispute=<disputeId> autonomously.\n\n\
             【错误映射】\n\
             - `canReveal=false` → CLI 已预检拒绝，无需重试；本轮可能已结算（等 dispute_resolved）或未 commit（正常跳过）\n\
             - `voter has not committed` → 本轮未 commit，跳过 reveal 是正常的\n\
             - 其他失败最多重试 3 次（未 reveal 会触发超时罚没，具体比例见 `staking-config`）\n"
        ),

        // ─── 结算完成（子session 静默处理；入账/罚没通过后续 reward_claimed / slashed 事件再推user session） ─
        "dispute_resolved" => format!(
            "【当前状态】dispute_resolved（DisputeSettled 上链，仲裁结算完成）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 仲裁 子session（agent 自主 claim + 清理，不通知用户）。首次到达可能落在 user session，按 Step 0 路由进子session 后再 claim。用户侧的入账/罚没通知由后续 reward_claimed / slashed arm 负责。\n\n\
             【Payload 约束】envelope 不携带胜负/数额。是否赢得本轮、要不要 claim 统一**用账面反推**（结算自动入账，靠 `arbitration-claimable` 反查）。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从 envelope 顶层提取 `agentId` 和 `jobId`。**\n\n\
             **Step 2 — 调 `arbitration-claimable` 看本账户有没有可领奖励（透传 envelope 顶层 `agentId`）：**\n\
             ```bash\n\
             onchainos agent arbitration-claimable --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             返回 `rewards: [{{symbol, tokenAddress, rawAmount, amount}}, ...]`。**任一项 amount > 0** 视为有可领奖励。\n\
             - **0 项 / 全 0** → 跳过 Step 3，直接进 Step 4（你这次不是多数方，可能会收到 slashed 事件）\n\
             - **≥ 1 项 amount > 0** → 进入 Step 3 领取\n\n\
             **Step 3 — 立即领取奖励（account 级 pull）：**\n\
             ```bash\n\
             onchainos agent arbitration-claim --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ account 级 pull 模式：除 `--agent-id` 外不带其它业务参数，一次把所有已结算 dispute 的待领奖励一起领出来（空 body）。\n\
             失败最多重试 3 次。真正的入账确认会通过稍后到达的 `reward_claimed` 事件告知用户（那个 arm 会 通知user session）。\n\n\
             **Step 4 — 输出一行 子session 日志后结束。禁止调用 通知user session：**\n\n\
             > Settled jobId=<jobId> claim_submitted=<true|false>.\n"
        ),

        // ─── 本轮失效（子session 静默；若被罚会通过 slashed arm 再推user session） ──
        "round_failed" =>
            "【当前状态】round_failed（DisputeInvalidated 上链，本轮无效：票数不足 / 无人揭示 / 全员弃票）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【你的下一步动作】\n\
             从 payload 提取 `disputeId`，输出一行 子session 日志后结束。禁止调用 通知user session：\n\n\
             > round_failed disputeId=<disputeId>; awaiting next round.\n"
                .to_string(),

        // ─── 被罚没（VoterStaking.Slashed） ─────────────────────────────
        "slashed" => format!(
            "【当前状态】slashed（VoterStaking.Slashed 上链，你的 stake 被罚没）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `amount`（被罚金额）、`reason`（commit/reveal 超时 / 少数方 / 弃权）、`disputeId`。**\n\n\
             **Step 2 — 用 `xmtp_dispatch_user` 把罚没通知推给用户**：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[Stake 罚没 ⚠️] 任务 jobId={job_id}\n\
             \x20\x20\x20\x20  - 金额：<amount> OKB\n\
             \x20\x20\x20\x20  - 原因：<reason>\n\
             \x20\x20\x20\x20  - disputeId：<disputeId>\n\n\
             **Step 3 — 输出一行 子session 日志后结束：**\n\n\
             > Slashed amount=<amount> reason=<reason> relayed.\n"
        ),

        // ─── 奖励到账（claimRewards tx 上链结果） ──────────────────────
        "reward_claimed" => format!(
            "【当前状态】reward_claimed（claimRewards tx 上链完成，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Payload 约束】到达本 arm 即代表 success（失败不会派发到这条事件流）。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1（可选）— 如需播报具体到账金额，调 `arbitration-claimable` 或 `wallet history` 拉真值；不需要数字就跳过。**\n\
             - `arbitration-claimable` 一般已归零（刚领完），可作为入账完成的旁证\n\
             - 真要数额可拉 `onchainos wallet history --chain xlayer --token-symbol OKB --limit 5` 看最近一笔到账\n\n\
             **Step 2 — 用 `xmtp_dispatch_user` 把入账通知推给用户：**\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[仲裁奖励 💰] 任务 jobId={job_id} 奖励已到账。\n\n\
             **Step 3 — 输出一行 子session 日志后结束：**\n\n\
             > reward_claimed relayed.\n"
        ),

        // ─── 质押生命周期：子session 收到 → 通知user session 推人话给用户 ─────────────────
        // jobId 固定为 `system_voter_staking`（不是真任务）。
        // 首次质押与追加质押统一发 `staked` 事件——CLI 命令层 stake / increase-stake 仍区分
        // 入口，但事件流只看到一个 `staked`。
        "staked" => "【当前状态】staked（VoterStaking.Staked 上链，质押 tx 回执——首次质押与追加质押均发此事件，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Step 1（可选）】如需播报具体金额，先跑 `evaluator my-stake --agent-id <你的 agentId>` 拿 `activeStake`；不需要数字就跳过。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把质押结果推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[质押 ✅] 质押已上链生效。\n\
             \x20\x20\x20\x20（若已拉到 my-stake，可改成）当前 activeStake=<my-stake.activeStake> OKB。\n\n\
             【Step 3】输出日志结束：`> staked relayed.`\n".to_string(),

        "unstake_requested" => "【当前状态】unstake_requested（VoterStaking.UnstakeRequested 上链，申请解质押 tx 回执，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Step 1（必做）】跑 `evaluator my-stake --agent-id <你的 agentId>`，取 `pendingUnstake`（OKB）和 `unstakeAvailableAt`（unix 秒）。把秒级时间戳转本地时间字符串再填 content。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把申请受理通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ⏳] 申请已受理：<my-stake.pendingUnstake> OKB 进入冷却期，可领取时间 <unstakeAvailableAt 本地时间>。冷却期到了跟我说『领取解质押』我来提走；想中途撤销随时跟我说『取消解质押』（仅冷却期内有效）。\n\n\
             【Step 3】输出日志结束：`> unstake_requested relayed.`\n\n\
             ⚠️ **禁止**写死『7 天后』之类的天数——冷却期长度由 `staking-config.unstakeCooldownSeconds` 决定（可动态变化），始终用 my-stake 返回的 `unstakeAvailableAt` 真值。\n".to_string(),

        "unstake_claimed" => "【当前状态】unstake_claimed（VoterStaking.UnstakeClaimed 上链，领取解质押 tx 回执，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把到账通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ✅] 已领取，OKB 已入钱包。\n\n\
             【Step 3】输出日志结束：`> unstake_claimed relayed.`\n".to_string(),

        "unstake_cancelled" => "【当前状态】unstake_cancelled（VoterStaking.UnstakeCancelled 上链，取消解质押 tx 回执，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把取消通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ✅] 已取消：待解 OKB 回到质押状态。\n\n\
             【Step 3】输出日志结束：`> unstake_cancelled relayed.`\n".to_string(),

        "stake_stopped" => "【当前状态】stake_stopped（VoterStaking.VoterStakeStopped 上链，已退出 voter 池，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Step 1】用 `xmtp_dispatch_user` 把退出通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[质押 🚪] 已退出 voter 池，不再被选为陪审。\n\n\
             【Step 2】输出日志结束：`> stake_stopped relayed.`\n".to_string(),

        "cooldown_entered" => "【当前状态】cooldown_entered（DisputeManager.VoterCooldownEntered 上链，进入缺席冷却期，无 user tx，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【Step 1（必做）】跑 `evaluator my-stake --agent-id <你的 agentId>`，取 `cooldownEndsAt`（unix 秒）。把秒级时间戳转本地时间字符串再填 content。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把进入冷却通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[冷却 ⏸️] 已进入缺席冷却期，<my-stake.cooldownEndsAt 本地时间> 前不会被选为陪审。\n\n\
             【Step 3】输出日志结束：`> cooldown_entered relayed.`\n\n\
             ⚠️ **禁止**写死冷却时长——长度由 `staking-config.slashedCooldownSeconds` 决定（可动态变化），始终用 my-stake 返回的 `cooldownEndsAt` 真值。\n".to_string(),

        // ─── 自己的投票 tx 回执 ──────────────────────────────────────────
        "vote_committed" => "【当前状态】vote_committed（你自己的 commit tx 上链 success，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【动作】无；禁止重复 commit（重复调用会被拒：`voter has already committed`）。**不调用 通知user session，不通知用户**\n\n\
             【输出】一行日志后结束：`> vote_committed recorded (silent).`\n".to_string(),

        "vote_revealed" => "【当前状态】vote_revealed（你自己的 reveal tx 上链 success，子session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 子session。\n\n\
             【动作】无——输出一行日志 `> vote_revealed ignored.` 后结束。禁止 通知user session。\n".to_string(),

        other => format!(
            "【未知事件】{other}（jobId={job_id}）—— evaluator 不响应。\n\
             【动作】无——输出一行日志 `> unknown event={other} at jobId={job_id} ignored.` 后结束。\n\
             禁止 通知user session、禁止拉 context、禁止猜测其他通知。\n"
        ),
    }
}
