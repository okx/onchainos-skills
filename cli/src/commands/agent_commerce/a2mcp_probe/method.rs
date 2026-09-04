use super::*;

pub(super) fn normalize_a2mcp_method(method: &str) -> Result<String, ContractError> {
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

/// Resolve the HTTP method for an A2MCP request without executing or trusting
/// free-form text directly. A runnable curl example is the strongest evidence;
/// the labelled method line is next; a structured caller value is only a final
/// candidate. Missing evidence defaults to GET; a later unsigned 405 may
/// switch the probe once before any payment intent or signature exists.
pub(super) fn resolve_request_method(
    service_description: Option<&str>,
    endpoint: &Url,
    fallback_method: Option<&str>,
) -> Result<String, ContractError> {
    if let Some(description) = service_description {
        if let Some(curl) = extract_curl_example(description) {
            return method_from_curl(&curl, endpoint);
        }
        if let Some(method_text) =
            extract_labelled_value(description, &["request method", "请求方式", "请求方法"])
        {
            return method_from_declared_text(method_text, endpoint);
        }
    }
    if let Some(candidate) = fallback_method {
        return normalize_a2mcp_method(candidate);
    }
    Ok("GET".to_string())
}

/// Resolve a single unsigned-probe fallback after HTTP 405. When `Allow` is
/// present it is authoritative; otherwise 405 itself is sufficient evidence
/// to try the only other method supported by the initial A2MCP contract.
pub(super) fn fallback_method_for_405(current_method: &str, allow: Option<&str>) -> Option<String> {
    let alternate = match current_method {
        "GET" => "POST",
        "POST" => "GET",
        _ => return None,
    };
    if let Some(allow) = allow {
        let permits_alternate = allow
            .split(',')
            .any(|method| method.trim().eq_ignore_ascii_case(alternate));
        return permits_alternate.then(|| alternate.to_string());
    }
    Some(alternate.to_string())
}

/// A default GET that only reached an x402 challenge has not yet proved that
/// the merchant's business handler accepts query parameters. Verify POST with
/// the same typed values before creating prepared payment state. Both requests
/// are unsigned, so this never adds another authorization or confirmation.
pub(super) fn should_verify_default_get_challenge_with_post(
    input: &ProbeInput,
    outcome: &HttpOutcome,
) -> bool {
    input.snapshot.method_was_defaulted
        && input.snapshot.method == "GET"
        && matches!(outcome, HttpOutcome::Challenge { .. })
}

pub(super) fn post_verification_action(
    input: &ProbeInput,
    outcome: &HttpOutcome,
) -> PostVerificationAction {
    match outcome {
        HttpOutcome::Challenge { .. }
        | HttpOutcome::InputRequired(_)
        | HttpOutcome::Free { .. } => PostVerificationAction::AdoptPost,
        HttpOutcome::Failed { status: 400, body } => {
            if discover_endpoint_param_issues(body, &input.typed_params, &input.snapshot.param_plan)
                .is_some()
            {
                PostVerificationAction::AdoptPost
            } else {
                PostVerificationAction::Block
            }
        }
        HttpOutcome::MethodRequired { allow } => {
            if allow.as_deref().is_some_and(|methods| {
                methods
                    .split(',')
                    .any(|method| method.trim().eq_ignore_ascii_case("POST"))
            }) {
                PostVerificationAction::Block
            } else {
                PostVerificationAction::KeepGet
            }
        }
        HttpOutcome::Failed { .. } => PostVerificationAction::Block,
    }
}

pub(super) fn method_verification_blocked() -> ProbeDecision {
    ProbeDecision::blocked(
        "endpoint_failure",
        json!({
            "schemaVersion": 1,
            "message": "The endpoint request method could not be verified before payment."
        }),
    )
}

pub(super) fn request_method_is_defaulted(
    service_description: Option<&str>,
    fallback_method: Option<&str>,
) -> bool {
    if fallback_method.is_some() {
        return false;
    }
    service_description.is_none_or(|description| {
        extract_curl_example(description).is_none()
            && extract_labelled_value(description, &["request method", "请求方式", "请求方法"])
                .is_none()
    })
}

/// Switch an unsigned GET probe to POST only when a structured 400 response
/// proves that multiple submitted top-level values were not received. The
/// caller performs at most one fallback request; payment outcomes never reach
/// this helper.
pub(super) fn fallback_method_for_400(
    current_method: &str,
    status: u16,
    body: &Value,
    params: &Map<String, Value>,
    plan: &[FieldConstraint],
) -> Option<String> {
    if status != 400 || current_method != "GET" || params.is_empty() {
        return None;
    }
    if body.get("expectedMethod").and_then(Value::as_str) == Some("POST")
        || matches!(
            body.get("code").and_then(Value::as_str),
            Some("request_body_required" | "body_required")
        )
    {
        return Some("POST".to_string());
    }

    let mut missing_submitted_fields = HashSet::new();
    for issue in body.get("issues").and_then(Value::as_array)? {
        let Some(name) = top_level_issue_path(issue) else {
            continue;
        };
        let submitted = params.get(name).is_some_and(|value| !value.is_null());
        if !submitted || !issue_reports_missing_value(issue) {
            continue;
        }
        if plan
            .iter()
            .find(|field| field.name == name)
            .is_some_and(|field| {
                field
                    .carrier
                    .as_deref()
                    .is_some_and(|carrier| carrier != "body")
            })
        {
            return None;
        }
        missing_submitted_fields.insert(name);
    }

    (missing_submitted_fields.len() >= 2).then(|| "POST".to_string())
}

/// Convert endpoint-owned validation failures into the existing parameter
/// collection contract. Values are never guessed or corrected by the CLI.
pub(super) fn discover_endpoint_param_issues(
    body: &Value,
    params: &Map<String, Value>,
    plan: &[FieldConstraint],
) -> Option<InputRequired> {
    let mut fields = Vec::new();
    let mut messages = Vec::new();
    let mut seen = HashSet::new();

    if let Some(issues) = body.get("issues").and_then(Value::as_array) {
        for issue in issues {
            let Some(name) = top_level_issue_path(issue) else {
                continue;
            };
            if !params.contains_key(name) && !plan.iter().any(|field| field.name == name) {
                continue;
            }
            if !seen.insert(name.to_string()) {
                continue;
            }
            // A server reporting a submitted value as missing on GET is method
            // evidence, not a request to ask the user for the same value again.
            if params.get(name).is_some_and(|value| !value.is_null())
                && issue_reports_missing_value(issue)
            {
                continue;
            }
            let description = endpoint_issue_description(issue);
            fields.push(endpoint_issue_field(
                name,
                issue,
                params,
                plan,
                &description,
            ));
            messages.push(format!("{name}: {description}"));
        }
    }

    if fields.is_empty() {
        let message = body.get("message").and_then(Value::as_str)?;
        if !message_reports_invalid_value(message) {
            return None;
        }
        let mut named_fields = params
            .keys()
            .filter(|name| contains_identifier(message, name))
            .collect::<Vec<_>>();
        if named_fields.len() == 1 {
            let name = named_fields.pop()?;
            fields.push(endpoint_issue_field(
                name,
                &Value::Null,
                params,
                plan,
                "The endpoint rejected this value. Provide a valid replacement.",
            ));
            messages.push(format!(
                "{name}: The endpoint rejected this value. Provide a valid replacement."
            ));
        }
    }

    (!fields.is_empty()).then(|| InputRequired {
        fields,
        required_any_of: Vec::new(),
        message: Some(messages.join("; ")),
        method: None,
    })
}

pub(super) fn endpoint_issue_description(issue: &Value) -> String {
    if let Some(expected) = issue
        .get("expected")
        .and_then(Value::as_str)
        .filter(|value| is_supported_param_type(value))
    {
        return format!("The endpoint expects a {expected} value.");
    }
    if issue.get("code").and_then(Value::as_str) == Some("too_small") {
        if let Some(minimum) = issue.get("minimum").and_then(Value::as_number) {
            return format!("The endpoint requires a value of at least {minimum}.");
        }
        return "The endpoint rejected this value because it is below the allowed minimum."
            .to_string();
    }
    "The endpoint rejected this value. Provide a valid replacement.".to_string()
}

pub(super) fn endpoint_issue_field(
    name: &str,
    issue: &Value,
    params: &Map<String, Value>,
    plan: &[FieldConstraint],
    message: &str,
) -> FieldConstraint {
    let planned = plan.iter().find(|field| field.name == name);
    let expected_type = issue
        .get("expected")
        .and_then(Value::as_str)
        .filter(|value| is_supported_param_type(value));
    let type_ = expected_type
        .map(ToOwned::to_owned)
        .or_else(|| planned.map(|field| field.type_.clone()))
        .or_else(|| params.get(name).map(json_value_type))
        .unwrap_or_else(default_string_type);
    FieldConstraint {
        name: name.to_string(),
        type_,
        required: true,
        carrier: planned.and_then(|field| field.carrier.clone()),
        description: Some(message.to_string()),
    }
}

pub(super) fn top_level_issue_path(issue: &Value) -> Option<&str> {
    let path = issue.get("path")?.as_array()?;
    if path.len() != 1 {
        return None;
    }
    path.first()?.as_str().filter(|name| !name.is_empty())
}

pub(super) fn issue_reports_missing_value(issue: &Value) -> bool {
    let message_is_required = issue
        .get("message")
        .and_then(Value::as_str)
        .is_some_and(|message| message.to_ascii_lowercase().contains("required"));
    let received_is_missing = issue
        .get("received")
        .is_some_and(|received| match received {
            Value::Null => true,
            Value::String(value) => {
                matches!(value.to_ascii_lowercase().as_str(), "undefined" | "missing")
            }
            _ => false,
        });
    message_is_required && received_is_missing
}

pub(super) fn json_value_type(value: &Value) -> String {
    match value {
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "string",
    }
    .to_string()
}

pub(super) fn contains_identifier(message: &str, identifier: &str) -> bool {
    message.match_indices(identifier).any(|(start, matched)| {
        let before = message[..start].chars().next_back();
        let after = message[start + matched.len()..].chars().next();
        before.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
            && after.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
    })
}

pub(super) fn message_reports_invalid_value(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    ["must", "required", "invalid", "expected", "unsupported"]
        .iter()
        .any(|marker| message.contains(marker))
}

pub(super) fn extract_labelled_value<'a>(description: &'a str, labels: &[&str]) -> Option<&'a str> {
    description
        .lines()
        .find_map(|line| labelled_value(line, labels))
}

