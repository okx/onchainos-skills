//! Display-ready refund-task lists shared by buyer and provider query flows.

use std::collections::HashSet;

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde_json::{json, Value};

use super::common::network::task_api_client::TaskApiClient;
use super::common::{self, query};
use super::user::refund;
use super::user::subscription_ops::{self, SubscriptionRole};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum RefundListRole {
    Buyer,
    Provider,
}

impl RefundListRole {
    fn agent_role(self) -> i64 {
        match self {
            Self::Buyer => common::AGENT_ROLE_USER,
            Self::Provider => common::AGENT_ROLE_ASP,
        }
    }

    fn subscription_role(self) -> SubscriptionRole {
        match self {
            Self::Buyer => SubscriptionRole::Buyer,
            Self::Provider => SubscriptionRole::Provider,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Buyer => "buyer",
            Self::Provider => "provider",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum RefundListScope {
    Available,
    Requested,
}

impl RefundListScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Requested => "requested",
        }
    }

    fn one_time_status(self) -> &'static str {
        match self {
            Self::Available => "submitted",
            Self::Requested => "rejected",
        }
    }

    fn subscription_status(self) -> i32 {
        match self {
            Self::Available => 1,
            Self::Requested => 3,
        }
    }
}

#[derive(Debug)]
struct Candidate {
    job_id: String,
}

fn collect_candidates(one_time: &Value, subscriptions: &Value) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for row in one_time
        .get("list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            subscriptions
                .get("list")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
    {
        let Some(job_id) = row
            .get("jobId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if seen.insert(job_id.to_string()) {
            candidates.push(Candidate {
                job_id: job_id.to_string(),
            });
        }
    }
    candidates
}

fn sort_rows(rows: &mut [(Value, Option<i64>)], role: RefundListRole) {
    if role == RefundListRole::Provider {
        rows.sort_by_key(|(_, deadline)| deadline.unwrap_or(i64::MAX));
    }
}

fn require_display_string(display: &Value, key: &str, job_id: &str) -> Result<()> {
    if display
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    bail!("refund display for {job_id} is missing {key}")
}

fn validate_list_display(display: &Value, job_id: &str) -> Result<()> {
    for key in [
        "serviceName",
        "jobId",
        "taskType",
        "refundAmount",
        "resultDeadline",
    ] {
        require_display_string(display, key, job_id)?;
    }
    Ok(())
}

fn validate_detail_display(display: &Value, job_id: &str, role: RefundListRole) -> Result<()> {
    validate_list_display(display, job_id)?;
    require_display_string(display, "reasonForRefund", job_id)?;
    if role == RefundListRole::Buyer {
        require_display_string(display, "serviceProviderName", job_id)?;
        require_display_string(display, "agentId", job_id)?;
    } else if display.get("taskType").and_then(Value::as_str) == Some("Subscription") {
        require_display_string(display, "currentPeriod", job_id)?;
    }
    Ok(())
}

pub async fn handle_refund_list(
    client: &mut TaskApiClient,
    role: RefundListRole,
    scope: RefundListScope,
    page: u32,
    page_size: u32,
    agent_id: &str,
) -> Result<()> {
    if role == RefundListRole::Provider && scope != RefundListScope::Requested {
        bail!("provider refund-list supports only --scope requested");
    }
    let agent_id = query::resolve_agent_id_or_error(agent_id, role.agent_role()).await?;
    let one_time_path = format!(
        "/priapi/v1/aieco/task/my?page={page}&page_size={page_size}&status={}",
        scope.one_time_status()
    );
    let one_time = client.get_with_identity(&one_time_path, &agent_id).await?;
    let subscriptions = subscription_ops::fetch_my_subscriptions_snapshot_for_agent_read_only(
        client,
        role.subscription_role(),
        Some(scope.subscription_status()),
        agent_id.clone(),
    )
    .await?
    .data;

    let mut rows = Vec::new();
    for candidate in collect_candidates(&one_time, &subscriptions) {
        let item = refund::fetch_refund_list_item_for_identity(
            client,
            &candidate.job_id,
            &agent_id,
            role == RefundListRole::Buyer,
        )
        .await?;
        if (scope == RefundListScope::Available && !item.refund_request_available)
            || (scope == RefundListScope::Requested && item.status != 3)
        {
            continue;
        }
        if scope == RefundListScope::Requested {
            validate_list_display(&item.display, &candidate.job_id)?;
        }
        rows.push((item.display, item.deadline));
    }
    sort_rows(&mut rows, role);
    let items = rows
        .into_iter()
        .map(|(display, _)| display)
        .collect::<Vec<_>>();

    crate::output::success(json!({
        "role": role.as_str(),
        "scope": scope.as_str(),
        "total": items.len(),
        "pendingCount": items.len(),
        "items": items,
    }));
    Ok(())
}

