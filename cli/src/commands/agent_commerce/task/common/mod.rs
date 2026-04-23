//! common — 任务系统通用查询命令
//!
//! 核心命令：`context`
//! 根据 job_id + 角色，从后端拉取任务详情，生成结构化自然语言上下文，
//! 供大模型（openclaw buyer/seller/evaluator AI）理解当前任务状态。

use anyhow::{bail, Result};
use clap::Subcommand;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub mod dispute_upload;
pub mod network;
pub mod rate_agent;

use crate::commands::Context;

// ─── 链常量 ──────────────────────────────────────────────────────────────

/// XLayer chain ID（用于任务系统合约部署链）
pub const XLAYER_CHAIN_ID: i32 = 196;
/// XLayer chain index 字符串形式（用于钱包 API）
pub const XLAYER_CHAIN_INDEX: &str = "196";
/// XLayer chain name（用于 wallet_store 地址查找，wallets.json 中 chainIndex=196 的 chainName）
pub const XLAYER_CHAIN_NAME: &str = "okb";

// ─── 支付模式常量 ────────────────────────────────────────────────────────

/// 担保支付：资金锁定在合约中
pub const PAYMENT_MODE_ESCROW: &str = "escrow";
/// 非担保支付：任务完成后买家手动转账
pub const PAYMENT_MODE_NON_ESCROW: &str = "non_escrow";
/// x402 按需微支付
pub const PAYMENT_MODE_X402: &str = "x402";

// ─── 支付模式 int ↔ str 映射 ────────────────────────────────────────────

/// 后端 paymentMode int 值
pub const PAYMENT_MODE_INT_ESCROW: i32 = 0;
pub const PAYMENT_MODE_INT_DIRECT: i32 = 1;
pub const PAYMENT_MODE_INT_X402: i32 = 2;

/// str → int（用于 setPaymentMode 接口）
pub fn payment_mode_to_int(mode: &str) -> i32 {
    match mode {
        PAYMENT_MODE_ESCROW | "0" => PAYMENT_MODE_INT_ESCROW,
        PAYMENT_MODE_NON_ESCROW | "direct" | "1" => PAYMENT_MODE_INT_DIRECT,
        PAYMENT_MODE_X402 | "2" => PAYMENT_MODE_INT_X402,
        _ => PAYMENT_MODE_INT_ESCROW,
    }
}

/// int → str（用于展示）
pub fn payment_mode_to_str(mode: i32) -> &'static str {
    match mode {
        PAYMENT_MODE_INT_ESCROW => PAYMENT_MODE_ESCROW,
        PAYMENT_MODE_INT_DIRECT => PAYMENT_MODE_NON_ESCROW,
        PAYMENT_MODE_INT_X402 => PAYMENT_MODE_X402,
        _ => PAYMENT_MODE_ESCROW,
    }
}

// ─── CLI 定义 ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum CommonCommand {
    /// 查询当前 buyer agent 在 ws-mock 身份系统中的注册信息（ERC-8004）
    ///
    /// 示例：
    ///   onchainos agent get
    ///   onchainos agent get --ws-url ws://127.0.0.1:9000
    Get {
        /// ws-mock server 地址（默认 ws://127.0.0.1:9000）
        #[arg(long, default_value = "ws://127.0.0.1:9000")]
        ws_url: String,

        /// 查询指定地址（不传则读 ~/.openclaw/ws-mock-addresses.json 中的 default）
        #[arg(long)]
        addr: Option<String>,
    },

    /// 查询任务上下文，输出供大模型使用的结构化自然语言描述
    ///
    /// 示例：
    ///   onchainos agent context task-001 --role buyer
    ///   onchainos agent context task-001 --role seller --agent-id mock-seller-001
    Context {
        /// 任务 ID（jobId），如 task-001 或 0x1a2b...
        job_id: String,

        /// 调用者角色：buyer | seller | evaluator
        #[arg(long, default_value = "buyer")]
        role: String,

        /// 调用者的 AgentID（可选，用于标注身份）
        #[arg(long)]
        agent_id: Option<String>,

        /// 调用者钱包地址（可选）
        #[arg(long)]
        address: Option<String>,

        /// mock-api 地址（默认 http://127.0.0.1:9001）
        #[arg(long, default_value = "http://127.0.0.1:9001")]
        api_url: String,
    },
}

