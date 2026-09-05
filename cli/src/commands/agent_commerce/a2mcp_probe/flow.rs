use super::*;

pub(super) async fn run_probe(args: &ProbeArgs) -> Result<ProbeDecision> {
    let mut input = match parse_probe_input(&args.routing_json, &args.params_json) {
        Ok(input) => input,
        Err(error) if error.code == "invalid_a2mcp_param_value" => {
            return Ok(invalid_params_decision(args, error))
        }
        Err(error) => {
            return Ok(ProbeDecision::blocked(
                error.code,
                json!({"schemaVersion":1,"message":error.message}),
            ))
        }
    };
    if let Some(required) = outstanding_request_input(&input) {
        return Ok(input_required_decision(input, required));
    }
    let mut outcome = match send_probe(&input).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let message = error.to_string();
            let reason = if message.starts_with("a2mcp_invalid_typed_params")
                || message.starts_with("invalid_a2mcp_params")
            {
                "invalid_a2mcp_params"
            } else {
                "endpoint_failure"
            };
            return Ok(ProbeDecision::blocked(
                reason,
                json!({"schemaVersion":1,"message":message}),
            ));
        }
    };
    let fallback_method = match &outcome {
        HttpOutcome::MethodRequired { allow } => {
            fallback_method_for_405(&input.snapshot.method, allow.as_deref())
        }
        HttpOutcome::Failed { status, body } => fallback_method_for_400(
            &input.snapshot.method,
            *status,
            body,
            &input.typed_params,
            &input.snapshot.param_plan,
        ),
        HttpOutcome::Free { .. }
        | HttpOutcome::InputRequired(_)
        | HttpOutcome::Challenge { .. } => None,
    };
    if let Some(fallback_method) = fallback_method {
        input.snapshot.method = fallback_method;
        input.snapshot.method_was_defaulted = false;
        outcome = match send_probe(&input).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let message = error.to_string();
                let reason = if message.starts_with("a2mcp_invalid_typed_params")
                    || message.starts_with("invalid_a2mcp_params")
                {
                    "invalid_a2mcp_params"
                } else {
                    "endpoint_failure"
                };
                return Ok(ProbeDecision::blocked(
                    reason,
                    json!({"schemaVersion":1,"message":message}),
                ));
            }
        };
    }
    if should_verify_default_get_challenge_with_post(&input, &outcome) {
        let mut post_input = input.clone();
        post_input.snapshot.method = "POST".to_string();
        post_input.snapshot.method_was_defaulted = false;
        let post_outcome = match send_probe(&post_input).await {
            Ok(outcome) => outcome,
            Err(_) => return Ok(method_verification_blocked()),
        };
        match post_verification_action(&post_input, &post_outcome) {
            PostVerificationAction::AdoptPost => {
                input = post_input;
                outcome = post_outcome;
            }
            PostVerificationAction::KeepGet => {
                input.snapshot.method_was_defaulted = false;
            }
            PostVerificationAction::Block => return Ok(method_verification_blocked()),
        }
    }
    match outcome {
        HttpOutcome::InputRequired(mut required) => {
            if let Err(error) = apply_input_required_method(&mut input, &mut required) {
                return Ok(ProbeDecision::blocked(
                    error.code,
                    json!({"schemaVersion":1,"message":error.message}),
                ));
            }
            Ok(input_required_decision(input, required))
        }
        HttpOutcome::Free { status, body } => {
            let stored = store_free_result(
                FreeResultInput {
                    service_id: input.snapshot.service_id.clone(),
                    service_name: input.snapshot.service_name.clone(),
                    endpoint: input.snapshot.endpoint.to_string(),
                    method: input.snapshot.method.clone(),
                    typed_params: input.typed_params.clone(),
                    status_code: status,
                    result: body,
                },
                crate::commands::payment::session_state::now_unix(),
            )?;
            Ok(free_confirmation_decision(&input, stored.confirmation_id()))
        }
        HttpOutcome::MethodRequired { allow } => Ok(ProbeDecision::blocked(
            "request_method_required",
            json!({"schemaVersion":1,"allow":allow}),
        )),
        HttpOutcome::Failed { status, body } => {
            if status == 400 {
                if let Some(required) = discover_endpoint_param_issues(
                    &body,
                    &input.typed_params,
                    &input.snapshot.param_plan,
                ) {
                    return Ok(input_required_decision(input, required));
                }
            }
            Ok(ProbeDecision::blocked(
                "endpoint_failure",
                json!({"schemaVersion":1,"statusCode":status,"result":body}),
            ))
        }
        HttpOutcome::Challenge { challenge, body } => {
            build_payment_decision(&input, challenge, body).await
        }
    }
}

