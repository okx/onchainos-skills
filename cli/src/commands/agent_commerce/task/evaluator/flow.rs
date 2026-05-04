//! Evaluator（仲裁者）端任务流程驱动器
//!
//! **链事件优先在仲裁 sub session 处理**（openclaw runtime 自动路由到 `conv-arb-*`）；但首次到达
//! 可能落在 user session（仲裁 sub 还没 bootstrap、或 sub 被回收/重启），所以 `evaluator_selected`
//! / `reveal_started` / `dispute_resolved` 三个入口事件的 arm 都先跑 Step 0（idempotent routing）：
//! 始终调 `xmtp_start_evaluate_conversation` 拿 arb sub key（幂等），再调 `session_status` 拿
//! 当前 key，两者相等 → 走原剧本；不等 → `xmtp_dispatch_session` 把 envelope **原样**转发到
//! arb sub 后结束 turn，由 sub agent 接手。
//! 事件命名对齐后端 event 枚举。
//!
//! | 分类 | 事件 | 行为 |
//! |---|---|---|
//! | 仲裁自主闭环（不 通知user session） | evaluator_selected / reveal_started / dispute_resolved / round_failed | sub 里执行 CLI 动作 + 静默结束 |
//! | 资金/罚没（通知user session 推用户） | reward_claimed / slashed | sub 提取字段 + 通知user session 推人话 |
//! | 质押 tx 回执（通知user session 推用户） | staked / unstake_requested / unstake_claimed / unstake_cancelled | sub 提取字段 + 通知user session 推人话 |
//! | 自己的投票 tx 回执 | vote_committed（静默记录）/ vote_revealed（完全忽略） | 都不通知用户 |
//! | 其他方事件 | job_disputed | 完全忽略 |
//!
//! evaluator 在 `evaluator_selected`（VotersSelected 上链）时即介入——此刻 CommitPhase 已开，
//! 在 sub session 里**自主闭环**完成 "拉证据（含看图）→ 按 references/evaluator-decision-rubric.md 2(决策原则) + 5(L4 自检) 判决 → 归约到 vote ∈ {{0,1}} → commit"。
//! 判决过程不通知用户；用户感知由后续 dispute_resolved → reward_claimed / slashed 负责。
//! 评估者规范 L2 + references/evaluator-decision-rubric.md 7：用户偏好会引入社会压力/贿赂风险，必须隔离。
//! 证据上传是链下操作（task 系统设计 doc 7.8: No chain event for evidence），不再等"证据封期"信号。
//!
//! 文案中若需要经济参数（罚金比例、最低质押、冷却期）真值，由 agent 自行调
//! `onchainos agent staking-config` 获取；本模块不再预拉。

use crate::commands::agent_commerce::task::common::state_machine::Status;

