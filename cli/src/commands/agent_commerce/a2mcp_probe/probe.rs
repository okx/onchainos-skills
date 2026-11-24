use super::*;

pub(super) async fn send_probe(input: &ProbeInput) -> Result<HttpOutcome> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .context("endpoint_failure: failed to build endpoint client")?;
    let plan = to_payment_param_plan(&input.snapshot.param_plan, &input.snapshot.method)?;
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
    // Keep 405 distinct from schema-like response bodies so the caller may
    // perform one unsigned method fallback before returning a final decision.
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
    if let Some(required) = discover_input_fallback_hint(&body) {
        return Ok(HttpOutcome::InputRequired(required));
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
