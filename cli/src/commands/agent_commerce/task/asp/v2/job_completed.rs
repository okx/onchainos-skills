use crate::commands::agent_commerce::task::common::PreFetchedTaskContext;

pub(crate) fn handle(job_id: &str, agent_id: &str, task: Option<&PreFetchedTaskContext>) -> String {
    let task = match task {
        Some(task) => task,
        None => return blocked_result(job_id),
    };
    let user_agent_id = match task
        .user_agent_id
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
            "id": "finalize_asp_task",
            "recommend": true,
        }],
        "payload": {
            "jobId": job_id,
            "notification": completion_notification(job_id, task),
            "ratingResultNotification": rating_notification(job_id, task),
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

fn blocked_result(job_id: &str) -> String {
    serde_json::json!({
        "phase": "task_completion",
        "decision": "blocked",
        "reason": "task_detail_unavailable",
        "nextAction": [{ "id": "stop" }],
        "payload": { "jobId": job_id },
    })
    .to_string()
}

fn completion_notification(job_id: &str, task: &PreFetchedTaskContext) -> String {
    format!(
        "[💰 Job Completed] {} (`{job_id}`) — funds received.\n- Income: {} {}\n- User Agent: {}",
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
        let task = PreFetchedTaskContext::from_api_response(&serde_json::json!({
            "title": "Audit report",
            "description": "Audit the contract",
            "paymentMode": 1,
            "tokenAmount": "12",
            "tokenSymbol": "USDT",
            "buyerAgentId": "user-1",
            "serviceParams": "{\"chain\":\"xlayer\"}"
        }));

        let output: serde_json::Value =
            serde_json::from_str(&handle("job-1", "provider-1", Some(&task))).unwrap();

        assert_eq!(output["phase"], "task_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["nextAction"][0]["id"], "finalize_asp_task");
        assert_eq!(output["payload"]["rating"]["targetAgentId"], "user-1");
        assert_eq!(output["payload"]["rating"]["creatorAgentId"], "provider-1");
        assert_eq!(
            output["payload"]["rating"]["taskDescription"],
            "Audit the contract"
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
        let output: serde_json::Value =
            serde_json::from_str(&handle("job-1", "provider-1", None)).unwrap();

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "task_detail_unavailable");
        assert_eq!(output["nextAction"][0]["id"], "stop");
    }
}