// ─── 任务详情响应结构（对应 mock-api / 真实后端响应） ──────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskResp {
    code: i32,
    data: Option<TaskRespData>,
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskRespData {
    task: TaskDetail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskDetail {
    job_id: String,
    task_id: Option<String>,
    title: String,
    description: String,
    description_summary: Option<String>,
    token_address: Option<String>,
    token_amount: Option<String>,
    payment_type: Option<i32>,
    open_type: Option<i32>,
    status: Option<i32>,
    status_str: Option<String>,
    chain_id: Option<i32>,
    min_credit_score: Option<f64>,
    designated_provider: Option<String>,
    buyer_agent_address: Option<String>,
    buyer_agent_id: Option<String>,
    provider_agent_address: Option<String>,
    provider_agent_id: Option<String>,
    group_id: Option<String>,
    expire_config: Option<serde_json::Value>,
    create_time: Option<String>,
    update_time: Option<String>,
}

// ─── Agent 资料响应结构 ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentListResp {
    code: Option<String>,
    data: Option<AgentListData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentListData {
    list: Vec<AgentProfile>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AgentProfile {
    #[allow(dead_code)]
    agent_id: Option<String>,
    name: Option<String>,
    profile_description: Option<String>,
}

/// [MOCK] 查询 agent 资料（name / profileDescription）
///
/// TODO: 替换为真实后端接口。当前返回 mock 数据。
async fn fetch_agent_profile(_agent_id: &str, api_url: &str) -> Option<AgentProfile> {
    let url = format!("{api_url}/priapi/v1/aieco/agent/list");
    let resp: Result<AgentListResp, _> = match reqwest::get(&url).await {
        Ok(r) => r.json().await,
        Err(_) => return Some(mock_profile()),
    };
    match resp {
        Ok(r) if r.code.as_deref() == Some("0") => r.data.and_then(|d| d.list.into_iter().next()),
        _ => Some(mock_profile()),
    }
}

fn mock_profile() -> AgentProfile {
    AgentProfile {
        agent_id: Some("10001".to_string()),
        name: Some("My DeFi Agent".to_string()),
        profile_description: Some("A DeFi trading agent".to_string()),
    }
}

// ─── 状态说明 ──────────────────────────────────────────────────────────────

fn status_desc(s: &str) -> &str {
    match s {
        "init"      => "初始化中（等待上链确认）",
        "open"      => "等待接单（Open）",
        "accepted"  => "已接单，卖家执行中（Accepted）",
        "submitted" => "卖家已提交交付，等待买家验收（Submitted）",
        "refused"   => "买家拒绝验收，冻结期内可申请仲裁（Refused）",
        "disputed"  => "仲裁进行中（Disputed）",
        "complete"  => "任务已完成，款项已释放（Complete）",
        "rejected"  => "仲裁结束，任务关闭（Rejected）",
        "close"     => "买家主动关闭（Close）",
        "expired"   => "任务已过期（Expired）",
        _           => "未知状态",
    }
}

fn payment_type_desc(pt: i32) -> &'static str {
    match pt {
        0 => "托管支付（Escrow）",
        1 => "非托管支付（Non-Escrow）",
        2 => "x402 按需支付",
        _ => "未设置",
    }
}

/// 根据角色 + 任务状态，列出当前可执行的 CLI 操作
fn available_actions(role: &str, status: &str, job_id: &str) -> Vec<String> {
    match (role, status) {
        ("buyer", "open") => vec![
            format!("onchainos agent recommend {job_id}      # 查看推荐卖家"),
            format!("onchainos agent confirm-accept {job_id} --provider <addr>  # 接受卖家并注资"),
            format!("onchainos agent close {job_id}          # 关闭任务"),
            format!("onchainos agent set-public {job_id}     # 转为公开任务"),
        ],
        ("buyer", "submitted") => vec![
            format!("onchainos agent complete {job_id}       # 验收通过，释放款项"),
            format!("onchainos agent reject {job_id} --reason <reason>  # 拒绝验收"),
        ],
        ("buyer", "disputed") => vec![
            format!("onchainos agent dispute evidence {job_id} --summary <摘要>  # 提交证据"),
        ],
        ("seller", "open") => vec![
            format!("onchainos agent apply {job_id} --token-amount <price> --token-symbol USDT --agent-id <agentId>  # 申请接单"),
        ],
        ("seller", "accepted") => vec![
            format!("onchainos agent deliver {job_id} --file <deliverable> --message <msg>  # 提交交付"),
        ],
        ("seller", "refused") => vec![
            format!("onchainos agent dispute raise {job_id} --reason <reason>  # 发起仲裁"),
            format!("onchainos agent agree-refund {job_id}  # 同意退款"),
        ],
        ("seller", "disputed") => vec![
            format!("onchainos agent dispute evidence {job_id} --summary <摘要>  # 提交证据"),
        ],
        ("evaluator", "disputed") => vec![
            format!("onchainos agent dispute info {job_id}        # 查看仲裁详情"),
            format!("onchainos agent dispute vote {job_id} --side 1 --reason <reason>  # 投票支持卖家"),
            format!("onchainos agent dispute vote {job_id} --side 0 --reason <reason>  # 投票支持买家"),
        ],
        _ => vec![
            format!("onchainos agent status {job_id}         # 查询最新任务状态"),
        ],
    }
}

// ─── 命令处理 ──────────────────────────────────────────────────────────────

pub async fn run(cmd: CommonCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        CommonCommand::Get { ws_url, addr } => run_get(&ws_url, addr.as_deref()).await,
        CommonCommand::Context { job_id, role, agent_id, address, api_url } => {
            run_context(&job_id, &role, agent_id.as_deref(), address.as_deref(), &api_url).await
        }
    }
}

