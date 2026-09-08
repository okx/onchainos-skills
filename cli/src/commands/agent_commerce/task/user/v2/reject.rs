use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

const MAX_REASON_CHARS: usize = 2000;
const JOB_TYPE_SUBSCRIBE: i64 = 1;

pub(crate) fn validate_rejection_reason(reason: &str) -> Result<&str> {
    let reason = reason.trim();
    if reason.is_empty() {
        bail!("--reason is required for reject");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Reject reason exceeds {MAX_REASON_CHARS} characters");
    }
    Ok(reason)
}

pub(crate) async fn handle(
    client: &mut TaskApiClient,
    job_id: &str,
    reason: &str,
) -> Result<serde_json::Value> {
    let reason = validate_rejection_reason(reason)?;
    let (local_agent_id, _) = super::super::create::resolve_user_agent().await?;
    let task = client
        .get_with_identity(&client.task_path(job_id), &local_agent_id)
        .await?;
    let job_type = task["jobType"]
        .as_i64()
        .or_else(|| {
            task["jobType"]
                .as_str()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(0);

    let tx_hash = if job_type == JOB_TYPE_SUBSCRIBE {
        super::super::subscription_ops::handle_subscribe_reject_inner(
            client,
            job_id,
            reason,
            &local_agent_id,
        )
        .await?
    } else {
        let (account_id, address, agent_id) =
            signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
        let reason_json = serde_json::json!({ "reason": reason });
        let result = signing::task_dual_sign_and_broadcast(
            client,
            job_id,
            "pre-reject",
            "reject",
            None,
            &account_id,
            &address,
            &agent_id,
            Some(&reason_json),
        )
        .await?;

        audit::log(
            "cli",
            "user/reject_submitted",
            true,
            Duration::default(),
            Some(vec![
                format!("jobId={job_id}"),
                format!("agentId={agent_id}"),
                format!("reasonLen={}", reason.chars().count()),
                format!("txHash={}", result.tx_hash),
            ]),
            None,
        );
        result.tx_hash
    };

    Ok(submitted_result(job_id, &tx_hash))
}

fn submitted_result(job_id: &str, tx_hash: &str) -> serde_json::Value {
    serde_json::json!({
        "phase": "deliverable_review",
        "decision": "ready",
        "reason": "rejection_submitted",
        "nextAction": [{ "id": "stop" }],
        "payload": {
            "jobId": job_id,
            "txHash": tx_hash,
        },
    })
}

pub(crate) fn reason_required_result(
    job_id: &str,
    agent_id: &str,
    short_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "phase": "deliverable_review",
        "decision": "requires_user_input",
        "reason": "rejection_reason_required",
        "nextAction": [{
            "id": "request_rejection_reason",
            "recommend": true,
            "params": {
                "jobId": job_id,
                "agentId": agent_id,
                "shortJobId": short_id,
            },
        }],
        "payload": { "requiredParams": ["reason"] },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::agent_commerce::AgentCommand;
    use crate::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn reject_success_is_a_minimal_structured_result() {
        let output = submitted_result("job-1", "0xtx");

        assert_eq!(output["phase"], "deliverable_review");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["reason"], "rejection_submitted");
        assert_eq!(output["nextAction"][0]["id"], "stop");
        assert_eq!(output["payload"]["jobId"], "job-1");
        assert_eq!(output["payload"]["txHash"], "0xtx");
        assert_eq!(output["payload"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn missing_reason_returns_only_the_required_input() {
        let output = reason_required_result("job-1", "user-1", "job-1");

        assert_eq!(output["decision"], "requires_user_input");
        assert_eq!(output["reason"], "rejection_reason_required");
        assert_eq!(output["nextAction"][0]["id"], "request_rejection_reason");
        assert_eq!(output["nextAction"][0]["params"]["jobId"], "job-1");
        assert_eq!(output["nextAction"][0]["params"]["agentId"], "user-1");
        assert_eq!(output["nextAction"][0]["params"]["shortJobId"], "job-1");
        assert_eq!(
            output["payload"]["requiredParams"],
            serde_json::json!(["reason"])
        );
        assert_eq!(output["payload"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn reject_requires_a_non_blank_reason() {
        let error = validate_rejection_reason("  \n\t ")
            .expect_err("blank rejection reasons must be rejected");
        assert!(error.to_string().contains("--reason is required"));
    }

    #[test]
    fn reject_limits_reason_length() {
        let reason = "a".repeat(MAX_REASON_CHARS + 1);
        assert!(validate_rejection_reason(&reason).is_err());
    }

    #[test]
    fn review_cli_names_and_arguments_remain_stable() {
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let complete = Cli::try_parse_from(["onchainos", "agent", "complete", "job-1"])
                    .expect("complete command must keep its existing shape");
                assert!(matches!(
                    complete.command,
                    Commands::Agent {
                        command: AgentCommand::Complete { job_id }
                    } if job_id == "job-1"
                ));

                let reject = Cli::try_parse_from([
                    "onchainos",
                    "agent",
                    "reject",
                    "job-1",
                    "--reason",
                    "deliverable is incomplete",
                ])
                .expect("reject command must keep its existing shape");
                assert!(matches!(
                    reject.command,
                    Commands::Agent {
                        command: AgentCommand::Reject { job_id, reason }
                    } if job_id == "job-1" && reason == "deliverable is incomplete"
                ));

                assert!(Cli::try_parse_from(["onchainos", "agent", "reject", "job-1"]).is_err());
            })
            .expect("review CLI contract test thread must start")
            .join()
            .expect("review CLI contract test thread must complete");
    }
}
