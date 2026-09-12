//! Display-ready ASP task list and detail queries.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::query;
use crate::commands::agent_commerce::task::user::subscription_ops;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskKind {
    OneTime,
    Subscription,
}

const ONE_TIME_BACKEND_PAGE_SIZE: u64 = 20;

fn scalar_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn string_from_keys(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| scalar_string(value.get(*key)))
}

fn integer_from_keys(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
    })
}

fn bool_from_keys(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value
            .as_bool()
            .or_else(|| value.as_i64().map(|value| value != 0))
            .or_else(
                || match value.as_str()?.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                },
            )
    })
}

fn is_zero_amount(amount: &str) -> bool {
    let amount = amount.trim().trim_start_matches('+');
    !amount.is_empty()
        && amount
            .chars()
            .all(|character| matches!(character, '0' | '.'))
}

fn task_status(value: &Value) -> i64 {
    integer_from_keys(value, &["subStatus", "status"]).unwrap_or(-1)
}

fn task_type_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::OneTime => "One-time Task",
        TaskKind::Subscription => "Subscription Task",
    }
}

fn status_fields(kind: TaskKind, code: i64) -> (&'static str, String, &'static str) {
    match kind {
        TaskKind::OneTime => (
            query::status_name(code),
            query::task_status_label(code).to_string(),
            query::task_status_description(code),
        ),
        TaskKind::Subscription => (
            match code {
                -1 => "INIT",
                0 => "CREATED",
                1 => "ACTIVE",
                3 => "REJECTED",
                4 => "DISPUTED",
                6 => "COMPLETED",
                7 => "CLOSED",
                8 => "EXPIRED",
                9 => "FAILED",
                _ => "UNKNOWN",
            },
            match code {
                0 => "Awaiting acceptance".to_string(),
                1 => "In service".to_string(),
                _ => subscription_ops::status_label(code).to_string(),
            },
            subscription_ops::status_description(code),
        ),
    }
}

fn amount_and_symbol(value: &Value, kind: TaskKind) -> (Option<String>, Option<String>) {
    let amount_keys = match kind {
        TaskKind::OneTime => &["tokenAmount", "paymentTokenAmount"][..],
        TaskKind::Subscription => &["serviceTokenAmount", "paymentTokenAmount", "tokenAmount"][..],
    };
    let symbol_keys = match kind {
        TaskKind::OneTime => &["tokenSymbol", "paymentTokenSymbol"][..],
        TaskKind::Subscription => &["serviceTokenSymbol", "paymentTokenSymbol", "tokenSymbol"][..],
    };
    (
        string_from_keys(value, amount_keys),
        string_from_keys(value, symbol_keys),
    )
}

fn fee_label(value: &Value, kind: TaskKind) -> Value {
    let (amount, symbol) = amount_and_symbol(value, kind);
    let Some(amount) = amount else {
        return Value::Null;
    };
    if is_zero_amount(&amount) {
        return Value::String("Free".to_string());
    }
    let Some(symbol) = symbol else {
        return Value::Null;
    };
    let suffix = if kind == TaskKind::Subscription {
        " / month"
    } else {
        " / task"
    };
    Value::String(format!("{amount} {symbol}{suffix}"))
}

fn detail_fee_label(value: &Value, kind: TaskKind) -> Value {
    let (amount, symbol) = amount_and_symbol(value, kind);
    let Some(amount) = amount else {
        return Value::Null;
    };
    if is_zero_amount(&amount) {
        return Value::String("Free".to_string());
    }
    let Some(symbol) = symbol else {
        return Value::Null;
    };
    let suffix = if kind == TaskKind::Subscription {
        " / month"
    } else {
        ""
    };
    Value::String(format!("{amount} {symbol}{suffix}"))
}