pub(super) fn extract_curl_example(description: &str) -> Option<String> {
    let mut lines = description.lines();
    while let Some(line) = lines.next() {
        if let Some(example) = labelled_value(line, &["request example", "请求示例", "请求样例"])
        {
            let lower = example.to_ascii_lowercase();
            if let Some(index) = lower.find("curl") {
                let mut command = example[index..].trim().to_string();
                while command.trim_end().ends_with('\\') {
                    command.pop();
                    let continuation = lines.next()?.trim();
                    command.push(' ');
                    command.push_str(continuation);
                }
                return Some(command);
            }
        }
    }

    description.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        lower.find("curl").and_then(|index| {
            let before = lower[..index].chars().next_back();
            let after = lower[index + 4..].chars().next();
            let boundary_before = before.is_none_or(|ch| !ch.is_ascii_alphanumeric());
            let boundary_after = after.is_none_or(|ch| !ch.is_ascii_alphanumeric());
            let command = line[index..].trim();
            let tokens = shell_like_tokens(command);
            (boundary_before
                && boundary_after
                && tokens
                    .first()
                    .is_some_and(|token| token.eq_ignore_ascii_case("curl"))
                && tokens.iter().any(|token| token.starts_with("https://")))
            .then(|| command.to_string())
        })
    })
}