pub async fn handle_refund_detail(
    client: &mut TaskApiClient,
    job_id: &str,
    role: RefundListRole,
    agent_id: &str,
) -> Result<()> {
    let agent_id = query::resolve_agent_id_or_error(agent_id, role.agent_role()).await?;
    let item = refund::fetch_refund_list_item_for_identity(
        client,
        job_id,
        &agent_id,
        role == RefundListRole::Buyer,
    )
    .await?;
    if item.status != 3 {
        bail!(
            "refund-detail requires a rejected task; current status is {}",
            item.status
        );
    }
    validate_detail_display(&item.display, job_id, role)?;
    let next_action = if role == RefundListRole::Provider {
        let (refund_action, evaluation_action) = if item.job_type == 1 {
            ("sub_agree_refund", "raise_subscription_arbitration")
        } else {
            ("agree_refund", "raise_arbitration")
        };
        json!([
            {"id": refund_action, "recommend": false, "params": {"jobId": job_id}},
            {"id": evaluation_action, "recommend": false, "params": {"jobId": job_id}}
        ])
    } else {
        json!([])
    };
    crate::output::success(json!({
        "phase": "refund_request_detail",
        "decision": if role == RefundListRole::Provider { "requires_user_input" } else { "ready" },
        "reason": "refund_request_found",
        "nextAction": next_action,
        "payload": {"display": item.display},
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_map_to_authoritative_status_filters() {
        assert_eq!(RefundListScope::Available.one_time_status(), "submitted");
        assert_eq!(RefundListScope::Available.subscription_status(), 1);
        assert_eq!(RefundListScope::Requested.one_time_status(), "rejected");
        assert_eq!(RefundListScope::Requested.subscription_status(), 3);
    }

    #[test]
    fn provider_rows_sort_by_response_deadline_with_missing_values_last() {
        let mut rows = vec![
            (json!({"jobId":"missing"}), None),
            (json!({"jobId":"later"}), Some(20)),
            (json!({"jobId":"first"}), Some(10)),
        ];
        sort_rows(&mut rows, RefundListRole::Provider);
        let ids = rows
            .iter()
            .map(|(row, _)| row["jobId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["first", "later", "missing"]);
    }

    #[test]
    fn candidates_are_deduplicated_without_shortening_job_ids() {
        let one_time = json!({"list":[{"jobId":"job-one"},{"jobId":"same"}]});
        let subscriptions = json!({"list":[{"jobId":"same"},{"jobId":"job-sub"}]});
        let ids = collect_candidates(&one_time, &subscriptions)
            .into_iter()
            .map(|candidate| candidate.job_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["job-one", "same", "job-sub"]);
    }

    #[test]
    fn detail_display_fails_closed_when_reason_is_unavailable() {
        let display = json!({
            "serviceName": "Audit",
            "jobId": "job-full-id",
            "serviceProviderName": "Example ASP",
            "agentId": "asp-1",
            "taskType": "One-time",
            "currentPeriod": null,
            "refundAmount": "1 USDT",
            "reasonForRefund": null,
            "resultDeadline": "2026-09-08 12:00 (UTC+08:00)",
        });
        let error = validate_detail_display(&display, "job-full-id", RefundListRole::Buyer)
            .unwrap_err()
            .to_string();
        assert!(error.contains("reasonForRefund"));
    }
}
