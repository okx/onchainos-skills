//! Marketplace service search with a stable output contract.

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;
use serde_json::{json, Map, Number, Value};

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context;
use crate::output;

use super::utils::{format_search_rate, wallet_client};
use super::ServiceMatchArgs;

const SERVICE_MATCH_PATH: &str = "/priapi/v1/aieco/task/asp/service/search";

pub async fn service_match(args: ServiceMatchArgs, ctx: &Context) -> Result<()> {
    let body = build_request(&args)?;
    let access_token = ensure_tokens_refreshed().await?;
    let extra_headers = agentic_id_header(&args);
    let mut client = wallet_client(ctx)?;
    // Injects `Authorization: Bearer <accessToken>` and retries once with a
    // refreshed token if the backend reports server-side token revocation.
    let mut data = client
        .post_authed_with_headers(
            SERVICE_MATCH_PATH,
            &access_token,
            &body,
            extra_headers.as_ref().map(|headers| headers.as_slice()),
        )
        .await?;
    normalize_security_ratings(&mut data);
    output::success(with_progression_contract(data));
    Ok(())
}

/// Add the shared Skill/CLI progression contract while preserving the legacy
/// service-match fields consumed by existing task adapters. `payload` is the
/// untouched normalized backend result; the duplicate top-level fields are a
/// temporary compatibility surface and can be removed after all callers read
/// the contract explicitly.
fn with_progression_contract(mut data: Value) -> Value {
    let payload = data.clone();
    let Some(object) = data.as_object_mut() else {
        return json!({
            "phase": "service_selection",
            "decision": "blocked",
            "reason": "search_unavailable",
            "nextAction": [
                {"id": "retry_search", "recommend": true},
                {"id": "stop", "recommend": false}
            ],
            "payload": payload
        });
    };

    let Some(services) = payload.get("services").and_then(Value::as_array) else {
        object.insert("phase".into(), json!("service_selection"));
        object.insert("decision".into(), json!("blocked"));
        object.insert("reason".into(), json!("search_unavailable"));
        object.insert(
            "nextAction".into(),
            json!([
                {"id": "retry_search", "recommend": true},
                {"id": "stop", "recommend": false}
            ]),
        );
        object.insert("payload".into(), payload);
        return data;
    };

    let mut actions = Vec::new();
    for service in services {
        let sid = service.get("sid").and_then(scalar_string);
        if let Some(sid) = sid {
            actions.push(json!({
                "id": "select_service",
                "recommend": actions.is_empty(),
                "params": {"sid": sid}
            }));
        }
    }

    let has_more = payload
        .get("hasMore")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let search_after = payload
        .get("searchAfter")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if has_more {
        if let Some(cursor) = search_after {
            let recommend = actions.is_empty();
            actions.push(json!({
                "id": "load_more",
                "recommend": recommend,
                "params": {"searchAfter": cursor}
            }));
        }
    }
    let recommend_refine = actions.is_empty();
    actions.push(json!({"id": "refine_search", "recommend": recommend_refine}));
    actions.push(json!({"id": "stop", "recommend": false}));

    let reason = match services.len() {
        0 => "no_services",
        1 => "single_service",
        _ => "multiple_services",
    };
    object.insert("phase".into(), json!("service_selection"));
    object.insert("decision".into(), json!("requires_user_input"));
    object.insert("reason".into(), json!(reason));
    object.insert("nextAction".into(), Value::Array(actions));
    object.insert("payload".into(), payload);
    data
}

fn scalar_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

