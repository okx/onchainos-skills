//! Common read-only query commands (shared by buyer / provider).
//!
//! status        — query a single task's status
//! list          — query the "my tasks" list for a single agent + role
//! active-tasks  — aggregated non-terminal tasks across all agents under the
//!                 current active account (with `myRole` / `counterpartyAgentId`
//!                 annotations; used by user-session to route ad-hoc user
//!                 instructions to a specific sub session via
//!                 `xmtp_sessions_query` → `xmtp_dispatch_session`)

use anyhow::Result;
use serde_json::{json, Value};

use super::network::task_api_client::TaskApiClient;
use super::DEBUG_LOG;
use crate::commands::agent_commerce::task::signing;

/// Resolves agentId from the local identity list by role when --agent-id is omitted.
/// When falling back, picks the first agent matching the role — may be wrong when
/// multiple agents of the same role exist (e.g. multiple providers).
pub async fn resolve_agent_id(agent_id: &str, role: i64) -> String {
    if !agent_id.is_empty() {
        return agent_id.to_string();
    }
    let resolved = signing::resolve_agent_id_by_role(role)
        .await
        .unwrap_or_default();
    if !resolved.is_empty() && DEBUG_LOG {
        eprintln!(
            "⚠ --agent-id omitted; falling back to first local agent with role={role}: {resolved}. \
             If you have multiple agents of this role, pass --agent-id explicitly."
        );
    }
    resolved
}

/// Query task status.
pub async fn handle_status(client: &mut TaskApiClient, job_id: &str, agent_id: &str, role: i64) -> Result<()> {
    let agent_id = resolve_agent_id(agent_id, role).await;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let t = &resp;
    let token_sym = t["tokenSymbol"].as_str().unwrap_or("?");
    println!("Task status: {}", t["status"].as_i64().map(status_name).unwrap_or("?"));
    println!("  jobId:    {job_id}");
    println!("  title:    {}", t["title"].as_str().unwrap_or("?"));
    println!("  budget:   {} {}", t["tokenAmount"].as_str().unwrap_or("?"), token_sym);
    println!("  buyer:    {}", t["buyerAgentId"].as_str().unwrap_or("?"));
    if let Some(pid) = t["providerAgentId"].as_str() {
        println!("  provider: {pid}");
    }
    Ok(())
}

/// Query the "my tasks" list.
pub async fn handle_list(
    client: &mut TaskApiClient,
    status: Option<&str>,
    page: u32,
    limit: u32,
    agent_id: &str,
    role: i64,
) -> Result<()> {
    let agent_id = resolve_agent_id(agent_id, role).await;
    let mut path = format!("/priapi/v1/aieco/task/my?page={page}&page_size={limit}");
    if let Some(s) = status { path.push_str(&format!("&status={s}")); }

    let resp = client.get_with_identity(&path, &agent_id).await?;
    let tasks = resp["list"].as_array().cloned().unwrap_or_default();
    let total = resp["total"].as_u64().unwrap_or(0);
    println!("Task list ({total} total, page {page}):");
    for t in &tasks {
        let sym = t["tokenSymbol"].as_str().unwrap_or("?");
        println!("  [{}] {} — {} {}",
            t["status"].as_i64().map(status_name).unwrap_or("?"),
            t["jobId"].as_str().unwrap_or("?"),
            t["tokenAmount"].as_str().unwrap_or("?"),
            sym,
        );
        println!("       {}", t["title"].as_str().unwrap_or("?"));
    }
    Ok(())
}

// ─── active-tasks ───────────────────────────────────────────────────────

pub fn status_name(code: i64) -> &'static str {
    match code {
        0 => "created",
        1 => "accepted",
        2 => "submitted",
        3 => "rejected",
        4 => "disputed",
        5 => "admin_stopped",
        6 => "complete",
        7 => "close",
        8 => "expired",
        9 => "failed",
        _ => "unknown",
    }
}

fn role_name(code: i64) -> &'static str {
    match code {
        1 => "buyer",
        2 => "provider",
        3 => "evaluator",
        _ => "unknown",
    }
}

/// Non-terminal statuses (per SKILL.md Critical Field Mapping Table):
/// 0 created / 1 accepted / 2 submitted / 3 rejected / 4 disputed.
/// Terminal (excluded by default): 5 admin_stopped / 6 complete / 7 close / 8 expired / 9 failed.
fn is_non_terminal(code: i64) -> bool {
    matches!(code, 0..=4)
}

fn short_job_id(jid: &str) -> String {
    if jid.len() < 12 {
        return jid.to_string();
    }
    format!("{}…{}", &jid[..6], &jid[jid.len() - 4..])
}

fn parse_role_arg(raw: &str) -> Option<i64> {
    match raw.trim().to_lowercase().as_str() {
        "buyer" | "requestor" | "1" => Some(1),
        "provider" | "seller" | "2" => Some(2),
        "evaluator" | "arbiter" | "3" => Some(3),
        _ => None,
    }
}