pub(super) fn free_confirmation_decision(
    input: &ProbeInput,
    confirmation_id: &str,
) -> ProbeDecision {
    build_free_confirmation_decision(
        &input.snapshot.service_id,
        input.snapshot.service_name.as_deref(),
        input.snapshot.endpoint.as_str(),
        &input.snapshot.method,
        &input.typed_params,
        confirmation_id,
    )
}

fn free_confirmation_decision_from_state(state: &FreeResultState) -> ProbeDecision {
    build_free_confirmation_decision(
        state.service_id(),
        state.service_name(),
        state.endpoint(),
        state.method(),
        state.typed_params(),
        state.confirmation_id(),
    )
}

fn build_free_confirmation_decision(
    service_id: &str,
    service_name: Option<&str>,
    endpoint: &str,
    method: &str,
    typed_params: &Map<String, Value>,
    confirmation_id: &str,
) -> ProbeDecision {
    ProbeDecision {
        phase: "payment_confirmation".to_string(),
        decision: "requires_user_input".to_string(),
        reason: "free_confirmation_required".to_string(),
        next_action: vec![
            Action::new("confirm_a2mcp_free", true)
                .with_params(json!({"confirmationId": confirmation_id})),
            Action::new("cancel_a2mcp", false),
        ],
        payload: json!({
            "schemaVersion":1,
            "serviceId":service_id,
            "serviceName":service_name,
            "endpoint":endpoint,
            "method":method,
            "typedParams":typed_params,
            "amountDisplay":"Free",
            "confirmationEnabled":true,
            "confirmationId":confirmation_id,
        }),
    }
}

pub(super) fn run_confirm_free(args: &ConfirmFreeArgs) -> Result<ProbeDecision> {
    let now = crate::commands::payment::session_state::now_unix();
    if !args.yes {
        let state = load_free_result(&args.confirmation_id, now)?;
        return Ok(free_confirmation_decision_from_state(&state));
    }

    let state = consume_free_result(&args.confirmation_id, now)?;
    Ok(ProbeDecision {
        phase: "endpoint_result".to_string(),
        decision: "ready".to_string(),
        reason: "free_result".to_string(),
        next_action: Vec::new(),
        payload: json!({
            "schemaVersion":1,
            "serviceId":state.service_id(),
            "serviceName":state.service_name(),
            "endpoint":state.endpoint(),
            "method":state.method(),
            "typedParams":state.typed_params(),
            "amountDisplay":"Free",
            "statusCode":state.status_code(),
            "result":state.result(),
        }),
    })
}

pub(super) fn apply_input_required_method(
    input: &mut ProbeInput,
    required: &mut InputRequired,
) -> Result<(), ContractError> {
    if let Some(method) = required.method.take() {
        input.snapshot.method = normalize_a2mcp_method(&method)?;
        input.snapshot.method_was_defaulted = false;
    }
    Ok(())
}

