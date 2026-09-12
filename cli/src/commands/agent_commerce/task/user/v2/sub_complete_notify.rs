use crate::commands::agent_commerce::task::common::{
    deliverables, has_same_agent_owner, network::task_api_client::TaskApiClient, onchainos_self,
    session_cleanup, util::validate_job_id, PreFetchedTaskContext, DEBUG_LOG,
};

const MAX_SAMPLE_DELIVERABLES: usize = 5;
const MAX_TEXT_PREVIEW_CHARS: usize = 500;

pub(crate) async fn handle(
    agent_id: &str,
    message: Option<&serde_json::Value>,
) -> serde_json::Value {
    let job_id = match message_job_id(message) {
        Some(value) => value,
        None => return blocked_result("job_id_required", None, None),
    };
    if let Err(error) = validate_job_id(job_id) {
        return blocked_result("invalid_job_id", Some(job_id), Some(&error));
    }

    let mut client = TaskApiClient::new();
    let response = match client
        .get_with_identity(&client.task_path(job_id), agent_id)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let _ = session_cleanup::handle_session_cleanup(job_id, false);
            return blocked_result(
                "task_detail_unavailable",
                Some(job_id),
                Some(&error.to_string()),
            );
        }
    };
    if response.get("jobId").and_then(serde_json::Value::as_str) != Some(job_id) {
        let _ = session_cleanup::handle_session_cleanup(job_id, false);
        return blocked_result(
            "task_detail_job_id_mismatch",
            Some(job_id),
            Some("response jobId does not match message.jobId"),
        );
    }
    let task = PreFetchedTaskContext::from_api_response(&response);
    let human_rating_allowed = can_rate_subscription(&response, &task);
    let notification = completion_notification(job_id, &task, human_rating_allowed);

    let rating = if human_rating_allowed {
        build_rating_payload(agent_id, job_id, &task)
    } else {
        serde_json::json!({ "required": false })
    };
    success_result(job_id, task_title(&task), &notification, rating)
}

fn success_result(
    job_id: &str,
    title: &str,
    notification: &str,
    rating: serde_json::Value,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "jobId": job_id,
        "notification": {
            "content": notification,
            "localize": true,
        },
        "rating": rating,
    });
    if rating["required"].as_bool() == Some(true) {
        payload["ratingResultNotification"] = serde_json::json!(
            super::super::content::rating_submitted_user_notify(job_id, title)
        );
    }

    serde_json::json!({
        "phase": "subscription_completion",
        "decision": "ready",
        "reason": "notification_required",
        "nextAction": [{
            "id": "finalize_user_subscription",
            "recommend": true,
        }],
        "payload": payload,
    })
}

fn message_job_id(message: Option<&serde_json::Value>) -> Option<&str> {
    message
        .and_then(|value| value.get("jobId"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
}

fn blocked_result(reason: &str, job_id: Option<&str>, error: Option<&str>) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    if let Some(job_id) = job_id {
        payload.insert("jobId".to_string(), serde_json::json!(job_id));
    }
    if let Some(error) = error {
        payload.insert("error".to_string(), serde_json::json!(error));
    }
    serde_json::json!({
        "phase": "subscription_completion",
        "decision": "blocked",
        "reason": reason,
        "nextAction": [{ "id": "stop" }],
        "payload": payload,
    })
}

fn completion_notification(
    job_id: &str,
    task: &PreFetchedTaskContext,
    include_rating_invitation: bool,
) -> String {
    super::super::content::sub_complete_notify_user_notify(
        task_title(task),
        job_id,
        None,
        include_rating_invitation,
    )
}

fn task_title(task: &PreFetchedTaskContext) -> &str {
    if task.title.is_empty() {
        "subscription"
    } else {
        task.title.as_str()
    }
}

fn can_rate_subscription(response: &serde_json::Value, task: &PreFetchedTaskContext) -> bool {
    !has_same_agent_owner(response)
        && task
            .provider_agent_id
            .as_deref()
            .is_some_and(|value| !value.is_empty())
}

fn build_rating_payload(
    agent_id: &str,
    job_id: &str,
    task: &PreFetchedTaskContext,
) -> serde_json::Value {
    let provider_id = match task
        .provider_agent_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return serde_json::json!({ "required": false }),
    };

    match onchainos_self::task_feedback_exists(agent_id, job_id) {
        Ok(true) => return serde_json::json!({ "required": false }),
        Ok(false) => {}
        Err(error) => {
            if DEBUG_LOG {
                eprintln!(
                    "[sub_complete_notify] feedback lookup failed for {}: {error}",
                    job_id
                );
            }
            return serde_json::json!({ "required": false });
        }
    }

    let description = task.description.as_str();
    let service_params = task.service_params.as_deref();
    let deliverable_context = build_deliverable_sample(job_id);

    rating_payload(
        provider_id,
        agent_id,
        description,
        service_params,
        &deliverable_context,
    )
}

