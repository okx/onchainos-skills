use crate::commands::agent_commerce::task::common::{
    has_same_agent_owner, network::task_api_client::TaskApiClient, onchainos_self,
    PreFetchedTaskContext, DEBUG_LOG, TERMINAL_NOTIFICATION_MARKER,
};

pub(crate) async fn handle(job_id: &str, agent_id: &str) -> String {
    let mut client = TaskApiClient::new();
    let response = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await;
    let result = result_from_task_detail(job_id, agent_id, response);
    let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(&result) else {
        return result;
    };
    if parsed["payload"]["rating"]["required"].as_bool() == Some(true) {
        let feedback_exists = onchainos_self::task_feedback_exists(agent_id, job_id);
        preserve_existing_provider_rating(&mut parsed, feedback_exists);
    }
    parsed.to_string()
}

fn preserve_existing_provider_rating(
    result: &mut serde_json::Value,
    feedback_exists: anyhow::Result<bool>,
) {
    match feedback_exists {
        Ok(false) => return,
        Ok(true) => {}
        Err(error) => {
            if DEBUG_LOG {
                eprintln!(
                    "[asp_job_completed] feedback lookup failed for {}: {error}",
                    result["payload"]["jobId"].as_str().unwrap_or("unknown"),
                );
            }
        }
    }

    result["reason"] = serde_json::json!("notification_required");
    result["payload"]["rating"]["required"] = serde_json::json!(false);
    if let Some(payload) = result["payload"].as_object_mut() {
        payload.remove("ratingResultNotification");
    }
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
    let rating_required = !has_same_agent_owner(&response);
    let reason = if rating_required {
        "notification_and_rating_required"
    } else {
        "notification_required"
    };
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
        "reason": reason,
        "nextAction": [{
            "id": "finalize_asp_task",
            "recommend": true,
        }],
        "payload": {
            "jobId": job_id,
            "notification": {
                "content": completion_notification(job_id, &task),
                "localize": true,
            },
            "ratingResultNotification": rating_notification(job_id, &task),
            "rating": {
                "required": rating_required,
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
        "{TERMINAL_NOTIFICATION_MARKER} [💰 Job Completed] Job {job_id} ({}) — approved by the User Agent; funds received.\n      - Income: {} {}\n      - User Agent: {}\n    \n    This job is complete.\n\n    To rate the User Agent, reply \"Rate User Agent\". Your rating for Job ID `{job_id}` replaces the AI-generated rating.",
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

        let output: serde_json::Value =
            serde_json::from_str(&result_from_task_detail("job-1", "provider-1", Ok(task)))
                .unwrap();

        assert_eq!(output["phase"], "task_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "finalize_asp_task");
        assert_eq!(output["payload"]["rating"]["targetAgentId"], "user-1");
        assert_eq!(output["payload"]["rating"]["creatorAgentId"], "provider-1");
        assert_eq!(output["payload"]["rating"]["required"], true);
        assert_eq!(
            output["payload"]["rating"]["taskDescription"],
            "Audit the contract"
        );
        assert_eq!(
            output["payload"]["notification"]["content"],
            "[onchainos:task-terminal] [💰 Job Completed] Job job-1 (Audit report) — approved by the User Agent; funds received.\n      - Income: 12 USDT\n      - User Agent: user-1\n    \n    This job is complete.\n\n    To rate the User Agent, reply \"Rate User Agent\". Your rating for Job ID `job-1` replaces the AI-generated rating."
        );
        assert_eq!(output["payload"]["notification"]["localize"], true);
        assert!(output["payload"]["ratingResultNotification"]
            .as_str()
            .unwrap()
            .contains("<score>"));
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("reply \"Rate User Agent\""));
    }

    #[test]
    fn existing_provider_rating_is_not_overwritten_by_completion_ai() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "buyerAgentId": "user-1"
        });
        let raw = result_from_task_detail("job-1", "provider-1", Ok(task));
        let mut output: serde_json::Value = serde_json::from_str(&raw).unwrap();

        preserve_existing_provider_rating(&mut output, Ok(true));

        assert_eq!(output["reason"], "notification_required");
        assert_eq!(output["payload"]["rating"]["required"], false);
        assert!(output["payload"].get("ratingResultNotification").is_none());
    }

    #[test]
    fn same_owner_skips_asp_rating_but_keeps_terminal_marker() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "buyerAgentAddress": "0xAbC",
            "providerAgentAddress": "0xabc",
            "buyerAgentId": "user-1"
        });

        let output: serde_json::Value =
            serde_json::from_str(&result_from_task_detail("job-1", "provider-1", Ok(task)))
                .unwrap();

        assert_eq!(output["reason"], "notification_required");
        assert_eq!(output["payload"]["rating"]["required"], false);
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .starts_with(TERMINAL_NOTIFICATION_MARKER));
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