async fn run_get(ws_url: &str, addr_override: Option<&str>) -> Result<()> {
    // 解析要查询的地址
    let addr = if let Some(a) = addr_override {
        a.to_string()
    } else {
        // 读 ~/.openclaw/ws-mock-addresses.json → {"default": "0x..."}
        let path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?
            .join(".openclaw/ws-mock-addresses.json");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {e}\n提示: 先连接 ws-mock 使 openclaw gateway 注册地址", path.display()))?;
        let v: serde_json::Value = serde_json::from_str(&raw)?;
        v["default"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("ws-mock-addresses.json 中未找到 default 字段"))?
            .to_string()
    };

    // 连接 ws-mock
    let (mut ws, _) = connect_async(ws_url)
        .await
        .map_err(|e| anyhow::anyhow!("无法连接 {ws_url}: {e}\n提示: 先启动 ws-mock server"))?;

    // 发送 LookupAddr
    let req = serde_json::json!({ "action": "LookupAddr", "addr": addr });
    ws.send(Message::Text(req.to_string().into())).await?;

    // 等待 addr_lookup 响应（超时 3s）
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(Ok(Message::Text(text))) = ws.next().await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if v["type"].as_str() == Some("addr_lookup") {
                    return Some(v);
                }
            }
        }
        None
    })
    .await
    .map_err(|_| anyhow::anyhow!("LookupAddr 超时（3s），ws-mock server 无响应"))?;

    let resp = result.ok_or_else(|| anyhow::anyhow!("ws-mock 连接关闭，未收到 addr_lookup 响应"))?;

    // 输出结果
    match resp.get("identity") {
        Some(identity) if !identity.is_null() => {
            println!("{}", serde_json::to_string_pretty(identity)?);
        }
        _ => {
            println!("{}", serde_json::json!({
                "registered": false,
                "comm_addr": addr,
                "msg": "该地址在 ws-mock 身份系统中尚未注册（ERC-8004）"
            }));
        }
    }

    let _ = ws.close(None).await;
    Ok(())
}

async fn run_context(
    job_id: &str,
    role: &str,
    agent_id: Option<&str>,
    address: Option<&str>,
    api_url: &str,
) -> Result<()> {
    // 校验角色
    if !["buyer", "seller", "evaluator"].contains(&role) {
        bail!("--role 必须是 buyer / seller / evaluator");
    }

    // 调用后端获取任务详情（带身份 header，便于 mock-api 日志区分调用方）
    let client = network::task_api_client::TaskApiClient::with_base_url(api_url.to_string());
    let url = format!("{api_url}/priapi/v1/aieco/task/{job_id}");
    let resp_val = client
        .get_with_identity(&url, agent_id.unwrap_or(""), address.unwrap_or(""))
        .await
        .map_err(|e| anyhow::anyhow!("无法获取任务详情（{api_url}）: {e}"))?;

    let body: TaskResp = serde_json::from_value(resp_val)
        .map_err(|e| anyhow::anyhow!("解析响应失败: {e}"))?;

    let task = body.data
        .ok_or_else(|| anyhow::anyhow!("响应中无 data 字段"))?
        .task;

    // 卖家额外拉取 agent 资料（name / profileDescription）
    let profile = if role == "seller" {
        fetch_agent_profile(agent_id.unwrap_or(""), api_url).await
    } else {
        None
    };

    // 生成上下文
    let ctx_text = build_context(&task, role, agent_id, address, profile.as_ref());
    println!("{ctx_text}");
    Ok(())
}

