//! common — common query commands for the task system.
//!
//! Core command: `context`
//! Given a job_id + role, pull task details from the backend and generate a structured
//! natural-language context for the LLM (openclaw buyer/provider/evaluator AI) to understand
//! the current task state.

use anyhow::{bail, Result};
use clap::Subcommand;
use serde::Deserialize;

pub mod claim;
pub mod a2a_binding;
pub mod config;
pub mod deliverables;
pub mod dispute_upload;
pub mod in_progress;
pub mod network;
pub mod okx_a2a;
pub mod onchainos_self;
pub mod payment_mode;
pub mod pending_v2;
pub mod prefilled_notify;
pub mod prefilled_rating;
pub mod query;
pub mod review_gate;
pub mod search;
pub mod session_cleanup;
pub mod state_machine;
pub mod util;
pub mod version_notice;

use util::{fmt_unix_secs, validate_job_id};

use crate::commands::Context;

// ─── Chain constants ────────────────────────────────────────────────────

/// XLayer chain ID (the task system contract deployment chain).
pub const XLAYER_CHAIN_ID: i32 = 196;
/// XLayer chain index in string form (for the wallet API).
pub const XLAYER_CHAIN_INDEX: &str = "196";
/// XLayer chain name (for wallet_store address lookup; the chainName of chainIndex=196 in wallets.json).
pub const XLAYER_CHAIN_NAME: &str = "okb";

// ─── Agent role constants (the identity module's API `role` field) ──────

/// Buyer / requestor.
pub const AGENT_ROLE_BUYER: i64 = 1;
/// Seller / provider.
pub const AGENT_ROLE_PROVIDER: i64 = 2;
/// Evaluator (arbiter).
pub const AGENT_ROLE_EVALUATOR: i64 = 3;

pub use payment_mode::PaymentMode;

pub use util::{ensure_sufficient_balance, ensure_sufficient_balance_at};

/// Master switch for diagnostic `eprintln!` output across the task system.
/// Enabled by `cargo build --features debug-log`; default off (zero runtime cost).
pub const DEBUG_LOG: bool = cfg!(feature = "debug-log");

// ─── CLI definition ─────────────────────────────────────────────────────
#[derive(Subcommand)]
pub enum CommonCommand {
    /// Query task context and print a structured natural-language description for the LLM.
    ///
    /// Examples:
    ///   onchainos agent context task-001 --role buyer --agent-id 426
    ///   onchainos agent context task-001 --role provider --agent-id 558
    Context {
        /// Task ID (jobId), e.g. task-001 or 0x1a2b...
        job_id: String,

        /// Caller role: buyer | provider | evaluator.
        #[arg(long, default_value = "buyer")]
        role: String,

        /// Caller AgentID (**required**). The beta backend requires a non-empty agenticId header;
        /// a wallet may have multiple provider agents and the caller must pick one explicitly —
        /// the CLI does not auto-select. Wallet / communication addresses are looked up via
        /// `agent get-agents --agent-ids <agent_id>` automatically and need not be passed via the CLI.
        #[arg(long)]
        agent_id: String,
    },
}

// ─── Task detail response structure ─────────────────────────────────────
// Fields align with the backend spec: data field on /priapi/v1/aieco/task/{jobId} (flat).

/// Aligns with the spec: data field on /priapi/v1/aieco/task/{jobId}.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskDetail {
    job_id: String,
    task_id: Option<i64>,
    title: String,
    description: String,
    content_hash: Option<String>,
    token_address: Option<String>,
    /// Backend spec: the token symbol returned directly (USDT / USDG).
    token_symbol: Option<String>,
    token_amount: Option<String>,
    /// 0=unset / 1=escrow / 3=x402
    payment_mode: Option<i32>,
    /// Backend VisibilityEnum: 0=PUBLIC / 1=PRIVATE
    visibility: Option<i32>,
    /// 0=open / 1=accepted / 2=submitted / 3=rejected / 4=disputed / 5=complete / 7=close
    status: Option<i32>,
    sensitive_status: Option<i32>,
    category_codes: Option<Vec<String>>,
    chain_id: Option<i32>,
    min_credit_score: Option<f64>,
    buyer_agent_address: Option<String>,
    buyer_agent_id: Option<String>,
    provider_agent_address: Option<String>,
    provider_agent_id: Option<String>,
    group_id: Option<String>,
    expire_config: Option<serde_json::Value>,
    /// unix seconds; 0 means unset.
    expire_time: Option<i64>,
    payment_most_token_amount: Option<String>,
    create_time: Option<i64>,
}

// ─── Pre-fetched task context (lightweight, for playbook inline) ────────

/// Lightweight snapshot of the task detail, built from the same GET /task/{jobId}
/// response that `check_status_freshness` already makes. Passed into
/// `generate_next_action` so the playbook can inline key fields and skip the
/// redundant "Step 1: run common context" CLI round-trip.
#[derive(Debug, Clone)]
pub struct PreFetchedDeliverable {
    pub path: String,
    pub deliverable_type: String,
    pub original_name: String,
    pub text_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreFetchedTaskContext {
    pub title: String,
    pub description: String,
    pub token_symbol: String,
    pub token_amount: String,
    pub payment_mode: Option<i64>,
    pub max_budget: Option<String>,
    pub provider_agent_id: Option<String>,
    pub buyer_agent_id: Option<String>,
    pub visibility: Option<i64>,
    pub status: Option<i64>,
    pub deliverable: Option<PreFetchedDeliverable>,
    pub service_id: Option<String>,
    pub service_token_address: Option<String>,
    pub service_token_amount: Option<String>,
    pub service_params: Option<String>,
    pub buyer_agent_address: Option<String>,
    pub token_address: Option<String>,
}

impl PreFetchedTaskContext {
    /// Build from the raw `serde_json::Value` returned by GET /task/{jobId}.
    pub fn from_api_response(v: &serde_json::Value) -> Self {
        Self {
            title: v["title"].as_str().unwrap_or("").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
            token_symbol: v["tokenSymbol"].as_str().unwrap_or("?").to_string(),
            token_amount: v["tokenAmount"].as_str().unwrap_or("").to_string(),
            payment_mode: v["paymentMode"].as_i64(),
            max_budget: v["paymentMostTokenAmount"].as_str().map(String::from),
            provider_agent_id: v["providerAgentId"].as_str().map(String::from),
            buyer_agent_id: v["buyerAgentId"].as_str().map(String::from),
            visibility: v["visibility"].as_i64(),
            status: v["status"].as_i64(),
            deliverable: None,
            service_id: v["serviceId"].as_str().map(String::from),
            service_token_address: v["serviceTokenAddress"].as_str().map(String::from),
            service_token_amount: v["serviceTokenAmount"].as_str().map(String::from),
            service_params: v["serviceParams"].as_str().map(String::from),
            buyer_agent_address: v["buyerAgentAddress"].as_str().map(String::from),
            token_address: v["tokenAddress"].as_str().map(String::from),
        }
    }

