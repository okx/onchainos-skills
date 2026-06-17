pub mod chat;
pub mod identity;
pub mod task;

use anyhow::Result;
use clap::Subcommand;

use crate::commands::Context;

use task::common::DEBUG_LOG;

/// Shared `agent` namespace for identity + task-system commands.
#[derive(Subcommand)]
pub enum AgentCommand {
    // ── Identity ────────────────────────────────────────────────────────────
    /// Register a new Agent identity
    Create(identity::CreateArgs),

    /// First-time-creation terms consent (legal module). Two-step: step 1 (no
    /// flags) fetches the terms; step 2 (`--consent-key` + `--agreed`)
    /// finalizes the decision. Must run BEFORE `create` for new users.
    Consent(identity::ConsentArgs),

    /// Update Agent identity and services
    Update(identity::UpdateArgs),

    /// Query your Agents / agent details
    Get(identity::GetArgs),

    /// Reverse-lookup an Agent by communication address + chainIndex (hidden, internal).
    #[command(name = "get-by-address", hide = true)]
    GetByAddress(identity::GetByAddressArgs),

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

    /// Sign an arbitrary message with keyUuid + signing_seed (xmtp etc.); does not broadcast
    #[command(name = "xmtp-sign")]
    XmtpSign(identity::XmtpSignArgs),

    /// Submit an Agent for marketplace listing review (called after activate returns approvalStatus=1)
    #[command(name = "submit-approval")]
    SubmitApproval(identity::SubmitApprovalArgs),

    // ── Task system (Client) ────────────────────────────────────────────────
    /// Create a new task (Client)
    #[command(name = "create-task")]
    CreateTask {
        #[arg(long)] description: String,
        #[arg(long = "description-summary")] description_summary: Option<String>,
        #[arg(long)] budget: f64,
        #[arg(long = "max-budget")] max_budget: f64,
        #[arg(long)] currency: String,
        #[arg(long = "deadline-open")]  deadline_open: String,
        #[arg(long = "deadline-submit")] deadline_submit: String,
        #[arg(long)] title: Option<String>,
        /// Specified provider agentId (skip asp-match, negotiate directly with this provider or x402 accept)
        #[arg(long)] provider: Option<String>,
        /// Designated service endpoint (persisted for multi-service providers)
        #[arg(long)] endpoint: Option<String>,
        /// Local file paths to attach to the task after creation.
        #[arg(long = "file")] attachments: Option<Vec<String>>,
        /// Payment mode to set at creation time (escrow / x402).
        #[arg(long = "payment-mode")] payment_mode: Option<String>,
        /// Service ID from asp/match response
        #[arg(long = "service-id")] service_id: Option<String>,
        /// Service input parameters (natural language string)
        #[arg(long = "service-params")] service_params: Option<String>,
        /// Service token contract address
        #[arg(long = "service-token-address")] service_token_address: Option<String>,
        /// Service price (from asp/match feeAmount)
        #[arg(long = "service-token-amount")] service_token_amount: Option<String>,
        /// Task visibility: 1 = private (requires --provider), 0 = public
        #[arg(long, default_value = "1")] visibility: i32,
        /// Accepted for compatibility but ignored — buyer identity is auto-resolved.
        #[arg(long = "agentId", alias = "agent-id", hide = true)]
        _agent_id: Option<String>,
    },

