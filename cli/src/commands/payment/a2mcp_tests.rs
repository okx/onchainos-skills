use serde_json::{json, Map, Value};

use super::a2mcp::{
    create_a2mcp_payment_intent, inspect_payment_source, A2mcpExecutionState, A2mcpFrozenRequestV1,
    A2mcpIntentCreateInput, A2mcpPaymentSource, A2mcpPreparedCandidate, A2mcpSelectedAcceptV1,
};
use crate::home;

fn with_home<F: FnOnce()>(sub: &str, f: F) {
    let _lock = home::TEST_ENV_MUTEX.lock().unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(sub);
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("ONCHAINOS_HOME", &dir);
    f();
    std::env::remove_var("ONCHAINOS_HOME");
    let _ = std::fs::remove_dir_all(&dir);
}

fn frozen_request() -> A2mcpFrozenRequestV1 {
    A2mcpFrozenRequestV1::new(
        "https://merchant.example/pay".into(),
        "POST".into(),
        Map::from_iter([("count".into(), json!(2)), ("enabled".into(), json!(true))]),
        vec![],
        Some(json!({"url":"https://merchant.example/pay"})),
    )
    .unwrap()
}

fn selected_accept() -> A2mcpSelectedAcceptV1 {
    A2mcpSelectedAcceptV1::from_prepared_candidate(A2mcpPreparedCandidate::new_for_test(
        "candidate_0".into(),
        json!({
            "scheme":"exact",
            "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1000000",
            "payTo":"0x2222222222222222222222222222222222222222",
            "extra":{"name":"USDC","version":"2"}
        }),
        "USDC".into(),
        6,
        "eip3009".into(),
        "sufficient".into(),
    ))
}

#[test]
fn confirmed_creation_sets_unforgeable_source_and_preserves_typed_params() {
    with_home("a2mcp_intent_round_trip", || {
        let created = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
            probe_id: "probe_1".into(),
            owner_account_id: "account_1".into(),
            payer_address: "0x3333333333333333333333333333333333333333".into(),
            frozen_request: frozen_request(),
            selected_accept: selected_accept(),
            created_at: 1_000,
            expires_at: 1_200,
            user_confirmed: true,
        })
        .unwrap();

        assert_eq!(created.source(), A2mcpPaymentSource::OkxAiA2mcp);
        assert_eq!(created.execution_state(), A2mcpExecutionState::Prepared);
        assert_eq!(created.signature_attempts(), 0);
        assert_eq!(created.frozen_request().typed_params()["count"], json!(2));
        assert_eq!(
            created.frozen_request().typed_params()["enabled"],
            json!(true)
        );
        assert_eq!(
            inspect_payment_source(created.payment_id()).unwrap(),
            A2mcpPaymentSource::OkxAiA2mcp
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(
                home::onchainos_home()
                    .unwrap()
                    .join("payments")
                    .join(format!("{}.json", created.payment_id())),
            )
            .unwrap()
            .permissions()
            .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    });
}

#[test]
fn intent_creation_requires_explicit_confirmation_and_sufficient_balance() {
    with_home("a2mcp_intent_confirmation", || {
        let mut input = A2mcpIntentCreateInput {
            probe_id: "probe_1".into(),
            owner_account_id: "account_1".into(),
            payer_address: "0x3333333333333333333333333333333333333333".into(),
            frozen_request: frozen_request(),
            selected_accept: selected_accept(),
            created_at: 1_000,
            expires_at: 1_200,
            user_confirmed: false,
        };
        assert!(create_a2mcp_payment_intent(input.clone())
            .unwrap_err()
            .to_string()
            .starts_with("a2mcp_payment_confirmation_required"));

        input.user_confirmed = true;
        input.selected_accept =
            A2mcpSelectedAcceptV1::from_prepared_candidate(A2mcpPreparedCandidate::new_for_test(
                "candidate_0".into(),
                json!({
                    "scheme":"exact", "network":"eip155:196",
                    "asset":"0x1111111111111111111111111111111111111111",
                    "amount":"1000000", "payTo":"0x2222222222222222222222222222222222222222",
                    "extra":{"name":"USDC","version":"2"}
                }),
                "USDC".into(),
                6,
                "eip3009".into(),
                "insufficient".into(),
            ));
        assert!(create_a2mcp_payment_intent(input)
            .unwrap_err()
            .to_string()
            .starts_with("a2mcp_insufficient_balance"));
    });
}

