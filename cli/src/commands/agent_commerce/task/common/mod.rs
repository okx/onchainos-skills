//! common — 任务系统通用查询命令
//!
//! 核心命令：`context`
//! 根据 job_id + 角色，从后端拉取任务详情，生成结构化自然语言上下文，
//! 供大模型（openclaw buyer/provider/evaluator AI）理解当前任务状态。

use anyhow::{bail, Result};
use clap::Subcommand;
use serde::Deserialize;

pub mod claim;
pub mod config;
pub mod dispute_upload;
pub mod network;
pub mod payment_mode;
pub mod pending;
pub mod query;
pub mod review_gate;
pub mod state_machine;
pub mod util;

use util::fmt_unix_secs;

use crate::commands::Context;

// ─── 链常量 ──────────────────────────────────────────────────────────────

/// XLayer chain ID（用于任务系统合约部署链）
pub const XLAYER_CHAIN_ID: i32 = 196;
/// XLayer chain index 字符串形式（用于钱包 API）
pub const XLAYER_CHAIN_INDEX: &str = "196";
/// XLayer chain name（用于 wallet_store 地址查找，wallets.json 中 chainIndex=196 的 chainName）
pub const XLAYER_CHAIN_NAME: &str = "okb";

// ─── Agent 角色常量（身份模块 API role 字段值）────────────────────────────

/// 买家 / 需求方（requestor）
pub const AGENT_ROLE_BUYER: i64 = 1;
/// 卖家 / 服务方（provider）
pub const AGENT_ROLE_PROVIDER: i64 = 2;
/// 仲裁者（evaluator）
pub const AGENT_ROLE_EVALUATOR: i64 = 3;

pub use payment_mode::PaymentMode;

pub use util::ensure_sufficient_balance;

// ─── CLI 定义 ──────────────────────────────────────────────────────────────
#[derive(Subcommand)]
pub enum CommonCommand {
    /// 查询任务上下文，输出供大模型使用的结构化自然语言描述
    ///
    /// 示例：
    ///   onchainos agent context task-001 --role buyer --agent-id 426
    ///   onchainos agent context task-001 --role provider --agent-id 558
    Context {
        /// 任务 ID（jobId），如 task-001 或 0x1a2b...
        job_id: String,

        /// 调用者角色：buyer | provider | evaluator
        #[arg(long, default_value = "buyer")]
        role: String,

        /// 调用者的 AgentID（**必填**）。beta 后端要求 agenticId header 非空，
        /// 一个钱包可能有多个 provider agent，调用方必须显式选定，CLI 不自动挑。
        /// 钱包地址 / 通信地址会通过 `agent get --agent-ids <agent_id>` 自动反查，
        /// 无需 CLI 传入。
        #[arg(long)]
        agent_id: String,
    },
}

// ─── 任务详情响应结构 ──────────────────────────────────────────────────────
// 字段对齐后端 spec：/priapi/v1/aieco/task/{jobId} 响应 data 字段（平铺）。

/// 对齐 spec：/priapi/v1/aieco/task/{jobId} 响应 data 字段
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskDetail {
    job_id: String,
    task_id: Option<i64>,
    title: String,
    description: String,
    content_hash: Option<String>,
    token_address: Option<String>,
    /// 后端 spec：直接返回的代币符号（USDT / USDG）。
    token_symbol: Option<String>,
    token_amount: Option<String>,
    /// 0=未设置 / 1=escrow / 3=x402
    payment_mode: Option<i32>,
    /// 后端 VisibilityEnum：0=PUBLIC（公开） / 1=PRIVATE（私有）
    visibility: Option<i32>,
    /// 0=open / 1=accepted / 2=submitted / 3=refused / 4=disputed / 5=complete / 7=close
    status: Option<i32>,
    sensitive_status: Option<i32>,
    category_codes: Option<Vec<String>>,
    chain_id: Option<i32>,
    min_credit_score: Option<f64>,
    designated_provider: Option<String>,
    buyer_agent_address: Option<String>,
    buyer_agent_id: Option<String>,
    provider_agent_address: Option<String>,
    provider_agent_id: Option<String>,
    group_id: Option<String>,
    expire_config: Option<serde_json::Value>,
    /// unix 秒；0 表示未设置
    expire_time: Option<i64>,
    payment_most_token_amount: Option<String>,
    create_time: Option<i64>,
    update_time: Option<i64>,
}