fn build_context(
    task: &TaskDetail,
    role: &str,
    agent_id: Option<&str>,
    address: Option<&str>,
    profile: Option<&AgentProfile>,
) -> String {
    let mut out = String::with_capacity(1024);

    let role_cn = match role {
        "buyer"     => "买家（Client）",
        "seller"    => "卖家（Provider）",
        "evaluator" => "仲裁者（Evaluator）",
        _           => role,
    };

    let status_raw = task.status_str.as_deref().unwrap_or("unknown");
    let status_text = format!("{status_raw} — {}", status_desc(status_raw));

    // ── 角色声明 ──────────────────────────────────────────────────────────
    out.push_str(&format!("你是任务系统中的{role_cn}。\n\n"));

    // ── 身份信息 ──────────────────────────────────────────────────────────
    out.push_str("【你的身份】\n");
    out.push_str(&format!("- 角色：{role_cn}\n"));
    if let Some(id) = agent_id {
        out.push_str(&format!("- AgentID：{id}\n"));
    }
    if let Some(addr) = address {
        out.push_str(&format!("- 钱包地址：{addr}\n"));
    }
    if let Some(p) = profile {
        if let Some(n) = &p.name {
            out.push_str(&format!("- 名称：{n}\n"));
        }
        if let Some(d) = &p.profile_description {
            out.push_str(&format!("- Provider 描述：{d}\n"));
        }
    }
    out.push('\n');

    // ── 任务详情 ──────────────────────────────────────────────────────────
    out.push_str("【任务详情】\n");
    out.push_str(&format!("- 任务ID：{}\n", task.job_id));
    if let Some(tid) = &task.task_id {
        if tid != &task.job_id {
            out.push_str(&format!("- 内部ID：{tid}\n"));
        }
    }
    out.push_str(&format!("- 标题：{}\n", task.title));
    out.push_str(&format!("- 描述：{}\n", task.description));
    if let Some(summary) = &task.description_summary {
        if !summary.is_empty() {
            out.push_str(&format!("- 摘要：{summary}\n"));
        }
    }

    let amount = task.token_amount.as_deref().unwrap_or("未设置");
    let token  = task.token_address.as_deref().unwrap_or("");
    out.push_str(&format!("- 预算：{amount} （token: {token}）\n"));

    if let Some(pt) = task.payment_type {
        out.push_str(&format!("- 支付方式：{}\n", payment_type_desc(pt)));
    }
    let open_type = match task.open_type {
        Some(1) => "公开（Public）",
        _       => "私有（Private）",
    };
    out.push_str(&format!("- 可见性：{open_type}\n"));
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
    out.push_str(&format!("- 创建时间：{}\n", task.create_time.as_deref().unwrap_or("—")));
    out.push_str(&format!("- 更新时间：{}\n", task.update_time.as_deref().unwrap_or("—")));
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
            out.push_str(&format!("- 地址：{addr}\n"));
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
            out.push_str(&format!("- 地址：{addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID：{id}\n")),
        _ => out.push_str("- 尚未匹配卖家\n"),
    }
    if let Some(gid) = &task.group_id {
        out.push_str(&format!("- 聊天会话ID：{gid}\n"));
    }
    out.push('\n');

    // ── 专业匹配检查（仅卖家 + open 状态 + 有 profile） ───────────────────
    if role == "seller" && status_raw == "open" {
        if let Some(p) = profile {
            if let Some(desc) = &p.profile_description {
                out.push_str("【⚠️ 第一步：专业匹配检查（必做，不得跳过）】\n");
                out.push_str(&format!("- 你的 Provider 描述：{desc}\n"));
                out.push_str(&format!("- 任务标题：{}\n", task.title));
                out.push_str(&format!("- 任务描述：{}\n", task.description));
                out.push('\n');
                out.push_str("判断：上述「Provider 描述」和「任务领域」是否匹配？\n");
                out.push_str("- 匹配（同一专业领域）→ 进入下方「可执行操作」继续协商\n");
                out.push_str("- 不匹配（领域明显不同，如 DeFi trading vs 合约审计 / 前端 / 文案）→ **必须拒绝**：\n");
                out.push_str("  1. 以 header 格式回复拒绝消息（示例如下）\n");
                out.push_str("  2. **禁止**执行 onchainos agent apply 或任何后续操作\n\n");
                out.push_str("拒绝回复模板（必须包含 header）：\n");
                out.push_str(&format!("jobId:  {}\n", task.job_id));
                out.push_str("来自:   <你的 agentId> [PROVIDER]\n");
                out.push_str("类型:   REPLY\n");
                out.push_str("会话:   <来源消息的会话 ID>\n");
                out.push_str("----------------------------------------\n");
                out.push_str(&format!(
                    "抱歉，此任务（{}）超出我的专业领域（{}），无法承接。祝您找到合适的卖家。\n\n",
                    task.title, desc
                ));
            }
        }
    }

    // ── 可执行操作 ────────────────────────────────────────────────────────
    let actions = available_actions(role, status_raw, &task.job_id);
    out.push_str("【你当前可以执行的操作】\n");
    for a in &actions {
        out.push_str(&format!("- {a}\n"));
    }
    out.push('\n');

    // ── 必须加载的角色指南 ──────────────────────────────────────────────
    let skill_file = match role {
        "buyer"     => "client.md",
        "seller"    => "provider.md",
        "evaluator" => "evaluator.md",
        _           => "",
    };
    if !skill_file.is_empty() {
        out.push_str("【⚠️ 必须立即执行】\n");
        out.push_str(&format!(
            "请立即读取角色指南 {skill_file}（与 SKILL.md 同目录），该文件包含完整的协商规则和接单流程。\n"
        ));
    }

    out
}
