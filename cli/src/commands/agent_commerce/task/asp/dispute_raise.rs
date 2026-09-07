//! Request evaluation for a one-time task with one combined approve-and-create call.
//!
//! The command sends the exact ASP reason to the task session before broadcasting
//! `approveAndCreateDispute`. The later `job_disputed` event uses that reason when
//! it assembles and uploads evidence.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL, Engine as _};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
};
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;
const ARBITRATION_REASON_CONTEXT: &str = "[ARBITRATION_REASON_CONTEXT]";

#[derive(Clone, Copy)]
enum ReasonHandoffFlow {
    OneTime,
    Subscription,
}

fn build_reason_handoff_for(
    job_id: &str,
    provider_agent_id: &str,
    reason: &str,
    flow: ReasonHandoffFlow,
) -> String {
    let reason_b64 = BASE64_URL.encode(reason.as_bytes());
    let mut context = serde_json::json!({
        "version": 1,
        "intent": "arbitration_reason_context",
        "jobId": job_id,
        "providerAgentId": provider_agent_id,
        "reason": reason,
        "reasonB64": reason_b64.clone(),
    });

    let instruction = match flow {
        ReasonHandoffFlow::OneTime => {
            context["taskType"] = serde_json::json!("one_time");
            context["resumeEvent"] = serde_json::json!("job_disputed");
            "Keep this exact reason in the current task conversation and end this turn. \
             When the matching job_disputed event arrives, include it as the ASP's \
             evaluation reason in the evidence upload."
        }
        ReasonHandoffFlow::Subscription => {
            context["taskType"] = serde_json::json!("subscription");
            context["resumeEvent"] = serde_json::json!("sub_asp_dispute");
            "Keep this exact reason in the current task conversation and end this turn. \
             When the matching sub_asp_dispute event arrives, include it as the ASP's \
             evaluation reason in the evidence upload."
        }
    };

    format!("{ARBITRATION_REASON_CONTEXT}\n{context}\n{instruction}")
}

fn build_reason_handoff(job_id: &str, provider_agent_id: &str, reason: &str) -> String {
    build_reason_handoff_for(
        job_id,
        provider_agent_id,
        reason,
        ReasonHandoffFlow::OneTime,
    )
}

pub(super) fn build_subscription_reason_handoff(
    job_id: &str,
    provider_agent_id: &str,
    reason: &str,
) -> String {
    build_reason_handoff_for(
        job_id,
        provider_agent_id,
        reason,
        ReasonHandoffFlow::Subscription,
    )
}

pub async fn handle_dispute_raise(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }
    if reason.trim().is_empty() {
        bail!("Evaluation reason is required. Pass the provided evaluation reason with --reason.");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Evaluation reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;

    // Dispute deposit precheck: wallet's matching token balance must be ≥ 5% of the job amount.
    // Insufficient balance bails immediately to avoid wasting gas on later approve / dispute on-chain txs.
    let task_path = client.task_path(job_id);
    let task_resp = client
        .get_with_identity(&task_path, agent_id)
        .await
        .context("dispute raise: failed to fetch task details (deposit precheck)")?;
    let task_amount: f64 = task_resp["tokenAmount"]
        .as_str()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0.0);
    let token_symbol = task_resp["tokenSymbol"].as_str().unwrap_or("?");
    if task_amount > 0.0 {
        let required = task_amount * 0.05;
        if let Err(e) = common::ensure_sufficient_balance_at(required, token_symbol, &address).await
        {
            // Preserve the dispute-bond framing, then enrich with the ASP signing
            // account's deposit address + stderr QR (FR-2 — explicit address, no
            // agentId resolution). enrich_blocking_at folds the full `{:#}` chain
            // (incl. this context) into the error message byte-for-byte.
            let e = e.context(format!(
                "Requesting evaluation requires a deposit >= 5% of the task amount ({required} {token_symbol}; task amount {task_amount} {token_symbol})"
            ));
            let enriched = common::deposit_qr::enrich_blocking_at(e, &address);
            return print_dispute_funding_block_from_error(enriched);
        }
    }

    let body = serde_json::json!({});

    // One-time and subscription evaluations use the same combined endpoint.
    let evaluation_path = client.endpoint(job_id, "dispute/approveAndCreateDispute");
    let evaluation_resp = client
        .post_with_identity(&evaluation_path, &body, agent_id)
        .await
        .context("dispute raise: approveAndCreateDispute API request failed")?;

    // Hand the exact reason to the existing task session before the combined
    // transaction is broadcast. The future `job_disputed` event is delivered to
    // that session, so it can reuse the main-session reason without machine storage.
    let buyer_agent_id = task_resp["buyerAgentId"]
        .as_str()
        .or_else(|| task_resp["userAgentId"].as_str())
        .filter(|value| !value.trim().is_empty())
        .context("dispute raise: task detail missing buyerAgentId for reason handoff")?;
    let reason_handoff = build_reason_handoff(job_id, agent_id, reason);
    common::okx_a2a::session_send(job_id, Some(buyer_agent_id), &reason_handoff).context(
        "dispute raise: failed to hand off the evaluation reason to the task session; combined transaction was not broadcast",
    )?;

    let reason_json = serde_json::json!({ "reason": reason });
    let evaluation_tx = signing::sign_uop_and_broadcast(
        client,
        &evaluation_resp["uopData"],
        &account_id,
        &address,
        job_id,
        signing::extract_biz_type(&evaluation_resp),
        agent_id,
        Some(&reason_json),
    )
    .await
    .context("dispute raise: approveAndCreateDispute on-chain broadcast failed")?;

    audit::log(
        "cli",
        "ASP/evaluation_requested",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("reasonLen={}", reason.chars().count()),
            format!("txHash={evaluation_tx}"),
        ]),
        None,
    );

    println!("✓ Evaluation request submitted");
    println!("  Progress will update in this task.");
    println!("  Check: onchainos agent arbitration-detail {job_id} --agent-id {agent_id}");
    Ok(())
}