fn rating_payload(
    provider_id: &str,
    user_agent_id: &str,
    description: &str,
    service_params: Option<&str>,
    deliverables: &str,
) -> serde_json::Value {
    serde_json::json!({
        "required": true,
        "providerAgentId": provider_id,
        "creatorAgentId": user_agent_id,
        "taskDescription": description,
        "taskParameters": service_params.filter(|value| !value.is_empty()),
        "deliverables": deliverables,
    })
}

fn build_deliverable_sample(job_id: &str) -> String {
    let manifest = match deliverables::read_manifest("user", job_id) {
        Ok(Some(manifest)) if !manifest.entries.is_empty() => manifest,
        _ => return "Deliverables: none found.\n".to_string(),
    };
    let directory = match deliverables::deliverables_dir("user", job_id) {
        Ok(directory) => directory,
        Err(_) => return "Deliverables: directory unavailable.\n".to_string(),
    };
    let indices = pick_sample_indices(manifest.entries.len(), MAX_SAMPLE_DELIVERABLES, job_id);
    let mut samples = Vec::with_capacity(indices.len());

    for (position, index) in indices.into_iter().enumerate() {
        let entry = &manifest.entries[index];
        let path = directory.join(&entry.filename);
        let mut sample = format!(
            "{}. {} (type: {}, path: {})",
            position + 1,
            entry.original_name,
            entry.deliverable_type,
            path.display(),
        );
        if entry.deliverable_type == "text" {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let preview: String = content.chars().take(MAX_TEXT_PREVIEW_CHARS).collect();
                let suffix = if content.chars().count() > MAX_TEXT_PREVIEW_CHARS {
                    "...(truncated)"
                } else {
                    ""
                };
                sample.push_str(&format!("\n   Preview: {preview}{suffix}"));
            }
        }
        samples.push(sample);
    }

    format!(
        "Deliverables ({} total, sampled {}):\n{}\n",
        manifest.entries.len(),
        samples.len(),
        samples.join("\n"),
    )
}

/// Stable pseudo-random sampling keeps retries idempotent for the same job.
fn pick_sample_indices(total: usize, max_pick: usize, job_id: &str) -> Vec<usize> {
    if total <= max_pick {
        return (0..total).collect();
    }
    let mut random = fnv1a_seed(job_id);
    let mut pool: Vec<usize> = (0..total).collect();
    for index in 0..max_pick {
        random = random
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let selected = index + ((random >> 33) as usize) % (total - index);
        pool.swap(index, selected);
    }
    let mut result = pool[..max_pick].to_vec();
    result.sort_unstable();
    result
}

