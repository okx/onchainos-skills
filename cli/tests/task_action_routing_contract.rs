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

#[test]
fn refund_v2_actions_are_registered_and_context_bound() {
    for action in [
        "resolve_refund_target",
        "prepare_refund",
        "provide_refund_reason",
        "cancel_trial_conversion",
        "close_zero_price",
        "execute_direct_refund",
        "submit_refund_request",
        "view_refund_status",
        "view_arbitration",
    ] {
        assert!(
            ACTION_ROUTING.contains(&format!("| `{action}` |")),
            "missing Refund V2 action {action}"
        );
    }
    assert!(ACTION_ROUTING.contains("params.refundContextId"));
    assert!(ACTION_ROUTING.contains("Do not execute an action not returned by the CLI"));
}