pub(super) fn labelled_value<'a>(line: &'a str, labels: &[&str]) -> Option<&'a str> {
    let open = line.find('[')?;
    let close = line[open + 1..].find(']')? + open + 1;
    let label = line[open + 1..close].trim().to_ascii_lowercase();
    labels
        .iter()
        .any(|candidate| label == candidate.to_ascii_lowercase())
        .then(|| line[close + 1..].trim())
}

pub(super) fn method_from_curl(curl: &str, endpoint: &Url) -> Result<String, ContractError> {
    let tokens = shell_like_tokens(curl);
    if tokens
        .first()
        .is_none_or(|token| !token.eq_ignore_ascii_case("curl"))
    {
        return Err(invalid_method_contract(
            "request example is not a curl command",
        ));
    }
    let target = tokens
        .iter()
        .skip(1)
        .find(|token| token.starts_with("https://"))
        .ok_or_else(|| invalid_method_contract("curl example must contain an HTTPS URL"))?;
    validate_contract_target(target, endpoint, "curl example")?;

    let mut explicit = None;
    let mut force_get = false;
    let mut has_body = false;
    let mut implicit_unsupported = None;
    let mut index = 1;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "-X" || token == "--request" {
            explicit = tokens.get(index + 1).map(String::as_str);
            index += 1;
        } else if let Some(method) = token.strip_prefix("--request=") {
            explicit = Some(method);
        } else if token.starts_with("-X") && token.len() > 2 {
            explicit = Some(&token[2..]);
        } else if token == "-G" || token == "--get" {
            force_get = true;
        } else if token == "-I" || token == "--head" {
            implicit_unsupported = Some("HEAD");
        } else if token == "-T"
            || token == "--upload-file"
            || token.starts_with("-T") && token.len() > 2
            || token.starts_with("--upload-file=")
        {
            implicit_unsupported = Some("PUT");
        } else if token == "-d"
            || token == "-F"
            || token.starts_with("-d") && token.len() > 2
            || token.starts_with("-F") && token.len() > 2
            || token == "--data"
            || token.starts_with("--data=")
            || token.starts_with("--data-")
            || token == "--json"
            || token.starts_with("--json=")
            || token == "--form"
            || token.starts_with("--form=")
        {
            has_body = true;
        }
        index += 1;
    }
    if let Some(method) = explicit {
        return normalize_a2mcp_method(method);
    }
    if let Some(method) = implicit_unsupported {
        return normalize_a2mcp_method(method);
    }
    if force_get {
        return Ok("GET".to_string());
    }
    if has_body {
        return Ok("POST".to_string());
    }
    Ok("GET".to_string())
}

