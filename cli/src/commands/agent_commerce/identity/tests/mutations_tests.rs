use super::*;

// ─── extract_agent_id_from_push ───────────────────────────────────────

#[test]
fn extract_agent_id_string_from_push() {
    let push = json!({ "agentId": "12345", "txHash": "0xabc" });
    assert_eq!(extract_agent_id_from_push(Some(&push)).as_deref(), Some("12345"));
}

#[test]
fn extract_agent_id_numeric_stringified() {
    let push = json!({ "agentId": 777 });
    assert_eq!(extract_agent_id_from_push(Some(&push)).as_deref(), Some("777"));
}

#[test]
fn extract_agent_id_missing_push_is_none() {
    assert_eq!(extract_agent_id_from_push(None), None);
}

#[test]
fn extract_agent_id_missing_field_is_none() {
    let push = json!({ "txHash": "0xabc" });
    assert_eq!(extract_agent_id_from_push(Some(&push)), None);
}

#[test]
fn envelope_always_carries_new_agent_id_key() {
    // Present case.
    let env = assemble_identity_envelope("0xhash".to_string(), None, Some("42".to_string()));
    assert_eq!(env["newAgentId"], json!("42"));
    // Null case — key still present.
    let env = assemble_identity_envelope("0xhash".to_string(), None, None);
    assert_eq!(env["newAgentId"], Value::Null);
    assert!(env.as_object().unwrap().contains_key("newAgentId"));
}

#[test]
fn envelope_with_push_populates_agent_key() {
    let push = json!({ "agentId": "77", "txHash": "0xaaa" });
    let env =
        assemble_identity_envelope("0xhash".to_string(), Some(push.clone()), Some("77".to_string()));
    assert_eq!(env["txHash"], json!("0xhash"));
    assert_eq!(env["agent"], push);
    assert_eq!(env["newAgentId"], json!("77"));
}

#[test]
fn envelope_without_push_omits_agent_key() {
    let env = assemble_identity_envelope("0xhash".to_string(), None, Some("42".to_string()));
    // `agent` key must be absent (not null) when no push arrived.
    assert!(env.get("agent").is_none());
    assert_eq!(env["newAgentId"], json!("42"));
}

// ─── parse_agent_info_row ─────────────────────────────────────────────

#[test]
fn parse_agent_info_row_string_roles_are_none() {
    // The live backend returns `role` as the integer code 1/2/3; a string role
    // (canonical, legacy enum, or alias) is not a backend form → does not parse.
    for role in ["asp", "user", "evaluator", "provider", "requester", "buyer"] {
        let row = json!({ "role": role, "name": "Agent" });
        assert!(parse_agent_info_row(&row).is_none(), "string role {role:?} must not parse");
    }
}

#[test]
fn parse_agent_info_row_integer_roles() {
    for (n, expected) in [(1u64, "user"), (2, "asp"), (3, "evaluator")] {
        let row = json!({ "role": n, "name": "Agent" });
        let info = parse_agent_info_row(&row)
            .unwrap_or_else(|| panic!("should parse integer role={n}"));
        assert_eq!(info.role, expected);
    }
}

#[test]
fn parse_agent_info_row_unknown_role_returns_none() {
    let row = json!({ "role": "seller", "name": "Unknown" });
    assert!(parse_agent_info_row(&row).is_none(), "unknown role must return None");
}

#[test]
fn parse_agent_info_row_empty_string_role_returns_none() {
    let row = json!({ "role": "", "name": "Empty" });
    assert!(parse_agent_info_row(&row).is_none());
}

#[test]
fn parse_agent_info_row_missing_role_returns_none() {
    let row = json!({ "name": "NoRole" });
    assert!(parse_agent_info_row(&row).is_none());
}

#[test]
fn parse_agent_info_row_reads_description_field() {
    let row = json!({ "role": 2, "name": "A", "description": "Legacy desc" });
    let info = parse_agent_info_row(&row).unwrap();
    assert_eq!(info.description, "Legacy desc");
}

#[test]
fn parse_agent_info_row_reads_profile_description_field() {
    // Live backend uses `profileDescription`.
    let row = json!({ "role": 2, "name": "A", "profileDescription": "Live desc" });
    let info = parse_agent_info_row(&row).unwrap();
    assert_eq!(info.description, "Live desc");
}

#[test]
fn parse_agent_info_row_description_wins_over_profile_description() {
    // The code checks ["description", "profileDescription"] in order — first non-empty wins.
    let row = json!({
        "role": 2,
        "name": "A",
        "description": "first",
        "profileDescription": "second",
    });
    let info = parse_agent_info_row(&row).unwrap();
    assert_eq!(info.description, "first");
}

#[test]
fn parse_agent_info_row_missing_name_and_desc_defaults_to_empty() {
    let row = json!({ "role": 1 });
    let info = parse_agent_info_row(&row).unwrap();
    assert_eq!(info.name, "");
    assert_eq!(info.description, "");
}

// ─── build_erc8004_overlay: task_id inclusion / omission ─────────────────
// The feedback_submit_impl builds an erc8004 overlay with ("taskId", &task_id).
// build_erc8004_overlay filters empty strings, so an absent / empty task_id
// must not produce a "taskId" key in the overlay.

#[test]
fn erc8004_overlay_with_non_empty_task_id_includes_taskid() {
    let overlay = build_erc8004_overlay(&[
        ("taskId", "JOB-123"),
        ("feedBackAgentId", "agent-42"),
    ]);
    let overlay = overlay.expect("non-empty fields must produce Some(overlay)");
    let inner = overlay["erc8004Msg"].as_object().unwrap();
    assert_eq!(inner.get("taskId").and_then(|v| v.as_str()), Some("JOB-123"));
}

#[test]
fn erc8004_overlay_with_empty_task_id_omits_taskid() {
    // Mirrors: task_id = trim_or_empty(args.task_id.as_deref()); → empty string.
    let overlay = build_erc8004_overlay(&[
        ("taskId", ""),           // empty → filtered
        ("feedBackAgentId", "agent-42"),
    ]);
    let overlay = overlay.expect("feedBackAgentId is non-empty → Some(overlay)");
    let inner = overlay["erc8004Msg"].as_object().unwrap();
    assert!(
        inner.get("taskId").is_none(),
        "empty taskId must be omitted; got {inner:?}"
    );
}

#[test]
fn erc8004_overlay_all_empty_fields_returns_none() {
    // If every field is empty, the overlay itself must be None (no erc8004Msg).
    let overlay = build_erc8004_overlay(&[("taskId", ""), ("feedBackAgentId", "")]);
    assert!(overlay.is_none(), "all-empty fields must yield None");
}

#[test]
fn erc8004_overlay_task_id_present_without_feedback_agent() {
    // taskId alone (feedBackAgentId absent) is still a valid overlay.
    let overlay = build_erc8004_overlay(&[("taskId", "JOB-XYZ"), ("feedBackAgentId", "")]);
    let overlay = overlay.expect("non-empty taskId must produce Some(overlay)");
    let inner = overlay["erc8004Msg"].as_object().unwrap();
    assert_eq!(inner.get("taskId").and_then(|v| v.as_str()), Some("JOB-XYZ"));
    assert!(inner.get("feedBackAgentId").is_none());
}
