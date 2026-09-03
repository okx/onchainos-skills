use serde_json::json;

use super::{
    amount_semantics, decimal_strings_equal, discover_input_required, input_required_decision,
    invalid_params_decision, merge_field_constraints, normalize_a2mcp_method, outstanding_input,
    outstanding_request_input, parse_probe_input, run_probe, Action, FieldConstraint, ProbeArgs,
    ProbeDecision,
};

fn routing_payload() -> serde_json::Value {
    json!({
        "schemaVersion": 1,
        "serviceSnapshot": {
            "asp": {
                "aspAgentId": "5421",
                "aspName": "PixelBrief",
                "feedbackRate": 96.92,
                "onlineStatus": 1,
                "rating": "★ 4.86",
                "securityRate": 4.86,
                "soldCount": 21721
            },
            "endpoint": "https://pixelbrief.tech/v1/logo",
            "feeAmount": 0.05,
            "feeToken": "0x779ded0c9e1022225f8e0630b35a9b54be713736",
            "feeTokenSymbol": "USDT",
            "freeTrial": null,
            "isSubscribing": false,
            "serviceDescription": "Returns logo SVG and palette for a brand name and mood.\n1. brand name 2. mood 3. optional style",
            "serviceId": "9a5041d8-e03d-461d-b5cd-d2ffdd6111f3",
            "serviceName": "Logo SVG only",
            "serviceType": "A2MCP",
            "sid": 33803,
            "sortOrder": null,
            "subscription": [],
            "supportTrial": false
        }
    })
}

#[test]
fn routing_payload_requires_schema_version_one_and_a2mcp_snapshot() {
    let parsed = parse_probe_input(&routing_payload().to_string(), "{}").expect("valid payload");
    assert_eq!(
        parsed.snapshot.endpoint.as_str(),
        "https://pixelbrief.tech/v1/logo"
    );
    assert!(parsed.typed_params.is_empty());

    let mut bad = routing_payload();
    bad["schemaVersion"] = json!(2);
    assert_eq!(
        parse_probe_input(&bad.to_string(), "{}").unwrap_err().code,
        "invalid_a2mcp_routing"
    );

    let mut wrong_type = routing_payload();
    wrong_type["serviceSnapshot"]["serviceType"] = json!("A2A");
    assert_eq!(
        parse_probe_input(&wrong_type.to_string(), "{}")
            .unwrap_err()
            .code,
        "invalid_a2mcp_routing"
    );

    let mut missing_endpoint = routing_payload();
    missing_endpoint["serviceSnapshot"]
        .as_object_mut()
        .unwrap()
        .remove("endpoint");
    assert_eq!(
        parse_probe_input(&missing_endpoint.to_string(), "{}")
            .unwrap_err()
            .code,
        "invalid_a2mcp_routing"
    );

    let mut insecure_endpoint = routing_payload();
    insecure_endpoint["serviceSnapshot"]["endpoint"] = json!("http://pixelbrief.tech/v1/logo");
    let error = parse_probe_input(&insecure_endpoint.to_string(), "{}").unwrap_err();
    assert_eq!(error.code, "invalid_a2mcp_routing");
    assert!(error.message.contains("must use HTTPS"));

    let mut unsafe_method = routing_payload();
    unsafe_method["requestSpec"] = json!({"method":"DELETE","fields":[]});
    let error = parse_probe_input(&unsafe_method.to_string(), "{}").unwrap_err();
    assert_eq!(error.code, "invalid_a2mcp_routing");
    assert!(error.message.contains("expected GET or POST"));

    let mut null_snapshot = routing_payload();
    null_snapshot["serviceSnapshot"] = serde_json::Value::Null;
    assert_eq!(
        parse_probe_input(&null_snapshot.to_string(), "{}")
            .unwrap_err()
            .code,
        "invalid_a2mcp_routing"
    );
}

#[test]
fn every_structured_a2mcp_method_source_uses_the_get_post_allowlist() {
    assert_eq!(normalize_a2mcp_method(" get ").unwrap(), "GET");
    assert_eq!(normalize_a2mcp_method("post").unwrap(), "POST");
    for method in ["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        assert_eq!(
            normalize_a2mcp_method(method).unwrap_err().code,
            "invalid_a2mcp_routing"
        );
    }
}

#[test]
fn typed_params_preserve_json_types_and_ignore_service_description() {
    let parsed = parse_probe_input(
        &routing_payload().to_string(),
        r#"{"count":2,"enabled":true,"filter":{"kind":"new"}}"#,
    )
    .expect("typed params");

    assert_eq!(parsed.typed_params["count"], json!(2));
    assert_eq!(parsed.typed_params["enabled"], json!(true));
    assert_eq!(parsed.typed_params["filter"], json!({"kind":"new"}));
    assert!(!parsed.typed_params.contains_key("must"));
}

