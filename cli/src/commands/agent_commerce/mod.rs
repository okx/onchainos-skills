pub mod chat;
pub mod identity;
pub mod task;

use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::Context;

use task::common::DEBUG_LOG;

/// Shared `agent` namespace for identity + task-system commands.
#[derive(Subcommand)]
pub enum AgentCommand {
    // ── Identity ────────────────────────────────────────────────────────────
    /// Register a new Agent identity
    Create(identity::CreateArgs),

    /// Update Agent identity and services
    Update(identity::UpdateArgs),

    /// Query your Agents / agent details (agent-list; optional --agent-ids / --page / --page-size)
    #[command(name = "get", hide = true)]
    Get(identity::GetArgs),

    /// List your own Agents (optional --role / --owner-address filters)
    #[command(name = "get-my-agents")]
    GetMyAgents(identity::GetMyAgentsArgs),

    /// Query Agent details by ID(s) (batch-list lookup)
    #[command(name = "get-agents")]
    GetAgents(identity::GetAgentsArgs),

    /// Unified registration entry (role required, consentKey optional): first-time consent + per-wallet uniqueness verdict
    #[command(name = "pre-check")]
    Precheck(identity::PrecheckArgs),

    /// Reverse-lookup an Agent by communication address + chainIndex (hidden, internal).
    #[command(name = "get-by-address", hide = true)]
    GetByAddress(identity::GetByAddressArgs),

    /// Activate an Agent (agent-status + submit-approval; QA runs at register/update, not here)
    Activate(identity::ActivateArgs),

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

    /// Get the feedback a rater left on a task (by rater agentId + taskId)
    #[command(name = "task-feedback")]
    TaskFeedback(identity::TaskFeedbackArgs),

    /// Sign an arbitrary message with keyUuid + signing_seed (xmtp etc.); does not broadcast
    #[command(name = "xmtp-sign", hide = true)]
    XmtpSign(identity::XmtpSignArgs),

    /// Validate an Agent listing's fields against marketplace rules (pure-local, no network)
    #[command(name = "validate-listing", hide = true)]
    ValidateListing(identity::ValidateListingArgs),

    // ── Task system (Client) ────────────────────────────────────────────────
    /// Create a new task (Client)
    #[command(name = "create-task")]
    CreateTask {
        #[arg(long)]
        description: String,
        #[arg(long)]
        budget: f64,
        #[arg(long = "max-budget")]
        max_budget: f64,
        #[arg(long)]
        currency: String,
        #[arg(long)]
        title: Option<String>,
        /// Specified provider agentId (required; skip asp-match, negotiate directly with this provider or x402 accept)
        #[arg(long)]
        provider: String,
        /// Designated service endpoint (persisted for multi-service providers)
        #[arg(long)]
        endpoint: Option<String>,
        /// Local file paths to attach to the task after creation.
        #[arg(long = "file")]
        attachments: Option<Vec<String>>,
        /// Payment mode to set at creation time (required; escrow / x402).
        #[arg(long = "payment-mode")]
        payment_mode: String,
        /// Service ID from asp/match response (required)
        #[arg(long = "service-id")]
        service_id: String,
        /// Service input parameters (natural language string)
        #[arg(long = "service-params")]
        service_params: Option<String>,
        /// Service token contract address
        #[arg(long = "service-token-address")]
        service_token_address: Option<String>,
        /// Service price (from asp/match feeAmount)
        #[arg(long = "service-token-amount")]
        service_token_amount: Option<String>,
        /// Accepted for compatibility but ignored — user identity is auto-resolved.
        #[arg(long = "agentId", alias = "agent-id", hide = true)]
        _agent_id: Option<String>,
    },

    /// Create a subscription task
    #[command(name = "create-subscribe")]
    CreateSubscribe {
        #[arg(long = "service-id")]
        service_id: String,
        #[arg(long = "use-trial", action = clap::ArgAction::Set, value_parser = clap::builder::BoolishValueParser::new(), default_value = "false")]
        use_trial: bool,
        #[arg(long = "service-params", default_value = "")]
        service_params: String,
        #[arg(long = "service-token-amount")]
        service_token_amount: String,
        #[arg(long = "service-token-address")]
        service_token_address: String,
        #[arg(long = "auto-renew")]
        auto_renew: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        description: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: Option<String>,
        /// Exact service description returned by asp-match. Only bounded
        /// asset/tool hints are persisted; the raw prose is never executed.
        #[arg(long = "service-description", default_value = "")]
        service_description: String,
        #[arg(long = "service-interval", default_value = "month")]
        service_interval: String,
        /// Explicit user-confirmed automatic signal execution (`auto`).
        #[arg(long = "autotrade-mode")]
        autotrade_mode: Option<String>,
        /// Fixed quote amount used for every delivered signal.
        /// [UNIT: human-readable decimal selected by --autotrade-quote; number only,
        /// e.g. 20.5; never minimal units; do not include a USDT/USDC suffix.]
        #[arg(long = "autotrade-amount")]
        autotrade_amount: Option<String>,
        /// Per-delivery automatic-execution quote cap.
        /// [UNIT: human-readable decimal selected by --autotrade-quote; number only,
        /// e.g. 50; never minimal units; do not include a USDT/USDC suffix.]
        #[arg(long = "autotrade-cap")]
        autotrade_cap: Option<String>,
        /// Quote currency for amount/cap (`usdt` or `usdc`).
        #[arg(long = "autotrade-quote")]
        autotrade_quote: Option<String>,
        #[arg(long, default_value = "")]
        format: String,
        /// Legacy compatibility input. Create-time device selection is rejected.
        #[arg(long = "exclude-device", hide = true)]
        exclude_device: Option<Vec<String>>,
    },

    /// Cancel a subscription (unified: trial cancel + close auto-renew)
    #[command(name = "subscribe-cancel")]
    SubscribeCancel { sub_id: String },

    /// Enable auto-renew on a subscription (needs EIP-712 terms signing)
    #[command(name = "start-autorenew")]
    StartAutorenew { sub_id: String },

    /// Reject a subscription delivery
    #[command(name = "subscribe-reject")]
    SubscribeReject {
        sub_id: String,
        #[arg(long)]
        reason: String,
    },

    /// Show subscription detail
    #[command(name = "subscribe-detail")]
    SubscribeDetail {
        sub_id: String,
        #[arg(long, default_value = "")]
        format: String,
    },

    /// Show total monthly cost of active subscriptions
    #[command(name = "subscribe-cost")]
    SubscribeCost {},

    /// Overwrite the receive-device list for one or more subscriptions (batch).
    /// The passed list wholly replaces the stored list; empty/omitted clears it.
    #[command(name = "subscribe-device-update")]
    SubscribeDeviceUpdate {
        /// Subscription jobId to overwrite (Form A, single subscription).
        #[arg(long = "job-id")]
        job_id: Option<String>,
        /// Comma-separated device ids (Form A); empty/omitted clears the list.
        #[arg(long = "device-list")]
        device_list: Option<String>,
        /// JSON array of {jobId, deviceList} (Form B, batch). Mutually exclusive with --job-id/--device-list.
        #[arg(long, conflicts_with_all = ["job_id", "device_list"])]
        items: Option<String>,
    },

    /// Set a subscription's offline-receive flag (0 = keep backlog, 1 = discard backlog).
    #[command(name = "subscribe-offline-update")]
    SubscribeOfflineUpdate {
        /// Subscription jobId whose offline-receive flag is being set.
        #[arg(long = "job-id")]
        job_id: String,
        /// Offline-receive flag: `0` keeps the backlog, `1` discards it.
        #[arg(long)]
        flag: String,
    },

    /// List the devices this agent is logged in on (paginated to completion).
    #[command(name = "device-list")]
    DeviceList {
        /// Starting page (`<1` normalized to 1).
        #[arg(long, default_value = "1")]
        page: i64,
        /// Page size (`<1` normalized to 20; `>100` → backend error 81001).
        #[arg(long = "page-size", default_value = "20")]
        page_size: i64,
    },

    /// Search matching ASPs for an existing task
    #[command(name = "asp-match")]
    AspMatch {
        /// Existing task ID
        #[arg(long = "job-id")]
        job_id: String,
        /// Narrow to this ASP's services
        #[arg(long = "provider-agent-id")]
        provider_agent_id: Option<String>,
        /// Budget amount for backend filtering
        #[arg(long = "payment-token-amount")]
        payment_token_amount: Option<f64>,
        /// Page number
        #[arg(long, default_value = "1")]
        page: usize,
        /// User agent ID
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// Output format: "json" for raw JSON (no formatted list)
        #[arg(long, default_value = "")]
        format: String,
    },

