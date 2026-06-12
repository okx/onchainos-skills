use super::*;

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
