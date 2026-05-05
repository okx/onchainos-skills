//! common — 任务系统通用查询命令
//!
//! 核心命令：`context`
//! 根据 job_id + 角色，从后端拉取任务详情，生成结构化自然语言上下文，
//! 供大模型（openclaw buyer/provider/evaluator AI）理解当前任务状态。

use anyhow::{bail, Result};
use clap::Subcommand;
use serde::Deserialize;

pub mod claim;
pub mod dispute_upload;
pub mod network;
pub mod payment_mode;
pub mod query;
pub mod state_machine;
pub mod util;

use util::fmt_unix_secs;

use crate::commands::Context;

// ─── 链常量 ──────────────────────────────────────────────────────────────

/// XLayer chain ID（用于任务系统合约部署链）
pub const XLAYER_CHAIN_ID: i32 = 196;
/// XLayer chain index 字符串形式（用于钱包 API）
pub const XLAYER_CHAIN_INDEX: &str = "196";
/// XLayer chain name（用于 wallet_store 地址查找，wallets.json 中 chainIndex=196 的 chainName）
pub const XLAYER_CHAIN_NAME: &str = "okb";

// ─── Agent 角色常量（身份模块 API role 字段值）────────────────────────────

/// 买家 / 需求方（requestor）
pub const AGENT_ROLE_BUYER: i64 = 1;
/// 卖家 / 服务方（provider）
pub const AGENT_ROLE_PROVIDER: i64 = 2;
/// 仲裁者（evaluator）
pub const AGENT_ROLE_EVALUATOR: i64 = 3;

pub use payment_mode::PaymentMode;

pub use util::ensure_sufficient_balance;

// ─── CLI 定义 ──────────────────────────────────────────────────────────────
#[derive(Subcommand)]
pub enum CommonCommand {
    /// 查询任务上下文，输出供大模型使用的结构化自然语言描述
    ///
    /// 示例：
    ///   onchainos agent context task-001 --role buyer --agent-id 426
    ///   onchainos agent context task-001 --role provider --agent-id 558
    Context {
        /// 任务 ID（jobId），如 task-001 或 0x1a2b...
        job_id: String,

        /// 调用者角色：buyer | provider | evaluator
        #[arg(long, default_value = "buyer")]
        role: String,

        /// 调用者的 AgentID（**必填**）。beta 后端要求 agenticId header 非空，
        /// 一个钱包可能有多个 provider agent，调用方必须显式选定，CLI 不自动挑。
        /// 钱包地址 / 通信地址会通过 `agent get --agent-ids <agent_id>` 自动反查，
        /// 无需 CLI 传入。
        #[arg(long)]
        agent_id: String,
    },
}

// ─── 任务详情响应结构 ──────────────────────────────────────────────────────
// 字段对齐后端 spec：/priapi/v1/aieco/task/{jobId} 响应 data 字段（平铺）。

/// 对齐 spec：/priapi/v1/aieco/task/{jobId} 响应 data 字段
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskDetail {
    job_id: String,
    task_id: Option<i64>,
    title: String,
    description: String,
    content_hash: Option<String>,
    token_address: Option<String>,
    /// 后端 spec：直接返回的代币符号（USDT / USDG）。
    token_symbol: Option<String>,
    token_amount: Option<String>,
    /// 0=未设置 / 1=escrow / 2=non_escrow / 3=x402
    payment_mode: Option<i32>,
    /// 后端 VisibilityEnum：0=PUBLIC（公开） / 1=PRIVATE（私有）
    visibility: Option<i32>,
    /// 0=open / 1=accepted / 2=submitted / 3=refused / 4=disputed / 5=complete / 7=close
    status: Option<i32>,
    sensitive_status: Option<i32>,
    category_codes: Option<Vec<String>>,
    chain_id: Option<i32>,
    min_credit_score: Option<f64>,
    designated_provider: Option<String>,
    buyer_agent_address: Option<String>,
    buyer_agent_id: Option<String>,
    provider_agent_address: Option<String>,
    provider_agent_id: Option<String>,
    group_id: Option<String>,
    expire_config: Option<serde_json::Value>,
    /// unix 秒；0 表示未设置
    expire_time: Option<i64>,
    create_time: Option<i64>,
    update_time: Option<i64>,
}