    /// Set/replace ASP + service on existing task (off-chain, triggers job_asp_selected)
    #[command(name = "set-asp")]
    SetAsp {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        #[arg(long = "service-id")]
        service_id: String,
        #[arg(long = "service-type")]
        service_type: String,
        #[arg(long = "service-params")]
        service_params: String,
        #[arg(long = "service-token-address")]
        service_token_address: String,
        #[arg(long = "service-token-amount")]
        service_token_amount: String,
        #[arg(long = "payment-token-symbol")]
        payment_token_symbol: Option<String>,
        #[arg(long = "payment-token-amount")]
        payment_token_amount: Option<String>,
        #[arg(long = "payment-most-token-amount")]
        payment_most_token_amount: Option<String>,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Clear ASP + service fields (off-chain)
    #[command(name = "reset-asp")]
    ResetAsp {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Reject current ASP (off-chain, triggers job_user_reject)
    #[command(name = "user-reject")]
    UserReject {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Mark a provider as failed negotiation (excluded from future asp-match lists)
    #[command(name = "mark-failed")]
    MarkFailed {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
    },

    /// Get current task status
    Status {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// List "tasks I have" (accepted / published by me).
    #[command(visible_alias = "list")]
    Tasks {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "1")]
        page: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// List the logged-in agent's AI-service subscriptions (buyer or provider view).
    #[command(name = "my-subscriptions")]
    MySubscriptions {
        /// Subscription viewpoint: buyer (subscriber) or provider (ASP).
        #[arg(long, value_enum, default_value_t = task::user::subscription_ops::SubscriptionRole::Buyer)]
        role: task::user::subscription_ops::SubscriptionRole,
        /// Optional status filter, applied client-side. Accepts a status code
        /// (-1/1/3/4/6/7/9) or a case-insensitive name (INIT/ACTIVE/REJECTED/
        /// DISPUTED/COMPLETED/CLOSED/FAILED).
        #[arg(long, value_parser = task::user::subscription_ops::parse_status_filter)]
        status: Option<i32>,
    },

    /// Aggregated non-terminal tasks across **all agents under the current
    /// active account**, with `myRole` / `counterpartyAgentId` annotations so
    /// the user-session can route ad-hoc user instructions to the correct sub
    /// session (via `okx-a2a session query` → `okx-a2a session send --no-wait`).
    /// Status filter: includes 0 created / 1 accepted / 2 submitted / 3 refused
    /// / 4 disputed by default; pass `--include-terminal` to also list 5-9.
    #[command(name = "active-tasks")]
    ActiveTasks {
        /// Optional role filter: user | asp | evaluator
        #[arg(long)]
        role: Option<String>,
        /// Include terminal statuses (complete / close / expired / rejected / admin_stopped)
        #[arg(long = "include-terminal")]
        include_terminal: bool,
    },

    /// Set payment mode on-chain (standalone, before confirm-accept)
    #[command(name = "set-payment-mode")]
    SetPaymentMode {
        job_id: String,
        /// escrow / x402
        #[arg(long = "payment-mode")]
        payment_mode: Option<String>,
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
        /// x402 service endpoint URL
        #[arg(long)]
        endpoint: Option<String>,
    },

    /// Client confirms provider and executes payment (setPaymentMode must be done first).
    /// All parameters are auto-resolved from the task detail API.
    #[command(name = "confirm-accept")]
    ConfirmAccept { job_id: String },

    /// x402 Phase 2: x402_pay signing + direct/accept + endpoint replay
    #[command(name = "task-402-pay")]
    Task402Pay {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        /// JSON accepts array from the HTTP 402 response
        #[arg(long)]
        accepts: String,
        /// x402 provider endpoint URL (for replay after signing)
        #[arg(long)]
        endpoint: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        /// Payer address (optional)
        #[arg(long)]
        from: Option<String>,
        /// JSON business body to POST during replay (for endpoints that require business parameters)
        #[arg(long)]
        body: Option<String>,
        /// Bypass the confirming gate and broadcast the on-chain accept immediately (FR-7.3)
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Validate an x402 endpoint and extract pricing info
    #[command(name = "x402-check")]
    X402Check {
        /// x402 provider endpoint URL
        #[arg(long)]
        endpoint: String,
        /// User agent ID (used for auth on token detail queries)
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// JSON business body to POST (for endpoints that require business parameters to return 402)
        #[arg(long)]
        body: Option<String>,
    },

    /// Designated-provider routing: service-list + profile in one call
    #[command(name = "designated-route")]
    DesignatedRoute {
        /// Target provider agentId
        #[arg(long)]
        provider: String,
        /// Target service endpoint (for multi-service providers)
        #[arg(long)]
        endpoint: Option<String>,
    },

    /// Validate x402 endpoint + price match + budget check in one call
    #[command(name = "x402-validate")]
    X402Validate {
        /// x402 provider endpoint URL
        #[arg(long)]
        endpoint: String,
        /// User agent ID
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Job ID (for budget lookup)
        #[arg(long = "job-id")]
        job_id: String,
        /// Registered fee amount from designated-route
        #[arg(long = "fee-amount")]
        fee_amount: String,
        /// Registered fee token symbol from designated-route
        #[arg(long = "fee-token")]
        fee_token: String,
    },

    /// Client confirms task complete and releases payment
    Complete { job_id: String },

    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)]
        reason: String,
    },

    /// Client closes task (only valid while Open)
    Close {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Provider generates payment invoice after provider_applied
    Payment {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Provider account-pull: query pending claimable rewards
    #[command(name = "asp-claimable")]
    AspClaimable {
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider account-pull: claim all pending rewards in one call
    #[command(name = "asp-claim-rewards")]
    AspClaimRewards {
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// List agents belonging to the **current active account**, flat output.
    ///
    /// Wrapper over `fetch_my_agents` — hides the agent-list response shape
    /// (`data[0].list[].agentList[]` nesting) from the LLM. Optional `--role`
    /// filter; output is a flat JSON array `[{agentId, name, role, status, ...}]`
    /// already scoped to the current account's XLayer ownerAddress.
    #[command(name = "my-agents")]
    MyAgents {
        /// Optional role filter: user | asp | evaluator
        #[arg(long)]
        role: Option<String>,
    },

    /// Business gate-check: wallet login + agent identity + communication channel.
    /// Pure read-only diagnostic — does not modify any state.
    #[command(name = "gate-check")]
    GateCheck {
        /// Role to check identity for: user | asp | evaluator
        #[arg(long)]
        role: String,
    },

    /// Prepare-create: validate fields + gate-check + designated-route in one call.
    /// Returns structured JSON for the confirmation form. Does NOT create the task.
    #[command(name = "prepare-create")]
    PrepareCreate {
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long = "max-budget")]
        max_budget: Option<f64>,
        #[arg(long)]
        currency: Option<String>,
        #[arg(long)]
        provider: Option<String>,
    },

    /// Look up a single agent's profile by `agentId` (any owner, not limited
    /// to current account). Wrapper over `agent get-agents --agent-ids` that flattens
    /// the `list[].agentList[]` nesting and returns the matched agent as a
    /// single flat object. Used for verifying peer / designated provider
    /// identities (e.g. user-sub-playbook.md Provider validation).
    ///
    /// `ok: false` when not found / agentId malformed; otherwise `data` is
    /// the agent object `{agentId, name, role, status, ownerAddress,
    /// communicationAddress, agentWalletAddress, profileDescription, ...}`.
    Profile {
        /// Target agentId (ERC-8004 token ID, decimal string)
        agent_id: String,
    },

    // ── Task system (Provider) ──────────────────────────────────────────────
    /// Provider applies for a task (apply API → sign → broadcast)
    Apply {
        job_id: String,
        /// Negotiated token amount. **Required**; must be > 0 (empty / 0 = apply for free, irreversible — CLI rejects).
        #[arg(long = "token-amount")]
        token_amount: String,
        /// Actual task currency (USDT / USDG); read from negotiation context, do not assume USDT
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider submits deliverable (submit API → sign → broadcast)
    Deliver {
        job_id: String,
        #[arg(long, default_value = "")]
        file: String,
        #[arg(long, default_value = "Task completed, please review")]
        message: String,
        /// Text deliverable content for auto-save. When non-empty and --file is empty,
        /// the CLI writes this to a temp file and persists it as a text deliverable.
        #[arg(long = "deliverable-text", default_value = "")]
        deliverable_text: String,
        /// Provider agentId (required). Beta backend rejects empty agenticId header → 3001 auth fail.
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Deprecated compatibility argument. Accepted but ignored; only the
        /// explicit text/file deliverable is sent and processed.
        #[arg(long, default_value = "")]
        autotrade: String,
    },

    /// Check a per-trade amount against the buyer's written authorization (bespoke
    /// `{ok,reason?}` process contract; NOT the standard `data` envelope).
    #[command(name = "autotrade-grant-check")]
    AutotradeGrantCheck {
        #[arg(long = "job-id")]
        job_id: String,
        /// `dex` | `hyperliquid` (alias of `dex`) | `defi` | `polymarket` | `trade_kit`.
        #[arg(long)]
        venue: String,
        /// `buy` | `sell`.
        #[arg(long)]
        action: String,
        /// Decimal per-trade amount to check against the written cap (mandatory).
        #[arg(long)]
        amount: String,
        /// Only `json` is accepted; any other value denies.
        #[arg(long)]
        format: String,
    },

    /// [dev only] Seed a local grant file for testing (compiled out of release; AC-11).
    #[cfg(debug_assertions)]
    #[command(name = "autotrade-grant-write", hide = true)]
    AutotradeGrantWrite {
        #[arg(long = "job-id")]
        job_id: String,
        /// `dex` | `hyperliquid` (alias of `dex`) | `defi` | `polymarket` | `trade_kit`.
        #[arg(long)]
        venue: String,
        #[arg(long = "max-buy")]
        max_buy: Option<String>,
        #[arg(long = "max-sell")]
        max_sell: Option<String>,
        #[arg(long = "ttl-sec")]
        ttl_sec: u64,
    },

    /// [consent flow, 2026-07-17] Persist the buyer's auto-trade consent for a
    /// subscription (auto/manual/decline + per-trade cap). This command persists
    /// policy only; the model session retains and executes the current delivery.
    #[command(name = "autotrade-consent-set")]
    AutotradeConsentSet {
        #[arg(long = "job-id")]
        job_id: String,
        /// `auto` | `manual` | `decline` | `pause` | `cap-adjust` | `plugin-ready-check`.
        #[arg(long)]
        mode: String,
        /// Per-trade cap in quote-stablecoin units (USDT by default); required for `auto`.
        #[arg(long)]
        cap: Option<String>,
        /// Fixed quote-stablecoin amount used by the model-driven subscription.
        #[arg(long = "trade-amount")]
        trade_amount: Option<String>,
        /// Buyer agent id (optional; retained only for rolling CLI compatibility).
        /// Local consent operations, including `pause`, do not use it.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
        /// Consent lifetime in seconds (default 365 days).
        #[arg(long = "ttl-sec", default_value_t = 31_536_000)]
        ttl_sec: u64,
        /// Plugin-store id checked by `plugin-ready-check` (the legacy
        /// `plugin-approved` alias is retained for compatibility).
        #[arg(long)]
        plugin: Option<String>,
        /// Deprecated. Model routes are persisted with `subscription-route-set`.
        #[arg(long)]
        tool: Option<String>,
        /// Quote stablecoin dex trades pay with / settle into: `usdc` | `usdt`.
        /// Pass ONLY when the user named one; omitted keeps the stored choice
        /// (or the default, USDT).
        #[arg(long)]
        quote: Option<String>,
    },

    /// Queue and push a delivery's consent/manual decision. The CLI adds a
    /// bounded canonical delivery summary before the unchanged choices.
    #[command(name = "autotrade-consent-request", hide = true)]
    AutotradeConsentRequest {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        #[arg(long = "delivery-id")]
        delivery_id: String,
        #[arg(long = "signal-type")]
        signal_type: String,
    },

    /// Persist a short-lived, exact-delivery permit after the user chooses the
    /// over-cap card's one-time execution option.
    #[command(name = "autotrade-once-authorize", hide = true)]
    AutotradeOnceAuthorize {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long = "delivery-id")]
        delivery_id: String,
        #[arg(long)]
        amount: String,
    },

    /// Execute one admitted delivery through a venue-specific CLI and
    /// deterministically report its terminal result to the job UI.
    #[command(name = "autotrade-execute", hide = true)]
    AutotradeExecute {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long = "delivery-id")]
        delivery_id: String,
        /// dex | defi | trade_kit | polymarket | hyperliquid
        #[arg(long)]
        venue: String,
        /// buy | sell
        #[arg(long)]
        action: String,
        /// Exact persisted policy amount.
        #[arg(long)]
        amount: String,
        /// `auto` for persisted grant execution; `manual` for the persisted
        /// manual policy; `one_time` for an exact over-cap permit.
        #[arg(long = "execution-mode", default_value = "auto")]
        execution_mode: String,
        /// JSON array containing only the target CLI's argv (no program/shell).
        #[arg(long = "command-json")]
        command_json: String,
        #[arg(long = "timeout-sec", default_value_t = 120)]
        timeout_sec: u64,
    },

    /// Retry only pending UI notifications; never retries a trade command.
    #[command(name = "autotrade-outcome-flush", hide = true)]
    AutotradeOutcomeFlush {
        #[arg(long = "job-id")]
        job_id: String,
    },

