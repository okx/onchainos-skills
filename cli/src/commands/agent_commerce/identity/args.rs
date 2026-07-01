//! CLI `Args` definitions for every `onchainos agent ...` subcommand under
//! the identity module. Only clap structs live here — no business logic.

use clap::Args;

#[derive(Args, Clone, Debug)]
pub struct CreateArgs {
    /// Required (all roles). The agent name. Missing / empty → `missing required
    /// parameter: --name`.
    #[arg(long)]
    pub name: Option<String>,
    /// Required. One of `user` / `asp` / `evaluator`. Fixed at create — cannot
    /// be changed by `update`.
    #[arg(long)]
    pub role: Option<String>,
    /// Agent description. Required for `asp`; optional for `user` / `evaluator`.
    #[arg(long)]
    pub description: Option<String>,
    /// Profile-picture URL (upload an image via `agent upload` first, then pass
    /// the returned URL). Required for `asp` (no avatar → `ASP agents require an
    /// avatar`); optional for `user` / `evaluator` (omitted → default avatar).
    #[arg(long)]
    pub picture: Option<String>,
    /// Service list as a JSON array. Element keys: `serviceName`,
    /// `serviceDescription`, `serviceType` (`A2A` | `A2MCP`), `fee` (A2MCP
    /// required, A2A optional — plain number, USDT implied, ≤6 decimals),
    /// `endpoint` (A2MCP only; A2A must omit it). Required for `asp`: at least
    /// one service (empty → `ASP agents require at least one service`); ignored
    /// for `user` / `evaluator`.
    #[arg(long)]
    pub service: Option<String>,
}

/// INTERNAL — not a CLI subcommand. There is no `onchainos agent consent`
/// command; this struct backs `consent_impl`, which `pre-check` calls
/// internally to run the legal module's two-step consent flow. Step 1 (no
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

