const ACTION_ROUTING: &str =
    include_str!("../../skills/okx-ai/references/task-action-routing.md");

#[test]
fn service_param_response_action_is_registered() {
    assert!(ACTION_ROUTING.contains("| `send_task_params_response` |"));
    assert!(ACTION_ROUTING.contains("`task-asp-accept.md`, `NEED_PARAMS`"));
    assert!(ACTION_ROUTING.contains("`okx-a2a session send`"));
}

#[test]
fn notification_action_is_registered() {
    const COMPLETION_ACTIONS: &str =
        include_str!("../../skills/okx-ai/references/task-actions-completion.md");

    assert!(ACTION_ROUTING.contains("| `notify_user` |"));
    assert!(ACTION_ROUTING.contains("task-actions-completion.md#notification-only"));
    assert!(COMPLETION_ACTIONS.contains("For `nextAction.id=notify_user`:"));
    assert!(COMPLETION_ACTIONS.contains("payload.notification.content"));
    assert!(COMPLETION_ACTIONS.contains("onchainos agent user-notify"));
}