/// 链事件入口的会话路由公共块：始终先 `xmtp_start_evaluate_conversation` 拿本任务仲裁 sub 的
/// sessionKey（工具幂等），再用 `session_status` 拿当前 sessionKey，两者一致则继续原剧本，不一致
/// 则 `xmtp_dispatch_session` 把入站 envelope **原样**转发到 arb sub 并结束 turn。
///
/// 比"猜 sub key 形态"更稳——不依赖 `conv-arb-*` 命名约定，直接拿幂等返回值做精确比较。
///
/// 适用 arm：`evaluator_selected` / `reveal_started` / `dispute_resolved`——这三个事件首次
/// 到达可能落在 user session（仲裁 sub 还没 bootstrap、或 sub 被回收/重启），都需要先 bootstrap。
fn arb_session_routing_step(job_id: &str) -> String {
    format!(
        "**Step 0 — 路由到本任务仲裁 sub session（必须最先做，先于其它 CLI / 业务工具调用）：**\n\n\
         **0.1** 调 `xmtp_start_evaluate_conversation`，参数 `myAgentId=<envelope 顶层 agentId>`、`jobId={job_id}`。\n\
         \x20\x20- 工具幂等：本任务仲裁 sub 已存在时直接返回原 sessionKey，不会重复建 session；首次调用则建好 sub 后返回新 sessionKey。\n\
         \x20\x20- 调用前输出：`[evaluator-xmtp] xmtp_start_evaluate_conversation: myAgentId=<agentId>, jobId={job_id}`\n\
         \x20\x20- 调用后输出：`[evaluator-xmtp] xmtp_start_evaluate_conversation result: sessionKey=<返回值>`\n\
         \x20\x20→ 把这个 sessionKey 记作 `arbKey`。\n\n\
         **0.2** 调 `session_status` 工具拿当前 session 的 `sessionKey`，记作 `currentKey`。\n\n\
         **0.3** 比较两个 key：\n\
         - `currentKey == arbKey` → 已在仲裁 sub session 内，跳过本 Step，直接进入 Step 1 走原剧本。\n\
         - `currentKey != arbKey`（含 user session——`agent:main:main` 默认入口 / IM 桥接 session 等；或其他任务/角色的 sub）→ 必须把入站系统通知**原样转发**到 arb sub，本 turn 不要执行后续 Step：\n\
         \x20\x201) 调 `xmtp_dispatch_session`，参数 `sessionKey=<arbKey>`、`content=<把当前 inbound system envelope 整体序列化为 JSON 字符串原样塞入>`。\n\
         \x20\x20\x20\x20- ⚠️ **原样转发**：保留 envelope 全部字段（agentId / message.source / message.event / message.jobStatus / 业务 payload），**不要**改写、摘要、加 `[USER_DECISION_RELAY]` / `[STATUS_NOTIFY]` 之类前缀（那些前缀仅 user→sub 决策中继使用）。sub session agent 收到后会按系统通知标准流程重新触发 next-action。\n\
         \x20\x202) 结束本轮 turn——**禁止**在当前 session 继续执行 Step 1+，由 sub session agent 接手剧本。\n\n\
         ⚠️ **同 turn 不重复 `xmtp_start_evaluate_conversation` / `session_status`**：这两个工具结果在同 turn 内稳定，调过一次缓存复用；重复调 = 死循环征兆，立即停止。\n\n"
    )
}

