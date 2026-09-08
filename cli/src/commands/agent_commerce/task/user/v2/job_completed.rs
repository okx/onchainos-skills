use crate::commands::agent_commerce::task::common::{
    deliverables, has_same_agent_owner, network::task_api_client::TaskApiClient, onchainos_self,
    PreFetchedTaskContext, DEBUG_LOG, TERMINAL_NOTIFICATION_MARKER,
};

pub(crate) async fn handle(job_id: &str, agent_id: &str) -> serde_json::Value {
    let mut client = TaskApiClient::new();
    let response = client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await;
    let mut result = result_from_task_detail(job_id, agent_id, response);
    if result["payload"]["rating"]["required"].as_bool() == Some(true) {
        let feedback_exists = onchainos_self::task_feedback_exists(agent_id, job_id);
        preserve_existing_user_rating(&mut result, feedback_exists);
    }
    result
}

fn preserve_existing_user_rating(
    result: &mut serde_json::Value,
    feedback_exists: anyhow::Result<bool>,
) {
    match feedback_exists {
        Ok(false) => return,
        Ok(true) => {}
        Err(error) => {
            if DEBUG_LOG {
                eprintln!(
                    "[job_completed] feedback lookup failed for {}: {error}",
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
) -> serde_json::Value {
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
    let provider_agent_id = match task
        .provider_agent_id
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
            "id": "finalize_user_task",
            "recommend": true,
        }],
        "payload": {
            "jobId": job_id,
            "notification": {
                "content": completion_notification(job_id, &task),
                "localize": true,
            },
            "ratingResultNotification": super::super::content::rating_submitted_user_notify(
                job_id,
                title(&task),
            ),
            "rating": {
                "required": rating_required,
                "targetAgentId": provider_agent_id,
                "creatorAgentId": agent_id,
                "taskDescription": task.description,
                "taskParameters": task.service_params.as_deref().filter(|value| !value.is_empty()),
                "deliverables": deliverable_files(job_id),
                "taskAttachments": super::super::attachments::list_attachment_paths(job_id),
            },
        },
    })
}

fn blocked_result(job_id: &str, reason: &str) -> serde_json::Value {
    serde_json::json!({
        "phase": "task_completion",
        "decision": "blocked",
        "reason": reason,
        "nextAction": [{ "id": "stop" }],
        "payload": { "jobId": job_id },
    })
}

fn title(task: &PreFetchedTaskContext) -> &str {
    if task.title.is_empty() {
        "Task"
    } else {
        task.title.as_str()
    }
}

fn completion_notification(job_id: &str, task: &PreFetchedTaskContext) -> String {
    let content = if task.payment_mode == Some(3) {
        format!(
            "[x402 Job Completed] {} (`{job_id}`) — all steps complete.\n- Spent: {} {}\n- Payment: x402\n\nTo rate this job, reply \"Rate job\". Your rating for Job ID `{job_id}` replaces the AI-generated rating.",
            title(task), task.token_amount, task.token_symbol,
        )
    } else {
        super::super::content::job_completed_escrow_user_notify(
            job_id,
            title(task),
            &task.token_amount,
            &task.token_symbol,
        )
    };
    format!("{TERMINAL_NOTIFICATION_MARKER} {content}")
}

fn deliverable_files(job_id: &str) -> Vec<serde_json::Value> {
    let manifest = match deliverables::read_manifest("user", job_id) {
        Ok(Some(manifest)) => manifest,
        _ => return Vec::new(),
    };
    let directory = match deliverables::deliverables_dir("user", job_id) {
        Ok(directory) => directory,
        Err(_) => return Vec::new(),
    };

    manifest
        .entries
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.original_name,
                "type": entry.deliverable_type,
                "path": directory.join(entry.filename).display().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_user_completion_action_with_scoring_context() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "description": "Audit the contract",
            "paymentMode": 1,
            "tokenAmount": "12",
            "tokenSymbol": "USDT",
            "providerAgentId": "provider-1",
            "serviceParams": "{\"chain\":\"xlayer\"}"
        });

        let output = result_from_task_detail("job-1", "user-1", Ok(task));

        assert_eq!(output["phase"], "task_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "finalize_user_task");
        assert_eq!(output["payload"]["rating"]["targetAgentId"], "provider-1");
        assert_eq!(output["payload"]["rating"]["creatorAgentId"], "user-1");
        assert_eq!(output["payload"]["rating"]["required"], true);
        assert_eq!(
            output["payload"]["rating"]["taskDescription"],
            "Audit the contract"
        );
        assert_eq!(
            output["payload"]["rating"]["taskParameters"],
            "{\"chain\":\"xlayer\"}"
        );
        assert_eq!(output["payload"]["notification"]["localize"], true);
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .starts_with(TERMINAL_NOTIFICATION_MARKER));
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("Audit report"));
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("reply \"Rate job\""));
        assert!(output["payload"]["ratingResultNotification"]
            .as_str()
            .unwrap()
            .contains("<score>"));
    }

    #[test]
    fn same_owner_skips_user_rating_but_keeps_terminal_marker() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "buyerAgentAddress": "0xAbC",
            "providerAgentAddress": "0xabc",
            "providerAgentId": "provider-1"
        });

        let output = result_from_task_detail("job-1", "user-1", Ok(task));

        assert_eq!(output["reason"], "notification_required");
        assert_eq!(output["payload"]["rating"]["required"], false);
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .starts_with(TERMINAL_NOTIFICATION_MARKER));
    }

    #[test]
    fn existing_user_rating_is_not_overwritten_by_completion_ai() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "providerAgentId": "provider-1"
        });
        let mut output = result_from_task_detail("job-1", "user-1", Ok(task));

        preserve_existing_user_rating(&mut output, Ok(true));

        assert_eq!(output["reason"], "notification_required");
        assert_eq!(output["payload"]["rating"]["required"], false);
        assert!(output["payload"].get("ratingResultNotification").is_none());
    }

    #[test]
    fn failed_feedback_lookup_does_not_risk_overwriting_user_rating() {
        let task = serde_json::json!({
            "jobId": "job-1",
            "status": 6,
            "title": "Audit report",
            "providerAgentId": "provider-1"
        });
        let mut output = result_from_task_detail("job-1", "user-1", Ok(task));

        preserve_existing_user_rating(&mut output, Err(anyhow::anyhow!("lookup failed")));

        assert_eq!(output["payload"]["rating"]["required"], false);
    }

    #[test]
    fn blocks_when_task_detail_request_fails() {
        let output =
            result_from_task_detail("job-1", "user-1", Err(anyhow::anyhow!("request failed")));

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "task_detail_unavailable");
        assert_eq!(output["nextAction"][0]["id"], "stop");
    }

    #[test]
    fn blocks_when_task_detail_job_id_mismatches() {
        let output = result_from_task_detail(
            "job-1",
            "user-1",
            Ok(serde_json::json!({ "jobId": "job-2", "status": 6 })),
        );

        assert_eq!(output["reason"], "task_detail_job_id_mismatch");
    }

    #[test]
    fn blocks_when_task_status_is_not_completed() {
        let output = result_from_task_detail(
            "job-1",
            "user-1",
            Ok(serde_json::json!({ "jobId": "job-1", "status": 2 })),
        );

        assert_eq!(output["reason"], "stale_task_status");
    }
}