#[test]
fn missing_challenge_expiry_uses_bounded_local_ttl() {
    with_home("a2mcp_intent_default_expiry", || {
        let created = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
            probe_id: "probe_no_expiry".into(),
            owner_account_id: "account_1".into(),
            payer_address: "0x3333333333333333333333333333333333333333".into(),
            frozen_request: frozen_request(),
            selected_accept: selected_accept(),
            created_at: 1_000,
            expires_at: 0,
            user_confirmed: true,
        })
        .unwrap();
        assert!(
            super::a2mcp::read_a2mcp_payment_intent(created.payment_id(), "account_1", 1_299)
                .is_ok()
        );
        assert!(
            super::a2mcp::read_a2mcp_payment_intent(created.payment_id(), "account_1", 1_300)
                .unwrap_err()
                .to_string()
                .starts_with("a2mcp_payment_intent_expired")
        );
    });
}

#[test]
fn selected_accept_rejects_unsupported_token_and_scheme() {
    let bad_token = A2mcpPreparedCandidate::new_for_test(
        "candidate_0".into(),
        json!({
            "scheme":"exact", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1", "payTo":"0x2222222222222222222222222222222222222222"
        }),
        "DAI".into(),
        18,
        "eip3009".into(),
        "sufficient".into(),
    );
    assert!(A2mcpSelectedAcceptV1::try_from_prepared_candidate(bad_token).is_err());

    let bad_scheme = A2mcpPreparedCandidate::new_for_test(
        "candidate_1".into(),
        json!({
            "scheme":"period", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1", "payTo":"0x2222222222222222222222222222222222222222"
        }),
        "USDC".into(),
        6,
        "permit2".into(),
        "sufficient".into(),
    );
    assert!(A2mcpSelectedAcceptV1::try_from_prepared_candidate(bad_scheme).is_err());
}

#[test]
fn aggr_deferred_is_session_authorized_not_eip3009() {
    let deferred = A2mcpPreparedCandidate::new_for_test(
        "candidate_0".into(),
        json!({
            "scheme":"aggr_deferred", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1", "payTo":"0x2222222222222222222222222222222222222222"
        }),
        "USDC".into(),
        6,
        "session".into(),
        "sufficient".into(),
    );
    assert_eq!(
        A2mcpSelectedAcceptV1::try_from_prepared_candidate(deferred)
            .unwrap()
            .scheme(),
        "aggr_deferred"
    );

    let mislabeled = A2mcpPreparedCandidate::new_for_test(
        "candidate_1".into(),
        json!({
            "scheme":"aggr_deferred", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1", "payTo":"0x2222222222222222222222222222222222222222"
        }),
        "USDC".into(),
        6,
        "eip3009".into(),
        "sufficient".into(),
    );
    assert!(A2mcpSelectedAcceptV1::try_from_prepared_candidate(mislabeled).is_err());
}

#[test]
fn frozen_request_rejects_non_scalar_non_body_carrier_values() {
    let err = A2mcpFrozenRequestV1::new(
        "https://merchant.example/pay".into(),
        "GET".into(),
        Map::from_iter([("filter".into(), json!({"nested": true}))]),
        vec![super::state::ParamSpec {
            name: "filter".into(),
            carrier: super::state::ParamCarrier::Query,
            required: true,
            type_: "object".into(),
        }],
        None,
    )
    .unwrap_err();
    assert!(err.to_string().starts_with("a2mcp_invalid_typed_params"));

    let reserved_header = A2mcpFrozenRequestV1::new(
        "https://merchant.example/pay".into(),
        "POST".into(),
        Map::from_iter([("PAYMENT-SIGNATURE".into(), json!("attacker-value"))]),
        vec![super::state::ParamSpec {
            name: "PAYMENT-SIGNATURE".into(),
            carrier: super::state::ParamCarrier::Header,
            required: true,
            type_: "string".into(),
        }],
        None,
    )
    .unwrap_err();
    assert!(reserved_header
        .to_string()
        .contains("reserved header parameter"));
}

