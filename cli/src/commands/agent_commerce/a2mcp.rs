//! Unified A2MCP service invocation from an immutable marketplace snapshot.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::{Args, Subcommand};
use serde_json::{Map, Value};
use std::time::Duration;

use crate::mcp_client::{McpClient, ToolCallOutcome};

mod prepare;
pub(crate) use prepare::prepare_service;

const INVOKE_TIMEOUT_SECS: u64 = 10;

#[derive(Subcommand, Debug)]
pub enum A2mcpCommand {
    /// Invoke one MCP or HTTP service from its marketplace metadata.
    Invoke(InvokeArgs),
}

#[derive(Args, Debug)]
pub struct InvokeArgs {
    /// Base64-encoded UTF-8 marketplace Service JSON.
    #[arg(long = "service-base64")]
    pub service_base64: String,
    /// Base64-encoded UTF-8 JSON object containing typed business parameters.
    #[arg(long = "params-base64")]
    pub params_base64: String,
}

fn decode_json(value: &str, label: &str) -> Result<Value> {
    let bytes = STANDARD
        .decode(value)
        .with_context(|| format!("{label} is not valid base64"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("{label} is not valid UTF-8 JSON"))
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("service is missing required field `{key}`"))
}

async fn invoke(args: InvokeArgs) -> Result<()> {
    let service = decode_json(&args.service_base64, "--service-base64")?;
    let params = decode_json(&args.params_base64, "--params-base64")?
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("--params-base64 must decode to a JSON object"))?;
    if !required_string(&service, "serviceType")?.eq_ignore_ascii_case("A2MCP") {
        bail!("serviceType must be A2MCP");
    }
    let endpoint = required_string(&service, "endpoint")?;
    let req_type = required_string(&service, "reqType")?.to_ascii_uppercase();
    let method = service
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .trim()
        .to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST") {
        bail!("service method must be GET or POST");
    }
    let tool_name = service.get("toolName").and_then(Value::as_str);
    let input_schema = service.get("inputSchema");
    let result = invoke_service(
        endpoint,
        &req_type,
        &method,
        tool_name,
        input_schema,
        params,
    )
    .await?;
    crate::output::success(result);
    Ok(())
}

enum InvokeOutcome {
    Free(String),
    Challenge { header: String, body: String },
}

async fn invoke_service(
    endpoint: &str,
    req_type: &str,
    method: &str,
    tool_name: Option<&str>,
    input_schema: Option<&Value>,
    params: Map<String, Value>,
) -> Result<Value> {
    match req_type {
        "MCP" => {
            let tool = tool_name
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| anyhow!("invalid_input: MCP service is missing toolName"))?;
            let arguments = crate::mcp_client::coerce_arguments(&params, input_schema);
            let mut client = McpClient::new(endpoint)?;
            client.initialize().await?;
            match client.call_tool(tool, &arguments).await? {
                ToolCallOutcome::Free(result) => Ok(serde_json::json!({
                    "needsConfirm": false,
                    "summary": format!("MCP tool '{tool}' returned a result — no payment required"),
                    "result": result,
                })),
                ToolCallOutcome::Paid { header, body } => {
                    let typed = arguments.as_object().cloned().unwrap_or_default();
                    build_payment_intent(endpoint, Some(tool), typed, "POST", &header, body).await
                }
            }
        }
        "HTTP" => match invoke_http(endpoint, method, &params).await? {
            InvokeOutcome::Free(body) => Ok(serde_json::json!({
                "needsConfirm": false,
                "summary": "Endpoint returned a result — no payment required",
                "result": serde_json::from_str::<Value>(&body).unwrap_or(Value::String(body)),
            })),
            InvokeOutcome::Challenge { header, body } => {
                build_payment_intent(endpoint, None, params, method, &header, body).await
            }
        },
        _ => bail!("invalid_input: reqType must be MCP or HTTP"),
    }
}

async fn invoke_http(
    endpoint: &str,
    method: &str,
    params: &Map<String, Value>,
) -> Result<InvokeOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(INVOKE_TIMEOUT_SECS))
        .build()?;
    let request = match method {
        "GET" => {
            let query = params
                .iter()
                .map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    (key.clone(), value)
                })
                .collect::<Vec<_>>();
            client.get(endpoint).query(&query)
        }
        "POST" => client.post(endpoint).json(params),
        _ => bail!("invalid_input: method must be GET or POST"),
    };
    let response = request.send().await?;
    let status = response.status();
    let header = response
        .headers()
        .get("PAYMENT-REQUIRED")
        .or_else(|| response.headers().get("WWW-Authenticate"))
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response.text().await.unwrap_or_default();
    if status.as_u16() == 402 {
        return Ok(InvokeOutcome::Challenge {
            header: header.unwrap_or_else(|| body.clone()),
            body,
        });
    }
    if status.is_success() {
        return Ok(InvokeOutcome::Free(body));
    }
    bail!(
        "endpoint_unreachable: unexpected HTTP {} (expected 402 or 2xx)",
        status.as_u16()
    )
}

