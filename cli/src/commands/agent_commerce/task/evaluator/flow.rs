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
        Status::Completed | Status::Refunded => vec![
            next_action("dispute_resolved"),
            ref_header,
            "  onchainos agent arbitration-claim --agent-id <agentId>                        # 领取所有已结算仲裁奖励（account-pull，无 jobId）".to_string(),
            "（流程结束）裁决已上链，奖励/罚没由 reward_claimed / slashed 通知触发".to_string(),
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
        Status::Other(s) => {
            let mut v = vec![
                format!("当前状态 `{s}` 不在标准状态机内 → 先 `onchainos agent status {job_id}` 查最新状态"),
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
/// 2) 推 user 的成功通知（staked / unstake_requested success）payload 已带 `amount` / `availableAt` 等关键字段；
/// 3) 失败通知或需要真值的场合，由 agent 现场调 `onchainos agent staking-config`。
/// todo 验证系统事件是否也会派发 failed 状态
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
             **Step 1 — 从入站消息提取 `disputeId`、`disputeType`（质量/超时/恶意）、顶层 `agentId`（你的 evaluator agentId）。**\n\
             ⚠️ `disputeId` 缺省时直接中止本轮处理，输出 `missing disputeId in payload; abort` 日志结束——真后端 `disputeId = keccak256(jobId, roundNumber)`，第 2+ 轮重选时 `d-{job_id}-r1` 一定对不上合约。\n\
             `disputeType` 缺省时按质量争议处理（最常见）。\n\
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
             **Step 4 — 按 `disputeType` 选对应 Rubric 打分（Rubric），再按 references/evaluator-decision-rubric.md 2 决策原则（优先级从高到低：证据为王 > 规格至上 > 举证责任 > 比例原则 > 模糊不利于起草方 > 沟通义务 > 善意推定 > 时间戳权威）收敛到 原生选项：**\n\
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
             【你的下一步动作（严格顺序）】\n\n\
             {step_zero}\
             **Step 1 — 从 payload 提取 `winningSide` / `yourVote`。**\n\n\
             **Step 2 — 若 `yourVote` 与 `winningSide` 一致（多数方），立即领取奖励（透传 envelope 顶层 `agentId`）：**\n\
             ```bash\n\
             onchainos agent arbitration-claim --agent-id <envelope 顶层 agentId>\n\
             ```\n\
             ⚠️ account 级 pull 模式：除 `--agent-id` 外不带其它业务参数，一次把所有已结算 dispute 的待领奖励一起领出来（后端 `POST /task/claim`，空 body）。\n\
             失败最多重试 3 次。真正的入账确认会通过稍后到达的 `reward_claimed` 事件告知用户（那个 arm 会 通知user session）。\n\
             若 `yourVote` 与多数不一致 / 为空，跳过 claim（不会有奖励，可能会收到 slashed 事件）。\n\n\
             **Step 3 — 输出一行 sub session 日志后结束。禁止调用 通知user session：**\n\n\
             > Settled dispute=<disputeId> winningSide=<1|2> yourVote=<1|2> claim_submitted={{true|false}}.\n\n\
             【后续事件】\n\
             - reward_claimed（claim tx 回执）→ 另一个 arm，会 通知user session 推入账/失败给用户\n\
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
             【你的下一步动作（严格顺序）】\n\n\
             **Step 1 — 从 payload 提取 `status`（success / failed）、`txHash`、`rewardAmount`、`errorCode`（若 failed）。**\n\n\
             **Step 2 — 拉任务上下文为通知加标题：**\n\
             ```bash\n\
             onchainos agent common context {job_id} --role evaluator\n\
             ```\n\n\
             **Step 3 — 用 `xmtp_dispatch_user` 把入账/失败通知推给用户**（按 status 二选一填 content）：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20success → [仲裁奖励 💰] 任务『<title>』(jobId={job_id}) 奖励已到账 <rewardAmount> OKB，txHash=<txHash>。\n\
             \x20\x20\x20\x20failed  → [仲裁奖励失败 ⚠️] 任务『<title>』(jobId={job_id}) claim 失败 (errorCode=<errorCode>, txHash=<txHash>)，请按错误码重试。\n\n\
             **Step 4 — 输出一行 sub session 日志后结束：**\n\n\
             > reward_claimed status=<status> amount=<rewardAmount> relayed.\n\n\
             【流程结束】此 disputeId 的 evaluator 生命周期完成；后续事件无需响应。\n"
        ),

        // ─── 质押生命周期：sub 收到 → 通知user session 推人话给用户 ─────────────────
        // 当前剧本只覆盖 success 推送路径；失败分支后续按需补回（届时由 agent 现场调 staking-config 取真值）。
        // 若会，需要补回 failed 分支的用户通知文案（含 errorCode 解释、最低质押门槛、冷却期等真值）。
        "staked" => "【当前状态】staked（VoterStaking.Staked 上链，质押 tx 结果（首次质押或追加质押均发此事件），sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 从 payload 提取字段 → 通知user session 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `amount`、`txHash`（仅处理 success；failed 暂不推送，后续按需扩展）。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把质押结果推给用户：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[质押 ✅] 质押已生效：+<amount> OKB，txHash=<txHash>。你现在是活跃仲裁者候选。\n\n\
             【Step 3】输出日志结束：`> staked amount=<amount> relayed.`\n".to_string(),

        "unstake_requested" => "【当前状态】unstake_requested（VoterStaking.UnstakeRequested 上链，申请解质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `amount`、`availableAt`（冷却结束毫秒时间戳）、`txHash`（仅处理 success；failed 暂不推送，后续按需扩展）。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把申请解质押结果推给用户（`availableAt` 转本地时间后再填进 content）：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20[解质押 ⏳] 申请已受理：-<amount> OKB 进入冷却期，可领取时间 <availableAt 本地时间>。冷却期到了跟我说『领取解质押』我来提走；想中途撤销随时跟我说『取消解质押』（仅冷却期内有效）。\n\n\
             【Step 3】输出日志结束：`> unstake_requested amount=<amount> relayed.`\n".to_string(),

        "unstake_claimed" => "【当前状态】unstake_claimed（VoterStaking.UnstakeClaimed 上链，领取解质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `status`、`amount`、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把领取解质押结果推给用户（按 status 二选一填 content）：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20success → [解质押 ✅] 已提走 <amount> OKB，已入钱包，txHash=<txHash>。\n\
             \x20\x20\x20\x20failed  → [解质押失败 ⚠️] 领取失败（errorCode=<errorCode>, txHash=<txHash>），请按错误码重试。常见原因：锁定期未满 / 无待解质押。\n\n\
             【Step 3】输出日志结束：`> unstake_claimed status=<status> amount=<amount> relayed.`\n".to_string(),

        "unstake_cancelled" => "【当前状态】unstake_cancelled（VoterStaking.UnstakeCancelled 上链，取消解质押 tx 结果，sub session 侧）\n\
             【角色】仲裁者（Evaluator）\n\
             【会话类型】⚠️ Sub session — 通知user session 推人话给用户。\n\n\
             【Step 1】从 payload 提取 `status`、`amount`、`txHash`、`errorCode`（若 failed）。\n\n\
             【Step 2】用 `xmtp_dispatch_user` 把取消解质押结果推给用户（按 status 二选一填 content）：\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20success → [解质押 ✅] 已取消：<amount> OKB 回到质押状态，txHash=<txHash>。\n\
             \x20\x20\x20\x20failed  → [解质押失败 ⚠️] 取消失败（errorCode=<errorCode>, txHash=<txHash>）。常见原因：冷却期已过 / 无待解质押。\n\n\
             【Step 3】输出日志结束：`> unstake_cancelled status=<status> amount=<amount> relayed.`\n".to_string(),

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
