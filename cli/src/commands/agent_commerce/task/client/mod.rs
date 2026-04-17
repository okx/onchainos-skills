use anyhow::{bail, Result};
use clap::Subcommand;

use crate::commands::Context;

// ─── mock-api helpers ──────────────────────────────────────────────────────

fn task_api_url() -> String {
    std::env::var("TASK_API_URL").unwrap_or_else(|_| "http://127.0.0.1:9001".to_string())
}

/// 解析 "72h" / "30m" / "3600" → 秒
fn parse_duration_secs(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(h) = s.strip_suffix('h') {
        Ok(h.parse::<u64>()? * 3600)
    } else if let Some(m) = s.strip_suffix('m') {
        Ok(m.parse::<u64>()? * 60)
    } else {
        Ok(s.parse::<u64>()?)
    }
}

/// 校验货币符号
fn validate_currency(currency: &str) -> Result<()> {
    match currency.to_uppercase().as_str() {
        "USDT" | "USDG" => Ok(()),
        other => bail!("不支持的代币: {other}，仅支持 USDT 和 USDG"),
    }
}

// ─── task subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Create a new task (Client only)
    Create {
        #[arg(long)]
        description: String,
        #[arg(long = "description-summary")]
        description_summary: Option<String>,
        #[arg(long)]
        budget: f64,
        #[arg(long)]
        currency: String,
        #[arg(long = "deadline-open")]
        deadline_open: String,
        #[arg(long = "deadline-submit")]
        deadline_submit: String,
        #[arg(long)]
        title: Option<String>,
    },
    /// Get recommended providers for a task
    Recommend {
        job_id: String,
    },
    /// Get current task status
    Status {
        job_id: String,
    },
    /// List tasks
    List {
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "1")]
        page: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Client confirms provider and stakes funds into escrow
    ConfirmAccept {
        job_id: String,
        #[arg(long)]
        provider: String,
    },
    /// Client rejects provider application
    RejectApply {
        job_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        reason: String,
    },
    /// Provider confirms on-chain acceptance
    Confirm {
        job_id: String,
    },
    /// Provider submits deliverable
    Deliver {
        job_id: String,
        #[arg(long)]
        file: String,
        #[arg(long)]
        message: Option<String>,
    },
    /// Client confirms task complete and releases payment
    Complete {
        job_id: String,
    },
    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Client closes task (only valid while Open)
    Close {
        job_id: String,
    },
    /// Client converts private task to public listing
    SetPublic {
        job_id: String,
    },
    /// AI-assisted deliverable quality assessment
    AiEvaluate {
        job_id: String,
    },
    /// Initialize config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Initialize configuration
    Init,
    /// Show current configuration
    Show,
}

// ─── negotiate subcommands ─────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum NegotiateCommand {
    /// Client initiates negotiation with a provider
    Start {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        message: String,
    },
    /// Provider sends a quote to client
    Quote {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        price: f64,
        #[arg(long)]
        currency: String,
        #[arg(long = "delivery-hours")]
        delivery_hours: u32,
        #[arg(long = "skill-id")]
        skill_id: Option<String>,
        #[arg(long)]
        message: Option<String>,
    },
    /// Either party counters with a new price
    Counter {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        price: f64,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Either party accepts current terms
    Accept {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        price: f64,
        #[arg(long = "delivery-hours")]
        delivery_hours: u32,
        #[arg(long = "payment-mode", default_value = "escrow")]
        payment_mode: String,
    },
    /// Either party rejects and ends negotiation
    Reject {
        #[arg(long)]
        to: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        reason: String,
    },
}

// ─── dispute subcommands ───────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DisputeCommand {
    /// Provider raises a dispute after client rejects deliverable
    Raise {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Either party submits evidence during dispute
    Evidence {
        job_id: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        file: Option<String>,
        #[arg(long = "type")]
        evidence_type: Option<String>,
    },
    /// Evaluator retrieves dispute details
    Info {
        dispute_id: String,
    },
    /// Evaluator votes on dispute outcome
    Vote {
        dispute_id: String,
        #[arg(long)]
        side: u8,
        #[arg(long)]
        reason: String,
    },
    /// Either party appeals the arbitration result
    Appeal {
        job_id: String,
        #[arg(long)]
        reason: String,
    },
}

// ─── handlers (TODO) ──────────────────────────────────────────────────────