pub(super) fn input_required_decision(input: ProbeInput, required: InputRequired) -> ProbeDecision {
    let mut required_fields = required.fields.clone();
    for name in &required.required_any_of {
        if !required_fields.iter().any(|field| field.name == *name) {
            required_fields.push(FieldConstraint {
                name: name.clone(),
                type_: default_string_type(),
                required: false,
                carrier: None,
                description: None,
            });
        }
    }
    let request_fields = merge_field_constraints(&input.snapshot.param_plan, &required_fields);
    let request_spec = RequestSpec {
        method: (!input.snapshot.method_was_defaulted).then(|| input.snapshot.method.clone()),
        fields: request_fields,
        required_any_of: required.required_any_of.clone(),
    };
    ProbeDecision {
        phase: "parameter_collection".to_string(),
        decision: "requires_user_input".to_string(),
        reason: "input_required".to_string(),
        next_action: vec![
            Action::new("provide_a2mcp_params", true),
            Action::new("cancel_a2mcp", false),
        ],
        payload: json!({
            "schemaVersion":1,"serviceId":input.snapshot.service_id,"fields":required.fields,
            "requiredAnyOf":required.required_any_of,"message":required.message,"typedParams":input.typed_params,
            "autoProbeOnValid":true,
            "nextProbePayload":{"schemaVersion":1,"serviceSnapshot":input.snapshot.raw,"requestSpec":request_spec},
        }),
    }
}

