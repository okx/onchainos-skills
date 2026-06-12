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
    /// Optional pre-write snapshot: comma-separated agent ids that existed
    /// BEFORE this create (the caller's pre-check `agent get` result). When
    /// provided and the WS push is absent, the CLI diffs the post-broadcast
    /// agent list against this snapshot to compute the top-level `newAgentId`.
    #[arg(long = "known-agent-ids")]
    pub known_agent_ids: Option<String>,
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
    /// Optional pre-write snapshot: comma-separated agent ids that existed
    /// BEFORE this update (the caller's pre-check `agent get` result). When
    /// provided and the WS push is absent, the CLI diffs the post-broadcast
    /// agent list against this snapshot to compute the top-level `newAgentId`.
    /// Rarely meaningful for `update` (no new id is minted) but accepted for
    /// symmetry with `create`.
    #[arg(long = "known-agent-ids")]
    pub known_agent_ids: Option<String>,
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


/// `onchainos agent validate-listing`: pure-local (no HTTP, no network)
/// validator that checks an agent listing's fields against mechanical
/// marketplace rules. Used during registration / update QA flows (before
/// `create` / `update`), separately from `activate`.
#[derive(Args, Clone, Debug)]
pub struct ValidateListingArgs {
    /// requester / provider / evaluator (aliases: 1/buyer/requestor →
    /// requester, 2 → provider, 3 → evaluator). Defaults to provider.
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// JSON array string with the same element shape as create/update's
    /// `--service`. Ignored for non-provider roles.
    #[arg(long)]
    pub service: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct ServiceListArgs {
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
}

/// `onchainos agent get-by-address`: 按通信地址 + 链反查 agent。
/// 隐藏指令（hide=true），仅服务于 sub agent / xmtp 场景，不进 `agent -h`。
#[derive(Args, Clone, Debug)]
pub struct GetByAddressArgs {
    /// 通信地址（agent 上链注册时绑定的 communicationAddress）— 必填
    #[arg(long = "communication-address", required = true)]
    pub communication_address: String,
    /// 链 chainIndex，缺省走 XLayer (196)
    #[arg(long = "chain-index")]
    pub chain_index: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FeedbackSubmitArgs {
    /// 必填：被评价的 agent id，进 create-comment 请求体 `comment.agentid`。
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// 必填：评价发起方的 agent id，进广播 `extraData.erc8004Msg.feedBackAgentId`。
    #[arg(long = "creator-id")]
    pub creator_id: Option<String>,
    /// 必填：0.00-5.00 的星数，最多 2 位小数（步长 0.01）。CLI 内部 *20 后
    /// round-half-up 转成 0-100 u32 写入 create-comment 请求体
    /// `comment.value`（后端 wire 格式仍是 0-100 整数）。格式校验 + 映射
    /// 规则统一在 `utils::parse_stars_arg`。
    #[arg(long)]
    pub score: Option<String>,
    /// 选填：文字评价，进 create-comment 请求体 `comment.comment`。
    #[arg(long)]
    pub description: Option<String>,
    /// 选填：taskId，进广播 `extraData.erc8004Msg.taskId`；为空则不写入。
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

/// `onchainos agent xmtp-sign` 用户使用本地 signing_seed 对任意 message 做代签。
/// 不走广播，直接 POST 到 pre-transaction/sign-msg 拿后端返回的 signature。
#[derive(Args, Clone, Debug)]
pub struct XmtpSignArgs {
    /// keyUuid：之前 create 时生成过的那个 UUID，用户可通过 agent get 查出来
    #[arg(long = "key-uuid")]
    pub key_uuid: Option<String>,
    /// 要签名的消息，原样传给后端
    #[arg(long)]
    pub message: Option<String>,
}