    /// Format as the inline `[Pre-fetched task context]` block for playbook output.
    pub fn format_inline(&self) -> String {
        let pm_label = match self.payment_mode {
            Some(1) => String::from("escrow (1)"),
            Some(3) => String::from("x402 (3)"),
            Some(v) => format!("{v} (unknown)"),
            None => String::from("unknown"),
        };
        let max_b = self.max_budget.as_deref().unwrap_or("not set");
        let prov = self.provider_agent_id.as_deref().unwrap_or("none");
        let buyer = self.buyer_agent_id.as_deref().unwrap_or("none");
        let desc_line = if self.description.is_empty() {
            String::new()
        } else {
            format!("\x20\x20description: {}\n", self.description)
        };
        let sp_line = match &self.service_params {
            Some(sp) if !sp.is_empty() => format!("\x20\x20serviceParams: {}\n", sp),
            _ => String::new(),
        };
        let deliv_line = match &self.deliverable {
            Some(d) => format!(
                "\x20\x20deliverable: saved | path: {} | type: {} | name: {}\n",
                d.path, d.deliverable_type, d.original_name
            ),
            None => String::new(),
        };
        format!(
            "[Pre-fetched task context] (from status-check API — no need to call `common context` again unless a field below is missing)\n\
             \x20\x20title: {title}\n\
             {desc_line}\
             \x20\x20tokenSymbol: {sym} | tokenAmount: {amt} | paymentMode: {pm}\n\
             \x20\x20maxBudget (paymentMostTokenAmount): {max_b} | providerAgentId: {prov} | buyerAgentId: {buyer}\n\
             {sp_line}\
             {deliv_line}",
            title = self.title,
            sym = self.token_symbol,
            amt = self.token_amount,
            pm = pm_label,
        )
    }
}

// ─── Agent profile response structure ───────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    #[allow(dead_code)]
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub profile_description: Option<String>,
    /// Wallet address (owner / the EOA that deployed this agent).
    pub agent_wallet_address: Option<String>,
    /// XMTP communication address (used for agent-to-agent P2P communication).
    pub communication_address: Option<String>,
}

// ─── Identity-system subprocess wrappers ──────────────────────────────────
//
// All identity queries from the task system go through these two helpers.
// When the identity CLI changes (command names, response shapes), only
// these two functions need updating.

/// Query agents by ID (`get-agents --agent-ids`). Returns the flattened
/// list of matched agent JSON objects. Works for any agent (current
/// account or peer).
async fn raw_query_by_ids(agent_ids: &str) -> Result<Vec<serde_json::Value>> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("current_exe failed: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["agent", "get-agents", "--agent-ids", agent_ids])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("spawn `get-agents` failed: {e}"))?;

    let body: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!(
            "parse `get-agents` stdout failed: {e}; raw={}",
            String::from_utf8_lossy(&output.stdout)
        ))?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        bail!("`get-agents` returned failure: {err}");
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    Ok(flatten_agent_groups(&data))
}

/// List the current account's agents (`get-my-agents`). Always passes
/// `--owner-address` so the backend filters by the active account
/// server-side. When `role` is provided, also passes `--role`.
async fn raw_query_my_agents(role: Option<&str>) -> Result<Vec<serde_json::Value>> {
    let my_owner = current_account_xlayer_address()
        .ok_or_else(|| anyhow::anyhow!("no current XLayer address"))?;

    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("current_exe failed: {e}"))?;

    let mut args = vec!["agent", "get-my-agents", "--owner-address", &my_owner];
    let role_val: String;
    if let Some(r) = role {
        role_val = r.to_string();
        args.extend(["--role", &role_val]);
    }
    args.extend(["--page-size", "100"]);

    if DEBUG_LOG {
        eprintln!(
            "[raw_query_my_agents] running: {} {}",
            exe.display(),
            args.join(" ")
        );
    }

    let output = tokio::process::Command::new(&exe)
        .args(&args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("spawn `get-my-agents` failed: {e}"))?;

    let body: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!(
            "parse `get-my-agents` stdout failed: {e}; raw={}",
            String::from_utf8_lossy(&output.stdout)
        ))?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        bail!("`get-my-agents` returned failure: {err}");
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    Ok(flatten_agent_groups(&data))
}