// ─── Agent 资料响应结构 ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    #[allow(dead_code)]
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub profile_description: Option<String>,
    /// 钱包地址（owner / 部署该 agent 的 EOA）
    pub agent_wallet_address: Option<String>,
    /// XMTP 通信地址（agent 之间 P2P 通讯用）
    pub communication_address: Option<String>,
}

/// 查询指定 agentId 的 agent 资料（name / profileDescription / 钱包地址 / 通信地址）。
///
/// 直接 spawn `onchainos agent get --agent-ids <id>` 子进程 + parse stdout——
/// 不复刻 token / wallet client / URL 拼装逻辑，`agent get` 实现以后改了这里自动跟上。
/// 任何错误路径都回退到带 agentId 的占位符（地址字段为 None），保证返回值非空。
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
            eprintln!("[fetch_agent_profile] current_exe 失败: {e}; fallback");
            return fallback();
        }
    };

    // 子进程会继承父进程 env（含 OKX_BASE_URL），跟父进程打的 URL 完全一致。
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["agent", "get", "--agent-ids", agent_id]);
    let output = match cmd.output().await
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[fetch_agent_profile] spawn `agent get` 失败: {e}; fallback");
            return fallback();
        }
    };

    let body: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[fetch_agent_profile] 解析 `agent get` stdout 失败: {e}; raw={}; fallback",
                String::from_utf8_lossy(&output.stdout)
            );
            return fallback();
        }
    };

    // `agent get` 的输出形状由 output::success 包装：{ ok: true, data: <value> }
    // 失败时是 { ok: false, error: "..." }
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error message)");
        eprintln!("[fetch_agent_profile] `agent get` 返回失败: {err}; fallback");
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

/// 卖家自我能力匹配的真相来源：service-list（agent 主动注册的服务列表）。
#[derive(Debug, Default)]
struct AgentService {
    name: Option<String>,
    description: Option<String>,
    service_type: Option<String>,
    /// 该服务的注册费用（字符串形式，单位通常 USDT）。
    /// 空字符串 / "0" / "0.0" 视为未设置——provider 应基于任务工作量定价；
    /// 非零正值视为该服务的标准价，provider 协商时以此为锚。
    fee: Option<String>,
}

/// 子进程调 `onchainos agent service-list --agent-id <id>` 拿服务列表。
/// 失败 / 空列表都回 vec![]，调用方按空处理。
async fn fetch_agent_services(agent_id: &str) -> Vec<AgentService> {
    if agent_id.is_empty() {
        return vec![];
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[fetch_agent_services] current_exe 失败: {e}");
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
            eprintln!("[fetch_agent_services] spawn `agent service-list` 失败: {e}");
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
            eprintln!("[fetch_agent_services] 解析 stdout 失败: {e}");
            return vec![];
        }
    };
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("(no error)");
        eprintln!("[fetch_agent_services] CLI 返回失败: {err}");
        return vec![];
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    eprintln!(
        "[fetch_agent_services] body.data 解析前: {}",
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
            "[fetch_agent_services] data[0].list 字段缺失，shape 异常 (agentId={agent_id}) — data 完整内容见上一行 body.data 输出"
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

// ─── 状态说明 ──────────────────────────────────────────────────────────────
fn status_desc(s: &str) -> &str {
    match s {
        "init"      => "初始化中（等待上链确认）",
        "created"   => "等待接单（Created）",
        "accepted"  => "已接单，卖家执行中（Accepted）",
        "submitted" => "卖家已提交交付，等待买家验收（Submitted）",
        "refused"   => "买家拒绝验收，冻结期内可申请仲裁（Refused）",
        "disputed"      => "仲裁进行中（Disputed）",
        "admin_stopped" => "管理员已停止任务（AdminStopped）",
        "completed" | "complete" => "任务已完成，款项已释放（Complete）",
        "rejected"  => "仲裁结束，任务关闭（Rejected）",
        "close"     => "买家主动关闭（Close）",
        "expired"   => "任务已过期（Expired）",
        _           => "未知状态",
    }
}

fn payment_mode_desc(pm: i32) -> &'static str {
    PaymentMode::from_int(pm).desc()
}