// ─── Agent 资料响应结构 ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AgentProfile {
    #[allow(dead_code)]
    agent_id: Option<String>,
    name: Option<String>,
    profile_description: Option<String>,
    /// 钱包地址（owner / 部署该 agent 的 EOA）
    agent_wallet_address: Option<String>,
    /// XMTP 通信地址（agent 之间 P2P 通讯用）
    communication_address: Option<String>,
}

/// 查询指定 agentId 的 agent 资料（name / profileDescription / 钱包地址 / 通信地址）。
///
/// 直接 spawn `onchainos agent get --agent-ids <id>` 子进程 + parse stdout——
/// 不复刻 token / wallet client / URL 拼装逻辑，`agent get` 实现以后改了这里自动跟上。
/// 任何错误路径都回退到带 agentId 的占位符（地址字段为 None），保证返回值非空。
async fn fetch_agent_profile(agent_id: &str) -> AgentProfile {
    let fallback = || AgentProfile {
        agent_id: Some(agent_id.to_string()),
        name: Some(format!("Agent {agent_id}")),
        profile_description: Some("(profile unavailable)".to_string()),
        agent_wallet_address: None,
        communication_address: None,
    };
    if agent_id.is_empty() {
        return fallback();
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[fetch_agent_profile] current_exe 失败: {e}; fallback");
            return fallback();
        }
    };

    // 子进程会继承父进程 env（含 OKX_BASE_URL），跟父进程打的 URL 完全一致。
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get", "--agent-ids", agent_id]);
    let output = match cmd.output().await
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[fetch_agent_profile] spawn `agent get` 失败: {e}; fallback");
            return fallback();
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[fetch_agent_profile] 解析 `agent get` stdout 失败: {e}; raw={}; fallback",
                String::from_utf8_lossy(&output.stdout)
            );
            return fallback();
        }
    };

    // `agent get` 的输出形状由 output::success 包装：{ ok: true, data: <value> }
    // 失败时是 { ok: false, error: "..." }
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        eprintln!("[fetch_agent_profile] `agent get` 返回失败: {err}; fallback");
        return fallback();
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);

    // backend shape: data = [{ list, page, pageSize, total }]
    let list_val = data
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|x| x.get("list"))
        .cloned();
    let list_arr = list_val.as_ref().and_then(|v| v.as_array());
    if list_arr.is_none() {
        eprintln!(
            "[fetch_agent_profile] `agent get` 返回不含 data[0].list 字段，shape 异常；fallback (agentId={agent_id})"
        );
    }

    let matched = list_arr.and_then(|arr| {
        arr.iter()
            .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(agent_id))
            .map(|a| AgentProfile {
                agent_id: Some(agent_id.to_string()),
                name: a.get("name").and_then(|v| v.as_str()).map(String::from),
                profile_description: a
                    .get("profileDescription")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                agent_wallet_address: a
                    .get("agentWalletAddress")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                communication_address: a
                    .get("communicationAddress")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            })
    });
    if list_arr.is_some() && matched.is_none() {
        eprintln!(
            "[fetch_agent_profile] agentId={agent_id} 不在 `agent get` 返回列表中；fallback"
        );
    }
    matched.unwrap_or_else(fallback)
}

// ─── 状态说明 ──────────────────────────────────────────────────────────────
fn status_desc(s: &str) -> &str {
    match s {
        "init"      => "初始化中（等待上链确认）",
        "open"      => "等待接单（Open）",
        "accepted"  => "已接单，卖家执行中（Accepted）",
        "submitted" => "卖家已提交交付，等待买家验收（Submitted）",
        "refused"   => "买家拒绝验收，冻结期内可申请仲裁（Refused）",
        "disputed"      => "仲裁进行中（Disputed）",
        "admin_stopped" => "管理员已停止任务（AdminStopped）",
        "completed" | "complete" => "任务已完成，款项已释放（Complete）",
        "rejected"  => "仲裁结束，任务关闭（Rejected）",
        "close"     => "买家主动关闭（Close）",
        "expired"   => "任务已过期（Expired）",
        _           => "未知状态",
    }
}

fn payment_mode_desc(pm: i32) -> &'static str {
    PaymentMode::from_int(pm).desc()
}

/// 根据角色 + 任务状态，列出当前可执行的 CLI 操作
/// 按 role 路由到对应 flow.rs 的 `available_actions`，
/// single source of truth 留在 buyer/provider/evaluator 各自模块。
fn available_actions(role: &str, status: &str, job_id: &str) -> Vec<String> {
    use state_machine::{Role, Status};
    let status = Status::parse(status);
    match Role::parse(role) {
        Some(Role::Buyer)     => super::buyer::flow::available_actions(&status, job_id),
        Some(Role::Provider)  => super::provider::flow::available_actions(&status, job_id),
        Some(Role::Evaluator) => super::evaluator::flow::available_actions(&status, job_id),
        None => vec![
            format!("onchainos agent status {job_id}         # 查询最新任务状态"),
        ],
    }
}