#[test]
fn input_required_uses_only_structured_sources_in_priority_order() {
    let body = json!({
        "input_required": {
            "fields": [{"name":"brand","type":"string","required":true}]
        },
        "outputSchema": {
            "input": [{"name":"ignored","type":"number","required":true}]
        },
        "missingParams": ["also_ignored"]
    });
    let required = discover_input_required(&body).expect("structured requirement");
    assert_eq!(required.fields.len(), 1);
    assert_eq!(required.fields[0].name, "brand");

    let description_only = json!({"message":"serviceDescription says brand is required"});
    assert!(discover_input_required(&description_only).is_none());
}

#[test]
fn a_402_with_missing_fields_is_input_required_before_payment() {
    let body = json!({
        "missingParams": ["brand"],
        "accepts": [{"scheme":"exact", "amount":"50000", "asset":"0x1"}]
    });
    let required = discover_input_required(&body).expect("missing field wins");
    assert_eq!(required.fields[0].name, "brand");
}

#[test]
fn submitted_fields_and_satisfied_any_of_are_not_requested_again() {
    let required = discover_input_required(&json!({
        "missingParams":["brand","count"],
        "requiredAnyOf":["id","slug"]
    }))
    .expect("requirements");
    let params = json!({"brand":"OKX","slug":"okx"})
        .as_object()
        .cloned()
        .expect("object");
    let outstanding = outstanding_input(required, &params).expect("count remains");
    assert_eq!(outstanding.fields.len(), 1);
    assert_eq!(outstanding.fields[0].name, "count");
    assert!(outstanding.required_any_of.is_empty());
}

#[test]
fn required_any_of_is_one_choice_not_all_fields_required() {
    let required = discover_input_required(&json!({
        "input_required": {
            "fields": [
                {"name":"id","type":"string","required":true,"carrier":"query"},
                {"name":"slug","type":"string","required":true,"carrier":"query"}
            ],
            "requiredAnyOf":["id","slug"]
        }
    }))
    .expect("requirements");
    let outstanding = outstanding_input(required, &serde_json::Map::new()).expect("one choice");
    assert!(outstanding.fields.is_empty());
    assert_eq!(outstanding.required_any_of, vec!["id", "slug"]);

    let schema_only = discover_input_required(&json!({
        "outputSchema": {"requiredAnyOf":["id","slug"]}
    }))
    .expect("nested schema alternatives");
    assert!(schema_only.fields.is_empty());
    assert_eq!(schema_only.required_any_of, vec!["id", "slug"]);
}

#[test]
fn request_specs_merge_incremental_fields_without_losing_existing_carriers() {
    let base = vec![FieldConstraint {
        name: "brand".into(),
        type_: "string".into(),
        required: true,
        carrier: Some("body".into()),
        description: None,
    }];
    let updates = vec![FieldConstraint {
        name: "mood".into(),
        type_: "string".into(),
        required: true,
        carrier: Some("header".into()),
        description: None,
    }];
    let merged = merge_field_constraints(&base, &updates);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].carrier.as_deref(), Some("body"));
    assert_eq!(merged[1].carrier.as_deref(), Some("header"));
}

