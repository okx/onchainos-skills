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
pub mod config;
pub mod deliverables;
pub mod dispute_upload;
pub mod in_progress;
pub mod network;
pub mod okx_a2a;
pub mod payment_mode;
pub mod pending_v2;
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
/// Flip to `true` and recompile to enable verbose debug logging; `false` (default)
/// lets the compiler eliminate all guarded branches (zero runtime cost).
pub const DEBUG_LOG: bool = true;

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
        /// `agent get --agent-ids <agent_id>` automatically and need not be passed via the CLI.
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

/// Query the agent profile for the given agentId (name / profileDescription / wallet address / communication address).
///
/// Spawns `onchainos agent get --agent-ids <id>` as a subprocess and parses stdout — does not
/// re-implement token / wallet-client / URL assembly logic, so this automatically follows any
/// future changes in the `agent get` implementation.
/// On any error path it falls back to a placeholder with agentId set (address fields None),
/// guaranteeing a non-empty return.
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

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_profile] current_exe failed: {e}; fallback"); }
            return fallback();
        }
    };

    // The subprocess inherits the parent's env (including OKX_BASE_URL), so it hits the exact same URL as the parent.
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get", "--agent-ids", agent_id]);
    let output = match cmd.output().await
    {
        Ok(o) => o,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_profile] spawn `agent get` failed: {e}; fallback"); }
            return fallback();
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            if DEBUG_LOG {
                eprintln!(
                    "[fetch_agent_profile] parse `agent get` stdout failed: {e}; raw={}; fallback",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            return fallback();
        }
    };

    // The output shape of `agent get` is wrapped by output::success: { ok: true, data: <value> }
    // On failure it is { ok: false, error: "..." }.
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        if DEBUG_LOG { eprintln!("[fetch_agent_profile] `agent get` returned failure: {err}; fallback"); }
        return fallback();
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);

    // Flatten the response (new shape: `list[].agentList[]` groups; old shape:
    // `list[]` flat agents). No ownerAddress filter — we're looking up any
    // agent by id, possibly belonging to another user (e.g. peer buyer profile).
    let all_agents = flatten_agent_groups(&data);
    if all_agents.is_empty() && DEBUG_LOG {
        eprintln!(
            "[fetch_agent_profile] `agent get` returned empty agent list (agentId={agent_id}); fallback"
        );
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
        eprintln!(
            "[fetch_agent_profile] agentId={agent_id} not present in `agent get` response; fallback"
        );
    }
    matched.unwrap_or_else(fallback)
}

/// Source of truth for the provider's self capability matching: service-list (the list of services the agent has actively registered).
#[derive(Debug, Default)]
struct AgentService {
    name: Option<String>,
    description: Option<String>,
    service_type: Option<String>,
    /// The service's registered fee (string form, unit usually USDT).
    /// Empty string / "0" / "0.0" is treated as unset — the provider should price by task workload.
    /// A non-zero positive value is treated as the service's standard price and used as the negotiation anchor.
    fee: Option<String>,
}

/// Shell out to `onchainos agent service-list --agent-id <id>` to fetch the service list.
/// Returns vec![] on failure / empty list; callers treat that as empty.
async fn fetch_agent_services(agent_id: &str) -> Vec<AgentService> {
    if agent_id.is_empty() {
        return vec![];
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_services] current_exe failed: {e}"); }
            return vec![];
        }
    };
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "service-list", "--agent-id", agent_id]);
    if DEBUG_LOG {
        eprintln!(
            "[fetch_agent_services] running: {} agent service-list --agent-id {agent_id}",
            exe.display()
        );
    }
    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_services] spawn `agent service-list` failed: {e}"); }
            return vec![];
        }
    };
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if DEBUG_LOG {
        eprintln!(
            "[fetch_agent_services] exit_code={:?} stdout_len={} stderr_len={}",
            output.status.code(),
            stdout_str.len(),
            stderr_str.len()
        );
        eprintln!("[fetch_agent_services] stdout=\n{stdout_str}");
        if !stderr_str.is_empty() {
            eprintln!("[fetch_agent_services] stderr=\n{stderr_str}");
        }
    }
    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_agent_services] parse stdout failed: {e}"); }
            return vec![];
        }
    };
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        if DEBUG_LOG { eprintln!("[fetch_agent_services] CLI returned failure: {err}"); }
        return vec![];
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    if DEBUG_LOG {
        eprintln!(
            "[fetch_agent_services] body.data before parsing: {}",
            serde_json::to_string_pretty(&data).unwrap_or_else(|_| "<unprintable>".to_string())
        );
    }
    let list = data
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|x| x.get("list"))
        .and_then(|v| v.as_array())
        .cloned();
    let Some(list) = list else {
        if DEBUG_LOG {
            eprintln!(
                "[fetch_agent_services] data[0].list missing; shape anomaly (agentId={agent_id}) — full data content in the previous body.data log line"
            );
        }
        return vec![];
    };
    list.iter()
        .map(|s| AgentService {
            name: s.get("serviceName").and_then(|v| v.as_str()).map(String::from),
            description: s
                .get("serviceDescription")
                .and_then(|v| v.as_str())
                .map(String::from),
            service_type: s.get("serviceType").and_then(|v| v.as_str()).map(String::from),
            fee: s.get("fee").and_then(|v| v.as_str()).map(String::from),
        })
        .collect()
}

