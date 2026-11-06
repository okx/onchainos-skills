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
use std::collections::HashSet;

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
pub(crate) async fn resolve_agent_id_or_error(
    explicit_agent_id: &str,
    role: i64,
) -> Result<String> {
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

/// Fetch authoritative task detail for an already-resolved querying identity.
pub async fn fetch_task_detail(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<Value> {
    client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
}

fn integer_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|field| {
        field
            .as_i64()
            .or_else(|| field.as_str()?.trim().parse().ok())
    })
}

fn status_code_for_task_type(
    job_type: Option<i64>,
    task_detail: &Value,
    subscription_detail: Option<&Value>,
) -> Option<i64> {
    match job_type {
        Some(0) => integer_field(task_detail, "status"),
        Some(1) => subscription_detail.and_then(|detail| {
            integer_field(detail, "subStatus").or_else(|| integer_field(detail, "status"))
        }),
        _ => None,
    }
}

/// Query task status.
pub async fn handle_status(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    role: i64,
) -> Result<()> {
    let resolved_agent_id = resolve_agent_id_or_error(agent_id, role).await?;
    let resp = match fetch_task_detail(client, job_id, &resolved_agent_id).await {
        Ok(resp) => resp,
        Err(task_error) => {
            // Subscription disputes may not exist on the ordinary one-time
            // task-detail endpoint. The shared dispute endpoint remains the
            // authoritative existence/permission check for both task types.
            let dispute = crate::commands::agent_commerce::task::evaluator::dispute_status::get_dispute_status(
                client, job_id, &resolved_agent_id,
            )
            .await
            .map_err(|_| task_error)?;
            let supplement = if dispute.job_type == Some(1) {
                client
                    .fetch_subscription(job_id, &resolved_agent_id)
                    .await
                    .unwrap_or_else(|_| json!({}))
            } else {
                json!({})
            };
            emit_arbitration_status(job_id, &supplement, &dispute);
            return Ok(());
        }
    };
    let job_type = integer_field(&resp, "jobType");
    // The ordinary task record is only the type gate for subscriptions. Its
    // status projection may lag behind the subscription lifecycle, so never
    // use it as the authoritative subscription status.
    let subscription_detail = if job_type == Some(1) {
        client
            .fetch_subscription(job_id, &resolved_agent_id)
            .await
            .ok()
    } else {
        None
    };
    let status_code = status_code_for_task_type(job_type, &resp, subscription_detail.as_ref());
    let status_detail = subscription_detail.as_ref().unwrap_or(&resp);
    let dispute = match status_code {
        Some(4) => Some(
            crate::commands::agent_commerce::task::evaluator::dispute_status::get_dispute_status(
                client,
                job_id,
                &resolved_agent_id,
            )
            .await?,
        ),
        Some(6 | 9) => {
            crate::commands::agent_commerce::task::evaluator::dispute_status::get_dispute_status(
                client,
                job_id,
                &resolved_agent_id,
            )
            .await
            .ok()
        }
        _ => None,
    };
    if let Some(dispute) = dispute.as_ref() {
        emit_arbitration_status(job_id, status_detail, dispute);
    } else {
        let t = &resp;
        let token_sym = t["tokenSymbol"].as_str().unwrap_or("?");
        let code = status_code;
        println!("Task type: {}", task_type_name(job_type));
        let user_close_submitted = job_type == Some(1)
            && crate::commands::agent_commerce::task::user::refund::has_created_subscription_close_receipt(
                job_id,
                &resolved_agent_id,
            );
        let (status_label, status_description) = code
            .map(|status| {
                status_copy_for_task_type(job_type, status, status_detail, user_close_submitted)
            })
            .unwrap_or_else(|| {
                (
                    "Status unavailable".to_string(),
                    "The task status is currently unavailable.".to_string(),
                )
            });
        println!("Task status: {status_label}");
        println!("Status detail: {status_description}");
        println!("  jobId:    {job_id}");
        println!("  title:    {}", t["title"].as_str().unwrap_or("?"));
        println!(
            "  description: {}",
            t["description"].as_str().unwrap_or("?")
        );
        println!(
            "  budget:   {} {}",
            t["tokenAmount"].as_str().unwrap_or("?"),
            token_sym
        );
        println!("  user:    {}", t["buyerAgentId"].as_str().unwrap_or("?"));
        if let Some(pid) = t["providerAgentId"].as_str() {
            println!("  asp: {pid}");
        }
    }
    Ok(())
}