/// Add a ready-to-render rating sourced from the ASP's 0–5 `securityRate`.
fn normalize_security_ratings(data: &mut Value) {
    let Some(services) = data.get_mut("services").and_then(Value::as_array_mut) else {
        return;
    };
    for service in services {
        let Some(asp) = service.get_mut("asp").and_then(Value::as_object_mut) else {
            continue;
        };
        let rating = match asp.get("securityRate") {
            Some(Value::Number(number)) => match number.as_f64() {
                Some(0.0) => "No rating yet".to_string(),
                Some(rate) => format!("★ {}", format_search_rate(rate)),
                None => "—".to_string(),
            },
            _ => "—".to_string(),
        };
        asp.insert("rating".to_string(), Value::String(rating));
    }
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchQuery {
    #[serde(default)]
    keywords: Vec<String>,
    asp_agent_id: Option<String>,
    asp_name: Option<String>,
    service_name: Option<String>,
    sid: Option<String>,
    min_payment_token_amount: Option<Number>,
    max_payment_token_amount: Option<Number>,
    #[serde(default)]
    unsupported_constraints: Vec<String>,
    #[serde(default)]
    requires_user_input: bool,
}

impl SearchQuery {
    fn from_args(args: &ServiceMatchArgs) -> Result<Self> {
        if let Some(raw) = trimmed(args.query_json.as_deref()) {
            let has_legacy_filter = !args.keywords.is_empty()
                || trimmed(args.asp_agent_id.as_deref()).is_some()
                || trimmed(args.asp_name.as_deref()).is_some()
                || trimmed(args.service_name.as_deref()).is_some()
                || trimmed(args.service_id.as_deref()).is_some()
                || trimmed(args.min_payment_token_amount.as_deref()).is_some()
                || trimmed(args.max_payment_token_amount.as_deref()).is_some();
            if has_legacy_filter || trimmed(args.search_after.as_deref()).is_some() {
                bail!("--query-json cannot be combined with legacy initial-search flags or --search-after");
            }
            let query: Self = serde_json::from_str(raw)
                .context("--query-json must match the service search query schema")?;
            query.validate()?;
            return Ok(query);
        }

        let query = Self {
            keywords: args.keywords.clone(),
            asp_agent_id: args.asp_agent_id.clone(),
            asp_name: args.asp_name.clone(),
            service_name: args.service_name.clone(),
            sid: args.service_id.clone(),
            min_payment_token_amount: args
                .min_payment_token_amount
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| parse_non_negative_decimal(value, "--min-payment-token-amount"))
                .transpose()?,
            max_payment_token_amount: args
                .max_payment_token_amount
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| parse_non_negative_decimal(value, "--max-payment-token-amount"))
                .transpose()?,
            unsupported_constraints: Vec::new(),
            requires_user_input: false,
        };
        query.validate()?;
        Ok(query)
    }

    fn validate(&self) -> Result<()> {
        if self.keywords.len() > 10 {
            bail!("service search query accepts at most 10 keywords");
        }
        if self.requires_user_input {
            bail!("service search query requires user clarification");
        }
        if !self.unsupported_constraints.is_empty() {
            bail!(
                "service search query contains unsupported constraints: {}",
                self.unsupported_constraints.join(", ")
            );
        }
        validate_amount(
            self.min_payment_token_amount.as_ref(),
            "minPaymentTokenAmount",
        )?;
        validate_amount(
            self.max_payment_token_amount.as_ref(),
            "maxPaymentTokenAmount",
        )?;
        if let (Some(min), Some(max)) = (
            self.min_payment_token_amount
                .as_ref()
                .and_then(Number::as_f64),
            self.max_payment_token_amount
                .as_ref()
                .and_then(Number::as_f64),
        ) {
            if min > max {
                bail!("minPaymentTokenAmount must be less than or equal to maxPaymentTokenAmount");
            }
        }
        Ok(())
    }
}

fn validate_amount(value: Option<&Number>, field: &str) -> Result<()> {
    if value.is_some_and(|number| {
        !number
            .as_f64()
            .is_some_and(|amount| amount.is_finite() && amount >= 0.0)
    }) {
        bail!("{field} must be greater than or equal to 0");
    }
    Ok(())
}

fn agentic_id_header(args: &ServiceMatchArgs) -> Option<[(&'static str, &str); 1]> {
    trimmed(args.agentic_id.as_deref()).map(|value| [("agenticId", value)])
}

fn build_request(args: &ServiceMatchArgs) -> Result<Value> {
    let query = SearchQuery::from_args(args)?;
    let keywords: Vec<&str> = query
        .keywords
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|keyword| !keyword.is_empty())
        .collect();
    let asp_agent_id = trimmed(query.asp_agent_id.as_deref());
    let asp_name = trimmed(query.asp_name.as_deref());
    let service_name = trimmed(query.service_name.as_deref());
    let service_id = trimmed(query.sid.as_deref());
    let search_after = trimmed(args.search_after.as_deref());
    let min_payment_token_amount = query.min_payment_token_amount;
    let max_payment_token_amount = query.max_payment_token_amount;

    let has_initial_condition = !keywords.is_empty()
        || asp_agent_id.is_some()
        || asp_name.is_some()
        || service_name.is_some()
        || service_id.is_some()
        || min_payment_token_amount.is_some()
        || max_payment_token_amount.is_some();

    if search_after.is_some() && has_initial_condition {
        bail!("--search-after cannot be combined with initial search conditions");
    }
    let mut body = Map::new();
    if let Some(cursor) = search_after {
        body.insert("searchAfter".into(), Value::String(cursor.to_string()));
    } else {
        if !keywords.is_empty() {
            body.insert(
                "keywords".into(),
                Value::Array(
                    keywords
                        .into_iter()
                        .map(|keyword| Value::String(keyword.to_string()))
                        .collect(),
                ),
            );
        }
        insert_string(&mut body, "aspAgentId", asp_agent_id);
        insert_string(&mut body, "aspName", asp_name);
        insert_string(&mut body, "serviceName", service_name);
        insert_string(&mut body, "sid", service_id);
        if let Some(amount) = min_payment_token_amount {
            body.insert("minPaymentTokenAmount".into(), Value::Number(amount));
        }
        if let Some(amount) = max_payment_token_amount {
            body.insert("maxPaymentTokenAmount".into(), Value::Number(amount));
        }
    }
    body.insert("limit".into(), Value::Number(Number::from(args.limit)));
    Ok(Value::Object(body))
}

