pub(crate) fn handle(
    job_id: &str,
    title: Option<&str>,
    period_end: Option<i64>,
) -> String {
    let notification =
        super::super::content::sub_complete_notify_asp_notify(title, job_id, period_end);
    serde_json::json!({
        "phase": "subscription_completion",
        "decision": "ready",
        "reason": "notification_required",
        "nextAction": [{
            "id": "notify_and_cleanup_subscription",
            "recommend": true,
            "params": { "jobId": job_id },
        }],
        "payload": {
            "role": "asp",
            "jobId": job_id,
            "notification": {
                "content": notification,
                "localize": true,
            },
            "rating": { "required": false },
            "cleanup": { "jobId": job_id },
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asp_completion_returns_structured_progression_without_rating() {
        let output: serde_json::Value =
            serde_json::from_str(&handle("job-1", Some("Service A"), None)).unwrap();

        assert_eq!(output["phase"], "subscription_completion");
        assert_eq!(output["decision"], "ready");
        assert_eq!(output["reason"], "notification_required");
        assert_eq!(
            output["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(output["nextAction"][0]["params"]["jobId"], "job-1");
        assert_eq!(output["payload"]["role"], "asp");
        assert_eq!(output["payload"]["rating"]["required"], false);
        assert_eq!(output["payload"]["cleanup"]["jobId"], "job-1");
        assert!(output["payload"]["notification"]["content"]
            .as_str()
            .unwrap()
            .contains("Service A"));
    }
}