async fn build_payment_intent(
    endpoint: &str,
    mcp_tool: Option<&str>,
    params: Map<String, Value>,
    method: &str,
    challenge: &str,
    merchant_body: String,
) -> Result<Value> {
    use crate::commands::payment::a2mcp::{
        create_a2mcp_payment_intent_for_quote, prepare_a2mcp_payment_from_challenge,
        A2mcpConfirmationContextV1, A2mcpFrozenRequestV1, A2mcpIntentCreateInput,
        A2mcpPreparedChallengeInput,
    };

    let frozen_request = if let Some(tool) = mcp_tool {
        A2mcpFrozenRequestV1::new_mcp(endpoint.to_string(), tool.to_string(), params, None)?
    } else {
        A2mcpFrozenRequestV1::new(
            endpoint.to_string(),
            method.to_string(),
            params,
            vec![],
            None,
        )?
    };
    let prepared = prepare_a2mcp_payment_from_challenge(A2mcpPreparedChallengeInput {
        challenge: challenge.to_string(),
        frozen_request,
        confirmation_context: A2mcpConfirmationContextV1::new(
            String::new(),
            None,
            None,
            None,
            None,
        ),
    })
    .await?;
    let candidate = prepared
        .candidates()
        .iter()
        .find(|candidate| candidate.balance_status() == "sufficient")
        .or_else(|| prepared.candidates().first())
        .ok_or_else(|| anyhow!("unsupported: 402 challenge has no payment candidates"))?;
    let selected_candidate_id = candidate.candidate_id().to_string();
    let selected = prepared.select(candidate.candidate_id())?;
    let owner_account_id = crate::commands::payment::state::current_owner_id()
        .ok_or_else(|| anyhow!("wallet_login_required: no selected wallet"))?;
    let (_, _, payer_address) =
        crate::commands::payment::payment_flow::resolve_chain_and_payer(selected.raw(), None)
            .await?;
    let now = crate::commands::payment::session_state::now_unix();
    let intent = create_a2mcp_payment_intent_for_quote(A2mcpIntentCreateInput {
        probe_id: format!("direct_{}", uuid::Uuid::new_v4().simple()),
        owner_account_id,
        payer_address,
        frozen_request: prepared.frozen_request().clone(),
        selected_accept: selected.clone(),
        created_at: now,
        expires_at: prepared.challenge_expires_at(),
        user_confirmed: false,
    })?;
    let mut candidates = Vec::new();
    let mut alternatives = Vec::new();
    for candidate in prepared.candidates() {
        let view = serde_json::json!({
            "candidateId": candidate.candidate_id(),
            "scheme": candidate.scheme(),
            "network": candidate.network(),
            "chainId": candidate.chain_id(),
            "chainName": candidate.chain_name(),
            "tokenSymbol": candidate.symbol(),
            "amountAtomic": candidate.amount_atomic(),
            "amountHuman": candidate.amount_display(),
            "balanceStatus": candidate.balance_status(),
            "availableAmount": candidate.available_amount(),
            "requiredAmount": candidate.required_amount(),
            "shortfall": candidate.shortfall(),
            "depositAddress": candidate.deposit_address(),
            "recommended": candidate.candidate_id() == selected_candidate_id,
        });
        if candidate.candidate_id() == selected_candidate_id {
            candidates.push(view);
        } else {
            alternatives.push(view);
        }
    }
    Ok(serde_json::json!({
        "paymentId": intent.payment_id(),
        "needsConfirm": true,
        "summary": format!("Pay {} {} on {}", selected.amount(), selected.symbol(), selected.network()),
        "nextStep": format!("onchainos payment pay --payment-id {} --yes", intent.payment_id()),
        "candidates": candidates,
        "alternatives": alternatives,
        "walletError": prepared.wallet_error(),
        "recipient": selected.pay_to(),
        "merchantBody": merchant_body,
    }))
}

pub async fn run(command: A2mcpCommand) -> Result<()> {
    match command {
        A2mcpCommand::Invoke(args) => invoke(args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_object_params() {
        let encoded = STANDARD.encode(b"[]");
        let value = decode_json(&encoded, "params").unwrap();
        assert!(value.as_object().is_none());
    }
}