fn fnv1a_seed(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_indices_are_stable_and_limited_to_five() {
        let first = pick_sample_indices(12, 5, "job-1");
        let second = pick_sample_indices(12, 5, "job-1");

        assert_eq!(first, second);
        assert_eq!(first.len(), 5);
        assert!(first.iter().all(|index| *index < 12));
    }

    #[test]
    fn sample_indices_include_all_when_there_are_five_or_fewer() {
        assert_eq!(pick_sample_indices(3, 5, "job-1"), vec![0, 1, 2]);
    }

    #[test]
    fn notification_reads_job_id_only() {
        let message = serde_json::json!({
            "jobId": "job-1",
            "jobTitle": "Ignored notification title",
            "description": "Ignored notification description",
        });

        assert_eq!(message_job_id(Some(&message)), Some("job-1"));
        assert_eq!(message_job_id(Some(&serde_json::json!({}))), None);
    }

    #[test]
    fn task_detail_drives_completion_content() {
        let task = PreFetchedTaskContext::from_api_response(&serde_json::json!({
            "jobId": "job-1",
            "title": "Detail title",
            "description": "Detail description",
            "providerAgentId": "provider-1",
        }));

        let notification = completion_notification("job-1", &task, true);
        let rating = rating_payload(
            "provider-1",
            "user-1",
            &task.description,
            task.service_params.as_deref(),
            "Deliverables: none found.",
        );

        assert!(notification.contains("Detail title"));
        assert!(notification.contains("reply \"Rate job\""));
        assert_eq!(rating["taskDescription"], "Detail description");
    }

    #[test]
    fn same_owner_subscription_omits_rating_invitation() {
        let response = serde_json::json!({
            "jobId": "job-1",
            "title": "Weekly report",
            "buyerAgentAddress": "0xAbC",
            "providerAgentAddress": "0xabc",
            "providerAgentId": "provider-1",
        });
        let task = PreFetchedTaskContext::from_api_response(&response);

        assert!(!can_rate_subscription(&response, &task));
        assert!(!completion_notification("job-1", &task, false).contains("Rate job"));
    }

    #[test]
    fn successful_completion_returns_minimal_structured_action() {
        let output = success_result(
            "job-1",
            "Weekly report",
            "Completed",
            serde_json::json!({ "required": false }),
        );

        assert_eq!(output["phase"], "subscription_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["reason"], "notification_required");
        assert_eq!(output["nextAction"][0]["id"], "finalize_user_subscription");
        assert_eq!(output["payload"]["jobId"], "job-1");
        assert_eq!(output["payload"]["notification"]["content"], "Completed");
        assert_eq!(output["payload"]["notification"]["localize"], true);
        assert_eq!(output["payload"]["rating"]["required"], false);
        assert!(output["payload"].get("ratingResultNotification").is_none());
        assert!(output["payload"].get("cleanup").is_none());
    }

    #[test]
    fn rated_completion_uses_existing_rating_template() {
        let output = success_result(
            "job-1",
            "Weekly report",
            "Completed",
            serde_json::json!({ "required": true }),
        );

        assert_eq!(
            output["payload"]["ratingResultNotification"],
            super::super::super::content::rating_submitted_user_notify("job-1", "Weekly report")
        );
    }

    #[test]
    fn failure_returns_structured_stop_action() {
        let output = blocked_result(
            "task_detail_unavailable",
            Some("job-1"),
            Some("request failed"),
        );

        assert_eq!(output["decision"], "blocked");
        assert_eq!(output["reason"], "task_detail_unavailable");
        assert_eq!(output["nextAction"][0]["id"], "stop");
        assert_eq!(output["payload"]["jobId"], "job-1");
        assert_eq!(output["payload"]["error"], "request failed");
    }

    #[test]
    fn rating_payload_contains_only_scoring_inputs() {
        let rating = rating_payload(
            "provider-1",
            "user-1",
            "Summarize the report",
            Some("{\"language\":\"zh-CN\"}"),
            "Deliverables (1 total, showing 1): report.md",
        );

        assert_eq!(rating["required"], true);
        assert_eq!(rating["providerAgentId"], "provider-1");
        assert_eq!(rating["creatorAgentId"], "user-1");
        assert_eq!(rating["taskParameters"], "{\"language\":\"zh-CN\"}");
        assert!(rating.get("taskId").is_none());
        assert!(rating.get("scoreMax").is_none());
        assert!(rating.get("commentMaxLength").is_none());
        assert!(rating.to_string().contains("report.md"));
        assert!(!rating.to_string().contains("onchainos agent"));
    }
}
