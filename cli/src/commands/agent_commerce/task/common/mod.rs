//! common — 任务系统通用查询命令
//!
//! 核心命令：`context`
//! 根据 job_id + 角色，从后端拉取任务详情，生成结构化自然语言上下文，
//! 供大模型（openclaw buyer/seller/evaluator AI）理解当前任务状态。

use anyhow::{bail, Result};
use clap::Subcommand;
use serde::Deserialize;

use crate::commands::Context;

// ─── CLI 定义 ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum CommonCommand {
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
            format!("onchainos agent confirm {job_id}        # 确认接单（链上）"),
        ],
        ("seller", "accepted") => vec![
            format!("onchainos agent deliver {job_id} --file <deliverable>  # 提交交付"),
        ],
        ("seller", "refused") => vec![
            format!("onchainos agent dispute raise {job_id} --reason <reason>  # 申请仲裁"),
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
        CommonCommand::Context { job_id, role, agent_id, address, api_url } => {
            run_context(&job_id, &role, agent_id.as_deref(), address.as_deref(), &api_url).await
        }
    }
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

    // 调用后端获取任务详情
    let url = format!("{api_url}/api/v1/task/{job_id}");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| anyhow::anyhow!("无法连接 mock-api（{api_url}）: {e}\n提示: 先启动 ./target/release/mock-api"))?;

    let body: TaskResp = resp.json().await
        .map_err(|e| anyhow::anyhow!("解析响应失败: {e}"))?;

    if body.code != 0 {
        bail!("后端错误 code={}: {}", body.code, body.msg.unwrap_or_default());
    }

    let task = body.data
        .ok_or_else(|| anyhow::anyhow!("响应中无 data 字段"))?
        .task;

    // 生成上下文
    let ctx_text = build_context(&task, role, agent_id, address);
    println!("{ctx_text}");
    Ok(())
}

fn build_context(
    task: &TaskDetail,
    role: &str,
    agent_id: Option<&str>,
    address: Option<&str>,
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

    // ── 可执行操作 ────────────────────────────────────────────────────────
    let actions = available_actions(role, status_raw, &task.job_id);
    out.push_str("【你当前可以执行的操作】\n");
    for a in &actions {
        out.push_str(&format!("- {a}\n"));
    }

    out
}
