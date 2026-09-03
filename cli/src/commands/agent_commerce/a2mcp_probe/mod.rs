//! OKX.AI A2MCP direct invocation with short-lived local prepared state.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::Url;

use crate::commands::Context as CommandContext;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Subcommand, Debug)]
pub enum A2mcpProbeCommand {
    Probe(ProbeArgs),
    RefreshBalance(RefreshBalanceArgs),
    PreparePayment(PreparePaymentArgs),
}

#[derive(Args, Debug)]
pub struct ProbeArgs {
    #[arg(long = "routing-json")]
    pub routing_json: String,
    #[arg(long, default_value = "{}")]
    pub params_json: String,
}

#[derive(Args, Debug)]
pub struct RefreshBalanceArgs {
    #[arg(long = "prepared-id")]
    pub prepared_id: String,
}

#[derive(Args, Debug)]
pub struct PreparePaymentArgs {
    #[arg(long = "prepared-id")]
    pub prepared_id: String,
    #[arg(long)]
    pub candidate_id: String,
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone)]
struct ProbeInput {
    snapshot: ServiceSnapshot,
    typed_params: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RoutingPayload {
    schema_version: u64,
    service_snapshot: Value,
    #[serde(default)]
    request_spec: Option<RequestSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fields: Vec<FieldConstraint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    required_any_of: Vec<String>,
}

#[derive(Debug, Clone)]
struct ServiceSnapshot {
    raw: Value,
    service_id: String,
    service_name: Option<String>,
    endpoint: Url,
    method: String,
    asp_amount: Option<String>,
    asp_symbol: Option<String>,
    param_plan: Vec<FieldConstraint>,
    required_any_of: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContractError {
    code: &'static str,
    message: String,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ContractError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FieldConstraint {
    name: String,
    #[serde(rename = "type", default = "default_string_type")]
    type_: String,
    #[serde(default = "default_true")]
    required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carrier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

fn default_string_type() -> String {
    "string".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct InputRequired {
    fields: Vec<FieldConstraint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    required_any_of: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Action {
    id: String,
    recommend: bool,
}

impl Action {
    fn new(id: impl Into<String>, recommend: bool) -> Self {
        Self {
            id: id.into(),
            recommend,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeDecision {
    phase: String,
    decision: String,
    reason: String,
    next_action: Vec<Action>,
    payload: Value,
}

impl ProbeDecision {
    fn payment_confirmation(candidate_id: &str, enabled: bool, can_select_other: bool) -> Self {
        let next_action = if enabled {
            vec![
                Action::new("confirm_a2mcp_payment", true),
                Action::new("cancel_a2mcp", false),
            ]
        } else {
            let mut actions = vec![Action::new("fund_a2mcp_token", true)];
            if can_select_other {
                actions.push(Action::new("select_a2mcp_token", false));
            }
            actions.push(Action::new("cancel_a2mcp", false));
            actions
        };
        Self {
            phase: "payment_confirmation".to_string(),
            decision: "requires_user_input".to_string(),
            reason: if enabled {
                "payment_confirmation_required"
            } else {
                "insufficient_balance"
            }
            .to_string(),
            next_action,
            payload: json!({"selectedCandidateId": candidate_id}),
        }
    }

    fn blocked(reason: &str, payload: Value) -> Self {
        Self {
            phase: "endpoint_probe".to_string(),
            decision: "blocked".to_string(),
            reason: reason.to_string(),
            next_action: vec![Action::new("cancel_a2mcp", true)],
            payload,
        }
    }
}

enum HttpOutcome {
    Free { status: u16, body: Value },
    InputRequired(InputRequired),
    Challenge { challenge: Value, body: Value },
    MethodRequired { allow: Option<String> },
    Failed { status: u16, body: Value },
}

pub async fn run(command: A2mcpProbeCommand, _ctx: &CommandContext) -> Result<()> {
    let decision = match command {
        A2mcpProbeCommand::Probe(args) => run_probe(&args).await?,
        A2mcpProbeCommand::RefreshBalance(args) => run_refresh_balance(&args).await?,
        A2mcpProbeCommand::PreparePayment(args) => run_prepare_payment(&args).await?,
    };
    crate::output::success(decision);
    Ok(())
}

async fn run_probe(args: &ProbeArgs) -> Result<ProbeDecision> {
    let input = match parse_probe_input(&args.routing_json, &args.params_json) {
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
    let outcome = match send_probe(&input).await {
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
    match outcome {
        HttpOutcome::InputRequired(mut required) => {
            if let Some(method) = required.method.as_deref() {
                required.method = match normalize_a2mcp_method(method) {
                    Ok(method) => Some(method),
                    Err(error) => {
                        return Ok(ProbeDecision::blocked(
                            error.code,
                            json!({"schemaVersion":1,"message":error.message}),
                        ))
                    }
                };
            }
            Ok(input_required_decision(input, required))
        }
        HttpOutcome::Free { status, body } => Ok(ProbeDecision {
            phase: "endpoint_result".to_string(),
            decision: "ready".to_string(),
            reason: "free_result".to_string(),
            next_action: Vec::new(),
            payload: json!({"schemaVersion":1,"serviceId":input.snapshot.service_id,"statusCode":status,"result":body}),
        }),
        HttpOutcome::MethodRequired { allow } => Ok(ProbeDecision::blocked(
            "request_method_required",
            json!({"schemaVersion":1,"allow":allow}),
        )),
        HttpOutcome::Failed { status, body } => Ok(ProbeDecision::blocked(
            "endpoint_failure",
            json!({"schemaVersion":1,"statusCode":status,"result":body}),
        )),
        HttpOutcome::Challenge { challenge, body } => {
            build_payment_decision(&input, challenge, body).await
        }
    }
}

fn input_required_decision(input: ProbeInput, required: InputRequired) -> ProbeDecision {
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
        method: required
            .method
            .clone()
            .or_else(|| Some(input.snapshot.method.clone())),
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

fn invalid_params_decision(args: &ProbeArgs, error: ContractError) -> ProbeDecision {
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

fn outstanding_request_input(input: &ProbeInput) -> Option<InputRequired> {
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

async fn run_refresh_balance(args: &RefreshBalanceArgs) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::{
        load_a2mcp_prepared_payment, refresh_a2mcp_prepared_payment, replace_a2mcp_prepared_payment,
    };

    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let loaded_at = crate::commands::payment::session_state::now_unix();
    let prepared = load_a2mcp_prepared_payment(&args.prepared_id, &owner_account_id, loaded_at)?;
    let prepared = refresh_a2mcp_prepared_payment(prepared).await?;
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
            json!({
                "candidateId":candidate.candidate_id(),"tokenSymbol":candidate.symbol(),
                "network":candidate.network(),"chainName":candidate.chain_name(),
                "amountAtomic":candidate.amount_atomic(),"amountDisplay":candidate.amount_display(),
                "amountSemantics":amount_semantics(candidate.scheme()),
                "requiredDisplay":candidate.required_amount(),
                "balanceStatus":candidate.balance_status(),"availableDisplay":candidate.available_amount(),
                "shortfallDisplay":candidate.shortfall(),"depositAddress":candidate.deposit_address(),
                "confirmationEnabled":candidate.balance_status()=="sufficient",
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
    let (reason, next_action) = if let Some(candidate_id) = selected_id {
        let confirmation =
            ProbeDecision::payment_confirmation(candidate_id, selected_enabled, false);
        (confirmation.reason, confirmation.next_action)
    } else {
        (
            "token_selection_required".to_string(),
            vec![
                Action::new("select_a2mcp_token", true),
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
            "schemaVersion":1,"endpoint":prepared.frozen_request().endpoint(),
            "method":prepared.frozen_request().method(),"typedParams":prepared.frozen_request().typed_params(),
            "selectedCandidateId":selected_id,"confirmationEnabled":selected_enabled,
            "walletError":prepared.wallet_error(),"candidates":candidates,
            "preparedId":replacement_id,
        }),
    })
}

async fn run_prepare_payment(args: &PreparePaymentArgs) -> Result<ProbeDecision> {
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
        let mut decision = ProbeDecision::payment_confirmation(
            candidate.candidate_id(),
            enabled,
            prepared.candidates().len() > 1,
        );
        decision.payload = json!({
            "schemaVersion":1,
            "endpoint":prepared.frozen_request().endpoint(),
            "method":prepared.frozen_request().method(),
            "typedParams":prepared.frozen_request().typed_params(),
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
    Ok(ProbeDecision {
        phase: "payment_ready".to_string(),
        decision: "ready".to_string(),
        reason: "payment_ready".to_string(),
        next_action: vec![Action::new("execute_a2mcp_payment", true)],
        payload: json!({"schemaVersion":1,"paymentId":intent.payment_id()}),
    })
}

fn parse_probe_input(routing_json: &str, params_json: &str) -> Result<ProbeInput, ContractError> {
    let routing: RoutingPayload =
        serde_json::from_str(routing_json).map_err(|error| ContractError {
            code: "invalid_a2mcp_routing",
            message: format!("routing JSON is invalid: {error}"),
        })?;
    if routing.schema_version != 1 {
        return Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: "schemaVersion must be the integer 1".to_string(),
        });
    }
    let object = routing
        .service_snapshot
        .as_object()
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot must be an object".to_string(),
        })?;
    if object.get("serviceType").and_then(Value::as_str) != Some("A2MCP") {
        return Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.serviceType must equal A2MCP".to_string(),
        });
    }
    let endpoint = object
        .get("endpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.endpoint must be a URL string".to_string(),
        })?;
    let endpoint = Url::parse(endpoint).map_err(|error| ContractError {
        code: "invalid_a2mcp_routing",
        message: format!("serviceSnapshot.endpoint is invalid: {error}"),
    })?;
    if endpoint.scheme() != "https" {
        return Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.endpoint must use HTTPS".to_string(),
        });
    }
    let typed_params = serde_json::from_str::<Value>(params_json)
        .map_err(|error| ContractError {
            code: "invalid_a2mcp_params",
            message: format!("params JSON is invalid: {error}"),
        })?
        .as_object()
        .cloned()
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_params",
            message: "params JSON must be an object".to_string(),
        })?;
    let request_spec = routing.request_spec.clone();
    let param_plan = request_spec
        .as_ref()
        .map(|spec| spec.fields.clone())
        .unwrap_or_else(|| {
            object
                .get("outputSchema")
                .and_then(|value| value.get("input"))
                .map(parse_fields)
                .unwrap_or_default()
        });
    let required_any_of = request_spec
        .as_ref()
        .map(|spec| spec.required_any_of.clone())
        .unwrap_or_else(|| {
            string_array(
                object
                    .get("outputSchema")
                    .and_then(|schema| schema.get("requiredAnyOf")),
            )
        });
    let service_id = scalar_string(object.get("serviceId"))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ContractError {
            code: "invalid_a2mcp_routing",
            message: "serviceSnapshot.serviceId is required".to_string(),
        })?;
    validate_typed_params(&typed_params, &param_plan)?;
    let raw = routing.service_snapshot.clone();
    let method = request_spec
        .and_then(|spec| spec.method)
        .or_else(|| {
            object
                .get("method")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            object
                .get("outputSchema")
                .and_then(|schema| schema.get("method"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "GET".to_string());
    let method = normalize_a2mcp_method(&method)?;
    Ok(ProbeInput {
        snapshot: ServiceSnapshot {
            raw,
            service_id,
            service_name: object
                .get("serviceName")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned),
            endpoint,
            method,
            asp_amount: scalar_string(object.get("feeAmount")),
            asp_symbol: object
                .get("feeTokenSymbol")
                .and_then(Value::as_str)
                .map(str::to_ascii_uppercase),
            param_plan,
            required_any_of,
        },
        typed_params,
    })
}

async fn send_probe(input: &ProbeInput) -> Result<HttpOutcome> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .context("endpoint_failure: failed to build endpoint client")?;
    let plan = to_payment_param_plan(&input.snapshot.param_plan)?;
    let request = crate::commands::payment::http_carrier::build_typed_request(
        &client,
        &input.snapshot.method,
        input.snapshot.endpoint.as_str(),
        &input.typed_params,
        &plan,
    )?;
    let response = request
        .send()
        .await
        .context("endpoint_failure: endpoint request failed")?;
    let status = response.status();
    let allow = response
        .headers()
        .get(reqwest::header::ALLOW)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let challenge_header = response
        .headers()
        .get("PAYMENT-REQUIRED")
        .or_else(|| response.headers().get("WWW-Authenticate"))
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let text = response.text().await.unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    // 405 is terminal for the initial A2MCP release. A response body that also
    // happens to contain schema-like fields must not turn it into a re-Probe.
    if status.as_u16() == 405 {
        return Ok(HttpOutcome::MethodRequired { allow });
    }
    if let Some(required) = discover_input_required(&body)
        .and_then(|required| outstanding_input(required, &input.typed_params))
    {
        return Ok(HttpOutcome::InputRequired(required));
    }
    if status.as_u16() == 402 {
        let raw = challenge_header.unwrap_or_else(|| body.to_string());
        let challenge = crate::commands::payment::dispatcher::decode_payment_blob(&raw)
            .context("unsupported_payment_scheme: malformed 402 challenge")?;
        if let Some(required) = discover_input_required(&challenge)
            .and_then(|required| outstanding_input(required, &input.typed_params))
        {
            return Ok(HttpOutcome::InputRequired(required));
        }
        return Ok(HttpOutcome::Challenge { challenge, body });
    }
    if status.is_success() {
        return Ok(HttpOutcome::Free {
            status: status.as_u16(),
            body,
        });
    }
    Ok(HttpOutcome::Failed {
        status: status.as_u16(),
        body,
    })
}

fn to_payment_param_plan(
    plan: &[FieldConstraint],
) -> Result<Vec<crate::commands::payment::state::ParamSpec>> {
    use crate::commands::payment::state::{ParamCarrier, ParamSpec};
    plan.iter()
        .map(|field| {
            let carrier = match field.carrier.as_deref().unwrap_or("query") {
                "query" => ParamCarrier::Query,
                "body" => ParamCarrier::Body,
                "header" => ParamCarrier::Header,
                "path" => ParamCarrier::Path,
                other => {
                    return Err(anyhow!(
                        "invalid_a2mcp_params: unsupported carrier `{other}`"
                    ))
                }
            };
            Ok(ParamSpec {
                name: field.name.clone(),
                carrier,
                required: field.required,
                type_: field.type_.clone(),
            })
        })
        .collect()
}

fn outstanding_input(
    mut required: InputRequired,
    params: &Map<String, Value>,
) -> Option<InputRequired> {
    let alternatives = &required.required_any_of;
    required.fields.retain(|field| {
        field.required && !alternatives.contains(&field.name) && !params.contains_key(&field.name)
    });
    if required
        .required_any_of
        .iter()
        .any(|name| params.contains_key(name))
    {
        required.required_any_of.clear();
    }
    if required.fields.is_empty() && required.required_any_of.is_empty() {
        None
    } else {
        Some(required)
    }
}

fn discover_input_required(value: &Value) -> Option<InputRequired> {
    if let Some(required) = value
        .get("input_required")
        .filter(|value| value.is_object())
    {
        let fields = required.get("fields").map(parse_fields).unwrap_or_default();
        let required_any_of = string_array(required.get("requiredAnyOf"));
        if !fields.is_empty() || !required_any_of.is_empty() {
            return Some(InputRequired {
                fields,
                required_any_of,
                message: required
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                method: required
                    .get("method")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        value
                            .get("outputSchema")
                            .and_then(|schema| schema.get("method"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    }),
            });
        }
    }
    if value.get("status").and_then(Value::as_str) == Some("input_required") {
        let fields = value
            .get("fields")
            .or_else(|| value.get("requiredArgs"))
            .map(parse_fields)
            .unwrap_or_default();
        let required_any_of = string_array(value.get("requiredAnyOf"));
        if !fields.is_empty() || !required_any_of.is_empty() {
            return Some(InputRequired {
                fields,
                required_any_of,
                message: value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                method: value
                    .get("method")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            });
        }
    }
    let output_schema = value.get("outputSchema");
    let output_fields = output_schema
        .and_then(|schema| schema.get("input"))
        .map(parse_fields)
        .unwrap_or_default()
        .into_iter()
        .filter(|field| field.required)
        .collect::<Vec<_>>();
    let output_required_any_of =
        string_array(output_schema.and_then(|schema| schema.get("requiredAnyOf")));
    if !output_fields.is_empty() || !output_required_any_of.is_empty() {
        return Some(InputRequired {
            fields: output_fields,
            required_any_of: output_required_any_of,
            message: None,
            method: value
                .get("outputSchema")
                .and_then(|schema| schema.get("method"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        });
    }
    let missing = string_array(value.get("missingParams"));
    let names = if missing.is_empty() {
        string_array(value.get("required"))
    } else {
        missing
    };
    if names.is_empty() {
        return None;
    }
    Some(InputRequired {
        fields: names
            .into_iter()
            .map(|name| FieldConstraint {
                name,
                type_: default_string_type(),
                required: true,
                carrier: None,
                description: None,
            })
            .collect(),
        required_any_of: string_array(value.get("requiredAnyOf")),
        message: value
            .get("message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        method: value
            .get("outputSchema")
            .and_then(|schema| schema.get("method"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn parse_fields(value: &Value) -> Vec<FieldConstraint> {
    match value {
        Value::Array(items) => items.iter().filter_map(parse_field).collect(),
        Value::Object(fields) => fields
            .iter()
            .map(|(name, schema)| field_from_schema(name, schema))
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_field(value: &Value) -> Option<FieldConstraint> {
    value
        .as_str()
        .map(|name| field_from_schema(name, &Value::Null))
        .or_else(|| {
            value
                .get("name")
                .and_then(Value::as_str)
                .map(|name| field_from_schema(name, value))
        })
}

fn field_from_schema(name: &str, schema: &Value) -> FieldConstraint {
    FieldConstraint {
        name: name.to_string(),
        type_: schema
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string")
            .to_string(),
        required: schema
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        carrier: schema
            .get("carrier")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        description: schema
            .get("description")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn validate_typed_params(
    params: &Map<String, Value>,
    plan: &[FieldConstraint],
) -> Result<(), ContractError> {
    for field in plan {
        if !is_supported_param_type(&field.type_) {
            return Err(ContractError {
                code: "invalid_a2mcp_routing",
                message: format!(
                    "requestSpec field `{}` has unsupported type `{}`",
                    field.name, field.type_
                ),
            });
        }
        let Some(value) = params.get(&field.name) else {
            continue;
        };
        if !typed_value_matches(value, &field.type_) {
            return Err(ContractError {
                code: "invalid_a2mcp_param_value",
                message: format!("parameter `{}` must be {}", field.name, field.type_),
            });
        }
    }
    Ok(())
}

fn is_supported_param_type(value: &str) -> bool {
    matches!(
        value,
        "string" | "number" | "integer" | "boolean" | "object" | "array"
    )
}

fn normalize_a2mcp_method(method: &str) -> Result<String, ContractError> {
    let method = method.trim().to_ascii_uppercase();
    if matches!(method.as_str(), "GET" | "POST") {
        Ok(method)
    } else {
        Err(ContractError {
            code: "invalid_a2mcp_routing",
            message: format!(
                "A2MCP request method `{method}` is unsupported; expected GET or POST"
            ),
        })
    }
}

fn typed_value_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => false,
    }
}

async fn build_payment_decision(
    input: &ProbeInput,
    challenge: Value,
    merchant_body: Value,
) -> Result<ProbeDecision> {
    use crate::commands::payment::a2mcp::{
        prepare_a2mcp_payment_from_challenge, store_a2mcp_prepared_payment, A2mcpFrozenRequestV1,
        A2mcpPreparedChallengeInput,
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
    let param_plan = to_payment_param_plan(&response_plan)?;
    let paid_method = challenge
        .get("outputSchema")
        .and_then(|schema| schema.get("method"))
        .and_then(Value::as_str)
        .or_else(|| {
            merchant_body
                .get("outputSchema")
                .and_then(|schema| schema.get("method"))
                .and_then(Value::as_str)
        })
        .unwrap_or(&input.snapshot.method);
    let paid_method = match normalize_a2mcp_method(paid_method) {
        Ok(method) => method,
        Err(error) => {
            return Ok(ProbeDecision::blocked(
                error.code,
                json!({"schemaVersion":1,"message":error.message}),
            ))
        }
    };
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
        let confirmation =
            ProbeDecision::payment_confirmation(candidate_id, selected_enabled, false);
        (confirmation.reason, confirmation.next_action)
    } else {
        (
            "token_selection_required".to_string(),
            vec![
                Action::new("select_a2mcp_token", true),
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

fn merge_field_constraints(
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

fn decimal_strings_equal(left: &str, right: &str) -> bool {
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

fn amount_semantics(scheme: &str) -> &'static str {
    if scheme.eq_ignore_ascii_case("upto") {
        "maximum"
    } else {
        "exact"
    }
}

fn scalar_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

#[cfg(test)]
mod tests;