fn parse_non_negative_decimal(value: &str, argument: &str) -> Result<Number> {
    let number = value
        .parse::<Number>()
        .with_context(|| format!("{argument} must be a valid decimal"))?;
    let non_negative = number
        .as_f64()
        .is_some_and(|amount| amount.is_finite() && amount >= 0.0);
    if !non_negative {
        bail!("{argument} must be greater than or equal to 0");
    }
    Ok(number)
}

fn insert_string(body: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        body.insert(key.to_string(), Value::String(value.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args() -> ServiceMatchArgs {
        ServiceMatchArgs {
            query_json: None,
            keywords: vec!["smart contract".into(), "audit".into()],
            asp_agent_id: None,
            asp_name: None,
            service_name: None,
            service_id: Some(" svc-001 ".into()),
            agentic_id: Some("user-agent-001".into()),
            min_payment_token_amount: Some("5.25".into()),
            max_payment_token_amount: Some("10.50".into()),
            search_after: None,
            limit: 3,
        }
    }

    #[test]
    fn normalizes_display_ratings_and_preserves_feedback_rate() {
        let mut data = json!({
            "services": [
                {"asp": {"securityRate": 4.8, "feedbackRate": 96.2}},
                {"asp": {"securityRate": 5, "feedbackRate": 100}},
                {"asp": {"securityRate": 0, "feedbackRate": 95}},
                {"asp": {"securityRate": null, "feedbackRate": 90}},
                {"asp": {"securityRate": "4.8", "feedbackRate": 80}},
                {"asp": {"feedbackRate": 75}},
                {}
            ]
        });

        normalize_security_ratings(&mut data);

        assert_eq!(data["services"][0]["asp"]["rating"], json!("★ 4.8"));
        assert_eq!(data["services"][1]["asp"]["rating"], json!("★ 5"));
        assert_eq!(data["services"][2]["asp"]["rating"], json!("No rating yet"));
        assert_eq!(data["services"][3]["asp"]["rating"], json!("—"));
        assert_eq!(data["services"][4]["asp"]["rating"], json!("—"));
        assert_eq!(data["services"][5]["asp"]["rating"], json!("—"));
        assert!(data["services"][6].get("asp").is_none());

        assert_eq!(data["services"][0]["asp"]["securityRate"], json!(4.8));
        assert_eq!(data["services"][0]["asp"]["feedbackRate"], json!(96.2));
        assert_eq!(data["services"][1]["asp"]["feedbackRate"], json!(100));
        assert_eq!(data["services"][2]["asp"]["feedbackRate"], json!(95));
        assert_eq!(data["services"][3]["asp"]["feedbackRate"], json!(90));
        assert_eq!(data["services"][4]["asp"]["feedbackRate"], json!(80));
        assert_eq!(data["services"][5]["asp"]["feedbackRate"], json!(75));
    }

    #[test]
    fn initial_request_excludes_header_only_agentic_id() {
        let input = args();
        let body = build_request(&input).unwrap();
        assert_eq!(body["keywords"], json!(["smart contract", "audit"]));
        assert_eq!(body["sid"], json!("svc-001"));
        assert!(body.get("serviceId").is_none());
        assert!(body.get("agenticId").is_none());
        assert_eq!(
            agentic_id_header(&input),
            Some([("agenticId", "user-agent-001")])
        );
        assert_eq!(body["minPaymentTokenAmount"], json!(5.25));
        assert_eq!(body["maxPaymentTokenAmount"], json!(10.50));
        assert_eq!(body["limit"], 3);
        assert!(body.get("searchAfter").is_none());
        assert!(body.get("aspName").is_none());
    }

    #[test]
    fn continuation_request_contains_only_cursor_and_limit() {
        let mut input = args();
        input.keywords.clear();
        input.service_id = None;
        input.min_payment_token_amount = None;
        input.max_payment_token_amount = None;
        input.search_after = Some(" next ".into());
        assert_eq!(
            build_request(&input).unwrap(),
            json!({"searchAfter":"next","limit":3})
        );
    }

    #[test]
    fn agentic_id_only_request_sends_limit_in_body() {
        let mut input = args();
        input.keywords.clear();
        input.service_id = None;
        input.min_payment_token_amount = None;
        input.max_payment_token_amount = None;
        assert_eq!(build_request(&input).unwrap(), json!({"limit":3}));
    }

    #[test]
    fn validates_min_max_price_range() {
        let mut input = args();
        input.min_payment_token_amount = Some("10.51".into());
        input.max_payment_token_amount = Some("10.50".into());
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("must be less than or equal"));

        input.min_payment_token_amount = Some("10.50".into());
        assert!(build_request(&input).is_ok());
    }

    #[test]
    fn rejects_invalid_search_modes_and_values() {
        let mut input = args();
        input.search_after = Some("next".into());
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("cannot be combined"));

        let mut input = args();
        input.keywords = vec!["x".into(); 11];
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("at most 10"));

        let mut input = args();
        input.keywords.clear();
        input.service_id = None;
        input.agentic_id = None;
        input.min_payment_token_amount = None;
        input.max_payment_token_amount = None;
        assert_eq!(build_request(&input).unwrap(), json!({"limit":3}));

        let mut input = args();
        input.min_payment_token_amount = Some("-1".into());
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("greater than or equal"));
    }

    #[test]
    fn structured_query_builds_the_same_backend_request_as_legacy_flags() {
        let legacy = args();
        let expected = build_request(&legacy).unwrap();
        let mut structured = args();
        structured.query_json = Some(
            r#"{"keywords":["smart contract","audit"],"sid":"svc-001","minPaymentTokenAmount":5.25,"maxPaymentTokenAmount":10.50}"#.into(),
        );
        structured.keywords.clear();
        structured.service_id = None;
        structured.min_payment_token_amount = None;
        structured.max_payment_token_amount = None;
        assert_eq!(build_request(&structured).unwrap(), expected);
    }

    #[test]
    fn structured_query_rejects_ambiguity_unsupported_constraints_and_unknown_fields() {
        let mut input = args();
        input.keywords.clear();
        input.service_id = None;
        input.min_payment_token_amount = None;
        input.max_payment_token_amount = None;

        input.query_json = Some(r#"{"requiresUserInput":true}"#.into());
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("clarification"));

        input.query_json = Some(r#"{"unsupportedConstraints":["online only"]}"#.into());
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("unsupported constraints"));

        input.query_json = Some(r#"{"unknown":"value"}"#.into());
        assert!(build_request(&input)
            .unwrap_err()
            .to_string()
            .contains("schema"));
    }

    #[test]
    fn progression_contract_preserves_legacy_fields_and_adds_selection_actions() {
        let result = with_progression_contract(json!({
            "services": [
                {"sid": 35153, "serviceName": "Alpha"},
                {"sid": "35154", "serviceName": "Beta"}
            ],
            "hasMore": true,
            "searchAfter": "cursor-1"
        }));

        assert_eq!(result["reason"], "multiple_services");
        assert_eq!(result["decision"], "requires_user_input");
        assert_eq!(result["services"].as_array().unwrap().len(), 2);
        assert_eq!(result["payload"]["services"].as_array().unwrap().len(), 2);
        assert_eq!(result["nextAction"][0]["id"], "select_service");
        assert_eq!(result["nextAction"][0]["params"]["sid"], "35153");
        assert_eq!(result["nextAction"][2]["id"], "load_more");
        assert_eq!(result["nextAction"][2]["params"]["searchAfter"], "cursor-1");
    }

    #[test]
    fn progression_contract_handles_empty_and_malformed_results() {
        let empty = with_progression_contract(json!({"services": [], "hasMore": false}));
        assert_eq!(empty["reason"], "no_services");
        assert_eq!(empty["nextAction"][0]["id"], "refine_search");

        let malformed = with_progression_contract(json!({"hasMore": false}));
        assert_eq!(malformed["decision"], "blocked");
        assert_eq!(malformed["reason"], "search_unavailable");
        assert_eq!(malformed["nextAction"][0]["id"], "retry_search");
    }
}
