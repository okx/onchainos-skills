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
pub mod dispute_upload;
pub mod network;
pub mod payment_mode;
pub mod pending;
pub mod pending_v2;
pub mod query;
pub mod review_gate;
pub mod state_machine;
pub mod util;
pub mod version_notice;

use util::fmt_unix_secs;

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

pub use util::ensure_sufficient_balance;

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
    /// 0=open / 1=accepted / 2=submitted / 3=refused / 4=disputed / 5=complete / 7=close
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
    update_time: Option<i64>,
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
            eprintln!("[fetch_agent_profile] current_exe failed: {e}; fallback");
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
            eprintln!("[fetch_agent_profile] spawn `agent get` failed: {e}; fallback");
            return fallback();
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[fetch_agent_profile] parse `agent get` stdout failed: {e}; raw={}; fallback",
                String::from_utf8_lossy(&output.stdout)
            );
            return fallback();
        }
    };

    // The output shape of `agent get` is wrapped by output::success: { ok: true, data: <value> }
    // On failure it is { ok: false, error: "..." }.
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        eprintln!("[fetch_agent_profile] `agent get` returned failure: {err}; fallback");
        return fallback();
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);

    // Flatten the response (new shape: `list[].agentList[]` groups; old shape:
    // `list[]` flat agents). No ownerAddress filter — we're looking up any
    // agent by id, possibly belonging to another user (e.g. peer buyer profile).
    let all_agents = flatten_agent_groups(&data);
    if all_agents.is_empty() {
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
    if !all_agents.is_empty() && matched.is_none() {
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
            eprintln!("[fetch_agent_services] current_exe failed: {e}");
            return vec![];
        }
    };
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "service-list", "--agent-id", agent_id]);
    eprintln!(
        "[fetch_agent_services] running: {} agent service-list --agent-id {agent_id}",
        exe.display()
    );
    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[fetch_agent_services] spawn `agent service-list` failed: {e}");
            return vec![];
        }
    };
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);
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
    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[fetch_agent_services] parse stdout failed: {e}");
            return vec![];
        }
    };
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        eprintln!("[fetch_agent_services] CLI returned failure: {err}");
        return vec![];
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    eprintln!(
        "[fetch_agent_services] body.data before parsing: {}",
        serde_json::to_string_pretty(&data).unwrap_or_else(|_| "<unprintable>".to_string())
    );
    let list = data
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|x| x.get("list"))
        .and_then(|v| v.as_array())
        .cloned();
    let Some(list) = list else {
        eprintln!(
            "[fetch_agent_services] data[0].list missing; shape anomaly (agentId={agent_id}) — full data content in the previous body.data log line"
        );
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
        eprintln!("[fetch_my_agents] no current XLayer address; returning empty");
        return Vec::new();
    };

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[fetch_my_agents] current_exe failed: {e}");
            return Vec::new();
        }
    };

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get"]);
    eprintln!(
        "[fetch_my_agents] running: {} agent get (filter ownerAddress={my_owner})",
        exe.display()
    );

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[fetch_my_agents] spawn `agent get` failed: {e}");
            return Vec::new();
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[fetch_my_agents] parse stdout failed: {e}; raw={}",
                String::from_utf8_lossy(&output.stdout)
            );
            return Vec::new();
        }
    };

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        eprintln!("[fetch_my_agents] `agent get` returned failure: {err}");
        return Vec::new();
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let agents = flatten_my_agents(&data, &my_owner);
    eprintln!(
        "[fetch_my_agents] matched {} agents under ownerAddress={my_owner}",
        agents.len()
    );
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
        eprintln!("[fetch_my_agent_by_id] empty agent_id; returning None");
        return None;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[fetch_my_agent_by_id] current_exe failed: {e}");
            return None;
        }
    };

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get"]);
    eprintln!(
        "[fetch_my_agent_by_id] running: {} agent get (filter agentId={id})",
        exe.display()
    );

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[fetch_my_agent_by_id] spawn `agent get` failed: {e}");
            return None;
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[fetch_my_agent_by_id] parse stdout failed: {e}; raw={}",
                String::from_utf8_lossy(&output.stdout)
            );
            return None;
        }
    };

    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        eprintln!("[fetch_my_agent_by_id] `agent get` returned failure: {err}");
        return None;
    }

    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let hit = flatten_agent_groups(&data)
        .into_iter()
        .find(|a| a.get("agentId").and_then(|v| v.as_str()) == Some(id));
    eprintln!(
        "[fetch_my_agent_by_id] {} for agentId={id}",
        if hit.is_some() { "matched" } else { "no match" }
    );
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

