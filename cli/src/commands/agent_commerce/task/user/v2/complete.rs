use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::{
    deliverables, network::task_api_client::TaskApiClient, PaymentMode,
};
use crate::commands::agent_commerce::task::signing;

pub(crate) async fn handle(client: &mut TaskApiClient, job_id: &str) -> Result<serde_json::Value> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
    let task = client
        .get_with_identity(&client.task_path(job_id), &agent_id)
        .await?;
    let payment_mode = PaymentMode::from_int(task["paymentMode"].as_i64().unwrap_or(0) as i32);

    let (tx_hash, payment_mode) = if payment_mode == PaymentMode::Escrow {
        crate::commands::agent_commerce::task::common::review_gate::check_and_consume(job_id)?;
        let result = signing::task_dual_sign_and_broadcast(
            client,
            job_id,
            "pre-complete",
            "complete",
            None,
            &account_id,
            &address,
            &agent_id,
            None,
        )
        .await?;
        (result.tx_hash, 1)
    } else {
        let has_deliverable = deliverables::read_manifest("user", job_id)
            .ok()
            .flatten()
            .is_some_and(|manifest| !manifest.entries.is_empty());
        if !has_deliverable {
            return Ok(no_deliverable_result(job_id));
        }

        let response = client
            .post_with_identity(
                &client.endpoint(job_id, "direct/complete"),
                &serde_json::json!({}),
                &agent_id,
            )
            .await?;
        let tx_hash = signing::sign_uop_and_broadcast(
            client,
            &response["uopData"],
            &account_id,
            &address,
            job_id,
            signing::extract_biz_type(&response),
            &agent_id,
            None,
        )
        .await?;
        (tx_hash, 3)
    };

    audit::log(
        "cli",
        "user/complete_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("paymentMode={payment_mode}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    Ok(submitted_result(job_id, &tx_hash))
}

fn submitted_result(job_id: &str, tx_hash: &str) -> serde_json::Value {
    serde_json::json!({
        "phase": "deliverable_review",
        "decision": "ready",
        "reason": "completion_submitted",
        "nextAction": [{ "id": "stop" }],
        "payload": {
            "jobId": job_id,
            "txHash": tx_hash,
        },
    })
}

fn no_deliverable_result(job_id: &str) -> serde_json::Value {
    serde_json::json!({
        "phase": "deliverable_review",
        "decision": "blocked",
        "reason": "x402_no_deliverable",
        "nextAction": [{ "id": "stop" }],
        "payload": { "jobId": job_id },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_success_is_a_minimal_structured_result() {
        let output = submitted_result("job-1", "0xtx");

        assert_eq!(output["phase"], "deliverable_review");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["reason"], "completion_submitted");
        assert_eq!(output["nextAction"][0]["id"], "stop");
        assert_eq!(output["payload"]["jobId"], "job-1");
        assert_eq!(output["payload"]["txHash"], "0xtx");
        assert_eq!(output["payload"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn x402_without_deliverable_is_blocked() {
        let output = no_deliverable_result("job-1");

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "x402_no_deliverable");
        assert_eq!(output["nextAction"][0]["id"], "stop");
        assert_eq!(output["payload"]["jobId"], "job-1");
    }
}
