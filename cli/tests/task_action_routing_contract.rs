const ACTION_ROUTING: &str =
    include_str!("../../skills/okx-ai/references/task-action-routing.md");

#[test]
fn service_param_response_action_is_registered() {
    assert!(ACTION_ROUTING.contains("| `send_task_params_response` |"));
    assert!(ACTION_ROUTING.contains("`task-asp-accept.md`, `NEED_PARAMS`"));
    assert!(ACTION_ROUTING.contains("`okx-a2a session send`"));
}
