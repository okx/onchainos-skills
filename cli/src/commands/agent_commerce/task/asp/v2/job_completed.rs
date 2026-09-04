use crate::commands::agent_commerce::task::common::{
    network::task_api_client::TaskApiClient, PreFetchedTaskContext,
};

pub(crate) async fn handle(job_id: &str, agent_id: &str) -> String {
    let mut client = TaskApiClient::new();
    let response = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await;
    result_from_task_detail(job_id, agent_id, response)
}

fn result_from_task_detail(
    job_id: &str,
    agent_id: &str,
    response: anyhow::Result<serde_json::Value>,
) -> String {
    let response = match response {
        Ok(response) => response,
        Err(_) => return blocked_result(job_id, "task_detail_unavailable"),
    };
    if response.get("jobId").and_then(serde_json::Value::as_str) != Some(job_id) {
        return blocked_result(job_id, "task_detail_job_id_mismatch");
    }
    if response.get("status").and_then(serde_json::Value::as_i64) != Some(6) {
        return blocked_result(job_id, "stale_task_status");
    }
    let task = PreFetchedTaskContext::from_api_response(&response);
    let user_agent_id = match task
        .user_agent_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return blocked_result(job_id, "task_detail_unavailable"),
    };

    serde_json::json!({
        "phase": "task_completion",
        "decision": "ready",
        "reason": "notification_and_rating_required",
        "nextAction": [{
            "id": "finalize_asp_task",
            "recommend": true,
        }],
        "payload": {
            "jobId": job_id,
            "notification": completion_notification(job_id, &task),
            "ratingResultNotification": rating_notification(job_id, &task),
            "rating": {
                "targetAgentId": user_agent_id,
                "creatorAgentId": agent_id,
                "taskDescription": task.description,
                "taskParameters": task.service_params.as_deref().filter(|value| !value.is_empty()),
            },
        },
    })
    .to_string()
}

fn blocked_result(job_id: &str, reason: &str) -> String {
    serde_json::json!({
        "phase": "task_completion",
        "decision": "blocked",
        "reason": reason,
        "nextAction": [{ "id": "stop" }],
        "payload": { "jobId": job_id },
    })
    .to_string()
}

fn completion_notification(job_id: &str, task: &PreFetchedTaskContext) -> String {
    format!(
        "[💰 Job Completed] Job {job_id} ({}) — approved by the User Agent; funds received.\n      - Income: {} {}\n      - User Agent: {}\n    \n    This job is complete.",
        title(task),
        task.token_amount,
        task.token_symbol,
        task.user_agent_id.as_deref().unwrap_or_default(),
    )
}

fn rating_notification(job_id: &str, task: &PreFetchedTaskContext) -> String {
    super::super::content::rating_submitted_user_notify(job_id).replace("<title>", title(task))
}

fn title(task: &PreFetchedTaskContext) -> &str {
    if task.title.is_empty() {
        "Task"
    } else {
        task.title.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_asp_completion_action_with_scoring_context() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "description": "Audit the contract",
            "paymentMode": 1,
            "tokenAmount": "12",
            "tokenSymbol": "USDT",
            "buyerAgentId": "user-1",
            "serviceParams": "{\"chain\":\"xlayer\"}"
        });

        let output: serde_json::Value = serde_json::from_str(&result_from_task_detail(
            "job-1",
            "provider-1",
            Ok(task),
        ))
        .unwrap();

        assert_eq!(output["phase"], "task_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "finalize_asp_task");
        assert_eq!(output["payload"]["rating"]["targetAgentId"], "user-1");
        assert_eq!(output["payload"]["rating"]["creatorAgentId"], "provider-1");
        assert_eq!(
            output["payload"]["rating"]["taskDescription"],
            "Audit the contract"
        );
        assert_eq!(
            output["payload"]["notification"],
            "[💰 Job Completed] Job job-1 (Audit report) — approved by the User Agent; funds received.\n      - Income: 12 USDT\n      - User Agent: user-1\n    \n    This job is complete."
        );
        assert!(output["payload"]["ratingResultNotification"]
            .as_str()
            .unwrap()
            .contains("<score>"));
    }

    #[test]
    fn blocks_when_task_detail_request_fails() {
        let output: serde_json::Value = serde_json::from_str(&result_from_task_detail(
            "job-1",
            "provider-1",
            Err(anyhow::anyhow!("request failed")),
        ))
        .unwrap();

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "task_detail_unavailable");
        assert_eq!(output["nextAction"][0]["id"], "stop");
    }

    #[test]
    fn blocks_when_task_detail_job_id_mismatches() {
        let output: serde_json::Value = serde_json::from_str(&result_from_task_detail(
            "job-1",
            "provider-1",
            Ok(serde_json::json!({ "jobId": "job-2", "status": 6 })),
        ))
        .unwrap();

        assert_eq!(output["reason"], "task_detail_job_id_mismatch");
    }

    #[test]
    fn blocks_when_task_status_is_not_completed() {
        let output: serde_json::Value = serde_json::from_str(&result_from_task_detail(
            "job-1",
            "provider-1",
            Ok(serde_json::json!({ "jobId": "job-1", "status": 2 })),
        ))
        .unwrap();

        assert_eq!(output["reason"], "stale_task_status");
    }
}
