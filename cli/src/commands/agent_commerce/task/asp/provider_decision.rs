//! V2 designated-provider accept/decline mutations for tasks and subscriptions.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

const JOB_ACCEPT_BIZ_TYPE: i64 = 203;
const JOB_DECLINE_BIZ_TYPE: i64 = 202;
const SUB_ACCEPT_BIZ_TYPE: i64 = 205;
const SUB_DECLINE_BIZ_TYPE: i64 = 206;
const MAX_DECLINE_REASON_CHARS: usize = 512;

#[derive(Clone, Copy)]
enum DecisionKind {
    AcceptJob,
    DeclineJob,
    AcceptSubscription,
    DeclineSubscription,
}

impl DecisionKind {
    fn biz_type(self) -> i64 {
        match self {
            Self::AcceptJob => JOB_ACCEPT_BIZ_TYPE,
            Self::DeclineJob => JOB_DECLINE_BIZ_TYPE,
            Self::AcceptSubscription => SUB_ACCEPT_BIZ_TYPE,
            Self::DeclineSubscription => SUB_DECLINE_BIZ_TYPE,
        }
    }

    fn action(self) -> &'static str {
        match self {
            Self::AcceptJob => "acceptJobByProvider",
            Self::DeclineJob => "declineJobByProvider",
            Self::AcceptSubscription => "acceptSubscription",
            Self::DeclineSubscription => "declineSubscription",
        }
    }

    fn is_subscription(self) -> bool {
        matches!(self, Self::AcceptSubscription | Self::DeclineSubscription)
    }

    fn is_decline(self) -> bool {
        matches!(self, Self::DeclineJob | Self::DeclineSubscription)
    }

    fn path(self, client: &TaskApiClient, job_id: &str) -> String {
        if self.is_subscription() {
            format!("{}/{}", client.subscribe_path(job_id), self.action())
        } else {
            client.endpoint(job_id, self.action())
        }
    }
}

fn validate_inputs(
    job_id: &str,
    agent_id: &str,
    kind: DecisionKind,
    reason: Option<&str>,
) -> Result<()> {
    if job_id.trim().is_empty() {
        bail!("jobId is required");
    }
    if agent_id.trim().is_empty() {
        bail!("--agent-id is required");
    }
    if kind.is_decline() {
        let reason = reason
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("--reason is required for provider decline"))?;
        if reason.chars().count() > MAX_DECLINE_REASON_CHARS {
            bail!("--reason exceeds {MAX_DECLINE_REASON_CHARS} Unicode characters");
        }
    }
    Ok(())
}

fn validate_response(job_id: &str, kind: DecisionKind, value: &Value) -> Result<()> {
    let returned_job_id = value["jobId"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{} response missing jobId", kind.action()))?;
    if returned_job_id != job_id {
        bail!(
            "{} returned jobId {returned_job_id}, expected {job_id}",
            kind.action()
        );
    }
    if value.get("uopData").is_none() || value["uopData"].is_null() {
        bail!("{} response missing uopData", kind.action());
    }
    let biz_type = signing::extract_biz_type(value);
    if biz_type != kind.biz_type() {
        bail!(
            "{} returned bizType {biz_type}, expected {}",
            kind.action(),
            kind.biz_type()
        );
    }
    Ok(())
}

fn detail_status(kind: DecisionKind, detail: &Value) -> Option<i64> {
    if kind.is_subscription() {
        detail["subStatus"]
            .as_i64()
            .or_else(|| detail["status"].as_i64())
    } else {
        detail["status"].as_i64()
    }
}

async fn execute(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    kind: DecisionKind,
    reason: Option<&str>,
) -> Result<()> {
    validate_inputs(job_id, agent_id, kind, reason)?;
    let has_session_cert = crate::wallet_store::load_session()?
        .is_some_and(|session| !session.session_cert.trim().is_empty());
    if !has_session_cert {
        bail!("current login has no sessionCert; run `onchainos wallet login` again");
    }
    let detail_path = if kind.is_subscription() {
        client.subscribe_path(job_id)
    } else {
        client.task_path(job_id)
    };
    let detail = client
        .get_with_identity(&detail_path, agent_id)
        .await
        .with_context(|| {
            format!(
                "cannot fetch latest detail before {}; no mutation was attempted",
                kind.action()
            )
        })?;
    match detail_status(kind, &detail) {
        Some(0) => {}
        Some(1) => {
            crate::output::success(json!({
                "phase": "provider_decision",
                "decision": "noop",
                "reason": "already_accepted",
                "payload": {
                    "jobId": job_id,
                    "taskType": if kind.is_subscription() { "subscription" } else { "single" },
                    "providerDecision": "already_accepted",
                    "status": 1,
                    "broadcast": Value::Null,
                }
            }));
            return Ok(());
        }
        Some(status) => {
            bail!("latest status is {status}, not CREATED(0); no mutation was attempted");
        }
        None => bail!("latest detail has no status; no mutation was attempted"),
    }
    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let path = kind.path(client, job_id);
    let response = client
        .post_mutation_with_identity(&path, &json!({}), agent_id)
        .await
        .with_context(|| {
            format!(
                "{} failed or returned an unknown network result",
                kind.action()
            )
        })?;
    validate_response(job_id, kind, &response)?;
    let extra = reason.map(|reason| json!({"reason": reason.trim()}));
    let broadcast = signing::sign_uop_and_broadcast_full(
        client,
        &response["uopData"],
        &account_id,
        &address,
        job_id,
        kind.biz_type(),
        agent_id,
        extra.as_ref(),
    )
    .await
    .with_context(|| {
        format!(
            "{} broadcast failed or returned an unknown result",
            kind.action()
        )
    })?;
    if broadcast.is_null() {
        bail!("{} broadcast returned no receipt", kind.action());
    }

    audit::log(
        "cli",
        &format!("ASP/{}_submitted", kind.action()),
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("bizType={}", kind.biz_type()),
        ]),
        None,
    );
    crate::output::success(json!({
        "phase": "provider_decision",
        "decision": "ready",
        "reason": "broadcast_submitted",
        "nextAction": [{"id":"watch_task","recommend":true,"params":{"jobId":job_id}}],
        "payload": {
            "jobId": job_id,
            "taskType": if kind.is_subscription() { "subscription" } else { "single" },
            "providerDecision": if kind.is_decline() { "decline" } else { "accept" },
            "type": kind.biz_type(),
            "bizType": kind.biz_type(),
            "status": "broadcast_submitted",
            "broadcast": broadcast,
        }
    }));
    Ok(())
}

