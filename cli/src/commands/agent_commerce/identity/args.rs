//! CLI `Args` definitions for every `onchainos agent ...` subcommand under
//! the identity module. Only clap structs live here — no business logic.

use clap::Args;

#[derive(Args, Clone, Debug)]
pub struct CreateArgs {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub service: Option<String>,
}

/// `onchainos agent consent`: standalone first-time-creation terms consent
/// (the legal module's two-step flow, decoupled from `create`). Step 1 (no
/// flags) issues a `consentKey` + `terms`; step 2 (`--consent-key` +
/// `--agreed`) finalizes the user's accept/decline decision. fromAddr +
/// chainIndex are auto-filled (current XLayer wallet). See API doc
/// `pre-transaction/agent-consent`.
#[derive(Args, Clone, Debug)]
pub struct ConsentArgs {
    /// Step 2: the one-time consentKey returned by step 1; pass back together
    /// with `--agreed`.
    #[arg(long = "consent-key")]
    pub consent_key: Option<String>,
    /// Step 2: `true` = user agreed, `false` = user declined. Pass together
    /// with `--consent-key`.
    #[arg(long)]
    pub agreed: Option<bool>,
}

#[derive(Args, Clone, Debug)]
pub struct UpdateArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub service: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct GetArgs {
    #[arg(long = "agent-ids")]
    pub agent_ids: Option<String>,
    #[arg(long)]
    pub page: Option<String>,
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
}

/// `onchainos agent precheck`: unified registration entry (see the registration
/// flow diagram). `--role` is REQUIRED; `--consent-key` optional. Always returns
/// `{ canCreate, role, agentList?, reason?, consent? }`:
///   • canCreate:true                          → may register this role
///   • canCreate:false + reason + agentList    → blocked (single role already exists)
///   • canCreate:false + reason + consent{...}  → first-time wallet, terms not yet
///     accepted; the skill shows `consent.terms`, then re-invokes with `--consent-key`.
#[derive(Args, Clone, Debug)]
pub struct PrecheckArgs {
    /// Required (same shape as `agent create`: clap-optional, runtime-enforced).
    /// requester / provider / evaluator (aliases: 1/buyer/requestor → requester,
    /// 2 → provider, 3 → evaluator). Missing → `missing required parameter`;
    /// an unrecognized value → `invalid value for --role`.
    #[arg(long)]
    pub role: Option<String>,
    /// The one-time consentKey from a prior `consent` block. PRESENCE means "the
    /// user agreed" — the CLI submits the consent with `agreed=true`. Omit it and
    /// (for a first-time wallet) the CLI checks consent status / returns terms.
    #[arg(long = "consent-key")]
    pub consent_key: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct AgentStatusArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
}

/// `onchainos agent activate`: unified activation that handles role guard,
/// agent-status(1), and (when approvalStatus ∈ {1,5}) the full QA + submit-approval
/// pipeline internally. All data fetching is done by the CLI itself.
#[derive(Args, Clone, Debug)]
pub struct ActivateArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Preferred language for backend review messages (BCP-47, e.g. `zh-CN`,
    /// `en-US`). Normalized to canonical BCP-47; blank / malformed is omitted.
    #[arg(long = "preferred-language")]
    pub preferred_language: Option<String>,
    /// Skip validate-listing and submit for approval regardless of QA findings.
    /// Use after the user explicitly acknowledges a blockType:2 warning.
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args, Clone, Debug)]
pub struct UploadArgs {
    #[arg(long)]
    pub file: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct SearchArgs {
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub feedback: Vec<String>,
    #[arg(long = "agent-info", value_delimiter = ',')]
    pub agent_info: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub status: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    pub service: Vec<String>,
    #[arg(long)]
    pub page: Option<String>,
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
}


#[derive(Args, Clone, Debug)]
pub struct ServiceListArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
}

/// `onchainos agent get-by-address`: reverse-lookup an agent by communication
/// address + chain. Hidden (hide=true); only used by sub-agent / xmtp flows.
#[derive(Args, Clone, Debug)]
pub struct GetByAddressArgs {
    /// Communication address bound to the agent on-chain — required.
    #[arg(long = "communication-address", required = true)]
    pub communication_address: String,
    /// Chain index; defaults to XLayer (196).
    #[arg(long = "chain-index")]
    pub chain_index: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FeedbackSubmitArgs {
    /// Required: agent id being reviewed; maps to `comment.agentid` in the create-comment body.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Required: agent id of the reviewer; maps to `extraData.erc8004Msg.feedBackAgentId`.
    #[arg(long = "creator-id")]
    pub creator_id: Option<String>,
    /// Required: star rating 0.00–5.00 (up to 2 decimal places, step 0.01).
    /// The CLI multiplies by 20 with round-half-up to produce the 0–100 u32
    /// wire value for `comment.value`. Validation and mapping live in
    /// `utils::parse_stars_arg`.
    #[arg(long)]
    pub score: Option<String>,
    /// Optional: free-text review; maps to `comment.comment` in the create-comment body.
    #[arg(long)]
    pub description: Option<String>,
    /// Optional: taskId; maps to `extraData.erc8004Msg.taskId`. Omitted when empty.
    #[arg(long = "task-id")]
    pub task_id: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FeedbackListArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    #[arg(long)]
    pub page: Option<String>,
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
    #[arg(long = "sort-by")]
    pub sort_by: Option<String>,
}

/// `onchainos agent xmtp-sign`: sign an arbitrary message with the local
/// signing_seed. No broadcast — POSTs directly to pre-transaction/sign-msg
/// and returns the backend's signature.
#[derive(Args, Clone, Debug)]
pub struct XmtpSignArgs {
    /// The keyUuid generated at create time; retrievable via `agent get`.
    #[arg(long = "key-uuid")]
    pub key_uuid: Option<String>,
    /// Message to sign; forwarded verbatim to the backend.
    #[arg(long)]
    pub message: Option<String>,
}
