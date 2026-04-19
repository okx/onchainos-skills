use anyhow::{bail, Result};
use clap::Subcommand;

use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::mock_identity::{self as identity, AgentRole, AccountBalance};
use crate::commands::agent_commerce::task::common::{
    PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW,
    XLAYER_CHAIN_ID, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME,
};
use crate::commands::agent_commerce::task::messaging::{self, MessageSender};
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;
use crate::wallet_api::UnsignedInfoResponse;

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

/// 单次任务预算上限
const MAX_BUDGET: f64 = 10_000_000.0;

/// 校验货币符号
fn validate_currency(currency: &str) -> Result<()> {
    match currency.to_uppercase().as_str() {
        "USDT" | "USDG" => Ok(()),
        other => bail!("不支持的代币: {other}，仅支持 USDT 和 USDG"),
    }
}

/// 余额不足时输出提示（仅警告，不阻断流程）
fn warn_insufficient_balance(bal: &AccountBalance, budget: f64, currency: &str) {
    let available = match currency.to_uppercase().as_str() {
        "USDT" => bal.usdt,
        "USDG" => bal.usdg,
        _ => return,
    };
    if available < budget {
        println!(
            "⚠ 当前账户 {} 余额不足: {} {} (任务预算 {} {})，请在上链前充值",
            bal.address, available, currency.to_uppercase(),
            budget, currency.to_uppercase()
        );
    }
}

