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

    /// Query your Agents / ws-mock identity (legacy)
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
    },

    /// Get recommended providers for a task
    Recommend {
        job_id: String,
        #[arg(long)] next: bool,
        #[arg(long)] current: bool,
    },

    /// Get current task status
    Status { job_id: String },

    /// List tasks
    List {
        #[arg(long)] role: Option<String>,
        #[arg(long)] status: Option<String>,
        #[arg(long, default_value = "1")]  page: u32,
        #[arg(long, default_value = "20")] limit: u32,
    },

    /// Client confirms provider and stakes funds into escrow
    #[command(name = "confirm-accept")]
    ConfirmAccept {
        job_id: String,
        #[arg(long)] provider: String,
        #[arg(long = "payment-mode", default_value = "escrow")] payment_mode: String,
    },

    /// Client rejects provider application
    #[command(name = "reject-apply")]
    RejectApply {
        job_id: String,
        #[arg(long)] provider: String,
        #[arg(long)] reason: String,
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
    Payment { job_id: String },

    /// Client manually transfers payment to provider (non-escrow mode)
    Pay { job_id: String },

    /// Client claims refund/reward after arbitration
    Claim { job_id: String },

    // ── Task system (Provider) ──────────────────────────────────────────────
    /// Provider fetches recommended Public tasks matching their skill
    #[command(name = "recommend-task")]
    RecommendTask {
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// 开始接单：调 `agent get` 拉所有在线 provider agent，对每个循环 recommend-task
    #[command(name = "find-jobs")]
    FindJobs,

    /// Provider initiates contact with a buyer (xmtp, placeholder)
    #[command(name = "contact-buyer")]
    ContactBuyer {
        #[arg(long = "to")]
        to_agent_id: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        message: Option<String>,
    },

    /// Provider applies for a task (apply API → sign → broadcast)
    Apply {
        job_id: String,
        #[arg(long = "token-amount", default_value = "0")]
        token_amount: String,
        #[arg(long = "token-symbol", default_value = "USDT")]
        token_symbol: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider submits deliverable (submit API → sign → broadcast)
    Deliver {
        job_id: String,
        #[arg(long, default_value = "")] file: String,
        #[arg(long, default_value = "任务已完成，请验收")] message: String,
    },

    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    #[command(name = "agree-refund")]
    AgreeRefund { job_id: String },

    /// Provider fetches on-chain payment pre-info after provider_applied
    #[command(name = "get-payment")]
    GetPayment {
        job_id: String,
        #[arg(long = "token-symbol", default_value = "USDT")]
        token_symbol: String,
    },

    /// Client claims auto-refund after provider timeout
    #[command(name = "claim-auto-refund")]
    ClaimAutoRefund { job_id: String },

    // ── Task system (sub-groups) ────────────────────────────────────────────
    /// Task config: init | show
    Config {
        #[command(subcommand)]
        action: task::buyer::ConfigAction,
    },

    /// Dispute actions (provider): raise, evidence, info, upload
    #[command(subcommand)]
    Dispute(task::provider::DisputeCommand),

    /// Evaluator actions (arbitrator): info, commit, reveal, claim, forget, stake
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
    SensitiveWords {
        #[arg(long)]
        agent_id: String,
    },

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
    },

    /// Get XMTP system config (system account addresses)
    #[command(name = "system-config")]
    SystemConfig {
        #[arg(long)]
        agent_id: String,
    },

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
            deadline_open, deadline_submit, title,
        } => task::buyer::run_task(
            T::Create {
                description, description_summary, budget, max_budget, currency,
                deadline_open, deadline_submit, title,
            }, ctx,
        ).await,

        AgentCommand::Recommend { job_id, next, current } =>
            task::buyer::run_task(T::Recommend { job_id, next, current }, ctx).await,

        AgentCommand::Status { job_id } =>
            task::buyer::run_task(T::Status { job_id }, ctx).await,

        AgentCommand::List { role, status, page, limit } =>
            task::buyer::run_task(T::List { role, status, page, limit }, ctx).await,

        AgentCommand::ConfirmAccept { job_id, provider, payment_mode } =>
            task::buyer::run_task(T::ConfirmAccept { job_id, provider, payment_mode }, ctx).await,

        AgentCommand::RejectApply { job_id, provider, reason } =>
            task::buyer::run_task(T::RejectApply { job_id, provider, reason }, ctx).await,

        AgentCommand::Complete { job_id } =>
            task::buyer::run_task(T::Complete { job_id }, ctx).await,

        AgentCommand::Reject { job_id, reason } =>
            task::buyer::run_task(T::Reject { job_id, reason }, ctx).await,

        AgentCommand::Close { job_id } =>
            task::buyer::run_task(T::Close { job_id }, ctx).await,

        AgentCommand::SetPublic { job_id } =>
            task::buyer::run_task(T::SetPublic { job_id }, ctx).await,

        AgentCommand::Payment { job_id } =>
            task::buyer::run_task(T::Payment { job_id }, ctx).await,

        AgentCommand::Pay { job_id } =>
            task::buyer::run_task(T::Pay { job_id }, ctx).await,

        AgentCommand::Claim { job_id } =>
            task::buyer::run_task(T::Claim { job_id }, ctx).await,

        AgentCommand::ClaimAutoRefund { job_id } =>
            task::buyer::run_task(T::ClaimAutoRefund { job_id }, ctx).await,

        // ── Provider task commands ──────────────────────────────────
        AgentCommand::RecommendTask { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::provider::recommend_task::handle_recommend_task(&mut c, agent_id.as_deref()).await
        }

        AgentCommand::FindJobs =>
            task::provider::find_jobs::handle_find_jobs().await,

        AgentCommand::ContactBuyer { to_agent_id, job_id, message } =>
            task::provider::contact_buyer::handle_contact_buyer(&to_agent_id, &job_id, message.as_deref()).await,

        AgentCommand::Apply { job_id, token_amount, token_symbol, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Apply { job_id, token_amount, token_symbol, agent_id },
                ctx,
            ).await,

        AgentCommand::Deliver { job_id, file, message } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Deliver { job_id, file, message }, ctx,
            ).await,

        AgentCommand::AgreeRefund { job_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::AgreeRefund { job_id }, ctx,
            ).await,

        AgentCommand::GetPayment { job_id, token_symbol } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::provider::get_payment::handle_get_payment(&mut c, &job_id, &token_symbol).await
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
            // 状态脱节 → block 输出剧本（避免 sub 按 stale event 跑老剧本上链）
            // 只在 PSEUDO_EVENTS / unknown / network failure 时跳过校验，正常情况下严格守门
            if let Some(w) = check_status_freshness(&job_id, &job_status).await {
                println!("{w}");
                return Ok(());
            }
            let prompt = match role.as_str() {
                "provider" | "seller" =>
                    task::provider::flow::generate_next_action(&job_id, &job_status, &agent_id),
                "buyer" | "client" =>
                    task::buyer::flow::generate_next_action(&job_id, &job_status, &agent_id),
                "evaluator" =>
                    task::evaluator::flow::generate_next_action(&job_id, &job_status, &agent_id),
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

        AgentCommand::SensitiveWords { agent_id } =>
            chat::run(chat::ChatCommand::SensitiveWords { agent_id }, ctx).await,

        AgentCommand::MessageEligible { agent_id, client_agent_id, provider_agent_id, job_id, group_id, direction } =>
            chat::run(
                chat::ChatCommand::MessageEligible {
                    agent_id, client_agent_id, provider_agent_id, job_id, group_id, direction,
                },
                ctx,
            ).await,

        AgentCommand::SystemConfig { agent_id } =>
            chat::run(chat::ChatCommand::SystemConfig { agent_id }, ctx).await,

        AgentCommand::Heartbeat { chain_index } =>
            chat::run(chat::ChatCommand::Heartbeat { chain_index }, ctx).await,
    }
}