/// Query the agent profile for the given agentId.
/// On any error path it falls back to a placeholder with agentId set.
pub async fn fetch_agent_profile(agent_id: &str) -> AgentProfile {
    let fallback = || AgentProfile {
        agent_id: Some(agent_id.to_string()),
        name: Some(format!("Agent {agent_id}")),
        profile_description: Some("(profile unavailable)".to_string()),
        agent_wallet_address: None,
        communication_address: None,
    };
    if agent_id.is_empty() {
        return fallback();
    }

    let all_agents = match raw_query_by_ids(agent_id).await {
        Ok(agents) => agents,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_profile] {e}; fallback"); }
            return fallback();
        }
    };

    if all_agents.is_empty() && DEBUG_LOG {
        eprintln!("[fetch_agent_profile] empty agent list (agentId={agent_id}); fallback");
    }

    let matched = all_agents.iter()
        .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(agent_id))
        .map(|a| AgentProfile {
            agent_id: Some(agent_id.to_string()),
            name: a.get("name").and_then(|v| v.as_str()).map(String::from),
            profile_description: a
                .get("profileDescription")
                .and_then(|v| v.as_str())
                .map(String::from),
            agent_wallet_address: a
                .get("agentWalletAddress")
                .and_then(|v| v.as_str())
                .map(String::from),
            communication_address: a
                .get("communicationAddress")
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    if !all_agents.is_empty() && matched.is_none() && DEBUG_LOG {
        eprintln!("[fetch_agent_profile] agentId={agent_id} not present in response; fallback");
    }
    matched.unwrap_or_else(fallback)
}


// ─── Current-account agent lookup ───────────────────────────────────────────
//
// New /agent/agent-list response shape returns multiple ownerAddress groups
// (it's a generic communication-lookup endpoint, no longer JWT-filtered to the
// current user). The CLI side must filter to the active account's XLayer
// address. These helpers centralize that logic for every task-side caller.

/// Resolve the current active account's XLayer (chainIndex=196) wallet address.
///
/// Returns lowercase string (chain addresses are case-insensitive; lowercase
/// makes downstream `==` comparisons safe).
/// Returns `None` if not logged in / no active account / no XLayer address.
pub fn current_account_xlayer_address() -> Option<String> {
    let wallets = match crate::wallet_store::load_wallets() {
        Ok(Some(w)) => w,
        _ => return None,
    };
    let account_id = crate::commands::agentic_wallet::account::resolve_active_account_id(&wallets).ok()?;
    let entry = wallets.accounts_map.get(&account_id)?;
    entry
        .address_list
        .iter()
        .find(|a| a.chain_index == XLAYER_CHAIN_INDEX)
        .map(|a| a.address.to_lowercase())
}

/// List agents belonging to the current active account.
/// Returns empty `Vec` on any failure — robust by design.
pub async fn fetch_my_agents() -> Vec<serde_json::Value> {
    match raw_query_my_agents(None).await {
        Ok(agents) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agents] matched {} agents", agents.len()); }
            agents
        }
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agents] {e}; returning empty"); }
            Vec::new()
        }
    }
}

/// List agents belonging to the current active account, filtered by role.
/// Returns empty `Vec` on any failure — robust by design.
pub async fn fetch_my_agents_by_role(role: &str) -> Vec<serde_json::Value> {
    match raw_query_my_agents(Some(role)).await {
        Ok(agents) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agents_by_role] matched {} agents (role={role})", agents.len()); }
            agents
        }
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agents_by_role] {e}; returning empty"); }
            Vec::new()
        }
    }
}

/// Find a specific agent by ID (not limited to current account).
/// Returns `None` on any failure or when no agent matches.
pub async fn fetch_agent_by_id(agent_id: &str) -> Option<serde_json::Value> {
    let id = agent_id.trim();
    if id.is_empty() {
        if DEBUG_LOG { eprintln!("[fetch_agent_by_id] empty agent_id; returning None"); }
        return None;
    }

    let agents = match raw_query_by_ids(id).await {
        Ok(a) => a,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_by_id] {e}; returning None"); }
            return None;
        }
    };

    let hit = agents.into_iter()
        .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(id));
    if DEBUG_LOG {
        eprintln!(
            "[fetch_agent_by_id] {} for agentId={id}",
            if hit.is_some() { "matched" } else { "no match" }
        );
    }
    hit
}

/// Resolve a `--role` CLI arg into the corresponding `role` numeric value
/// (1/2/3). Accepts both names (buyer / provider / requestor / evaluator)
/// and raw integers ("1" / "2" / "3"). Returns `None` for unrecognized input.
fn parse_role_filter(raw: &str) -> Option<i64> {
    match raw.trim().to_lowercase().as_str() {
        "buyer" | "requestor" | "1" => Some(AGENT_ROLE_BUYER),
        "provider" | "seller" | "2" => Some(AGENT_ROLE_PROVIDER),
        "evaluator" | "arbiter" | "3" => Some(AGENT_ROLE_EVALUATOR),
        _ => None,
    }
}

/// By-id direct lookup against the agent registry. Returns the matched
/// agent JSON object without printing anything. Works for any agent
/// (current account or peer).
pub async fn query_agent_by_id_direct(agent_id: &str) -> Result<serde_json::Value> {
    let id = agent_id.trim();
    if id.is_empty() {
        bail!("agent_id must not be empty");
    }

    let all = raw_query_by_ids(id).await?;
    all.into_iter()
        .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(id))
        .ok_or_else(|| anyhow::anyhow!("agentId={id} not found in `get-agents` response"))
}

/// `onchainos agent profile <agent_id>` — look up a single agent by id and
/// return its flat JSON profile. Works for **any** agent (current account or
/// peer), used to verify peer / designated-provider identities.
///
/// Thin wrapper over `query_agent_by_id_direct` that adds CLI-style output
/// (prints the agent JSON via `crate::output::success`).
pub async fn handle_profile(agent_id: &str) -> Result<()> {
    let agent = query_agent_by_id_direct(agent_id).await?;
    crate::output::success(agent);
    Ok(())
}

/// Spawn `onchainos agent service-list --agent-id <id>` as subprocess and
/// return the parsed `data` field (services array/object).
pub(crate) async fn spawn_service_list(agent_id: &str) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("current_exe failed: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["agent", "service-list", "--agent-id", agent_id])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("spawn `agent service-list` failed: {e}"))?;

    let body: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!(
            "parse `agent service-list` stdout failed: {e}; raw={}",
            String::from_utf8_lossy(&output.stdout)
        ))?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        bail!("`agent service-list` returned failure: {err}");
    }

    Ok(body.get("data").cloned().unwrap_or(serde_json::Value::Null))
}