fn formatted_time(value: &Value, keys: &[&str]) -> Value {
    integer_from_keys(value, keys)
        .and_then(common::deadline::format_local_timestamp_with_offset)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

fn date_only(value: &Value, keys: &[&str]) -> Option<String> {
    let formatted = integer_from_keys(value, keys)
        .and_then(common::deadline::format_local_timestamp_with_offset)?;
    formatted.get(..10).map(ToOwned::to_owned)
}

fn current_period(value: &Value, kind: TaskKind) -> Value {
    if kind != TaskKind::Subscription
        || integer_from_keys(value, &["trialType"]) == Some(1)
        || integer_from_keys(value, &["periodIndex"]).unwrap_or(0) <= 0
    {
        return Value::Null;
    }
    match (
        date_only(value, &["periodStartTime", "subStartTime"]),
        date_only(value, &["periodEndTime", "subEndTime"]),
    ) {
        (Some(start), Some(end)) => Value::String(format!("{start}–{end}")),
        _ => Value::Null,
    }
}

fn billing_period_label(value: &Value, kind: TaskKind) -> Value {
    if kind != TaskKind::Subscription {
        return Value::Null;
    }
    if integer_from_keys(value, &["trialType"]) == Some(1) {
        return Value::String("Trial Period".to_string());
    }
    integer_from_keys(value, &["periodIndex"])
        .filter(|period| *period > 0)
        .map(|period| Value::String(format!("Billing Period {period}")))
        .unwrap_or(Value::Null)
}

fn auto_renew(value: &Value, kind: TaskKind) -> (Value, bool) {
    if kind != TaskKind::Subscription {
        return (Value::Null, false);
    }
    match bool_from_keys(value, &["autoRenew"]) {
        Some(true) => (Value::String("Enabled".to_string()), true),
        Some(false) => (Value::String("Disabled".to_string()), false),
        None => (Value::Null, false),
    }
}

fn normalize_item(value: &Value, kind: TaskKind) -> Value {
    let code = task_status(value);
    let (status_name, status_label, status_description) = status_fields(kind, code);
    let (auto_renew_label, auto_renew_enabled) = auto_renew(value, kind);
    let next_charge_at = if kind == TaskKind::Subscription && code == 1 && auto_renew_enabled {
        formatted_time(value, &["nextChargeTime", "subEndTime"])
    } else {
        Value::Null
    };
    json!({
        "jobName": string_from_keys(value, &["title", "jobName", "serviceName"]),
        "jobId": string_from_keys(value, &["jobId", "subId"]),
        "userName": string_from_keys(value, &["buyerAgentName", "userAgentName", "buyerName", "userName"]),
        "userAgentId": string_from_keys(value, &["buyerAgentId", "userAgentId"]),
        "testFlag": common::is_test_task(value),
        "taskType": if kind == TaskKind::Subscription { "subscription" } else { "one_time" },
        "taskTypeLabel": task_type_label(kind),
        "status": status_name,
        "statusCode": code,
        "statusLabel": status_label,
        "statusDescription": status_description,
        "feeLabel": fee_label(value, kind),
        "detailFeeLabel": detail_fee_label(value, kind),
        "billingPeriodLabel": billing_period_label(value, kind),
        "billingCycleLabel": if kind == TaskKind::Subscription { Value::String("Monthly".to_string()) } else { Value::Null },
        "currentPeriod": current_period(value, kind),
        "nextChargeAt": next_charge_at,
        "autoRenewLabel": auto_renew_label,
        "createdAt": formatted_time(value, &["createTime", "createdAt", "subCreateTime"]),
    })
}

fn page_items(value: &Value) -> Vec<Value> {
    value
        .get("list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn page_total(value: &Value) -> u64 {
    value
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| page_items(value).len() as u64)
}

fn page_has_more(value: &Value, page: u32, page_size: u32) -> bool {
    u64::from(page).saturating_mul(u64::from(page_size)) < page_total(value)
}

fn subscription_status_code(status: &str) -> Option<i64> {
    status.trim().parse::<i64>().ok().or_else(|| {
        match status.trim().to_ascii_lowercase().as_str() {
            "init" => Some(-1),
            "created" => Some(0),
            "accepted" | "active" => Some(1),
            "rejected" => Some(3),
            "disputed" => Some(4),
            "complete" | "completed" => Some(6),
            "close" | "closed" => Some(7),
            "expired" => Some(8),
            "failed" => Some(9),
            // One-time-only or unknown states must not leak unrelated
            // subscriptions into a filtered combined list.
            _ => None,
        }
    })
}

fn subscription_matches_status(value: &Value, status: Option<&str>) -> bool {
    let Some(status) = status.map(str::trim).filter(|status| !status.is_empty()) else {
        return true;
    };
    subscription_status_code(status).is_some_and(|expected| task_status(value) == expected)
}

fn subscription_list_path(page: u32, page_size: u32, status: Option<&str>) -> Option<String> {
    let mut path =
        format!("/priapi/v1/aieco/task/subscribe/my?page={page}&pageSize={page_size}&statusType=0");
    if let Some(status) = status.map(str::trim).filter(|status| !status.is_empty()) {
        path.push_str(&format!(
            "&statusList={}",
            subscription_status_code(status)?
        ));
    }
    Some(path)
}

fn one_time_list_path(page: u64, status: Option<&str>) -> String {
    let mut path =
        format!("/priapi/v1/aieco/task/my?page={page}&page_size={ONE_TIME_BACKEND_PAGE_SIZE}");
    if let Some(status) = status.map(str::trim).filter(|status| !status.is_empty()) {
        path.push_str(&format!("&status={status}"));
    }
    path
}

async fn fetch_one_time_page(
    client: &mut TaskApiClient,
    agent_id: &str,
    page: u32,
    page_size: u32,
    status: Option<&str>,
) -> Result<Value> {
    // The provider endpoint currently returns fixed 20-row backend pages even
    // when page_size differs. Translate the public page/limit into that fixed
    // window so the CLI contract remains exact and stateless.
    let start = u64::from(page - 1).saturating_mul(u64::from(page_size));
    let end = start.saturating_add(u64::from(page_size));
    let first_backend_page = start / ONE_TIME_BACKEND_PAGE_SIZE + 1;
    let last_backend_page = end.saturating_sub(1) / ONE_TIME_BACKEND_PAGE_SIZE + 1;
    let mut total = 0;
    let mut items = Vec::new();

    for backend_page in first_backend_page..=last_backend_page {
        let path = one_time_list_path(backend_page, status);
        let response = client.get_with_identity(&path, agent_id).await?;
        total = page_total(&response);
        items.extend(page_items(&response).iter().cloned());
        if backend_page.saturating_mul(ONE_TIME_BACKEND_PAGE_SIZE) >= total {
            break;
        }
    }

    let skip = (start % ONE_TIME_BACKEND_PAGE_SIZE) as usize;
    let list = items
        .into_iter()
        .skip(skip)
        .take(page_size as usize)
        .collect::<Vec<_>>();
    Ok(json!({"page": page, "pageSize": page_size, "total": total, "list": list}))
}

fn build_list_result(
    agent_id: &str,
    page: u32,
    page_size: u32,
    status: Option<&str>,
    one_time: &Value,
    subscriptions: &Value,
) -> Value {
    let subscription_items = page_items(subscriptions)
        .into_iter()
        .filter(|item| {
            string_from_keys(item, &["providerAgentId", "aspAgentId"])
                .is_none_or(|provider_id| provider_id == agent_id)
        })
        .filter(|item| subscription_matches_status(item, status))
        .collect::<Vec<_>>();
    let subscription_total = page_total(subscriptions);
    let one_time_total = page_total(one_time);
    let mut items = subscription_items
        .iter()
        .map(|item| normalize_item(item, TaskKind::Subscription))
        .collect::<Vec<_>>();
    items.extend(
        page_items(one_time)
            .iter()
            .map(|item| normalize_item(item, TaskKind::OneTime)),
    );
    let subscription_has_more = page_has_more(subscriptions, page, page_size);
    let one_time_has_more = page_has_more(one_time, page, page_size);
    let allowed_job_ids = items
        .iter()
        .filter_map(|item| item.get("jobId").and_then(Value::as_str))
        .collect::<Vec<_>>();
    json!({
        "phase": "provider_task_list",
        "decision": "ready",
        "reason": if items.is_empty() { "no_tasks" } else { "tasks_found" },
        "nextAction": [{
            "id": "view_provider_task",
            "actionLabel": "View task details",
            "recommend": false,
            "params": {"allowedJobIds": allowed_job_ids, "confirmationRequired": false}
        }],
        "payload": {
            "agentId": agent_id,
            "page": page,
            "pageSize": page_size,
            "paginationScope": "per_task_type",
            "subscriptionTotal": subscription_total,
            "subscriptionHasMore": subscription_has_more,
            "oneTimeTotal": one_time_total,
            "oneTimeHasMore": one_time_has_more,
            "total": one_time_total.saturating_add(subscription_total),
            "hasMore": subscription_has_more || one_time_has_more,
            "hasSubscriptionTasks": items.iter().any(|item| item["taskType"] == "subscription"),
            "items": items,
        }
    })
}

pub async fn handle_list(
    client: &mut TaskApiClient,
    status: Option<&str>,
    page: u32,
    page_size: u32,
    agent_id: &str,
) -> Result<()> {
    if page == 0 || page_size == 0 {
        bail!("page and page size must be greater than 0");
    }
    let agent_id = query::resolve_agent_id_or_error(agent_id, common::AGENT_ROLE_ASP).await?;
    if status.is_some_and(|status| status.trim().eq_ignore_ascii_case("disputed")) {
        return crate::commands::agent_commerce::task::arbitration::handle_provider_arbitration_list(
            client, &agent_id, page, page_size,
        )
        .await;
    }
    // Refresh once before spawning independent reads. Each request still reads
    // the stored token, but neither branch can race to rotate an expiring token.
    crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    let mut one_time_client = client.clone();
    let mut subscription_client = client.clone();
    let one_time_request =
        fetch_one_time_page(&mut one_time_client, &agent_id, page, page_size, status);
    let subscription_request = async {
        match subscription_list_path(page, page_size, status) {
            Some(subscription_path) => {
                subscription_client
                    .get_with_agent_id(&subscription_path, &agent_id)
                    .await
            }
            // A one-time-only status cannot match any subscription row.
            None => Ok(json!({
                "page": page,
                "pageSize": page_size,
                "total": 0,
                "list": []
            })),
        }
    };
    let (one_time, subscriptions) = tokio::try_join!(one_time_request, subscription_request)?;
    let result = build_list_result(
        &agent_id,
        page,
        page_size,
        status,
        &one_time,
        &subscriptions,
    );
    crate::output::success(result);
    Ok(())
}

fn build_detail_result(agent_id: &str, value: &Value, kind: TaskKind) -> Value {
    json!({
        "phase": "provider_task_detail",
        "decision": "ready",
        "reason": "task_found",
        "nextAction": [],
        "payload": {
            "agentId": agent_id,
            "task": normalize_item(value, kind),
        }
    })
}

fn build_provider_arbitration_detail_result(
    agent_id: &str,
    detail: &Value,
    kind: TaskKind,
    dispute: &crate::commands::agent_commerce::task::evaluator::dispute_status::DisputeStatusResponse,
) -> Value {
    let mut result = build_detail_result(agent_id, detail, kind);
    let arbitration = crate::commands::agent_commerce::task::arbitration::build_detail_result(
        detail
            .get("jobId")
            .and_then(Value::as_str)
            .unwrap_or(&dispute.job_id),
        detail,
        Some(dispute),
        None,
        None,
    );
    result["nextAction"] = arbitration["nextAction"].clone();
    result["payload"]["arbitration"] = arbitration["payload"].clone();
    result
}

async fn arbitration_detail_if_present(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    detail: &Value,
) -> Result<
    Option<crate::commands::agent_commerce::task::evaluator::dispute_status::DisputeStatusResponse>,
> {
    let dispute = match task_status(detail) {
        4 => Some(
            crate::commands::agent_commerce::task::evaluator::dispute_status::get_dispute_status(
                client, job_id, agent_id,
            )
            .await?,
        ),
        6 | 9 => {
            crate::commands::agent_commerce::task::evaluator::dispute_status::get_dispute_status(
                client, job_id, agent_id,
            )
            .await
            .ok()
        }
        _ => None,
    };
    Ok(dispute)
}

pub async fn handle_detail(client: &mut TaskApiClient, job_id: &str, agent_id: &str) -> Result<()> {
    let job_id = job_id.trim();
    if job_id.is_empty() {
        bail!("jobId must not be empty");
    }
    let agent_id = query::resolve_agent_id_or_error(agent_id, common::AGENT_ROLE_ASP).await?;
    let ordinary = client
        .get_with_identity(&client.task_path(job_id), &agent_id)
        .await;
    let (detail, kind) = match ordinary {
        Ok(detail) if integer_from_keys(&detail, &["jobType"]) == Some(1) => (
            client
                .fetch_subscription(job_id, &agent_id)
                .await
                .unwrap_or(detail),
            TaskKind::Subscription,
        ),
        Ok(detail) => (detail, TaskKind::OneTime),
        Err(task_error) => match client.fetch_subscription(job_id, &agent_id).await {
            Ok(detail) => (detail, TaskKind::Subscription),
            Err(_) => {
                let dispute = crate::commands::agent_commerce::task::evaluator::dispute_status::get_dispute_status(
                    client, job_id, &agent_id,
                )
                .await
                .map_err(|_| task_error)?;
                let kind = if dispute.job_type == Some(1) {
                    TaskKind::Subscription
                } else {
                    TaskKind::OneTime
                };
                let detail = json!({
                    "jobId": job_id,
                    "jobType": dispute.job_type,
                    "status": dispute.task_status,
                    "tokenAmount": dispute.token_amount.clone(),
                    "tokenSymbol": dispute.token_symbol.clone(),
                });
                crate::output::success(build_provider_arbitration_detail_result(
                    &agent_id, &detail, kind, &dispute,
                ));
                return Ok(());
            }
        },
    };
    if let Some(dispute) = arbitration_detail_if_present(client, job_id, &agent_id, &detail).await?
    {
        crate::output::success(build_provider_arbitration_detail_result(
            &agent_id, &detail, kind, &dispute,
        ));
        return Ok(());
    }
    crate::output::success(build_detail_result(&agent_id, &detail, kind));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_combines_subscription_and_one_time_rows_for_the_provider() {
        let subscriptions = json!({
            "total": 1,
            "list": [{
                "jobId": "sub-1",
                "title": "Daily Brief",
                "buyerAgentName": "Market Agent",
                "buyerAgentId": "8415",
                "providerAgentId": "9001",
                "status": 1,
                "serviceTokenAmount": "10",
                "paymentTokenSymbol": "USDT",
                "periodIndex": 2,
                "subStartTime": 1_788_192_600,
                "subEndTime": 1_790_784_600,
                "autoRenew": 1,
                "testFlag": true,
                "createTime": 1_788_192_600
            }]
        });
        let one_time = json!({
            "total": 1,
            "list": [{
                "jobId": "job-1",
                "title": "Risk Analysis",
                "buyerAgentName": "Research Agent",
                "buyerAgentId": "5331",
                "status": 0,
                "tokenAmount": "0.1",
                "tokenSymbol": "USDT"
            }]
        });

        let result = build_list_result("9001", 1, 20, None, &one_time, &subscriptions);
        let items = result["payload"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["taskTypeLabel"], "Subscription Task");
        assert_eq!(items[0]["testFlag"], true);
        assert_eq!(items[0]["feeLabel"], "10 USDT / month");
        assert_eq!(items[0]["billingPeriodLabel"], "Billing Period 2");
        assert_eq!(items[0]["autoRenewLabel"], "Enabled");
        assert!(items[0]["nextChargeAt"].is_string());
        assert_eq!(items[1]["taskTypeLabel"], "One-time Task");
        assert_eq!(items[1]["feeLabel"], "0.1 USDT / task");
        assert!(items[1]["billingPeriodLabel"].is_null());
        assert_eq!(result["payload"]["hasSubscriptionTasks"], true);
    }

    #[test]
    fn list_applies_one_time_status_names_to_subscription_rows() {
        let subscriptions = json!({
            // The backend total reflects statusList=1. Keep an extra row in the
            // fixture to prove the client-side defensive filter still applies.
            "total": 1,
            "list": [
                {"jobId": "sub-active", "providerAgentId": "9001", "status": 1},
                {"jobId": "sub-closed", "providerAgentId": "9001", "status": 7}
            ]
        });
        let one_time = json!({"total": 0, "list": []});

        let accepted =
            build_list_result("9001", 1, 20, Some("accepted"), &one_time, &subscriptions);
        assert_eq!(accepted["payload"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(accepted["payload"]["items"][0]["jobId"], "sub-active");
        assert_eq!(accepted["payload"]["total"], 1);
        assert_eq!(accepted["payload"]["hasMore"], false);

        let submitted =
            build_list_result("9001", 1, 20, Some("submitted"), &one_time, &subscriptions);
        assert!(submitted["payload"]["items"].as_array().unwrap().is_empty());
    }

    #[test]
    fn list_pages_subscriptions_and_one_time_tasks_independently() {
        let subscription_page = |start: u32, count: u32| {
            let list = (start..start + count)
                .map(|index| {
                    json!({
                        "jobId": format!("sub-{index}"),
                        "providerAgentId": "9001",
                        "status": 1
                    })
                })
                .collect::<Vec<_>>();
            json!({"total": 10, "list": list})
        };
        let one_time_page = |start: u32, count: u32| {
            let list = (start..start + count)
                .map(|index| json!({"jobId": format!("task-{index}"), "status": 0}))
                .collect::<Vec<_>>();
            json!({"total": 10, "list": list})
        };

        let first = build_list_result(
            "9001",
            1,
            3,
            None,
            &one_time_page(1, 3),
            &subscription_page(1, 3),
        );
        let first_ids = first["payload"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["jobId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            first_ids,
            ["sub-1", "sub-2", "sub-3", "task-1", "task-2", "task-3"]
        );
        assert_eq!(first["payload"]["pageSize"], 3);
        assert_eq!(first["payload"]["subscriptionTotal"], 10);
        assert_eq!(first["payload"]["subscriptionHasMore"], true);
        assert_eq!(first["payload"]["oneTimeTotal"], 10);
        assert_eq!(first["payload"]["oneTimeHasMore"], true);
        assert_eq!(first["payload"]["total"], 20);
        assert_eq!(first["payload"]["hasMore"], true);
        assert_eq!(first["payload"]["hasSubscriptionTasks"], true);
        assert_eq!(first["payload"]["paginationScope"], "per_task_type");

        let second = build_list_result(
            "9001",
            2,
            3,
            None,
            &one_time_page(4, 3),
            &subscription_page(4, 3),
        );
        let second_ids = second["payload"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["jobId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            second_ids,
            ["sub-4", "sub-5", "sub-6", "task-4", "task-5", "task-6"]
        );
        assert_eq!(second["payload"]["subscriptionTotal"], 10);
        assert_eq!(second["payload"]["total"], 20);
        assert_eq!(second["payload"]["hasMore"], true);
        assert_eq!(second["payload"]["hasSubscriptionTasks"], true);
        assert_eq!(
            second["nextAction"][0]["params"]["allowedJobIds"],
            json!(["sub-4", "sub-5", "sub-6", "task-4", "task-5", "task-6"])
        );

        let fourth = build_list_result(
            "9001",
            4,
            3,
            None,
            &one_time_page(10, 1),
            &subscription_page(10, 1),
        );
        assert_eq!(fourth["payload"]["items"].as_array().unwrap().len(), 2);
        assert_eq!(fourth["payload"]["items"][0]["jobId"], "sub-10");
        assert_eq!(fourth["payload"]["items"][1]["jobId"], "task-10");
        assert_eq!(fourth["payload"]["hasMore"], false);
        assert_eq!(fourth["payload"]["hasSubscriptionTasks"], true);
    }

    #[test]
    fn subscription_list_path_uses_backend_pagination_and_exact_status_filter() {
        assert_eq!(
            subscription_list_path(2, 3, None).as_deref(),
            Some("/priapi/v1/aieco/task/subscribe/my?page=2&pageSize=3&statusType=0")
        );
        assert_eq!(
            subscription_list_path(2, 3, Some("active")).as_deref(),
            Some("/priapi/v1/aieco/task/subscribe/my?page=2&pageSize=3&statusType=0&statusList=1")
        );
        assert!(subscription_list_path(2, 3, Some("submitted")).is_none());
    }

    #[test]
    fn one_time_list_path_uses_backend_pagination_and_status_filter() {
        assert_eq!(
            one_time_list_path(2, None),
            "/priapi/v1/aieco/task/my?page=2&page_size=20"
        );
        assert_eq!(
            one_time_list_path(2, Some(" submitted ")),
            "/priapi/v1/aieco/task/my?page=2&page_size=20&status=submitted"
        );
    }

    #[test]
    fn detail_keeps_review_flag_on_the_user_and_formats_subscription_fields() {
        let detail = json!({
            "jobId": "sub-1",
            "title": "Risk Analysis",
            "buyerAgentName": "Alice",
            "buyerAgentId": "5678",
            "testFlag": true,
            "status": 1,
            "serviceTokenAmount": "10",
            "paymentTokenSymbol": "USDT",
            "periodIndex": 2,
            "subStartTime": 1_788_192_600,
            "subEndTime": 1_790_784_600,
            "periodStartTime": 1_790_784_000,
            "periodEndTime": 1_793_376_000,
            "autoRenew": 1,
            "createTime": 1_788_192_600
        });
        let expected_period = format!(
            "{}–{}",
            date_only(&detail, &["periodStartTime"]).unwrap(),
            date_only(&detail, &["periodEndTime"]).unwrap(),
        );
        let subscription_window = format!(
            "{}–{}",
            date_only(&detail, &["subStartTime"]).unwrap(),
            date_only(&detail, &["subEndTime"]).unwrap(),
        );
        let result = build_detail_result("9001", &detail, TaskKind::Subscription);
        let task = &result["payload"]["task"];
        assert_eq!(task["userName"], "Alice");
        assert_eq!(task["userAgentId"], "5678");
        assert_eq!(task["testFlag"], true);
        assert_eq!(task["billingCycleLabel"], "Monthly");
        assert_eq!(task["currentPeriod"], expected_period);
        assert_ne!(task["currentPeriod"], subscription_window);
        assert!(task["createdAt"].is_string());
    }

    #[test]
    fn provider_arbitration_detail_keeps_the_provider_task_envelope() {
        let detail = json!({
            "jobId": "job-1",
            "title": "Risk Analysis",
            "buyerAgentName": "Alice",
            "buyerAgentId": "5678",
            "testFlag": true,
            "status": 4,
            "tokenAmount": "1",
            "tokenSymbol": "USDT"
        });
        let dispute = serde_json::from_value(json!({
            "jobId": "job-1",
            "jobType": 0,
            "taskStatus": 4,
            "currentRound": 1,
            "disputeRoundStatus": 1
        }))
        .unwrap();

        let result =
            build_provider_arbitration_detail_result("9001", &detail, TaskKind::OneTime, &dispute);
        assert_eq!(result["phase"], "provider_task_detail");
        assert_eq!(result["payload"]["task"]["jobId"], "job-1");
        assert_eq!(result["payload"]["task"]["testFlag"], true);
        assert_eq!(result["payload"]["arbitration"]["jobId"], "job-1");
    }
}