pub(super) fn method_from_declared_text(
    text: &str,
    endpoint: &Url,
) -> Result<String, ContractError> {
    let has_get = contains_ascii_token(text, "GET");
    let has_post = contains_ascii_token(text, "POST");
    let method = match (has_get, has_post) {
        (false, false) => {
            return Err(invalid_method_contract(
                "request method must identify GET or POST",
            ))
        }
        (true, false) => "GET",
        (false, true) | (true, true) => "POST",
    };

    for token in shell_like_tokens(text) {
        if token.starts_with("https://") {
            validate_contract_target(&token, endpoint, "request method")?;
        } else if token.starts_with('/') {
            let declared_path = token.split(['?', '#']).next().unwrap_or(&token);
            if declared_path != endpoint.path() {
                return Err(invalid_method_contract(
                    "request method path does not match serviceSnapshot.endpoint",
                ));
            }
        }
    }
    Ok(method.to_string())
}

pub(super) fn validate_contract_target(
    target: &str,
    endpoint: &Url,
    source: &str,
) -> Result<(), ContractError> {
    let parsed = Url::parse(target)
        .map_err(|error| invalid_method_contract(&format!("{source} URL is invalid: {error}")))?;
    let same_target = parsed.scheme() == endpoint.scheme()
        && parsed.host_str() == endpoint.host_str()
        && parsed.port_or_known_default() == endpoint.port_or_known_default()
        && parsed.path() == endpoint.path();
    if same_target {
        Ok(())
    } else {
        Err(invalid_method_contract(&format!(
            "{source} URL does not match serviceSnapshot.endpoint"
        )))
    }
}

pub(super) fn contains_ascii_token(text: &str, expected: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.match_indices(expected).any(|(index, value)| {
        let before = upper[..index].chars().next_back();
        let after = upper[index + value.len()..].chars().next();
        before.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
            && after.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
    })
}

pub(super) fn shell_like_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub(super) fn invalid_method_contract(message: &str) -> ContractError {
    ContractError {
        code: "invalid_a2mcp_routing",
        message: message.to_string(),
    }
}