/// Evaluator 在某 status 下可执行的 CLI 命令清单（用于 `agent common context` 菜单）。
///
/// 结构对齐 buyer/provider：每个分支首行 `next_action` 把 `next-action` 摆在
/// 「下一步必做」位，紧跟 `ref_header` 把后面的 CLI 标成"参考"。
/// 非 Disputed 状态下 evaluator 没有任务级动作，但应维持质押资格——所以列出
/// staking lifecycle 命令（陪审按 active stake 加权随机选取）。
pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**下一步必做** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role evaluator --agentId <agentId>`（拿当前 status 的完整剧本，**按剧本走**，不要绕过 next-action 直接调下方 CLI）")
    };
    let ref_header = "（参考·剧本里会用到的相关 CLI；不要直接调，先调 next-action 拿剧本）".to_string();

    // evaluator 在非 Disputed 状态下没有任务级动作，列质押 lifecycle 让 LLM
    // 主动维持/调整陪审资格。每次调用返回新 Vec（让多个分支都能 extend）。
    let staking_lifecycle = || -> Vec<String> {
        vec![
            "  # 质押 CLI 决策规则：先 my-stake 看 `registered` —— false → stake，true → increase-stake。`registered=false` 三种成因（任一都需重新走 stake 注册 + 一次性补齐到门槛）：① 从未注册 ② 被 slash 累计跌破 minCumulativeStakeOkb 后合约去注册 ③ 解质押后 claim-unstake 全部领走、stake 清零合约去注册".to_string(),
            "  # 兜底（registered 读漏 / 链上状态滞后）：stake 报任何错 → **自动用同 amount + 同 agentId 跑一次 increase-stake** 再试；只重试一次，再失败把错误码原样推用户决定".to_string(),
            "  onchainos agent my-stake --agent-id <agentId>                       # 【决策入口】查个人质押状态：重点看 `registered`（决定下一步 stake 还是 increase-stake）+ activeStake / pendingUnstake / cooldown / activeDisputes".to_string(),
            "  onchainos agent staking-config --agent-id <agentId>                 # 查平台门槛（minCumulativeStakeOkb / cooldown / 罚没比例）".to_string(),
            "  onchainos agent stake --amount <okb> --agent-id <agentId>            # my-stake.registered=false 时用：注册 + 首次/重新质押（amount ≥ minCumulativeStakeOkb；含 slash 跌破门槛 / 全额 claim-unstake 后的重新质押）。⚠️ 报错 → 自动 fallback 跑一次 increase-stake（同 amount/agentId）再试".to_string(),
            "  onchainos agent increase-stake --amount <okb> --agent-id <agentId>   # my-stake.registered=true 时用：追加质押（无最小限制；registered=false 时调本命令会被合约拒）。也作为 stake 的失败兜底".to_string(),
            "  onchainos agent request-unstake --amount <okb> --agent-id <agentId>  # 申请解质押（进入 cooldown；activeDisputes>0 会被合约 revert）".to_string(),
            "  onchainos agent claim-unstake --agent-id <agentId>                  # cooldown 结束后领取解质押 OKB".to_string(),
            "  onchainos agent cancel-unstake --agent-id <agentId>                 # cooldown 期内撤销解质押申请".to_string(),
        ]
    };

    match status {
        Status::Disputed => vec![
            next_action("evaluator_selected"),
            ref_header,
            "  onchainos agent evidence-info <disputeId> --agent-id <agentId>                # 查看仲裁详情（含证据；commit 阶段第一步）".to_string(),
            "  onchainos agent vote-commit <disputeId> --vote <0|1> --agent-id <agentId>     # 提交投票（0=Approve/Client 胜 / 1=Reject/Provider 胜，commit 阶段）".to_string(),
            "  onchainos agent vote-reveal <disputeId> --agent-id <agentId>                  # 揭示投票（reveal_started 到达后才能调；不传 --vote，后端反查 vote+salt）".to_string(),
        ],
        Status::Completed | Status::Rejected => vec![
            next_action("dispute_resolved"),
            ref_header,
            "  onchainos agent arbitration-claim --agent-id <agentId>                        # 领取所有已结算仲裁奖励（account-pull，无 jobId）".to_string(),
            "（终态，仲裁裁决已上链）COMPLETE = 资金释放给卖家（卖家胜）；REJECTED = 资金退还买家（买家胜）。".to_string(),
            "evaluator 奖励 / 罚没由 reward_claimed / slashed 通知触发；只要本轮投票跟多数一致就有奖。".to_string(),
        ],
        Status::Open | Status::Accepted | Status::Submitted | Status::Refused => {
            let label = status.as_str();
            let mut v = vec![
                format!("当前任务 status={label} → evaluator 未被选为陪审；任务进入 disputed 后才会触发 evaluator_selected 通知。期间维持质押资格即可（参考下方）。"),
                ref_header,
            ];
            v.extend(staking_lifecycle());
            v
        }
        Status::Close | Status::Expired | Status::AdminStopped | Status::Init => vec![
            format!("任务 status={} → 非活跃状态，evaluator 无需操作。", status.as_str()),
        ],
        Status::Other(s) => {
            let mut v = vec![
                format!("当前任务 status=`{s}` 不在 evaluator 关心的状态集（disputed 主动 / completed | rejected 终态领奖；open/accepted/submitted/refused 被动等待；close/expired/admin_stopped 终态无奖）内"),
                "→ 本角色无需任何任务级动作，等下一个相关链事件再处理".to_string(),
                "→ **不要**重复跑 `agent status` / `agent common context`（结果会一样），结束本轮 turn".to_string(),
                ref_header,
            ];
            v.extend(staking_lifecycle());
            v
        }
    }
}

/// 根据 jobStatus 生成 evaluator 下一步动作的结构化提示词。
///
/// 经济参数（罚金比例 / 最低质押 / 冷却期）不在此预拉：
/// 1) sub session 给 agent 自看的剧本（evaluator_selected / reveal_started）只用作动机性文案，规则不依赖具体数值；
/// 2) 推 user 通知所需的数值字段（amount / availableAt / txHash / rewardAmount / errorCode 等）
///    **真后端 envelope 一律不带**——只发 event / jobId / timestamp / source / description；
///    要播报数值，arm 内已要求 agent 现场调 `my-stake` / `arbitration-claimable` / `staking-config` 拉真值；
/// 3) `disputeType` / `yourVote` 同样不在 envelope 上，evaluator_selected 由 agent 从 task 详情 + 双方 reason
///    自行判断争议类型；dispute_resolved 由 `arbitration-claimable` 反推自己赢没赢决定要不要 claim。
pub fn generate_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> String {
    let step_zero = arb_session_routing_step(job_id);
    match job_status {
        // ─── 入口：本轮陪审选出，CommitPhase 已开（sub session 侧，agent 自主闭环） ──
        // 判决方法论严格对齐评估者规范（誓约 + 决策原则 + Rubric + 证据等级 + 裁决书规范）。
        // V1 合约只接受 vote ∈ {0, 1}（0=Approve/Client 胜，1=Reject/Provider 胜），原生 3 选项按 Step 4.5 归约表压到 0/1。
        // 结果不推给用户（不 通知user session）。
        "evaluator_selected" => format!(
            "【当前状态】evaluator_selected（VotersSelected 上链，你是本轮陪审，CommitPhase 已开）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 仲裁 sub session（结果不通知用户）。首次到达可能落在 user session，按 Step 0 路由进 sub 后再走判决流程。\n\
             【判决权威】评估者规范（誓约 L1-L5 + 决策原则 / Rubric / 证据等级 / 裁决书规范）。冲突以本规范为准。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从入站消息提取 `disputeId` 和顶层 `agentId`（你的 evaluator agentId）。**\n\
             ⚠️ envelope.message **不下发 `disputeType`**——争议类型由 agent 在 Step 4 从 task 详情 + 双方 `clientReason` / `providerReason` 自行判断（关键词：质量/规格/验收 → 质量；超时/逾期/拖延 → 超时；欺诈/恶意/串谋 → 恶意；判不出按「质量争议」兜底）。\n\
             ⚠️ `disputeId` 缺省时直接中止本轮处理，输出 `missing disputeId in payload; abort` 日志结束——真后端 `disputeId = keccak256(jobId, roundNumber)`，第 2+ 轮重选时 `d-{job_id}-r1` 一定对不上合约。\n\
             顶层 `agentId` 缺省时同样中止：后续 evaluator CLI 必须靠它定位钱包，缺了就签不了。\n\n\
             **Step 2 — 拉取当前证据（必须把 inbound envelope 顶层 `agentId` 透传给 `--agent-id`，CLI 据此定位钱包/身份）：**\n\
             ```bash\n\
             onchainos agent evidence-info <disputeId> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             返回真后端结构 `evidences: {{ provider: {{texts[], images[]}}, client: {{texts[], images[]}} }}`。\n\
             每张 `images[].fileKey` 已由 CLI 下载到本地，`localPath` 是绝对路径。\n\n\
             **⚠️ Step 2.5 — 必须实际打开每张图片阅读（最重要，禁止跳过）：**\n\
             - 遍历 `evidences.provider.images[].localPath` 和 `evidences.client.images[].localPath`\n\
             - **逐张调用多模态 read / view 能力读图**——截图里写了什么、展示了什么交付物、时间戳、对话内容，全要实际看过\n\
             - **禁止**只凭 `texts[]` 或 fileKey 名称猜测图片内容；不看图 = 放弃双方可能最关键的证据 = 违反 L3 义务 #1『必须完整阅读双方提交的所有材料』\n\
             - **下载失败处理（硬约束）**：图片项含 `downloadError` 字段 = 该证据视为**缺失**，直接按 举证规则『一方未提交视为放弃举证』处理。\n\
             \x20\x20**禁止**用 `ls` / `find` / `cat` / `tree` / `stat` / `glob` / `Read` 等任何工具去本地磁盘找替代文件——这是 SKILL.md Layer 0 安全门违例（『列目录、扫描磁盘』），且 `localPath` 不存在意味着 CLI 已知道这张图拿不到。\n\
             \x20\x20**禁止**重试 `evidence-info` 期望下次能下到（CLI 内部已尝试过 3 次）。直接进 Step 3 把这张图标记为缺失，继续走流程。\n\n\
             **Step 3 — 按 证据流程 材料读取流程构建证据清单：**\n\
             - ① 完整性：双方各提交了什么文本/图片？缺失什么？\n\
             - ② 任务基线：从 qualityStandards / description 建立\"任务应该是什么样\"\n\
             - ③ 分歧点：对比 clientReason / providerReason 标记双方说法不同的地方\n\
             - ④ 证据关联：每个分歧点对应哪些证据（文本 + 图片），按 证据等级 打等级 S/A/B/C/D\n\
             - ⑤ 链上验证：若证据引用链上记录，做交叉验证（S/A 级直接采信；C/D 级需对方承认或交叉佐证）\n\n\
             **Step 4 — 自行判定 `disputeType`（envelope 不下发）后选对应 Rubric 打分，再按 references/evaluator-decision-rubric.md 2 决策原则（优先级从高到低：证据为王 > 规格至上 > 举证责任 > 比例原则 > 模糊不利于起草方 > 沟通义务 > 善意推定 > 时间戳权威）收敛到 原生选项：**\n\
             \n\
             | disputeType | Rubric 权重（满分 100） | 原生选项 |\n\
             |---|---|---|\n\
             | 质量争议 | 规格匹配 40 + 验收达标 30 + 功能正确 20 + 专业标准 10 | 完成 / 部分完成 / 未完成 |\n\
             | 超时争议 | 时间线 35 + 沟通响应 25 + 阻塞依赖 25 + 外部因素 15 | 责任在 Client / 责任在 Provider / 不可抗力 |\n\
             | 恶意行为 | 行为性质 + 证据强度 + 行为模式 + 损害程度（汉隆剃刀：先排除能力不足） | 成立 / 不成立 |\n\
             \n\
             **Step 4.5 — 归约到 V1 合约的 vote ∈ {{0, 1}}（V1 二元投票强制约束，原生 3 选项不能直接上链）：**\n\
             \n\
             | disputeType | 原生选项 | vote | 语义 |\n\
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
             body 只带 `vote`；裁决书（Step 5）仅保留在 session 记忆，**不写入后端、不写本地、不推 user session**（后端反查 vote+salt 提供 reveal 所需，CLI 不持久化）。\n\n\
             ⚠️ **错误兜底硬约束（agent 失控反例）**：commit 报 `当前账户没有 evaluator（仲裁者） 身份，请先注册` / `code=2004` 时——\n\
             - **禁止**调 `onchainos agent create` / `agent register` / `identity_register` 任何注册类命令（链上写入、烧 gas、修改全局状态——evaluator 身份注册是用户主动决定的事，不是 sub session 自作主张能干的）\n\
             - **禁止**fallback 到查 identity / 找钱包 / 改 config 之类的迂回操作\n\
             - 直接：输出一行 `> commit aborted: evaluator identity not registered for this wallet; report to user via xmtp_dispatch_user`，**不**继续 Step 8，**不**自己跑识别流程，结束 turn 等用户处理\n\n\
             **Step 8 — 输出一行 sub session 日志后结束本回合。不调用 通知user session，不通知用户：**\n\n\
             > Committed dispute=<disputeId> vote=<0|1> autonomously per references/evaluator-decision-rubric.md 6 commit 执行.\n\n\
             【原则】\n\
             - **完全静默**：本 arm 不 escalate_to_main、不 通知user session；用户只会在后续结算/罚没/奖励事件被通知\n\
             - **判决权威**：所有打分规则、决策原则、裁决书格式以 评估者规范 为准\n\
             - **图片必读**：不读图即违反 L3 义务 #1 + references/evaluator-decision-rubric.md 2 决策原则 #3 举证责任；这是本 arm 最重要的执行要求\n\n\
             【后续事件】\n\
             - vote_committed → sub 里仅记录\n\
             - reveal_started → sub 里自动 reveal（envelope 带 disputeId）\n\
             - dispute_resolved → sub 里自动 claim（若赢）+ 通知user session\n\
             - round_failed → sub 里 通知user session\n"
        ),

        // ─── reveal 窗口开启（sub session，完全静默） ──────────────────
        "reveal_started" => format!(
            "【当前状态】reveal_started（RevealStarted 上链，reveal 窗口开启）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 仲裁 sub session（agent 自主 reveal，不通知用户）。首次到达可能落在 user session，按 Step 0 路由进 sub 后再 reveal。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从 inbound envelope 提取 `disputeId` 与顶层 `agentId`，执行 reveal：**\n\
             ```bash\n\
             onchainos agent vote-reveal <disputeId> --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ `disputeId` 缺省 → 输出 `missing disputeId in payload; abort` 日志结束，不要 fallback 编造（真后端 `disputeId = keccak256(jobId, roundNumber)`，第 2+ 轮重选时旧 id 一定对不上合约）。\n\
             \x20**不传 `--vote`**：post-2026-05 协议下后端从 `task_dispute_voter` 反查 vote+salt，CLI body 只发空 `{{}}`。\n\n\
             **Step 2 — 输出一行 sub session 日志后结束。禁止调用 通知user session：**\n\n\
             > Revealed dispute=<disputeId> autonomously.\n\n\
             【错误映射】\n\
             - `canReveal=false` → CLI 已预检拒绝，无需重试；本轮可能已结算（等 dispute_resolved）或未 commit（正常跳过）\n\
             - `voter has not committed` → 本轮未 commit，跳过 reveal 是正常的\n\
             - 其他失败最多重试 3 次（未 reveal 会触发超时罚没，具体比例见 `staking-config`）\n\n\
             【后续事件】dispute_resolved / round_failed / reward_claimed / slashed 会继续在同一 sub session 到达。仅 reward_claimed 和 slashed 会转发到user session。\n"
        ),

        // ─── 结算完成（sub 静默处理；入账/罚没通过后续 reward_claimed / slashed 事件再推user session） ─
        "dispute_resolved" => format!(
            "【当前状态】dispute_resolved（DisputeSettled 上链，仲裁结算完成）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ 仲裁 sub session（agent 自主 claim + 清理，不通知用户）。首次到达可能落在 user session，按 Step 0 路由进 sub 后再 claim。用户侧的入账/罚没通知由后续 reward_claimed / slashed arm 负责。\n\n\
             【Payload 约束】envelope.message 仅含 event / jobId / timestamp / source / description——\n\
             **不带 `yourVote` / `winningSide`** 等扩展字段。是否赢得本轮、要不要 claim，统一**用账面反推**（自己投了什么后端会自动结算到账户）。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从 payload 提取 `disputeId`（必需，缺省 abort）和顶层 `agentId`。**\n\n\
             **Step 2 — 调 `arbitration-claimable` 看本账户有没有可领奖励（透传 envelope 顶层 `agentId`）：**\n\
             ```bash\n\
             onchainos agent arbitration-claimable --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             返回 `rewards: [{{symbol, tokenAddress, rawAmount, amount}}, ...]`。**任一项 amount > 0** 视为有可领奖励。\n\
             - **0 项 / 全 0** → 跳过 claim（你这次不是多数方，可能会收到 slashed 事件）\n\
             - **≥ 1 项 amount > 0** → 进入 Step 3 领取\n\n\
             **Step 3 — 立即领取奖励（account 级 pull）：**\n\
             ```bash\n\
             onchainos agent arbitration-claim --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ account 级 pull 模式：除 `--agent-id` 外不带其它业务参数，一次把所有已结算 dispute 的待领奖励一起领出来（后端 `POST /task/claim`，空 body）。\n\
             失败最多重试 3 次。真正的入账确认会通过稍后到达的 `reward_claimed` 事件告知用户（那个 arm 会 通知user session）。\n\n\
             **Step 4 — 输出一行 sub session 日志后结束。禁止调用 通知user session：**\n\n\
             > Settled dispute=<disputeId> claim_submitted=<true|false>.\n\n\
             【后续事件】\n\
             - reward_claimed（claim tx 回执）→ 另一个 arm，会 通知user session 推入账给用户\n\
             - slashed（被罚通知）→ 另一个 arm，会 通知user session 推罚没金额+原因给用户\n\
             本 arm 到这里结束，**不抢这两个 arm 的通知职责**。\n"
        ),

        // ─── 本轮失效（sub 静默；若被罚会通过 slashed arm 再推user session） ──
        "round_failed" =>
            "【当前状态】round_failed（DisputeInvalidated 上链，本轮无效：票数不足 / 无人揭示 / 全员弃票）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 没有用户。**被动事件，无需链上操作 / 无本地清理**。\n\n\
             【你的下一步动作】\n\
             从 payload 提取 `disputeId`，输出一行 sub session 日志后结束。禁止调用 通知user session：\n\n\
             > round_failed disputeId=<disputeId>; awaiting next round.\n\n\
             【后续事件】\n\
             - 若被罚（未 commit / 未 reveal / 弃票）→ slashed arm 会 通知user session 告知用户\n\
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
             **Step 3 — 用 `xmtp_dispatch_user` 把罚没通知推给用户**：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[Stake 罚没 ⚠️] 任务『<title>』(jobId={job_id})\n\
             \x20\x20\x20\x20  - 金额：<amount> OKB\n\
             \x20\x20\x20\x20  - 原因：<reason>\n\
             \x20\x20\x20\x20  - disputeId：<disputeId>\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > Slashed amount=<amount> reason=<reason> relayed.\n"
        ),

        // ─── 奖励到账（claimRewards tx 上链结果） ──────────────────────
        "reward_claimed" => format!(
            "【当前状态】reward_claimed（claimRewards tx 上链完成，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session。\n\n\
             【Payload 约束】envelope.message 仅含 event / jobId / timestamp / source / description——\n\
             **不带 `status` / `txHash` / `rewardAmount` / `errorCode`**。到达 sub 即代表 success（failed 不会派发到这条事件流）。\n\n\
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1（必做）— 拉任务上下文为通知加标题：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\n\
             **Step 2（可选）— 如需播报具体到账金额，调 `arbitration-claimable` 或 `wallet history` 拉真值；不需要数字就跳过。**\n\
             - `arbitration-claimable` 一般已归零（刚领完），可作为入账完成的旁证\n\
             - 真要数额可拉 `onchainos wallet history --chain xlayer --token-symbol OKB --limit 5` 看最近一笔到账\n\n\
             **Step 3 — 用 `xmtp_dispatch_user` 把入账通知推给用户：**\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[仲裁奖励 💰] 任务『<title>』(jobId={job_id}) 奖励已到账。\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > reward_claimed relayed.\n\n\
             【流程结束】此 disputeId 的 evaluator 生命周期完成；后续事件无需响应。\n"
        ),

        // ─── 质押生命周期：sub 收到 → 通知user session 推人话给用户 ─────────────────
        //
        // ⚠️ 真后端推送的 envelope.message **仅含 event / jobId / timestamp / source / description**——
        // **不带 amount / availableAt / txHash / status / errorCode 等业务字段**（jobId 固定为
        // `system_voter_staking`，不是真任务）。需要播报数值时一律先调 `evaluator my-stake`
        // 拉链上权威值；禁止从 envelope 读不存在的字段。
        // 真后端首次质押与追加质押**统一**发 `staked` 事件——CLI 命令层 stake / increase-stake
        // 仍区分（对应不同后端 API），但事件流只看到一个 `staked`。模板兼顾两种场景：
        // 用户视角"质押已生效"通用文案；要区分首次/追加只能由 my-stake 看 activeStake 增量决定。
        "staked" => "【当前状态】staked（VoterStaking.Staked 上链，质押 tx 回执——首次质押与追加质押均发此事件，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Payload 约束】envelope.message 只含 event / jobId / timestamp / source / description，**没有 amount / txHash**，也无法从 event 区分首次/追加。\n\n\
             【Step 1（可选）】如需播报具体金额或区分首次/追加，先跑 `evaluator my-stake --agent-id <你的 agentId>` 拿 `activeStake`；不需要数字就跳过。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把质押结果推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[质押 ✅] 质押已上链生效。\n\
             \x20\x20\x20\x20（若已拉到 my-stake，可改成）当前 activeStake=<my-stake.activeStake> OKB。\n\n\
             【Step 3】输出日志结束：`> staked relayed.`\n".to_string(),

        "unstake_requested" => "【当前状态】unstake_requested（VoterStaking.UnstakeRequested 上链，申请解质押 tx 回执，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Payload 约束】envelope.message **没有 amount / availableAt / txHash**——必须主动调链才有真值。\n\n\
             【Step 1（必做）】跑 `evaluator my-stake --agent-id <你的 agentId>`，取 `pendingUnstake`（OKB）和 `unstakeAvailableAt`（unix 秒）。把秒级时间戳转本地时间字符串再填 content。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把申请受理通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ⏳] 申请已受理：<my-stake.pendingUnstake> OKB 进入冷却期，可领取时间 <unstakeAvailableAt 本地时间>。冷却期到了跟我说『领取解质押』我来提走；想中途撤销随时跟我说『取消解质押』（仅冷却期内有效）。\n\n\
             【Step 3】输出日志结束：`> unstake_requested relayed.`\n\n\
             ⚠️ **禁止**写死『7 天后』之类的天数——冷却期长度由 `staking-config.unstakeCooldownSeconds` 决定（可被 Apollo 动态改），始终用 my-stake 返回的 `unstakeAvailableAt` 真值。\n".to_string(),

        "unstake_claimed" => "【当前状态】unstake_claimed（VoterStaking.UnstakeClaimed 上链，领取解质押 tx 回执，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Payload 约束】envelope.message **没有 amount / txHash / status**——到达 sub 即代表 success（failed 不会派发）。\n\n\
             【Step 1（可选）】如需播报到账金额或最新余额，先跑 `evaluator my-stake --agent-id <你的 agentId>`——`pendingUnstake` 应已归零。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把到账通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ✅] 已领取，OKB 已入钱包。\n\n\
             【Step 3】输出日志结束：`> unstake_claimed relayed.`\n".to_string(),

        "unstake_cancelled" => "【当前状态】unstake_cancelled（VoterStaking.UnstakeCancelled 上链，取消解质押 tx 回执，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Payload 约束】envelope.message **没有 amount / txHash / status**。\n\n\
             【Step 1（可选）】如需播报新余额，先跑 `evaluator my-stake --agent-id <你的 agentId>`——`pendingUnstake` 应已归零，`activeStake` 增量。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把取消通知推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ✅] 已取消：待解 OKB 回到质押状态。\n\n\
             【Step 3】输出日志结束：`> unstake_cancelled relayed.`\n".to_string(),

        // ─── 自己的投票 tx 回执 ──────────────────────────────────────────
        "vote_committed" => "【当前状态】vote_committed（你自己的 commit tx 上链 success，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 无用户。这是**确认通知**，不是动作触发点。\n\n\
             【动作】仅记录 tx 成功状态；禁止重复 commit（后端会返回 `voter has already committed`）。**不调用 通知user session，不通知用户**——commit 是 agent 内部决策过程，用户感知由后续 dispute_resolved → reward_claimed / slashed 负责。\n\n\
             【输出】一行日志后结束：`> vote_committed recorded (silent).`\n\n\
             【后续事件】等 `reveal_started`（开启 reveal 窗口）→ sub 里跑 `vote-reveal`。\n".to_string(),

        "vote_revealed" => "【当前状态】vote_revealed（你自己的 reveal tx 上链 success，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — **完全忽略**，不记录不通知。\n\n\
             【动作】无——输出一行日志 `> vote_revealed ignored.` 后结束。禁止 通知user session。reveal 成功的用户感知由后续 dispute_resolved → reward_claimed / slashed 负责。\n\n\
             【后续事件】等 `dispute_resolved` / `round_failed`（结算/失效）→ sub 里跑对应 arm。\n".to_string(),

        // ─── 兜底：未知事件静默丢弃 ─────────────────────────────────
        // evaluator 事件集由后端 / 链事件枚举闭合，"未知" 只意味着 CLI 没跟上后端枚举——
        // 此时 agent 也没有逻辑去处理它（拉 context 也帮不上）。直接一行 trace 留痕即可，
        // 真正的修复是开发者补 arm。
        other => format!(
            "【未知事件】{other}（jobId={job_id}）—— evaluator 不响应。\n\
             【动作】无——输出一行日志 `> unknown event={other} at jobId={job_id} ignored.` 后结束。\n\
             禁止 通知user session、禁止拉 context、禁止猜测其他通知。\n"
        ),
    }
}