/// `onchainos agent update`: edit an existing agent. Only `--agent-id` is
/// required; every other flag is an optional partial change — omit a flag to
/// leave that field untouched. `role` and CommunicationAddress are immutable
/// and are not accepted here. Updates are incremental: agent-level fields are
/// sent only when provided, and `--service` carries only the services you want
/// to add / modify / remove (each tagged with an `operation`), never the full
/// list.
#[derive(Args, Clone, Debug)]
pub struct UpdateArgs {
    /// REQUIRED. The target agent's id (becomes cardJson `agentId`). Missing →
    /// `missing required parameter: --agent-id`.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Optional. New agent name. Omitted / empty → name left unchanged.
    #[arg(long)]
    pub name: Option<String>,
    /// Optional. New agent-level description. Omitted / empty → unchanged; an
    /// empty string does NOT clear an existing description.
    #[arg(long)]
    pub description: Option<String>,
    /// Optional. New profile-picture URL. Omitted / empty → unchanged.
    #[arg(long)]
    pub picture: Option<String>,
    /// Optional. Incremental service changes as a JSON array — only the services
    /// you want to add / modify / remove, NOT the full list. Element keys:
    /// `serviceName`, `serviceDescription`, `serviceType` (`A2A` | `A2MCP`),
    /// `fee` (A2MCP required — plain number, USDT implied, ≤6 decimals),
    /// `endpoint` (A2MCP only), plus `operation`: `create` (new service, no
    /// `id`) / `update` (modify, carry the existing service `id`) / `delete`
    /// (remove, carry the existing service `id`). Omitted → the `services` field
    /// is left out entirely (omission does NOT clear existing services).
    ///
    /// Example — add one A2MCP service and delete an existing one:
    ///   --service '[{"operation":"create","serviceName":"Price feed","serviceDescription":"Realtime prices","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"},{"operation":"delete","id":"svc_123"}]'
    #[arg(long)]
    pub service: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct GetMyAgentsArgs {
    /// Optional. Filter to one role: `user` / `asp` / `evaluator`.
    #[arg(long)]
    pub role: Option<String>,
    /// Filter to agents owned by this address.
    #[arg(long = "owner-address")]
    pub owner_address: Option<String>,
    /// Page number (1-based). Omitted → backend default.
    #[arg(long)]
    pub page: Option<String>,
    /// Results per page. Omitted → backend default.
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct GetAgentsArgs {
    /// Agent ID(s), comma-separated.
    #[arg(long = "agent-ids")]
    pub agent_ids: Option<String>,
}

/// `onchainos agent get`: the original dual-mode agent-list query — list mode
/// (no ids, paginated) or detail mode (`--agent-ids`, comma-joined into a single
/// `agentIdList` param). Hits `GET /agent/agent-list`.
#[derive(Args, Clone, Debug)]
pub struct GetArgs {
    /// Agent ID(s), comma-separated → detail mode. Omitted → list mode
    /// (your own agents, paginated).
    #[arg(long = "agent-ids")]
    pub agent_ids: Option<String>,
    /// Page number (1-based; list mode only). Omitted → backend default.
    #[arg(long)]
    pub page: Option<String>,
    /// Results per page (list mode only). Omitted → backend default.
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
    /// Required. One of `user` / `asp` / `evaluator`.
    #[arg(long)]
    pub role: Option<String>,
    /// Optional. Only needed the first time a wallet registers, when a prior
    /// `pre-check` (run without this flag) returned `consent.consentKey` plus
    /// the terms to display. After the user accepts those terms, re-run
    /// `pre-check` passing that key here — its presence submits the agreement
    /// (agreed=true). Omit it otherwise (already-consented wallets never
    /// receive one).
    #[arg(long = "consent-key")]
    pub consent_key: Option<String>,
}

/// `onchainos agent deactivate`: state toggle to unpublish an agent. Also the
/// arg shape for any single-agent-id status command.
#[derive(Args, Clone, Debug)]
pub struct AgentStatusArgs {
    /// REQUIRED (runtime-enforced). The target agent's id. Missing →
    /// `missing required parameter: --agent-id`.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
}

/// `onchainos agent activate`: unified activation that handles role guard,
/// agent-status(1), and (when approvalStatus ∈ {1,5}) the full QA + submit-approval
/// pipeline internally. All data fetching is done by the CLI itself.
#[derive(Args, Clone, Debug)]
pub struct ActivateArgs {
    /// REQUIRED (runtime-enforced). The target agent's id. Missing →
    /// `missing required parameter: --agent-id`.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Required: preferred language for backend review messages (BCP-47,
    /// e.g. `zh-CN`, `en-US`). Normalized to canonical BCP-47.
    #[arg(long = "preferred-language", required = true)]
    pub preferred_language: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct UploadArgs {
    /// REQUIRED (runtime-enforced). Local image file path to upload as an
    /// avatar; returns a CDN URL to pass to `create`/`update` `--picture`.
    /// Missing → `missing required parameter: --file`.
    #[arg(long)]
    pub file: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct SearchArgs {
    /// REQUIRED (runtime-enforced). Search keyword(s). Missing / empty →
    /// `missing required parameter: --query`.
    #[arg(long)]
    pub query: Option<String>,
    /// Optional feedback / rating filters (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub feedback: Vec<String>,
    /// Optional agent-info filters (comma-separated).
    #[arg(long = "agent-info", value_delimiter = ',')]
    pub agent_info: Vec<String>,
    /// Optional status filters (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub status: Vec<String>,
    /// Optional service filters (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub service: Vec<String>,
    /// Page number (1-based). Omitted → backend default.
    #[arg(long)]
    pub page: Option<String>,
    /// Results per page. Omitted → backend default.
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
}


#[derive(Args, Clone, Debug)]
pub struct ServiceListArgs {
    /// REQUIRED (runtime-enforced). The target agent's id whose services to
    /// list. Missing → `missing required parameter: --agent-id`.
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
    /// Required: agent id being reviewed.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Required: your (reviewer's) agent id.
    #[arg(long = "creator-id")]
    pub creator_id: Option<String>,
    /// Required: star rating 0.00–5.00 (step 0.01).
    #[arg(long)]
    pub score: Option<String>,
    /// Optional: free-text review.
    #[arg(long)]
    pub description: Option<String>,
    /// Required: related task id.
    #[arg(long = "task-id")]
    pub task_id: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FeedbackListArgs {
    /// REQUIRED (runtime-enforced). The target agent's id whose reviews to
    /// list. Missing → `missing required parameter: --agent-id`.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Page number (1-based). Omitted → backend default.
    #[arg(long)]
    pub page: Option<String>,
    /// Results per page. Omitted → backend default.
    #[arg(long = "page-size")]
    pub page_size: Option<String>,
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

/// `onchainos agent validate-listing`: pure-local (no HTTP, no network)
/// validator. Hidden (`hide=true`) — not shown in `--help`; used by the
/// skill during registration QA.
#[derive(Args, Clone, Debug)]
pub struct ValidateListingArgs {
    /// One of `user` / `asp` / `evaluator`. Defaults to `asp`.
    #[arg(long)]
    pub role: Option<String>,
    /// Agent name to validate against marketplace naming rules.
    #[arg(long)]
    pub name: Option<String>,
    /// Agent-level description to validate.
    #[arg(long)]
    pub description: Option<String>,
    /// JSON array string with the same element shape as create/update's
    /// `--service`. Ignored for non-ASP roles.
    #[arg(long)]
    pub service: Option<String>,
}