pub(super) fn invalid_params_decision(args: &ProbeArgs, error: ContractError) -> ProbeDecision {
    let routing = serde_json::from_str::<RoutingPayload>(&args.routing_json).ok();
    let next_probe_payload =
        serde_json::from_str::<Value>(&args.routing_json).unwrap_or(Value::Null);
    let typed_params = serde_json::from_str::<Value>(&args.params_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let fields = routing
        .as_ref()
        .map(|routing| {
            routing
                .request_spec
                .as_ref()
                .map(|spec| spec.fields.clone())
                .unwrap_or_else(|| {
                    routing
                        .service_snapshot
                        .get("outputSchema")
                        .and_then(|schema| schema.get("input"))
                        .map(parse_fields)
                        .unwrap_or_default()
                })
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|field| {
            typed_params
                .get(&field.name)
                .is_some_and(|value| !typed_value_matches(value, &field.type_))
        })
        .collect::<Vec<_>>();
    ProbeDecision {
        phase: "parameter_collection".to_string(),
        decision: "requires_user_input".to_string(),
        reason: "invalid_a2mcp_params".to_string(),
        next_action: vec![
            Action::new("provide_a2mcp_params", true),
            Action::new("cancel_a2mcp", false),
        ],
        payload: json!({
            "schemaVersion":1,"fields":fields,"message":error.message,"typedParams":typed_params,
            "autoProbeOnValid":true,"nextProbePayload":next_probe_payload,
        }),
    }
}

pub(super) fn outstanding_request_input(input: &ProbeInput) -> Option<InputRequired> {
    outstanding_input(
        InputRequired {
            fields: input.snapshot.param_plan.clone(),
            required_any_of: input.snapshot.required_any_of.clone(),
            message: None,
            method: Some(input.snapshot.method.clone()),
        },
        &input.typed_params,
    )
}

pub(super) async fn run_refresh_balance(args: &RefreshBalanceArgs) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::{
        load_a2mcp_prepared_payment, refresh_a2mcp_prepared_payment, replace_a2mcp_prepared_payment,
    };

    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let loaded_at = crate::commands::payment::session_state::now_unix();
    let prepared = load_a2mcp_prepared_payment(&args.prepared_id, &owner_account_id, loaded_at)?;
    let prepared = refresh_a2mcp_prepared_payment(prepared).await?;
    let confirmation_context = prepared.confirmation_context();
    let refreshed_at = crate::commands::payment::session_state::now_unix();
    let replacement_id = replace_a2mcp_prepared_payment(
        &args.prepared_id,
        prepared.clone(),
        &owner_account_id,
        refreshed_at,
    )?;
    let candidates = prepared
        .candidates()
        .iter()
        .map(|candidate| {
            let mismatch = confirmation_context
                .asp_amount()
                .is_some_and(|amount| !decimal_strings_equal(amount, candidate.amount_display()));
            json!({
                "candidateId":candidate.candidate_id(),"tokenSymbol":candidate.symbol(),
                "network":candidate.network(),"chainName":candidate.chain_name(),
                "amountAtomic":candidate.amount_atomic(),"amountDisplay":candidate.amount_display(),
                "amountSemantics":amount_semantics(candidate.scheme()),
                "requiredDisplay":candidate.required_amount(),
                "balanceStatus":candidate.balance_status(),"availableDisplay":candidate.available_amount(),
                "shortfallDisplay":candidate.shortfall(),"depositAddress":candidate.deposit_address(),
                "confirmationEnabled":candidate.balance_status()=="sufficient",
                "amountMismatch":mismatch,
            })
        })
        .collect::<Vec<_>>();
    let single = prepared
        .candidates()
        .first()
        .filter(|_| prepared.candidates().len() == 1);
    let selected_id = single.map(|candidate| candidate.candidate_id());
    let selected_enabled =
        single.is_some_and(|candidate| candidate.balance_status() == "sufficient");
    let selected_mismatch = single.map(|candidate| {
        confirmation_context
            .asp_amount()
            .is_some_and(|amount| !decimal_strings_equal(amount, candidate.amount_display()))
    });
    let (reason, next_action) = if let Some(candidate_id) = selected_id {
        let confirmation = ProbeDecision::payment_confirmation(
            &replacement_id,
            candidate_id,
            selected_enabled,
            false,
        );
        (confirmation.reason, confirmation.next_action)
    } else {
        (
            "token_selection_required".to_string(),
            vec![
                Action::new("select_a2mcp_token", true)
                    .with_params(json!({"preparedId": replacement_id})),
                Action::new("cancel_a2mcp", false),
            ],
        )
    };
    Ok(ProbeDecision {
        phase: "payment_confirmation".to_string(),
        decision: "requires_user_input".to_string(),
        reason,
        next_action,
        payload: json!({
            "schemaVersion":1,"serviceId":confirmation_context.service_id(),
            "serviceName":confirmation_context.service_name(),
            "endpoint":prepared.frozen_request().endpoint(),
            "method":prepared.frozen_request().method(),"typedParams":prepared.frozen_request().typed_params(),
            "aspPrice":{"amount":confirmation_context.asp_amount(),"symbol":confirmation_context.asp_symbol()},
            "amountMismatch":selected_mismatch,
            "selectedCandidateId":selected_id,"confirmationEnabled":selected_enabled,
            "walletError":prepared.wallet_error(),"candidates":candidates,
            "preparedId":replacement_id,
        }),
    })
}

pub(super) async fn run_resume_after_funding(
    args: &ResumeAfterFundingArgs,
) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::{
        claim_a2mcp_prepared_payment, create_a2mcp_payment_intent, refresh_a2mcp_prepared_payment,
        A2mcpIntentCreateInput, ERR_CONFIRMATION_REQUIRED,
    };

    if !args.yes {
        return Err(anyhow!(
            "{ERR_CONFIRMATION_REQUIRED}: funding completion must be explicit"
        ));
    }
    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let loaded_at = crate::commands::payment::session_state::now_unix();
    let claim = claim_a2mcp_prepared_payment(&args.prepared_id, &owner_account_id, loaded_at)?;
    if claim.prepared().funding_candidate_id() != Some(args.candidate_id.as_str()) {
        return Err(anyhow!(
            "a2mcp_funding_continuation_required: enter Funding before resuming"
        ));
    }

    let mut refreshed = refresh_a2mcp_prepared_payment(claim.prepared().clone()).await?;
    let refreshed_candidate = refreshed
        .candidates()
        .iter()
        .find(|candidate| candidate.candidate_id() == args.candidate_id)
        .ok_or_else(|| anyhow!("a2mcp_invalid_payment_candidate: unknown candidate"))?;
    if refreshed_candidate.balance_status() == "sufficient" {
        let selected = refreshed.select(&args.candidate_id)?;
        let (_, _, payer_address) =
            crate::commands::payment::payment_flow::resolve_chain_and_payer(selected.raw(), None)
                .await?;
        let created_at = crate::commands::payment::session_state::now_unix();
        let intent = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
            probe_id: args.prepared_id.clone(),
            owner_account_id,
            payer_address,
            frozen_request: refreshed.frozen_request().clone(),
            selected_accept: selected,
            created_at,
            expires_at: refreshed.challenge_expires_at(),
            user_confirmed: true,
        })?;
        claim.commit();
        return Ok(payment_ready_decision(intent.payment_id()));
    }

    refreshed.clear_funding_continuation();
    let replacement_id = claim.replace(refreshed)?;
    run_prepare_payment(&PreparePaymentArgs {
        prepared_id: replacement_id,
        candidate_id: args.candidate_id.clone(),
        yes: false,
    })
    .await
}