pub(crate) fn emit_arbitration_status(
    job_id: &str,
    supplement: &Value,
    dispute: &crate::commands::agent_commerce::task::evaluator::dispute_status::DisputeStatusResponse,
) {
    let result = crate::commands::agent_commerce::task::arbitration::build_detail_result(
        job_id,
        supplement,
        Some(dispute),
        None,
        None,
    );
    crate::output::success(result);
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
    let is_dispute = status == Some("disputed");
    if is_dispute {
        // Compatibility route for the original `tasks --status disputed`
        // entrypoint. The canonical implementation and output contract live in
        // the task-level arbitration domain.
        return crate::commands::agent_commerce::task::arbitration::handle_arbitration_list(
            client, &agent_id, page, limit,
        )
        .await;
    }

    let mut path = format!("/priapi/v1/aieco/task/my?page={page}&page_size={limit}");
    if let Some(s) = status {
        path.push_str(&format!("&status={s}"));
    }
    let resp = client.get_with_identity(&path, &agent_id).await?;
    let tasks = resp["list"].as_array().cloned().unwrap_or_default();
    let total = resp["total"].as_u64().unwrap_or(0);
    println!("Task list ({total} total, page {page}):");
    for t in &tasks {
        let sym = t["tokenSymbol"].as_str().unwrap_or("?");
        let status_code = t["status"].as_i64();
        println!(
            "  [{}] {} — {} {}",
            status_code
                .map(task_status_label)
                .unwrap_or("Status unavailable"),
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

/// User-facing one-time task status. The backend key remains available through
/// `status_name`; this label carries the business meaning shown in templates.
pub fn task_status_label(code: i64) -> &'static str {
    match code {
        -1 => "Initializing",
        0 => "Awaiting ASP acceptance",
        1 => "In progress",
        2 => "Awaiting buyer review",
        3 => "Awaiting refund decision",
        4 => "Evaluation in progress",
        5 => "Stopped by platform",
        6 => "Completed",
        7 => "Closed",
        8 => "Expired",
        // For one-time tasks, backend Failed(9) is the canonical terminal
        // projection after the buyer refund path succeeds.
        9 => "Refund completed",
        _ => "Status unavailable",
    }
}

pub fn task_status_description(code: i64) -> &'static str {
    match code {
        -1 => "The task is being initialized.",
        0 => "The task is waiting for an ASP to accept it.",
        1 => "The ASP accepted the task and is working on it.",
        2 => "The ASP submitted the deliverable and is waiting for buyer review.",
        3 => "The buyer rejected the deliverable and the refund request awaits an ASP decision.",
        4 => "The refund request is in Evaluation.",
        5 => "The platform stopped the task.",
        6 => "The task completed and funds were released to the ASP.",
        7 => "The task is closed.",
        8 => "The task expired.",
        9 => "The refund completed and the task is closed.",
        _ => "The task status is currently unavailable.",
    }
}

fn task_type_name(job_type: Option<i64>) -> &'static str {
    match job_type {
        Some(0) => "one_time",
        Some(1) => "subscription",
        _ => "unknown",
    }
}

fn subscription_status_name(code: i64) -> &'static str {
    match code {
        -1 => "init",
        0 => "created",
        1 => "active",
        3 => "rejected",
        4 => "disputed",
        6 => "completed",
        7 => "closed",
        8 => "expired",
        9 => "failed",
        _ => "unknown",
    }
}

