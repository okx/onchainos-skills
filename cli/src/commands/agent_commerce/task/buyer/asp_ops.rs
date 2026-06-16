//! ASP lifecycle operations (escrow simplified flow).
//!
//! - `asp-match`   — search matching ASPs (pre-publish or post-publish)
//! - `set-asp`     — set/replace ASP + service on an existing task
//! - `reset-asp`   — clear ASP + service fields
//! - `user-reject` — buyer rejects current ASP

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

// ── asp-match ────────────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/task/asp/match
///
/// At least one of `job_id` or `task_desc` must be non-empty.
/// When `job_id` is provided, backend uses the on-chain task context;
/// when only `task_desc` is provided, it's a pre-publish search.
pub async fn handle_asp_match(
    client: &mut TaskApiClient,
    job_id: Option<&str>,
    task_desc: &str,
    provider_agent_id: Option<&str>,
    page: usize,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    if job_id.is_none_or(|s| s.is_empty()) && task_desc.is_empty() {
        anyhow::bail!("at least one of --job-id or --task-desc is required for asp-match");
    }

    let agent_id = match explicit_agent_id {
        Some(id) => id.to_string(),
        None => signing::resolve_agent_id_by_role(
            crate::commands::agent_commerce::task::common::AGENT_ROLE_BUYER,
        )
        .await?,
    };

    let mut body = serde_json::json!({
        "page": page,
    });
    if let Some(jid) = job_id {
        if !jid.is_empty() {
            body["jobId"] = serde_json::Value::String(jid.to_string());
        }
    }
    if !task_desc.is_empty() {
        body["taskDesc"] = serde_json::Value::String(task_desc.to_string());
    }
    if let Some(pid) = provider_agent_id {
        body["providerAgentId"] = serde_json::Value::String(pid.to_string());
    }

    let resp = client
        .post_with_identity("/priapi/v1/aieco/task/asp/match", &body, &agent_id)
        .await?;

    let recs = resp["recommendations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let next_page = resp["nextPage"].as_u64();

    audit::log(
        "cli",
        "buyer/asp_match",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={agent_id}"),
            format!("taskDesc={task_desc}"),
            format!("page={page}"),
            format!("results={}", recs.len()),
        ]),
        None,
    );

    if recs.is_empty() {
        println!("No matching ASPs found for the given description.");
        return Ok(());
    }

    println!("Matched ASPs (page {page}, {} results):\n", recs.len());
    for (i, rec) in recs.iter().enumerate() {
        let pid = rec["providerAgentId"].as_str().unwrap_or("?");
        let sec = rec["securityRate"].as_f64().unwrap_or(0.0);
        let fb = rec["feedbackRate"].as_f64().unwrap_or(0.0);
        let sold = rec["soldCount"].as_u64().unwrap_or(0);
        let a2mcp = rec["supportA2MCP"].as_bool().unwrap_or(false);

        println!("━━━ {}. #{pid} ━━━", i + 1);
        println!(
            "  security: {sec:.2} | feedback: {fb:.2} | sold: {sold} | A2MCP: {a2mcp}"
        );

        if let Some(services) = rec["services"].as_array() {
            for svc in services {
                let sid = svc["serviceId"].as_str().unwrap_or("?");
                let sname = svc["serviceName"].as_str().unwrap_or("");
                let sdesc = svc["serviceDescription"].as_str().unwrap_or("");
                let stype = svc["serviceType"].as_str().unwrap_or("");
                let fee_amt = svc["feeAmount"].as_f64();
                let fee_sym = svc["feeTokenSymbol"].as_str().unwrap_or("");

                print!("  Service: {sid}");
                if !sname.is_empty() {
                    print!(" — {sname}");
                }
                println!(" [{stype}]");
                if !sdesc.is_empty() {
                    println!("    {sdesc}");
                }
                if let Some(amt) = fee_amt {
                    println!("    Fee: {amt} {fee_sym}");
                } else {
                    println!("    Fee: (no price — negotiation required)");
                }
            }
        }
        println!();
    }

    if let Some(np) = next_page {
        println!("Next page: {np}");
    }

    Ok(())
}

// ── set-asp ──────────────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/task/{jobId}/set/asp
///
/// Body: `{serviceId, serviceParams, servicePrice}`.
/// Does NOT carry tokenSymbol/tokenAmount — switching ASP does not change the
/// task's payment token or budget.
pub async fn handle_set_asp(
    client: &mut TaskApiClient,
    job_id: &str,
    service_id: &str,
    service_params: &str,
    service_price: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let agent_id = resolve_agent(client, job_id, explicit_agent_id).await?;

    client
        .post_with_identity(
            &client.endpoint(job_id, "set/asp"),
            &serde_json::json!({
                "serviceId": service_id,
                "serviceParams": service_params,
                "servicePrice": service_price,
            }),
            &agent_id,
        )
        .await?;

    audit::log(
        "cli",
        "buyer/set_asp",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("serviceId={service_id}"),
            format!("servicePrice={service_price}"),
        ]),
        None,
    );

    println!("✓ ASP and service updated (off-chain). Backend will trigger job_asp_selected.");
    println!("  serviceId: {service_id}");
    println!("  servicePrice: {service_price}");
    Ok(())
}

// ── reset-asp ────────────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/task/{jobId}/reset/asp
pub async fn handle_reset_asp(
    client: &mut TaskApiClient,
    job_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let agent_id = resolve_agent(client, job_id, explicit_agent_id).await?;

    client
        .post_with_identity(
            &client.endpoint(job_id, "reset/asp"),
            &serde_json::json!({}),
            &agent_id,
        )
        .await?;

    audit::log(
        "cli",
        "buyer/reset_asp",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
        ]),
        None,
    );

    println!("✓ ASP and service fields cleared (off-chain).");
    Ok(())
}

// ── user-reject ──────────────────────────────────────────────────────────

/// POST /priapi/v1/aieco/task/{jobId}/user/reject
pub async fn handle_user_reject(
    client: &mut TaskApiClient,
    job_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let agent_id = resolve_agent(client, job_id, explicit_agent_id).await?;

    client
        .post_with_identity(
            &client.endpoint(job_id, "user/reject"),
            &serde_json::json!({}),
            &agent_id,
        )
        .await?;

    audit::log(
        "cli",
        "buyer/user_reject",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
        ]),
        None,
    );

    println!("✓ Current ASP rejected (off-chain). ASP and service fields cleared.");
    println!("  Backend will trigger job_user_reject notification.");
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────────────

async fn resolve_agent(
    client: &mut TaskApiClient,
    job_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<String> {
    match explicit_agent_id {
        Some(id) => Ok(id.to_string()),
        None => {
            let (_, _, id) =
                signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
            Ok(id)
        }
    }
}
