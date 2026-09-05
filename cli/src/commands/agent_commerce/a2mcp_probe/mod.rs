//! OKX.AI A2MCP direct invocation with short-lived local prepared state.

mod contract;
mod flow;
mod free_result;
mod method;
mod probe;

use contract::*;
use flow::*;
use free_result::*;
use method::*;
use probe::*;

use std::{collections::HashSet, time::Duration};

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
    ConfirmFree(ConfirmFreeArgs),
    RefreshBalance(RefreshBalanceArgs),
    Funding(FundingArgs),
    ResumeAfterFunding(ResumeAfterFundingArgs),
    PreparePayment(PreparePaymentArgs),
}

#[derive(Args, Debug)]
pub struct ConfirmFreeArgs {
    #[arg(long = "confirmation-id")]
    pub confirmation_id: String,
    #[arg(long)]
    pub yes: bool,
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
pub struct FundingArgs {
    #[arg(long = "prepared-id")]
    pub prepared_id: String,
    #[arg(long = "candidate-id")]
    pub candidate_id: String,
}

#[derive(Args, Debug)]
pub struct ResumeAfterFundingArgs {
    #[arg(long = "prepared-id")]
    pub prepared_id: String,
    #[arg(long = "candidate-id")]
    pub candidate_id: String,
    #[arg(long)]
    pub yes: bool,
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
    method_was_defaulted: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

impl Action {
    fn new(id: impl Into<String>, recommend: bool) -> Self {
        Self {
            id: id.into(),
            recommend,
            params: None,
        }
    }

    fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
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
    fn payment_confirmation(
        prepared_id: &str,
        candidate_id: &str,
        enabled: bool,
        can_select_other: bool,
    ) -> Self {
        let bound = || json!({"preparedId": prepared_id, "candidateId": candidate_id});
        let next_action = if enabled {
            vec![
                Action::new("confirm_a2mcp_payment", true).with_params(bound()),
                Action::new("cancel_a2mcp", false),
            ]
        } else {
            let mut actions = vec![Action::new("fund_a2mcp_token", true).with_params(bound())];
            if can_select_other {
                actions.push(
                    Action::new("select_a2mcp_token", false)
                        .with_params(json!({"preparedId": prepared_id})),
                );
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

fn normalize_invocation_result(result: Result<ProbeDecision>) -> Result<ProbeDecision> {
    let error = match result {
        Ok(decision) => return Ok(decision),
        Err(error) => error,
    };
    let message = error.to_string();
    let reason =
        if message.starts_with(crate::commands::payment::a2mcp::ERR_PREPARED_EXPIRED_OR_MISSING) {
            "a2mcp_prepared_expired_or_missing"
        } else if message.starts_with(ERR_FREE_RESULT_EXPIRED_OR_MISSING) {
            "a2mcp_free_result_expired_or_missing"
        } else if message.starts_with("a2mcp_invalid_payment_candidate")
            || (message.starts_with(crate::commands::payment::a2mcp::ERR_INVALID_INTENT)
                && message.contains("unknown candidate"))
        {
            "a2mcp_candidate_invalid_or_missing"
        } else if message.starts_with("a2mcp_funding_continuation_required") {
            "a2mcp_funding_continuation_required"
        } else {
            return Err(error);
        };

    Ok(ProbeDecision {
        phase: "invocation_recovery".to_string(),
        decision: "blocked".to_string(),
        reason: reason.to_string(),
        next_action: vec![Action::new("cancel_a2mcp", true)],
        payload: json!({"schemaVersion": 1, "message": message}),
    })
}

enum HttpOutcome {
    Free { status: u16, body: Value },
    InputRequired(InputRequired),
    Challenge { challenge: Value, body: Value },
    MethodRequired { allow: Option<String> },
    Failed { status: u16, body: Value },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostVerificationAction {
    AdoptPost,
    KeepGet,
    Block,
}

pub async fn run(command: A2mcpProbeCommand, _ctx: &CommandContext) -> Result<()> {
    let decision = match command {
        A2mcpProbeCommand::Probe(args) => run_probe(&args).await?,
        A2mcpProbeCommand::ConfirmFree(args) => {
            normalize_invocation_result(run_confirm_free(&args))?
        }
        A2mcpProbeCommand::RefreshBalance(args) => {
            normalize_invocation_result(run_refresh_balance(&args).await)?
        }
        A2mcpProbeCommand::Funding(args) => normalize_invocation_result(run_funding(&args).await)?,
        A2mcpProbeCommand::ResumeAfterFunding(args) => {
            normalize_invocation_result(run_resume_after_funding(&args).await)?
        }
        A2mcpProbeCommand::PreparePayment(args) => {
            normalize_invocation_result(run_prepare_payment(&args).await)?
        }
    };
    crate::output::success(decision);
    Ok(())
}

#[cfg(test)]
mod tests;
