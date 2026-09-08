//! Display-ready refund-task lists shared by buyer and provider query flows.

use std::collections::HashSet;

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde_json::{json, Value};

use super::common::network::task_api_client::TaskApiClient;
use super::common::{self, query};
use super::evaluator::dispute_status::{self, DisputeStatusResponse};
use super::user::refund_v2;
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
    response_deadline: Option<i64>,
}

fn integer_from_key(row: &Value, key: &str) -> Option<i64> {
    row.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|raw| raw.trim().parse().ok()))
    })
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
                // `rejectDeadline` belongs to the pending refund-review list,
                // not to a filed evaluation. Preserve it before task-detail
                // enrichment, which may not repeat this list-only field.
                response_deadline: integer_from_key(row, "rejectDeadline"),
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
        let item = refund_v2::fetch_refund_list_item_for_identity(
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
        let deadline = if role == RefundListRole::Provider && scope == RefundListScope::Requested {
            candidate.response_deadline.or(item.deadline)
        } else {
            item.deadline
        };
        let mut display = item.display;
        if let Some(response_deadline) = candidate.response_deadline {
            display["responseDeadlineTimestamp"] = json!(response_deadline);
            display["responseDeadline"] =
                common::deadline::format_local_timestamp_with_offset(response_deadline)
                    .map(Value::String)
                    .unwrap_or(Value::Null);
        }
        rows.push((display, deadline));
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
    let item = refund_v2::fetch_refund_list_item_for_identity(
        client,
        job_id,
        &agent_id,
        role == RefundListRole::Buyer,
    )
    .await?;
    // Completed(6) alone can be an ordinary successful task.  Enrich only a
    // buyer-owned, durably recorded refund request when dispute/status also
    // confirms Completed(6); a failed optional read must not hide Refund V2.
    let terminal_evaluation =
        if role == RefundListRole::Buyer && item.status == 6 && item.refund_request_provenance {
            dispute_status::get_dispute_status(client, job_id, &agent_id)
                .await
                .ok()
                .filter(|status| status.task_status == 6)
        } else {
            None
        };
    crate::output::success(build_refund_detail_result(
        job_id,
        role,
        &item,
        terminal_evaluation.as_ref(),
    ));
    Ok(())
}

fn build_refund_detail_result(
    job_id: &str,
    role: RefundListRole,
    item: &refund_v2::RefundListItem,
    terminal_evaluation: Option<&DisputeStatusResponse>,
) -> Value {
    let provider_decision_required = role == RefundListRole::Provider && item.status == 3;
    let next_action = if provider_decision_required {
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
    let mut display = item.display.clone();
    if terminal_evaluation.is_some() {
        // The documented dispute/status response establishes the outcome but
        // does not contain voteReportSummaries or a decision-rationale field.
        // Do not mislabel the buyer's refund reason as the evaluator's reason.
        display["evaluationResult"] = json!("asp_won");
        display["evaluationResultLabel"] = json!("ASP won; refund not issued");
        display["evaluationResultDescription"] = json!(
            "The Evaluation concluded in favor of the ASP. The task funds were released to the ASP and no refund was issued."
        );
        display["evaluationReason"] =
            json!("The Evaluation service did not return a specific evaluator rationale.");
    }
    json!({
        "phase": if item.status == 3 { "refund_request_detail" } else { "refund_status_detail" },
        "decision": if provider_decision_required { "requires_user_input" } else { "ready" },
        "reason": item.reason,
        "nextAction": next_action,
        "payload": {"display": display},
    })
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
        let one_time = json!({"list":[
            {"jobId":"job-one", "rejectDeadline": 1_700_000_000_000i64},
            {"jobId":"same"}
        ]});
        let subscriptions = json!({"list":[{"jobId":"same"},{"jobId":"job-sub"}]});
        let ids = collect_candidates(&one_time, &subscriptions)
            .into_iter()
            .map(|candidate| candidate.job_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["job-one", "same", "job-sub"]);
        assert_eq!(
            collect_candidates(&one_time, &subscriptions)[0].response_deadline,
            Some(1_700_000_000_000i64)
        );
    }

    #[test]
    fn provider_terminal_refund_detail_reports_result_without_decision_actions() {
        let item = refund_v2::RefundListItem {
            display: json!({
                "jobId": "job-refunded",
                "statusLabel": "Refund completed",
                "statusDescription": "The refund completed successfully."
            }),
            deadline: None,
            job_type: 1,
            status: 9,
            reason: "refund_confirmed",
            refund_request_provenance: true,
            refund_request_available: false,
        };
        let result =
            build_refund_detail_result("job-refunded", RefundListRole::Provider, &item, None);
        assert_eq!(result["phase"], "refund_status_detail");
        assert_eq!(result["decision"], "ready");
        assert_eq!(result["reason"], "refund_confirmed");
        assert_eq!(
            result["payload"]["display"]["statusLabel"],
            "Refund completed"
        );
        assert_eq!(result["nextAction"], json!([]));
    }

    #[test]
    fn buyer_terminal_evaluation_explains_asp_win_without_fabricating_a_reason() {
        let item = refund_v2::RefundListItem {
            display: json!({"statusLabel": "Refund not issued"}),
            deadline: None,
            job_type: 0,
            status: 6,
            reason: "refund_not_approved_or_task_completed",
            refund_request_provenance: true,
            refund_request_available: false,
        };
        let evaluation = DisputeStatusResponse {
            job_id: "job-evaluation".to_string(),
            job_type: Some(0),
            current_round: None,
            selected_voter: None,
            task_status: 6,
            dispute_round_status: None,
            prepare_end_time: None,
            round_end_time: None,
            token_amount: None,
            token_symbol: None,
        };
        let result = build_refund_detail_result(
            "job-evaluation",
            RefundListRole::Buyer,
            &item,
            Some(&evaluation),
        );
        let display = &result["payload"]["display"];
        assert_eq!(display["evaluationResult"], "asp_won");
        assert_eq!(
            display["evaluationResultLabel"],
            "ASP won; refund not issued"
        );
        assert!(display["evaluationResultDescription"]
            .as_str()
            .is_some_and(|text| text.contains("released to the ASP")));
        assert_eq!(
            display["evaluationReason"],
            "The Evaluation service did not return a specific evaluator rationale."
        );
    }
}