// ─── 命令处理 ──────────────────────────────────────────────────────────────

pub async fn run(cmd: CommonCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        CommonCommand::Context { job_id, role, agent_id } => {
            run_context(&job_id, &role, &agent_id).await
        }
    }
}

async fn run_context(
    job_id: &str,
    role: &str,
    agent_id: &str,
) -> Result<()> {
    // 校验角色
    if !["buyer", "provider", "evaluator"].contains(&role) {
        bail!("--role 必须是 buyer / provider / evaluator");
    }
    if agent_id.is_empty() {
        bail!("--agent-id 必填：beta 后端要求 agenticId header 非空");
    }

    // 调用后端获取任务详情。base url 由 TaskApiClient::new 内部按
    // OKX_BASE_URL env > TASK_BASE_URL env > 常量 兜底解析，无需 CLI 显式指定。
    let mut client = network::task_api_client::TaskApiClient::new();
    let resp_val = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("无法获取任务详情: {e}"))?;

    // 后端 spec：响应 data 直接是平铺的 task 对象（WalletApiClient 已剥掉 body["data"]）
    let task: TaskDetail = serde_json::from_value(resp_val)
        .map_err(|e| anyhow::anyhow!("解析响应失败: {e}"))?;

    // 拉自己 agent 的资料：name / profileDescription / agentWalletAddress / communicationAddress
    // 三种角色都需要——【你的身份】块要展示钱包地址 + 通信地址；provider 还会用 description 做专业匹配。
    // fetch 出错时返回带 agentId 的 fallback，永不为空。
    let profile = fetch_agent_profile(agent_id).await;

    // 生成上下文
    let ctx_text = build_context(&task, role, agent_id, &profile);
    println!("{ctx_text}");
    Ok(())
}

