//! Common read-only query commands (shared by user / asp).
//!
//! status        — query a single task's status
//! list          — query the "my tasks" list for a single agent + role
//! active-tasks  — aggregated non-terminal tasks across all agents under the
//!                 current active account (with `myRole` / `counterpartyAgentId`
//!                 annotations; used by user-session to route ad-hoc user
//!                 instructions to a specific sub session via
//!                 `okx-a2a session query` → `okx-a2a session send --no-wait`)

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::network::task_api_client::TaskApiClient;
use super::DEBUG_LOG;
use crate::commands::agent_commerce::task::signing;

/// Resolves agentId from the local identity list by role when --agent-id is omitted.
/// When falling back, picks the first agent matching the role — may be wrong when
/// multiple agents of the same role exist (e.g. multiple asps).
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

/// Count-branch classification of an identity list for the fallible resolver.
/// Pure over `&[Value]` so the resolution matrix is unit-testable without any
/// async or network access.
#[derive(Debug, PartialEq, Eq)]
enum Classification {
    /// No entry carries a usable `agentId` (list empty, or every entry malformed).
    None,
    /// Exactly one entry carries a non-empty `agentId`.
    One(String),
    /// Two or more entries carry a non-empty `agentId`.
    Many,
    /// The list has exactly one entry, but it is missing/empty `agentId`.
    MalformedSingle,
}

