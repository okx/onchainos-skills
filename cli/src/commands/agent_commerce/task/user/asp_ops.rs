//! ASP lifecycle operations (escrow simplified flow).
//!
//! - `asp-match`   — search matching ASPs (pre-publish or post-publish)
//! - `set-asp`     — set/replace ASP + service on an existing task
//! - `reset-asp`   — clear ASP + service fields
//! - `user-reject` — user rejects current ASP

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::autotrade::tooling;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::PaymentMode;
use crate::commands::agent_commerce::task::signing;

// ── asp-match ────────────────────────────────────────────────────────────

/// ASP-match exposes subscription billing under `subscription[].fee`. Normalize
/// that response shape for the text renderer so it shows the fixed subscription
/// price instead of a possibly unrelated one-time listing price. The compact
/// JSON response keeps subscription billing under `subscriptionInfo.feeAmount`.
fn normalize_subscription_fee(service: &mut serde_json::Value) {
    if let Some(fee) = selected_subscription_fee(service) {
        service["feeAmount"] = fee;
    }
}

fn scalar_display(value: &serde_json::Value) -> Option<String> {
    if let Some(value) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(value.to_string());
    }
    if value.is_number() {
        return Some(value.to_string());
    }
    None
}

fn supports_trial(service: &serde_json::Value) -> bool {
    service["supportTrial"].as_bool().unwrap_or(false)
}

fn selected_subscription(service: &serde_json::Value) -> Option<&serde_json::Value> {
    let subscriptions = service.get("subscription")?.as_array()?;
    subscriptions
        .iter()
        .find(|entry| entry.get("interval").and_then(|v| v.as_str()) == Some("month"))
        .or_else(|| subscriptions.first())
}

fn selected_subscription_fee(service: &serde_json::Value) -> Option<serde_json::Value> {
    selected_subscription(service)
        .and_then(|entry| entry.get("fee"))
        .filter(|value| !value.is_null() && !value.as_str().is_some_and(|s| s.trim().is_empty()))
        .cloned()
}

fn build_subscription_info(service: &serde_json::Value) -> serde_json::Value {
    let subscription = selected_subscription(service);
    let subscription_fee = selected_subscription_fee(service);
    let support_subscription = subscription.is_some()
        || service
            .get("supportSubscription")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
    if !support_subscription {
        return serde_json::Value::Null;
    }

    serde_json::json!({
        "interval": subscription
            .and_then(|entry| entry.get("interval"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "feeAmount": subscription_fee.unwrap_or(serde_json::Value::Null),
        "supportTrial": supports_trial(service),
        "freeTrial": service.get("freeTrial").cloned().unwrap_or(serde_json::json!(0)),
    })
}

fn copy_field(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Value,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_string(), value.clone());
    }
}

fn compact_service_for_ai(service: &serde_json::Value) -> serde_json::Value {
    let subscription_info = build_subscription_info(&service);
    let support_subscription = !subscription_info.is_null();
    let mut compact = serde_json::Map::new();
    for key in [
        "serviceId",
        "serviceName",
        "serviceType",
        "serviceDescription",
        "feeToken",
        "feeTokenSymbol",
        "endpoint",
        "autoTradePreflight",
    ] {
        copy_field(&mut compact, &service, key);
    }
    if !support_subscription {
        copy_field(&mut compact, &service, "feeAmount");
    }
    compact.insert(
        "supportSubscription".to_string(),
        serde_json::Value::Bool(support_subscription),
    );
    compact.insert("subscriptionInfo".to_string(), subscription_info);

    serde_json::Value::Object(compact)
}