/// Fetch an agent's service catalog (via `spawn_service_list`) and return the
/// single entry matching `service_id`. Returns:
/// - `Ok(Some(entry))` — service-list fetched, entry found
/// - `Ok(None)`         — service-list fetched, but no entry has this serviceId
///                        (e.g. buyer designated a stale / unregistered serviceId)
/// - `Err(e)`           — service-list fetch failed entirely (subprocess died,
///                        backend rejected, JSON parse failed). Callers usually
///                        want to treat this as "no match" — use `.ok().flatten()`.
///
/// Response navigation: `data[0].list[*]` (flattened by the same logic that
/// `designated_route_inner` uses). Empty `service_id` returns `Ok(None)`.
pub(crate) async fn find_service(
    agent_id: &str,
    service_id: &str,
) -> Result<Option<serde_json::Value>> {
    if service_id.is_empty() {
        return Ok(None);
    }
    let data = spawn_service_list(agent_id).await?;
    // service-list returns the service ID under the `id` key as a JSON number
    // (e.g. `"id": 1270`), while the buyer's designation envelope sends
    // `"serviceId"` as a string. Try both keys, coerce number↔string for
    // comparison.
    let matched = data
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("list"))
        .and_then(|list| list.as_array())
        .and_then(|list| list.iter().find(|s| {
            let id_val = s.get("id").or_else(|| s.get("serviceId"));
            let id_str = id_val.and_then(|v| {
                v.as_str().map(String::from)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
            id_str.as_deref() == Some(service_id)
        }).cloned());
    Ok(matched)
}

/// `onchainos agent designated-route --provider <agentId>` — runs service-list
/// + profile in parallel, applies role/online/endpoint routing logic, and
///   returns a single JSON with the route decision.
///
/// Output shape:
/// ```json
/// { "route": "x402"|"a2a"|"error",
///   "errorType": "not_provider"|"offline",   // only when route=error
///   "providerName": "...",
///   "onlineStatus": 1|2,
///   "endpoint": "https://...",               // only when route=x402
///   "feeAmount": "0.01",                     // only when route=x402
///   "feeTokenSymbol": "USDT"                 // only when route=x402
/// }
/// ```
/// In-process variant of the `designated-route` query — returns the resolved
/// route JSON (the same shape that `handle_designated_route` would print to
/// stdout). Used by buyer CLI flows to inline the routing query without an
/// LLM round-trip. Errors propagate; success cases (a2a / x402 / error) are
/// all encoded as `Ok(json)`.
pub async fn designated_route_inner(provider_id: &str, target_endpoint: Option<&str>) -> Result<serde_json::Value> {
    let id = provider_id.trim();
    if id.is_empty() {
        bail!("--provider must not be empty");
    }

    let (profile_res, svc_res) = tokio::join!(
        query_agent_by_id_direct(id),
        spawn_service_list(id),
    );

    // --- profile gate ---
    let profile = match profile_res {
        Ok(p) => p,
        Err(_) => {
            return Ok(serde_json::json!({
                "route": "error",
                "errorType": "not_provider",
            }));
        }
    };

    let role = profile.get("role").and_then(|v| v.as_i64()).unwrap_or(0);
    if role != 2 {
        return Ok(serde_json::json!({
            "route": "error",
            "errorType": "not_provider",
            "providerName": profile.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        }));
    }

    let provider_name = profile.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let online_status = profile.get("onlineStatus").and_then(|v| v.as_i64()).unwrap_or(1);

    // --- service-list ---
    let services_data = match svc_res {
        Ok(v) => v,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[designated-route] service-list fetch failed for {id}: {e}"); }
            serde_json::Value::Null
        }
    };
    // API shape: data = [{agentInfo: {...}, list: [{endpoint, fee, ...}], ...}]
    // Flatten data[*].list[*] to get individual service entries.
    let service_entries: Vec<&serde_json::Value> = services_data
        .as_array()
        .map(|arr| arr.iter()
            .flat_map(|item| item.get("list").and_then(|v| v.as_array()).into_iter().flatten())
            .collect())
        .unwrap_or_default();

    // Collect ALL services that have a non-empty endpoint.
    let all_with_endpoint: Vec<&serde_json::Value> = service_entries.iter()
        .filter(|s| s.get("endpoint").and_then(|v| v.as_str()).map(|e| !e.is_empty()).unwrap_or(false))
        .copied()
        .collect();

    // When --endpoint is specified, require an exact match; do NOT fall back.
    let selected = if let Some(target) = target_endpoint.filter(|s| !s.is_empty()) {
        match all_with_endpoint.iter().find(|s| {
            s.get("endpoint").and_then(|v| v.as_str()) == Some(target)
        }).copied() {
            Some(svc) => Some(svc),
            None => {
                return Ok(serde_json::json!({
                    "route": "error",
                    "errorType": "endpoint_not_found",
                    "providerName": provider_name,
                    "onlineStatus": online_status,
                    "requestedEndpoint": target,
                }));
            }
        }
    } else {
        all_with_endpoint.first().copied()
    };

    if let Some(svc) = selected {
        let endpoint = svc.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
        // service-list API returns `fee` (string) not `feeAmount`; `/match` API returns `feeAmount` (f64).
        let fee_amount = svc.get("feeAmount").and_then(|v| v.as_str())
            .or_else(|| svc.get("fee").and_then(|v| v.as_str()))
            .unwrap_or("");
        // service-list API may omit `feeTokenSymbol`; fall back to chainIndex + contractAddress lookup.
        let fee_token = match svc.get("feeTokenSymbol").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => util::resolve_symbol_from_svc(svc).await.unwrap_or_else(|e| {
                if DEBUG_LOG {
                    eprintln!("⚠ designated-route: failed to resolve feeTokenSymbol: {e}");
                }
                String::new()
            }),
        };
        let mut result = serde_json::json!({
            "route": "x402",
            "providerName": provider_name,
            "onlineStatus": online_status,
            "endpoint": endpoint,
            "feeAmount": fee_amount,
            "feeTokenSymbol": fee_token,
        });
        // When multiple services exist and no --endpoint was specified, expose
        // all services so the LLM can pick the correct one by matching against
        // the task description context.
        if target_endpoint.filter(|s| !s.is_empty()).is_none() && all_with_endpoint.len() > 1 {
            let svc_list: Vec<serde_json::Value> = all_with_endpoint.iter().map(|s| {
                serde_json::json!({
                    "serviceName": s.get("serviceName").and_then(|v| v.as_str()).unwrap_or(""),
                    "serviceDescription": s.get("serviceDescription").and_then(|v| v.as_str()).unwrap_or(""),
                    "endpoint": s.get("endpoint").and_then(|v| v.as_str()).unwrap_or(""),
                    "feeAmount": s.get("feeAmount").and_then(|v| v.as_str())
                        .or_else(|| s.get("fee").and_then(|v| v.as_str()))
                        .unwrap_or(""),
                    "feeTokenSymbol": s.get("feeTokenSymbol").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect();
            result["services"] = serde_json::json!(svc_list);
        }
        return Ok(result);
    } else {
        // No endpoint → A2A path; check online status
        if online_status == 2 {
            return Ok(serde_json::json!({
                "route": "error",
                "errorType": "offline",
                "providerName": provider_name,
                "onlineStatus": online_status,
            }));
        } else {
            return Ok(serde_json::json!({
                "route": "a2a",
                "providerName": provider_name,
                "onlineStatus": online_status,
            }));
        }
    }
}

/// CLI entry point — wraps `designated_route_inner` and prints the resulting
/// JSON to stdout. Existing callers (mod.rs `AgentCommand::DesignatedRoute`)
/// keep their `Result<()>` contract unchanged.
pub async fn handle_designated_route(provider_id: &str, target_endpoint: Option<&str>) -> Result<()> {
    let result = designated_route_inner(provider_id, target_endpoint).await?;
    crate::output::success(result);
    Ok(())
}

/// Spawn `onchainos agent x402-check --endpoint <url> --agent-id <id>` as
/// subprocess and return the parsed JSON output (the full `{ok, data}` body).
async fn spawn_x402_check(endpoint: &str, agent_id: &str, body: Option<&str>) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("current_exe failed: {e}"))?;

    let mut args = vec!["agent", "x402-check", "--endpoint", endpoint, "--agent-id", agent_id];
    let body_owned: String;
    if let Some(b) = body.filter(|s| !s.is_empty()) {
        body_owned = b.to_string();
        args.push("--body");
        args.push(&body_owned);
    }
    let output = tokio::process::Command::new(&exe)
        .args(&args)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("spawn `agent x402-check` failed: {e}"))?;

    let body: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!(
            "parse `agent x402-check` stdout failed: {e}; raw={}",
            String::from_utf8_lossy(&output.stdout)
        ))?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        bail!("`agent x402-check` returned failure: {err}");
    }

    Ok(body.get("data").cloned().unwrap_or(serde_json::Value::Null))
}