/// `onchainos agent profile <agent_id>` — look up a single agent by id and
/// return its flat JSON profile. Works for **any** agent (current account or
/// peer), used to verify peer / designated-provider identities.
///
/// Internally calls `agent get --agent-ids <id>` then walks the response via
/// `flatten_agent_groups` to find the matching agent and prints it as the
/// `data` payload. Errors when agentId is empty, the subprocess fails, the
/// response shape is broken, or no agent matches the queried id.
pub async fn handle_profile(agent_id: &str) -> Result<()> {
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
    let matched = all.into_iter().find(|a| {
        a.get("agentId").and_then(|v| v.as_str()) == Some(id)
    });

    match matched {
        Some(agent) => {
            crate::output::success(agent);
            Ok(())
        }
        None => bail!("agentId={id} not found in `agent get` response"),
    }
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
        eprintln!(
            "[flatten_agent_groups] response missing `list` field (tried both shapes); raw data: {}",
            serde_json::to_string(data).unwrap_or_default()
        );
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
        "refused"   => "User Agent rejected deliverable; arbitration possible within freeze period (Refused)",
        "disputed"      => "Arbitration in progress (Disputed)",
        "admin_stopped" => "Admin stopped the task (AdminStopped)",
        "completed" | "complete" => "Task completed; funds released (Complete)",
        "rejected"  => "Arbitration concluded; task closed (Rejected)",
        "close"     => "User Agent closed the task (Close)",
        "expired"   => "Task expired (Expired)",
        _           => "Unknown status",
    }
}

fn payment_mode_desc(pm: i32) -> &'static str {
    PaymentMode::from_int(pm).desc()
}

/// Given the role + task status, list the CLI actions currently available.
/// Routes by role into the corresponding `available_actions` in each flow.rs;
/// the single source of truth lives in the buyer/provider/evaluator modules.
fn available_actions(role: &str, status: &str, job_id: &str) -> Vec<String> {
    use state_machine::{Role, Status};
    let status = Status::parse(status);
    match Role::parse(role) {
        Some(Role::Buyer)     => super::buyer::flow::available_actions(&status, job_id),
        Some(Role::Provider)  => super::provider::flow::available_actions(&status, job_id),
        Some(Role::Evaluator) => super::evaluator::flow::available_actions(&status, job_id),
        None => vec![
            format!("onchainos agent status {job_id}         # query latest task status"),
        ],
    }
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
                "- Expiry: accept deadline {}h, delivery deadline {}h\n",
                open_sec / 3600,
                acc_sec / 3600
            ));
        }
    }
    out.push_str(&format!("- Created: {}\n", fmt_unix_secs(task.create_time)));
    out.push_str(&format!("- Updated: {}\n", fmt_unix_secs(task.update_time)));
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
    if role_enum == Some(state_machine::Role::Provider)
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
            out.push_str("1. Call `xmtp_start_conversation` to create a group + sub session (see skills/okx-agent-task/SKILL.md Session Communication Contract §4.7):\n");
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
            out.push_str("   - **This turn ends here**; wait for the User Agent's reply. Only **after** the User Agent replies call `onchainos agent next-action --jobid <jobId> --jobStatus job_created --role provider --agentId <agentId>` to get the negotiation playbook.\n\n");
            out.push_str("🛑 **Must use `xmtp_send`; do NOT substitute `xmtp_dispatch_session` / `xmtp_dispatch_user` / `xmtp_prompt_user`** — sending a2a-agent-chat business messages to a peer agent uses **only `xmtp_send`**. Even if the intent feels like \"establish negotiation channel / dispatch to sub\", **the only valid tool is `xmtp_send`**. `xmtp_dispatch_session` is exclusively for user→sub `[USER_DECISION_RELAY]` decision relay and does not match the a2a-agent-chat shape at all.\n\n");
        } else {
            // Private task → ASP passively waits for the User Agent to reach out first.
            out.push_str("Current task **visibility = Private** → **do NOT proactively create a group**:\n\n");
            out.push_str("- Private tasks are assigned by the User Agent; you must **wait for the User Agent to send first** a2a-agent-chat envelope (that is your entry point to contact them)\n");
            out.push_str("- After receiving the User Agent's first inquiry + passing capability match, **you must first call `onchainos agent next-action --jobid <jobId> --jobStatus job_created --role provider --agentId <agentId>` to get the first-round negotiation playbook**, then follow it to `xmtp_send` — do not compose negotiation content from this abbreviated version\n");
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
    out.push('\n');

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