    /// Persist and notify a terminal delivery result reached before a trade
    /// command exists. This closes headless Job Session processing paths.
    #[command(name = "autotrade-delivery-report", hide = true)]
    AutotradeDeliveryReport {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long = "delivery-id")]
        delivery_id: String,
        /// skipped | failed_before_execution
        #[arg(long)]
        status: String,
        /// Concise user-safe reason; command output and credentials are forbidden.
        #[arg(long)]
        reason: String,
    },

    /// Persist a bounded model-selected route for an Active subscription.
    #[command(name = "subscription-route-set", hide = true)]
    SubscriptionRouteSet {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long = "asset-class")]
        asset_class: String,
        #[arg(long = "skill-id")]
        skill_id: String,
        #[arg(long = "plugin-id")]
        plugin_id: Option<String>,
        #[arg(long)]
        protocol: Option<String>,
        #[arg(long = "requirement")]
        requirements: Vec<String>,
        #[arg(long = "delivery-id")]
        delivery_id: String,
    },

    /// Clear cached model routes after an explicit incompatibility or reset.
    #[command(name = "subscription-route-clear", hide = true)]
    SubscriptionRouteClear {
        #[arg(long = "job-id")]
        job_id: String,
    },

    /// Ask whether to raise the cap after a successful over-cap one-shot.
    #[command(name = "autotrade-cap-adjust-request", hide = true)]
    AutotradeCapAdjustRequest {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    #[command(name = "agree-refund")]
    AgreeRefund {
        job_id: String,
        /// Provider agentId (required)
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider declines a user-designated task (off-chain backend call, no signing).
    /// Used by the `job_asp_selected` flow when capability / price gate fails.
    #[command(name = "asp-reject")]
    AspReject {
        job_id: String,
        /// Provider agentId (required)
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Optional decline reason recorded by the backend.
        #[arg(long, default_value = "")]
        reason: String,
    },

    /// ASP: list my still-active subscription jobs (continuous-delivery phase) as a JSON array
    /// — the resident dispatch script's fan-out set. Source: GET /subscribe/my.
    #[command(name = "subscribe-active")]
    SubscribeActive {
        /// ASP agentId (required)
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// ASP: agree to refund a rejected subscription period
    /// (POST /subscribe/{subId}/agreeRefund → sign → broadcast). The "agree refund" outcome
    /// of a `sub_user_reject` decision.
    #[command(name = "subscribe-agree-refund")]
    SubscribeAgreeRefund {
        job_id: String,
        /// ASP agentId (required)
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// ASP: claim accrued, not-yet-claimed subscription income
    /// (POST /subscribe/{subId}/aspClaim → sign → broadcast).
    /// Prompted by the `sub_renew` notification (the previous period's income becomes
    /// claimable at renewal); also safe to run ad-hoc — claims everything outstanding.
    #[command(name = "subscribe-asp-claim")]
    SubscribeAspClaim {
        job_id: String,
        /// ASP agentId (required)
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// ASP: raise arbitration for a rejected subscription period via the §2.10 single combined
    /// endpoint (POST /task/{jobId}/dispute/approveAndCreateDispute → sign → broadcast). The
    /// "dispute" outcome of a `sub_user_reject` decision.
    #[command(name = "subscribe-dispute")]
    SubscribeDispute {
        job_id: String,
        /// ASP's dispute reason — persisted on-chain via the broadcast bizContext (like
        /// `dispute confirm`). Optional; omitted → empty reason.
        #[arg(long = "reason")]
        reason: Option<String>,
        /// ASP agentId (required)
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Client claims auto-refund after provider timeout
    #[command(name = "claim-auto-refund")]
    ClaimAutoRefund { job_id: String },

    /// Reject a provider's apply (on-chain pass-through; status stays `created`)
    #[command(name = "reject-apply")]
    RejectApply {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Send a user-facing notification via okx-a2a (fire-and-forget)
    #[command(name = "user-notify")]
    UserNotify {
        #[arg(long)]
        content: String,
        /// Optional image attachment path. Encoded as MEDIA:<path> for runtimes that render media markers.
        #[arg(long = "image-path")]
        image_path: Option<PathBuf>,
    },

    /// Build a localizable insufficient-funding notice and deposit QR PNG.
    #[command(name = "funding-notice")]
    FundingNotice(task::common::funding_notice::FundingNoticeArgs),

    /// Cache a pre-translated notification for a future event playbook so the
    /// on-chain event can dispatch `user-notify` without an LLM translation
    /// round-trip. Populated by the user sub at prefetch time.
    #[command(name = "cache-notify", hide = true)]
    CacheNotify {
        #[arg(long = "job-id")]
        job_id: String,
        /// Event key, e.g. `job_completed_escrow`
        #[arg(long = "event-key")]
        event_key: String,
        /// Translated, ready-to-dispatch notification content
        #[arg(long)]
        content: String,
    },

    /// Cache a pre-decided ASP rating (score + comment) so the `job_completed`
    /// playbook can dispatch `feedback-submit` in-process without an LLM
    /// decision round-trip. Populated by the user sub at deliverable_received
    /// time based on the deliverable content vs task description.
    #[command(name = "cache-rating", hide = true)]
    CacheRating {
        #[arg(long = "job-id")]
        job_id: String,
        /// Score in `X.XX` form (0.00–5.00)
        #[arg(long)]
        score: String,
        /// Reviewer comment (≤100 chars)
        #[arg(long)]
        comment: String,
    },

    /// Attach local file(s) to a task
    #[command(name = "task-attach")]
    TaskAttach {
        job_id: String,
        /// Path(s) to the file(s) to attach (repeatable, at least one required)
        #[arg(long = "file", required = true)]
        file_paths: Vec<String>,
    },
    /// List attachments for a task
    #[command(name = "list-attachments")]
    ListAttachments { job_id: String },

    /// Save a deliverable file to persistent local storage
    #[command(name = "task-deliverable-save")]
    TaskDeliverableSave {
        #[arg(long)]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        file: String,
        #[arg(long, default_value = "file")]
        deliverable_type: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        short_id: String,
        #[arg(long = "file-key")]
        file_key: Option<String>,
        #[arg(long = "token-symbol")]
        token_symbol: Option<String>,
        #[arg(long = "token-amount")]
        token_amount: Option<String>,
        #[arg(long = "counterparty-agent-id")]
        counterparty_agent_id: Option<String>,
        #[arg(long = "counterparty-name")]
        counterparty_name: Option<String>,
    },

    /// List deliverables for a job or all jobs
    #[command(name = "task-deliverable-list")]
    TaskDeliverableList {
        /// If provided, list deliverables for this job only
        #[arg(long)]
        job_id: Option<String>,
        #[arg(long, default_value = "user")]
        role: String,
        /// Substring search across all jobs (only used when --job-id is omitted)
        #[arg(long)]
        search: Option<String>,
    },

    /// Provider claims auto-complete after user review timeout (review_expired)
    #[command(name = "claim-auto-complete")]
    ClaimAutoComplete {
        job_id: String,
        /// Provider's own agentId
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    // ── Task system (sub-groups) ────────────────────────────────────────────
    /// Dispute actions (provider): raise, evidence, info, upload
    #[command(subcommand)]
    Dispute(task::asp::DisputeCommand),

    /// Pending-decisions v2 queue management.
    #[command(name = "pending-decisions-v2", subcommand)]
    PendingDecisionsV2(task::common::pending_v2::PendingDecisionsV2Command),

    // ── Task system (Evaluator Agent) ────────────────────────────────────────
    // Historically wrapped as `Evaluator(EvaluatorCommand)`; flattened to the top level in 2026-05
    // to align with the user/provider style. The `agent evaluator <sub>` form is no longer supported;
    // see the file header comment in `evaluator/mod.rs` for per-command correspondence.
    /// Fetch dispute evidence: each side's `reason` (provider = dispute-raise reason; client =
    /// reject-delivery reason), `texts[]` (free text), and `files[]` (any file type, downloaded
    /// locally **without extensions** — the evaluator agent probes type itself via `file
    /// --mime-type` per the playbook). Backend resolves the active dispute round from jobId —
    /// CLI does not need disputeId.
    ///
    /// Internal precondition gate (merged from the former `dispute-status`): before fetching/downloading
    /// evidence, validate that `taskStatus` is non-terminal / `--round-num` == on-chain currentRound /
    /// `disputeStatus = CommitPhase` / the current account is hit in this round's selectedVoter. If any
    /// check fails, output `reason: ...` + `selected: no` and early-return without downloading (to avoid
    /// later commit being slashed due to a stale envelope). If all pass, output `selected: yes` and
    /// continue to download and print the evidence JSON.
    #[command(name = "evidence-info")]
    EvidenceInfo {
        job_id: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Pass-through of inbound envelope's top-level `roundNum` — compared against on-chain currentRound to detect a stale envelope.
        #[arg(long = "round-num")]
        round_num: String,
    },
    /// Commit a vote (Phase 1 of commit-reveal). vote: 0 = Approve (Client wins), 1 = Reject (Provider wins).
    /// Broadcast bizContext carries `{ vote, voteReport, voteReportSummary }`. Backend resolves the active
    /// dispute round from jobId.
    #[command(name = "vote-commit")]
    VoteCommit {
        job_id: String,
        #[arg(long)]
        vote: u8,
        /// Full verdict text produced by Step 5 per the Verdict template defined in
        /// `references/evaluator-decision-rubric.md` (whichever heading the user-customized
        /// rubric uses to define it; required). Sent to backend in the broadcast bizContext as
        /// `voteReport` — the human-readable on-chain audit trail; whatever fields the rubric's
        /// Verdict template prescribes. Flatten to a single line with `\n` / `\t` / `\r` / `\\` / `\"`
        /// escapes (CLI unescapes before transport); escape `"` / `` ` `` / `$` to survive the shell.
        #[arg(long = "reason")]
        reason: String,
        /// One-sentence summary of the verdict, ≤30 Unicode characters (counted by
        /// `chars().count()`). Produced by Step 5 by compressing the full verdict text;
        /// sent to backend as `voteReportSummary` alongside the full `voteReport`. Empty
        /// values and overlength inputs are rejected by the CLI.
        #[arg(long = "reason-summary")]
        reason_summary: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Reveal a previously-committed vote (Phase 2 of commit-reveal). Driven by the
    /// `reveal_started` system event. CLI sends an empty body `{}` — backend reads
    /// vote+salt from `task_dispute_voter` keyed by (active dispute round, voter),
    /// so no `--vote` is required.
    #[command(name = "vote-reveal")]
    VoteReveal {
        job_id: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Claim arbitration reward after task/dispute resolved. Account-level pull — one call drains
    /// every pending reward across all settled disputes (POST /task/claim, no jobId).
    /// Distinct from user's `claim` (which pulls per-job refund/reward).
    #[command(name = "arbitration-claim")]
    ArbitrationClaim {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// List account-level claimable arbitration rewards across all settled disputes
    /// (GET /task/claimable). Read-only; no tx.
    #[command(name = "arbitration-claimable")]
    ArbitrationClaimable {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// First-time stake OKB to become an active evaluator (onboarding handoff from identity skill).
    /// Requires the current wallet's agentId to already be registered with evaluator role
    /// (identity=2). Backend enforces amount >= minCumulativeStakeOkb on first stake (see staking-config).
    /// For top-up use `increase-stake` (backend `/staking/increaseStake`).
    Stake {
        #[arg(long)]
        amount: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Top up an existing stake (no minimum). Used to replenish slashed stake or increase
    /// selection weight. Hits a different backend endpoint than `stake`.
    #[command(name = "increase-stake")]
    IncreaseStake {
        #[arg(long)]
        amount: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Request unstake: OKB enters cooldown (period from staking-config). Partial unstake supported.
    /// Backend/contract will revert if you have active dispute participation.
    #[command(name = "request-unstake")]
    RequestUnstake {
        #[arg(long)]
        amount: String,
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Claim unstaked OKB after the cooldown period. No parameters — contract knows the
    /// pending amount and unlock time.
    #[command(name = "claim-unstake")]
    ClaimUnstake {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Cancel a pending unstake request within the cooldown window; OKB returns to staked state.
    #[command(name = "cancel-unstake")]
    CancelUnstake {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Read platform staking & arbitration config (Apollo-driven, JWT auth, no body).
    /// Mirrors GET /priapi/v1/aieco/task/staking/config.
    #[command(name = "staking-config", visible_alias = "stakingconfig")]
    StakingConfig {
        /// Evaluator agentId (required — backend interceptor needs it for header auth).
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// Read the current account's on-chain stake state (activeStake / pendingUnstake /
    /// validStake / activeDisputes / cooldown timestamps / registered flag).
    /// Mirrors GET /priapi/v1/aieco/task/staking/myStake.
    #[command(name = "my-stake", visible_alias = "mystake")]
    MyStake {
        /// Evaluator agentId from inbound system envelope's top-level `agentId` field. Required.
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Common queries: context lookup for AI agents
    #[command(subcommand)]
    Common(task::common::CommonCommand),

    /// Get next-step instruction prompt for current job state.
    ///
    /// Invocation contract — exactly **three** flags:
    ///   `--role <user|asp|evaluator|auto>` — playbook routing role
    ///   `--agentId <agentId>`                    — receiving agent
    ///   `--message <envelope JSON>`              — the full `message` object
    ///                                              from the inbound notification
    ///
    /// All other inputs (`jobId`, `event`, `code`, `jobTitle`, `provider`, `data`,
    /// `peerTaskMinVersion`, etc.) are extracted from inside the `--message` JSON.
    /// This keeps the LLM-facing surface minimal: copy the envelope through, the
    /// CLI parses out whatever it needs.
    #[command(name = "next-action")]
    NextAction {
        /// Accepts both `--agentId` (legacy) and `--agent-id` (kebab).
        #[arg(long = "agentId", alias = "agent-id")]
        agent_id: String,
        /// Role: `user` / `asp` / `evaluator`, or `auto` to let the CLI
        /// resolve the role from `--agentId` (saves a separate `agent profile` round-trip).
        #[arg(long)]
        role: String,
        /// Full system event envelope as a JSON string — the entire `message` object.
        /// Required. Must contain at least `event` and `jobId`; optional fields the
        /// CLI reads: `code` / `jobTitle` / `provider` / `data` / `taskMinVersion`
        /// (plus any task-detail fields like `paymentMode` /
        /// `tokenAmount` / `tokenSymbol` / `serviceParams` that downstream scenes
        /// may consume directly).
        #[arg(long)]
        message: String,
        /// Read the full raw inbound A2A message from a secure temp JSON file.
        /// The validated path is injected into `--message` as `a2aFile` before
        /// dispatch so the user-side delivery handler can parse and save it.
        #[arg(long = "a2a-file")]
        a2a_file: Option<String>,
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
        provider_security_rate: Option<String>,
        #[arg(long)]
        client_communication_address: String,
        #[arg(long)]
        provider_communication_address: String,
        /// True only for messages recovered during communication-package startup.
        #[arg(
            long,
            action = clap::ArgAction::Set,
            value_parser = clap::builder::BoolishValueParser::new()
        )]
        is_offline_replay: Option<bool>,
    },

    /// Get XMTP system config (system account addresses)
    #[command(name = "system-config")]
    SystemConfig,

    /// Send agent heartbeat to report online status
    Heartbeat {
        #[arg(long)]
        chain_index: u64,
    },

    /// Wake up all in-flight tasks under the given agent wallets (system notify)
    #[command(name = "wakeup-notify")]
    WakeupNotify {
        /// Agent IDs to notify (comma-separated, or pass --agent-ids multiple times)
        #[arg(long, value_delimiter = ',')]
        agent_ids: Vec<String>,
    },

    /// Terminal-state session cleanup: cancel pending decisions + output
    /// `okx-a2a session delete` instructions. Replaces the multi-step
    /// manual cleanup in terminal playbooks.
    #[command(name = "session-cleanup")]
    SessionCleanup {
        #[arg(long = "job-id")]
        job_id: String,
    },

    /// Query a single Agent's (or up to 20 Agents') in-progress tasks & disputes
    /// (POST /priapi/v1/aieco/task/inProgress). The backend validates the
    /// caller→agent binding via JWT and classifies results by role
    /// (buyerTasks / providerTasks / evaluatorDisputes).
    ///
    /// Powers okx-guide node 5a (registered-user home → "view what an Agent
    /// is working on").
    ///
    /// Examples:
    ///   onchainos agent task-in-progress --agent-ids 1001
    ///   onchainos agent task-in-progress --agent-ids 1001,2002,3003
    #[command(name = "task-in-progress")]
    TaskInProgress {
        /// Agent IDs to query (comma-separated, or repeat --agent-ids). Max 20.
        #[arg(long = "agent-ids", value_delimiter = ',')]
        agent_ids: Vec<String>,
    },
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    use task::user::TaskCommand as T;

    // Opportunistic notification worker: every Agent invocation (including
    // heartbeat/watch-driven commands) retries only due persisted notices.
    // This never retries or reconstructs a transaction command.
    let _ = task::common::autotrade::executor::reconcile_terminal_journals(
        4,
        std::time::Duration::from_millis(100),
    );
    let _ = task::common::autotrade::executor::flush_all_due(4);
    let _ = task::common::autotrade::executor::cleanup_expired_tickets(8);
    let _ = task::common::autotrade::delivery_queue::flush_due(
        1,
        std::time::Duration::from_millis(100),
    );

    match cmd {
        // ── Identity ────────────────────────────────────────────────
        AgentCommand::Create(args) => identity::create(args, ctx).await,
        AgentCommand::Update(args) => identity::update(args, ctx).await,
        AgentCommand::Get(args) => identity::get(args, ctx).await,
        AgentCommand::GetMyAgents(args) => identity::get_my_agents(args, ctx).await,
        AgentCommand::GetAgents(args) => identity::get_agents(args, ctx).await,
        AgentCommand::Precheck(args) => identity::precheck(args, ctx).await,
        AgentCommand::GetByAddress(args) => identity::get_by_address(args, ctx).await,
        AgentCommand::Activate(args) => identity::activate(args, ctx).await,
        AgentCommand::Deactivate(args) => identity::deactivate(args, ctx).await,
        AgentCommand::Upload(args) => identity::upload(args, ctx).await,
        AgentCommand::Search(args) => identity::search(args, ctx).await,
        AgentCommand::ServiceList(args) => identity::service_list(args, ctx).await,
        AgentCommand::FeedbackSubmit(args) => identity::feedback_submit(args, ctx).await,
        AgentCommand::FeedbackList(args) => identity::feedback_list(args, ctx).await,
        AgentCommand::TaskFeedback(args) => identity::task_feedback(args, ctx).await,
        AgentCommand::XmtpSign(args) => identity::xmtp_sign(args, ctx).await,
        AgentCommand::ValidateListing(args) => identity::validate_listing(args, ctx).await,

        // ── Client (user) task commands ────────────────────────────
        AgentCommand::CreateTask {
            description,
            budget,
            max_budget,
            currency,
            title,
            provider,
            endpoint,
            attachments,
            payment_mode,
            service_id,
            service_params,
            service_token_address,
            service_token_amount,
            _agent_id: _,
        } => {
            task::user::run_task(
                T::Create {
                    description,
                    budget,
                    max_budget,
                    currency,
                    title,
                    provider,
                    endpoint,
                    attachments,
                    payment_mode,
                    service_id,
                    service_params,
                    service_token_address,
                    service_token_amount,
                },
                ctx,
            )
            .await
        }

        AgentCommand::CreateSubscribe {
            service_id,
            use_trial,
            service_params,
            service_token_amount,
            service_token_address,
            auto_renew,
            title,
            description,
            provider_agent_id,
            service_description,
            service_interval,
            autotrade_mode,
            autotrade_amount,
            autotrade_cap,
            autotrade_quote,
            format,
            exclude_device,
        } => {
            task::user::run_task(
                T::CreateSubscribe {
                    service_id,
                    use_trial,
                    service_params,
                    service_token_amount,
                    service_token_address,
                    auto_renew,
                    title,
                    description,
                    provider_agent_id,
                    service_description,
                    service_interval,
                    autotrade_mode,
                    autotrade_amount,
                    autotrade_cap,
                    autotrade_quote,
                    format,
                    exclude_device,
                },
                ctx,
            )
            .await
        }

        AgentCommand::SubscribeCancel { sub_id } => {
            task::user::run_task(T::SubscribeCancel { sub_id }, ctx).await
        }
        AgentCommand::StartAutorenew { sub_id } => {
            task::user::run_task(T::StartAutorenew { sub_id }, ctx).await
        }
        AgentCommand::SubscribeReject { sub_id, reason } => {
            task::user::run_task(T::SubscribeReject { sub_id, reason }, ctx).await
        }
        AgentCommand::SubscribeDetail { sub_id, format } => {
            task::user::run_task(T::SubscribeDetail { sub_id, format }, ctx).await
        }
        AgentCommand::SubscribeCost {} => task::user::run_task(T::SubscribeCost {}, ctx).await,

        AgentCommand::SubscribeDeviceUpdate {
            job_id,
            device_list,
            items,
        } => {
            task::user::run_task(
                T::SubscribeDeviceUpdate {
                    job_id,
                    device_list,
                    items,
                },
                ctx,
            )
            .await
        }
        AgentCommand::SubscribeOfflineUpdate { job_id, flag } => {
            task::user::run_task(T::SubscribeOfflineUpdate { job_id, flag }, ctx).await
        }
        AgentCommand::DeviceList { page, page_size } => {
            task::user::run_task(T::DeviceList { page, page_size }, ctx).await
        }

        AgentCommand::AspMatch {
            job_id,
            provider_agent_id,
            payment_token_amount,
            page,
            agent_id,
            format,
        } => {
            task::user::run_task(
                T::AspMatch {
                    job_id,
                    provider_agent_id,
                    payment_token_amount,
                    page,
                    agent_id,
                    format,
                },
                ctx,
            )
            .await
        }

        AgentCommand::SetAsp {
            job_id,
            provider_agent_id,
            service_id,
            service_type,
            service_params,
            service_token_address,
            service_token_amount,
            payment_token_symbol,
            payment_token_amount,
            payment_most_token_amount,
            agent_id,
        } => {
            task::user::run_task(
                T::SetAsp {
                    job_id,
                    provider_agent_id,
                    service_id,
                    service_type,
                    service_params,
                    service_token_address,
                    service_token_amount,
                    payment_token_symbol,
                    payment_token_amount,
                    payment_most_token_amount,
                    agent_id,
                },
                ctx,
            )
            .await
        }

        AgentCommand::ResetAsp { job_id, agent_id } => {
            task::user::run_task(T::ResetAsp { job_id, agent_id }, ctx).await
        }

        AgentCommand::UserReject { job_id, agent_id } => {
            task::user::run_task(T::UserReject { job_id, agent_id }, ctx).await
        }

        AgentCommand::MarkFailed {
            job_id,
            provider_agent_id,
        } => {
            task::user::run_task(
                T::MarkFailed {
                    job_id,
                    provider_agent_id,
                },
                ctx,
            )
            .await
        }

        AgentCommand::Status { job_id, agent_id } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::query::handle_status(
                &mut client,
                &job_id,
                agent_id.as_deref().unwrap_or(""),
                task::common::AGENT_ROLE_USER,
            )
            .await
        }

        AgentCommand::Tasks {
            status,
            page,
            limit,
            agent_id,
        } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::query::handle_list(
                &mut client,
                status.as_deref(),
                page,
                limit,
                agent_id.as_deref().unwrap_or(""),
                task::common::AGENT_ROLE_USER,
            )
            .await
        }

        AgentCommand::MySubscriptions { role, status } => {
            task::user::run_task(T::MySubscriptions { role, status }, ctx).await
        }

        AgentCommand::ActiveTasks {
            role,
            include_terminal,
        } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::query::handle_active_tasks(&mut client, role.as_deref(), include_terminal)
                .await
        }

        AgentCommand::SetPaymentMode {
            job_id,
            payment_mode,
            token_symbol,
            token_amount,
            endpoint,
        } => {
            task::user::run_task(
                T::SetPaymentMode {
                    job_id,
                    payment_mode,
                    token_symbol,
                    token_amount,
                    endpoint,
                },
                ctx,
            )
            .await
        }

        AgentCommand::ConfirmAccept { job_id } => {
            task::user::run_task(T::ConfirmAccept { job_id }, ctx).await
        }

        AgentCommand::Task402Pay {
            job_id,
            provider_agent_id,
            accepts,
            endpoint,
            token_symbol,
            token_amount,
            from,
            body,
            force,
        } => {
            task::user::run_task(
                T::Task402Pay {
                    job_id,
                    provider_agent_id,
                    accepts,
                    endpoint,
                    token_symbol,
                    token_amount,
                    from,
                    body,
                    force,
                },
                ctx,
            )
            .await
        }

        AgentCommand::X402Check {
            endpoint,
            agent_id,
            body,
        } => {
            task::user::run_task(
                T::X402Check {
                    endpoint,
                    agent_id,
                    body,
                },
                ctx,
            )
            .await
        }

        AgentCommand::DesignatedRoute { provider, endpoint } => {
            task::common::handle_designated_route(&provider, endpoint.as_deref()).await
        }

        AgentCommand::X402Validate {
            endpoint,
            agent_id,
            job_id,
            fee_amount,
            fee_token,
        } => {
            task::common::handle_x402_validate(
                &endpoint,
                &agent_id,
                &job_id,
                &fee_amount,
                &fee_token,
            )
            .await
        }

        AgentCommand::Complete { job_id } => {
            task::user::run_task(T::Complete { job_id }, ctx).await
        }

        AgentCommand::Reject { job_id, reason } => {
            task::user::run_task(T::Reject { job_id, reason }, ctx).await
        }

        AgentCommand::Close { job_id, agent_id } => {
            task::user::run_task(T::Close { job_id, agent_id }, ctx).await
        }

        AgentCommand::Payment { job_id, agent_id } => {
            task::user::run_task(T::Payment { job_id, agent_id }, ctx).await
        }

        AgentCommand::ClaimAutoRefund { job_id } => {
            task::user::run_task(T::ClaimAutoRefund { job_id }, ctx).await
        }

        AgentCommand::RejectApply { job_id, agent_id } => {
            task::user::run_task(T::RejectApply { job_id, agent_id }, ctx).await
        }

        AgentCommand::UserNotify {
            content,
            image_path,
        } => task::common::okx_a2a::user_notify(&content, image_path.as_deref(), true),

        AgentCommand::FundingNotice(args) => task::common::funding_notice::execute(args),

        AgentCommand::CacheNotify {
            job_id,
            event_key,
            content,
        } => {
            task::common::prefilled_notify::save(&job_id, &event_key, &content)?;
            println!("OK");
            Ok(())
        }

        AgentCommand::CacheRating {
            job_id,
            score,
            comment,
        } => {
            task::common::prefilled_rating::save(&job_id, &score, &comment)?;
            println!("OK");
            Ok(())
        }

        AgentCommand::TaskAttach { job_id, file_paths } => {
            task::user::run_task(T::TaskAttach { job_id, file_paths }, ctx).await
        }
        AgentCommand::ListAttachments { job_id } => {
            task::user::run_task(T::ListAttachments { job_id }, ctx).await
        }

        AgentCommand::TaskDeliverableSave {
            job_id,
            role,
            file,
            deliverable_type,
            title,
            short_id,
            file_key,
            token_symbol,
            token_amount,
            counterparty_agent_id,
            counterparty_name,
        } => {
            let params = task::common::deliverables::SaveParams {
                job_id: &job_id,
                role: &role,
                file_path: &file,
                deliverable_type: &deliverable_type,
                title: &title,
                short_id: &short_id,
                file_key: file_key.as_deref(),
                token_symbol: token_symbol.as_deref(),
                token_amount: token_amount.as_deref(),
                counterparty_agent_id: counterparty_agent_id.as_deref(),
                counterparty_name: counterparty_name.as_deref(),
            };
            let result = task::common::deliverables::handle_save(&params)?;
            crate::output::success(result);
            Ok(())
        }

        AgentCommand::TaskDeliverableList {
            job_id,
            role,
            search,
        } => match job_id {
            Some(jid) => task::common::deliverables::handle_list(&jid, &role),
            None => task::common::deliverables::handle_list_all(&role, search.as_deref()),
        },

        AgentCommand::ClaimAutoComplete { job_id, agent_id } => {
            task::asp::run_provider(
                task::asp::ProviderCommand::ClaimAutoComplete { job_id, agent_id },
                ctx,
            )
            .await
        }

        AgentCommand::AspClaimable { agent_id } => {
            task::asp::run_provider(task::asp::ProviderCommand::Claimable { agent_id }, ctx).await
        }

        AgentCommand::AspClaimRewards { agent_id } => {
            task::asp::run_provider(task::asp::ProviderCommand::ClaimRewards { agent_id }, ctx)
                .await
        }

        AgentCommand::MyAgents { role } => task::common::handle_my_agents(role.as_deref()).await,

        AgentCommand::GateCheck { role } => task::common::handle_preflight(&role).await,

        AgentCommand::PrepareCreate {
            description,
            title,
            budget,
            max_budget,
            currency,
            provider,
        } => {
            task::common::handle_prepare_create(
                description.as_deref(),
                title.as_deref(),
                budget,
                max_budget,
                currency.as_deref(),
                provider.as_deref(),
            )
            .await
        }

        AgentCommand::Profile { agent_id } => task::common::handle_profile(&agent_id).await,

        AgentCommand::Apply {
            job_id,
            token_amount,
            token_symbol,
            agent_id,
        } => {
            task::asp::run_provider(
                task::asp::ProviderCommand::Apply {
                    job_id,
                    token_amount,
                    token_symbol,
                    agent_id,
                },
                ctx,
            )
            .await
        }

        AgentCommand::Deliver {
            job_id,
            file,
            message,
            deliverable_text,
            agent_id,
            autotrade,
        } => {
            task::asp::run_provider(
                task::asp::ProviderCommand::Deliver {
                    job_id,
                    file,
                    message,
                    deliverable_text,
                    agent_id,
                    autotrade,
                },
                ctx,
            )
            .await
        }

        // ── Auto copy-trade (grant-check public; grant-write debug-only) ─────
        AgentCommand::AutotradeGrantCheck {
            job_id,
            venue,
            action,
            amount,
            format,
        } => {
            use task::common::autotrade::{grants, CliBespokeExit};
            // --format must be exactly "json"; any other value denies.
            if format != "json" {
                crate::output::bespoke_deny(grants::DENY_INVALID_FORMAT);
                return Err(CliBespokeExit(1).into());
            }
            // Bespoke process contract: top-level {ok} / {ok,reason}, no `data` wrapper.
            // audit::log (main.rs) fires for both allow and deny before the exit.
            match grants::check_grant(&job_id, &venue, &action, &amount) {
                Ok(()) => {
                    crate::output::bespoke_ok();
                    Ok(())
                }
                Err(deny) => {
                    // NFR-3: `reason` is a process-level description only — never the file/path.
                    crate::output::bespoke_deny(deny.0);
                    Err(CliBespokeExit(1).into())
                }
            }
        }

        #[cfg(debug_assertions)]
        AgentCommand::AutotradeGrantWrite {
            job_id,
            venue,
            max_buy,
            max_sell,
            ttl_sec,
        } => {
            task::common::autotrade::grants::write_grant(
                &job_id,
                &venue,
                max_buy.as_deref(),
                max_sell.as_deref(),
                ttl_sec,
            )?;
            crate::output::success_empty();
            Ok(())
        }

        AgentCommand::SubscriptionRouteSet {
            job_id,
            asset_class,
            skill_id,
            plugin_id,
            protocol,
            requirements,
            delivery_id,
        } => {
            let asset_class = asset_class
                .parse::<crate::asset_class::AssetClass>()
                .map_err(anyhow::Error::msg)?;
            let route = task::common::autotrade::profile::write_model_route(
                &job_id,
                asset_class,
                &skill_id,
                plugin_id.as_deref(),
                protocol.as_deref(),
                &requirements,
                &delivery_id,
            )?;
            crate::output::success(route);
            Ok(())
        }

        AgentCommand::SubscriptionRouteClear { job_id } => {
            task::common::autotrade::profile::clear_model_routes(&job_id)?;
            crate::output::success_empty();
            Ok(())
        }

        AgentCommand::AutotradeConsentRequest {
            job_id,
            agent_id,
            delivery_id,
            signal_type,
        } => {
            use task::common::autotrade::{card, consent, delivery_queue};
            let signal_type = signal_type
                .parse::<crate::asset_class::AssetClass>()
                .map_err(anyhow::Error::msg)?;
            if consent::load_consent(&job_id)?
                .is_some_and(|policy| policy.mode == consent::ConsentMode::Auto)
            {
                crate::output::success(serde_json::json!({
                    "decision": false,
                    "decisionPushed": false,
                    "status": "policy_ready",
                    "reason": "auto_authorization_already_persisted",
                    "deliveryId": delivery_id,
                    "terminal": false,
                    "guidance": "Do not ask A/B/C again. Re-read this delivery, run the normal grant/readiness checks, and execute only through autotrade-execute if eligible.",
                }));
                return Ok(());
            }
            // Bind the user decision to a CLI-admitted delivery before pushing
            // the card. The reply may arrive in a fresh model session, so relying
            // on conversational memory alone is unsafe and loses savedPath.
            let delivery_context = match delivery_queue::enqueue(&job_id, &delivery_id)
                .map_err(|e| anyhow::anyhow!("delivery decision queue unavailable: {e}"))?
            {
                delivery_queue::EnqueueResult::Active {
                    context,
                    already_present: false,
                } => context,
                delivery_queue::EnqueueResult::Active {
                    already_present: true,
                    ..
                } => {
                    crate::output::success(serde_json::json!({
                        "decision": false,
                        "decisionPushed": false,
                        "status": "decision_pending",
                        "reason": "delivery_already_processing_or_awaiting_decision",
                        "deliveryId": delivery_id,
                        "terminal": false,
                        "guidance": "Do not push another A/B/C card and do not submit an order. The durable delivery queue already owns this delivery.",
                    }));
                    return Ok(());
                }
                delivery_queue::EnqueueResult::Queued {
                    active_delivery_id,
                    position,
                } => {
                    crate::output::success(serde_json::json!({
                        "decision": false,
                        "decisionPushed": false,
                        "status": "queued",
                        "reason": "awaiting_prior_user_decision",
                        "deliveryId": delivery_id,
                        "activeDeliveryId": active_delivery_id,
                        "queuePosition": position,
                        "terminal": false,
                    }));
                    return Ok(());
                }
            };
            // The queue serializes deliveries; the pending pointer binds the
            // visible card/reply to the active one only.
            match consent::activate_delivery_context_exclusive(&job_id, &delivery_id)
                .map_err(|e| anyhow::anyhow!("delivery context unavailable: {e}"))?
            {
                consent::DeliveryActivation::Activated(_)
                | consent::DeliveryActivation::AlreadyPending(_) => {}
                consent::DeliveryActivation::Conflict(pending) => {
                    anyhow::bail!(
                        "delivery queue/pending pointer mismatch: active {}",
                        pending.delivery_id
                    );
                }
            }
            let decision = match consent::load_consent(&job_id)? {
                Some(policy) if policy.mode == consent::ConsentMode::Manual => {
                    card::make_manual_signal_decision(
                        &delivery_id,
                        signal_type.as_str(),
                        &job_id,
                        &agent_id,
                        policy.trade_amount_u.as_deref(),
                    )
                }
                _ => card::make_first_time_decision(
                    &delivery_id,
                    signal_type.as_str(),
                    &job_id,
                    &agent_id,
                ),
            };
            delivery_queue::mark_awaiting_decision(&job_id, &delivery_id)
                .map_err(|e| anyhow::anyhow!("delivery decision queue update failed: {e}"))?;
            match task::common::pending_v2::push_decision_direct(
                &job_id,
                "user",
                &agent_id,
                Some(&delivery_context.provider_agent_id),
                &decision.user_content,
                &card::decision_list_label(&decision),
                &decision.source_event,
            ) {
                Ok(()) => crate::output::success(serde_json::json!({
                    "decision": true,
                    "decisionPushed": true,
                    "sourceEvent": decision.source_event,
                    "deliveryId": delivery_id,
                })),
                Err(_) => crate::output::success(decision),
            }
            Ok(())
        }

        AgentCommand::AutotradeExecute {
            job_id,
            delivery_id,
            venue,
            action,
            amount,
            execution_mode,
            command_json,
            timeout_sec,
        } => {
            let outcome = task::common::autotrade::executor::execute(
                task::common::autotrade::executor::ExecuteRequest {
                    job_id: &job_id,
                    delivery_id: &delivery_id,
                    venue: &venue,
                    action: &action,
                    amount: &amount,
                    execution_mode:
                        task::common::autotrade::executor::ExecutionMode::parse(&execution_mode)?,
                    command_json: &command_json,
                    timeout_sec,
                },
            )
            .await?;
            crate::output::success(outcome);
            Ok(())
        }

        AgentCommand::AutotradeOnceAuthorize {
            job_id,
            delivery_id,
            amount,
        } => {
            crate::output::success(
                task::common::autotrade::executor::authorize_one_time(
                    &job_id,
                    &delivery_id,
                    &amount,
                )?,
            );
            Ok(())
        }

        AgentCommand::AutotradeOutcomeFlush { job_id } => {
            crate::output::success(task::common::autotrade::executor::flush(&job_id)?);
            Ok(())
        }

        AgentCommand::AutotradeDeliveryReport {
            job_id,
            delivery_id,
            status,
            reason,
        } => {
            crate::output::success(task::common::autotrade::executor::report_delivery(
                &job_id,
                &delivery_id,
                &status,
                &reason,
            )?);
            Ok(())
        }

        AgentCommand::AutotradeCapAdjustRequest { job_id, agent_id } => {
            use task::common::autotrade::{card, consent};
            let file = consent::load_consent(&job_id)?
                .ok_or_else(|| anyhow::anyhow!("no live auto-trade consent"))?;
            if file.mode != consent::ConsentMode::Auto {
                anyhow::bail!("cap adjustment is only valid for auto consent");
            }
            let amount = file.trade_amount_u.as_deref().unwrap_or_default();
            let cap = file.cap_u.as_deref().unwrap_or_default();
            let amount_decimal = task::common::autotrade::amount::Decimal::parse(amount)?;
            if matches!(
                consent::evaluate_consent(&job_id, Some(&amount_decimal)),
                Ok(consent::ConsentDecision::AutoOverCap)
            ) {
                let d = card::make_cap_adjust_decision("trade", &job_id, &agent_id, amount, cap);
                let target = consent::load_pending_delivery_context(&job_id)?
                    .map(|context| context.provider_agent_id);
                match task::common::pending_v2::push_decision_direct(
                    &job_id,
                    "user",
                    &agent_id,
                    target.as_deref(),
                    &d.user_content,
                    &card::decision_list_label(&d),
                    &d.source_event,
                ) {
                    Ok(()) => crate::output::success(serde_json::json!({
                        "decision": true, "decisionPushed": true,
                        "sourceEvent": d.source_event
                    })),
                    Err(_) => crate::output::success(d),
                }
            } else {
                crate::output::success(serde_json::json!({"capAlreadySufficient": true}));
            }
            Ok(())
        }

        AgentCommand::AutotradeConsentSet {
            job_id,
            mode,
            cap,
            trade_amount,
            agent_id,
            ttl_sec,
            plugin,
            tool,
            quote,
        } => {
            use task::common::autotrade::{consent, grants};
            if tool.is_some() {
                anyhow::bail!("--tool is deprecated; use subscription-route-set");
            }
            if mode == "pause" {
                if cap.is_some() || trade_amount.is_some() || plugin.is_some() || quote.is_some() {
                    anyhow::bail!("pause does not accept policy arguments");
                }
                consent::clear_consent(&job_id);
                grants::clear_grant(&job_id);
                consent::clear_pending_signal(&job_id);
                crate::output::success(
                    serde_json::json!({"consentMode":"pause","cleared":true,"jobId":job_id}),
                );
                return Ok(());
            }
            if agent_id.is_none() {
                anyhow::bail!("--agent-id is required unless --mode pause");
            }
            if mode == "plugin-ready-check" || mode == "plugin-approved" {
                if cap.is_some() || trade_amount.is_some() || quote.is_some() {
                    anyhow::bail!("plugin-ready-check accepts only --plugin");
                }
                let plugin = plugin.ok_or_else(|| anyhow::anyhow!("--plugin is required"))?;
                let selected = match plugin.as_str() {
                    "trade-kit" => task::common::autotrade::tooling::ExecutionTool::TradeKit,
                    "polymarket-plugin" => {
                        task::common::autotrade::tooling::ExecutionTool::PolymarketPlugin
                    }
                    "hyperliquid-plugin" => {
                        task::common::autotrade::tooling::ExecutionTool::HyperliquidPlugin
                    }
                    _ => anyhow::bail!("unsupported execution plugin/tool"),
                };
                let inventory = task::common::autotrade::tooling::ToolInventory::detect();
                if inventory.readiness_of(selected)
                    != task::common::autotrade::tooling::Readiness::Ready
                {
                    anyhow::bail!("plugin/tool is not ready");
                }
                consent::write_plugin_approved(&job_id, &plugin)
                    .map_err(|e| anyhow::anyhow!(e.0))?;
                crate::output::success(
                    serde_json::json!({"pluginApproved":plugin,"replayed":false}),
                );
                return Ok(());
            }
            if mode == "cap-adjust" {
                if trade_amount.is_some() || plugin.is_some() || quote.is_some() {
                    anyhow::bail!("cap-adjust accepts only --cap");
                }
                let new_cap = cap
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--cap is required"))?;
                let existing = consent::load_consent(&job_id)?
                    .ok_or_else(|| anyhow::anyhow!("no live consent"))?;
                if existing.mode != consent::ConsentMode::Auto {
                    anyhow::bail!("cap adjustment requires auto consent");
                }
                let remaining = existing
                    .expires_at
                    .saturating_sub(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    )
                    .max(1);
                consent::write_consent_with_trade_amount(
                    &job_id,
                    consent::ConsentMode::Auto,
                    Some(new_cap),
                    existing.trade_amount_u.as_deref(),
                    existing.quote_token.as_deref(),
                    remaining,
                )?;
                grants::write_cap_grant(&job_id, new_cap, remaining)?;
                crate::output::success(serde_json::json!({"capAdjusted":true,"cap":new_cap}));
                return Ok(());
            }
            if plugin.is_some() {
                anyhow::bail!("--plugin is only valid with plugin-ready-check");
            }
            let mode_enum = match mode.as_str() {
                "auto" => consent::ConsentMode::Auto,
                "manual" => consent::ConsentMode::Manual,
                "decline" => consent::ConsentMode::Decline,
                _ => anyhow::bail!("unsupported --mode"),
            };
            let effective_amount = trade_amount.as_deref().or_else(|| {
                if mode_enum == consent::ConsentMode::Auto {
                    cap.as_deref()
                } else {
                    None
                }
            });
            consent::write_consent_with_trade_amount(
                &job_id,
                mode_enum,
                cap.as_deref(),
                effective_amount,
                quote.as_deref(),
                ttl_sec,
            )?;
            match mode_enum {
                consent::ConsentMode::Auto => {
                    grants::write_cap_grant(&job_id, cap.as_deref().unwrap_or_default(), ttl_sec)?
                }
                consent::ConsentMode::Manual | consent::ConsentMode::Decline => {
                    grants::clear_grant(&job_id)
                }
            }
            consent::clear_pending_signal(&job_id);
            crate::output::success(
                serde_json::json!({"consentMode":mode,"cap":cap,"replayed":false}),
            );
            Ok(())
        }
        AgentCommand::AgreeRefund { job_id, agent_id } => {
            task::asp::run_provider(
                task::asp::ProviderCommand::AgreeRefund { job_id, agent_id },
                ctx,
            )
            .await
        }

        AgentCommand::AspReject {
            job_id,
            agent_id,
            reason,
        } => {
            task::asp::run_provider(
                task::asp::ProviderCommand::AspReject {
                    job_id,
                    agent_id,
                    reason,
                },
                ctx,
            )
            .await
        }

        AgentCommand::SubscribeActive { agent_id } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::asp::subscription::handle_active(&mut client, &agent_id).await
        }
        AgentCommand::SubscribeAgreeRefund { job_id, agent_id } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::asp::subscription::handle_agree_refund(&mut client, &job_id, &agent_id).await
        }
        AgentCommand::SubscribeAspClaim { job_id, agent_id } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::asp::subscription::handle_asp_claim(&mut client, &job_id, &agent_id).await
        }
        AgentCommand::SubscribeDispute {
            job_id,
            reason,
            agent_id,
        } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::asp::subscription::handle_dispute(
                &mut client,
                &job_id,
                reason.as_deref().unwrap_or(""),
                &agent_id,
            )
            .await
        }

        // ── Sub-groups ──────────────────────────────────────────────
        AgentCommand::Dispute(c) => task::asp::run_dispute(c, ctx).await,

        AgentCommand::PendingDecisionsV2(c) => task::common::pending_v2::run(c).await,

        AgentCommand::SessionCleanup { job_id } => {
            task::common::session_cleanup::handle_session_cleanup(&job_id, true)
        }

        // ── Evaluator Agent flat dispatch ───────────────────────────
        AgentCommand::EvidenceInfo {
            job_id,
            agent_id,
            round_num,
        } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::info::handle_info(&mut c, &job_id, &agent_id, &round_num).await
        }
        AgentCommand::VoteCommit {
            job_id,
            vote,
            reason,
            reason_summary,
            agent_id,
        } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::commit::handle_commit(
                &mut c,
                &job_id,
                vote,
                &reason,
                &reason_summary,
                &agent_id,
            )
            .await
        }
        AgentCommand::VoteReveal { job_id, agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::reveal::handle_reveal(&mut c, &job_id, &agent_id).await
        }
        AgentCommand::ArbitrationClaim { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::claim::handle_claim(&mut c, &agent_id).await
        }
        AgentCommand::ArbitrationClaimable { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::claimable::handle_claimable(&mut c, &agent_id).await
        }
        AgentCommand::Stake { amount, agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::stake::handle_stake(&mut c, &amount, &agent_id).await
        }
        AgentCommand::IncreaseStake { amount, agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::stake::handle_increase_stake(&mut c, &amount, &agent_id).await
        }
        AgentCommand::RequestUnstake { amount, agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::unstake::handle_request_unstake(&mut c, &amount, &agent_id).await
        }
        AgentCommand::ClaimUnstake { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::unstake::handle_claim_unstake(&mut c, &agent_id).await
        }
        AgentCommand::CancelUnstake { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::unstake::handle_cancel_unstake(&mut c, &agent_id).await
        }
        AgentCommand::StakingConfig { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::staking_config::handle_staking_config(&mut c, &agent_id).await
        }
        AgentCommand::MyStake { agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::my_stake::handle_my_stake(&mut c, &agent_id).await
        }

        AgentCommand::Common(c) => task::common::run(c, ctx).await,

        AgentCommand::NextAction {
            agent_id,
            role,
            message,
            a2a_file,
        } => {
            // Parse the `--message` envelope (required). Try strict parse first;
            // on failure, attempt a one-shot repair that escapes raw control chars
            // (LF / CR / TAB) inside string scope and retries. This covers the
            // most common LLM mistake: emitting a literal newline inside a JSON
            // string value (e.g. for a long deliverable `text`) instead of the
            // two-char escape `\n`. Outside strings, JSON allows whitespace so
            // raw control chars survive untouched.
            let mut parsed_message: serde_json::Value = match serde_json::from_str(&message) {
                Ok(v) => v,
                Err(strict_err) => {
                    let repaired = escape_control_chars_in_strings(&message);
                    match serde_json::from_str::<serde_json::Value>(&repaired) {
                        Ok(v) => {
                            eprintln!(
                                "[next-action] --message had raw control chars inside string values; auto-repaired and parsed. \
                                 Strict parse error was: {strict_err}"
                            );
                            v
                        }
                        Err(_) => {
                            return Err(anyhow::anyhow!(
                                "--message must be a valid JSON object: {strict_err}"
                            ))
                        }
                    }
                }
            };
            if DEBUG_LOG {
                eprintln!(
                    "[next-action] --message parsed: {} keys",
                    parsed_message.as_object().map(|o| o.len()).unwrap_or(0)
                );
            }

            if let Some(path) = a2a_file.as_deref() {
                let message_job_id = parsed_message
                    .get("jobId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let validated_path = validate_a2a_file_arg(path, message_job_id, &agent_id)?;
                parsed_message["a2aFile"] = serde_json::Value::String(validated_path);
            }

            // Field extractors — all routing inputs live inside `--message`.
            let msg_str = |key: &str| -> Option<String> {
                parsed_message
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(String::from)
            };
            let msg_i64 =
                |key: &str| -> Option<i64> { parsed_message.get(key).and_then(|v| v.as_i64()) };

            let event: String =
                msg_str("event").ok_or_else(|| anyhow::anyhow!("--message.event is required"))?;
            let job_id: String = match msg_str("jobId") {
                Some(j) => j,
                None if event == "reward_claimed" || event == "create_task" => String::new(),
                None => anyhow::bail!("--message.jobId is required"),
            };
            let code: i32 = msg_i64("code")
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0);
            let job_title: Option<String> = msg_str("jobTitle");
            let provider: Option<String> = msg_str("provider");
            let data: Option<String> = msg_str("data");
            let peer_task_min_version: Option<u32> = parsed_message
                .get("taskMinVersion")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .or_else(|| {
                    parsed_message
                        .get("payload")
                        .and_then(|p| p.get("taskMinVersion"))
                        .and_then(|v| v.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                });
            let parsed_message = Some(parsed_message);
            if !job_id.is_empty() {
                if let Err(msg) = task::common::util::validate_job_id(&job_id) {
                    anyhow::bail!(msg);
                }
            }
            if DEBUG_LOG {
                eprintln!(
                    "[next-action] received system notification: job_id={job_id}, event={event}, role={role}, agent_id={agent_id}, code={code}, title={title}, provider={provider}, peer_task_min_version={peer_min}",
                    title = job_title.as_deref().unwrap_or("(none)"),
                    provider = provider.as_deref().unwrap_or("(none)"),
                    peer_min = peer_task_min_version.map(|v| v.to_string()).unwrap_or_else(|| "(none)".to_string()),
                );
            }

            // Version handshake: when peer's required minimum version is higher than the local TASK_MIN_VERSION,
            // only prepend a notice line at the top of the script (so the agent can push an upgrade suggestion to the user);
            // **do not block the flow** — the role flow continues to execute under the current protocol, and any actual
            // compatibility issues (if any) are escalated when subsequent CLI / business layer errors surface.
            let version_notice: Option<String> = if let Some(peer_min) = peer_task_min_version {
                let local = task::common::config::TASK_MIN_VERSION;
                if local < peer_min {
                    if task::common::version_notice::should_show() {
                        if DEBUG_LOG {
                            eprintln!(
                                "[next-action] version notice (non-blocking): local TASK_MIN_VERSION={local} < peer.taskMinVersion={peer_min}"
                            );
                        }
                        task::common::version_notice::mark_shown();
                        Some("Before executing the scene below, run `onchainos agent user-notify` to notify the user (recommend upgrade but do **not** halt the flow). The `--content` template below is a sample — translate it to the user's language before sending:\n\
                             ```bash\n\
                             onchainos agent user-notify --content 'Your local task-system protocol version is outdated. Please run `onchainos upgrade` to upgrade for the best compatibility with peers.'\n\
                             ```\n\
                             Then proceed to the scene below normally.\n\n".to_string())
                    } else {
                        if DEBUG_LOG {
                            eprintln!(
                                "[next-action] version notice suppressed (last shown within 48h): local TASK_MIN_VERSION={local} < peer.taskMinVersion={peer_min}"
                            );
                        }
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // When --provider is passed, write the designated-provider file so generate_next_action takes the specified-provider path
            if let Some(ref pid) = provider {
                if let Err(e) = task::user::negotiate::save_designated_provider(&job_id, pid) {
                    if DEBUG_LOG {
                        eprintln!("[next-action] save_designated_provider failed: {e}");
                    }
                }
            }

            // code ≠ 0 → tx failed; output the failure script directly and skip the event match
            if code != 0 {
                let label = tx_failure_label(&event);
                let title_part = match job_title.as_deref() {
                    Some(t) => format!(" **{t}**"),
                    None => " ".to_string(),
                };
                println!(
                    "【交易失败】{label}（code={code}）\n\n\
                     运行 `onchainos agent user-notify` 通知用户：\n\
                     ```bash\n\
                     onchainos agent user-notify --content '[{label}]{title_part}（{job_id}）交易执行失败（code={code}）。'\n\
                     ```\n\
                     → 结束 turn。"
                );
                return Ok(());
            }

            // Resolve `--role auto` by direct-lookup against the agent registry, so
            // the caller doesn't have to run `agent profile <id>` as a separate LLM
            // turn. Uses `query_agent_by_id_direct` (backend-direct lookup, no
            // pagination) — the same helper that powers `handle_profile`.
            let resolved_role: String = if role == "auto" {
                match task::common::query_agent_by_id_direct(&agent_id).await {
                    Ok(agent) => match agent["role"].as_i64() {
                        Some(1) => "user".to_string(),
                        Some(2) => "asp".to_string(),
                        Some(3) => "evaluator".to_string(),
                        other => anyhow::bail!(
                            "agentId={agent_id} has unsupported role={:?}; pass --role explicitly",
                            other
                        ),
                    },
                    Err(e) => anyhow::bail!(
                        "could not resolve role for agentId={agent_id}: {e}; pass --role explicitly"
                    ),
                }
            } else {
                role.clone()
            };

            if DEBUG_LOG {
                eprintln!("[next-action] resolved role: {role} -> {resolved_role}");
            }

            // ── job_created API fallback: when --provider is absent and no local file exists,
            // query the task detail API for providerAgentId and persist it.
            // Must run AFTER role resolution so --role auto is correctly resolved.
            if provider.is_none()
                && resolved_role == "user"
                && event == "job_created"
                && !task::user::negotiate::has_designated_provider(&job_id)
            {
                let mut fb_client = task::common::network::task_api_client::TaskApiClient::new();
                if let Ok(resp) = fb_client
                    .get_with_identity(&fb_client.task_path(&job_id), &agent_id)
                    .await
                {
                    if let Some(pid) = resp["providerAgentId"].as_str().filter(|s| !s.is_empty()) {
                        if DEBUG_LOG {
                            eprintln!("[next-action] job_created fallback: API providerAgentId={pid}, persisting");
                        }
                        if let Err(e) =
                            task::user::negotiate::save_designated_provider(&job_id, pid)
                        {
                            if DEBUG_LOG {
                                eprintln!(
                                    "[next-action] save_designated_provider (fallback) failed: {e}"
                                );
                            }
                        }
                    }
                }
            }

            // ── review gate: auto-mark user's review gate ──────────────────────
            // Must run AFTER role resolution so --role auto is correctly resolved.
            if resolved_role == "user" {
                if event == "job_submitted" {
                    if let Err(e) = task::common::review_gate::mark_pending(&job_id) {
                        if DEBUG_LOG {
                            eprintln!("[next-action] review_gate mark_pending failed: {e}");
                        }
                    }
                } else if event == "approve_review" {
                    if let Err(e) = task::common::review_gate::mark_approved(&job_id) {
                        if DEBUG_LOG {
                            eprintln!("[next-action] review_gate mark_approved failed: {e}");
                        }
                    }
                }
            }

            // Status mismatch → block script output (to prevent sub from running an old script on-chain based on a stale event).
            // Only skip validation for PSEUDO_EVENTS / unknown / network failure; under normal conditions enforce strictly.
            let (freshness_warning, prefetched) =
                check_status_freshness(&job_id, &event, &agent_id).await;
            if let Some(w) = freshness_warning {
                println!("{w}");
                return Ok(());
            }
            let payment_mode = prefetched.as_ref().and_then(|p| p.payment_mode);
            let title_ref = job_title.as_deref();
            let prompt = match resolved_role.as_str() {
                "asp" => {
                    crate::audit::log(
                        "cli",
                        "provider/next_action_received",
                        true,
                        std::time::Duration::default(),
                        Some(vec![
                            format!("jobId={job_id}"),
                            format!("agentId={agent_id}"),
                            format!("event={event}"),
                            format!("code={code}"),
                            format!("paymentMode={:?}", payment_mode),
                        ]),
                        None,
                    );
                    // x402 (paymentMode=3): the user paid the ASP at request time via
                    // the A2MCP service endpoint, so the on-chain events are pure
                    // receipts — provider has no business action for any of them.
                    // Route every x402 event to the observer-only a2mcp playbook.
                    let use_a2mcp = matches!(payment_mode, Some(3));
                    if use_a2mcp {
                        task::asp::flow::generate_a2mcp_next_action(
                            &job_id,
                            &event,
                            &agent_id,
                            title_ref,
                            data.as_deref(),
                            prefetched.as_ref(),
                            parsed_message.as_ref(),
                        )
                        .await
                    } else {
                        task::asp::flow::generate_next_action(
                            &job_id,
                            &event,
                            &agent_id,
                            title_ref,
                            data.as_deref(),
                            prefetched.as_ref(),
                            parsed_message.as_ref(),
                        )
                        .await
                    }
                }
                "user" => {
                    crate::audit::log(
                        "cli",
                        "user/next_action_received",
                        true,
                        std::time::Duration::default(),
                        Some(vec![
                            format!("jobId={job_id}"),
                            format!("agentId={agent_id}"),
                            format!("event={event}"),
                            format!("code={code}"),
                        ]),
                        None,
                    );
                    task::user::flow::generate_next_action(
                        &job_id,
                        &event,
                        &agent_id,
                        title_ref,
                        data.as_deref(),
                        payment_mode,
                        prefetched.as_ref(),
                        parsed_message.as_ref(),
                    )
                    .await
                }
                "evaluator" => {
                    crate::audit::log(
                        "cli",
                        "evaluator/next_action_received",
                        true,
                        std::time::Duration::default(),
                        Some(vec![
                            format!("jobId={job_id}"),
                            format!("agentId={agent_id}"),
                            format!("event={event}"),
                            format!("code={code}"),
                        ]),
                        None,
                    );
                    task::evaluator::flow::generate_next_action(
                        &job_id,
                        &event,
                        &agent_id,
                        parsed_message.as_ref(),
                    )
                    .await
                }
                other => anyhow::bail!("--role 必须是 asp/user/evaluator，当前: {other}"),
            };
            if let Some(notice) = &version_notice {
                print!("{notice}");
            }
            println!("{prompt}");
            Ok(())
        }

        // ── Chat (XMTP attachments + risk/eligibility + system config + heartbeat) ──
        AgentCommand::FileUpload {
            file,
            agent_id,
            job_id,
        } => {
            chat::run(
                chat::ChatCommand::FileUpload {
                    file,
                    agent_id,
                    job_id,
                },
                ctx,
            )
            .await
        }

        AgentCommand::FileDownload {
            file_key,
            agent_id,
            output,
        } => {
            chat::run(
                chat::ChatCommand::FileDownload {
                    file_key,
                    agent_id,
                    output,
                },
                ctx,
            )
            .await
        }

        AgentCommand::SensitiveWords => chat::run(chat::ChatCommand::SensitiveWords, ctx).await,

        AgentCommand::MessageEligible {
            agent_id,
            client_agent_id,
            provider_agent_id,
            job_id,
            group_id,
            direction,
            provider_security_rate,
            client_communication_address,
            provider_communication_address,
            is_offline_replay,
        } => {
            chat::run(
                chat::ChatCommand::MessageEligible {
                    agent_id,
                    client_agent_id,
                    provider_agent_id,
                    job_id,
                    group_id,
                    direction,
                    provider_security_rate,
                    client_communication_address,
                    provider_communication_address,
                    is_offline_replay,
                },
                ctx,
            )
            .await
        }

        AgentCommand::SystemConfig => chat::run(chat::ChatCommand::SystemConfig, ctx).await,

        AgentCommand::Heartbeat { chain_index } => {
            chat::run(chat::ChatCommand::Heartbeat { chain_index }, ctx).await
        }

        AgentCommand::WakeupNotify { agent_ids } => {
            chat::run(chat::ChatCommand::WakeupNotify { agent_ids }, ctx).await
        }

        AgentCommand::TaskInProgress { agent_ids } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::in_progress::handle_in_progress(&mut client, &agent_ids).await
        }
    }
}

fn tx_failure_label(event: &str) -> &'static str {
    task::common::state_machine::Event::parse(event).failure_label()
}

/// Escape raw ASCII control chars (LF / CR / TAB) that appear inside JSON string
/// scope, leaving everything else untouched. Tracks `"`/`\\` state to differentiate
/// "inside string" from "outside string". Used as a one-shot repair for
/// `--message` / `--service` / similar LLM-supplied payloads where the model
/// emitted a literal newline inside a long string value instead of the two-char
/// `\n` escape (the #1 LLM JSON mistake). Outside strings these bytes are
/// already legal JSON whitespace, so leaving them is fine; inside strings they
/// are illegal control chars, so escaping them produces the likely-intended
/// valid JSON.
pub(crate) fn escape_control_chars_in_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut in_string = false;
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            // Inside a string, the previous char was `\`; emit this one verbatim
            // (it forms an escape sequence like `\n` / `\"` / `\\` / `\u…`).
            out.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => {
                escaped = true;
                out.push(ch);
            }
            '"' => {
                in_string = !in_string;
                out.push(ch);
            }
            '\n' if in_string => out.push_str("\\n"),
            '\r' if in_string => out.push_str("\\r"),
            '\t' if in_string => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn is_safe_a2a_file_path(path: &std::path::Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    let c_path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let tmp_dir = std::env::temp_dir();
    if let Ok(c_tmp) = tmp_dir.canonicalize() {
        if c_path.starts_with(c_tmp) {
            return true;
        }
    }
    #[cfg(unix)]
    {
        if let Ok(c_tmp) = std::path::Path::new("/tmp").canonicalize() {
            if c_path.starts_with(c_tmp) {
                return true;
            }
        }
    }
    #[cfg(test)]
    {
        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp");
        if let Ok(c_tmp) = test_tmp.canonicalize() {
            if c_path.starts_with(c_tmp) {
                return true;
            }
        }
    }
    false
}

fn parse_a2a_json_arg(raw: &str) -> anyhow::Result<serde_json::Value> {
    match serde_json::from_str(raw) {
        Ok(v) => Ok(v),
        Err(strict_err) => {
            let repaired = escape_control_chars_in_strings(raw);
            match serde_json::from_str::<serde_json::Value>(&repaired) {
                Ok(v) => {
                    eprintln!(
                        "[next-action] --a2a-file payload had raw control chars inside string values; \
                         auto-repaired. Strict parse error was: {strict_err}"
                    );
                    Ok(v)
                }
                Err(_) => anyhow::bail!("--a2a-file payload is not valid JSON: {strict_err}"),
            }
        }
    }
}

fn write_secure_temp_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("a2a");
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for n in 0..20u32 {
        let tmp = parent.join(format!(".{fname}.{pid}.{ts}.{n}.tmp"));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = match opts.open(&tmp) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        if let Err(e) = f.write_all(contents).and_then(|_| f.flush()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        f.sync_all()?;
        drop(f);
        return std::fs::rename(&tmp, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        });
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique temp file",
    ))
}

fn a2a_intake_spool_dir() -> std::path::PathBuf {
    #[cfg(test)]
    {
        return std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp");
    }
    #[cfg(not(test))]
    {
        std::env::temp_dir()
    }
}

fn persist_validated_a2a_spool(job_id: &str, canonical: &str) -> anyhow::Result<String> {
    static A2A_SPOOL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    if job_id.is_empty()
        || !job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("--a2a-file: invalid jobId for the spool filename");
    }
    let dir = a2a_intake_spool_dir();
    std::fs::create_dir_all(&dir)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let seq = A2A_SPOOL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for n in 0..20u32 {
        let name = if n == 0 {
            format!("a2a_deliver_{job_id}_{ts}_{pid}_{seq}.json")
        } else {
            format!("a2a_deliver_{job_id}_{ts}_{pid}_{seq}_{n}.json")
        };
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        write_secure_temp_file(&path, canonical.as_bytes())
            .map_err(|e| anyhow::anyhow!("--a2a-file secure spool write failed: {e}"))?;
        return Ok(path.to_string_lossy().into_owned());
    }
    anyhow::bail!("--a2a-file: could not allocate a unique spool filename");
}

fn validate_a2a_file_arg(
    path: &str,
    message_job_id: &str,
    agent_id: &str,
) -> anyhow::Result<String> {
    let fp = std::path::Path::new(path);
    if !is_safe_a2a_file_path(fp) {
        anyhow::bail!("--a2a-file must point to a file under the OS temp directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fp)
            .map_err(|e| anyhow::anyhow!("--a2a-file metadata read failed: {e}"))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            anyhow::bail!("--a2a-file must not be readable, writable, or executable by group/others; use chmod 600");
        }
    }
    let raw =
        std::fs::read_to_string(fp).map_err(|e| anyhow::anyhow!("--a2a-file read failed: {e}"))?;
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("--a2a-file payload is empty");
    }
    let payload = parse_a2a_json_arg(raw)?;
    let msg_type = payload
        .get("msgType")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("--a2a-file payload.msgType is required"))?;
    if msg_type != "a2a-agent-chat" {
        anyhow::bail!("--a2a-file payload.msgType must be a2a-agent-chat");
    }
    let pj = payload
        .get("jobId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("--a2a-file payload.jobId is required"))?;
    if pj != message_job_id {
        anyhow::bail!(
            "--a2a-file payload jobId {pj} does not match --message jobId {message_job_id}"
        );
    }
    if let Some(receiver) = payload.get("receiverAgentId").and_then(|v| v.as_str()) {
        if receiver != agent_id {
            anyhow::bail!(
                "--a2a-file receiverAgentId {receiver} does not match --agentId {agent_id}"
            );
        }
    }
    let content = payload
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("--a2a-file payload.content is required"))?;
    if !content.contains("[intent:deliver]") {
        anyhow::bail!("--a2a-file content must contain [intent:deliver]");
    }
    let canonical = serde_json::to_string(&payload)?;
    persist_validated_a2a_spool(pj, &canonical)
}

#[cfg(test)]
mod escape_control_chars_tests {
    use super::{escape_control_chars_in_strings, validate_a2a_file_arg};

    #[test]
    fn escapes_raw_lf_inside_string() {
        let input = "{\"text\":\"line1\nline2\"}";
        let out = escape_control_chars_in_strings(input);
        assert_eq!(out, "{\"text\":\"line1\\nline2\"}");
    }

    #[test]
    fn preserves_lf_outside_string() {
        // Real newline between fields is legal JSON whitespace; leave it alone.
        let input = "{\n\"k\":\"v\"\n}";
        let out = escape_control_chars_in_strings(input);
        assert_eq!(out, "{\n\"k\":\"v\"\n}");
    }

    #[test]
    fn preserves_existing_escape_sequences() {
        let input = "{\"text\":\"a\\nb\\\"c\"}";
        let out = escape_control_chars_in_strings(input);
        // `\n` and `\"` are already escapes; nothing to repair.
        assert_eq!(out, input);
    }

    #[test]
    fn handles_cr_and_tab() {
        let input = "{\"text\":\"a\rb\tc\"}";
        let out = escape_control_chars_in_strings(input);
        assert_eq!(out, "{\"text\":\"a\\rb\\tc\"}");
    }

    #[test]
    fn repaired_json_round_trips_to_serde() {
        let input = "{\"event\":\"deliverable_received\",\"text\":\"line1\nline2\"}";
        let repaired = escape_control_chars_in_strings(input);
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed["text"], "line1\nline2");
    }

    fn write_temp_a2a(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("onchainos-a2a-file-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }

    #[test]
    fn validates_a2a_file_arg_for_deliver_message() {
        let job_id = "0xabc123";
        let raw = r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","receiverAgentId":"1696","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]"}"#;
        let path = write_temp_a2a("valid-a2a.json", raw);

        let got = validate_a2a_file_arg(path.to_str().unwrap(), job_id, "1696").unwrap();
        assert_ne!(got, path.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        let spool_name = std::path::Path::new(&got)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap();
        assert!(spool_name.starts_with("a2a_deliver_0xabc123_"));
        assert!(spool_name.ends_with(".json"));
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&got).unwrap()).unwrap();
        assert_eq!(parsed["jobId"], "0xabc123");
        std::fs::remove_file(got).ok();
    }

    #[test]
    fn rejects_a2a_file_arg_with_mismatched_job_id() {
        let path = write_temp_a2a(
            "wrong-job.json",
            r#"{"msgType":"a2a-agent-chat","jobId":"0xother","receiverAgentId":"1696","content":"jobId: 0xother\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]"}"#,
        );

        let err = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696")
            .expect_err("jobId mismatch must fail")
            .to_string();
        assert!(err.contains("payload jobId 0xother does not match --message jobId 0xabc123"));
    }

    #[test]
    fn rejects_a2a_file_arg_without_top_level_job_id() {
        let path = write_temp_a2a(
            "missing-job.json",
            r#"{"msgType":"a2a-agent-chat","receiverAgentId":"1696","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]"}"#,
        );

        let err = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696")
            .expect_err("missing top-level jobId must fail")
            .to_string();
        assert!(err.contains("payload.jobId is required"));
    }

    #[test]
    fn canonicalizes_repaired_a2a_file_arg_for_downstream_strict_parse() {
        let path = write_temp_a2a(
            "raw-control-char.json",
            "{ \"msgType\":\"a2a-agent-chat\", \"jobId\":\"0xabc123\", \"receiverAgentId\":\"1696\", \"content\":\"jobId: 0xabc123\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]\" }",
        );

        let original = std::fs::read_to_string(&path).unwrap();
        let got = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let rewritten = std::fs::read_to_string(&got).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(
            parsed["content"].as_str().unwrap().lines().next(),
            Some("jobId: 0xabc123")
        );
        std::fs::remove_file(got).ok();
    }

    #[test]
    fn creates_unique_spool_files_for_multiple_deliverables_in_same_job() {
        let first = write_temp_a2a(
            "multi-deliver-1.json",
            r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","receiverAgentId":"1696","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nfirst\n- - -\n[intent:deliver]"}"#,
        );
        let second = write_temp_a2a(
            "multi-deliver-2.json",
            r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","receiverAgentId":"1696","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nsecond\n- - -\n[intent:deliver]"}"#,
        );

        let spool1 = validate_a2a_file_arg(first.to_str().unwrap(), "0xabc123", "1696").unwrap();
        let spool2 = validate_a2a_file_arg(second.to_str().unwrap(), "0xabc123", "1696").unwrap();

        assert_ne!(spool1, spool2);
        assert_eq!(
            std::fs::read_to_string(&first).unwrap().contains("first"),
            true
        );
        assert_eq!(
            std::fs::read_to_string(&second).unwrap().contains("second"),
            true
        );
        assert_eq!(
            std::fs::read_to_string(&spool1).unwrap().contains("first"),
            true
        );
        assert_eq!(
            std::fs::read_to_string(&spool2).unwrap().contains("second"),
            true
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode1 = std::fs::metadata(&spool1).unwrap().permissions().mode() & 0o777;
            let mode2 = std::fs::metadata(&spool2).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode1, 0o600);
            assert_eq!(mode2, 0o600);
        }
        std::fs::remove_file(spool1).ok();
        std::fs::remove_file(spool2).ok();
    }

    #[test]
    fn rejects_a2a_file_arg_without_deliver_intent() {
        let path = write_temp_a2a(
            "no-intent.json",
            r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","receiverAgentId":"1696","content":"hello"}"#,
        );

        let err = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696")
            .expect_err("missing intent must fail")
            .to_string();
        assert!(err.contains("content must contain [intent:deliver]"));
    }

    #[test]
    fn rejects_a2a_file_arg_with_wrong_msg_type() {
        let path = write_temp_a2a(
            "wrong-msg-type.json",
            r#"{"msgType":"other","jobId":"0xabc123","receiverAgentId":"1696","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]"}"#,
        );

        let err = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696")
            .expect_err("wrong msgType must fail")
            .to_string();
        assert!(err.contains("payload.msgType must be a2a-agent-chat"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a2a_file_arg_with_group_readable_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = write_temp_a2a(
            "group-readable.json",
            r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","receiverAgentId":"1696","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]"}"#,
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let err = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696")
            .expect_err("group-readable file must fail")
            .to_string();
        assert!(err.contains("chmod 600"));
    }

    #[test]
    fn rejects_a2a_file_arg_for_wrong_receiver() {
        let path = write_temp_a2a(
            "wrong-receiver.json",
            r#"{"msgType":"a2a-agent-chat","jobId":"0xabc123","receiverAgentId":"8779","content":"jobId: 0xabc123\ndeliverableType: text\n- - -\nbody\n- - -\n[intent:deliver]"}"#,
        );

        let err = validate_a2a_file_arg(path.to_str().unwrap(), "0xabc123", "1696")
            .expect_err("receiver mismatch must fail")
            .to_string();
        assert!(err.contains("receiverAgentId 8779 does not match --agentId 1696"));
    }
}

/// Returns a warning text when inconsistent (used to prepend to the top of the script output).
///
/// Trigger scenarios: delayed system event, prior CLI operations have already advanced the status further;
/// returns None on network/parse failure (does not block script output, graceful fallback).
async fn check_status_freshness(
    job_id: &str,
    job_status_or_event: &str,
    agent_id: &str,
) -> (Option<String>, Option<task::common::PreFetchedTaskContext>) {
    use task::common::network::task_api_client::TaskApiClient;
    use task::common::state_machine::{parse_status_or_event, status_when_event, Event, Status};
    use task::common::PreFetchedTaskContext;

    // Events that skip freshness validation but still benefit from pre-fetching task data
    // (they have a valid jobId and their handlers currently run `common context` as Step 0/1).
    const PREFETCH_ONLY_EVENTS: &[&str] = &[
        "deliverable_received",
        "job_provider_reject",
        "attachment_added",
        "provider_conversation",
        // Subscription lifecycle: display-class notifications with no corresponding
        // standard task status — freshness check is meaningless (task stays `accepted`
        // while sub events flow on top), but prefetch is kept for service_name fallback.
        "sub_created",
        "sub_cancel",
        "sub_user_reject",
        "sub_asp_agree",
        "sub_asp_dispute",
        "sub_trial_into_active",
        "sub_renew",
        "sub_expire_warn",
        "sub_complete_notify",
        "sub_close_notify",
        "sub_failed_notify",
        "sub_reject_refund_notify",
        "sub_asp_selected",
    ];

    // Events that skip both freshness validation AND pre-fetching (no jobId yet, or irrelevant).
    const SKIP_ALL_EVENTS: &[&str] = &[
        "create_task",
        "approve_review",
        "reject_review",
        "user_attachment_received",
        "close",
        "job_user_reject",
        "dispute_raise",
        "agree_refund",
        "staked",
        "unstake_requested",
        "unstake_claimed",
        "unstake_cancelled",
        "stake_stopped",
        "evaluator_selected",
        "vote_committed",
        "reveal_started",
        "vote_revealed",
        "vote_commit_deadline_warn",
        "vote_reveal_deadline_warn",
        "cooldown_entered",
        "round_failed",
        "reward_claimed",
        "wakeup_notify",
    ];

    let is_prefetch_only = PREFETCH_ONLY_EVENTS.contains(&job_status_or_event);

    if SKIP_ALL_EVENTS.contains(&job_status_or_event) {
        return (None, None);
    }

    // For non-skip events, parse and check if the event is recognized.
    let event = parse_status_or_event(job_status_or_event);
    let expected = status_when_event(&event);
    // Display-class subscription events (sub_*) are notification-only: they emit no on-chain
    // action script and hold no task status of their own (status_when_event maps them all to the
    // synthetic "subscription" status that no real task ever reports). The freshness gate exists
    // to stop a sub from running a STALE on-chain action; it must NOT gate these, or every sub_*
    // notification is dropped on a live subscription (whose real status is accepted/closed/...).
    // Skip the gate but keep the pre-fetched context so the notification still renders title/service name.
    let is_display_only_sub = matches!(expected, Status::Other(ref s) if s == "subscription");
    if !is_prefetch_only && matches!(expected, Status::Other(ref s) if s == "unknown") {
        if DEBUG_LOG {
            eprintln!("[check-freshness] 跳过校验: 未识别的 event={job_status_or_event}");
        }
        return (None, None);
    }

    // Fetch task data — shared by both freshness-check and pre-fetch paths.
    let mut c = TaskApiClient::new();
    let resp = match c.get_with_identity(&c.task_path(job_id), agent_id).await {
        Ok(r) => r,
        Err(_) => return (None, None),
    };
    let mut ctx = PreFetchedTaskContext::from_api_response(&resp);

    // For job_submitted: prefer an unprocessed spool delivery over an existing
    // manifest. Subscription manifests are append-only, so checking the
    // manifest first could keep selecting an old delivery forever while a new
    // inbound signal remained stranded in the spool.
    //   ① temp file present → recover + save the oldest unprocessed delivery
    //   ② manifest present  → populate the newest saved delivery
    //   ③ neither           → leave ctx.deliverable=None; prompt outputs "wait"
    if job_status_or_event == "job_submitted" {
        if let Some(recovered) = {
            let short_id_fallback = &job_id[..job_id.len().min(10)];
            task::user::try_recover_from_temp_file(
                job_id,
                agent_id,
                short_id_fallback,
                &ctx.title,
                &ctx.token_symbol,
                &ctx.token_amount,
                ctx.provider_agent_id.as_deref(),
            )
        } {
            if DEBUG_LOG {
                eprintln!(
                    "[check-freshness] job_submitted: recovered deliverable from A2A spool file"
                );
            }
            ctx.deliverable = Some(task::common::PreFetchedDeliverable {
                path: recovered.saved_path.clone(),
                deliverable_type: recovered.deliverable_type.clone(),
                original_name: String::new(),
                text_content: recovered.text_content.clone(),
            });
            if let Some(prompt) = task::user::route_subscription_delivery_to_skill(
                job_id,
                agent_id,
                &recovered.saved_path,
                &recovered.deliverable_type,
                "recover",
                recovered.transport_identity.as_ref(),
            )
            .await
            {
                return (Some(prompt), Some(ctx));
            }
        } else if let Ok(Some(manifest)) =
            task::common::deliverables::read_manifest("user", job_id)
        {
            if let Some(entry) = manifest.entries.last() {
                let dir = task::common::deliverables::deliverables_dir("user", job_id)
                    .map(|d| d.join(&entry.filename).display().to_string())
                    .unwrap_or_default();
                let text_content = if entry.deliverable_type == "text" {
                    std::fs::read_to_string(&dir).ok()
                } else {
                    None
                };
                ctx.deliverable = Some(task::common::PreFetchedDeliverable {
                    path: dir,
                    deliverable_type: entry.deliverable_type.clone(),
                    original_name: entry.original_name.clone(),
                    text_content,
                });
            }
        } else if DEBUG_LOG {
            eprintln!("[check-freshness] job_submitted: no deliverable found — waiting for deliverable_received");
        }
    }

    let prefetched = Some(ctx);

    // Pre-fetch-only events + display-class sub_* events: return data without freshness validation.
    if is_prefetch_only || is_display_only_sub {
        return (None, prefetched);
    }

    // Freshness validation for chain events.
    let actual = match resp
        .get("status")
        .and_then(|v| v.as_i64())
        .and_then(|v| i32::try_from(v).ok())
    {
        Some(s) => Status::from_int(s),
        None => return (None, prefetched),
    };
    let actual_str = actual.as_str().to_string();

    let dispute_resolved_ok = matches!(event, Event::DisputeResolved)
        && matches!(actual, Status::Completed | Status::Failed);

    if DEBUG_LOG {
        eprintln!(
            "[check-freshness] job_id={job_id}, event={job_status_or_event}, expected_status={}, actual_status={actual_str}, match={}",
            expected.as_str(),
            actual == expected || dispute_resolved_ok,
        );
    }

    if actual == expected || dispute_resolved_ok {
        return (None, prefetched);
    }
    (Some(format!(
        "🛑 **Stale state — playbook blocked** (next-action's event arg is inconsistent with the task's real status; not emitting steps to prevent on-chain action on a stale event).\n\n\
         - You passed event = `{job_status_or_event}` (expected task status = `{expected_str}`)\n\
         - But task {job_id} real statusStr = `{actual_str}`\n\n\
         **MUST do** (pick one):\n\
         1. If the current inbound is a **P2P message** (a2a-agent-chat) → you likely picked the wrong event. Re-match the pseudo-event from the message content (e.g. `[intent:deliver]` → `deliverable_received`; a natural-language quote → `negotiate_reply`). Pseudo-events are not freshness-gated.\n\
         2. If the current inbound is a **system event** → re-run next-action with the `event` field in the `--message` JSON changed to `{actual_str}` (fetch the playbook matching the real status), or just ignore this stale notification and end the turn waiting for the next real chain event.\n\n\
         **MUST NOT**: do NOT guess the next step; do NOT call any task CLI before getting a fresh playbook; do NOT push this warning to the user via `onchainos agent user-notify`.\n",
        expected_str = expected.as_str(),
    )), prefetched)
}