/// Fetch task detail and extract budget fields (max budget + token symbol).
async fn fetch_task_budget(job_id: &str, agent_id: &str) -> Result<(Option<String>, Option<String>)> {
    let mut client = network::task_api_client::TaskApiClient::new();
    let resp_val = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to get task detail: {e}"))?;

    let task: TaskDetail = serde_json::from_value(resp_val)
        .map_err(|e| anyhow::anyhow!("failed to parse task detail: {e}"))?;

    Ok((task.payment_most_token_amount, task.token_symbol))
}

/// `onchainos agent x402-validate` — validates an x402 endpoint, compares the
/// on-chain price against the registered fee and the task's max budget, and
/// returns a single JSON with the combined result.
///
/// Output shape:
/// ```json
/// { "result": "pass"|"x402_invalid"|"price_mismatch"|"over_budget",
///   "amountHuman": "0.01", "tokenSymbol": "USDT",
///   "acceptsJson": "...", "x402Version": 1,
///   "endpoint": "https://...",
///   "maxBudget": "0.1", "taskTokenSymbol": "USDT",
///   "feeAmount": "0.005", "feeTokenSymbol": "USDT" }
/// ```
pub async fn handle_x402_validate(
    endpoint: &str,
    agent_id: &str,
    job_id: &str,
    fee_amount: &str,
    fee_token: &str,
) -> Result<()> {
    let (x402_res, budget_res) = tokio::join!(
        spawn_x402_check(endpoint, agent_id, None),
        fetch_task_budget(job_id, agent_id),
    );

    // --- x402-check gate ---
    let x402_data = match x402_res {
        Ok(d) => d,
        Err(e) => {
            crate::output::success(serde_json::json!({
                "result": "x402_invalid",
                "reason": format!("x402-check failed: {e}"),
            }));
            return Ok(());
        }
    };

    let valid = x402_data.get("valid").and_then(|v| v.as_bool()).unwrap_or(false);
    if !valid {
        // Detect input_required: the endpoint is a valid x402 service but
        // needs business parameters before it returns the 402 challenge.
        if x402_data.get("inputRequired").and_then(|v| v.as_bool()) == Some(true) {
            let mut out = serde_json::json!({
                "result": "input_required",
                "endpoint": endpoint,
            });
            if let Some(msg) = x402_data.get("message") { out["message"] = msg.clone(); }
            if let Some(rao) = x402_data.get("requiredAnyOf") { out["requiredAnyOf"] = rao.clone(); }
            if let Some(flds) = x402_data.get("fields") { out["fields"] = flds.clone(); }
            // Pass through fee info from designated-route for reference
            out["feeAmount"] = serde_json::json!(fee_amount);
            out["feeTokenSymbol"] = serde_json::json!(fee_token);
            crate::output::success(out);
            return Ok(());
        }
        let mut out = serde_json::json!({ "result": "x402_invalid" });
        if let Some(reason) = x402_data.get("reason") {
            out["reason"] = reason.clone();
        }
        crate::output::success(out);
        return Ok(());
    }

    let amount_human = x402_data.get("amountHuman").and_then(|v| v.as_str()).unwrap_or("");
    let token_symbol = x402_data.get("tokenSymbol").and_then(|v| v.as_str()).unwrap_or("");
    let accepts_json = x402_data.get("acceptsJson").cloned().unwrap_or(serde_json::Value::Null);
    let x402_version = x402_data.get("x402Version").cloned().unwrap_or(serde_json::Value::Null);

    // --- DX-Step 2: price mismatch check (delta > 1%) ---
    let fee_f: f64 = fee_amount.parse().unwrap_or(0.0);
    let amount_f: f64 = amount_human.parse().unwrap_or(0.0);
    if fee_f > 0.0 && amount_f > 0.0 {
        let delta = ((amount_f - fee_f) / fee_f).abs();
        if delta > 0.01 {
            crate::output::success(serde_json::json!({
                "result": "price_mismatch",
                "amountHuman": amount_human,
                "tokenSymbol": token_symbol,
                "feeAmount": fee_amount,
                "feeTokenSymbol": fee_token,
                "acceptsJson": accepts_json,
                "x402Version": x402_version,
                "endpoint": endpoint,
            }));
            return Ok(());
        }
    }

    // --- DX-Step 3: budget check ---
    let (max_budget, task_token) = budget_res.unwrap_or((None, None));
    let max_budget_str = max_budget.as_deref().unwrap_or("");
    let task_token_str = task_token.as_deref().unwrap_or("");

    let max_f: f64 = max_budget_str.parse().unwrap_or(0.0);
    if max_f > 0.0 && amount_f > max_f {
        crate::output::success(serde_json::json!({
            "result": "over_budget",
            "amountHuman": amount_human,
            "tokenSymbol": token_symbol,
            "maxBudget": max_budget_str,
            "taskTokenSymbol": task_token_str,
            "acceptsJson": accepts_json,
            "x402Version": x402_version,
            "endpoint": endpoint,
        }));
        return Ok(());
    }

    // --- all checks passed ---
    crate::output::success(serde_json::json!({
        "result": "pass",
        "amountHuman": amount_human,
        "tokenSymbol": token_symbol,
        "maxBudget": max_budget_str,
        "taskTokenSymbol": task_token_str,
        "acceptsJson": accepts_json,
        "x402Version": x402_version,
        "endpoint": endpoint,
    }));

    Ok(())
}