pub async fn run_task(cmd: TaskCommand, _ctx: &Context) -> Result<()> {
    let api = task_api_url();
    let client = reqwest::Client::new();

    match cmd {
        // ── 创建任务 (create → sign → broadcast) ────────────────────────────
        TaskCommand::Create {
            description, description_summary, budget, currency,
            deadline_open, deadline_submit, title,
        } => {
            validate_currency(&currency)?;

            let open_secs   = parse_duration_secs(&deadline_open)
                .map_err(|_| anyhow::anyhow!("--deadline-open 格式错误，例如 72h 或 3600"))?;
            let submit_secs = parse_duration_secs(&deadline_submit)
                .map_err(|_| anyhow::anyhow!("--deadline-submit 格式错误，例如 48h 或 3600"))?;

            let title_str = title.unwrap_or_else(|| description.chars().take(30).collect());
            let summary   = description_summary
                .unwrap_or_else(|| description.chars().take(200).collect());

            // ── Step 1: 生成 calldata (POST /api/v1/task/create) ────────
            let body = serde_json::json!({
                "title":              title_str,
                "description":        description,
                "description_summary": summary,
                "paymentTokenSymbol": currency.to_uppercase(),
                "paymentTokenAmount": budget.to_string(),
                "chainId":            196,
                "expireConfig": {
                    "acceptDeadline":    open_secs,
                    "submittedDeadline": submit_secs
                },
                "paymentMode":        0,
                "visibility":         0
            });

            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/create"))
                .json(&body)
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("创建失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
            }

            let job_id   = resp["data"]["jobId"].as_str().unwrap_or("?");
            let uop_data = &resp["data"]["uopData"];
            let uop_hash = uop_data["uopHash"].as_str().unwrap_or("?");

            println!("✓ Calldata 已生成");
            println!("  jobId:   {job_id}");
            println!("  uopHash: {uop_hash}");

            // ── Step 2: 签名 uopHash (TODO) ─────────────────────────────
            // TODO: 调用 onchainos agent wallet 对 uopHash 进行签名
            //   let sign_result = onchainos_agent_wallet_sign(uop_hash, &uop_data["extraData"]).await?;
            //   sign_result 包含 msgForSign { sessionCert, signature }
            println!("  [TODO] 等待 onchainos agent wallet 签名 uopHash...");

            // ── Step 3: 广播上链 (POST /api/v1/task/broadcast) ──────────
            // TODO: 签名完成后调用 broadcast
            //   let broadcast_body = serde_json::json!({
            //       "accountId":  wallet_account_id,
            //       "address":    wallet_address,
            //       "chainIndex": "196",
            //       "bizUniqKey": format!("create-task-{job_id}"),
            //       "extraData":  build_extra_data(&uop_data["extraData"], &sign_result)
            //   });
            //   let bc_resp: serde_json::Value = client
            //       .post(format!("{api}/api/v1/task/broadcast"))
            //       .json(&broadcast_body)
            //       .send().await?
            //       .json().await?;
            //   let tx_hash = bc_resp["data"][0]["txHash"].as_str().unwrap_or("?");
            println!("  [TODO] 广播上链...");
            println!();
            println!("  任务创建流程: calldata ✓ → 签名 [TODO] → 广播 [TODO]");
            println!("  jobId: {job_id}");
            println!();
            println!("下一步: onchainos agent recommend {job_id}");
        }

        // ── 查询推荐卖家 ────────────────────────────────────────────────────
        TaskCommand::Recommend { job_id } => {
            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/{job_id}/match"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("{}", resp["msg"].as_str().unwrap_or("error"));
            }
            let recs = resp["data"]["recommendations"].as_array()
                .cloned().unwrap_or_default();
            println!("推荐卖家列表（共 {} 个）：", recs.len());
            for (i, r) in recs.iter().enumerate() {
                println!("  {}. AgentID: {}  匹配分: {}  信用分: {}",
                    i + 1,
                    r["providerAgentId"].as_str().unwrap_or("?"),
                    r["matchScore"].as_f64().unwrap_or(0.0),
                    r["creditScore"].as_i64().unwrap_or(0),
                );
                println!("     能力: {}", r["capabilitySummary"].as_str().unwrap_or(""));
                println!("     地址: {}", r["providerAddress"].as_str().unwrap_or("?"));
            }
        }

        // ── 任务状态 ────────────────────────────────────────────────────────
        TaskCommand::Status { job_id } => {
            let resp: serde_json::Value = client
                .get(format!("{api}/api/v1/task/{job_id}"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("任务不存在: {job_id}");
            }
            let t = &resp["data"]["task"];
            println!("任务状态: {}", t["statusStr"].as_str().unwrap_or("?"));
            println!("  jobId:    {job_id}");
            println!("  标题:     {}", t["title"].as_str().unwrap_or("?"));
            println!("  预算:     {} {}", t["tokenAmount"].as_str().unwrap_or("?"), "USDT");
            println!("  买家:     {}", t["buyerAgentId"].as_str().unwrap_or("?"));
            if let Some(pid) = t["providerAgentId"].as_str() {
                println!("  卖家:     {pid}");
            }
            println!("  更新时间: {}", t["updateTime"].as_str().unwrap_or("?"));
        }

        // ── 任务列表 ────────────────────────────────────────────────────────
        TaskCommand::List { role, status, page, limit } => {
            let url = if role.as_deref() == Some("provider") || role.as_deref() == Some("client") {
                let r = role.as_deref().unwrap_or("client");
                format!("{api}/api/v1/tasks/my?role={r}&page={page}&page_size={limit}")
            } else {
                let mut u = format!("{api}/api/v1/task/list?page={page}&page_size={limit}");
                if let Some(s) = &status { u.push_str(&format!("&status={s}")); }
                u
            };
            let resp: serde_json::Value = client.get(&url).send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;
            let tasks = resp["data"]["list"].as_array().cloned().unwrap_or_default();
            let total = resp["data"]["total"].as_u64().unwrap_or(0);
            println!("任务列表（共 {total} 个，第 {page} 页）：");
            for t in &tasks {
                println!("  [{}] {} — {} USDT",
                    t["statusStr"].as_str().unwrap_or("?"),
                    t["jobId"].as_str().unwrap_or("?"),
                    t["tokenAmount"].as_str().unwrap_or("?"),
                );
                println!("       {}", t["title"].as_str().unwrap_or("?"));
            }
        }

        // ── confirm-accept ──────────────────────────────────────────────────
        TaskCommand::ConfirmAccept { job_id, provider } => {
            let body = serde_json::json!({ "providerAddress": provider, "providerAgentId": provider });
            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/{job_id}/accept"))
                .json(&body).send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;
            if resp["code"] != 0 { bail!("{}", resp["msg"].as_str().unwrap_or("error")); }
            println!("✓ 已接受卖家 {provider}，任务状态 → accepted");
            println!("  calldata: {}", resp["data"]["calldata"].as_str().unwrap_or("?"));
        }

        // ── complete ────────────────────────────────────────────────────────
        TaskCommand::Complete { job_id } => {
            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/{job_id}/complete"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;
            if resp["code"] != 0 { bail!("{}", resp["msg"].as_str().unwrap_or("error")); }
            println!("✓ 任务验收通过，状态 → complete，款项已释放");
        }

        // ── reject deliverable ──────────────────────────────────────────────
        TaskCommand::Reject { job_id, reason } => {
            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/{job_id}/refuse"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;
            if resp["code"] != 0 { bail!("{}", resp["msg"].as_str().unwrap_or("error")); }
            println!("✓ 已拒绝验收（原因：{reason}），状态 → refused");
            println!("  卖家有 24 小时内可申请仲裁");
        }

        // ── close ───────────────────────────────────────────────────────────
        TaskCommand::Close { job_id } => {
            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/{job_id}/close"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;
            if resp["code"] != 0 { bail!("{}", resp["msg"].as_str().unwrap_or("error")); }
            println!("✓ 任务已关闭，状态 → close");
        }

        // ── set-public ──────────────────────────────────────────────────────
        TaskCommand::SetPublic { job_id } => {
            let resp: serde_json::Value = client
                .post(format!("{api}/api/v1/task/{job_id}/setVisibility"))
                .json(&serde_json::json!({"visibility": 1}))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接 mock-api: {e}"))?
                .json().await?;
            if resp["code"] != 0 { bail!("{}", resp["msg"].as_str().unwrap_or("error")); }
            println!("✓ 任务已转为公开，其他卖家可以看到并报名");
        }

        // ── 剩余未实现（链上操作，暂 stub）────────────────────────────────
        TaskCommand::RejectApply { job_id, provider, reason } =>
            println!("[stub] reject-apply {job_id} provider={provider} reason={reason}"),
        TaskCommand::Confirm { job_id } =>
            println!("[stub] confirm {job_id} (provider on-chain confirm)"),
        TaskCommand::Deliver { job_id, file, message } =>
            println!("[stub] deliver {job_id} file={file} msg={message:?}"),
        TaskCommand::AiEvaluate { job_id } =>
            println!("[stub] ai-evaluate {job_id}"),
        TaskCommand::Config { action } => match action {
            ConfigAction::Init => println!("[stub] task config init"),
            ConfigAction::Show => println!("TASK_API_URL={}", task_api_url()),
        },
    }
    Ok(())
}

pub async fn run_negotiate(cmd: NegotiateCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        NegotiateCommand::Start { .. } => todo!("negotiate start: send XMTP DM"),
        NegotiateCommand::Quote { .. } => todo!("negotiate quote: send quote via XMTP"),
        NegotiateCommand::Counter { .. } => todo!("negotiate counter: send counter via XMTP"),
        NegotiateCommand::Accept { .. } => todo!("negotiate accept: send accept + trigger on-chain confirm"),
        NegotiateCommand::Reject { .. } => todo!("negotiate reject: send reject via XMTP"),
    }
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        DisputeCommand::Raise { .. } => todo!("dispute raise: on-chain + XMTP group notify"),
        DisputeCommand::Evidence { .. } => todo!("dispute evidence: upload file + XMTP"),
        DisputeCommand::Info { .. } => todo!("dispute info: fetch dispute state"),
        DisputeCommand::Vote { .. } => todo!("dispute vote: commit-reveal on-chain"),
        DisputeCommand::Appeal { .. } => todo!("dispute appeal: on-chain appeal"),
    }
}