/// Treats empty string / "0" / "0.0" / non-numeric junk as unset.
/// Returns `Some(non_zero_value)` only when `fee` parses as a positive number.
fn nonzero_fee(fee: &Option<String>) -> Option<&str> {
    let f = fee.as_deref()?.trim();
    if f.is_empty() {
        return None;
    }
    match f.parse::<f64>() {
        Ok(v) if v > 0.0 => Some(f),
        _ => None,
    }
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

/// Spawn `onchainos agent get` (paginated mode, no `--agent-ids`) and return the
/// list of agents belonging to the **current active account**.
///
/// Pipeline:
/// 1. resolve current account's XLayer ownerAddress (lowercase)
/// 2. shell out to `agent get` → parse JSON
/// 3. flatten the response (new shape: `list[].agentList[]`; old shape:
///    `list[]` flat agents) → filter by ownerAddress
///
/// Returns empty `Vec` on any failure (not logged in / no XLayer / network /
/// shape mismatch) — robust by design; callers can rely on non-panicking.
/// Each element of the returned `Vec` is the raw agent JSON object (fields:
/// `agentId` / `name` / `role` / `status` / `agentWalletAddress` / etc.).
pub async fn fetch_my_agents() -> Vec<serde_json::Value> {
    let Some(my_owner) = current_account_xlayer_address() else {
        if DEBUG_LOG { eprintln!("[fetch_my_agents] no current XLayer address; returning empty"); }
        return Vec::new();
    };

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agents] current_exe failed: {e}"); }
            return Vec::new();
        }
    };

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get"]);
    if DEBUG_LOG {
        eprintln!(
            "[fetch_my_agents] running: {} agent get (filter ownerAddress={my_owner})",
            exe.display()
        );
    }

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agents] spawn `agent get` failed: {e}"); }
            return Vec::new();
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            if DEBUG_LOG {
                eprintln!(
                    "[fetch_my_agents] parse stdout failed: {e}; raw={}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            return Vec::new();
        }
    };

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        if DEBUG_LOG { eprintln!("[fetch_my_agents] `agent get` returned failure: {err}"); }
        return Vec::new();
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let agents = flatten_my_agents(&data, &my_owner);
    if DEBUG_LOG {
        eprintln!(
            "[fetch_my_agents] matched {} agents under ownerAddress={my_owner}",
            agents.len()
        );
    }
    agents
}

