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
    /// For `asp` it must be ≤500 characters and carry no URLs
    /// and no test/env markers.
    #[arg(long)]
    pub description: Option<String>,
    /// Profile-picture URL (upload an image via `agent upload` first, then pass
    /// the returned URL). Required for `asp` (no avatar → `ASP agents require an
    /// avatar`); optional for `user` / `evaluator` (omitted → default avatar).
    #[arg(long)]
    pub picture: Option<String>,
    /// Service list as a JSON array (required for `asp`: ≥1 service; ignored
    /// for `user` / `evaluator`).
    ///
    /// Element keys:
    ///   • serviceName        — required. 5–30 chars; must differ from the agent
    ///                          name; no price info and no test/env markers.
    ///   • serviceDescription — required. A newline-separated structure whose
    ///                          part count and meanings depend on serviceType:
    ///                            A2MCP (request description — all FOUR parts
    ///                            REQUIRED; an A2MCP listing missing any is rejected
    ///                            at listing QA): line 1 = what the service does;
    ///                            line 2 = parameter spec — ALL key parameters on
    ///                            ONE line separated by `;`, each written
    ///                            `<name>(<type>, required/optional): <meaning>`
    ///                            (append the default for an optional one; concisely
    ///                            listing the key params is enough when they don't
    ///                            all fit the cap); line 3 = request method (POST /
    ///                            GET, or the MCP tool name); line 4 = request
    ///                            example — a working `curl` command using the real
    ///                            endpoint URL.
    ///                            A2A (same shape for per-call and subscription
    ///                            pricing): line 1 = core-capability summary
    ///                            (REQUIRED — capability points + who it's for,
    ///                            plus the kind of signals for a signal service);
    ///                            line 2 = what the user must provide (OPTIONAL —
    ///                            e.g. wallet address / amount / chain); line 3 =
    ///                            delivery note (OPTIONAL — delivery format, plus
    ///                            copy-trading notes for a signal service).
    ///                          Whole text ≤1000 CJK chars (2000 half-width); no
    ///                          per-part length limit. No URLs (A2A only — the
    ///                          A2MCP request example necessarily carries the
    ///                          endpoint URL), and no test/env markers. A wallet
    ///                          or contract address is allowed anywhere.
    ///   • serviceGuide       — optional for every A2A pricing model. Guided
    ///                          flows never collect or display it for A2MCP,
    ///                          but the CLI accepts and forwards an explicitly
    ///                          supplied A2MCP value. Any supplied value must be at most
    ///                          2000 East-Asian display width (CJK/full-width
    ///                          characters count as 2, Latin/half-width as 1).
    ///   • serviceType        — `A2A` (agent-to-agent) or `A2MCP` (API service).
    ///   • fee                — single-purchase price. A plain number as a JSON
    ///                          string ("10"), USDT implied, ≤6 decimals. An
    ///                          EMPTY string ("") means "no single price"
    ///                          (subscription-priced A2A) and is forwarded
    ///                          verbatim.
    ///   • subscription       — A2A only. Array of monthly tiers, e.g.
    ///                          [{"interval":"month","fee":"10"}]. `interval` is
    ///                          currently limited to "month"; each `fee` is a
    ///                          plain number.
    ///   • freeTrial          — OPTIONAL. Free-trial duration in HOURS as a
    ///                          positive integer string ("72" = 3 days) for a
    ///                          subscription-priced A2A service. The low-level CLI
    ///                          accepts legacy positive-hour values for write-back;
    ///                          guided product flows create 72-hour trials only.
    ///                          Only allowed when
    ///                          `subscription` is non-empty; forbidden on
    ///                          single-purchase A2A and on A2MCP. Absent or an
    ///                          empty "" both mean NO trial (equivalent).
    ///   • endpoint           — A2MCP only (https://…); A2A must omit it.
    ///
    /// Pricing rules:
    ///   • A2MCP — `fee` is REQUIRED and must be a real plain number (an empty
    ///     `fee` is rejected); `subscription` is forbidden.
    ///   • A2A   — EXACTLY ONE billing model: a single-purchase `fee` XOR a
    ///     non-empty `subscription`. Never neither, and never both (the two
    ///     models are mutually exclusive).
    ///
    /// The serviceDescription is a single string with `\n` separators (up to 3
    /// lines for A2A — only line 1 is required; exactly 4 for A2MCP).
    ///   A2A e.g.   "Provides DEX arbitrage trading signals for onchain traders\nUser provides the target chain and budget\nDelivers structured signals, copy-trading supported".
    ///   A2MCP e.g. "Returns realtime token price quotes\ntokenAddress (string, required): token contract; chainIndex (string, required): chain id\nPOST\ncurl -X POST https://api.example.com/mcp -H \"Content-Type: application/json\" -d '{\"tokenAddress\":\"0xdac17f...\",\"chainIndex\":\"1\"}'".
    ///
    /// Examples:
    ///   A2MCP:            [{"serviceName":"Realtime price feed","serviceDescription":"Returns realtime token price quotes\ntokenAddress (string, required): token contract; chainIndex (string, required): chain id\nPOST\ncurl -X POST https://api.example.com/mcp -H \"Content-Type: application/json\" -d '{\"tokenAddress\":\"0xdac17f...\",\"chainIndex\":\"1\"}'","serviceType":"A2MCP","fee":"0.5","endpoint":"https://api.example.com/mcp"}]
    ///   A2A single only:  [{"serviceName":"DEX arbitrage signals","serviceDescription":"Provides DEX arbitrage trading signals for onchain traders\nUser provides the target chain and budget\nDelivers structured signals, copy-trading supported","serviceType":"A2A","fee":"0.11"}]
    ///   A2A sub only:     [{"serviceName":"DEX arbitrage signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals, copy-trading supported","serviceGuide":"Choose a market and submit your risk limit.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}]}]
    ///   A2A sub + trial:  [{"serviceName":"DEX arbitrage signals","serviceDescription":"Provides DEX arbitrage trading signals\nUser provides the target chain and budget\nDelivers structured signals, copy-trading supported","serviceGuide":"Choose a market and submit your risk limit.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}],"freeTrial":"72"}]
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
    /// `serviceName` (5–30 chars, distinct from the agent name),
    /// `serviceDescription` (newline-separated structure whose part count and
    /// meanings depend on serviceType — A2MCP request description (all FOUR
    /// REQUIRED; an A2MCP listing missing any is rejected at listing QA): line 1
    /// = what the service does, line 2 = parameter spec (ALL key params on ONE
    /// line separated by `;`, each `<name>(<type>, required/optional): <meaning>`),
    /// line 3 = request method (POST / GET or the MCP tool name), line 4 =
    /// request example (a working `curl` command using the real endpoint URL);
    /// A2A (same shape for per-call and subscription pricing): line 1 =
    /// core-capability summary (REQUIRED), line 2 = what the user must provide
    /// (OPTIONAL), line 3 = delivery note (OPTIONAL); whole text ≤1000 CJK chars
    /// (2000 half-width), no per-part length limit; no URLs (A2A only — the
    /// A2MCP request example necessarily carries the endpoint URL) / test
    /// markers),
    /// `serviceGuide` (optional for every A2A pricing model; guided flows never
    /// collect or display it for A2MCP, but the CLI accepts and forwards an
    /// explicitly supplied A2MCP value; when provided, at most 2000
    /// East-Asian display width, with
    /// CJK/full-width characters counting as 2 and Latin/half-width as 1),
    /// `serviceType` (`A2A` | `A2MCP`),
    /// `fee` (single-purchase price — plain number, USDT implied, ≤6 decimals),
    /// `subscription` (A2A only — array of `{interval, fee}`, `interval`
    /// limited to `"month"`), `freeTrial` (OPTIONAL — free-trial duration in
    /// HOURS as a positive integer string, e.g. `"72"`; the low-level CLI
    /// accepts legacy positive-hour values for write-back, while guided product
    /// flows create 72-hour trials only; only allowed on a subscription-priced service,
    /// forbidden on single-purchase A2A / A2MCP;
    /// absent or an empty `""` both mean NO trial, so an update that omits it
    /// leaves the service with no trial), `endpoint` (A2MCP only), plus `operation`:
    /// `create` (new service, no `id`) / `update` (modify, carry the existing
    /// service `id`) / `delete` (remove, send only `operation` and the existing
    /// service `id`).
    /// Same pricing rules as create: A2MCP requires a real `fee` (a plain
    /// number — an empty `fee` is rejected) and forbids `subscription`; A2A
    /// carries EXACTLY ONE billing model — a single-purchase `fee` XOR a
    /// `subscription`, never both. The billing model is transmitted on every
    /// update: a subscription-priced service sends `fee: ""` (the "no single
    /// price" marker, forwarded verbatim) together with its `subscription`; a
    /// single-priced service sends `subscription: []` together with a real
    /// `fee`. Omitting the whole `services` flag changes no service (omission
    /// does NOT clear existing services).
    ///
    /// Example — add one A2A service and delete an existing one:
    ///   --service '[{"operation":"create","serviceName":"Market Signals","serviceDescription":"Provides market signals for onchain traders","serviceType":"A2A","fee":"10","subscription":[]},{"operation":"delete","id":"svc_123"}]'
    ///
    /// Example — update a subscription-priced A2A service (empty single fee):
    ///   --service '[{"operation":"update","id":"7","serviceName":"Market Signals","serviceDescription":"Provides market signals for onchain traders","serviceGuide":"Choose a market and submit your risk limit.","serviceType":"A2A","fee":"","subscription":[{"interval":"month","fee":"10"}]}]'
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