pub(super) async fn run_funding(args: &FundingArgs) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::replace_a2mcp_prepared_payment;

    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let loaded_at = crate::commands::payment::session_state::now_unix();
    let mut prepared = crate::commands::payment::a2mcp::load_a2mcp_prepared_payment(
        &args.prepared_id,
        &owner_account_id,
        loaded_at,
    )?;
    let candidate = prepared
        .candidates()
        .iter()
        .find(|candidate| candidate.candidate_id() == args.candidate_id)
        .ok_or_else(|| anyhow!("a2mcp_invalid_payment_candidate: unknown candidate"))?;
    if candidate.balance_status() == "sufficient" {
        return Err(anyhow!(
            "a2mcp_funding_not_required: selected candidate is sufficient"
        ));
    }
    let funding = crate::funding::build_funding_bundle_for_address(
        "",
        candidate.chain_id(),
        candidate.deposit_address(),
        crate::funding::FundingBlockedInput {
            asset: candidate.symbol(),
            token_address: candidate
                .raw_accept()
                .get("asset")
                .and_then(Value::as_str)
                .unwrap_or(""),
            required: candidate.required_amount(),
            balance: Some(candidate.available_amount()),
            operation: Some("a2mcp"),
            error_code: None,
            error_message: None,
        },
    )?;
    prepared.mark_funding_continuation(&args.candidate_id)?;
    let continuation_id =
        replace_a2mcp_prepared_payment(&args.prepared_id, prepared, &owner_account_id, loaded_at)?;
    Ok(ProbeDecision {
        phase: "funding_required".to_string(),
        decision: "blocked".to_string(),
        reason: "insufficient_balance".to_string(),
        next_action: vec![
            Action::new("resume_a2mcp_after_funding", true).with_params(json!({
                "preparedId": continuation_id,
                "candidateId": args.candidate_id,
            })),
        ],
        payload: funding["payload"].clone(),
    })
}

pub(super) async fn run_prepare_payment(args: &PreparePaymentArgs) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::{
        claim_a2mcp_prepared_payment, create_a2mcp_payment_intent, load_a2mcp_prepared_payment,
        A2mcpIntentCreateInput,
    };

    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let loaded_at = crate::commands::payment::session_state::now_unix();
    let prepared = load_a2mcp_prepared_payment(&args.prepared_id, &owner_account_id, loaded_at)?;
    let candidate = prepared
        .candidates()
        .iter()
        .find(|candidate| candidate.candidate_id() == args.candidate_id)
        .ok_or_else(|| anyhow!("a2mcp_invalid_payment_intent: unknown candidate"))?;
    if !args.yes {
        let enabled = candidate.balance_status() == "sufficient";
        let confirmation_context = prepared.confirmation_context();
        let mismatch = confirmation_context
            .asp_amount()
            .is_some_and(|amount| !decimal_strings_equal(amount, candidate.amount_display()));
        let mut decision = ProbeDecision::payment_confirmation(
            &args.prepared_id,
            candidate.candidate_id(),
            enabled,
            prepared.candidates().len() > 1,
        );
        decision.payload = json!({
            "schemaVersion":1,
            "serviceId":confirmation_context.service_id(),
            "serviceName":confirmation_context.service_name(),
            "endpoint":prepared.frozen_request().endpoint(),
            "method":prepared.frozen_request().method(),
            "typedParams":prepared.frozen_request().typed_params(),
            "aspPrice":{"amount":confirmation_context.asp_amount(),"symbol":confirmation_context.asp_symbol()},
            "amountMismatch":mismatch,
            "selectedCandidateId":candidate.candidate_id(),
            "confirmationEnabled":enabled,
            "candidate":{
                "candidateId":candidate.candidate_id(),"tokenSymbol":candidate.symbol(),
                "network":candidate.network(),"chainName":candidate.chain_name(),
                "amountAtomic":candidate.amount_atomic(),"amountDisplay":candidate.amount_display(),
                "amountSemantics":amount_semantics(candidate.scheme()),
                "requiredDisplay":candidate.required_amount(),
                "balanceStatus":candidate.balance_status(),"availableDisplay":candidate.available_amount(),
                "shortfallDisplay":candidate.shortfall(),"depositAddress":candidate.deposit_address(),
                "amountMismatch":mismatch,
            },
            "walletError":prepared.wallet_error(),
            "preparedId":args.prepared_id,
        });
        return Ok(decision);
    }
    let selected = prepared.select(&args.candidate_id)?;
    let (_, _, payer_address) =
        crate::commands::payment::payment_flow::resolve_chain_and_payer(selected.raw(), None)
            .await?;
    let created_at = crate::commands::payment::session_state::now_unix();
    let claim = claim_a2mcp_prepared_payment(&args.prepared_id, &owner_account_id, created_at)?;
    let selected = claim.prepared().select(&args.candidate_id)?;
    let intent = create_a2mcp_payment_intent(A2mcpIntentCreateInput {
        probe_id: args.prepared_id.clone(),
        owner_account_id,
        payer_address,
        frozen_request: claim.prepared().frozen_request().clone(),
        selected_accept: selected,
        created_at,
        expires_at: claim.prepared().challenge_expires_at(),
        user_confirmed: args.yes,
    })?;
    claim.commit();
    Ok(payment_ready_decision(intent.payment_id()))
}

