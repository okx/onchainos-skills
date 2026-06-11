use super::*;

fn two_account_envelope() -> Value {
    json!({
        "total": 2,
        "list": [
            {
                "ownerAddress": "0xSIGNER",
                "accountName": "wallet-1",
                "agentList": [
                    { "agentId": 10, "name": "old-a" },
                    { "agentId": 11, "name": "old-b" },
                    { "agentId": 99, "name": "new-one" },
                ],
            },
            {
                // Another derived wallet — must NOT be conflated.
                "ownerAddress": "0xOTHER",
                "accountName": "wallet-2",
                "agentList": [ { "agentId": 500, "name": "someone-else" } ],
            },
        ],
    })
}

/// Live `/agent-list` shape: single-layer `list[*]`, each row IS an agent
/// carrying its own `ownerAddress` (no `agentList` wrapper). `total` =
/// agent count.
fn single_layer_envelope() -> Value {
    json!({
        "total": 3,
        "list": [
            { "agentId": 10, "name": "old-a", "ownerAddress": "0xSIGNER" },
            { "agentId": 11, "name": "old-b", "ownerAddress": "0xSIGNER" },
            { "agentId": 99, "name": "new-one", "ownerAddress": "0xSIGNER" },
        ],
    })
}

#[test]
fn new_agent_id_diff_single_layer_one_new_row() {
    // The real backend shape: flat rows with per-row ownerAddress.
    let list = single_layer_envelope();
    let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11"));
    assert_eq!(got.as_deref(), Some("99"));
}

#[test]
fn new_agent_id_diff_single_layer_case_insensitive_and_no_owner_ok() {
    // Case-insensitive owner match; a row without ownerAddress is treated
    // as the caller's own (endpoint is JWT-scoped).
    let list = json!({
        "total": 2,
        "list": [
            { "agentId": 10, "name": "old", "ownerAddress": "0xSIGNER" },
            { "agentId": 99, "name": "new" }, // no ownerAddress → owned
        ],
    });
    let got = compute_new_agent_id(None, Some(&list), "0xsigner", Some("10"));
    assert_eq!(got.as_deref(), Some("99"));
}

#[test]
fn new_agent_id_diff_single_layer_zero_and_two_candidates_are_null() {
    let list = single_layer_envelope();
    // all known → none new → null.
    assert_eq!(
        compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11,99")),
        None
    );
    // only 10 known → 11 and 99 both new → ambiguous → null.
    assert_eq!(
        compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10")),
        None
    );
}

#[test]
fn new_agent_id_prefers_ws_push() {
    let push = json!({ "agentId": "12345", "txHash": "0xabc" });
    let list = two_account_envelope();
    // Even with a known-ids snapshot present, the WS push wins.
    let got = compute_new_agent_id(Some(&push), Some(&list), "0xSIGNER", Some("10,11,99"));
    assert_eq!(got.as_deref(), Some("12345"));
}

#[test]
fn new_agent_id_ws_push_numeric_id_stringified() {
    let push = json!({ "agentId": 777 });
    let got = compute_new_agent_id(Some(&push), None, "0xSIGNER", None);
    assert_eq!(got.as_deref(), Some("777"));
}

#[test]
fn new_agent_id_diff_one_new_row() {
    let list = two_account_envelope();
    let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11"));
    assert_eq!(got.as_deref(), Some("99"));
}

#[test]
fn new_agent_id_diff_case_insensitive_owner_match() {
    let list = two_account_envelope();
    // Signing address differs in case from the wrapper's ownerAddress.
    let got = compute_new_agent_id(None, Some(&list), "0xsigner", Some("10,11"));
    assert_eq!(got.as_deref(), Some("99"));
}

#[test]
fn new_agent_id_diff_zero_candidates_is_null() {
    let list = two_account_envelope();
    // All ids known → no new candidate.
    let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10,11,99"));
    assert_eq!(got, None);
}

#[test]
fn new_agent_id_diff_two_candidates_is_null() {
    let list = two_account_envelope();
    // Only 10 known → 11 and 99 are both "new" → ambiguous → null.
    let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", Some("10"));
    assert_eq!(got, None);
}

#[test]
fn new_agent_id_no_matching_wrapper_is_null() {
    let list = two_account_envelope();
    let got = compute_new_agent_id(None, Some(&list), "0xNOBODY", Some("10,11"));
    assert_eq!(got, None);
}

#[test]
fn new_agent_id_without_known_ids_and_no_push_is_null() {
    let list = two_account_envelope();
    // Rule 2 requires --known-agent-ids; absent → null (never errors).
    let got = compute_new_agent_id(None, Some(&list), "0xSIGNER", None);
    assert_eq!(got, None);
}

#[test]
fn envelope_always_carries_new_agent_id_key() {
    // Present case.
    let env = assemble_identity_envelope("0xhash".to_string(), None, None, Some("42".to_string()));
    assert_eq!(env["newAgentId"], json!("42"));
    // Null case — key still present.
    let env = assemble_identity_envelope("0xhash".to_string(), None, None, None);
    assert_eq!(env["newAgentId"], Value::Null);
    assert!(env.as_object().unwrap().contains_key("newAgentId"));
}