#[cfg(test)]
mod search_args_tests {
    use super::SearchArgs;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        search: SearchArgs,
    }

    #[test]
    fn search_accepts_query_without_format() {
        let cli = TestCli::parse_from(["test", "--query", "market analysis"]);
        assert_eq!(cli.search.query.as_deref(), Some("market analysis"));
    }

    #[test]
    fn search_rejects_removed_format_flag() {
        for value in ["table", "raw"] {
            assert!(TestCli::try_parse_from([
                "test",
                "--query",
                "market analysis",
                "--format",
                value,
            ])
            .is_err());
        }
    }
}

#[derive(Args, Clone, Debug)]
pub struct ServiceListArgs {
    /// REQUIRED (runtime-enforced). The target agent's id whose services to
    /// list. Missing → `missing required parameter: --agent-id`.
    #[arg(long = "agent-id")]
    pub agent_id: Option<String>,
    /// Optional service id (UUID) to narrow the listing to one service —
    /// backend-side filter, used to fetch a single service's detail
    /// (including its `serviceGuide`) without pulling the agent's full
    /// service page. Omitted → the backend returns all services.
    #[arg(long = "service-id")]
    pub service_id: Option<String>,
}

/// `onchainos agent service-match`: search marketplace services directly.
#[derive(Args, Clone, Debug)]
pub struct ServiceMatchArgs {
    /// Initial-search Service capability keywords; accepts at most 10 values.
    #[arg(long, num_args = 1..)]
    pub keywords: Vec<String>,
    /// Initial-search filter: match Services belonging to this ASP Agent ID.
    #[arg(long = "asp-agent-id")]
    pub asp_agent_id: Option<String>,
    /// Initial-search filter: match Services by ASP name.
    #[arg(long = "asp-name")]
    pub asp_name: Option<String>,
    /// Initial-search filter: match Services by Service name.
    #[arg(long = "service-name")]
    pub service_name: Option<String>,
    /// Initial-search filter: match a Service by its Service ID.
    #[arg(long = "service-id")]
    pub service_id: Option<String>,
    /// Optional User Agent ID sent as the `agenticId` request header to exclude already-subscribed Services; valid for initial and continuation requests.
    #[arg(long = "agentic-id")]
    pub agentic_id: Option<String>,
    /// Initial-search minimum acceptable Service price; maps to `minPaymentTokenAmount` and must be >= 0.
    #[arg(long = "min-payment-token-amount")]
    pub min_payment_token_amount: Option<String>,
    /// Initial-search maximum acceptable Service price; maps to `maxPaymentTokenAmount` and must be >= 0.
    #[arg(long = "max-payment-token-amount")]
    pub max_payment_token_amount: Option<String>,
    /// Cursor returned by the previous response; cannot be combined with initial-search filters.
    #[arg(long = "search-after")]
    pub search_after: Option<String>,
    /// Requested number of Services, from 1 through 10.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=10))]
    pub limit: u8,
}