/// `onchainos agent my-agents [--role <r>]` — flat list of the current active
/// account's agents (XLayer ownerAddress filter applied automatically),
/// optionally filtered by `role`. Hides the agent-list response shape
/// (`data[0].list[].agentList[]` nesting) from callers; downstream tooling /
/// LLM consumers receive a flat array.
pub async fn handle_my_agents(role: Option<&str>) -> Result<()> {
    let role_filter = match role {
        Some(raw) => match parse_role_filter(raw) {
            Some(n) => Some(n),
            None => bail!(
                "unrecognized --role value: {raw:?} (expected buyer / provider / evaluator, or 1 / 2 / 3)"
            ),
        },
        None => None,
    };

    let mut agents = fetch_my_agents().await;
    if let Some(want) = role_filter {
        agents.retain(|a| a.get("role").and_then(|v| v.as_i64()) == Some(want));
    }

    crate::output::success(serde_json::Value::Array(agents));
    Ok(())
}

// ── preflight ───────────────────────────────────────────────────────

pub(crate) async fn preflight_inner(role_raw: &str) -> Result<serde_json::Value> {
    let role_num = match parse_role_filter(role_raw) {
        Some(n) => n,
        None => bail!(
            "unrecognized --role value: {role_raw:?} (expected buyer / provider / evaluator, or 1 / 2 / 3)"
        ),
    };
    let role_label = match role_num {
        AGENT_ROLE_BUYER => "buyer",
        AGENT_ROLE_PROVIDER => "provider",
        AGENT_ROLE_EVALUATOR => "evaluator",
        _ => "unknown",
    };

    // ── 1. Wallet ─────────────────────────────────────────────────
    let wallet_ok;
    let wallet_detail;
    match crate::wallet_store::load_wallets() {
        Ok(Some(w)) => {
            let account_id = crate::commands::agentic_wallet::account::resolve_active_account_id(&w).ok();
            if let Some(ref id) = account_id {
                let name = w.accounts.iter()
                    .find(|a| a.account_id == *id)
                    .map(|a| a.account_name.clone())
                    .unwrap_or_default();
                wallet_ok = true;
                wallet_detail = serde_json::json!({
                    "ok": true,
                    "email": w.email,
                    "accountId": id,
                    "accountName": name,
                });
            } else {
                wallet_ok = false;
                wallet_detail = serde_json::json!({
                    "ok": false,
                    "hint": "wallet loaded but no active account; run `onchainos wallet login`",
                });
            }
        }
        _ => {
            wallet_ok = false;
            wallet_detail = serde_json::json!({
                "ok": false,
                "hint": "not logged in; run `onchainos wallet login`",
            });
        }
    }

    // ── 2. Identity ───────────────────────────────────────────────
    let identity_detail;
    if !wallet_ok {
        identity_detail = serde_json::json!({
            "ok": false,
            "hint": "skipped — wallet not logged in",
        });
    } else {
        let mut agents = fetch_my_agents().await;
        agents.retain(|a| a.get("role").and_then(|v| v.as_i64()) == Some(role_num));
        if agents.is_empty() {
            identity_detail = serde_json::json!({
                "ok": false,
                "role": role_label,
                "hint": format!("no {role_label} agent found; run `onchainos agent register` with role={role_label}"),
            });
        } else {
            let first = &agents[0];
            identity_detail = serde_json::json!({
                "ok": true,
                "role": role_label,
                "agentId": first.get("agentId").and_then(|v| v.as_str()).unwrap_or(""),
                "name": first.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "status": first.get("status"),
            });
        }
    }

    // ── 3. Communication (okx-a2a) ────────────────────────────────
    let communication_detail;
    if !wallet_ok {
        communication_detail = serde_json::json!({
            "ok": false,
            "hint": "skipped — wallet not logged in",
        });
    } else {
        let okx_a2a_found = std::process::Command::new("okx-a2a")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok();

        if !okx_a2a_found {
            communication_detail = serde_json::json!({
                "ok": false,
                "hint": "okx-a2a CLI not found; install via `npx @aspect-build/a2a-node` or load ensure-okx-a2a-communication-ready.md",
            });
        } else {
            let status_out = std::process::Command::new("okx-a2a")
                .arg("status")
                .output();
            match status_out {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let combined = format!("{stdout}{stderr}");
                    let running = combined.contains("running");
                    let stopped = combined.contains("stopped");
                    if running {
                        communication_detail = serde_json::json!({
                            "ok": true,
                            "state": "running",
                        });
                    } else if stopped {
                        communication_detail = serde_json::json!({
                            "ok": false,
                            "state": "stopped",
                            "hint": "okx-a2a is stopped; run `okx-a2a restart`",
                        });
                    } else {
                        communication_detail = serde_json::json!({
                            "ok": false,
                            "state": "unknown",
                            "raw": combined.trim(),
                            "hint": "okx-a2a status returned unexpected output; run `okx-a2a restart`",
                        });
                    }
                }
                Err(e) => {
                    communication_detail = serde_json::json!({
                        "ok": false,
                        "hint": format!("failed to run `okx-a2a status`: {e}"),
                    });
                }
            }
        }
    }

    let all_ok = wallet_detail.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
        && identity_detail.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
        && communication_detail.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);

    Ok(serde_json::json!({
        "ready": all_ok,
        "wallet": wallet_detail,
        "identity": identity_detail,
        "communication": communication_detail,
    }))
}