fn print_dispute_funding_block_from_error(err: anyhow::Error) -> Result<()> {
    match err.downcast_ref::<common::deposit_qr::InsufficientBalanceError>() {
        Some(ib) => {
            let mut warning = common::deposit_qr::balance_warning_base(ib);
            if let Some(address) = ib.deposit_address.as_deref() {
                warning["depositAddress"] = serde_json::Value::String(address.to_string());
                warning["depositChain"] = serde_json::Value::String(ib.deposit_chain.clone());
            }
            Err(crate::output::CliFundingBlocked {
                data: common::funding_notice::funding_blocked_envelope(
                    &warning,
                    "dispute-bond",
                    "Evaluation bond",
                ),
            }
            .into())
        }
        None => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_time_reason_handoff_preserves_raw_reason_for_evidence() {
        let reason = "The delivered output missed the requested scope; $(touch /tmp/nope)";
        let content = build_reason_handoff("job-1", "asp-1", reason);
        let mut lines = content.lines();
        assert_eq!(lines.next(), Some(ARBITRATION_REASON_CONTEXT));
        let payload: serde_json::Value =
            serde_json::from_str(lines.next().expect("context json")).unwrap();
        assert_eq!(payload["jobId"], "job-1");
        assert_eq!(payload["providerAgentId"], "asp-1");
        assert_eq!(payload["reason"], reason);

        let encoded = payload["reasonB64"].as_str().unwrap();
        assert!(encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
        assert_eq!(payload["taskType"], "one_time");
        assert_eq!(payload["resumeEvent"], "job_disputed");
        assert!(payload.get("confirmArgs").is_none());
        assert_eq!(
            BASE64_URL.decode(encoded).unwrap(),
            reason.as_bytes(),
            "the task session must recover the exact main-session reason"
        );
    }

    #[test]
    fn subscription_reason_handoff_preserves_raw_reason_for_evidence() {
        let reason =
            "The subscription output missed the agreement: keep + / = and a newline\nsecond line";
        let content = build_subscription_reason_handoff("sub-1", "asp-1", reason);
        let mut lines = content.lines();
        assert_eq!(lines.next(), Some(ARBITRATION_REASON_CONTEXT));
        let payload: serde_json::Value =
            serde_json::from_str(lines.next().expect("context json")).unwrap();
        assert_eq!(payload["jobId"], "sub-1");
        assert_eq!(payload["providerAgentId"], "asp-1");
        assert_eq!(payload["taskType"], "subscription");
        assert_eq!(payload["resumeEvent"], "sub_asp_dispute");
        assert_eq!(payload["reason"], reason);
        assert!(payload.get("confirmArgs").is_none());

        let encoded = payload["reasonB64"].as_str().unwrap();
        assert_eq!(BASE64_URL.decode(encoded).unwrap(), reason.as_bytes());
    }
}
