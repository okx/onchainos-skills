//! Raise dispute (ASP) step 2 — onchainos agent dispute confirm <jobId>
//!
//! Step 2 of the two-stage on-chain dispute flow. Preconditions:
//!   1. `dispute raise` has been run (stage 1 approve on-chain)
//!   2. On-chain `dispute_approved` system notification has been received
//!
//! This command calls POST /aieco/task/{jobId}/dispute → uopData → sign + broadcast.
//! After completion, wait for the on-chain `job_disputed` notification, then call next-action to enter the evidence preparation window.

use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;

fn ensure_one_time_task(detail: &serde_json::Value) -> Result<()> {
    let job_type = detail["jobType"].as_i64().or_else(|| {
        detail["jobType"]
            .as_str()
            .and_then(|value| value.parse().ok())
    });
    match job_type {
        Some(0) => Ok(()),
        Some(1) => bail!(
            "dispute confirm is not valid for a subscription task. Subscription arbitration is created in one step by `subscribe-dispute`; do not submit a second dispute transaction. Reconcile the subscription status instead"
        ),
        Some(other) => bail!(
            "dispute confirm requires a one-time task (jobType=0); backend returned unsupported jobType={other}"
        ),
        None => bail!(
            "dispute confirm requires a fresh task detail with jobType=0; backend response did not include jobType"
        ),
    }
}

pub async fn handle_dispute_confirm(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
    agent_id: &str,
) -> Result<()> {
    crate::commands::agent_commerce::task::arbitration_trace::record(
        "dispute-confirm-input",
        job_id,
        &serde_json::json!({
            "command": "agent dispute confirm",
            "agentId": agent_id,
            "reason": reason,
            "reasonChars": reason.chars().count(),
        }),
        Some(&serde_json::json!({"received": true})),
        None,
    );
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Dispute reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }

    // Fail closed before requesting uopData. Subscription arbitration uses the
    // one-shot `subscribe-dispute` endpoint and must never enter task phase 2.
    let task_detail = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
        .context("dispute confirm (stage 2): fresh task detail request failed")?;
    ensure_one_time_task(&task_detail)?;

    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({});

    let dispute_resp = client
        .post_with_identity(&client.endpoint(job_id, "dispute"), &body, agent_id)
        .await
        .context("dispute confirm (stage 2): dispute API request failed")?;

    let reason_json = serde_json::json!({ "reason": reason });
    let dispute_tx = signing::sign_uop_and_broadcast(
        client,
        &dispute_resp["uopData"],
        &account_id,
        &address,
        job_id,
        signing::extract_biz_type(&dispute_resp),
        agent_id,
        Some(&reason_json),
    )
    .await
    .context("dispute confirm (stage 2): dispute on-chain broadcast failed")?;

    crate::commands::agent_commerce::task::arbitration_trace::record(
        "dispute-confirm-complete",
        job_id,
        &serde_json::json!({"agentId": agent_id, "reason": reason}),
        Some(&serde_json::json!({"txHash": dispute_tx, "waitFor": "job_disputed"})),
        None,
    );

    audit::log(
        "cli",
        "ASP/dispute_confirm_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={dispute_tx}"),
        ]),
        None,
    );

    println!("✓ Dispute stage 2: dispute on-chain");
    println!("  txHash: {dispute_tx}");
    println!();
    println!("✓ Arbitration transaction submitted; wait for `job_disputed` to start the evidence workflow");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn one_time_task_may_enter_dispute_confirm() {
        assert!(ensure_one_time_task(&json!({"jobType": 0})).is_ok());
        assert!(ensure_one_time_task(&json!({"jobType": "0"})).is_ok());
    }

    #[test]
    fn subscription_task_must_not_enter_dispute_confirm() {
        let error = ensure_one_time_task(&json!({"jobType": 1})).unwrap_err();
        assert!(error.to_string().contains("subscription task"));
        assert!(error
            .to_string()
            .contains("do not submit a second dispute transaction"));
    }

    #[test]
    fn missing_or_unknown_job_type_fails_closed() {
        assert!(ensure_one_time_task(&json!({})).is_err());
        assert!(ensure_one_time_task(&json!({"jobType": 9})).is_err());
    }
}