pub async fn handle_preflight(role_raw: &str) -> Result<()> {
    let result = preflight_inner(role_raw).await?;
    crate::output::success(result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_prepare_create(
    description: Option<&str>,
    title: Option<&str>,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    use super::buyer::draft::validate_draft_fields;

    // ── 1. Validate fields (local, instant) ──────────────────────
    let validation = validate_draft_fields(
        description, title, budget, max_budget, currency,
    );
    let v_ok = validation.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !v_ok {
        crate::output::success(serde_json::json!({
            "ok": false,
            "stage": "validation",
            "validation": validation,
        }));
        return Ok(());
    }

    // ── 2. Gate-check (wallet + identity + communication) ─────────
    let preflight = preflight_inner("buyer").await?;
    let pf_ok = preflight.get("ready").and_then(|v| v.as_bool()).unwrap_or(false);
    if !pf_ok {
        crate::output::success(serde_json::json!({
            "ok": false,
            "stage": "preflight",
            "validation": validation,
            "preflight": preflight,
        }));
        return Ok(());
    }

    // ── 3. Routing (designated-route, only when --provider given) ─
    let routing = if let Some(pid) = provider.filter(|s| !s.is_empty()) {
        match designated_route_inner(pid, None).await {
            Ok(r) => Some(r),
            Err(e) => {
                crate::output::success(serde_json::json!({
                    "ok": false,
                    "stage": "routing",
                    "validation": validation,
                    "preflight": preflight,
                    "routing": { "error": format!("{e:#}") },
                }));
                return Ok(());
            }
        }
    } else {
        None
    };

    let mut result = serde_json::json!({
        "ok": true,
        "validation": validation,
        "preflight": preflight,
    });
    if let Some(r) = routing {
        result["routing"] = r;
    }
    crate::output::success(result);
    Ok(())
}

/// Flatten the agent-list response into a flat `Vec` of agent JSON objects —
/// **pure shape conversion, no filtering**. Handles three response shapes:
///
/// - **Grouped**: `data.list[]` with `{ownerAddress, accountName, agentList[]}` groups
/// - **Flat list**: `data.list[]` with flat agent objects
/// - **Bare array**: `data` is already `[{agentId, ...}, ...]` (get-agents response)
pub fn flatten_agent_groups(data: &serde_json::Value) -> Vec<serde_json::Value> {
    // Bare array: `get-agents` returns data as `[{agentId, ...}, ...]` directly
    if let Some(arr) = data.as_array() {
        if arr.first().and_then(|x| x.get("agentId")).is_some() {
            return arr.clone();
        }
    }

    let list_val = data.get("list").cloned().or_else(|| {
        data.as_array()
            .and_then(|arr| arr.first())
            .and_then(|x| x.get("list"))
            .cloned()
    });
    let Some(list) = list_val.as_ref().and_then(|v| v.as_array()) else {
        if DEBUG_LOG {
            eprintln!(
                "[flatten_agent_groups] response missing `list` field (tried all shapes); raw data: {}",
                serde_json::to_string(data).unwrap_or_default()
            );
        }
        return Vec::new();
    };

    let mut flat = Vec::new();
    for entry in list {
        // New shape: entry is a group with `agentList`
        if let Some(agents) = entry.get("agentList").and_then(|v| v.as_array()) {
            let group_owner = entry.get("ownerAddress").and_then(|v| v.as_str());
            let group_account = entry.get("accountName").and_then(|v| v.as_str());
            for a in agents {
                let mut agent = a.clone();
                if let Some(obj) = agent.as_object_mut() {
                    if !obj.contains_key("ownerAddress") {
                        if let Some(o) = group_owner {
                            obj.insert(
                                "ownerAddress".to_string(),
                                serde_json::Value::String(o.to_string()),
                            );
                        }
                    }
                    if !obj.contains_key("accountName") {
                        if let Some(n) = group_account {
                            obj.insert(
                                "accountName".to_string(),
                                serde_json::Value::String(n.to_string()),
                            );
                        }
                    }
                }
                flat.push(agent);
            }
            continue;
        }
        // Old shape fallback: entry is an agent itself
        if entry.get("agentId").is_some() {
            flat.push(entry.clone());
        }
    }
    flat
}

// ─── Status descriptions ────────────────────────────────────────────────
fn status_desc(s: &str) -> &str {
    match s {
        "init"      => "Initializing (awaiting on-chain confirmation)",
        "created"   => "Awaiting acceptance (Created)",
        "accepted"  => "Accepted; ASP executing (Accepted)",
        "submitted" => "ASP submitted deliverable; awaiting User Agent review (Submitted)",
        "rejected"  => "User Agent rejected deliverable; arbitration possible within freeze period (Rejected)",
        "disputed"      => "Arbitration in progress (Disputed)",
        "admin_stopped" => "Admin stopped the task (AdminStopped)",
        "completed" | "complete" => "Task completed; funds released (Complete)",
        "failed"    => "Arbitration concluded; task closed (Failed)",
        "close"     => "User Agent closed the task (Close)",
        "expired"   => "Task expired (Expired)",
        _           => "Unknown status",
    }
}

fn payment_mode_desc(pm: i32) -> &'static str {
    PaymentMode::from_int(pm).desc()
}

// ─── Command handling ───────────────────────────────────────────────────

pub async fn run(cmd: CommonCommand, _ctx: &Context) -> Result<()> {
    match cmd {
        CommonCommand::Context { job_id, role, agent_id } => {
            run_context(&job_id, &role, &agent_id).await
        }
    }
}

async fn run_context(
    job_id: &str,
    role: &str,
    agent_id: &str,
) -> Result<()> {
    if let Err(msg) = validate_job_id(job_id) {
        bail!("{msg}");
    }
    // Validate role.
    if !["buyer", "provider", "evaluator"].contains(&role) {
        bail!("--role must be buyer / provider / evaluator");
    }
    if agent_id.is_empty() {
        bail!("--agent-id is required (beta backend requires non-empty agenticId header)");
    }

    // Fetch task details from the backend. The base url is resolved internally by TaskApiClient::new
    // via OKX_BASE_URL env > TASK_BASE_URL env > constant fallback; the CLI does not specify it explicitly.
    let mut client = network::task_api_client::TaskApiClient::new();
    let resp_val = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to get task detail: {e}"))?;

    // Backend spec: the response `data` is a flat task object directly (WalletApiClient already strips body["data"]).
    let task: TaskDetail = serde_json::from_value(resp_val)
        .map_err(|e| anyhow::anyhow!("failed to parse response: {e}"))?;

    // Fetch the agent's own profile: name / profileDescription / agentWalletAddress / communicationAddress.
    // All three roles need this — the "Your identity" block displays wallet + communication addresses;
    // provider additionally uses description for capability matching.
    // On fetch error this returns a fallback with agentId set; never empty.
    let profile = fetch_agent_profile(agent_id).await;

    // Build the context.
    let ctx_text = build_context(&task, role, agent_id, &profile).await;
    println!("{ctx_text}");
    Ok(())
}

async fn build_context(
    task: &TaskDetail,
    role: &str,
    agent_id: &str,
    profile: &AgentProfile,
) -> String {
    let mut out = String::with_capacity(1024);

    let role_enum = state_machine::Role::parse(role);
    let role_cn = match role_enum {
        Some(state_machine::Role::Buyer)     => "User Agent",
        Some(state_machine::Role::Provider)  => "Agent Service Provider (ASP)",
        Some(state_machine::Role::Evaluator) => "Evaluator Agent",
        None                                 => role,
    };

    // The spec returns status as an integer only; derive the enum locally via Status::from_int and use as_str() for the display string.
    let task_status = task
        .status
        .map(state_machine::Status::from_int)
        .unwrap_or_else(|| state_machine::Status::Other("unknown".to_string()));
    let status_str = task_status.as_str().to_string();
    let status_text = format!("{status_str} — {}", status_desc(&status_str));

    // ── Role declaration ─────────────────────────────────────────────────
    out.push_str(&format!("You are the {role_cn} in the task system.\n\n"));

    // ── Identity info ────────────────────────────────────────────────────
    // Wallet / communication addresses come from `agent get-agents` lookup (fetch_agent_profile);
    // buyerAgentAddress / providerAgentAddress in the task detail are still used in the
    // "User Agent info" / "ASP info" blocks below.
    out.push_str("[Your Identity]\n");
    out.push_str(&format!("- Role: {role_cn}\n"));
    out.push_str(&format!("- AgentID: {agent_id}\n"));
    if let Some(w) = &profile.agent_wallet_address {
        out.push_str(&format!("- Wallet address: {w}\n"));
    }
    if let Some(c) = &profile.communication_address {
        out.push_str(&format!("- Communication address: {c}\n"));
    }
    if let Some(n) = &profile.name {
        out.push_str(&format!("- Name: {n}\n"));
    }
    if let Some(d) = &profile.profile_description {
        out.push_str(&format!("- Description: {d}\n"));
    }
    out.push('\n');

    // ── Task details ─────────────────────────────────────────────────────
    out.push_str("[Task Details]\n");
    out.push_str(&format!("- Job ID: {}\n", task.job_id));
    if let Some(tid) = task.task_id {
        out.push_str(&format!("- Internal ID: {tid}\n"));
    }
    out.push_str(&format!("- Title: {}\n", task.title));
    out.push_str(&format!("- Description: {}\n", task.description));

    let amount = task.token_amount.as_deref().unwrap_or("not set");
    let token  = task.token_address.as_deref().unwrap_or("");
    let symbol = task.token_symbol.as_deref().unwrap_or("UNKNOWN");
    out.push_str(&format!("- Budget: {amount} {symbol} (token: {token})\n"));
    if let Some(max_amt) = &task.payment_most_token_amount {
        out.push_str(&format!("- 🔒 INTERNAL max budget (paymentMostTokenAmount): {max_amt} {symbol} ← for internal decisions only; NEVER include in any message sent to the ASP\n"));
    }

    let pm = task.payment_mode.unwrap_or(0);
    out.push_str(&format!(
        "- Payment mode (paymentType={}): {}\n",
        pm,
        payment_mode_desc(pm)
    ));
    let visibility = match task.visibility {
        Some(0) => "Public",
        Some(1) => "Private",
        _       => "Unknown",
    };
    out.push_str(&format!("- Visibility: {visibility}\n"));
    if let Some(chain) = task.chain_id {
        out.push_str(&format!("- Chain: chainId={chain}\n"));
    }
    if let Some(score) = task.min_credit_score {
        out.push_str(&format!("- Min credit score: {score}\n"));
    }
    if let Some(ec) = &task.expire_config {
        if let (Some(open_sec), Some(acc_sec)) = (
            ec.get("openExpireSec").and_then(|v| v.as_u64()),
            ec.get("acceptedExpireSec").and_then(|v| v.as_u64()),
        ) {
            out.push_str(&format!(
                "- Expiry: acceptance window {}h, delivery window {}h\n",
                open_sec / 3600,
                acc_sec / 3600
            ));
        }
    }
    out.push_str(&format!("- Created: {}\n", fmt_unix_secs(task.create_time)));
    out.push('\n');

    // ── Current status ───────────────────────────────────────────────────
    out.push_str("[Current Status]\n");
    out.push_str(&format!("- {status_text}\n"));
    out.push('\n');

    // ── User Agent info ─────────────────────────────────────────────────
    out.push_str("[User Agent Info]\n");
    match (&task.buyer_agent_id, &task.buyer_agent_address) {
        (Some(id), Some(addr)) => {
            out.push_str(&format!("- AgentID: {id}\n"));
            out.push_str(&format!("- Communication address: {addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID: {id}\n")),
        _ => out.push_str("- Unknown\n"),
    }
    out.push('\n');

    // ── ASP info ──────────────────────────────────────────────────────────
    out.push_str("[ASP Info]\n");
    match (&task.provider_agent_id, &task.provider_agent_address) {
        (Some(id), Some(addr)) => {
            out.push_str(&format!("- AgentID: {id}\n"));
            out.push_str(&format!("- Communication address: {addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID: {id}\n")),
        _ => out.push_str("- No ASP matched yet\n"),
    }

    // ── Role guide that must be loaded ───────────────────────────────────
    let skill_file = match role {
        "buyer"     => "client.md",
        "provider"    => "provider.md",
        "evaluator" => "evaluator.md",
        _           => "",
    };
    if !skill_file.is_empty() {
        out.push_str("[⚠️ Must Execute Immediately]\n");
        out.push_str(&format!(
            "Read the role guide skills/okx-agent-task/{skill_file} (same directory as skills/okx-agent-task/SKILL.md) immediately; it contains the complete negotiation rules and acceptance flow.\n"
        ));
    }

    out
}