/// Spawn `onchainos agent get` (paginated mode, no `--agent-ids`) and return
/// the single agent whose `agentId` matches the argument, by filtering the
/// flattened response client-side.
///
/// Same pipeline as [`fetch_my_agents`] but the filter key is `agentId` rather
/// than `ownerAddress`. Returns `None` on any failure (empty id / subprocess /
/// parse / shape mismatch) or when no agent matches.
pub async fn fetch_my_agent_by_id(agent_id: &str) -> Option<serde_json::Value> {
    let id = agent_id.trim();
    if id.is_empty() {
        if DEBUG_LOG { eprintln!("[fetch_my_agent_by_id] empty agent_id; returning None"); }
        return None;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agent_by_id] current_exe failed: {e}"); }
            return None;
        }
    };

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get"]);
    if DEBUG_LOG {
        eprintln!(
            "[fetch_my_agent_by_id] running: {} agent get (filter agentId={id})",
            exe.display()
        );
    }

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            if DEBUG_LOG { eprintln!("[fetch_my_agent_by_id] spawn `agent get` failed: {e}"); }
            return None;
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            if DEBUG_LOG {
                eprintln!(
                    "[fetch_my_agent_by_id] parse stdout failed: {e}; raw={}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            return None;
        }
    };

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        if DEBUG_LOG { eprintln!("[fetch_my_agent_by_id] `agent get` returned failure: {err}"); }
        return None;
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let hit = flatten_agent_groups(&data)
        .into_iter()
        .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(id));
    if DEBUG_LOG {
        eprintln!(
            "[fetch_my_agent_by_id] {} for agentId={id}",
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

/// Internal helper: by-id direct lookup against the agent registry. Returns
/// the matched agent JSON object without printing anything. Suitable for
/// reuse by other handlers (e.g. `next-action --role auto`) that need an
/// agent's metadata mid-flow without polluting stdout.
///
/// Spawns `onchainos agent get --agent-ids <id>` (backend-direct lookup, no
/// pagination), flattens the response groups, and filters by `agentId`. No
/// ownerAddress restriction — works for any agent (current account or peer).
pub async fn query_agent_by_id_direct(agent_id: &str) -> Result<serde_json::Value> {
    let id = agent_id.trim();
    if id.is_empty() {
        bail!("agent_id must not be empty");
    }

    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("current_exe failed: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["agent", "get", "--agent-ids", id])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("spawn `agent get` failed: {e}"))?;

    let body: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow::anyhow!(
            "parse `agent get` stdout failed: {e}; raw={}",
            String::from_utf8_lossy(&output.stdout)
        ))?;

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        bail!("`agent get` returned failure: {err}");
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let all = flatten_agent_groups(&data);
    all.into_iter()
        .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(id))
        .ok_or_else(|| anyhow::anyhow!("agentId={id} not found in `agent get` response"))
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
    deadline_open: Option<&str>,
    deadline_submit: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    use super::buyer::draft::validate_draft_fields;

    // ── 1. Validate fields (local, instant) ──────────────────────
    let validation = validate_draft_fields(
        description, title, budget, max_budget, currency,
        deadline_open, deadline_submit,
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

    // ── 2. Preflight (wallet + identity + communication) ─────────
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
/// **pure shape conversion, no filtering**. Single source of truth for handling
/// both old and new response shapes; callers layer their own filters on top.
///
/// Shapes handled:
/// - **New**: `data.list[]` is groups, each `{ownerAddress, accountName, agentList[]}`.
///   Returns all `agentList[]` items across all groups. Group-level
///   `ownerAddress` / `accountName` are injected into each agent if the
///   agent itself is missing them (defensive — current spec already
///   duplicates `ownerAddress` at agent level, but next spec rev might not).
/// - **Old**: `data.list[]` is flat agent objects. Pass through.
///
/// `data` is the value at `body.data` after the `{ok, data}` envelope is
/// stripped. Handles both object shape (`{list:...}` after
/// `normalize_singleton_object`) and array shape (`[{list:...}]`).
pub fn flatten_agent_groups(data: &serde_json::Value) -> Vec<serde_json::Value> {
    // data may be:
    //   - object {list, page, ...} after normalize_singleton_object unwraps singleton
    //   - array [{list, page, ...}]
    let list_val = data.get("list").cloned().or_else(|| {
        data.as_array()
            .and_then(|arr| arr.first())
            .and_then(|x| x.get("list"))
            .cloned()
    });
    let Some(list) = list_val.as_ref().and_then(|v| v.as_array()) else {
        if DEBUG_LOG {
            eprintln!(
                "[flatten_agent_groups] response missing `list` field (tried both shapes); raw data: {}",
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

/// Extract agents matching `my_owner` (lowercase) from the agent-list response.
/// Thin wrapper over `flatten_agent_groups` + per-agent `ownerAddress` filter.
fn flatten_my_agents(data: &serde_json::Value, my_owner: &str) -> Vec<serde_json::Value> {
    flatten_agent_groups(data)
        .into_iter()
        .filter(|a| {
            a.get("ownerAddress")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase())
                .as_deref()
                == Some(my_owner)
        })
        .collect()
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
    // Wallet / communication addresses come from `agent get` lookup (fetch_agent_profile);
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
    // ── Capability-match check (provider + created status only) ─────────
    // Source of truth: service-list (the agent's registered service catalogue).
    // **Any single service** matching the task domain passes; only when **all** services
    // fail to match is it judged a mismatch. profileDescription is a fallback reference
    // only and is not the sole criterion (the description is a generic self-intro,
    // service-list is the real capability set).
    /*if role_enum == Some(state_machine::Role::Provider)
        && task_status == state_machine::Status::Created
    {
        let services = fetch_agent_services(profile.agent_id.as_deref().unwrap_or("")).await;
        out.push_str("[⚠️ Step 1: Capability Match Check (mandatory, do not skip)]\n");
        if services.is_empty() {
            out.push_str("- Your service list (service-list): **empty** — no services registered\n");
            if let Some(desc) = &profile.profile_description {
                out.push_str(&format!("- Fallback reference — ASP description: {desc}\n"));
            }
        } else {
            out.push_str("- Your service list (service-list, **source of truth for capability match + quote anchor**):\n");
            for (i, svc) in services.iter().enumerate() {
                let name = svc.name.as_deref().unwrap_or("(no name)");
                let desc = svc.description.as_deref().unwrap_or("(no description)");
                let stype = svc.service_type.as_deref().unwrap_or("?");
                // fee field: non-zero positive value displays "registered price X USDT" as the negotiation anchor;
                // unset / 0 / empty displays "unset" so the agent estimates by workload.
                let fee_hint = match nonzero_fee(&svc.fee) {
                    Some(f) => format!("registered fee {f} USDT (use as negotiation anchor)"),
                    None => "registered fee not set (estimate by workload; do not overcharge)".to_string(),
                };
                out.push_str(&format!("  {}. [{stype}] {name}: {desc} — {fee_hint}\n", i + 1));
            }
            if let Some(desc) = &profile.profile_description {
                out.push_str(&format!("- Fallback reference — ASP description: {desc}\n"));
            }
        }
        out.push_str(&format!("- Task title: {}\n", task.title));
        out.push_str(&format!("- Task description: {}\n", task.description));
        out.push('\n');
        out.push_str("Match rules (**any single service** matching the task domain counts as a match; only when **all** services fail to match is it a mismatch):\n");
        out.push_str("- ✅ **Any one service** in the list matches the task domain → match; proceed to the visibility-based routing below\n");
        out.push_str("- ❌ Service list is empty / all services clearly mismatched with the task domain (e.g. all cat-image generation vs task is contract audit) → **must reject**:\n");
        out.push_str("  1. Call `xmtp_send` to send a rejection message (template below)\n");
        out.push_str("  2. **Do NOT** execute onchainos agent apply or any further operations\n\n");
        out.push_str("Rejection reply template (send via `xmtp_send`; `content` field = the plain natural-language body below):\n");
        let summary = if services.is_empty() {
            profile
                .profile_description
                .clone()
                .unwrap_or_else(|| "no services registered".to_string())
        } else {
            services
                .iter()
                .filter_map(|s| s.name.as_deref())
                .collect::<Vec<_>>()
                .join(" / ")
        };
        out.push_str(&format!(
            "Sorry, this task ({}) is outside my current service scope ({}); I cannot take it on. Best of luck finding the right ASP.\n\n",
            task.title, summary
        ));
        out.push_str("Note: `content` is plain natural-language body; do not add any text headers (e.g. `jobId: / from: ... / type: REPLY`). The XMTP plugin automatically wraps content into an a2a-agent-chat envelope.\n\n");

        // Once capability matching passes, branch by task.visibility to give different action guidance (VisibilityEnum: 0=PUBLIC / 1=PRIVATE).
        let buyer_id = task.buyer_agent_id.as_deref().unwrap_or("<task.buyerAgentId>");
        let agent_id_hint = profile.agent_id.as_deref().unwrap_or("<yourAgentId>");
        out.push_str("[⚠️ Step 2: Route by Visibility (only if match passed)]\n\n");
        if task.visibility == Some(0) {
            // Public task → ASP proactively creates the group + sends the cold-start opener (does not call next-action).
            out.push_str("Current task **visibility = Public** → you must **proactively contact the User Agent to initiate negotiation**:\n\n");
            out.push_str("1. Call `xmtp_start_conversation` to create a group + sub session (see skills/okx-agent-task/_shared/xmtp-tools.md → Path 7 `xmtp_start_conversation`):\n");
            out.push_str(&format!(
                "   - Args: `myAgentId={agent_id_hint}`, `toAgentId={buyer_id}` (User Agent's agentId), `jobId={}`\n",
                task.job_id
            ));
            out.push_str("   - Returns `sessionKey` (the new sub's key — use it directly in step 2; **do NOT call `session_status`** — during bootstrap it may return the current user session's key, which is wrong) + `xmtpGroupId`\n");
            out.push_str("2. **Send a cold-start opener via `xmtp_send`** (natural-language template; see `provider.md §2.1 end — \"how to negotiate after user selects\"`):\n");
            out.push_str(&format!(
                "   - Content: self-introduction + noticed the \"{}\" task + I can do it + ask the User Agent about budget / acceptance criteria / payment mode preference\n",
                task.title
            ));
            out.push_str("   - ❌ **Do NOT quote a specific price in the first message** (service-list registered fee / workload estimation judgment waits until the User Agent replies → then call next-action)\n");
            out.push_str("   - ❌ **Do NOT produce work content / fabricate protocol literals** (`[INTEREST]` / `[CONTACT_INIT]` etc. are hallucinations)\n");
            out.push_str("   - **This turn ends here**; wait for the User Agent's reply. Only **after** the User Agent replies call `onchainos agent next-action --jobid <jobId> --event job_created --role provider --agentId <agentId>` to get the negotiation playbook.\n\n");
            out.push_str("🛑 **Must use `xmtp_send`; do NOT substitute `xmtp_dispatch_session` / `xmtp_dispatch_user` / `xmtp_prompt_user`** — sending a2a-agent-chat business messages to a peer agent uses **only `xmtp_send`**. Even if the intent feels like \"establish negotiation channel / dispatch to sub\", **the only valid tool is `xmtp_send`**. `xmtp_dispatch_session` is exclusively for user→sub user-decision relay (a `source:\"system\"` envelope with `event:\"user_decision_<src>\"`) and does not match the a2a-agent-chat shape at all.\n\n");
        } else {
            // Private task → ASP passively waits for the User Agent to reach out first.
            out.push_str("Current task **visibility = Private** → **do NOT proactively create a group**:\n\n");
            out.push_str("- Private tasks are assigned by the User Agent; you must **wait for the User Agent to send first** a2a-agent-chat envelope (that is your entry point to contact them)\n");
            out.push_str("- After receiving the User Agent's first inquiry + passing capability match, **you must first call `onchainos agent next-action --jobid <jobId> --event job_created --role provider --agentId <agentId>` to get the first-round negotiation playbook**, then follow it to `xmtp_send` — do not compose negotiation content from this abbreviated version\n");
            out.push_str("- **Do NOT** call `xmtp_start_conversation` to create a group — private tasks do not have this permission\n\n");
        }

        // First-round negotiation hint (shared by public / private) — only semantic anti-patterns go here;
        // the actual three-step handshake + price judgment script is provided by next-action.
        out.push_str("📌 **First-round negotiation essence: you are there to 'ask + state your position', NOT 'self-confirm'**\n");
        out.push_str("- Task capability / acceptance criteria: can you do it? any follow-up questions?\n");
        out.push_str("- Price position: is the original price reasonable? If too low, **counter-offer** (state a new price + reason); do not mechanically accept\n");
        out.push_str("- paymentMode position: A2A negotiation path is fixed to escrow\n\n");
        out.push_str("❌ **Do NOT use self-confirm wording**: do not write in `xmtp_send` content things like \"I confirm the following three items / three items confirmed / I accept / I will apply immediately / I will submit the acceptance request\". The three items are to **ask** the User Agent; after sending, wait for the User Agent's `[intent:propose]` before the next handshake step — the specific three-step handshake playbook ([intent:propose] → [intent:ack] → [intent:confirm]) is provided by next-action; **you must not skip next-action and apply directly** (this has caused production incidents).\n\n");
    }

    // ── Next actions ─────────────────────────────────────────────────────
    let actions = available_actions(role, &status_str, &task.job_id);
    out.push_str("[Next Actions] (call next-action first to get the full playbook for the current status; follow the playbook — do not bypass next-action and call CLI directly)\n");
    for a in &actions {
        out.push_str(&format!("- {a}\n"));
    }
    out.push('\n');*/

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