#[test]
fn request_spec_overrides_snapshot_method_and_schema_without_mutating_snapshot() {
    let mut payload = routing_payload();
    payload["requestSpec"] = json!({
        "method":"POST",
        "fields":[{"name":"count","type":"number","carrier":"body"}]
    });
    let parsed = parse_probe_input(&payload.to_string(), r#"{"count":2}"#).expect("payload");
    assert_eq!(parsed.snapshot.method, "POST");
    assert_eq!(
        parsed.snapshot.param_plan[0].carrier.as_deref(),
        Some("body")
    );
    assert!(parsed.snapshot.raw.get("requestSpec").is_none());
}

#[test]
fn decimal_comparison_ignores_non_significant_zeroes() {
    assert!(decimal_strings_equal("0.050", "0.05"));
    assert!(decimal_strings_equal("0001.00", "1"));
    assert!(!decimal_strings_equal("0.05", "0.051"));
}

#[test]
fn upto_amount_is_presented_as_a_maximum_without_scheme_jargon() {
    assert_eq!(amount_semantics("upto"), "maximum");
    assert_eq!(amount_semantics("UPTO"), "maximum");
    assert_eq!(amount_semantics("exact"), "exact");
    assert_eq!(amount_semantics("aggr_deferred"), "exact");
}

#[test]
fn action_without_input_omits_params() {
    let value = serde_json::to_value(Action::new("cancel_a2mcp", false)).expect("serialize action");
    assert_eq!(value, json!({"id":"cancel_a2mcp","recommend":false}));
}

#[test]
fn next_actions_are_alternatives_not_an_event_sequence() {
    let decision = ProbeDecision::payment_confirmation("candidate-1", true, false);
    assert_eq!(decision.next_action.len(), 2);
    assert_eq!(decision.next_action[0].id, "confirm_a2mcp_payment");
    assert_eq!(decision.next_action[1].id, "cancel_a2mcp");
    assert_eq!(decision.payload["selectedCandidateId"], "candidate-1");
}

#[test]
fn parameter_submission_is_an_automatic_reprobe_not_a_confirmation_gate() {
    let input = parse_probe_input(&routing_payload().to_string(), "{}").expect("valid payload");
    let required =
        discover_input_required(&json!({"missingParams":["brand"]})).expect("required field");
    let decision = input_required_decision(input, required);

    assert_eq!(decision.payload["autoProbeOnValid"], true);
    assert_eq!(decision.payload["fields"][0]["name"], "brand");
    assert_eq!(decision.next_action[0].id, "provide_a2mcp_params");
    assert!(decision
        .next_action
        .iter()
        .all(|action| action.id != "confirm_a2mcp_params"));
}

#[test]
fn required_request_fields_block_before_endpoint_probe() {
    let mut payload = routing_payload();
    payload["requestSpec"] = json!({
        "method":"POST",
        "fields":[{"name":"brand","type":"string","required":true,"carrier":"body"}]
    });
    let input = parse_probe_input(&payload.to_string(), "{}").expect("valid payload");
    let required = outstanding_request_input(&input).expect("brand is still required");

    assert_eq!(required.fields.len(), 1);
    assert_eq!(required.fields[0].name, "brand");

    let complete =
        parse_probe_input(&payload.to_string(), r#"{"brand":"OKX"}"#).expect("complete payload");
    assert!(outstanding_request_input(&complete).is_none());
}

#[test]
fn invalid_typed_value_is_correctable_and_never_becomes_payment_confirmation() {
    let mut payload = routing_payload();
    payload["requestSpec"] = json!({
        "method":"POST",
        "fields":[{"name":"count","type":"number","required":true,"carrier":"body"}]
    });
    let args = ProbeArgs {
        routing_json: payload.to_string(),
        params_json: r#"{"count":"not-a-number"}"#.to_string(),
    };
    let error = parse_probe_input(&args.routing_json, &args.params_json)
        .expect_err("wrong type must fail validation");
    let decision = invalid_params_decision(&args, error);

    assert_eq!(decision.phase, "parameter_collection");
    assert_eq!(decision.decision, "requires_user_input");
    assert_eq!(decision.reason, "invalid_a2mcp_params");
    assert_eq!(decision.payload["fields"][0]["name"], "count");
    assert_eq!(decision.payload["autoProbeOnValid"], true);
    assert_eq!(decision.next_action[0].id, "provide_a2mcp_params");
    assert!(decision
        .next_action
        .iter()
        .all(|action| action.id != "confirm_a2mcp_payment"));
}

#[test]
fn malformed_params_and_unsupported_schema_types_are_not_correction_loops() {
    let malformed = parse_probe_input(&routing_payload().to_string(), "not-json")
        .expect_err("malformed params must fail");
    assert_eq!(malformed.code, "invalid_a2mcp_params");

    let mut payload = routing_payload();
    payload["requestSpec"] = json!({
        "method":"POST",
        "fields":[{"name":"when","type":"date","required":true,"carrier":"body"}]
    });
    let unsupported = parse_probe_input(&payload.to_string(), "{}")
        .expect_err("unsupported schema type must fail");
    assert_eq!(unsupported.code, "invalid_a2mcp_routing");
}

#[tokio::test]
async fn run_probe_never_contacts_endpoint_until_known_params_are_valid() {
    let mut payload = routing_payload();
    payload["serviceSnapshot"]["endpoint"] = json!("https://127.0.0.1:9/a2mcp");
    payload["requestSpec"] = json!({
        "method":"POST",
        "fields":[{"name":"count","type":"number","required":true,"carrier":"body"}]
    });
    let routing_json = payload.to_string();

    let missing = run_probe(&ProbeArgs {
        routing_json: routing_json.clone(),
        params_json: "{}".to_string(),
    })
    .await
    .expect("missing input decision");
    assert_eq!(missing.phase, "parameter_collection");
    assert_eq!(missing.reason, "input_required");

    let invalid = run_probe(&ProbeArgs {
        routing_json: routing_json.clone(),
        params_json: r#"{"count":"wrong"}"#.to_string(),
    })
    .await
    .expect("invalid input decision");
    assert_eq!(invalid.phase, "parameter_collection");
    assert_eq!(invalid.reason, "invalid_a2mcp_params");

    let complete = run_probe(&ProbeArgs {
        routing_json,
        params_json: r#"{"count":2}"#.to_string(),
    })
    .await
    .expect("endpoint failure is represented as a decision");
    assert_eq!(complete.phase, "endpoint_probe");
    assert_eq!(complete.reason, "endpoint_failure");

    let mut unsafe_method = payload;
    unsafe_method["requestSpec"]["method"] = json!("DELETE");
    let blocked = run_probe(&ProbeArgs {
        routing_json: unsafe_method.to_string(),
        params_json: r#"{"count":2}"#.to_string(),
    })
    .await
    .expect("unsafe method is represented as a routing decision");
    assert_eq!(blocked.decision, "blocked");
    assert_eq!(blocked.reason, "invalid_a2mcp_routing");
}
