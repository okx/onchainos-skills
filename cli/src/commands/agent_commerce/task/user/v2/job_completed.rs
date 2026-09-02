use crate::commands::agent_commerce::task::common::{deliverables, PreFetchedTaskContext};

pub(crate) fn handle(
    job_id: &str,
    agent_id: &str,
    task: Option<&PreFetchedTaskContext>,
) -> serde_json::Value {
    let task = match task {
        Some(task) => task,
        None => return blocked_result(job_id),
    };
    let provider_agent_id = match task
        .provider_agent_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return blocked_result(job_id),
    };

    serde_json::json!({
        "phase": "task_completion",
        "decision": "ready",
        "reason": "notification_and_rating_required",
        "nextAction": [{
            "id": "finalize_user_task",
            "recommend": true,
        }],
        "payload": {
            "jobId": job_id,
            "notification": completion_notification(job_id, task),
            "ratingResultNotification": super::super::content::rating_submitted_user_notify(
                job_id,
                title(task),
            ),
            "rating": {
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

fn blocked_result(job_id: &str) -> serde_json::Value {
    serde_json::json!({
        "phase": "task_completion",
        "decision": "blocked",
        "reason": "task_detail_unavailable",
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
    if task.payment_mode == Some(3) {
        format!(
            "[x402 Job Completed] {} (`{job_id}`) — all steps complete.\n- Spent: {} {}\n- Payment: x402",
            title(task), task.token_amount, task.token_symbol,
        )
    } else {
        super::super::content::job_completed_escrow_user_notify(
            job_id,
            title(task),
            &task.token_amount,
            &task.token_symbol,
        )
    }
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
        let task = PreFetchedTaskContext::from_api_response(&serde_json::json!({
            "title": "Audit report",
            "description": "Audit the contract",
            "paymentMode": 1,
            "tokenAmount": "12",
            "tokenSymbol": "USDT",
            "providerAgentId": "provider-1",
            "serviceParams": "{\"chain\":\"xlayer\"}"
        }));

        let output = handle("job-1", "user-1", Some(&task));

        assert_eq!(output["phase"], "task_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "finalize_user_task");
        assert_eq!(output["payload"]["rating"]["targetAgentId"], "provider-1");
        assert_eq!(output["payload"]["rating"]["creatorAgentId"], "user-1");
        assert_eq!(
            output["payload"]["rating"]["taskDescription"],
            "Audit the contract"
        );
        assert_eq!(
            output["payload"]["rating"]["taskParameters"],
            "{\"chain\":\"xlayer\"}"
        );
        assert!(output["payload"]["notification"]
            .as_str()
            .unwrap()
            .contains("Audit report"));
        assert!(output["payload"]["ratingResultNotification"]
            .as_str()
            .unwrap()
            .contains("<score>"));
    }

    #[test]
    fn blocks_when_task_detail_is_unavailable() {
        let output = handle("job-1", "user-1", None);

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "task_detail_unavailable");
        assert_eq!(output["nextAction"][0]["id"], "stop");
    }
}