/// 根据角色 + 任务状态，列出当前可执行的 CLI 操作
/// 按 role 路由到对应 flow.rs 的 `available_actions`，
/// single source of truth 留在 buyer/provider/evaluator 各自模块。
fn available_actions(role: &str, status: &str, job_id: &str) -> Vec<String> {
    use state_machine::{Role, Status};
    let status = Status::parse(status);
    match Role::parse(role) {
        Some(Role::Buyer)     => super::buyer::flow::available_actions(&status, job_id),
        Some(Role::Provider)  => super::provider::flow::available_actions(&status, job_id),
        Some(Role::Evaluator) => super::evaluator::flow::available_actions(&status, job_id),
        None => vec![
            format!("onchainos agent status {job_id}         # 查询最新任务状态"),
        ],
    }
}

// ─── 命令处理 ──────────────────────────────────────────────────────────────

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
    // 校验角色
    if !["buyer", "provider", "evaluator"].contains(&role) {
        bail!("--role 必须是 buyer / provider / evaluator");
    }
    if agent_id.is_empty() {
        bail!("--agent-id 必填：beta 后端要求 agenticId header 非空");
    }

    // 调用后端获取任务详情。base url 由 TaskApiClient::new 内部按
    // OKX_BASE_URL env > TASK_BASE_URL env > 常量 兜底解析，无需 CLI 显式指定。
    let mut client = network::task_api_client::TaskApiClient::new();
    let resp_val = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("无法获取任务详情: {e}"))?;

    // 后端 spec：响应 data 直接是平铺的 task 对象（WalletApiClient 已剥掉 body["data"]）
    let task: TaskDetail = serde_json::from_value(resp_val)
        .map_err(|e| anyhow::anyhow!("解析响应失败: {e}"))?;

    // 拉自己 agent 的资料：name / profileDescription / agentWalletAddress / communicationAddress
    // 三种角色都需要——【你的身份】块要展示钱包地址 + 通信地址；provider 还会用 description 做专业匹配。
    // fetch 出错时返回带 agentId 的 fallback，永不为空。
    let profile = fetch_agent_profile(agent_id).await;

    // 生成上下文
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
        Some(state_machine::Role::Buyer)     => "买家（Client）",
        Some(state_machine::Role::Provider)  => "卖家（Provider）",
        Some(state_machine::Role::Evaluator) => "仲裁者（Evaluator）",
        None                                 => role,
    };

    // spec 只回 status 整数，本地用 Status::from_int 派生枚举；展示串走 as_str()。
    let task_status = task
        .status
        .map(state_machine::Status::from_int)
        .unwrap_or_else(|| state_machine::Status::Other("unknown".to_string()));
    let status_str = task_status.as_str().to_string();
    let status_text = format!("{status_str} — {}", status_desc(&status_str));

    // ── 角色声明 ──────────────────────────────────────────────────────────
    out.push_str(&format!("你是任务系统中的{role_cn}。\n\n"));

    // ── 身份信息 ──────────────────────────────────────────────────────────
    // 钱包地址 / 通信地址来自 `agent get` 反查（fetch_agent_profile）；任务详情里的
    // buyerAgentAddress / providerAgentAddress 仍用于下方【买家信息】/【卖家信息】块。
    out.push_str("【你的身份】\n");
    out.push_str(&format!("- 角色：{role_cn}\n"));
    out.push_str(&format!("- AgentID：{agent_id}\n"));
    if let Some(w) = &profile.agent_wallet_address {
        out.push_str(&format!("- 钱包地址：{w}\n"));
    }
    if let Some(c) = &profile.communication_address {
        out.push_str(&format!("- 通信地址：{c}\n"));
    }
    if let Some(n) = &profile.name {
        out.push_str(&format!("- 名称：{n}\n"));
    }
    if let Some(d) = &profile.profile_description {
        out.push_str(&format!("- 描述：{d}\n"));
    }
    out.push('\n');

    // ── 任务详情 ──────────────────────────────────────────────────────────
    out.push_str("【任务详情】\n");
    out.push_str(&format!("- 任务ID：{}\n", task.job_id));
    if let Some(tid) = task.task_id {
        out.push_str(&format!("- 内部ID：{tid}\n"));
    }
    out.push_str(&format!("- 标题：{}\n", task.title));
    out.push_str(&format!("- 描述：{}\n", task.description));

    let amount = task.token_amount.as_deref().unwrap_or("未设置");
    let token  = task.token_address.as_deref().unwrap_or("");
    let symbol = task.token_symbol.as_deref().unwrap_or("UNKNOWN");
    out.push_str(&format!("- 创建预算：{amount} {symbol} （token: {token}）\n"));
    if let Some(max_amt) = &task.payment_most_token_amount {
        out.push_str(&format!("- 最高预算（paymentMostTokenAmount）：{max_amt} {symbol}\n"));
    }

    let pm = task.payment_mode.unwrap_or(0);
    out.push_str(&format!(
        "- 支付方式（paymentType={}）：{}\n",
        pm,
        payment_mode_desc(pm)
    ));
    let visibility = match task.visibility {
        Some(0) => "公开（Public）",
        Some(1) => "私有（Private）",
        _       => "未知",
    };
    out.push_str(&format!("- 可见性：{visibility}\n"));
    if let Some(chain) = task.chain_id {
        out.push_str(&format!("- 链：chainId={chain}\n"));
    }
    if let Some(score) = task.min_credit_score {
        out.push_str(&format!("- 最低信用分要求：{score}\n"));
    }
    if let Some(dp) = &task.designated_provider {
        out.push_str(&format!("- 指定卖家：{dp}\n"));
    }
    if let Some(ec) = &task.expire_config {
        if let (Some(open_sec), Some(acc_sec)) = (
            ec.get("openExpireSec").and_then(|v| v.as_u64()),
            ec.get("acceptedExpireSec").and_then(|v| v.as_u64()),
        ) {
            out.push_str(&format!(
                "- 有效期：接单时限 {}h，交付时限 {}h\n",
                open_sec / 3600,
                acc_sec / 3600
            ));
        }
    }
    out.push_str(&format!("- 创建时间：{}\n", fmt_unix_secs(task.create_time)));
    out.push_str(&format!("- 更新时间：{}\n", fmt_unix_secs(task.update_time)));
    out.push('\n');

    // ── 当前状态 ──────────────────────────────────────────────────────────
    out.push_str("【当前状态】\n");
    out.push_str(&format!("- {status_text}\n"));
    out.push('\n');

    // ── 买家信息 ──────────────────────────────────────────────────────────
    out.push_str("【买家信息】\n");
    match (&task.buyer_agent_id, &task.buyer_agent_address) {
        (Some(id), Some(addr)) => {
            out.push_str(&format!("- AgentID：{id}\n"));
            out.push_str(&format!("- 通信地址：{addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID：{id}\n")),
        _ => out.push_str("- 信息未知\n"),
    }
    out.push('\n');

    // ── 卖家信息 ──────────────────────────────────────────────────────────
    out.push_str("【卖家信息】\n");
    match (&task.provider_agent_id, &task.provider_agent_address) {
        (Some(id), Some(addr)) => {
            out.push_str(&format!("- AgentID：{id}\n"));
            out.push_str(&format!("- 通信地址：{addr}\n"));
        }
        (Some(id), None) => out.push_str(&format!("- AgentID：{id}\n")),
        _ => out.push_str("- 尚未匹配卖家\n"),
    }
    // ── 专业匹配检查（仅卖家 + created 状态） ────────────────────────────────
    // 真相来源：service-list（agent 注册的服务清单）。**只要任意一项服务**和任务领域
    // 匹配就算通过；只有**全部**服务都对不上才判定为不匹配。profileDescription 仅做
    // 兜底参考，不作为唯一判断依据（描述是泛泛的自我介绍，service-list 才是实际能力）。
    if role_enum == Some(state_machine::Role::Provider)
        && task_status == state_machine::Status::Created
    {
        let services = fetch_agent_services(profile.agent_id.as_deref().unwrap_or("")).await;
        out.push_str("【⚠️ 第一步：专业匹配检查（必做，不得跳过）】\n");
        if services.is_empty() {
            out.push_str("- 你的服务列表（service-list）：**空** —— 没有注册任何服务\n");
            if let Some(desc) = &profile.profile_description {
                out.push_str(&format!("- 备用参考·Provider 描述：{desc}\n"));
            }
        } else {
            out.push_str("- 你的服务列表（service-list，**专业匹配 + 报价锚的真相来源**）：\n");
            for (i, svc) in services.iter().enumerate() {
                let name = svc.name.as_deref().unwrap_or("(no name)");
                let desc = svc.description.as_deref().unwrap_or("(no description)");
                let stype = svc.service_type.as_deref().unwrap_or("?");
                // fee 字段:非零正值显示「注册价 X USDT」给协商锚;未设置/0/空 显示「未设置」让 agent 按工作量估
                let fee_hint = match nonzero_fee(&svc.fee) {
                    Some(f) => format!("注册价 {f} USDT(协商以此为锚)"),
                    None => "注册价未设置(按工作量估,不要瞎要价)".to_string(),
                };
                out.push_str(&format!("  {}. [{stype}] {name}: {desc} — {fee_hint}\n", i + 1));
            }
            if let Some(desc) = &profile.profile_description {
                out.push_str(&format!("- 备用参考·Provider 描述：{desc}\n"));
            }
        }
        out.push_str(&format!("- 任务标题：{}\n", task.title));
        out.push_str(&format!("- 任务描述：{}\n", task.description));
        out.push('\n');
        out.push_str("判断规则（**只要任意一项服务**和任务领域吻合就算匹配；只有**所有**服务都对不上才判定不匹配）：\n");
        out.push_str("- ✅ 服务列表里**任意一项**和任务领域吻合 → 匹配，进入下方「按可见性分流」继续协商\n");
        out.push_str("- ❌ 服务列表为空 / 所有服务都和任务领域明显不符（如全是猫图生成 vs 任务是合约审计）→ **必须拒绝**：\n");
        out.push_str("  1. 调用 `xmtp_send` 工具发送拒绝消息（模板如下）\n");
        out.push_str("  2. **禁止**执行 onchainos agent apply 或任何后续操作\n\n");
        out.push_str("拒绝回复模板（通过 `xmtp_send` 工具发送，`content` 字段 = 下方纯自然语言正文）：\n");
        let summary = if services.is_empty() {
            profile
                .profile_description
                .clone()
                .unwrap_or_else(|| "未注册任何服务".to_string())
        } else {
            services
                .iter()
                .filter_map(|s| s.name.as_deref())
                .collect::<Vec<_>>()
                .join(" / ")
        };
        out.push_str(&format!(
            "抱歉，此任务（{}）不在我目前提供的服务范围（{}）内，无法承接。祝您找到合适的卖家。\n\n",
            task.title, summary
        ));
        out.push_str("注意：`content` 是纯自然语言正文，不要加任何 text header（如 `jobId: / 来自: ... / 类型: REPLY` 之类）。XMTP 插件会自动把 content 包装成 a2a-agent-chat envelope。\n\n");

        // 专业匹配通过后，按 task.visibility 给不同动作引导（VisibilityEnum: 0=PUBLIC / 1=PRIVATE）
        let buyer_id = task.buyer_agent_id.as_deref().unwrap_or("<task.buyerAgentId>");
        let agent_id_hint = profile.agent_id.as_deref().unwrap_or("<你的agentId>");
        out.push_str("【⚠️ 第二步：按可见性分流（匹配通过才走这里）】\n\n");
        if task.visibility == Some(0) {
            // 公开任务 → provider 主动建群 + 发冷启动开场白(不调 next-action)
            out.push_str("当前任务**可见性 = 公开（Public）** → 你需要**主动联系买家发起协商**：\n\n");
            out.push_str("1. 调 `xmtp_start_conversation` 工具建群 + 创建 sub session（机制见 skills/okx-agent-task/SKILL.md Session 通信契约 4.7）：\n");
            out.push_str(&format!(
                "   - 参数：`myAgentId={agent_id_hint}`，`toAgentId={buyer_id}`（买家 agentId），`jobId={}`\n",
                task.job_id
            ));
            out.push_str("   - 成功返回 `sessionKey`（新 sub 的 key，下面 step 2 直接用，**不要再调 `session_status`**——bootstrap 阶段 `session_status` 可能返回当前所在 user session 的 key，会拿错）+ `xmtpGroupId`\n");
            out.push_str("2. **直接 `xmtp_send` 一条冷启动开场白**（自然语言模板，详见 `provider.md §2.1 末尾「用户选定后怎么协商」`）：\n");
            out.push_str(&format!(
                "   - 内容只是：自我介绍 + 看到了「{}」任务 + 我能做 + 问买家预算 / 验收标准 / 支付方式偏好\n",
                task.title
            ));
            out.push_str("   - ❌ **首条禁止报具体价格**（service-list 注册价 / 工作量估算的判断等买家回信后再走 next-action）\n");
            out.push_str("   - ❌ **首条禁止产工作内容 / 杜撰协议字面量**（`[INTEREST]` / `[CONTACT_INIT]` 等都是幻觉）\n");
            out.push_str("   - **本 turn 在这里结束**，等买家回信。买家回信后**才**调 `onchainos agent next-action --jobid <jobId> --jobStatus job_created --role provider --agentId <agentId>` 拿协商剧本。\n\n");
            out.push_str("🛑 **必须用 `xmtp_send`，禁止用 `xmtp_dispatch_session` / `xmtp_dispatch_user` / `xmtp_prompt_user` 替代**——给 peer agent 发 a2a-agent-chat 业务消息**只有 `xmtp_send` 一种路径**。看到「建立协商通道 / 派发到 sub / dispatch」这种语感**也只能选 `xmtp_send`**。`xmtp_dispatch_session` 是 user→sub `[USER_DECISION_RELAY]` 决策回传专用，跟协商首条 a2a-agent-chat 形态完全不符。\n\n");
        } else {
            // 私有任务 → provider 被动等买家先来
            out.push_str("当前任务**可见性 = 私有（Private）** → 你**不要主动建群**：\n\n");
            out.push_str("- 私有任务由买家选定 provider，必须**等买家先发** a2a-agent-chat envelope（你才有联系对方的入口）\n");
            out.push_str("- 收到买家首条 inquire + 专业匹配通过后，**必须先调 `onchainos agent next-action --jobid <jobId> --jobStatus job_created --role provider --agentId <agentId>` 拿协商首回合剧本**，再按剧本输出去 `xmtp_send`——不要凭这里简版自己拼协商内容\n");
            out.push_str("- **禁止**调 `xmtp_start_conversation` 主动建群——私有任务没有这个权限\n\n");
        }

        // 协商首回合提示（公开 / 私有共用）—— 这里只放语义性反提示，
        // 具体三步握手 + 报价主观判断剧本由 next-action 提供。
        out.push_str("📌 **协商首回合本质：你是去『问 + 表态』，不是『自我确认』**\n");
        out.push_str("- 任务能力 / 验收标准：能不能做、有没有补充问题\n");
        out.push_str("- 价格立场：原价是否合理；偏低就**还价**（明确报新价 + 理由），不要机械接受\n");
        out.push_str("- paymentMode 立场：A2A 协商路径固定 escrow（担保）\n\n");
        out.push_str("❌ **禁止自我 confirm 措辞**：不要在 `xmtp_send` content 里写「我确认以下三项 / 三项确认完毕 / 我接受 / 我将立即 apply / 我将提交接单申请」。三项是要**问**买家的，发完等 buyer 的 `[intent:propose]` 才进下一步握手——具体三步握手剧本（[intent:propose] → [intent:ack] → [intent:confirm]）由 next-action 给出，**这里不能跳过 next-action 直接 apply**（已发生过线上事故）。\n\n");
    }

    // ── 下一步动作 ────────────────────────────────────────────────────────
    let actions = available_actions(role, &status_str, &task.job_id);
    out.push_str("【下一步动作】（先调 next-action 拿当前 status 的完整剧本，按剧本走，不要绕过 next-action 直接调 CLI）\n");
    for a in &actions {
        out.push_str(&format!("- {a}\n"));
    }
    out.push('\n');

    // ── 必须加载的角色指南 ──────────────────────────────────────────────
    let skill_file = match role {
        "buyer"     => "client.md",
        "provider"    => "provider.md",
        "evaluator" => "evaluator.md",
        _           => "",
    };
    if !skill_file.is_empty() {
        out.push_str("【⚠️ 必须立即执行】\n");
        out.push_str(&format!(
            "请立即读取角色指南 skills/okx-agent-task/{skill_file}（与 skills/okx-agent-task/SKILL.md 同目录），该文件包含完整的协商规则和接单流程。\n"
        ));
    }

    out
}