pub async fn handle_accept_job(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    execute(client, job_id, agent_id, DecisionKind::AcceptJob, None).await
}

pub async fn handle_decline_job(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    reason: &str,
) -> Result<()> {
    execute(
        client,
        job_id,
        agent_id,
        DecisionKind::DeclineJob,
        Some(reason),
    )
    .await
}

pub async fn handle_accept_subscription(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    execute(
        client,
        job_id,
        agent_id,
        DecisionKind::AcceptSubscription,
        None,
    )
    .await
}

pub async fn handle_decline_subscription(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    reason: &str,
) -> Result<()> {
    execute(
        client,
        job_id,
        agent_id,
        DecisionKind::DeclineSubscription,
        Some(reason),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_all_four_backend_biz_types() {
        for (kind, biz_type) in [
            (DecisionKind::AcceptJob, 203),
            (DecisionKind::DeclineJob, 202),
            (DecisionKind::AcceptSubscription, 205),
            (DecisionKind::DeclineSubscription, 206),
        ] {
            let response = json!({"jobId":"job-1","type":biz_type,"uopData":{}});
            validate_response("job-1", kind, &response).unwrap();
        }
    }

    #[test]
    fn reads_single_and_subscription_status_fields() {
        assert_eq!(
            detail_status(DecisionKind::AcceptJob, &json!({"status": 0})),
            Some(0)
        );
        assert_eq!(
            detail_status(DecisionKind::AcceptSubscription, &json!({"subStatus": 1})),
            Some(1)
        );
    }

    #[test]
    fn decline_reason_is_required_and_unicode_limited() {
        assert!(validate_inputs("job-1", "asp-1", DecisionKind::DeclineJob, Some("")).is_err());
        let too_long = "理".repeat(MAX_DECLINE_REASON_CHARS + 1);
        assert!(validate_inputs(
            "job-1",
            "asp-1",
            DecisionKind::DeclineSubscription,
            Some(&too_long)
        )
        .is_err());
        assert!(validate_inputs(
            "job-1",
            "asp-1",
            DecisionKind::DeclineSubscription,
            Some("out of scope")
        )
        .is_ok());
    }

    #[test]
    fn selects_documented_provider_decision_endpoints() {
        let client = TaskApiClient::new();
        assert_eq!(
            DecisionKind::AcceptJob.path(&client, "job-1"),
            "/priapi/v1/aieco/task/job-1/acceptJobByProvider"
        );
        assert_eq!(
            DecisionKind::DeclineJob.path(&client, "job-1"),
            "/priapi/v1/aieco/task/job-1/declineJobByProvider"
        );
        assert_eq!(
            DecisionKind::AcceptSubscription.path(&client, "job-1"),
            "/priapi/v1/aieco/task/subscribe/job-1/acceptSubscription"
        );
        assert_eq!(
            DecisionKind::DeclineSubscription.path(&client, "job-1"),
            "/priapi/v1/aieco/task/subscribe/job-1/declineSubscription"
        );
    }
}