#[test]
fn frozen_request_rejects_plain_http_endpoint() {
    let error = A2mcpFrozenRequestV1::new(
        "http://merchant.example/pay".into(),
        "POST".into(),
        Map::new(),
        Vec::new(),
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Endpoint must use HTTPS"));
}

#[test]
fn frozen_request_allows_only_get_and_post() {
    for method in ["GET", "post"] {
        let request = A2mcpFrozenRequestV1::new(
            "https://merchant.example/pay".into(),
            method.into(),
            Map::new(),
            Vec::new(),
            None,
        )
        .expect("GET and POST are valid A2MCP methods");
        assert!(matches!(request.method(), "GET" | "POST"));
    }

    for method in ["PUT", "PATCH", "DELETE"] {
        let error = A2mcpFrozenRequestV1::new(
            "https://merchant.example/pay".into(),
            method.into(),
            Map::new(),
            Vec::new(),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("method must be GET or POST"));
    }
}

#[test]
fn generic_state_is_not_misclassified_as_a2mcp() {
    with_home("a2mcp_source_isolation", || {
        let generic = json!({"payment_id":"pay_generic","owner_wallet":"account_1"});
        let dir = home::onchainos_home().unwrap().join("payments");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pay_generic.json"),
            serde_json::to_vec(&generic).unwrap(),
        )
        .unwrap();
        assert_eq!(
            inspect_payment_source("pay_generic").unwrap(),
            A2mcpPaymentSource::GenericQuote
        );
    });
}

#[test]
fn immutable_fields_are_stable_across_execution_state_updates() {
    with_home("a2mcp_execution_state", || {
        let mut intent = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
            probe_id: "probe_1".into(),
            owner_account_id: "account_1".into(),
            payer_address: "0x3333333333333333333333333333333333333333".into(),
            frozen_request: frozen_request(),
            selected_accept: selected_accept(),
            created_at: 1_000,
            expires_at: 1_200,
            user_confirmed: true,
        })
        .unwrap();
        let before: Value = serde_json::to_value(&intent).unwrap();
        intent.begin_signing(1_050).unwrap();
        intent.record_signature_attempt().unwrap();
        intent.mark_proof_generated().unwrap();
        intent.mark_replaying().unwrap();
        intent.mark_failed_terminal().unwrap();
        let after: Value = serde_json::to_value(&intent).unwrap();

        for key in [
            "source",
            "paymentId",
            "probeId",
            "ownerAccountId",
            "payerAddress",
            "frozenRequest",
            "selectedAccept",
            "createdAt",
            "expiresAt",
        ] {
            assert_eq!(before[key], after[key], "immutable field changed: {key}");
        }
        assert_eq!(after["execution"]["state"], "failed_terminal");
        assert_eq!(after["execution"]["signatureAttempts"], 1);
        assert!(intent
            .begin_signing(1_060)
            .unwrap_err()
            .to_string()
            .starts_with("a2mcp_payment_already_executed"));
    });
}

#[test]
fn proof_assembly_failure_can_be_recorded_terminal_before_replay() {
    with_home("a2mcp_proof_terminal", || {
        let mut intent = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
            probe_id: "probe_proof_failure".into(),
            owner_account_id: "account_1".into(),
            payer_address: "0x3333333333333333333333333333333333333333".into(),
            frozen_request: frozen_request(),
            selected_accept: selected_accept(),
            created_at: 1_000,
            expires_at: 1_200,
            user_confirmed: true,
        })
        .unwrap();
        intent.begin_signing(1_050).unwrap();
        intent.record_signature_attempt().unwrap();
        intent.mark_proof_generated().unwrap();
        intent.mark_failed_terminal().unwrap();
        assert_eq!(
            intent.execution_state(),
            A2mcpExecutionState::FailedTerminal
        );
        assert!(intent.begin_signing(1_060).is_err());
    });
}

#[test]
fn payment_dispatch_rejects_a2mcp_pay_time_overrides_before_signing() {
    with_home("a2mcp_override_rejection", || {
        let intent = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
            probe_id: "probe_override".into(),
            owner_account_id: "account_1".into(),
            payer_address: "0x3333333333333333333333333333333333333333".into(),
            frozen_request: frozen_request(),
            selected_accept: selected_accept(),
            created_at: 1_000,
            expires_at: 1_200,
            user_confirmed: true,
        })
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let selected = runtime
            .block_on(super::payment_flow::fetch_pay(
                intent.payment_id(),
                Some(0),
                &[],
                true,
            ))
            .unwrap_err();
        assert!(selected
            .to_string()
            .starts_with("a2mcp_payment_overrides_forbidden"));

        let param = runtime
            .block_on(super::payment_flow::fetch_pay(
                intent.payment_id(),
                None,
                &["count=3".into()],
                true,
            ))
            .unwrap_err();
        assert!(param
            .to_string()
            .starts_with("a2mcp_payment_overrides_forbidden"));

        let missing_yes = runtime
            .block_on(super::payment_flow::fetch_pay(
                intent.payment_id(),
                None,
                &[],
                false,
            ))
            .unwrap_err();
        assert!(missing_yes
            .to_string()
            .starts_with("a2mcp_payment_confirmation_required"));
    });
}