/// Aggregated non-terminal task list across all agents under the current active
/// account. Designed for the user-session "ad-hoc instruction → sub session"
/// routing flow:
///
///   1. user-session calls `agent active-tasks` (this command)
///   2. user-session renders the returned JSON to the user, lets the user pick a jobId
///   3. take `myAgentId` + `counterpartyAgentId` from the chosen row
///   4. `xmtp_sessions_query(myAgentId, toAgentId=counterpartyAgentId, jobId)` → sessionKey
///   5. `xmtp_dispatch_session(sessionKey, content=<user's verbatim instruction>)`
///
/// Output schema (via `output::success`):
///
/// ```jsonc
/// {
///   "totalAgents": 2,
///   "totalTasks": 3,
///   "tasks": [
///     {
///       "jobId":               "0xabc...",
///       "shortJobId":          "0xabc…1234",
///       "status":              "accepted",
///       "statusCode":          1,
///       "title":               "小猫图片",
///       "tokenAmount":         "1",
///       "tokenSymbol":         "USDT",
///       "myAgentId":           "796",
///       "myRole":              "buyer",
///       "counterpartyAgentId": "963",      // null when not yet designated (e.g. status=created with no provider)
///       "counterpartyRole":    "provider", // null in the evaluator case
///     }
///   ]
/// }
/// ```
pub async fn handle_active_tasks(
    client: &mut TaskApiClient,
    role_filter: Option<&str>,
    include_terminal: bool,
) -> Result<()> {
    use crate::commands::agent_commerce::task::common::fetch_my_agents;

    // 1. Get all agents under the current active account (already filtered by ownerAddress).
    let mut agents = fetch_my_agents().await;

    // Optional --role filter.
    if let Some(raw) = role_filter {
        let want = parse_role_arg(raw).ok_or_else(|| {
            anyhow::anyhow!(
                "unrecognized --role value: {raw:?} (expected buyer / provider / evaluator, or 1 / 2 / 3)"
            )
        })?;
        agents.retain(|a| a.get("role").and_then(|v| v.as_i64()) == Some(want));
    }

    // 2. For each agent, query `task/my` and aggregate.
    let mut all_tasks: Vec<Value> = Vec::new();
    for agent in &agents {
        let agent_id = agent.get("agentId").and_then(|v| v.as_str()).unwrap_or("");
        let role = agent.get("role").and_then(|v| v.as_i64()).unwrap_or(0);
        if agent_id.is_empty() {
            continue;
        }

        let path = "/priapi/v1/aieco/task/my?page=1&page_size=100";
        let resp = match client.get_with_identity(path, agent_id).await {
            Ok(r) => r,
            Err(e) => {
                if DEBUG_LOG { eprintln!("[active-tasks] agent {agent_id} query failed: {e}"); }
                continue;
            }
        };

        let tasks = resp["list"].as_array().cloned().unwrap_or_default();
        for t in tasks {
            let status_code = t.get("status").and_then(|v| v.as_i64()).unwrap_or(-1);
            if !include_terminal && !is_non_terminal(status_code) {
                continue;
            }

            let buyer_id = t.get("buyerAgentId").and_then(|v| v.as_str()).unwrap_or("");
            let provider_id = t.get("providerAgentId").and_then(|v| v.as_str()).unwrap_or("");

            // Counterparty inferred from my role:
            // - I'm buyer (1) → counterparty is provider
            // - I'm provider (2) → counterparty is buyer
            // - I'm evaluator (3) → no single counterparty (both buyer + provider are parties)
            let (counterparty_id, counterparty_role) = match role {
                1 => (provider_id, "provider"),
                2 => (buyer_id, "buyer"),
                _ => ("", ""),
            };

            let job_id = t.get("jobId").and_then(|v| v.as_str()).unwrap_or("");

            all_tasks.push(json!({
                "jobId":               job_id,
                "shortJobId":          short_job_id(job_id),
                "status":               status_name(status_code),
                "statusCode":           status_code,
                "title":                t.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "tokenAmount":          t.get("tokenAmount").and_then(|v| v.as_str()).unwrap_or(""),
                "tokenSymbol":          t.get("tokenSymbol").and_then(|v| v.as_str()).unwrap_or(""),
                "myAgentId":            agent_id,
                "myRole":               role_name(role),
                "counterpartyAgentId":  if counterparty_id.is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(counterparty_id.to_string())
                                        },
                "counterpartyRole":     if counterparty_role.is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(counterparty_role.to_string())
                                        },
            }));
        }
    }

    crate::output::success(json!({
        "totalAgents": agents.len(),
        "totalTasks":  all_tasks.len(),
        "tasks":       all_tasks,
    }));
    Ok(())
}