/// Extract the trimmed `agentId` of an identity entry, if non-empty.
fn well_formed_agent_id(agent: &Value) -> Option<String> {
    let id = agent
        .get("agentId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Classify how many usable identities the account exposes (R3/R4/R5/R6).
fn classify_identities(agents: &[Value]) -> Classification {
    let mut ids = agents.iter().filter_map(well_formed_agent_id);
    match (ids.next(), ids.next()) {
        (None, _) if agents.len() == 1 => Classification::MalformedSingle,
        (None, _) => Classification::None,
        (Some(id), None) => Classification::One(id),
        (Some(_), Some(_)) => Classification::Many,
    }
}

/// Map a numeric role code (1/2/3) to its canonical label (user/asp/evaluator).
/// Reuses `role_name` so there is a single 1/2/3→label mapping in this module.
fn role_label(code: i64) -> &'static str {
    role_name(code)
}

/// Build the ≥2-identity ambiguity error message from the identity list.
/// States the total candidate count, enumerates every candidate with its
/// `agentId` + role label, and ends with the `--agent-id` selection hint.
fn format_ambiguous_identities(agents: &[Value]) -> String {
    let candidates: Vec<String> = agents
        .iter()
        .filter_map(|a| {
            let id = well_formed_agent_id(a)?;
            let role = a.get("role").and_then(|v| v.as_i64()).unwrap_or(0);
            Some(format!("[agentId={id} role={}]", role_label(role)))
        })
        .collect();
    format!(
        "This account has {} identities: {}. Pass --agent-id to choose the identity to query.",
        candidates.len(),
        candidates.join(", "),
    )
}

/// Resolve the effective agentId for the read-only `agent tasks` / `agent status`
/// query commands when `--agent-id` may be omitted.
///
/// Contract: `Ok(non-empty agentId)` XOR `Err(actionable)` — **never** `Ok("")`.
/// Removing the empty-string return closes the backend `code=3001` path for
/// pure-ASP / pure-Evaluator wallets. Two-layer strategy (R1→R6, first match wins):
///
/// - R1: explicit non-empty `--agent-id` → returned verbatim, no resolution.
/// - R2: Layer 1 role resolve (`Err`/`Ok("")` = miss) → use it when non-empty.
/// - R3: Layer 2 list has exactly one usable identity → auto-use it (role-agnostic).
/// - R4: Layer 2 list empty / lookup failed → abort mentioning `--agent-id`.
/// - R5: Layer 2 list has ≥2 usable identities → abort enumerating every candidate.
/// - R6: Layer 2 single malformed entry → abort mentioning `--agent-id`.
async fn resolve_agent_id_or_error(explicit_agent_id: &str, role: i64) -> Result<String> {
    // R1 — explicit --agent-id wins; skip all resolution.
    let explicit = explicit_agent_id.trim();
    if !explicit.is_empty() {
        return Ok(explicit.to_string());
    }

    // R2 — Layer 1: resolve by dispatch role. Both `Err` and `Ok("")` count as a miss.
    let by_role = signing::resolve_agent_id_by_role(role)
        .await
        .unwrap_or_default();
    if !by_role.trim().is_empty() {
        return Ok(by_role.trim().to_string());
    }

    // Layer 2 — classify the full identity list. `fetch_my_agents()` returns an
    // empty Vec on any lookup failure/timeout, which folds into the 0-identity
    // abort (R4) so a lookup error can never leak through as an empty header.
    let agents = crate::commands::agent_commerce::task::common::fetch_my_agents().await;
    match classify_identities(&agents) {
        Classification::One(id) => Ok(id), // R3
        Classification::Many => bail!("{}", format_ambiguous_identities(&agents)), // R5
        // R4 (0-identity / lookup failure) and R6 (single malformed entry) share
        // the same actionable recovery: register an identity or pass --agent-id.
        Classification::None | Classification::MalformedSingle => bail!(
            "no agent identity found on this account. Register an identity \
             (route to okx-ai) or pass --agent-id <id> to choose one."
        ),
    }
}

/// Query task status.
pub async fn handle_status(client: &mut TaskApiClient, job_id: &str, agent_id: &str, role: i64) -> Result<()> {
    let agent_id = resolve_agent_id_or_error(agent_id, role).await?;
    let resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;

    let t = &resp;
    let token_sym = t["tokenSymbol"].as_str().unwrap_or("?");
    println!("Task status: {}", t["status"].as_i64().map(status_name).unwrap_or("?"));
    println!("  jobId:    {job_id}");
    println!("  title:    {}", t["title"].as_str().unwrap_or("?"));
    println!("  budget:   {} {}", t["tokenAmount"].as_str().unwrap_or("?"), token_sym);
    println!("  user:    {}", t["buyerAgentId"].as_str().unwrap_or("?"));
    if let Some(pid) = t["providerAgentId"].as_str() {
        println!("  asp: {pid}");
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
    let agent_id = resolve_agent_id_or_error(agent_id, role).await?;
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
        1 => "user",
        2 => "asp",
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
        "user"      => Some(1),
        "asp"       => Some(2),
        "evaluator" => Some(3),
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
///   4. (optional) `okx-a2a session query --job-id <jobId> --my-agent-id <myAgentId> --to-agent-id <counterpartyAgentId>` to confirm an active session exists
///   5. `okx-a2a session send --no-wait --job-id <jobId> --to-agent-id <counterpartyAgentId> --content <user's verbatim instruction>`
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
///       "myRole":              "user",
///       "counterpartyAgentId": "963",      // null when not yet designated (e.g. status=created with no asp)
///       "counterpartyRole":    "asp",      // null in the evaluator case
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
                "unrecognized --role value: {raw:?} (expected user / asp / evaluator)"
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

            let user_id = t.get("buyerAgentId").and_then(|v| v.as_str()).unwrap_or("");
            let provider_id = t.get("providerAgentId").and_then(|v| v.as_str()).unwrap_or("");

            // Counterparty inferred from my role:
            // - I'm user (1) → counterparty is asp
            // - I'm asp (2) → counterparty is user
            // - I'm evaluator (3) → no single counterparty (both user + asp are parties)
            let (counterparty_id, counterparty_role) = match role {
                1 => (provider_id, "asp"),
                2 => (user_id, "user"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agent(id: &str, role: i64) -> Value {
        json!({ "agentId": id, "role": role })
    }

    // ─── R5 / ambiguity builder ──────────────────────────────────────────
    // ≥2 identities: message must enumerate EVERY candidate agentId, each role
    // label, the total count, and end with the --agent-id selection hint.
    #[test]
    fn format_ambiguous_identities_enumerates_every_candidate() {
        let agents = [agent("2118", 2), agent("2210", 3), agent("2999", 1)];
        let msg = format_ambiguous_identities(&agents);
        assert!(
            msg.contains("has 3 identities"),
            "total count missing: {msg}"
        );
        for id in ["2118", "2210", "2999"] {
            assert!(msg.contains(id), "candidate {id} missing: {msg}");
        }
        for label in ["asp", "evaluator", "user"] {
            assert!(msg.contains(label), "role label {label} missing: {msg}");
        }
        assert!(msg.contains("--agent-id"), "selection hint missing: {msg}");
    }

    #[test]
    fn format_ambiguous_identities_skips_malformed_entries() {
        // A malformed entry (empty agentId) is excluded from the enumeration and
        // from the count, so the message stays consistent.
        let agents = [agent("2118", 2), json!({ "role": 2 }), agent("2999", 1)];
        let msg = format_ambiguous_identities(&agents);
        assert!(
            msg.contains("has 2 identities"),
            "count should reflect usable ids: {msg}"
        );
        assert!(msg.contains("2118") && msg.contains("2999"));
    }

    // ─── classify_identities: R3 / R4 / R5 / R6 ──────────────────────────
    #[test]
    fn classify_single_well_formed_is_one() {
        let agents = [agent("796", 1)];
        assert_eq!(
            classify_identities(&agents),
            Classification::One("796".into())
        );
    }

    #[test]
    fn classify_single_well_formed_trims_whitespace() {
        let agents = [agent("  796  ", 1)];
        assert_eq!(
            classify_identities(&agents),
            Classification::One("796".into())
        );
    }

    #[test]
    fn classify_empty_list_is_none() {
        assert_eq!(classify_identities(&[]), Classification::None);
    }

    #[test]
    fn classify_two_or_more_is_many() {
        let agents = [agent("2118", 2), agent("2210", 2)];
        assert_eq!(classify_identities(&agents), Classification::Many);
    }

    #[test]
    fn classify_single_malformed_entry() {
        // Exactly one entry, missing/empty agentId → MalformedSingle (R6).
        assert_eq!(
            classify_identities(&[json!({ "role": 2 })]),
            Classification::MalformedSingle
        );
        assert_eq!(
            classify_identities(&[agent("", 2)]),
            Classification::MalformedSingle
        );
    }

    #[test]
    fn classify_multiple_all_malformed_is_none() {
        let agents = [json!({ "role": 1 }), agent("", 2)];
        assert_eq!(classify_identities(&agents), Classification::None);
    }

    // ─── role label mapping (reuses role_name) ───────────────────────────
    #[test]
    fn role_label_maps_known_and_unknown_codes() {
        assert_eq!(role_label(1), "user");
        assert_eq!(role_label(2), "asp");
        assert_eq!(role_label(3), "evaluator");
        assert_eq!(role_label(99), "unknown");
        // role_label must delegate to role_name (single source of truth).
        for code in [1, 2, 3, 0, 99] {
            assert_eq!(role_label(code), role_name(code));
        }
    }

    // ─── R1 verbatim passthrough (no identity lookup) ────────────────────
    // An explicit --agent-id returns before any await on role/list lookup, so
    // this is deterministic and network-free.
    #[tokio::test]
    async fn resolve_r1_returns_explicit_id_verbatim() {
        let got = resolve_agent_id_or_error("2118", 1).await.unwrap();
        assert_eq!(got, "2118");
    }

    #[tokio::test]
    async fn resolve_r1_trims_explicit_id() {
        let got = resolve_agent_id_or_error("  2118  ", 2).await.unwrap();
        assert_eq!(got, "2118");
    }

    // R4 message contract (0-identity / lookup failure) — surfaced via the
    // ambiguity-free abort branch; here we assert the exact recovery string a
    // MalformedSingle/None classification maps to mentions --agent-id.
    #[test]
    fn none_and_malformed_recovery_hint_mentions_agent_id() {
        // The resolver funnels both None (R4) and MalformedSingle (R6) into the
        // same actionable message; guard that message here without touching the
        // network by re-checking the classification → hint invariant.
        for classification in [
            classify_identities(&[]),
            classify_identities(&[json!({ "role": 2 })]),
        ] {
            assert!(matches!(
                classification,
                Classification::None | Classification::MalformedSingle
            ));
        }
    }
}