#[cfg(test)]
mod service_match_args_tests {
    use super::ServiceMatchArgs;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        service_match: ServiceMatchArgs,
    }

    #[test]
    fn accepts_multiple_keywords_and_agentic_id() {
        let cli = TestCli::parse_from([
            "test",
            "--keywords",
            "smart contract",
            "audit",
            "--agentic-id",
            "user-agent-001",
            "--service-id",
            "svc-001",
            "--min-payment-token-amount",
            "5",
            "--max-payment-token-amount",
            "10",
        ]);
        assert_eq!(cli.service_match.keywords, ["smart contract", "audit"]);
        assert_eq!(
            cli.service_match.agentic_id.as_deref(),
            Some("user-agent-001")
        );
        assert_eq!(cli.service_match.service_id.as_deref(), Some("svc-001"));
        assert_eq!(
            cli.service_match.min_payment_token_amount.as_deref(),
            Some("5")
        );
        assert_eq!(
            cli.service_match.max_payment_token_amount.as_deref(),
            Some("10")
        );
        assert_eq!(cli.service_match.limit, 1);
    }

    #[test]
    fn rejects_limit_outside_documented_range() {
        for limit in ["0", "11"] {
            assert!(
                TestCli::try_parse_from(["test", "--keywords", "audit", "--limit", limit,])
                    .is_err()
            );
        }
    }
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

/// `onchainos agent task-feedback`: fetch the feedback a given rater (the review
/// INITIATOR) left for a specific task. Read-only; hits `GET /agent/task-feedback`.
/// Returns the backend `data` array verbatim. When the rater already reviewed the
/// task it holds one review row (the echoed `agentId`, `taskId` and `chainIndex`
/// plus `feedbackId` and `comment`); otherwise it is empty, which also serves as
/// the duplicate-review guard before `feedback-submit`.
#[derive(Args, Clone, Debug)]
pub struct TaskFeedbackArgs {
    /// The rater's agent id — the review INITIATOR (backend `feedBackAgentId`),
    /// NOT the agent being reviewed. Required (runtime-enforced).
    #[arg(
        long = "agent-id",
        help = "Required. The rater's agent id."
    )]
    pub agent_id: Option<String>,
    /// The ERC-8004 task id the review is about. Required (runtime-enforced).
    #[arg(
        long = "task-id",
        help = "Required. The task id being reviewed."
    )]
    pub task_id: Option<String>,
    // chainIndex is not a flag: agent identities live on XLayer only, so the CLI
    // always sends chainIndex=196 (see task_feedback_impl).
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