/// 校验预算金额
fn validate_budget(budget: f64) -> Result<()> {
    if budget <= 0.0 {
        bail!("预算金额必须大于 0");
    }
    if budget > MAX_BUDGET {
        bail!("单次任务预算不得超过 {} USDT/USDG", MAX_BUDGET as u64);
    }
    Ok(())
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
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
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
        #[arg(long = "payment-mode", default_value = PAYMENT_MODE_ESCROW)]
        payment_mode: String,
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
    /// Provider applies for a public task
    Apply {
        job_id: String,
    },
    /// AI-assisted deliverable quality assessment
    AiEvaluate {
        job_id: String,
    },
    /// Client manually transfers payment to provider (non-escrow mode)
    Pay {
        job_id: String,
    },
    /// Client claims refund/reward after arbitration
    Claim {
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
        #[arg(long = "payment-mode", default_value = PAYMENT_MODE_ESCROW)]
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
    let http = reqwest::Client::new();

    match cmd {
        // ── 创建任务 (create → sign → broadcast) ────────────────────────────
        TaskCommand::Create {
            description, description_summary, budget, max_budget, currency,
            deadline_open, deadline_submit, title,
        } => {
            validate_currency(&currency)?;
            validate_budget(budget)?;

            let max_budget_val = max_budget.unwrap_or(budget);
            if max_budget_val < budget {
                bail!("--max-budget ({max_budget_val}) 不能小于 --budget ({budget})");
            }
            validate_budget(max_budget_val)?;

            let open_secs   = parse_duration_secs(&deadline_open)
                .map_err(|_| anyhow::anyhow!("--deadline-open 格式错误，例如 72h 或 3600"))?;
            let submit_secs = parse_duration_secs(&deadline_submit)
                .map_err(|_| anyhow::anyhow!("--deadline-submit 格式错误，例如 48h 或 3600"))?;

            let title_str = title.unwrap_or_else(|| description.chars().take(30).collect());
            let summary   = description_summary
                .unwrap_or_else(|| description.chars().take(200).collect());

            // ── Step 0: 身份检查 + 余额提示 ───────────────────────────
            let wallets = crate::wallet_store::load_wallets()?
                .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

            let selected_account_id = &wallets.selected_account_id;
            let (_, selected_addr) = resolve_address(&wallets, None, XLAYER_CHAIN_NAME)?;

            // 0-a: 静默检查当前账户是否已注册买家身份
            let (account_id, addr_info) = if identity::has_role(
                selected_account_id,
                &selected_addr.address,
                AgentRole::Buyer,
            ).await? {
                // 当前账户是买家 → 告知用户并继续
                println!("✓ 当前账户已具有买家身份 (account: {selected_account_id})");

                // 0-b: 查询当前账户余额，与任务预算对比（仅提示，不强制）
                let bal = identity::get_account_balance(
                    selected_account_id, &selected_addr.address,
                ).await?;
                warn_insufficient_balance(&bal, budget, &currency);

                (selected_account_id.clone(), selected_addr)
            } else {
                // 0-c: 当前账户无买家身份 → 查找其他有买家身份的账户
                let buyer_accounts = identity::list_accounts_with_role(
                    &wallets,
                    XLAYER_CHAIN_NAME,
                    AgentRole::Buyer,
                ).await?;

                if buyer_accounts.is_empty() {
                    // 0-d: 所有账户都没有买家身份 → 提示注册当前账户
                    println!("当前无任何账户具有买家身份");
                    println!("正在为当前账户注册买家身份...");
                    let _agent_id = identity::register_identity(
                        selected_account_id,
                        &selected_addr.address,
                        AgentRole::Buyer,
                    ).await?;
                    (selected_account_id.clone(), selected_addr)
                } else {
                    // 0-e: 列出有买家身份的账户（附带 USDT/USDG 余额）
                    let acct_pairs: Vec<(&str, &str)> = buyer_accounts
                        .iter()
                        .map(|a| (a.account_id.as_str(), a.address.as_str()))
                        .collect();
                    let balances = identity::get_accounts_balance(&acct_pairs).await?;

                    println!("当前账户未注册买家身份，以下账户可用：");
                    for (i, acct) in buyer_accounts.iter().enumerate() {
                        let bal = balances.iter().find(|b| b.account_id == acct.account_id);
                        let (usdt, usdg) = bal
                            .map(|b| (b.usdt, b.usdg))
                            .unwrap_or((0.0, 0.0));
                        println!(
                            "  {}. account: {}  address: {}  agent: {}  USDT: {}  USDG: {}",
                            i + 1, acct.account_id, acct.address, acct.agent_id, usdt, usdg
                        );
                    }
                    // CLI 模式下默认使用第一个可用账户
                    let chosen = &buyer_accounts[0];
                    println!("使用账户: {} ({})", chosen.account_id, chosen.address);
                    let (_, addr) = resolve_address(&wallets, Some(&chosen.address), XLAYER_CHAIN_NAME)?;
                    (chosen.account_id.clone(), addr)
                }
            };

            // ── Step 1: 生成 calldata (POST /priapi/v1/aieco/task/create) ────────
            let body = serde_json::json!({
                "title":              title_str,
                "description":        description,
                "description_summary": summary,
                "paymentTokenSymbol": currency.to_uppercase(),
                "paymentTokenAmount": budget.to_string(),
                "maxPaymentTokenAmount": max_budget_val.to_string(),
                "chainId":            XLAYER_CHAIN_ID,
                "expireConfig": {
                    "acceptDeadline":    open_secs,
                    "submittedDeadline": submit_secs
                },
                "paymentMode":        0,
                "visibility":         0
            });

            let resp: serde_json::Value = http
                .post(format!("{api}/priapi/v1/aieco/task/create"))
                .json(&body)
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("创建失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
            }

            let job_id   = resp["data"]["jobId"].as_str().unwrap_or("?").to_string();
            let uop_data = &resp["data"]["uopData"];

            println!("✓ Calldata 已生成 (jobId: {job_id})");

            // ── Step 2: 签名 uopHash (build_broadcast_body) ─────────────
            let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
                .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

            let broadcast_body = build_broadcast_body(
                &unsigned,
                &account_id,
                &addr_info.address,
                XLAYER_CHAIN_INDEX,
                true,   // is_contract_call
                false,  // mev_protection — XLayer 不需要
                false,  // force
            )
            .await?;

            println!("✓ 签名完成");

            // ── Step 3: 广播上链 (POST /priapi/v1/aieco/task/broadcast) ──────────
            let bc_resp: serde_json::Value = http
                .post(format!("{api}/priapi/v1/aieco/task/broadcast"))
                .json(&broadcast_body)
                .send().await
                .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?
                .json().await?;

            if bc_resp["code"] != 0 {
                bail!("广播失败: {}", bc_resp["msg"].as_str().unwrap_or("unknown"));
            }

            let tx_hash = bc_resp["data"][0]["txHash"].as_str().unwrap_or("pending");
            println!("✓ 任务已上链");
            println!("  jobId:  {job_id}");
            println!("  txHash: {tx_hash}");
            println!("  状态:   open（等待 Provider 报名）");
            println!();
            println!("下一步: onchainos agent recommend {job_id}");
        }

        // ── 查询推荐卖家 ────────────────────────────────────────────────────
        TaskCommand::Recommend { job_id } => {
            let resp: serde_json::Value = http
                .post(format!("{api}/priapi/v1/aieco/task/{job_id}/match"))
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
            let resp: serde_json::Value = http
                .get(format!("{api}/priapi/v1/aieco/task/{job_id}"))
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
            println!("  预算:     {} USDT", t["tokenAmount"].as_str().unwrap_or("?"));
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
                format!("{api}/priapi/v1/aieco/task/my?role={r}&page={page}&page_size={limit}")
            } else {
                let mut u = format!("{api}/priapi/v1/aieco/task/list?page={page}&page_size={limit}");
                if let Some(s) = &status { u.push_str(&format!("&status={s}")); }
                u
            };
            let resp: serde_json::Value = http.get(&url).send().await
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

        // ── confirm-accept（双签/单签上链）─────────────────────────────────
        TaskCommand::ConfirmAccept { job_id, provider, payment_mode } => {
            let (account_id, address) = signing::resolve_wallet_for_task(&http, &api, &job_id).await?;
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");

            if payment_mode == PAYMENT_MODE_NON_ESCROW {
                // 非担保：标准单签 direct/accept
                let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
                let body = serde_json::json!({
                    "providerAddress": provider,
                    "providerAgentId": provider,
                });
                let result = signing::task_sign_and_broadcast(
                    &http, &endpoint, &body, &broadcast, &account_id, &address,
                ).await?;
                println!("✓ 已接受卖家 {provider}（非担保支付），任务状态 → accepted");
                println!("  注意：任务完成后需手动转账给卖家");
                println!("  txHash: {}", result.tx_hash);
            } else {
                // 担保：双签 pre-accept → 签 digest → accept → 签 uopHash → broadcast
                let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-accept");
                let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/accept");
                let pre_body = serde_json::json!({
                    "providerAddress": provider,
                    "providerAgentId": provider,
                });
                let provider_clone = provider.clone();
                let result = signing::task_dual_sign_and_broadcast(
                    &http,
                    &pre_endpoint,
                    &pre_body,
                    &main_endpoint,
                    move |signature| serde_json::json!({
                        "providerAddress": provider_clone,
                        "providerAgentId": provider_clone,
                        "paymentMode": PAYMENT_MODE_ESCROW,
                        "signature": signature,  // 【待确认】字段名
                    }),
                    &broadcast,
                    &account_id,
                    &address,
                ).await?;
                println!("✓ 已接受卖家 {provider}（担保支付），任务状态 → accepted");
                println!("  txHash: {}", result.tx_hash);
            }
        }

        // ── complete（双签上链）────────────────────────────────────────────
        TaskCommand::Complete { job_id } => {
            let (account_id, address) = signing::resolve_wallet_for_task(&http, &api, &job_id).await?;
            let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-complete");
            let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/complete");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let pre_body = serde_json::json!({});

            let result = signing::task_dual_sign_and_broadcast(
                &http,
                &pre_endpoint,
                &pre_body,
                &main_endpoint,
                |signature| serde_json::json!({
                    "signature": signature,  // 【待确认】字段名
                }),
                &broadcast,
                &account_id,
                &address,
            ).await?;

            println!("✓ 任务验收通过，状态 → complete，款项已释放");
            println!("  txHash: {}", result.tx_hash);
        }

        // ── reject/refuse（双签上链）─────────────────────────────────────
        TaskCommand::Reject { job_id, reason } => {
            let (account_id, address) = signing::resolve_wallet_for_task(&http, &api, &job_id).await?;
            let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-refuse");
            let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/refuse");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let pre_body = serde_json::json!({});

            let reason_clone = reason.clone();
            let result = signing::task_dual_sign_and_broadcast(
                &http,
                &pre_endpoint,
                &pre_body,
                &main_endpoint,
                move |signature| serde_json::json!({
                    "signature": signature,  // 【待确认】字段名
                    "reason": reason_clone,
                }),
                &broadcast,
                &account_id,
                &address,
            ).await?;

            println!("✓ 已拒绝验收（原因：{reason}），状态 → refused");
            println!("  卖家有 24 小时内可申请仲裁");
            println!("  txHash: {}", result.tx_hash);
        }

        // ── close（单签上链）──────────────────────────────────────────────
        TaskCommand::Close { job_id } => {
            let (account_id, address) = signing::resolve_wallet_for_task(&http, &api, &job_id).await?;
            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/close");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({});

            let result = signing::task_sign_and_broadcast(
                &http, &endpoint, &body, &broadcast, &account_id, &address,
            ).await?;

            println!("✓ 任务已关闭，状态 → close");
            println!("  txHash: {}", result.tx_hash);
        }

        // ── set-public（单签上链）─────────────────────────────────────────
        TaskCommand::SetPublic { job_id } => {
            let (account_id, address) = signing::resolve_wallet_for_task(&http, &api, &job_id).await?;
            let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/setVisibility");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({"visibility": 1});

            let result = signing::task_sign_and_broadcast(
                &http, &endpoint, &body, &broadcast, &account_id, &address,
            ).await?;

            println!("✓ 任务已转为公开，其他卖家可以看到并报名");
            println!("  txHash: {}", result.tx_hash);
        }

        // ── apply — TODO(provider): 需改为签名流程 ────────────────────────
        TaskCommand::Apply { job_id } => {
            // TODO(provider): 改为 task_sign_and_broadcast 签名上链
            let resp: serde_json::Value = http
                .post(format!("{api}/priapi/v1/aieco/task/{job_id}/apply"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
                .json().await?;
            if resp["code"] != 0 {
                bail!("{}", resp["msg"].as_str().unwrap_or("error"));
            }
            println!("✓ 已申请任务 {job_id}，等待买家确认");
        }

        // ── pay（非担保模式手动转账）──────────────────────────────────────
        TaskCommand::Pay { job_id } => {
            // 查询任务详情，获取 Provider 地址、金额、代币
            let resp: serde_json::Value = http
                .get(format!("{api}/priapi/v1/aieco/task/{job_id}"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法查询任务详情: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("查询任务失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
            }

            let task = &resp["data"]["task"];
            let status = task["statusStr"].as_str().unwrap_or("");
            if status != "complete" {
                bail!("任务状态为 {status}，仅 complete 状态可执行 pay");
            }

            let provider_addr = task["providerAgentAddress"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("任务详情缺少 providerAgentAddress"))?;
            let amount = task["tokenAmount"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("任务详情缺少 tokenAmount"))?;
            let token_symbol = task["paymentTokenSymbol"]
                .as_str()
                .unwrap_or("USDT");
            let token_address = task["tokenAddress"]
                .as_str()
                .unwrap_or("");

            println!("非担保任务付款信息：");
            println!("  Provider: {provider_addr}");
            println!("  金额:     {amount} {token_symbol}");
            println!("  链:       xlayer (chainId={})", XLAYER_CHAIN_ID);
            println!();
            println!("请执行以下命令完成转账：");
            if token_address.is_empty() {
                println!("  onchainos wallet send --readable-amount {amount} --recipient {provider_addr} --chain xlayer");
            } else {
                println!("  onchainos wallet send --readable-amount {amount} --recipient {provider_addr} --chain xlayer --contract-token {token_address}");
            }
        }

        // ── claim（仲裁奖金领取，单签上链）─────────────────────────────────
        TaskCommand::Claim { job_id } => {
            let (account_id, address) = signing::resolve_wallet_for_task(&http, &api, &job_id).await?;
            let endpoint = format!("{api}/priapi/v1/aieco/task/claim");
            let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
            let body = serde_json::json!({ "jobId": job_id });

            let result = signing::task_sign_and_broadcast(
                &http, &endpoint, &body, &broadcast, &account_id, &address,
            ).await?;

            println!("✓ 仲裁奖金已领取");
            println!("  txHash: {}", result.tx_hash);
        }

        // ── 待确认/待实现 ─────────────────────────────────────────────────
        // 【待确认】Scene 3 C8: Client 拒绝 Provider 接单申请，需求细节/后端接口/是否需链上签名均待确认
        TaskCommand::RejectApply { job_id, provider, reason } =>
            println!("[TODO] reject-apply {job_id} provider={provider} reason={reason} — 待确认需求"),
        // TODO(provider): 实现 Provider 链上确认签名
        TaskCommand::Confirm { job_id } =>
            println!("[TODO(provider)] confirm {job_id}"),
        // TODO(provider): 实现文件上传 + submit 签名流程
        TaskCommand::Deliver { job_id, file, message } =>
            println!("[TODO(provider)] deliver {job_id} file={file} msg={message:?}"),
        TaskCommand::AiEvaluate { job_id } =>
            println!("[TODO] ai-evaluate {job_id}"),
        TaskCommand::Config { action } => match action {
            ConfigAction::Init => println!("[stub] task config init"),
            ConfigAction::Show => println!("TASK_API_URL={}", task_api_url()),
        },
    }
    Ok(())
}

pub async fn run_negotiate(cmd: NegotiateCommand, _ctx: &Context) -> Result<()> {
    let sender = messaging::create_sender();
    let now = chrono::Utc::now().to_rfc3339();

    match cmd {
        NegotiateCommand::Start { to, job_id, message } => {
            let msg = serde_json::json!({
                "type": "negotiate:start",
                "jobId": job_id,
                "to": to,
                "message": message,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Quote { to, job_id, price, currency, delivery_hours, skill_id, message } => {
            let msg = serde_json::json!({
                "type": "negotiate:quote",
                "jobId": job_id,
                "to": to,
                "price": price,
                "currency": currency.to_uppercase(),
                "deliveryHours": delivery_hours,
                "skillId": skill_id,
                "message": message,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Counter { to, job_id, price, reason } => {
            let msg = serde_json::json!({
                "type": "negotiate:counter",
                "jobId": job_id,
                "to": to,
                "price": price,
                "reason": reason,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Accept { to, job_id, price, delivery_hours, payment_mode } => {
            let msg = serde_json::json!({
                "type": "negotiate:accept",
                "jobId": job_id,
                "to": to,
                "price": price,
                "deliveryHours": delivery_hours,
                "paymentMode": payment_mode,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
        NegotiateCommand::Reject { to, job_id, reason } => {
            let msg = serde_json::json!({
                "type": "negotiate:reject",
                "jobId": job_id,
                "to": to,
                "reason": reason,
                "timestamp": now,
            });
            sender.send_dm(&to, &msg).await?;
        }
    }
    Ok(())
}

pub async fn run_dispute(cmd: DisputeCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        // TODO(provider): Provider 发起仲裁，捆绑签名 approve(DisputeManager, 5%) + createDispute(jobId)
        DisputeCommand::Raise { .. } => todo!("dispute raise"),
        // TODO(client): Phase 4 实现 — multipart 文件上传（jpg/jpeg/png/gif/webp），无链上签名
        DisputeCommand::Evidence { .. } => todo!("dispute evidence"),
        // ── dispute info（GET 只读查询）────────────────────────────────
        DisputeCommand::Info { dispute_id } => {
            let api = task_api_url();
            let http = reqwest::Client::new();
            let resp: serde_json::Value = http
                .get(format!("{api}/priapi/v1/aieco/task/dispute/{dispute_id}"))
                .send().await
                .map_err(|e| anyhow::anyhow!("无法查询争议详情: {e}"))?
                .json().await?;

            if resp["code"] != 0 {
                bail!("查询争议失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
            }

            let d = &resp["data"];
            println!("争议详情：");
            println!("  disputeId: {dispute_id}");
            println!("  jobId:     {}", d["jobId"].as_str().unwrap_or("?"));
            println!("  状态:      {}", d["statusStr"].as_str().unwrap_or("?"));
            println!("  发起方:    {}", d["raiserAddress"].as_str().unwrap_or("?"));
            println!("  发起原因:  {}", d["reason"].as_str().unwrap_or("?"));
            println!("  创建时间:  {}", d["createTime"].as_str().unwrap_or("?"));

            let evidences = d["evidences"].as_array();
            if let Some(evs) = evidences {
                println!("\n证据列表（共 {} 条）：", evs.len());
                for (i, ev) in evs.iter().enumerate() {
                    println!("  {}. 提交方: {}  类型: {}",
                        i + 1,
                        ev["submitter"].as_str().unwrap_or("?"),
                        ev["type"].as_str().unwrap_or("?"),
                    );
                    println!("     摘要: {}", ev["summary"].as_str().unwrap_or("?"));
                    if let Some(url) = ev["fileUrl"].as_str() {
                        println!("     文件: {url}");
                    }
                }
            } else {
                println!("\n暂无证据提交");
            }
        }
        // TODO(evaluator): Commit-Reveal 投票第一步
        DisputeCommand::Vote { .. } => todo!("dispute vote"),
        // TODO(provider): Provider 上诉
        DisputeCommand::Appeal { .. } => todo!("dispute appeal"),
    }
    Ok(())
}
