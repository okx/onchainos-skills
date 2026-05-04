pub mod chat;
pub mod identity;
pub mod task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

/// Shared `agent` namespace for identity + task-system commands.
#[derive(Subcommand)]
pub enum AgentCommand {
    // ── Identity ────────────────────────────────────────────────────────────
    /// Register a new Agent identity
    Create(identity::CreateArgs),

    /// Update Agent identity and services
    Update(identity::UpdateArgs),

    /// Query your Agents / agent details
    Get(identity::GetArgs),

    /// Activate an Agent
    Activate(identity::AgentStatusArgs),

    /// Deactivate an Agent
    Deactivate(identity::AgentStatusArgs),

    /// Upload an Agent avatar image
    Upload(identity::UploadArgs),

    /// Search public Agents
    Search(identity::SearchArgs),

    /// Query an Agent's services
    #[command(name = "service-list")]
    ServiceList(identity::ServiceListArgs),

    /// Submit an Agent review
    #[command(name = "feedback-submit", visible_alias = "feedbacksubmit")]
    FeedbackSubmit(identity::FeedbackSubmitArgs),

    /// Query Agent reviews
    #[command(name = "feedback-list")]
    FeedbackList(identity::FeedbackListArgs),

    /// 用 keyUuid + signing_seed 代签任意 message（xmtp 等场景），不走广播
    #[command(name = "xmtp-sign")]
    XmtpSign(identity::XmtpSignArgs),

    // ── Task system (Client) ────────────────────────────────────────────────
    /// Create a new task (Client)
    #[command(name = "create-task")]
    CreateTask {
        #[arg(long)] description: String,
        #[arg(long = "description-summary")] description_summary: Option<String>,
        #[arg(long)] budget: f64,
        #[arg(long = "max-budget")] max_budget: Option<f64>,
        #[arg(long)] currency: String,
        #[arg(long = "deadline-open")]  deadline_open: String,
        #[arg(long = "deadline-submit")] deadline_submit: String,
        #[arg(long)] title: Option<String>,
        /// 支付方式: escrow / non_escrow / x402（不指定则为"未设置"）
        #[arg(long = "payment-mode")] payment_mode: Option<String>,
        /// Buyer agent ID（多 buyer 时必传，单 buyer 时自动选择）
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Get recommended providers for a task
    Recommend {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
        #[arg(long)] next: bool,
        #[arg(long)] current: bool,
    },

    /// Get current task status
    Status {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// List my tasks
    List {
        #[arg(long)] status: Option<String>,
        #[arg(long, default_value = "1")]  page: u32,
        #[arg(long, default_value = "20")] limit: u32,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },


    /// Client confirms provider and stakes funds into escrow
    #[command(name = "confirm-accept")]
    ConfirmAccept {
        job_id: String,
        #[arg(long)] provider: String,
        /// 不指定时自动从任务详情 paymentType 获取
        #[arg(long = "payment-mode")] payment_mode: Option<String>,
        /// a2a_pay payment_id（卖家通过 XMTP 传递，non_escrow 必填；escrow 不需要）
        #[arg(long = "payment-id")] payment_id: Option<String>,
        /// 协商确定的支付代币符号（如 USDT），escrow 必填
        #[arg(long = "token-symbol")] token_symbol: Option<String>,
        /// 协商确定的支付金额（人类可读，如 "50"），escrow 必填
        #[arg(long = "token-amount")] token_amount: Option<String>,
        /// x402 服务端点 URL（不指定时从 recommend 缓存或 service-list API 获取）
        #[arg(long)] endpoint: Option<String>,
        /// 跳过 setPaymentMode，直接执行后续支付流程（由 job_payment_mode_changed 事件触发）
        #[arg(long)] resume: bool,
    },

    /// x402 Phase 2b: direct/accept after job_payment_mode_changed + x402 endpoint interaction
    #[command(name = "direct-accept")]
    DirectAccept {
        job_id: String,
        #[arg(long)] provider: String,
        #[arg(long = "token-symbol")] token_symbol: Option<String>,
        #[arg(long = "token-amount")] token_amount: Option<String>,
    },

    /// x402 Phase 2: x402_pay signing + direct/accept + endpoint replay
    #[command(name = "task-402-pay")]
    Task402Pay {
        job_id: String,
        #[arg(long)] provider: String,
        /// JSON accepts array from the HTTP 402 response
        #[arg(long)] accepts: String,
        /// x402 provider endpoint URL (for replay after signing)
        #[arg(long)] endpoint: String,
        #[arg(long = "token-symbol")] token_symbol: Option<String>,
        #[arg(long = "token-amount")] token_amount: Option<String>,
        /// Payer address (optional)
        #[arg(long)] from: Option<String>,
    },

    /// Client confirms task complete and releases payment
    Complete { job_id: String },

    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)] reason: String,
    },

