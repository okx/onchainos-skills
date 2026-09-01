//! Marketplace service search with a stable output contract.

use anyhow::{bail, Context as _, Result};
use serde_json::{Map, Number, Value};

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context;
use crate::output;

use super::utils::{format_search_rate, wallet_client};
use super::ServiceMatchArgs;

const SERVICE_MATCH_PATH: &str = "/priapi/v1/aieco/task/asp/service/search";
const ACTION_RESTORE_SUBSCRIPTION: &str = "restore_subscription";
const TIP_NO_MATCH: &str =
    "No matching services were found on OKX.AI. Try another keyword and search again.";
const TIP_OFFLINE: &str =
    "This Agent is offline and cannot provide the service right now. Search for another service.";
const TIP_CONFIRM: &str = "Reply \"confirm\" to use this service.";
const TIP_MORE: &str = "Tell me which service you want to use, or reply \"show more\".";
const TIP_NO_MORE: &str =
    "There are no more matching services. Tell me which service you want to use.";

pub async fn service_match(args: ServiceMatchArgs, ctx: &Context) -> Result<()> {
    let body = build_request(&args)?;
    let access_token = ensure_tokens_refreshed().await?;
    let mut client = wallet_client(ctx)?;
    // Injects `Authorization: Bearer <accessToken>` and retries once with a
    // refreshed token if the backend reports server-side token revocation.
    let mut data = client
        .post_authed(SERVICE_MATCH_PATH, &access_token, &body)
        .await?;
    normalize_security_ratings(&mut data);
    add_flow_metadata(&mut data, &args);
    output::success(data);
    Ok(())
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

fn add_flow_metadata(data: &mut Value, args: &ServiceMatchArgs) {
    let Some(object) = data.as_object() else {
        return;
    };
    let services = object
        .get("services")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let precise_search = trimmed(args.service_id.as_deref()).is_some()
        || trimmed(args.asp_agent_id.as_deref()).is_some();

    let (action, tip) = if services.is_empty() {
        (Value::Null, Value::String(TIP_NO_MATCH.to_string()))
    } else if precise_search && services.iter().any(service_is_offline) {
        (Value::Null, Value::String(TIP_OFFLINE.to_string()))
    } else if services.len() == 1 {
        if precise_search && services[0].get("isSubscribing").and_then(Value::as_bool) == Some(true)
        {
            (
                Value::String(ACTION_RESTORE_SUBSCRIPTION.to_string()),
                Value::Null,
            )
        } else {
            (Value::Null, Value::String(TIP_CONFIRM.to_string()))
        }
    } else if object.get("hasMore").and_then(Value::as_bool) == Some(true) {
        (Value::Null, Value::String(TIP_MORE.to_string()))
    } else {
        (Value::Null, Value::String(TIP_NO_MORE.to_string()))
    };

    if let Some(object) = data.as_object_mut() {
        object.insert("action".to_string(), action);
        object.insert("tip".to_string(), tip);
    }
}

fn service_is_offline(service: &Value) -> bool {
    match service.get("asp").and_then(|asp| asp.get("onlineStatus")) {
        Some(Value::Number(value)) => value.as_i64() != Some(1),
        Some(Value::String(value)) => value.trim() != "1",
        _ => false,
    }
}

fn build_request(args: &ServiceMatchArgs) -> Result<Value> {
    if args.keywords.len() > 10 {
        bail!("--keywords accepts at most 10 values");
    }

    let keywords: Vec<&str> = args
        .keywords
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|keyword| !keyword.is_empty())
        .collect();
    let asp_agent_id = trimmed(args.asp_agent_id.as_deref());
    let asp_name = trimmed(args.asp_name.as_deref());
    let service_name = trimmed(args.service_name.as_deref());
    let service_id = trimmed(args.service_id.as_deref());
    let search_after = trimmed(args.search_after.as_deref());
    let min_payment_token_amount = args
        .min_payment_token_amount
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| parse_non_negative_decimal(value, "--min-payment-token-amount"))
        .transpose()?;
    let max_payment_token_amount = args
        .max_payment_token_amount
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| parse_non_negative_decimal(value, "--max-payment-token-amount"))
        .transpose()?;

    if let (Some(min), Some(max)) = (
        min_payment_token_amount.as_ref().and_then(Number::as_f64),
        max_payment_token_amount.as_ref().and_then(Number::as_f64),
    ) {
        if min > max {
            bail!(
                "--min-payment-token-amount must be less than or equal to --max-payment-token-amount"
            );
        }
    }

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
            keywords: vec!["smart contract".into(), "audit".into()],
            asp_agent_id: None,
            asp_name: None,
            service_name: None,
            service_id: Some(" svc-001 ".into()),
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
    fn initial_request_uses_only_search_fields() {
        let input = args();
        let body = build_request(&input).unwrap();
        assert_eq!(body["keywords"], json!(["smart contract", "audit"]));
        assert_eq!(body["sid"], json!("svc-001"));
        assert!(body.get("serviceId").is_none());
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
    fn empty_request_sends_default_limit_in_body() {
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
    fn adds_no_match_and_pagination_tips() {
        let mut no_match = json!({"services": [], "hasMore": false});
        add_flow_metadata(&mut no_match, &args());
        assert!(no_match["action"].is_null());
        assert_eq!(no_match["tip"], TIP_NO_MATCH);

        let mut more = json!({"services": [{}, {}], "hasMore": true});
        let mut fuzzy_args = args();
        fuzzy_args.service_id = None;
        add_flow_metadata(&mut more, &fuzzy_args);
        assert!(more["action"].is_null());
        assert_eq!(more["tip"], TIP_MORE);

        more["hasMore"] = json!(false);
        add_flow_metadata(&mut more, &fuzzy_args);
        assert_eq!(more["tip"], TIP_NO_MORE);
    }

    #[test]
    fn precise_search_handles_offline_and_existing_subscription() {
        let mut offline = json!({
            "services": [{"asp": {"onlineStatus": 0}, "isSubscribing": true}],
            "hasMore": false
        });
        add_flow_metadata(&mut offline, &args());
        assert!(offline["action"].is_null());
        assert_eq!(offline["tip"], TIP_OFFLINE);

        let mut subscribed = json!({
            "services": [{"asp": {"onlineStatus": 1}, "isSubscribing": true}],
            "hasMore": false
        });
        add_flow_metadata(&mut subscribed, &args());
        assert_eq!(subscribed["action"], ACTION_RESTORE_SUBSCRIPTION);
        assert!(subscribed["tip"].is_null());

        subscribed["services"][0]["isSubscribing"] = json!(false);
        add_flow_metadata(&mut subscribed, &args());
        assert!(subscribed["action"].is_null());
        assert_eq!(subscribed["tip"], TIP_CONFIRM);
    }
}
