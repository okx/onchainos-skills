//! ASP lifecycle operations (escrow simplified flow).
//!
//! - `asp-match`   — search matching ASPs (pre-publish or post-publish)
//! - `set-asp`     — set/replace ASP + service on an existing task
//! - `reset-asp`   — clear ASP + service fields
//! - `user-reject` — buyer rejects current ASP

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PaymentMode;
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
    format: &str,
) -> Result<()> {
    if job_id.is_none_or(|s| s.is_empty()) && task_desc.is_empty() {
        anyhow::bail!("at least one of --job-id or --task-desc is required for asp-match");
    }

    let json_mode = format.eq_ignore_ascii_case("json");

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

    if json_mode {
        crate::output::success(resp);
        return Ok(());
    }

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

/// Map service-type ("A2A" / "A2MCP") to the corresponding on-chain paymentMode.
fn service_type_to_payment_mode(service_type: &str) -> Result<PaymentMode> {
    match service_type.to_ascii_uppercase().as_str() {
        "A2A" => Ok(PaymentMode::Escrow),
        "A2MCP" => Ok(PaymentMode::X402),
        _ => bail!(
            "unsupported --service-type \"{service_type}\"; valid values: A2A, A2MCP"
        ),
    }
}

/// POST /priapi/v1/aieco/task/{jobId}/set/asp
///
/// Body: `{providerAgentId, serviceId, serviceType, serviceParams, serviceTokenAddress, serviceTokenAmount,
///         paymentTokenSymbol?, paymentTokenAmount?, paymentMostTokenAmount?}`.
#[allow(clippy::too_many_arguments)]
pub async fn handle_set_asp(
    client: &mut TaskApiClient,
    job_id: &str,
    provider_agent_id: &str,
    service_id: &str,
    service_type: &str,
    service_params: &str,
    service_token_address: &str,
    service_token_amount: &str,
    payment_token_symbol: Option<&str>,
    payment_token_amount: Option<&str>,
    payment_most_token_amount: Option<&str>,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let desired_mode = service_type_to_payment_mode(service_type)?;

    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, explicit_agent_id).await?;
    let task_resp = client.get_with_identity(&client.task_path(job_id), &agent_id).await?;
    let current_mode = PaymentMode::from_int(
        task_resp["paymentMode"].as_i64().unwrap_or(0) as i32,
    );

    // Step 1: sync paymentMode on-chain if it does not match the service_type.
    if current_mode != desired_mode {
        let resp = client.post_with_identity(
            &client.endpoint(job_id, "setPaymentMode"),
            &serde_json::json!({ "paymentMode": desired_mode.as_int() }),
            &agent_id,
        ).await?;
        let tx_hash = signing::sign_uop_and_broadcast(
            client,
            &resp["uopData"],
            &account_id,
            &address,
            job_id,
            signing::extract_biz_type(&resp),
            &agent_id,
            None,
        ).await?;
        audit::log(
            "cli",
            "buyer/set_asp_payment_mode_sync",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("from={}", current_mode.as_str()),
                format!("to={}", desired_mode.as_str()),
                format!("txHash={tx_hash}"),
            ]),
            None,
        );
        println!(
            "✓ Payment mode synced on-chain: {} → {} (txHash {tx_hash})",
            current_mode.as_str(),
            desired_mode.as_str(),
        );
    }

    // Step 2: POST set/asp (off-chain).
    let mut body = serde_json::json!({
        "providerAgentId": provider_agent_id,
        "serviceId": service_id,
        "serviceType": service_type,
        "serviceParams": service_params,
        "serviceTokenAddress": service_token_address,
        "serviceTokenAmount": service_token_amount,
    });
    if let Some(s) = payment_token_symbol {
        body["paymentTokenSymbol"] = serde_json::Value::String(s.to_string());
    }
    if let Some(a) = payment_token_amount {
        body["paymentTokenAmount"] = serde_json::Value::String(a.to_string());
    }
    if let Some(m) = payment_most_token_amount {
        body["paymentMostTokenAmount"] = serde_json::Value::String(m.to_string());
    }

    client
        .post_with_identity(
            &client.endpoint(job_id, "set/asp"),
            &body,
            &agent_id,
        )
        .await?;

    super::negotiate::save_designated_provider(job_id, provider_agent_id)?;

    audit::log(
        "cli",
        "buyer/set_asp",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("providerAgentId={provider_agent_id}"),
            format!("serviceId={service_id}"),
            format!("serviceType={service_type}"),
            format!("serviceTokenAmount={service_token_amount}"),
        ]),
        None,
    );

    // CLI mode: drop " Waiting for job_created event." — passive turn-end cue
    // suppresses LLM-driven watch re-arm.
    let waiting = if super::content::is_cli_mode() {
        ""
    } else {
        " Waiting for job_created event."
    };
    println!("✓ ASP and service updated (off-chain).{waiting}");
    println!("  providerAgentId: {provider_agent_id}");
    println!("  serviceId: {service_id}");
    println!("  serviceType: {service_type}");
    println!("  serviceTokenAmount: {service_token_amount}");
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::super::TaskCommand,
    }

    // ── asp-match ───────────────────────────────────────────────────

    #[test]
    fn cli_asp_match_task_desc_only() {
        let cli = TestCli::parse_from([
            "test", "asp-match", "--task-desc", "build a trading bot",
        ]);
        match cli.cmd {
            super::super::TaskCommand::AspMatch { task_desc, job_id, provider_agent_id, page, agent_id, format } => {
                assert_eq!(task_desc, "build a trading bot");
                assert!(job_id.is_none());
                assert!(provider_agent_id.is_none());
                assert_eq!(page, 1);
                assert!(agent_id.is_none());
                assert_eq!(format, "");
            }
            _ => panic!("expected AspMatch"),
        }
    }

    #[test]
    fn cli_asp_match_with_job_id_and_provider() {
        let cli = TestCli::parse_from([
            "test", "asp-match",
            "--job-id", "job-123",
            "--provider-agent-id", "agent-456",
            "--page", "2",
        ]);
        match cli.cmd {
            super::super::TaskCommand::AspMatch { job_id, provider_agent_id, page, .. } => {
                assert_eq!(job_id.as_deref(), Some("job-123"));
                assert_eq!(provider_agent_id.as_deref(), Some("agent-456"));
                assert_eq!(page, 2);
            }
            _ => panic!("expected AspMatch"),
        }
    }

    // ── set-asp ─────────────────────────────────────────────────────

    #[test]
    fn cli_set_asp_required_fields() {
        let cli = TestCli::parse_from([
            "test", "set-asp", "job-abc",
            "--provider-agent-id", "prov-1",
            "--service-id", "svc-99",
            "--service-type", "A2MCP",
            "--service-params", "查询内容：BTC price",
            "--service-token-address", "0xUSDT",
            "--service-token-amount", "10.5",
        ]);
        match cli.cmd {
            super::super::TaskCommand::SetAsp {
                job_id, provider_agent_id, service_id, service_type, service_params,
                service_token_address, service_token_amount,
                payment_token_symbol, payment_token_amount, payment_most_token_amount, agent_id,
            } => {
                assert_eq!(job_id, "job-abc");
                assert_eq!(provider_agent_id, "prov-1");
                assert_eq!(service_id, "svc-99");
                assert_eq!(service_type, "A2MCP");
                assert_eq!(service_params, "查询内容：BTC price");
                assert_eq!(service_token_address, "0xUSDT");
                assert_eq!(service_token_amount, "10.5");
                assert!(payment_token_symbol.is_none());
                assert!(payment_token_amount.is_none());
                assert!(payment_most_token_amount.is_none());
                assert!(agent_id.is_none());
            }
            _ => panic!("expected SetAsp"),
        }
    }

    #[test]
    fn cli_set_asp_with_payment_fields() {
        let cli = TestCli::parse_from([
            "test", "set-asp", "job-abc",
            "--provider-agent-id", "prov-1",
            "--service-id", "svc-1",
            "--service-type", "A2A",
            "--service-params", "none",
            "--service-token-address", "0xAddr",
            "--service-token-amount", "5",
            "--payment-token-symbol", "USDT",
            "--payment-token-amount", "5",
            "--payment-most-token-amount", "10",
        ]);
        match cli.cmd {
            super::super::TaskCommand::SetAsp {
                service_type, payment_token_symbol, payment_token_amount, payment_most_token_amount, ..
            } => {
                assert_eq!(service_type, "A2A");
                assert_eq!(payment_token_symbol.as_deref(), Some("USDT"));
                assert_eq!(payment_token_amount.as_deref(), Some("5"));
                assert_eq!(payment_most_token_amount.as_deref(), Some("10"));
            }
            _ => panic!("expected SetAsp"),
        }
    }

    #[test]
    fn cli_set_asp_missing_required_fails() {
        assert!(TestCli::try_parse_from(["test", "set-asp", "job-1"]).is_err());
    }

    #[test]
    fn cli_set_asp_missing_service_type_fails() {
        assert!(TestCli::try_parse_from([
            "test", "set-asp", "job-abc",
            "--provider-agent-id", "prov-1",
            "--service-id", "svc-1",
            "--service-params", "none",
            "--service-token-address", "0xAddr",
            "--service-token-amount", "5",
        ]).is_err());
    }

    // ── reset-asp ───────────────────────────────────────────────────

    #[test]
    fn cli_reset_asp_parses_job_id() {
        let cli = TestCli::parse_from(["test", "reset-asp", "job-xyz"]);
        match cli.cmd {
            super::super::TaskCommand::ResetAsp { job_id, agent_id } => {
                assert_eq!(job_id, "job-xyz");
                assert!(agent_id.is_none());
            }
            _ => panic!("expected ResetAsp"),
        }
    }

    #[test]
    fn cli_reset_asp_missing_job_id_fails() {
        assert!(TestCli::try_parse_from(["test", "reset-asp"]).is_err());
    }

    // ── user-reject ─────────────────────────────────────────────────

    #[test]
    fn cli_user_reject_parses_job_id() {
        let cli = TestCli::parse_from(["test", "user-reject", "job-rej"]);
        match cli.cmd {
            super::super::TaskCommand::UserReject { job_id, agent_id } => {
                assert_eq!(job_id, "job-rej");
                assert!(agent_id.is_none());
            }
            _ => panic!("expected UserReject"),
        }
    }

    #[test]
    fn cli_user_reject_with_agent_id() {
        let cli = TestCli::parse_from([
            "test", "user-reject", "job-rej", "--agent-id", "buyer-42",
        ]);
        match cli.cmd {
            super::super::TaskCommand::UserReject { job_id, agent_id } => {
                assert_eq!(job_id, "job-rej");
                assert_eq!(agent_id.as_deref(), Some("buyer-42"));
            }
            _ => panic!("expected UserReject"),
        }
    }

    #[test]
    fn cli_user_reject_missing_job_id_fails() {
        assert!(TestCli::try_parse_from(["test", "user-reject"]).is_err());
    }

    // ── create-task visibility ──────────────────────────────────────

    #[test]
    fn cli_create_visibility_defaults_to_private() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
        ]);
        match cli.cmd {
            super::super::TaskCommand::Create { visibility, .. } => {
                assert_eq!(visibility, 1);
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_visibility_public() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--visibility", "0",
        ]);
        match cli.cmd {
            super::super::TaskCommand::Create { visibility, .. } => {
                assert_eq!(visibility, 0);
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_with_service_fields() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--provider", "agent-1",
            "--service-id", "svc-1",
            "--service-params", "参数：x=1",
            "--service-token-address", "0xAddr",
            "--service-token-amount", "5.0",
        ]);
        match cli.cmd {
            super::super::TaskCommand::Create {
                provider, service_id, service_params,
                service_token_address, service_token_amount, visibility, ..
            } => {
                assert_eq!(provider.as_deref(), Some("agent-1"));
                assert_eq!(service_id.as_deref(), Some("svc-1"));
                assert_eq!(service_params.as_deref(), Some("参数：x=1"));
                assert_eq!(service_token_address.as_deref(), Some("0xAddr"));
                assert_eq!(service_token_amount.as_deref(), Some("5.0"));
                assert_eq!(visibility, 1);
            }
            _ => panic!("expected Create"),
        }
    }
}