    /// Client closes task (only valid while Open)
    Close { job_id: String },

    /// Convert private task to public listing
    #[command(name = "set-public")]
    SetPublic { job_id: String },

    /// Provider generates payment invoice after provider_applied
    Payment {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Client manually transfers payment to provider (non-escrow mode)
    Pay {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Client claims refund/reward after arbitration
    Claim { job_id: String },

    /// Provider account-pull 查待领奖励
    #[command(name = "provider-claimable")]
    ProviderClaimable {
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// Provider account-pull 一次性领取所有可领奖励
    #[command(name = "provider-claim-rewards")]
    ProviderClaimRewards {
        #[arg(long = "agent-id")] agent_id: String,
    },

    // ── Task system (Provider) ──────────────────────────────────────────────
    /// Provider fetches recommended Public tasks matching their skill
    #[command(name = "recommend-task")]
    RecommendTask {
        /// 卖家 agentId（必填）。beta 后端拒空 agenticId header → 3001 auth fail。
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// 开始接单：调 `agent get` 拉所有在线 provider agent，对每个循环 recommend-task
    #[command(name = "find-jobs")]
    FindJobs,

    /// Provider applies for a task (apply API → sign → broadcast)
    Apply {
        job_id: String,
        #[arg(long = "token-amount", default_value = "0")]
        token_amount: String,
        /// 任务实际币种（USDT / USDG），从任务详情读取，不要假设 USDT
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider submits deliverable (submit API → sign → broadcast)
    Deliver {
        job_id: String,
        #[arg(long, default_value = "")] file: String,
        #[arg(long, default_value = "任务已完成，请验收")] message: String,
        /// 卖家 agentId（必填）。beta 后端拒空 agenticId header → 3001 auth fail。
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    #[command(name = "agree-refund")]
    AgreeRefund {
        job_id: String,
        /// 卖家 agentId（必填）
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// Provider fetches prePayTaskInfo, then calls a2a-pay create to mint a payment_id.
    /// Both escrow and non_escrow go through this command — `--payment-mode` decides
    /// which a2a-pay branch (`charge` for non_escrow, `escrow` otherwise). The
    /// returned `paymentId` is meant to be xmtp-sent to the buyer.
    #[command(name = "get-payment")]
    GetPayment {
        job_id: String,
        /// 任务实际币种（USDT / USDG），从任务详情读取，不要假设 USDT
        #[arg(long = "token-symbol")]
        token_symbol: String,
        /// 协商价格（whole tokens, 如 "50" 表示 50 USDT）。escrow 锁仓金额 / non_escrow 直转金额。
        #[arg(long = "token-amount")]
        token_amount: String,
        /// `escrow` 或 `non_escrow`（必填，弄错支付方式 → paymentId 会落到错的合约 / 流程）
        #[arg(long = "payment-mode")]
        payment_mode: String,
        /// 卖家 agentId（必填）。non_escrow 路径在 status=open 时就调用，
        /// task.providerAgentId 此时还没设，没法从任务详情反查；
        /// escrow 路径也建议显式传，避免本地多 provider agent 时拿错。
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Save negotiated payment params locally (agent calls after negotiation)
    #[command(name = "save-agreed")]
    SaveAgreed {
        job_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
    },

    /// Client claims auto-refund after provider timeout
    #[command(name = "claim-auto-refund")]
    ClaimAutoRefund { job_id: String },

    /// Provider claims auto-complete after buyer review timeout (review_expired)
    #[command(name = "claim-auto-complete")]
    ClaimAutoComplete {
        job_id: String,
        /// Provider's own agentId
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    // ── Task system (sub-groups) ────────────────────────────────────────────
    /// Task config: init | show
    Config {
        #[command(subcommand)]
        action: task::buyer::ConfigAction,
    },

    /// Dispute actions (provider): raise, evidence, info, upload
    #[command(subcommand)]
    Dispute(task::provider::DisputeCommand),

    /// Evaluator actions (arbitrator): info, commit, reveal, claim, claimable, stake/unstake
    #[command(subcommand)]
    Evaluator(task::evaluator::EvaluatorCommand),

    /// Common queries: context lookup for AI agents
    #[command(subcommand)]
    Common(task::common::CommonCommand),

    /// Get next-step instruction prompt for current job state
    #[command(name = "next-action")]
    NextAction {
        #[arg(long = "jobid")] job_id: String,
        #[arg(long = "jobStatus")] job_status: String,
        #[arg(long = "agentId")] agent_id: String,
        #[arg(long)] role: String,
    },

    /// Rate an agent (wraps feedback-submit; usable by buyer/provider/evaluator)
    #[command(name = "rate-agent")]
    RateAgent {
        /// 被评价的 Agent ID
        #[arg(long = "agent-id")]
        agent_id: String,
        /// 评价发起方 Agent ID
        #[arg(long = "creator-id")]
        creator_id: String,
        /// 评分（0-100）
        #[arg(long)]
        score: String,
        /// 文字评价（可选）
        #[arg(long)]
        description: Option<String>,
        /// 任务 ID（可选）
        #[arg(long = "task-id")]
        task_id: Option<String>,
    },

    // Chat
    /// Upload an encrypted file attachment and receive a file key
    #[command(name = "file-upload")]
    FileUpload {
        #[arg(long)]
        file: String,
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        job_id: String,
    },

    /// Download an encrypted file attachment by file key
    #[command(name = "file-download")]
    FileDownload {
        #[arg(long)]
        file_key: String,
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        output: String,
    },

    /// Get sensitive word list for A2A risk filtering
    #[command(name = "sensitive-words")]
    SensitiveWords,

    /// Check if a message is eligible to be sent
    #[command(name = "message-eligible")]
    MessageEligible {
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        client_agent_id: String,
        #[arg(long)]
        provider_agent_id: String,
        #[arg(long)]
        job_id: String,
        #[arg(long)]
        group_id: String,
        #[arg(long)]
        direction: String,
        #[arg(long)]
        provider_security_rate: String,
    },

    /// Get XMTP system config (system account addresses)
    #[command(name = "system-config")]
    SystemConfig,

    /// Send agent heartbeat to report online status
    Heartbeat {
        #[arg(long)]
        chain_index: u64,
    },
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    use task::buyer::TaskCommand as T;

    match cmd {
        // ── Identity ────────────────────────────────────────────────
        AgentCommand::Create(args) => identity::create(args, ctx).await,
        AgentCommand::Update(args) => identity::update(args, ctx).await,
        AgentCommand::Get(args) => identity::get(args, ctx).await,
        AgentCommand::Activate(args) => identity::activate(args, ctx).await,
        AgentCommand::Deactivate(args) => identity::deactivate(args, ctx).await,
        AgentCommand::Upload(args) => identity::upload(args, ctx).await,
        AgentCommand::Search(args) => identity::search(args, ctx).await,
        AgentCommand::ServiceList(args) => identity::service_list(args, ctx).await,
        AgentCommand::FeedbackSubmit(args) => identity::feedback_submit(args, ctx).await,
        AgentCommand::FeedbackList(args) => identity::feedback_list(args, ctx).await,
        AgentCommand::XmtpSign(args) => identity::xmtp_sign(args, ctx).await,

        // ── Client (buyer) task commands ────────────────────────────
        AgentCommand::CreateTask {
            description, description_summary, budget, max_budget, currency,
            deadline_open, deadline_submit, title, payment_mode, agent_id,
        } => task::buyer::run_task(
            T::Create {
                description, description_summary, budget, max_budget, currency,
                deadline_open, deadline_submit, title, payment_mode, agent_id,
            }, ctx,
        ).await,

        AgentCommand::Recommend { job_id, agent_id, next, current } =>
            task::buyer::run_task(T::Recommend { job_id, agent_id, next, current }, ctx).await,

        AgentCommand::Status { job_id, agent_id } =>
            task::buyer::run_task(T::Status { job_id, agent_id }, ctx).await,

        AgentCommand::List { status, page, limit, agent_id } =>
            task::buyer::run_task(T::List { status, page, limit, agent_id }, ctx).await,


        AgentCommand::ConfirmAccept { job_id, provider, payment_mode, payment_id, token_symbol, token_amount, endpoint, resume } =>
            task::buyer::run_task(T::ConfirmAccept { job_id, provider, payment_mode, payment_id, token_symbol, token_amount, endpoint, resume }, ctx).await,

        AgentCommand::DirectAccept { job_id, provider, token_symbol, token_amount } =>
            task::buyer::run_task(T::DirectAccept { job_id, provider, token_symbol, token_amount }, ctx).await,

        AgentCommand::Task402Pay { job_id, provider, accepts, endpoint, token_symbol, token_amount, from } =>
            task::buyer::run_task(T::Task402Pay { job_id, provider, accepts, endpoint, token_symbol, token_amount, from }, ctx).await,

        AgentCommand::Complete { job_id } =>
            task::buyer::run_task(T::Complete { job_id }, ctx).await,

        AgentCommand::Reject { job_id, reason } =>
            task::buyer::run_task(T::Reject { job_id, reason }, ctx).await,

        AgentCommand::Close { job_id } =>
            task::buyer::run_task(T::Close { job_id }, ctx).await,

        AgentCommand::SetPublic { job_id } =>
            task::buyer::run_task(T::SetPublic { job_id }, ctx).await,

        AgentCommand::Payment { job_id, agent_id } =>
            task::buyer::run_task(T::Payment { job_id, agent_id }, ctx).await,

        AgentCommand::Pay { job_id, agent_id } =>
            task::buyer::run_task(T::Pay { job_id, agent_id }, ctx).await,

        AgentCommand::Claim { job_id } =>
            task::buyer::run_task(T::Claim { job_id }, ctx).await,

        AgentCommand::SaveAgreed { job_id, token_symbol, token_amount } =>
            task::buyer::run_task(T::SaveAgreed { job_id, token_symbol, token_amount }, ctx).await,

        AgentCommand::ClaimAutoRefund { job_id } =>
            task::buyer::run_task(T::ClaimAutoRefund { job_id }, ctx).await,

        AgentCommand::ClaimAutoComplete { job_id, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::ClaimAutoComplete { job_id, agent_id }, ctx,
            ).await,

        AgentCommand::ProviderClaimable { agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Claimable { agent_id }, ctx,
            ).await,

        AgentCommand::ProviderClaimRewards { agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::ClaimRewards { agent_id }, ctx,
            ).await,

        // ── Provider task commands ──────────────────────────────────
        AgentCommand::RecommendTask { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::provider::recommend_task::handle_recommend_task(&mut c, &agent_id).await
        }

        AgentCommand::FindJobs =>
            task::provider::find_jobs::handle_find_jobs().await,

        AgentCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id },
                ctx,
            ).await,

        AgentCommand::Deliver { job_id, file, message, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Deliver { job_id, file, message, agent_id }, ctx,
            ).await,

        AgentCommand::AgreeRefund { job_id, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::AgreeRefund { job_id, agent_id }, ctx,
            ).await,

        AgentCommand::GetPayment { job_id, token_symbol, token_amount, payment_mode, agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::provider::get_payment::handle_get_payment(
                &mut c,
                &job_id,
                &token_symbol,
                &token_amount,
                &payment_mode,
                &agent_id,
            )
            .await
        }

        // ── Sub-groups ──────────────────────────────────────────────
        AgentCommand::Config { action } =>
            task::buyer::run_task(T::Config { action }, ctx).await,

        AgentCommand::Dispute(c) =>
            task::provider::run_dispute(c, ctx).await,

        AgentCommand::Evaluator(c) =>
            task::evaluator::run(c, ctx).await,

        AgentCommand::Common(c) =>
            task::common::run(c, ctx).await,

        AgentCommand::NextAction { job_id, job_status, agent_id, role } => {
            eprintln!(
                "[next-action] 收到系统通知: job_id={job_id}, job_status={job_status}, role={role}, agent_id={agent_id}"
            );
            // 状态脱节 → block 输出剧本（避免 sub 按 stale event 跑老剧本上链）
            // 只在 PSEUDO_EVENTS / unknown / network failure 时跳过校验，正常情况下严格守门
            if let Some(w) = check_status_freshness(&job_id, &job_status, &agent_id).await {
                println!("{w}");
                return Ok(());
            }
            let prompt = match role.as_str() {
                "provider" | "seller" =>
                    task::provider::flow::generate_next_action(&job_id, &job_status, &agent_id),
                "buyer" | "client" =>
                    task::buyer::flow::generate_next_action(&job_id, &job_status, &agent_id),
                "evaluator" => {
                    // best-effort 拉平台质押 & 仲裁配置；拉不到回退到 cfg_defaults，
                    // 避免 sub session 因网络抖动跑不出剧本。
                    let staking_cfg = task::common::network::task_api_client::TaskApiClient::new()
                        .get_staking_config(&agent_id)
                        .await
                        .ok();
                    task::evaluator::flow::generate_next_action(
                        &job_id, &job_status, &agent_id, staking_cfg.as_ref(),
                    )
                }
                other => anyhow::bail!("--role 必须是 provider/buyer/client/evaluator，当前: {other}"),
            };
            println!("{prompt}");
            Ok(())
        }

        AgentCommand::RateAgent { agent_id, creator_id, score, description, task_id } =>
            task::common::rate_agent::handle_rate_agent(
                &agent_id, &creator_id, &score,
                description.as_deref(), task_id.as_deref(),
            ).await,

        // ── Chat (XMTP attachments + risk/eligibility + system config + heartbeat) ──
        AgentCommand::FileUpload { file, agent_id, job_id } =>
            chat::run(chat::ChatCommand::FileUpload { file, agent_id, job_id }, ctx).await,

        AgentCommand::FileDownload { file_key, agent_id, output } =>
            chat::run(chat::ChatCommand::FileDownload { file_key, agent_id, output }, ctx).await,

        AgentCommand::SensitiveWords =>
            chat::run(chat::ChatCommand::SensitiveWords, ctx).await,

        AgentCommand::MessageEligible {
            agent_id,
            client_agent_id,
            provider_agent_id,
            job_id,
            group_id,
            direction,
            provider_security_rate,
        } => chat::run(
            chat::ChatCommand::MessageEligible {
                agent_id,
                client_agent_id,
                provider_agent_id,
                job_id,
                group_id,
                direction,
                provider_security_rate,
            },
            ctx,
        ).await,

        AgentCommand::SystemConfig =>
            chat::run(chat::ChatCommand::SystemConfig, ctx).await,

        AgentCommand::Heartbeat { chain_index } =>
            chat::run(chat::ChatCommand::Heartbeat { chain_index }, ctx).await,
    }
}

/// 比对 next-action 入参的 jobStatus/event 暗示的 status 与任务真实 statusStr，
/// 不一致时返回一段 warning 文本（用于 prepend 到剧本输出顶部）。
///
/// 触发场景：system event 延迟、之前的 CLI 操作已经把 status 推得更靠前、
/// 网络/解析失败时返回 None（不阻塞剧本输出，graceful fallback）。
async fn check_status_freshness(job_id: &str, job_status_or_event: &str, agent_id: &str) -> Option<String> {
    use task::common::network::task_api_client::TaskApiClient;
    use task::common::state_machine::{parse_status_or_event, status_when_event, Status};

    // user-instruction 伪 event 不是链事件，不直接对应 status——它们在某个 status 下被触发
    // 后才会上链改 status。校验它们的"对应 status"会误报，所以这里直接跳过。
    const PSEUDO_EVENTS: &[&str] = &[
        "dispute_raise", "agree_refund", "dispute_evidence",
        "close", "set_public",
    ];
    if PSEUDO_EVENTS.contains(&job_status_or_event) {
        return None;
    }

    let event = parse_status_or_event(job_status_or_event);
    let expected = status_when_event(&event);

    // 如果 event 解析成 Status::Other("unknown")（即未识别的 Event::Other），
    // 也跳过校验（避免对不认识的 event 误报）
    if matches!(expected, Status::Other(ref s) if s == "unknown") {
        eprintln!("[check-freshness] 跳过校验: 未识别的 event={job_status_or_event}");
        return None;
    }

    let mut c = TaskApiClient::new();
    // 必须带 agenticId header——beta 后端没 header 就返回 code=3001 auth fail。
    // next-action 命令本身要求 --agentId 必填，所以这里直接用，不做 empty fallback。
    let resp = c.get_with_identity(&c.task_path(job_id), agent_id).await.ok()?;
    // 后端 spec：响应平铺，status 是 int
    let actual = Status::from_int(i32::try_from(resp.get("status")?.as_i64()?).ok()?);
    let actual_str = actual.as_str().to_string();

    eprintln!(
        "[check-freshness] job_id={job_id}, event={job_status_or_event}, expected_status={}, actual_status={actual_str}, match={}",
        expected.as_str(),
        actual == expected,
    );

    if actual == expected {
        return None;
    }
    Some(format!(
        "🛑 **状态脱节，剧本已 block**（next-action 入参与任务真实状态不一致，不输出步骤防止你按 stale event 上链）\n\n\
         - 你传的 jobStatus/event = `{job_status_or_event}`，对应任务状态应为 `{expected_str}`\n\
         - 但任务 {job_id} 真实 statusStr = `{actual_str}`\n\n\
         **必须做**：重调 next-action 并传 `--jobStatus {actual_str}`（按真实状态拿剧本），或忽略本条过期通知结束 turn 等下一个真实链事件。\n\
         **禁止做**：不要硬猜下一步、不要在没拿到剧本前调任何 task CLI、不要把这条警告用 xmtp_dispatch_user 推用户。\n",
        expected_str = expected.as_str(),
    ))
}