pub(super) fn payment_ready_decision(payment_id: &str) -> ProbeDecision {
    ProbeDecision {
        phase: "payment_ready".to_string(),
        decision: "ready".to_string(),
        reason: "payment_ready".to_string(),
        next_action: vec![Action::new("execute_a2mcp_payment", true)
            .with_params(json!({"paymentId": payment_id}))],
        payload: json!({"schemaVersion":1,"paymentId":payment_id}),
    }
}

pub(super) async fn build_payment_decision(
    input: &ProbeInput,
    challenge: Value,
    merchant_body: Value,
) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::{
        prepare_a2mcp_payment_from_challenge, store_a2mcp_prepared_payment,
        A2mcpConfirmationContextV1, A2mcpFrozenRequestV1, A2mcpPreparedChallengeInput,
    };

    let body_plan = merchant_body
        .get("outputSchema")
        .and_then(|schema| schema.get("input"))
        .map(parse_fields)
        .unwrap_or_default();
    let challenge_plan = challenge
        .get("outputSchema")
        .and_then(|schema| schema.get("input"))
        .map(parse_fields)
        .unwrap_or_default();
    let response_plan = merge_field_constraints(&input.snapshot.param_plan, &body_plan);
    let response_plan = merge_field_constraints(&response_plan, &challenge_plan);
    let paid_method = input.snapshot.method.clone();
    let param_plan = to_payment_param_plan(&response_plan, &paid_method)?;
    let frozen_request = A2mcpFrozenRequestV1::new(
        input.snapshot.endpoint.to_string(),
        paid_method.clone(),
        input.typed_params.clone(),
        param_plan,
        challenge.get("resource").cloned(),
    )?;
    let prepared = match prepare_a2mcp_payment_from_challenge(A2mcpPreparedChallengeInput {
        challenge: challenge.to_string(),
        frozen_request,
        confirmation_context: A2mcpConfirmationContextV1::new(
            input.snapshot.service_id.clone(),
            input.snapshot.service_name.clone(),
            input.snapshot.asp_amount.clone(),
            input.snapshot.asp_symbol.clone(),
        ),
    })
    .await
    {
        Ok(prepared) => prepared,
        Err(error) if error.to_string().contains("unsupported_payment_asset") => {
            return Ok(ProbeDecision::blocked(
                "unsupported_payment_asset",
                json!({"schemaVersion":1,"serviceId":input.snapshot.service_id}),
            ));
        }
        Err(error) => return Err(error),
    };
    let candidate_views = prepared
        .candidates()
        .iter()
        .map(|candidate| {
            let mismatch = input
                .snapshot
                .asp_amount
                .as_deref()
                .is_some_and(|amount| !decimal_strings_equal(amount, candidate.amount_display()));
            json!({
                "candidateId":candidate.candidate_id(), "tokenSymbol":candidate.symbol(),
                "network":candidate.network(), "chainName":candidate.chain_name(),
                "amountAtomic":candidate.amount_atomic(), "amountDisplay":candidate.amount_display(),
                "amountSemantics":amount_semantics(candidate.scheme()),
                "requiredDisplay":candidate.required_amount(),
                "balanceStatus":candidate.balance_status(), "confirmationEnabled":candidate.balance_status()=="sufficient",
                "availableDisplay":candidate.available_amount(), "shortfallDisplay":candidate.shortfall(),
                "depositAddress":candidate.deposit_address(), "amountMismatch":mismatch,
            })
        })
        .collect::<Vec<_>>();
    let selected_mismatch = (candidate_views.len() == 1)
        .then(|| {
            candidate_views[0]
                .get("amountMismatch")
                .and_then(Value::as_bool)
        })
        .flatten();
    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let created_at = crate::commands::payment::session_state::now_unix();
    let prepared_id =
        store_a2mcp_prepared_payment(prepared.clone(), &owner_account_id, created_at)?;
    let single = prepared
        .candidates()
        .first()
        .filter(|_| prepared.candidates().len() == 1);
    let selected_id = single.map(|candidate| candidate.candidate_id());
    let selected_enabled =
        single.is_some_and(|candidate| candidate.balance_status() == "sufficient");
    let (reason, next_action) = if let Some(candidate_id) = selected_id {
        let confirmation = ProbeDecision::payment_confirmation(
            &prepared_id,
            candidate_id,
            selected_enabled,
            false,
        );
        (confirmation.reason, confirmation.next_action)
    } else {
        (
            "token_selection_required".to_string(),
            vec![
                Action::new("select_a2mcp_token", true)
                    .with_params(json!({"preparedId": prepared_id})),
                Action::new("cancel_a2mcp", false),
            ],
        )
    };
    Ok(ProbeDecision {
        phase: "payment_confirmation".to_string(),
        decision: "requires_user_input".to_string(),
        reason,
        next_action,
        payload: json!({
            "schemaVersion":1,"serviceId":input.snapshot.service_id,"serviceName":input.snapshot.service_name,
            "endpoint":input.snapshot.endpoint.as_str(),"method":paid_method,"typedParams":input.typed_params,
            "aspPrice":{"amount":input.snapshot.asp_amount,"symbol":input.snapshot.asp_symbol},
            "selectedCandidateId":selected_id,"confirmationEnabled":selected_enabled,
            "amountMismatch":selected_mismatch,
            "walletError":prepared.wallet_error(),"candidates":candidate_views,
            "preparedId":prepared_id,
        }),
    })
}

pub(super) fn merge_field_constraints(
    base: &[FieldConstraint],
    updates: &[FieldConstraint],
) -> Vec<FieldConstraint> {
    let mut merged = base.to_vec();
    for field in updates {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.name == field.name)
        {
            *existing = field.clone();
        } else {
            merged.push(field.clone());
        }
    }
    merged
}

pub(super) fn decimal_strings_equal(left: &str, right: &str) -> bool {
    fn normalize(value: &str) -> Option<(String, String)> {
        let value = value.trim();
        if value.is_empty() || value.starts_with('-') || value.starts_with('+') {
            return None;
        }
        let mut parts = value.split('.');
        let whole = parts.next()?;
        let fractional = parts.next().unwrap_or("");
        if parts.next().is_some()
            || whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fractional.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let whole = whole.trim_start_matches('0');
        Some((
            if whole.is_empty() { "0" } else { whole }.to_string(),
            fractional.trim_end_matches('0').to_string(),
        ))
    }
    normalize(left).is_some_and(|left| normalize(right).is_some_and(|right| left == right))
}

pub(super) fn amount_semantics(scheme: &str) -> &'static str {
    if scheme.eq_ignore_ascii_case("upto") {
        "maximum"
    } else {
        "exact"
    }
}