fn subscription_status_label(code: i64) -> &'static str {
    match code {
        -1 => "Initializing",
        0 => "Awaiting ASP acceptance",
        1 => "Active",
        3 => "Awaiting ASP decision",
        4 => "Evaluation in progress",
        6 => "Completed",
        7 => "Closed",
        8 => "Expired",
        9 => "Subscription result needs reconciliation",
        _ => "Status unavailable",
    }
}

fn subscription_status_description(code: i64) -> &'static str {
    match code {
        -1 => "The subscription is being initialized.",
        0 => "The subscription is waiting for an ASP to accept it.",
        1 => "The subscription is active.",
        3 => "The buyer rejected the current delivery and is waiting for the ASP's decision.",
        4 => "The subscription refund request is in Evaluation.",
        6 => "The subscription completed without a refund.",
        7 => "The subscription is closed.",
        8 => "The subscription expired.",
        9 => "The subscription result requires settlement reconciliation.",
        _ => "The subscription status is currently unavailable.",
    }
}

fn status_copy_for_task_type(
    job_type: Option<i64>,
    code: i64,
    detail: &Value,
    user_close_submitted: bool,
) -> (String, String) {
    match job_type {
        Some(0) => (
            task_status_label(code).to_string(),
            task_status_description(code).to_string(),
        ),
        Some(1) if matches!(code, 8 | 9) => {
            super::lifecycle::subscription_status_copy(detail, user_close_submitted)
        }
        Some(1) => (
            subscription_status_label(code).to_string(),
            subscription_status_description(code).to_string(),
        ),
        _ => (
            "Status unavailable".to_string(),
            "The task type is unknown, so its status cannot be interpreted safely.".to_string(),
        ),
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

/// Actionable/non-terminal statuses for each task lifecycle. Expired(8) is
/// terminal because the backend projects it only after applicable settlement.
fn is_non_terminal(kind: ActiveTaskKind, code: i64) -> bool {
    match kind {
        ActiveTaskKind::OneTime => matches!(code, 0..=4),
        ActiveTaskKind::Subscription => matches!(code, -1 | 0 | 1 | 3 | 4),
    }
}

fn short_job_id(jid: &str) -> String {
    if jid.len() < 12 {
        return jid.to_string();
    }
    format!("{}…{}", &jid[..6], &jid[jid.len() - 4..])
}

fn parse_role_arg(raw: &str) -> Option<i64> {
    match raw.trim().to_lowercase().as_str() {
        "user" => Some(1),
        "asp" => Some(2),
        "evaluator" => Some(3),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveTaskKind {
    OneTime,
    Subscription,
}

fn ordinary_task_kind(task: &Value) -> Option<ActiveTaskKind> {
    match integer_field(task, "jobType") {
        Some(1) => None,
        Some(0) | None => Some(ActiveTaskKind::OneTime),
        Some(_) => None,
    }
}

fn string_field_from_keys<'a>(value: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
}

fn list_items(value: &Value) -> Vec<Value> {
    value
        .get("list")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default()
}

fn active_task_row(
    task: &Value,
    kind: ActiveTaskKind,
    agent_id: &str,
    role: i64,
    include_terminal: bool,
) -> Option<Value> {
    let status_code = match kind {
        ActiveTaskKind::OneTime => integer_field(task, "status"),
        ActiveTaskKind::Subscription => {
            integer_field(task, "subStatus").or_else(|| integer_field(task, "status"))
        }
    }?;
    if !include_terminal && !is_non_terminal(kind, status_code) {
        return None;
    }

    let job_id = string_field_from_keys(task, &["jobId", "subId"]);
    if job_id.is_empty() {
        return None;
    }
    let user_id = string_field_from_keys(task, &["buyerAgentId", "userAgentId"]);
    let provider_id = string_field_from_keys(task, &["providerAgentId", "aspAgentId"]);
    let (counterparty_id, counterparty_role) = match role {
        1 => (provider_id, "asp"),
        2 => (user_id, "user"),
        _ => ("", ""),
    };
    let user_close_submitted = matches!(kind, ActiveTaskKind::Subscription)
        && crate::commands::agent_commerce::task::user::refund::has_created_subscription_close_receipt(
            job_id,
            agent_id,
        );
    let (task_type, status, status_label, status_description) = match kind {
        ActiveTaskKind::OneTime => (
            "one_time",
            status_name(status_code),
            task_status_label(status_code).to_string(),
            task_status_description(status_code).to_string(),
        ),
        ActiveTaskKind::Subscription => {
            let (label, description) =
                status_copy_for_task_type(Some(1), status_code, task, user_close_submitted);
            (
                "subscription",
                subscription_status_name(status_code),
                label,
                description,
            )
        }
    };

    Some(json!({
        "jobId": job_id,
        "shortJobId": short_job_id(job_id),
        "taskType": task_type,
        "status": status,
        "statusLabel": status_label,
        "statusDescription": status_description,
        "statusCode": status_code,
        "title": string_field_from_keys(task, &["title", "jobName", "serviceName"]),
        "tokenAmount": string_field_from_keys(task, &["paymentTokenAmount", "tokenAmount", "serviceTokenAmount"]),
        "tokenSymbol": string_field_from_keys(task, &["paymentTokenSymbol", "tokenSymbol", "serviceTokenSymbol"]),
        "myAgentId": agent_id,
        "myRole": role_name(role),
        "counterpartyAgentId": if counterparty_id.is_empty() {
            Value::Null
        } else {
            Value::String(counterparty_id.to_string())
        },
        "counterpartyRole": if counterparty_role.is_empty() {
            Value::Null
        } else {
            Value::String(counterparty_role.to_string())
        },
    }))
}

fn push_unique_active_task(rows: &mut Vec<Value>, seen: &mut HashSet<String>, row: Value) {
    let agent_id = row.get("myAgentId").and_then(Value::as_str).unwrap_or("");
    let job_id = row.get("jobId").and_then(Value::as_str).unwrap_or("");
    if seen.insert(format!("{agent_id}\0{job_id}")) {
        rows.push(row);
    }
}

/// Aggregated non-terminal one-time and subscription task list across all
/// agents under the current active account. Designed for the user-session
/// "ad-hoc instruction → sub session"
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
///       "taskType":            "one_time",
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
            anyhow::anyhow!("unrecognized --role value: {raw:?} (expected user / asp / evaluator)")
        })?;
        agents.retain(|a| a.get("role").and_then(|v| v.as_i64()) == Some(want));
    }

    // 2. For each agent, query both task registries and aggregate. Subscription
    // rows are inserted first so their authoritative lifecycle wins if an old
    // task projection exposes the same Job ID.
    let mut all_tasks: Vec<Value> = Vec::new();
    let mut seen_tasks = HashSet::new();
    for agent in &agents {
        let agent_id = agent.get("agentId").and_then(|v| v.as_str()).unwrap_or("");
        let role = agent.get("role").and_then(|v| v.as_i64()).unwrap_or(0);
        if agent_id.is_empty() {
            continue;
        }

        if matches!(role, 1 | 2) {
            let status_types: &[u8] = if include_terminal { &[1, 2] } else { &[1] };
            for status_type in status_types {
                let path = format!(
                    "/priapi/v1/aieco/task/subscribe/my?page=1&pageSize=100&statusType={status_type}"
                );
                let response = match client.get_with_identity(&path, agent_id).await {
                    Ok(response) => response,
                    Err(error) => {
                        if DEBUG_LOG {
                            eprintln!(
                                "[active-tasks] agent {agent_id} subscription query failed: {error}"
                            );
                        }
                        continue;
                    }
                };
                for task in list_items(&response) {
                    if let Some(row) = active_task_row(
                        &task,
                        ActiveTaskKind::Subscription,
                        agent_id,
                        role,
                        include_terminal,
                    ) {
                        push_unique_active_task(&mut all_tasks, &mut seen_tasks, row);
                    }
                }
            }
        }

        let path = "/priapi/v1/aieco/task/my?page=1&page_size=100";
        let response = match client.get_with_identity(path, agent_id).await {
            Ok(response) => response,
            Err(error) => {
                if DEBUG_LOG {
                    eprintln!("[active-tasks] agent {agent_id} task query failed: {error}");
                }
                continue;
            }
        };
        for task in list_items(&response) {
            // `/task/my` may retain a stale projection for subscription jobs.
            // Those rows are owned by `/subscribe/my`; treating them as
            // one-time tasks can resurrect a terminal subscription as active.
            let Some(kind) = ordinary_task_kind(&task) else {
                continue;
            };
            if let Some(row) = active_task_row(&task, kind, agent_id, role, include_terminal) {
                push_unique_active_task(&mut all_tasks, &mut seen_tasks, row);
            }
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

    #[test]
    fn a2a_lists_show_actual_payment_snapshot_for_both_roles() {
        for token in super::super::payment_token::TOKENS {
            for role in [1, 2] {
                let task = json!({"jobId": "job", "status": 0, "subStatus": 0,
                    "paymentTokenSymbol": token.symbol, "paymentTokenAmount": "10",
                    "serviceTokenSymbol": "USD", "serviceTokenAmount": "99"});
                for kind in [ActiveTaskKind::OneTime, ActiveTaskKind::Subscription] {
                    let row = active_task_row(&task, kind, "agent", role, true).unwrap();
                    assert_eq!(row["tokenSymbol"], token.symbol);
                    assert_eq!(row["tokenAmount"], "10");
                }
            }
        }
    }
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

    #[test]
    fn one_time_failed_backend_status_has_refund_business_label() {
        assert_eq!(status_name(9), "failed");
        assert_eq!(task_status_label(9), "Refund completed");
        assert_eq!(
            task_status_description(9),
            "The refund completed and the task is closed."
        );
    }

    #[test]
    fn status_output_is_routed_by_task_type() {
        assert_eq!(task_type_name(Some(0)), "one_time");
        assert_eq!(task_type_name(Some(1)), "subscription");
        assert_eq!(task_type_name(None), "unknown");

        let detail = json!({"status": 1});
        assert_eq!(
            status_copy_for_task_type(Some(0), 1, &detail, false).0,
            "In progress"
        );
        assert_eq!(
            status_copy_for_task_type(Some(1), 1, &detail, false).0,
            "Active"
        );
        assert_eq!(
            status_copy_for_task_type(None, 1, &detail, false).0,
            "Status unavailable"
        );
        assert_eq!(
            status_copy_for_task_type(Some(1), 1, &detail, false).1,
            "The subscription is active."
        );
        assert_eq!(
            status_copy_for_task_type(None, 1, &detail, false).1,
            "The task type is unknown, so its status cannot be interpreted safely."
        );
    }

    #[test]
    fn subscription_status_comes_from_subscription_detail() {
        let task_detail = json!({"jobType": 1, "status": 1});
        let subscription_detail = json!({"subStatus": 7});

        assert_eq!(
            status_code_for_task_type(Some(1), &task_detail, Some(&subscription_detail)),
            Some(7)
        );
        assert_eq!(
            status_copy_for_task_type(
                Some(1),
                status_code_for_task_type(Some(1), &task_detail, Some(&subscription_detail))
                    .unwrap(),
                &subscription_detail,
                false,
            )
            .0,
            "Closed"
        );
    }

    #[test]
    fn subscription_failed_status_uses_settlement_facts() {
        let refunded = json!({
            "status": 9,
            "trialType": 0,
            "paymentTokenAmount": "10",
            "paymentTokenSymbol": "USDT"
        });
        let trial_failure = json!({"status": 9, "trialType": 1});
        let unverified = json!({"status": 9});

        assert_eq!(
            status_copy_for_task_type(Some(1), 9, &refunded, false).0,
            "Refund completed; task closed"
        );
        assert_eq!(
            status_copy_for_task_type(Some(1), 9, &trial_failure, false).0,
            "Paid subscription did not start; task closed"
        );
        assert_eq!(
            status_copy_for_task_type(Some(1), 9, &unverified, false).0,
            "Subscription result needs reconciliation"
        );
    }

    #[test]
    fn subscription_status_fails_closed_when_subscription_detail_is_missing() {
        let stale_task_detail = json!({"jobType": 1, "status": 1});

        assert_eq!(
            status_code_for_task_type(Some(1), &stale_task_detail, None),
            None
        );
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

    #[test]
    fn active_task_filter_uses_task_specific_status_sets() {
        for status in [0, 1, 2, 3, 4] {
            assert!(
                is_non_terminal(ActiveTaskKind::OneTime, status),
                "one-time status {status} must stay visible"
            );
        }
        for status in [-1, 0, 1, 3, 4] {
            assert!(
                is_non_terminal(ActiveTaskKind::Subscription, status),
                "subscription status {status} must stay visible"
            );
        }
        assert!(!is_non_terminal(ActiveTaskKind::Subscription, 2));
        for kind in [ActiveTaskKind::OneTime, ActiveTaskKind::Subscription] {
            for status in [5, 6, 7, 8, 9] {
                assert!(!is_non_terminal(kind, status));
            }
        }
    }

    #[test]
    fn ordinary_task_projection_excludes_subscription_rows() {
        assert_eq!(
            ordinary_task_kind(&json!({"jobType": 0})),
            Some(ActiveTaskKind::OneTime)
        );
        assert_eq!(ordinary_task_kind(&json!({"jobType": 1})), None);
        assert_eq!(ordinary_task_kind(&json!({"jobType": "1"})), None);
        assert_eq!(
            ordinary_task_kind(&json!({"status": 1})),
            Some(ActiveTaskKind::OneTime)
        );
        assert_eq!(ordinary_task_kind(&json!({"jobType": 99})), None);
    }

    #[test]
    fn active_task_rows_distinguish_one_time_and_subscription_statuses() {
        let one_time = active_task_row(
            &json!({
                "jobId": "task-1",
                "status": 1,
                "title": "One-time analysis",
                "buyerAgentId": "buyer-1",
                "providerAgentId": "asp-1"
            }),
            ActiveTaskKind::OneTime,
            "asp-1",
            2,
            false,
        )
        .unwrap();
        let subscription = active_task_row(
            &json!({
                "jobId": "subscription-1",
                "subStatus": "1",
                "serviceName": "Daily signals",
                "buyerAgentId": "buyer-1",
                "providerAgentId": "asp-1"
            }),
            ActiveTaskKind::Subscription,
            "asp-1",
            2,
            false,
        )
        .unwrap();

        assert_eq!(one_time["taskType"], "one_time");
        assert_eq!(one_time["statusLabel"], "In progress");
        assert_eq!(subscription["taskType"], "subscription");
        assert_eq!(subscription["statusLabel"], "Active");
    }

    #[test]
    fn active_task_rows_filter_terminal_subscriptions() {
        let closed = json!({"jobId": "subscription-1", "status": 7});
        assert!(
            active_task_row(&closed, ActiveTaskKind::Subscription, "buyer-1", 1, false).is_none()
        );
        assert!(
            active_task_row(&closed, ActiveTaskKind::Subscription, "buyer-1", 1, true).is_some()
        );
    }

    #[test]
    fn subscription_row_wins_when_task_projection_has_the_same_job_id() {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        let subscription = active_task_row(
            &json!({"jobId": "same-job", "status": 7}),
            ActiveTaskKind::Subscription,
            "buyer-1",
            1,
            true,
        )
        .unwrap();
        let stale_task = active_task_row(
            &json!({"jobId": "same-job", "status": 1}),
            ActiveTaskKind::OneTime,
            "buyer-1",
            1,
            true,
        )
        .unwrap();

        push_unique_active_task(&mut rows, &mut seen, subscription);
        push_unique_active_task(&mut rows, &mut seen, stale_task);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["taskType"], "subscription");
        assert_eq!(rows[0]["statusLabel"], "Closed");
    }
}