    /// Search matching ASPs (pre-publish or post-publish)
    #[command(name = "asp-match")]
    AspMatch {
        /// Task description (required when no --job-id)
        #[arg(long = "task-desc", default_value = "")] task_desc: String,
        /// Job ID (required when task already exists)
        #[arg(long = "job-id")] job_id: Option<String>,
        /// Narrow to this ASP's services
        #[arg(long = "provider-agent-id")] provider_agent_id: Option<String>,
        /// Page number
        #[arg(long, default_value = "1")] page: usize,
        /// Buyer agent ID
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Set/replace ASP + service on existing task (off-chain, triggers job_asp_selected)
    #[command(name = "set-asp")]
    SetAsp {
        job_id: String,
        #[arg(long = "provider-agent-id")] provider_agent_id: String,
        #[arg(long = "service-id")] service_id: String,
        #[arg(long = "service-params")] service_params: String,
        #[arg(long = "service-token-address")] service_token_address: String,
        #[arg(long = "service-token-amount")] service_token_amount: String,
        #[arg(long = "payment-token-symbol")] payment_token_symbol: Option<String>,
        #[arg(long = "payment-token-amount")] payment_token_amount: Option<String>,
        #[arg(long = "payment-most-token-amount")] payment_most_token_amount: Option<String>,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Clear ASP + service fields (off-chain)
    #[command(name = "reset-asp")]
    ResetAsp {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Reject current ASP (off-chain, triggers job_user_reject)
    #[command(name = "user-reject")]
    UserReject {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Mark a provider as failed negotiation (excluded from future asp-match lists)
    #[command(name = "mark-failed")]
    MarkFailed {
        job_id: String,
        #[arg(long = "provider")] provider_agent_id: String,
    },

    /// Get current task status
    Status {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// List "tasks I have" (accepted / published by me). **Do not** use this to find new jobs — use `recommend-task` / `find-jobs` for that.
    #[command(visible_alias = "list")]
    Tasks {
        #[arg(long)] status: Option<String>,
        #[arg(long, default_value = "1")]  page: u32,
        #[arg(long, default_value = "20")] limit: u32,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Aggregated non-terminal tasks across **all agents under the current
    /// active account**, with `myRole` / `counterpartyAgentId` annotations so
    /// the user-session can route ad-hoc user instructions to the correct sub
    /// session (via `xmtp_sessions_query` → `xmtp_dispatch_session`).
    /// Status filter: includes 0 created / 1 accepted / 2 submitted / 3 refused
    /// / 4 disputed by default; pass `--include-terminal` to also list 5-9.
    #[command(name = "active-tasks")]
    ActiveTasks {
        /// Optional role filter: buyer | provider | evaluator (also accepts 1/2/3)
        #[arg(long)] role: Option<String>,
        /// Include terminal statuses (complete / close / expired / rejected / admin_stopped)
        #[arg(long = "include-terminal")] include_terminal: bool,
    },


    /// Set payment mode on-chain (standalone, before confirm-accept)
    #[command(name = "set-payment-mode")]
    SetPaymentMode {
        job_id: String,
        /// escrow / x402
        #[arg(long = "payment-mode")] payment_mode: Option<String>,
        #[arg(long = "token-symbol")] token_symbol: Option<String>,
        #[arg(long = "token-amount")] token_amount: Option<String>,
        /// x402 service endpoint URL
        #[arg(long)] endpoint: Option<String>,
    },

    /// Composite: save-agreed + conditional set-payment-mode → confirmNow branch
    #[command(name = "ack-to-confirm")]
    AckToConfirm {
        job_id: String,
        #[arg(long = "provider-agent-id")] provider_agent_id: String,
        #[arg(long = "token-symbol")] token_symbol: String,
        #[arg(long = "token-amount")] token_amount: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Read locally persisted negotiation result (no network)
    #[command(name = "get-agreed")]
    GetAgreed {
        job_id: String,
    },

    /// Client confirms provider and executes payment (setPaymentMode must be done first).
    /// All parameters are auto-resolved from the local negotiate-state written by save-agreed.
    #[command(name = "confirm-accept")]
    ConfirmAccept {
        job_id: String,
    },

    /// x402 Phase 2b: direct/accept after job_payment_mode_changed + x402 endpoint interaction
    #[command(name = "direct-accept")]
    DirectAccept {
        job_id: String,
        #[arg(long = "provider-agent-id")] provider_agent_id: String,
        #[arg(long = "token-symbol")] token_symbol: Option<String>,
        #[arg(long = "token-amount")] token_amount: Option<String>,
    },

    /// x402 Phase 2: x402_pay signing + direct/accept + endpoint replay
    #[command(name = "task-402-pay")]
    Task402Pay {
        job_id: String,
        #[arg(long = "provider-agent-id")] provider_agent_id: String,
        /// JSON accepts array from the HTTP 402 response
        #[arg(long)] accepts: String,
        /// x402 provider endpoint URL (for replay after signing)
        #[arg(long)] endpoint: String,
        #[arg(long = "token-symbol")] token_symbol: String,
        #[arg(long = "token-amount")] token_amount: String,
        /// Payer address (optional)
        #[arg(long)] from: Option<String>,
        /// JSON business body to POST during replay (for endpoints that require business parameters)
        #[arg(long)] body: Option<String>,
    },

    /// Validate an x402 endpoint and extract pricing info
    #[command(name = "x402-check")]
    X402Check {
        /// x402 provider endpoint URL
        #[arg(long)] endpoint: String,
        /// Buyer agent ID (used for auth on token detail queries)
        #[arg(long = "agent-id")] agent_id: Option<String>,
        /// JSON business body to POST (for endpoints that require business parameters to return 402)
        #[arg(long)] body: Option<String>,
    },

    /// Designated-provider routing: service-list + profile in one call
    #[command(name = "designated-route")]
    DesignatedRoute {
        /// Target provider agentId
        #[arg(long)] provider: String,
        /// Target service endpoint (for multi-service providers)
        #[arg(long)] endpoint: Option<String>,
    },

    /// Validate x402 endpoint + price match + budget check in one call
    #[command(name = "x402-validate")]
    X402Validate {
        /// x402 provider endpoint URL
        #[arg(long)] endpoint: String,
        /// Buyer agent ID
        #[arg(long = "agent-id")] agent_id: String,
        /// Job ID (for budget lookup)
        #[arg(long = "job-id")] job_id: String,
        /// Registered fee amount from designated-route
        #[arg(long = "fee-amount")] fee_amount: String,
        /// Registered fee token symbol from designated-route
        #[arg(long = "fee-token")] fee_token: String,
    },

    /// Client confirms task complete and releases payment
    Complete {
        job_id: String,
    },

    /// Client rejects deliverable
    Reject {
        job_id: String,
        #[arg(long)] reason: String,
    },

    /// Client closes task (only valid while Open)
    Close {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Convert private task to public listing
    #[command(name = "set-public")]
    SetPublic {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Provider generates payment invoice after provider_applied
    Payment {
        job_id: String,
        #[arg(long = "agent-id")] agent_id: Option<String>,
    },

    /// Provider account-pull: query pending claimable rewards
    #[command(name = "provider-claimable")]
    ProviderClaimable {
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// Provider account-pull: claim all pending rewards in one call
    #[command(name = "provider-claim-rewards")]
    ProviderClaimRewards {
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// List agents belonging to the **current active account**, flat output.
    ///
    /// Wrapper over `fetch_my_agents` — hides the agent-list response shape
    /// (`data[0].list[].agentList[]` nesting) from the LLM. Optional `--role`
    /// filter; output is a flat JSON array `[{agentId, name, role, status, ...}]`
    /// already scoped to the current account's XLayer ownerAddress.
    #[command(name = "my-agents")]
    MyAgents {
        /// Optional role filter: buyer | provider | evaluator (also accepts 1/2/3)
        #[arg(long)] role: Option<String>,
    },

    /// Look up a single agent's profile by `agentId` (any owner, not limited
    /// to current account). Wrapper over `agent get --agent-ids` that flattens
    /// the `list[].agentList[]` nesting and returns the matched agent as a
    /// single flat object. Used for verifying peer / designated provider
    /// identities (e.g. buyer-sub-playbook.md Provider validation).
    ///
    /// `ok: false` when not found / agentId malformed; otherwise `data` is
    /// the agent object `{agentId, name, role, status, ownerAddress,
    /// communicationAddress, agentWalletAddress, profileDescription, ...}`.
    Profile {
        /// Target agentId (ERC-8004 token ID, decimal string)
        agent_id: String,
    },

    // ── Task system (Provider) ──────────────────────────────────────────────
    /// Provider fetches recommended Public tasks matching their skill
    #[command(name = "recommend-task")]
    RecommendTask {
        /// Provider agentId (required). Beta backend rejects empty agenticId header → 3001 auth fail.
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Start accepting jobs: call `agent get` to pull all online provider agents and loop recommend-task over each
    #[command(name = "find-jobs")]
    FindJobs,

    /// Provider applies for a task (apply API → sign → broadcast)
    Apply {
        job_id: String,
        /// Negotiated token amount from `[intent:confirm]`. **Required**; must be > 0 (empty / 0 = apply for free, irreversible — CLI rejects).
        #[arg(long = "token-amount")]
        token_amount: String,
        /// Actual task currency (USDT / USDG); read from `[intent:confirm]` / `[intent:propose]`, do not assume USDT
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Provider submits deliverable (submit API → sign → broadcast)
    Deliver {
        job_id: String,
        #[arg(long, default_value = "")] file: String,
        #[arg(long, default_value = "Task completed, please review")] message: String,
        /// Text deliverable content for auto-save. When non-empty and --file is empty,
        /// the CLI writes this to a temp file and persists it as a text deliverable.
        #[arg(long = "deliverable-text", default_value = "")] deliverable_text: String,
        /// Provider agentId (required). Beta backend rejects empty agenticId header → 3001 auth fail.
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// Provider agrees to refund (agreeRefund API → sign → broadcast)
    #[command(name = "agree-refund")]
    AgreeRefund {
        job_id: String,
        /// Provider agentId (required)
        #[arg(long = "agent-id")] agent_id: String,
    },

    /// Provider declines a buyer-designated task (off-chain backend call, no signing).
    /// Used by the `job_asp_selected` flow when capability / price gate fails.
    #[command(name = "asp-reject")]
    AspReject {
        job_id: String,
        /// Provider agentId (required)
        #[arg(long = "agent-id")] agent_id: String,
        /// Optional decline reason recorded by the backend.
        #[arg(long, default_value = "")] reason: String,
    },

    /// Provider cold-start: contact the buyer in one shot.
    /// Combines `xmtp_start_conversation` (group + session create) + `xmtp_send`
    /// (the canonical self-intro / interest opener) so the LLM only runs ONE
    /// command instead of chaining two MCP tool calls. Opener content is fixed;
    /// no customization flag.
    #[command(name = "contact-buyer")]
    ContactBuyer {
        job_id: String,
        /// Provider agentId (required)
        #[arg(long = "agent-id")] agent_id: String,
    },



    /// Save negotiated payment params locally (agent calls after negotiation)
    #[command(name = "save-agreed")]
    SaveAgreed {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        /// Buyer agent ID (used to query task detail and validate budget ceiling)
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },

    /// Atomic save-agreed + set-payment-mode(escrow) — used by negotiate_ack
    /// to collapse two LLM steps into one CLI call. payment-mode is fixed to
    /// escrow (A2A negotiation path). If save-agreed fails the call short-
    /// circuits; if set-payment-mode fails the agreement is already persisted
    /// and the LLM can retry just step 2 via cli_failed.
    #[command(name = "save-agreed-and-set-payment")]
    SaveAgreedAndSetPayment {
        job_id: String,
        #[arg(long = "provider")]
        provider_agent_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long = "token-amount")]
        token_amount: String,
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    /// Client claims auto-refund after provider timeout
    #[command(name = "claim-auto-refund")]
    ClaimAutoRefund { job_id: String },

    /// Change payment token and amount (on-chain, wait for task_token_budget_change)
    #[command(name = "set-token-and-budget")]
    SetTokenAndBudget {
        job_id: String,
        #[arg(long = "token-symbol")]
        token_symbol: String,
        #[arg(long)]
        budget: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Change provider (on-chain, does not wait for confirmation)
    #[command(name = "set-provider")]
    SetProvider {
        job_id: String,
        #[arg(long = "provider-agent-id")]
        provider_agent_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Reject a provider's apply (on-chain pass-through; status stays `created`)
    #[command(name = "reject-apply")]
    RejectApply {
        job_id: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
    /// Change max budget (off-chain, succeeds immediately)
    #[command(name = "set-max-budget")]
    SetMaxBudget {
        job_id: String,
        #[arg(long = "max-budget")]
        max_budget: String,
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
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
    ListAttachments {
        job_id: String,
    },

    /// Save a deliverable file to persistent local storage
    #[command(name = "task-deliverable-save")]
    TaskDeliverableSave {
        #[arg(long)] job_id: String,
        #[arg(long)] role: String,
        #[arg(long)] file: String,
        #[arg(long, default_value = "file")] deliverable_type: String,
        #[arg(long)] title: String,
        #[arg(long)] short_id: String,
        #[arg(long = "file-key")] file_key: Option<String>,
        #[arg(long = "token-symbol")] token_symbol: Option<String>,
        #[arg(long = "token-amount")] token_amount: Option<String>,
        #[arg(long = "counterparty-agent-id")] counterparty_agent_id: Option<String>,
        #[arg(long = "counterparty-name")] counterparty_name: Option<String>,
    },

    /// List deliverables for a job or all jobs
    #[command(name = "task-deliverable-list")]
    TaskDeliverableList {
        /// If provided, list deliverables for this job only
        #[arg(long)] job_id: Option<String>,
        #[arg(long, default_value = "buyer")] role: String,
        /// Substring search across all jobs (only used when --job-id is omitted)
        #[arg(long)] search: Option<String>,
    },

    /// Provider claims auto-complete after buyer review timeout (review_expired)
    #[command(name = "claim-auto-complete")]
    ClaimAutoComplete {
        job_id: String,
        /// Provider's own agentId
        #[arg(long = "agent-id")]
        agent_id: String,
    },

    // ── Task system (sub-groups) ────────────────────────────────────────────
    /// Draft task commands: create, list, update, delete, publish
    #[command(subcommand)]
    Draft(task::buyer::DraftCommand),

    /// Dispute actions (provider): raise, evidence, info, upload
    #[command(subcommand)]
    Dispute(task::provider::DisputeCommand),

    /// Pending-decisions v2 — single-active queue with sessionKey primary key
    /// and LLM-playbook output. Design doc:
    /// https://okg-block.sg.larksuite.com/docx/URN9d8q49oYAJnxH6BYlYTkUgkd
    #[command(name = "pending-decisions-v2", subcommand)]
    PendingDecisionsV2(task::common::pending_v2::PendingDecisionsV2Command),

    // ── Task system (Evaluator Agent) ────────────────────────────────────────
    // Historically wrapped as `Evaluator(EvaluatorCommand)`; flattened to the top level in 2026-05
    // to align with the buyer/provider style. The `agent evaluator <sub>` form is no longer supported;
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
    /// Distinct from buyer's `claim` (which pulls per-job refund/reward).
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
    ///   `--role <buyer|provider|evaluator|auto>` — playbook routing role
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
        #[arg(long = "agentId", alias = "agent-id")] agent_id: String,
        /// Role: `buyer` / `provider` / `evaluator`, or `auto` to let the CLI
        /// resolve the role from `--agentId` (saves a separate `agent profile` round-trip).
        #[arg(long)] role: String,
        /// Full system event envelope as a JSON string — the entire `message` object.
        /// Required. Must contain at least `event` and `jobId`; optional fields the
        /// CLI reads: `code` / `jobTitle` / `provider` / `data` / `taskMinVersion`
        /// (plus any task-detail fields like `paymentMode` / `visibility` /
        /// `tokenAmount` / `tokenSymbol` / `serviceParams` that downstream scenes
        /// may consume directly).
        #[arg(long)]
        message: String,
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

    /// Search the public task marketplace (POST /priapi/v1/aieco/task/job/search).
    ///
    /// All filters are optional; passing none returns the whole pool paginated.
    ///
    /// Examples:
    ///   onchainos agent task-search --keyword "audit smart contract" --status 0 --order-by amount_asc
    ///   onchainos agent task-search --amount-min 10 --amount-max 500 --page 1 --page-size 20
    #[command(name = "task-search")]
    TaskSearch {
        /// Caller agent ID (sent as `agenticId` header).
        #[arg(long = "agent-id")]
        agent_id: String,

        /// Full-text keyword (matches title / description).
        #[arg(long)]
        keyword: Option<String>,

        /// Minimum task budget (human-readable, decimal-applied).
        #[arg(long = "amount-min")]
        amount_min: Option<f64>,

        /// Maximum task budget (human-readable, decimal-applied).
        #[arg(long = "amount-max")]
        amount_max: Option<f64>,

        /// Task statuses to include (repeatable / comma-separated). 0=OPEN, 1=ACCEPTED, 2=SUBMITTED, ...
        #[arg(long, value_delimiter = ',')]
        status: Vec<i32>,

        /// Sort order — one of `create_time_desc` / `create_time_asc` / `amount_desc` / `amount_asc`.
        #[arg(long = "order-by")]
        order_by: Option<task::common::search::TaskSearchOrderBy>,

        /// Filter by create time (unix milliseconds) — lower bound.
        #[arg(long = "create-time-start")]
        create_time_start: Option<i64>,

        /// Filter by create time (unix milliseconds) — upper bound.
        #[arg(long = "create-time-end")]
        create_time_end: Option<i64>,

        /// Page (1-based). Defaults to 1.
        #[arg(long, default_value_t = 1)]
        page: u32,

        /// Page size. Defaults to 20.
        #[arg(long = "page-size", default_value_t = 20)]
        page_size: u32,
    },

    /// Terminal-state session cleanup: cancel pending decisions + output
    /// xmtp_delete_conversation instructions. Replaces the multi-step
    /// manual cleanup in terminal playbooks.
    #[command(name = "session-cleanup")]
    SessionCleanup {
        #[arg(long = "job-id")]
        job_id: String,
        /// buyer | provider
        #[arg(long)]
        role: String,
    },

    /// Query a single Agent's (or up to 20 Agents') in-progress tasks & disputes
    /// (POST /priapi/v1/aieco/task/inProgress). The backend validates the
    /// caller→agent binding via JWT and classifies results by role
    /// (buyerTasks / providerTasks / evaluatorDisputes).
    ///
    /// Powers okx-ai-guide node 5a (registered-user home → "view what an Agent
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

    /// List the marketplace's top ASPs by sales (soldCount), highest first.
    /// Pulls the full ASP list and returns the top `--limit` (default 3; fewer
    /// if the marketplace has fewer).
    #[command(name = "top-asps")]
    TopAsps {
        /// How many to return, highest sales first. Default 3.
        #[arg(long, default_value_t = 3)]
        limit: usize,
    },
}

pub async fn run(cmd: AgentCommand, ctx: &Context) -> Result<()> {
    use task::buyer::TaskCommand as T;

    match cmd {
        // ── Identity ────────────────────────────────────────────────
        AgentCommand::Create(args) => identity::create(args, ctx).await,
        AgentCommand::Consent(args) => identity::consent(args, ctx).await,
        AgentCommand::Update(args) => identity::update(args, ctx).await,
        AgentCommand::Get(args) => identity::get(args, ctx).await,
        AgentCommand::GetByAddress(args) => identity::get_by_address(args, ctx).await,
        AgentCommand::Activate(args) => identity::activate(args, ctx).await,
        AgentCommand::Deactivate(args) => identity::deactivate(args, ctx).await,
        AgentCommand::Upload(args) => identity::upload(args, ctx).await,
        AgentCommand::Search(args) => identity::search(args, ctx).await,
        AgentCommand::ServiceList(args) => identity::service_list(args, ctx).await,
        AgentCommand::FeedbackSubmit(args) => identity::feedback_submit(args, ctx).await,
        AgentCommand::FeedbackList(args) => identity::feedback_list(args, ctx).await,
        AgentCommand::XmtpSign(args) => identity::xmtp_sign(args, ctx).await,
        AgentCommand::SubmitApproval(args) => identity::submit_approval(args, ctx).await,

        // ── Client (buyer) task commands ────────────────────────────
        AgentCommand::CreateTask {
            description, description_summary, budget, max_budget, currency,
            deadline_open, deadline_submit, title, provider, endpoint, attachments, payment_mode,
            service_id, service_params, service_token_address, service_token_amount, visibility,
            _agent_id: _,
        } => task::buyer::run_task(
            T::Create {
                description, description_summary, budget, max_budget, currency,
                deadline_open, deadline_submit, title, provider, endpoint, attachments, payment_mode,
                service_id, service_params, service_token_address, service_token_amount, visibility,
            }, ctx,
        ).await,

        AgentCommand::AspMatch { task_desc, job_id, provider_agent_id, page, agent_id } =>
            task::buyer::run_task(T::AspMatch { task_desc, job_id, provider_agent_id, page, agent_id }, ctx).await,

        AgentCommand::SetAsp { job_id, provider_agent_id, service_id, service_params, service_token_address, service_token_amount, payment_token_symbol, payment_token_amount, payment_most_token_amount, agent_id } =>
            task::buyer::run_task(T::SetAsp { job_id, provider_agent_id, service_id, service_params, service_token_address, service_token_amount, payment_token_symbol, payment_token_amount, payment_most_token_amount, agent_id }, ctx).await,

        AgentCommand::ResetAsp { job_id, agent_id } =>
            task::buyer::run_task(T::ResetAsp { job_id, agent_id }, ctx).await,

        AgentCommand::UserReject { job_id, agent_id } =>
            task::buyer::run_task(T::UserReject { job_id, agent_id }, ctx).await,

        AgentCommand::MarkFailed { job_id, provider_agent_id } =>
            task::buyer::run_task(T::MarkFailed { job_id, provider_agent_id }, ctx).await,

        AgentCommand::Status { job_id, agent_id } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::query::handle_status(&mut client, &job_id, agent_id.as_deref().unwrap_or(""), task::common::AGENT_ROLE_BUYER).await
        }

        AgentCommand::Tasks { status, page, limit, agent_id } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::query::handle_list(&mut client, status.as_deref(), page, limit, agent_id.as_deref().unwrap_or(""), task::common::AGENT_ROLE_BUYER).await
        }

        AgentCommand::ActiveTasks { role, include_terminal } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::query::handle_active_tasks(&mut client, role.as_deref(), include_terminal).await
        }


        AgentCommand::SetPaymentMode { job_id, payment_mode, token_symbol, token_amount, endpoint } =>
            task::buyer::run_task(T::SetPaymentMode { job_id, payment_mode, token_symbol, token_amount, endpoint }, ctx).await,

        AgentCommand::AckToConfirm { job_id, provider_agent_id, token_symbol, token_amount, agent_id } =>
            task::buyer::run_task(T::AckToConfirm { job_id, provider_agent_id, token_symbol, token_amount, agent_id }, ctx).await,

        AgentCommand::GetAgreed { job_id } =>
            task::buyer::run_task(T::GetAgreed { job_id }, ctx).await,

        AgentCommand::ConfirmAccept { job_id } =>
            task::buyer::run_task(T::ConfirmAccept { job_id }, ctx).await,

        AgentCommand::DirectAccept { job_id, provider_agent_id, token_symbol, token_amount } =>
            task::buyer::run_task(T::DirectAccept { job_id, provider_agent_id, token_symbol, token_amount }, ctx).await,

        AgentCommand::Task402Pay { job_id, provider_agent_id, accepts, endpoint, token_symbol, token_amount, from, body } =>
            task::buyer::run_task(T::Task402Pay { job_id, provider_agent_id, accepts, endpoint, token_symbol, token_amount, from, body }, ctx).await,

        AgentCommand::X402Check { endpoint, agent_id, body } =>
            task::buyer::run_task(T::X402Check { endpoint, agent_id, body }, ctx).await,

        AgentCommand::DesignatedRoute { provider, endpoint } =>
            task::common::handle_designated_route(&provider, endpoint.as_deref()).await,

        AgentCommand::X402Validate { endpoint, agent_id, job_id, fee_amount, fee_token } =>
            task::common::handle_x402_validate(&endpoint, &agent_id, &job_id, &fee_amount, &fee_token).await,

        AgentCommand::Complete { job_id } =>
            task::buyer::run_task(T::Complete { job_id }, ctx).await,

        AgentCommand::Reject { job_id, reason } =>
            task::buyer::run_task(T::Reject { job_id, reason }, ctx).await,

        AgentCommand::Close { job_id, agent_id } =>
            task::buyer::run_task(T::Close { job_id, agent_id }, ctx).await,

        AgentCommand::SetPublic { job_id, agent_id } =>
            task::buyer::run_task(T::SetPublic { job_id, agent_id }, ctx).await,

        AgentCommand::Payment { job_id, agent_id } =>
            task::buyer::run_task(T::Payment { job_id, agent_id }, ctx).await,

        AgentCommand::SaveAgreed { job_id, provider_agent_id, token_symbol, token_amount, agent_id } =>
            task::buyer::run_task(T::SaveAgreed { job_id, provider_agent_id, token_symbol, token_amount, agent_id }, ctx).await,

        AgentCommand::SaveAgreedAndSetPayment { job_id, provider_agent_id, token_symbol, token_amount, agent_id } =>
            task::buyer::run_task(T::SaveAgreedAndSetPayment { job_id, provider_agent_id, token_symbol, token_amount, agent_id }, ctx).await,

        AgentCommand::ClaimAutoRefund { job_id } =>
            task::buyer::run_task(T::ClaimAutoRefund { job_id }, ctx).await,

        AgentCommand::SetTokenAndBudget { job_id, token_symbol, budget, agent_id } =>
            task::buyer::run_task(T::SetTokenAndBudget { job_id, token_symbol, budget, agent_id }, ctx).await,
        AgentCommand::SetProvider { job_id, provider_agent_id, agent_id } =>
            task::buyer::run_task(T::SetProvider { job_id, provider_agent_id, agent_id }, ctx).await,
        AgentCommand::RejectApply { job_id, agent_id } =>
            task::buyer::run_task(T::RejectApply { job_id, agent_id }, ctx).await,
        AgentCommand::SetMaxBudget { job_id, max_budget, agent_id } =>
            task::buyer::run_task(T::SetMaxBudget { job_id, max_budget, agent_id }, ctx).await,

        AgentCommand::TaskAttach { job_id, file_paths } =>
            task::buyer::run_task(T::TaskAttach { job_id, file_paths }, ctx).await,
        AgentCommand::ListAttachments { job_id } =>
            task::buyer::run_task(T::ListAttachments { job_id }, ctx).await,

        AgentCommand::TaskDeliverableSave {
            job_id, role, file, deliverable_type, title, short_id,
            file_key, token_symbol, token_amount, counterparty_agent_id, counterparty_name,
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

        AgentCommand::TaskDeliverableList { job_id, role, search } => {
            match job_id {
                Some(jid) => task::common::deliverables::handle_list(&jid, &role),
                None => task::common::deliverables::handle_list_all(&role, search.as_deref()),
            }
        }

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

        AgentCommand::MyAgents { role } =>
            task::common::handle_my_agents(role.as_deref()).await,

        AgentCommand::Profile { agent_id } =>
            task::common::handle_profile(&agent_id).await,

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

        AgentCommand::Deliver { job_id, file, message, deliverable_text, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::Deliver { job_id, file, message, deliverable_text, agent_id }, ctx,
            ).await,

        AgentCommand::AgreeRefund { job_id, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::AgreeRefund { job_id, agent_id }, ctx,
            ).await,

        AgentCommand::AspReject { job_id, agent_id, reason } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::AspReject { job_id, agent_id, reason }, ctx,
            ).await,

        AgentCommand::ContactBuyer { job_id, agent_id } =>
            task::provider::run_provider(
                task::provider::ProviderCommand::ContactBuyer { job_id, agent_id }, ctx,
            ).await,


        // ── Sub-groups ──────────────────────────────────────────────
        AgentCommand::Draft(c) =>
            task::buyer::run_draft(c, ctx).await,

        AgentCommand::Dispute(c) =>
            task::provider::run_dispute(c, ctx).await,

        AgentCommand::PendingDecisionsV2(c) =>
            task::common::pending_v2::run(c).await,

        AgentCommand::SessionCleanup { job_id, role } =>
            task::common::session_cleanup::handle_session_cleanup(&job_id, &role),

        // ── Evaluator Agent flat dispatch ───────────────────────────
        AgentCommand::EvidenceInfo { job_id, agent_id, round_num } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::info::handle_info(&mut c, &job_id, &agent_id, &round_num).await
        }
        AgentCommand::VoteCommit { job_id, vote, reason, reason_summary, agent_id } => {
            let mut c = task::common::network::task_api_client::TaskApiClient::new();
            task::evaluator::commit::handle_commit(&mut c, &job_id, vote, &reason, &reason_summary, &agent_id).await
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

        AgentCommand::Common(c) =>
            task::common::run(c, ctx).await,

        AgentCommand::NextAction { agent_id, role, message } => {
            // Parse the `--message` envelope (required). Bail hard on parse failure —
            // it's the sole source of every routing field except `--role` / `--agentId`.
            let parsed_message: serde_json::Value = serde_json::from_str(&message)
                .map_err(|e| anyhow::anyhow!("--message must be a valid JSON object: {e}"))?;
            if DEBUG_LOG {
                eprintln!("[next-action] --message parsed: {} keys",
                    parsed_message.as_object().map(|o| o.len()).unwrap_or(0));
            }

            // Field extractors — all routing inputs live inside `--message`.
            let msg_str = |key: &str| -> Option<String> {
                parsed_message.get(key)
                    .and_then(|v| v.as_str())
                    .map(String::from)
            };
            let msg_i64 = |key: &str| -> Option<i64> {
                parsed_message.get(key).and_then(|v| v.as_i64())
            };

            let job_id: String = msg_str("jobId")
                .ok_or_else(|| anyhow::anyhow!("--message.jobId is required"))?;
            let event: String = msg_str("event")
                .ok_or_else(|| anyhow::anyhow!("--message.event is required"))?;
            let code: i32 = msg_i64("code").and_then(|v| i32::try_from(v).ok()).unwrap_or(0);
            let job_title: Option<String> = msg_str("jobTitle");
            let provider: Option<String> = msg_str("provider");
            let data: Option<String> = msg_str("data");
            let peer_task_min_version: Option<u32> = parsed_message.get("taskMinVersion")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .or_else(|| parsed_message.get("payload")
                    .and_then(|p| p.get("taskMinVersion"))
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()));
            let parsed_message = Some(parsed_message);
            if let Err(msg) = task::common::util::validate_job_id(&job_id) {
                anyhow::bail!(msg);
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
                        Some("Before executing the scene below, call `xmtp_dispatch_user` to notify the user (recommend upgrade but do **not** halt the flow). The `content:` template below is a sample — translate it to the user's language before sending:\n\
                             content: Your local task-system protocol version is outdated. Please run `onchainos upgrade` to upgrade for the best compatibility with peers.\n\
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
                if let Err(e) = task::buyer::negotiate::save_designated_provider(&job_id, pid) {
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
                     调用 `xmtp_dispatch_user` 通知用户：\n\
                     content: [{label}]{title_part}（{job_id}）交易执行失败（code={code}）。\n\
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
                        Some(1) => "buyer".to_string(),
                        Some(2) => "provider".to_string(),
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
                && matches!(resolved_role.as_str(), "buyer" | "client")
                && event == "job_created"
                && !task::buyer::negotiate::has_designated_provider(&job_id)
            {
                let mut fb_client = task::common::network::task_api_client::TaskApiClient::new();
                if let Ok(resp) = fb_client.get_with_identity(&fb_client.task_path(&job_id), &agent_id).await {
                    if let Some(pid) = resp["providerAgentId"].as_str().filter(|s| !s.is_empty()) {
                        if DEBUG_LOG {
                            eprintln!("[next-action] job_created fallback: API providerAgentId={pid}, persisting");
                        }
                        if let Err(e) = task::buyer::negotiate::save_designated_provider(&job_id, pid) {
                            if DEBUG_LOG {
                                eprintln!("[next-action] save_designated_provider (fallback) failed: {e}");
                            }
                        }
                    }
                }
            }

            // ── review gate: auto-mark buyer's review gate ──────────────────────
            // Must run AFTER role resolution so --role auto is correctly resolved.
            if matches!(resolved_role.as_str(), "buyer" | "client") {
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

            // Duplicate-event short-circuit: several chain events (job_created in
            // particular) can fire into both the task sub and the backup sub for the
            // same (jobId, role) pair. If a pending decision already exists for this
            // pair, the user has already been notified — emit a no-op playbook so the
            // current turn ends without re-notifying.
            //
            // Only dedup events that push a decision/notification to the user;
            // negotiation / handshake / lifecycle-only events have no user-visible
            // side effect and must always run their playbook.
            let dedup_eligible = matches!(
                event.as_str(),
                "job_created" | "job_submitted" | "review_deadline_warn" | "job_disputed" | "job_rejected"
            );
            if dedup_eligible
                && task::common::pending_v2::has_pending_for_job(&job_id, &resolved_role)
            {
                if DEBUG_LOG {
                    eprintln!(
                        "[next-action] duplicate event short-circuit: jobId={job_id} role={resolved_role} event={event} (pending entry already exists)"
                    );
                }
                println!(
                    "[Duplicate event] An entry for jobId={job_id} role={resolved_role} is already in the pending-decisions queue. The user has been notified already. **End the turn without re-notifying.** No tool calls required."
                );
                return Ok(());
            }

            // Status mismatch → block script output (to prevent sub from running an old script on-chain based on a stale event).
            // Only skip validation for PSEUDO_EVENTS / unknown / network failure; under normal conditions enforce strictly.
            let (freshness_warning, prefetched) = check_status_freshness(&job_id, &event, &agent_id).await;
            if let Some(w) = freshness_warning {
                println!("{w}");
                return Ok(());
            }
            let payment_mode = prefetched.as_ref().and_then(|p| p.payment_mode);
            let title_ref = job_title.as_deref();
            let prompt = match resolved_role.as_str() {
                "provider" | "seller" => {
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
                        ]),
                        None,
                    );
                    task::provider::flow::generate_next_action(&job_id, &event, &agent_id, title_ref, data.as_deref(), prefetched.as_ref(), parsed_message.as_ref()).await
                }
                "buyer" | "client" => {
                    crate::audit::log(
                        "cli",
                        "buyer/next_action_received",
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
                    task::buyer::flow::generate_next_action(&job_id, &event, &agent_id, title_ref, data.as_deref(), payment_mode, prefetched.as_ref(), parsed_message.as_ref()).await
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
                    task::evaluator::flow::generate_next_action(&job_id, &event, &agent_id).await
                }
                other => anyhow::bail!("--role 必须是 provider/buyer/client/evaluator，当前: {other}"),
            };
            if let Some(notice) = &version_notice {
                print!("{notice}");
            }
            println!("{prompt}");
            Ok(())
        }

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
            client_communication_address,
            provider_communication_address,
        } => chat::run(
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
            },
            ctx,
        ).await,

        AgentCommand::SystemConfig =>
            chat::run(chat::ChatCommand::SystemConfig, ctx).await,

        AgentCommand::Heartbeat { chain_index } =>
            chat::run(chat::ChatCommand::Heartbeat { chain_index }, ctx).await,

        AgentCommand::WakeupNotify { agent_ids } =>
            chat::run(chat::ChatCommand::WakeupNotify { agent_ids }, ctx).await,

        AgentCommand::TaskSearch {
            agent_id,
            keyword,
            amount_min,
            amount_max,
            status,
            order_by,
            create_time_start,
            create_time_end,
            page,
            page_size,
        } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::search::handle_task_search(
                &mut client,
                &agent_id,
                keyword.as_deref(),
                amount_min,
                amount_max,
                &status,
                order_by.as_ref(),
                create_time_start,
                create_time_end,
                page,
                page_size,
            )
            .await
        }
        AgentCommand::TaskInProgress { agent_ids } => {
            let mut client = task::common::network::task_api_client::TaskApiClient::new();
            task::common::in_progress::handle_in_progress(&mut client, &agent_ids).await
        }
        AgentCommand::TopAsps { limit } => identity::top_asps(limit, ctx).await,
    }
}

fn tx_failure_label(event: &str) -> &'static str {
    task::common::state_machine::Event::parse(event).failure_label()
}

/// Returns a warning text when inconsistent (used to prepend to the top of the script output).
///
/// Trigger scenarios: delayed system event, prior CLI operations have already advanced the status further;
/// returns None on network/parse failure (does not block script output, graceful fallback).
async fn check_status_freshness(job_id: &str, job_status_or_event: &str, agent_id: &str) -> (Option<String>, Option<task::common::PreFetchedTaskContext>) {
    use task::common::network::task_api_client::TaskApiClient;
    use task::common::state_machine::{parse_status_or_event, status_when_event, Event, Status};
    use task::common::PreFetchedTaskContext;

    // Events that skip freshness validation but still benefit from pre-fetching task data
    // (they have a valid jobId and their handlers currently run `common context` as Step 0/1).
    const PREFETCH_ONLY_EVENTS: &[&str] = &[
        "deliverable_received",
        "job_provider_reject",
    ];

    // Events that skip both freshness validation AND pre-fetching (no jobId yet, or irrelevant).
    const SKIP_ALL_EVENTS: &[&str] = &[
        "create_task", "switch_provider",
        "approve_review", "reject_review", "attachment_added", "buyer_attachment_received", "close", "set_public", "job_user_reject",
        "dispute_raise", "agree_refund",
        "staked", "unstake_requested", "unstake_claimed", "unstake_cancelled", "stake_stopped",
        "evaluator_selected", "vote_committed", "reveal_started", "vote_revealed", "vote_commit_deadline_warn", "vote_reveal_deadline_warn", "cooldown_entered", "round_failed",
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

    // For job_submitted: check local deliverable manifest to avoid an extra CLI round-trip.
    if job_status_or_event == "job_submitted" {
        if let Ok(Some(manifest)) = task::common::deliverables::read_manifest("buyer", job_id) {
            if let Some(entry) = manifest.entries.first() {
                let dir = task::common::deliverables::deliverables_dir("buyer", job_id)
                    .map(|d| d.join(&entry.filename).display().to_string())
                    .unwrap_or_default();
                ctx.deliverable = Some(task::common::PreFetchedDeliverable {
                    path: dir,
                    deliverable_type: entry.deliverable_type.clone(),
                    original_name: entry.original_name.clone(),
                });
            }
        }
    }

    let prefetched = Some(ctx);

    // Pre-fetch-only events: return data without freshness validation.
    if is_prefetch_only {
        return (None, prefetched);
    }

    // Freshness validation for chain events.
    let actual = match resp.get("status").and_then(|v| v.as_i64()).and_then(|v| i32::try_from(v).ok()) {
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
        "🛑 **状态脱节，剧本已 block**（next-action 入参与任务真实状态不一致，不输出步骤防止你按 stale event 上链）\n\n\
         - 你传的 event = `{job_status_or_event}`，对应任务状态应为 `{expected_str}`\n\
         - 但任务 {job_id} 真实 statusStr = `{actual_str}`\n\n\
         **必须做**（二选一）：\n\
         1. 如果当前 inbound 是 **P2P 消息**（a2a-agent-chat）→ 你很可能用错了 event。回到 buyer-sub-playbook.md / provider.md §3 Inbound Message Routing 重新匹配正确的事件（例如 `[intent:deliver]` → `deliverable_received`，自然语言报价 → `negotiate_reply`，`[intent:ack]` → `negotiate_ack`）。这些伪事件不受 freshness 限制。\n\
         2. 如果当前 inbound 是 **system event** → 重调 next-action，并在 `--message` JSON 里把 `event` 字段改成 `{actual_str}`（按真实状态拿剧本），或忽略本条过期通知结束 turn 等下一个真实链事件。\n\n\
         **禁止做**：不要硬猜下一步、不要在没拿到剧本前调任何 task CLI、不要把这条警告用 xmtp_dispatch_user 推用户。\n",
        expected_str = expected.as_str(),
    )), prefetched)
}