fn build_context(
    task: &TaskDetail,
    role: &str,
    agent_id: &str,
    profile: &AgentProfile,
) -> String {
    let mut out = String::with_capacity(1024);

    let role_enum = state_machine::Role::parse(role);
    let role_cn = match role_enum {
        Some(state_machine::Role::Buyer)     => "买家（Client）",
        Some(state_machine::Role::Provider)  => "卖家（Provider）",
        Some(state_machine::Role::Evaluator) => "仲裁者（Evaluator）",
        None                                 => role,
    };

    // spec 只回 status 整数，本地用 Status::from_int 派生枚举；展示串走 as_str()。
    let task_status = task
        .status
        .map(state_machine::Status::from_int)
        .unwrap_or_else(|| state_machine::Status::Other("unknown".to_string()));
    let status_str = task_status.as_str().to_string();
    let status_text = format!("{status_str} — {}", status_desc(&status_str));

    // ── 角色声明 ──────────────────────────────────────────────────────────
    out.push_str(&format!("你是任务系统中的{role_cn}。\n\n"));

    // ── 身份信息 ──────────────────────────────────────────────────────────
    // 钱包地址 / 通信地址来自 `agent get` 反查（fetch_agent_profile）；任务详情里的
    // buyerAgentAddress / providerAgentAddress 仍用于下方【买家信息】/【卖家信息】块。
    out.push_str("【你的身份】\n");
    out.push_str(&format!("- 角色：{role_cn}\n"));
    out.push_str(&format!("- AgentID：{agent_id}\n"));
    if let Some(w) = &profile.agent_wallet_address {
        out.push_str(&format!("- 钱包地址：{w}\n"));
    }
    if let Some(c) = &profile.communication_address {
        out.push_str(&format!("- 通信地址：{c}\n"));
    }
    if let Some(n) = &profile.name {
        out.push_str(&format!("- 名称：{n}\n"));
    }
    if let Some(d) = &profile.profile_description {
        out.push_str(&format!("- 描述：{d}\n"));
    }
    out.push('\n');

    // ── 任务详情 ──────────────────────────────────────────────────────────
    out.push_str("【任务详情】\n");
    out.push_str(&format!("- 任务ID：{}\n", task.job_id));
    if let Some(tid) = task.task_id {
        out.push_str(&format!("- 内部ID：{tid}\n"));
    }
    out.push_str(&format!("- 标题：{}\n", task.title));
    out.push_str(&format!("- 描述：{}\n", task.description));

    let amount = task.token_amount.as_deref().unwrap_or("未设置");
    let token  = task.token_address.as_deref().unwrap_or("");
    let symbol = task.token_symbol.as_deref().unwrap_or("UNKNOWN");
    out.push_str(&format!("- 创建预算：{amount} {symbol} （token: {token}）\n"));

    if let Some(pm) = task.payment_mode {
        out.push_str(&format!("- 支付方式：{}\n", payment_mode_desc(pm)));
    }
    let visibility = match task.visibility {
        Some(0) => "公开（Public）",
        Some(1) => "私有（Private）",
        _       => "未知",
    };
    out.push_str(&format!("- 可见性：{visibility}\n"));
    if let Some(chain) = task.chain_id {
        out.push_str(&format!("- 链：chainId={chain}\n"));
    }
    if let Some(score) = task.min_credit_score {
        out.push_str(&format!("- 最低信用分要求：{score}\n"));
    }
    if let Some(dp) = &task.designated_provider {
        out.push_str(&format!("- 指定卖家：{dp}\n"));
    }
    if let Some(ec) = &task.expire_config {
        if let (Some(open_sec), Some(acc_sec)) = (
            ec.get("openExpireSec").and_then(|v| v.as_u64()),
            ec.get("acceptedExpireSec").and_then(|v| v.as_u64()),
        ) {
            out.push_str(&format!(
                "- 有效期：接单截止 {}h，提交截止 {}h\n",
                open_sec / 3600,
                acc_sec / 3600
            ));
        }
    }
    out.push_str(&format!("- 创建时间：{}\n", fmt_unix_secs(task.create_time)));
    out.push_str(&format!("- 更新时间：{}\n", fmt_unix_secs(task.update_time)));
    out.push('\n');

    // ── 当前状态 ──────────────────────────────────────────────────────────
    out.push_str("【当前状态】\n");
    out.push_str(&format!("- {status_text}\n"));
    out.push('\n');

    // ── 买家信息 ──────────────────────────────────────────────────────────
    out.push_str("【买家信息】\n");
    match (&task.buyer_agent_id, &task.buyer_agent_address) {
        (Some(id), Some(addr)) => {
            out.push_str(&format!("- AgentID：{id}\n"));
            out.push_str(&format!("- 通信地址：{addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID：{id}\n")),
        _ => out.push_str("- 信息未知\n"),
    }
    out.push('\n');

    // ── 卖家信息 ──────────────────────────────────────────────────────────
    out.push_str("【卖家信息】\n");
    match (&task.provider_agent_id, &task.provider_agent_address) {
        (Some(id), Some(addr)) => {
            out.push_str(&format!("- AgentID：{id}\n"));
            out.push_str(&format!("- 通信地址：{addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID：{id}\n")),
        _ => out.push_str("- 尚未匹配卖家\n"),
    }
    // ── 专业匹配检查（仅卖家 + open 状态 + 有 profile） ───────────────────
    if role_enum == Some(state_machine::Role::Provider)
        && task_status == state_machine::Status::Open
    {
        if let Some(desc) = &profile.profile_description {
            out.push_str("【⚠️ 第一步：专业匹配检查（必做，不得跳过）】\n");
            out.push_str(&format!("- 你的 Provider 描述：{desc}\n"));
            out.push_str(&format!("- 任务标题：{}\n", task.title));
            out.push_str(&format!("- 任务描述：{}\n", task.description));
            out.push('\n');
            out.push_str("判断：上述「Provider 描述」和「任务领域」是否匹配？\n");
            out.push_str("- 匹配（同一专业领域）→ 进入下方「按可见性分流」继续协商\n");
            out.push_str("- 不匹配（领域明显不同，如 DeFi trading vs 合约审计 / 前端 / 文案）→ **必须拒绝**：\n");
            out.push_str("  1. 调用 `xmtp_send` 工具发送拒绝消息（模板如下）\n");
            out.push_str("  2. **禁止**执行 onchainos agent apply 或任何后续操作\n\n");
            out.push_str("拒绝回复模板（通过 `xmtp_send` 工具发送，`content` 字段 = 下方纯自然语言正文）：\n");
            out.push_str(&format!(
                "抱歉，此任务（{}）超出我的专业领域（{}），无法承接。祝您找到合适的卖家。\n\n",
                task.title, desc
            ));
            out.push_str("注意：`content` 是纯自然语言正文，不要加任何 text header（如 `jobId: / 来自: ... / 类型: REPLY` 之类）。XMTP 插件会自动把 content 包装成 a2a-agent-chat envelope。\n\n");
        }

        // 专业匹配通过后，按 task.visibility 给不同动作引导（VisibilityEnum: 0=PUBLIC / 1=PRIVATE）
        let buyer_id = task.buyer_agent_id.as_deref().unwrap_or("<task.buyerAgentId>");
        let agent_id_hint = profile.agent_id.as_deref().unwrap_or("<你的agentId>");
        out.push_str("【⚠️ 第二步：按可见性分流（匹配通过才走这里）】\n\n");
        if task.visibility == Some(0) {
            // 公开任务 → provider 主动建群
            out.push_str("当前任务**可见性 = 公开（Public）** → 你需要**主动联系买家发起协商**：\n\n");
            out.push_str("1. 调 `xmtp_start_conversation` 工具建群 + 创建 sub session（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.7）：\n");
            out.push_str(&format!(
                "   - 参数：`myAgentId={agent_id_hint}`，`toAgentId={buyer_id}`（买家 agentId），`jobId={}`\n",
                task.job_id
            ));
            out.push_str("   - 成功返回 `sessionKey`（新 sub 的 key，下面 step 2 直接用，**不要再调 `session_status`**——bootstrap 阶段 `session_status` 可能返回当前所在 user session 的 key，会拿错）+ `xmtpGroupId`\n");
            out.push_str("2. 立即调 **`xmtp_send`**（参数 `sessionKey` = step 1 `xmtp_start_conversation` 返回值的 `sessionKey` 字段原值，`content` = 协商三项确认提问）\n\n");
            out.push_str("🛑 **必须用 `xmtp_send`，禁止用 `xmtp_dispatch_session` / `xmtp_dispatch_user` / `xmtp_prompt_user` 替代**——给 peer agent 发 a2a-agent-chat 业务消息**只有 `xmtp_send` 一种路径**。看到「建立协商通道 / 派发到 sub / dispatch」这种语感**也只能选 `xmtp_send`**，工具名里有 `dispatch` 不代表选 `dispatch_session`。`xmtp_dispatch_session` 是 user→sub `[USER_DECISION_RELAY]` 决策回传专用，content 必字面以 `[USER_DECISION_RELAY] 用户决策：` 开头，跟协商首条 a2a-agent-chat 形态完全不符。\n\n");
            out.push_str("协商三项（一条 `xmtp_send` 一次问完）：\n");
            out.push_str("  1) 任务内容和验收标准是否在能力范围内\n");
            out.push_str("  2) 价格 / 币种 USDT or USDG\n");
            out.push_str("  3) 支付方式（escrow / non_escrow）\n\n");
        } else {
            // 私有任务 → provider 被动等买家先来
            out.push_str("当前任务**可见性 = 私有（Private）** → 你**不要主动建群**：\n\n");
            out.push_str("- 私有任务由买家选定 provider，必须**等买家先发** a2a-agent-chat envelope（你才有联系对方的入口）\n");
            out.push_str("- 收到买家首条 inquire 后，按上面「专业匹配检查」走，匹配通过则在已有 sub session 里 `xmtp_send` 回协商三项\n");
            out.push_str("- **禁止**调 `xmtp_start_conversation` 主动建群——私有任务没有这个权限\n\n");
        }
    }

    // ── 下一步动作 ────────────────────────────────────────────────────────
    let actions = available_actions(role, &status_str, &task.job_id);
    out.push_str("【下一步动作】（先调 next-action 拿当前 status 的完整剧本，按剧本走，不要绕过 next-action 直接调 CLI）\n");
    for a in &actions {
        out.push_str(&format!("- {a}\n"));
    }
    out.push('\n');

    // ── 必须加载的角色指南 ──────────────────────────────────────────────
    let skill_file = match role {
        "buyer"     => "client.md",
        "provider"    => "provider.md",
        "evaluator" => "evaluator.md",
        _           => "",
    };
    if !skill_file.is_empty() {
        out.push_str("【⚠️ 必须立即执行】\n");
        out.push_str(&format!(
            "请立即读取角色指南 skills/okx-agent-task/{skill_file}（与 skills/okx-agent-task/SKILL.md 同目录），该文件包含完整的协商规则和接单流程。\n"
        ));
    }

    out
}