/// 比对 next-action 入参的 jobStatus/event 暗示的 status 与任务真实 statusStr，
/// 不一致时返回一段 warning 文本（用于 prepend 到剧本输出顶部）。
///
/// 触发场景：system event 延迟、之前的 CLI 操作已经把 status 推得更靠前、
/// 或者 mock 测试中手动选 event 跟实际不一致。
/// 网络/解析失败时返回 None（不阻塞剧本输出，graceful fallback）。
async fn check_status_freshness(job_id: &str, job_status_or_event: &str) -> Option<String> {
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
        return None;
    }

    let mut c = TaskApiClient::new();
    let resp = c.get(&c.task_path(job_id)).await.ok()?;
    let actual_str = resp.get("task")?.get("statusStr")?.as_str()?.to_string();
    let actual = Status::parse(&actual_str);

    if actual == expected {
        return None;
    }
    Some(format!(
        "🛑 **状态脱节，剧本已 block**（next-action 入参与任务真实状态不一致，不输出步骤防止你按 stale event 上链）\n\n\
         - 你传的 jobStatus/event = `{job_status_or_event}`，对应任务状态应为 `{expected_str}`\n\
         - 但任务 {job_id} 真实 statusStr = `{actual_str}`\n\n\
         **必须做**：重调 next-action 并传 `--jobStatus {actual_str}`（按真实状态拿剧本），或忽略本条过期通知结束 turn 等下一个真实链事件。\n\
         **禁止做**：不要硬猜下一步、不要在没拿到剧本前调任何 task CLI、不要把这条警告当 STATUS_NOTIFY 推 user session。\n",
        expected_str = expected.as_str(),
    ))
}