fn compact_recommendation_for_ai(rec: &serde_json::Value) -> serde_json::Value {
    let mut compact = serde_json::Map::new();
    for key in [
        "providerAgentId",
        "providerAgentName",
        "securityRate",
        "feedbackRate",
        "soldCount",
        "supportA2MCP",
    ] {
        copy_field(&mut compact, rec, key);
    }
    let services = rec
        .get("services")
        .and_then(|value| value.as_array())
        .map(|services| {
            services
                .iter()
                .map(compact_service_for_ai)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    compact.insert("services".to_string(), serde_json::Value::Array(services));
    serde_json::Value::Object(compact)
}

fn compact_asp_match_response(resp: serde_json::Value) -> serde_json::Value {
    let recommendations = resp
        .get("recommendations")
        .and_then(|value| value.as_array())
        .map(|recs| {
            recs.iter()
                .map(compact_recommendation_for_ai)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut compact = serde_json::Map::new();
    compact.insert(
        "recommendations".to_string(),
        serde_json::Value::Array(recommendations),
    );
    if let Some(next_page) = resp.get("nextPage") {
        compact.insert("nextPage".to_string(), next_page.clone());
    }
    serde_json::Value::Object(compact)
}

/// Render a service provider header as `Agent <pid>(<pname>)`, degrading to
/// `Agent <pid>` (no parentheses) when the display name is empty or absent
/// (WBW-14172 FR-3.2 / FR-3.3). `providerAgentName` is optional in the backend
/// contract, so consumers MUST tolerate its absence.
fn format_provider(pid: &str, pname: &str) -> String {
    if pname.is_empty() {
        format!("Agent {pid}")
    } else {
        format!("Agent {pid}({pname})")
    }
}

/// POST /priapi/v1/aieco/task/asp/match
///
/// At least one of `job_id` or `task_desc` must be non-empty.
/// When `job_id` is provided, backend uses the on-chain task context;
/// when only `task_desc` is provided, it's a pre-publish search.
#[allow(clippy::too_many_arguments)]
pub async fn handle_asp_match(
    client: &mut TaskApiClient,
    job_id: Option<&str>,
    task_desc: &str,
    provider_agent_id: Option<&str>,
    payment_token_amount: Option<f64>,
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
            crate::commands::agent_commerce::task::common::AGENT_ROLE_USER,
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
    if let Some(amt) = payment_token_amount {
        body["paymentTokenAmount"] = serde_json::json!(amt);
    }

    let resp = client
        .post_with_identity("/priapi/v1/aieco/task/asp/match", &body, &agent_id)
        .await?;
    let mut resp = resp;

    // Attach a per-service `autoTradePreflight` (FR-1/2): deterministic, local,
    // non-networked. One inventory snapshot is shared across every service so all
    // rows see a consistent readiness view. Any per-service internal error degrades
    // to the sentinel preflight rather than failing the match.
    let inv = tooling::ToolInventory::detect();
    if let Some(recs_mut) = resp["recommendations"].as_array_mut() {
        for rec in recs_mut.iter_mut() {
            if let Some(services) = rec["services"].as_array_mut() {
                for svc in services.iter_mut() {
                    normalize_subscription_fee(svc);
                    let desc = svc["serviceDescription"].as_str().unwrap_or("");
                    // Build the (infallible) preflight, then serialize once here —
                    // the sole genuinely fallible boundary. Only a serialization
                    // failure degrades to the sentinel; classification never errors.
                    let pf = tooling::build_preflight(desc, &inv);
                    svc["autoTradePreflight"] = serde_json::to_value(&pf).unwrap_or_else(|_| {
                        serde_json::to_value(tooling::degraded_preflight()).unwrap_or_default()
                    });
                }
            }
        }
    }

    let recs = resp["recommendations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let next_page = resp["nextPage"].as_u64();

    audit::log(
        "cli",
        "user/asp_match",
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
        crate::output::success(compact_asp_match_response(resp));
        return Ok(());
    }

    if recs.is_empty() {
        println!("No matching ASPs found for the given description.");
        return Ok(());
    }

    println!("Matched ASPs (page {page}, {} results):\n", recs.len());
    for (i, rec) in recs.iter().enumerate() {
        let pid = rec["providerAgentId"].as_str().unwrap_or("?");
        let pname = rec["providerAgentName"].as_str().unwrap_or("");
        let sec = rec["securityRate"].as_f64().unwrap_or(0.0);
        let fb = rec["feedbackRate"].as_f64().unwrap_or(0.0);
        let sold = rec["soldCount"].as_u64().unwrap_or(0);
        let a2mcp = rec["supportA2MCP"].as_bool().unwrap_or(false);

        println!(
            "━━━ {}. {} ━━━",
            i + 1,
            format_provider(pid, pname)
        );
        println!(
            "  security: {sec:.2} | feedback: {fb:.2} | sold: {sold} | A2MCP: {a2mcp}"
        );

        if let Some(services) = rec["services"].as_array() {
            for svc in services {
                let sid = svc["serviceId"].as_str().unwrap_or("?");
                let sname = svc["serviceName"].as_str().unwrap_or("");
                let sdesc = svc["serviceDescription"].as_str().unwrap_or("");
                let stype = svc["serviceType"].as_str().unwrap_or("");
                let fee_amt = scalar_display(&svc["feeAmount"]);
                let fee_sym = svc["feeTokenSymbol"].as_str().unwrap_or("");
                let support_trial = supports_trial(svc);

                print!("  Service: {sid}");
                if !sname.is_empty() {
                    print!(" — {sname}");
                }
                println!(" [{stype}]");
                if !sdesc.is_empty() {
                    println!("    {sdesc}");
                }
                if let Some(amt) = fee_amt.as_deref() {
                    println!("    Fee: {amt} {fee_sym}");
                } else {
                    println!("    Fee: (no price — negotiation required)");
                }

                if let Some(subs) = svc["subscription"].as_array() {
                    if !subs.is_empty() {
                        for sub in subs {
                            let interval = sub["interval"].as_str().unwrap_or("month");
                            let fee = scalar_display(&sub["fee"]).unwrap_or_else(|| "?".to_string());
                            print!("    Subscription: {fee} {fee_sym}/{interval}");
                            if support_trial {
                                print!(" (trial available)");
                            }
                            println!();
                        }
                    }
                }

                if let Some(line) = preflight_summary_line(&svc["autoTradePreflight"]) {
                    print!("    {line}");
                    println!();
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
            "user/set_asp_payment_mode_sync",
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

    let old_provider = super::negotiate::get_designated_provider(job_id)
        .ok()
        .flatten();
    if let Some(ref old_pid) = old_provider {
        if old_pid != provider_agent_id {
            match super::super::common::okx_a2a::session_delete(job_id, Some(old_pid)) {
                Ok(()) => println!("✓ Old job session deleted (provider {old_pid})."),
                Err(e) => eprintln!("⚠ Old job session delete failed (provider {old_pid}): {e}"),
            }
        }
    }

    // FR-8.3/AC-9: resolve and persist the correct multi-service endpoint from the
    // provider's service catalog (previously persisted endpoint-less). A2A /
    // no-endpoint services resolve to None → unchanged routing (FR-8.5/AC-11).
    let resolved_endpoint: Option<String> =
        crate::commands::agent_commerce::task::common::find_service(provider_agent_id, service_id)
            .await?
            .and_then(|svc| svc.get("endpoint").and_then(|v| v.as_str()).map(str::to_string))
            .filter(|s| !s.is_empty());
    super::negotiate::save_designated_provider_with_endpoint(
        job_id,
        provider_agent_id,
        resolved_endpoint.as_deref(),
    )?;

    audit::log(
        "cli",
        "user/set_asp",
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
        "user/reset_asp",
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
        "user/user_reject",
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

/// Render the compact per-service preflight summary line for text mode, e.g.
/// `Trading-signal service: yes · classes: prediction · tools: Polymarket(missing), Trade Kit(verification_unknown) · 1 reminder`.
/// Returns `None` when the preflight object is absent.
///
/// NOTE: this line MUST NOT use `Copy-trade: on/off` wording — that reads as an
/// "auto-trading already authorized" switch, which is misleading. Service
/// classification is advisory; the saved delivery is interpreted later by the
/// subscription-signal Skill, and real execution still requires consent,
/// per-trade cap, tool readiness and any required account confirmation.
/// We therefore surface a neutral `Trading-signal service: yes/no` instead.
fn preflight_summary_line(pf: &serde_json::Value) -> Option<String> {
    if !pf.is_object() {
        return None;
    }
    // Neutral, non-authorizing phrasing. Missing/wrong-type values fail closed.
    let is_signal = pf["isTradingSignal"].as_bool().unwrap_or(false);
    let service = if is_signal { "yes" } else { "no" };
    let classes: Vec<String> = pf["assetClasses"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let classes_str = if classes.is_empty() {
        "—".to_string()
    } else {
        classes.join(", ")
    };
    let tools: Vec<String> = pf["tools"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| {
                    let name = t["displayName"].as_str().unwrap_or("?");
                    let readiness = t["readiness"].as_str().unwrap_or("?");
                    format!("{name}({readiness})")
                })
                .collect()
        })
        .unwrap_or_default();
    let tools_str = if tools.is_empty() {
        "none".to_string()
    } else {
        tools.join(", ")
    };
    let n = pf["reminders"].as_array().map(|a| a.len()).unwrap_or(0);
    let reminders_str = if n == 1 {
        "1 reminder".to_string()
    } else {
        format!("{n} reminders")
    };
    Some(format!(
        "Trading-signal service: {service} · classes: {classes_str} · tools: {tools_str} · {reminders_str}"
    ))
}

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
    use serde_json::json;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::super::TaskCommand,
    }

    // ── asp-match ───────────────────────────────────────────────────

    #[test]
    fn normalizes_numeric_subscription_fee_when_fee_amount_is_null() {
        let mut service = json!({
            "feeAmount": null,
            "subscription": [{"fee": 0.1, "interval": "month"}],
        });
        super::normalize_subscription_fee(&mut service);
        assert_eq!(service["feeAmount"], json!(0.1));
        assert_eq!(super::scalar_display(&service["feeAmount"]).as_deref(), Some("0.1"));
    }

    #[test]
    fn subscription_fee_wins_and_monthly_fallback_is_preferred() {
        let mut subscription_service = json!({
            "feeAmount": "2.5",
            "subscription": [{"fee": 0.1, "interval": "month"}],
        });
        super::normalize_subscription_fee(&mut subscription_service);
        assert_eq!(subscription_service["feeAmount"], json!(0.1));

        let mut fallback = json!({
            "subscription": [
                {"fee": 0.02, "interval": "day"},
                {"fee": "0.6", "interval": "month"}
            ],
        });
        super::normalize_subscription_fee(&mut fallback);
        assert_eq!(fallback["feeAmount"], json!("0.6"));
    }

    #[test]
    fn trial_support_reads_support_trial_only() {
        assert!(super::supports_trial(&json!({"supportTrial": true})));
        assert!(!super::supports_trial(&json!({})));
    }

    #[test]
    fn compact_response_preserves_ai_fields_and_derives_subscription_info() {
        let resp = json!({
            "recommendations": [{
                "providerAgentId": "6607",
                "providerAgentName": "Quick ASP",
                "securityRate": 4.8,
                "feedbackRate": 4.9,
                "soldCount": 12,
                "supportA2MCP": false,
                "avatar": "https://example.invalid/avatar.png",
                "services": [{
                    "serviceId": "svc-1",
                    "serviceName": "Quick Moment",
                    "serviceType": "A2A",
                    "serviceDescription": "Please provide the target market before subscribing.",
                    "feeAmount": "100",
                    "feeToken": "0xToken",
                    "feeTokenSymbol": "USDT",
                    "endpoint": null,
                    "supportTrial": true,
                    "freeTrial": 24,
                    "subscription": [
                        {"interval": "week", "fee": "0.03"},
                        {"interval": "month", "fee": "0.1"}
                    ],
                    "autoTradePreflight": {
                        "schemaVersion": 2,
                        "isTradingSignal": true,
                        "assetClasses": ["spot"],
                        "tools": [{
                            "displayName": "Trade Kit",
                            "readiness": "verification_unknown",
                            "reason": "authorization_not_checked",
                            "checkedAt": null
                        }],
                        "reminders": [],
                        "tradeKitProbe": {
                            "mode": "deferred_until_venue_selection",
                            "assetClasses": ["spot"]
                        }
                    },
                    "rawServiceStatus": "ACTIVE"
                }]
            }],
            "nextPage": null,
            "debug": {"requestId": "abc"}
        });

        let compact = super::compact_asp_match_response(resp);
        let rec = &compact["recommendations"][0];
        let svc = &rec["services"][0];

        assert_eq!(rec["providerAgentId"], json!("6607"));
        assert!(rec.get("avatar").is_none());
        assert!(compact.get("debug").is_none());

        assert_eq!(svc["serviceDescription"], json!("Please provide the target market before subscribing."));
        assert_eq!(svc["autoTradePreflight"]["schemaVersion"], json!(2));
        assert_eq!(svc["autoTradePreflight"]["assetClasses"], json!(["spot"]));
        assert_eq!(
            svc["autoTradePreflight"]["tools"][0]["readiness"],
            json!("verification_unknown")
        );
        assert_eq!(
            svc["autoTradePreflight"]["tools"][0]["reason"],
            json!("authorization_not_checked")
        );
        assert!(svc.get("feeAmount").is_none());
        assert_eq!(svc["supportSubscription"], json!(true));
        assert_eq!(svc["subscriptionInfo"]["interval"], json!("month"));
        assert_eq!(svc["subscriptionInfo"]["feeAmount"], json!("0.1"));
        assert_eq!(svc["subscriptionInfo"]["supportTrial"], json!(true));
        assert_eq!(svc["subscriptionInfo"]["freeTrial"], json!(24));
        assert!(svc.get("supportTrial").is_none());
        assert!(svc.get("freeTrial").is_none());
        assert!(svc.get("subscription").is_none());
        assert!(svc.get("rawServiceStatus").is_none());
    }

    #[test]
    fn compact_response_marks_regular_services_without_subscription_info() {
        let resp = json!({
            "recommendations": [{
                "providerAgentId": "42",
                "services": [{
                    "serviceId": "svc-2",
                    "serviceName": "One-shot Audit",
                    "serviceType": "A2A",
                    "serviceDescription": "Audit one transaction.",
                    "feeAmount": "5",
                    "feeToken": "0xToken",
                    "feeTokenSymbol": "USDT",
                    "autoTradePreflight": {"isTradingSignal": false}
                }]
            }]
        });

        let compact = super::compact_asp_match_response(resp);
        let svc = &compact["recommendations"][0]["services"][0];

        assert_eq!(svc["supportSubscription"], json!(false));
        assert!(svc["subscriptionInfo"].is_null());
        assert_eq!(svc["feeAmount"], json!("5"));
        assert_eq!(svc["serviceDescription"], json!("Audit one transaction."));
        assert_eq!(svc["autoTradePreflight"]["isTradingSignal"], json!(false));
        assert!(svc.get("subscription").is_none());
    }

    #[test]
    fn format_provider_with_name() {
        // FR-3.2: name present → `Agent <id>(<name>)`.
        assert_eq!(super::format_provider("1506", "AlphaBot"), "Agent 1506(AlphaBot)");
    }

    #[test]
    fn format_provider_empty_name_degrades() {
        // FR-3.3: name empty/absent → degrade to `Agent <id>` (no parentheses).
        assert_eq!(super::format_provider("1506", ""), "Agent 1506");
    }

    #[test]
    fn cli_asp_match_task_desc_only() {
        let cli = TestCli::parse_from([
            "test", "asp-match", "--task-desc", "build a trading bot",
        ]);
        match cli.cmd {
            super::super::TaskCommand::AspMatch { task_desc, job_id, provider_agent_id, payment_token_amount, page, agent_id, format } => {
                assert_eq!(task_desc, "build a trading bot");
                assert!(job_id.is_none());
                assert!(provider_agent_id.is_none());
                assert!(payment_token_amount.is_none());
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

    #[test]
    fn cli_asp_match_with_payment_token_amount() {
        let cli = TestCli::parse_from([
            "test", "asp-match",
            "--task-desc", "audit service",
            "--payment-token-amount", "0.7",
        ]);
        match cli.cmd {
            super::super::TaskCommand::AspMatch { task_desc, payment_token_amount, .. } => {
                assert_eq!(task_desc, "audit service");
                assert_eq!(payment_token_amount, Some(0.7));
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
            "test", "user-reject", "job-rej", "--agent-id", "user-42",
        ]);
        match cli.cmd {
            super::super::TaskCommand::UserReject { job_id, agent_id } => {
                assert_eq!(job_id, "job-rej");
                assert_eq!(agent_id.as_deref(), Some("user-42"));
            }
            _ => panic!("expected UserReject"),
        }
    }

    #[test]
    fn cli_user_reject_missing_job_id_fails() {
        assert!(TestCli::try_parse_from(["test", "user-reject"]).is_err());
    }

    // ── create-task: --visibility removed (public task type deleted) ────

    #[test]
    fn cli_create_rejects_visibility_flag() {
        // `--visibility` no longer exists on create-task; supplying it is a clap parse error (AC-3).
        // All other required flags are provided so `--visibility` is the sole cause of the error.
        assert!(TestCli::try_parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--provider", "agent-1",
            "--service-id", "svc-1",
            "--payment-mode", "escrow",
            "--visibility", "0",
        ]).is_err());
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
            "--payment-mode", "escrow",
            "--service-params", "参数：x=1",
            "--service-token-address", "0xAddr",
            "--service-token-amount", "5.0",
        ]);
        match cli.cmd {
            super::super::TaskCommand::Create {
                provider, service_id, payment_mode, service_params,
                service_token_address, service_token_amount, ..
            } => {
                assert_eq!(provider, "agent-1");
                assert_eq!(service_id, "svc-1");
                assert_eq!(payment_mode, "escrow");
                assert_eq!(service_params.as_deref(), Some("参数：x=1"));
                assert_eq!(service_token_address.as_deref(), Some("0xAddr"));
                assert_eq!(service_token_amount.as_deref(), Some("5.0"));
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_requires_provider_service_id_payment_mode() {
        // --provider, --service-id, --payment-mode are all required for create-task
        // (oli-feedback). Omitting them is a clap parse error.
        let base = [
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
        ];
        // Missing all three required flags.
        assert!(TestCli::try_parse_from(base).is_err());
        // Missing --payment-mode only.
        assert!(TestCli::try_parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--provider", "agent-1",
            "--service-id", "svc-1",
        ]).is_err());
        // Missing --service-id only.
        assert!(TestCli::try_parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--provider", "agent-1",
            "--payment-mode", "escrow",
        ]).is_err());
        // Missing --provider only.
        assert!(TestCli::try_parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--service-id", "svc-1",
            "--payment-mode", "escrow",
        ]).is_err());
        // All three present -> parses OK.
        assert!(TestCli::try_parse_from([
            "test", "create",
            "--description", "a long enough description text",
            "--budget", "10", "--max-budget", "20",
            "--currency", "USDT",
            "--provider", "agent-1",
            "--service-id", "svc-1",
            "--payment-mode", "escrow",
        ]).is_ok());
    }

    // ── preflight_summary_line — text-mode compact line (FR-1) ──────────
    //
    // Asserts the neutral, non-authorizing format (oli-feedback P0): the line
    // renders `Trading-signal service: yes/no`, never `Copy-trade: on/off`.
    //   `Trading-signal service: yes · classes: prediction · tools: Polymarket(missing), Trade Kit(verification_unknown) · 1 reminder`

    #[test]
    fn preflight_line_signal_singular_reminder() {
        // isTradingSignal=true → "yes"; classes joined by ", "; tools "Name(readiness)"
        // joined by ", "; a single reminder renders the singular "1 reminder".
        let pf = serde_json::json!({
            "isTradingSignal": true,
            "assetClasses": ["prediction"],
            "tools": [
                {"displayName": "Polymarket", "readiness": "missing"},
                {"displayName": "Trade Kit", "readiness": "verification_unknown"}
            ],
            "reminders": [{"kind": "install_plugin"}]
        });
        let line = super::preflight_summary_line(&pf).unwrap();
        assert_eq!(
            line,
            "Trading-signal service: yes · classes: prediction · tools: Polymarket(missing), Trade Kit(verification_unknown) · 1 reminder"
        );
        // The misleading on/off authorization wording must never appear.
        assert!(!line.contains("Copy-trade"));
    }

    #[test]
    fn preflight_line_non_signal_empty_classes_and_tools() {
        // isTradingSignal=false → "no"; empty assetClasses → em-dash placeholder;
        // empty tools → "none"; zero reminders → plural "0 reminders".
        let pf = serde_json::json!({
            "isTradingSignal": false,
            "assetClasses": [],
            "tools": [],
            "reminders": []
        });
        let line = super::preflight_summary_line(&pf).unwrap();
        assert_eq!(
            line,
            "Trading-signal service: no · classes: — · tools: none · 0 reminders"
        );
        assert!(!line.contains("Copy-trade"));
    }

    #[test]
    fn preflight_line_plural_reminders_multi_class() {
        // Multiple classes join with ", "; N (≠1) reminders render the plural form.
        let pf = serde_json::json!({
            "isTradingSignal": true,
            "assetClasses": ["spot", "perp"],
            "tools": [{"displayName": "OnchainOS", "readiness": "ready"}],
            "reminders": [{"kind": "choose_at_first_signal"}, {"kind": "configure_tool"}]
        });
        let line = super::preflight_summary_line(&pf).unwrap();
        assert_eq!(
            line,
            "Trading-signal service: yes · classes: spot, perp · tools: OnchainOS(ready) · 2 reminders"
        );
        assert!(!line.contains("Copy-trade"));
    }

    #[test]
    fn preflight_line_non_object_returns_none() {
        // A non-object preflight (absent / wrong type) yields no summary line.
        assert!(super::preflight_summary_line(&serde_json::Value::Null).is_none());
        assert!(super::preflight_summary_line(&serde_json::json!("x")).is_none());
        assert!(super::preflight_summary_line(&serde_json::json!([1, 2])).is_none());
    }
}
