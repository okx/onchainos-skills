//! Marketplace service search with a stable output contract.

use anyhow::{anyhow, bail, Context as _, Result};
use serde_json::{json, Map, Number, Value};

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::Context;
use crate::output;

use super::utils::{format_search_rate, wallet_client};
use super::{GetMyAgentsArgs, ServiceMatchArgs};

const SERVICE_MATCH_PATH: &str = "/priapi/v1/aieco/task/asp/service/search";
const PHASE_SUBSCRIPTION_VALIDATION: &str = "subscription_validation";
const TIP_NO_MATCH: &str =
    "No matching services were found on OKX.AI. Try another keyword and search again.";
const TIP_OFFLINE: &str =
    "This Agent is offline and cannot provide the service right now. Search for another service.";
const TIP_CONFIRM: &str = "Reply to confirm that you want to use this service.";
const TIP_MORE: &str = "Tell me which service you want to use, or ask for more.";
const TIP_NO_MORE: &str =
    "There are no more matching services. Tell me which service you want to use.";

pub async fn service_match(args: ServiceMatchArgs, ctx: &Context) -> Result<()> {
    let body = build_request(&args)?;
    let mut client = wallet_client(ctx)?;
    let mut data = if is_precise_search(&args) {
        let access_token = ensure_tokens_refreshed().await?;
        let user_agents = super::queries::get_my_agents_with_access_token(
            &GetMyAgentsArgs {
                role: Some("user".to_string()),
                owner_address: None,
                page: None,
                page_size: None,
            },
            ctx,
            &access_token,
        )
        .await?;
        let agentic_id = require_user_agent_id(&user_agents)?;
        let identity_headers = [("agenticId", agentic_id.as_str())];

        // Precise search is personalized to the current account's User Agent.
        // The client retries once with a refreshed JWT on token revocation while
        // preserving the agenticId header.
        client
            .post_authed_with_headers(
                SERVICE_MATCH_PATH,
                &access_token,
                &body,
                Some(&identity_headers),
            )
            .await?
    } else {
        // Fuzzy search is public: do not read login state and do not attach an
        // Authorization header.
        client.post_public(SERVICE_MATCH_PATH, &body).await?
    };
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

fn is_precise_search(args: &ServiceMatchArgs) -> bool {
    trimmed(args.service_id.as_deref()).is_some() || trimmed(args.asp_agent_id.as_deref()).is_some()
}

fn agent_id_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => trimmed(Some(value)).map(str::to_string),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn extract_user_agent_id(data: &Value) -> Option<String> {
    let entries = data
        .get("list")
        .and_then(Value::as_array)
        .or_else(|| data.as_array())?;

    entries.iter().find_map(|entry| {
        if let Some(agents) = entry.get("agentList").and_then(Value::as_array) {
            agents
                .iter()
                .find_map(|agent| agent.get("agentId").and_then(agent_id_string))
        } else {
            entry.get("agentId").and_then(agent_id_string)
        }
    })
}

fn require_user_agent_id(data: &Value) -> Result<String> {
    extract_user_agent_id(data).ok_or_else(|| {
        anyhow!(
            "no User identity found on this account; create a User identity before using precise service search"
        )
    })
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
    let precise_search = is_precise_search(args);

    let (tip, duplicate_subscription) = if services.is_empty() {
        (Some(TIP_NO_MATCH), None)
    } else if precise_search && services.iter().any(service_is_offline) {
        (Some(TIP_OFFLINE), None)
    } else if services.len() == 1 {
        if precise_search {
            let duplicate_subscription = active_subscription_payload(&services[0]);
            if duplicate_subscription.is_some() {
                (None, duplicate_subscription)
            } else {
                (Some(TIP_CONFIRM), None)
            }
        } else {
            (Some(TIP_CONFIRM), None)
        }
    } else if object.get("hasMore").and_then(Value::as_bool) == Some(true) {
        (Some(TIP_MORE), None)
    } else {
        (Some(TIP_NO_MORE), None)
    };

    if let Some(object) = data.as_object_mut() {
        object.remove("action");
        if let Some(payload) = duplicate_subscription {
            object.remove("tip");
            object.insert(
                "phase".to_string(),
                Value::String(PHASE_SUBSCRIPTION_VALIDATION.to_string()),
            );
            object.insert("decision".to_string(), Value::String("blocked".to_string()));
            object.insert(
                "reason".to_string(),
                Value::String("duplicate_subscription".to_string()),
            );
            object.insert(
                "nextAction".to_string(),
                json!([{"id": "restore_subscription", "recommend": true}]),
            );
            object.insert("payload".to_string(), payload);
        } else {
            for key in ["phase", "decision", "reason", "nextAction", "payload"] {
                object.remove(key);
            }
            if let Some(tip) = tip {
                object.insert("tip".to_string(), Value::String(tip.to_string()));
            }
        }
    }
}

fn active_subscription_payload(service: &Value) -> Option<Value> {
    let subscribed_info = service.get("subscribedInfo")?;
    if subscribed_info.get("isActive").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let job_id = subscribed_info.get("jobId")?.as_str()?.trim();
    if job_id.is_empty() {
        return None;
    }
    Some(json!({"jobId": job_id, "active": true}))
}

fn service_is_offline(service: &Value) -> bool {
    match service.get("asp").and_then(|asp| asp.get("onlineStatus")) {
        Some(Value::Number(value)) => value.as_i64() != Some(1),
        _ => true,
    }
}

fn build_request(args: &ServiceMatchArgs) -> Result<Value> {
    if args.keywords.len() > 10 {
        bail!("service search accepts at most 10 keywords");
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
    let min_payment_token_amount = trimmed(args.min_payment_token_amount.as_deref())
        .map(|value| parse_non_negative_decimal(value, "--min-payment-token-amount"))
        .transpose()?;
    let max_payment_token_amount = trimmed(args.max_payment_token_amount.as_deref())
        .map(|value| parse_non_negative_decimal(value, "--max-payment-token-amount"))
        .transpose()?;

    if let (Some(min), Some(max)) = (
        min_payment_token_amount
            .as_ref()
            .and_then(Number::as_f64),
        max_payment_token_amount
            .as_ref()
            .and_then(Number::as_f64),
    ) {
        if min > max {
            bail!("minPaymentTokenAmount must be less than or equal to maxPaymentTokenAmount");
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
    fn precise_search_requires_sid_or_asp_agent_id() {
        let mut input = args();
        assert!(is_precise_search(&input));

        input.service_id = None;
        assert!(!is_precise_search(&input));

        input.asp_agent_id = Some(" 2864 ".into());
        assert!(is_precise_search(&input));

        input.asp_agent_id = Some("   ".into());
        assert!(!is_precise_search(&input));
    }

    #[test]
    fn extracts_user_agent_id_from_get_my_agents_shapes() {
        let flat = json!({
            "list": [{"agentId": 42, "role": 1}]
        });
        assert_eq!(extract_user_agent_id(&flat).as_deref(), Some("42"));

        let grouped = json!({
            "list": [{
                "ownerAddress": "0xabc",
                "agentList": [{"agentId": " 77 ", "role": 1}]
            }]
        });
        assert_eq!(extract_user_agent_id(&grouped).as_deref(), Some("77"));

        assert_eq!(extract_user_agent_id(&json!({"list": []})), None);
        assert_eq!(
            extract_user_agent_id(&json!({"list": [{"name": "missing id"}]})),
            None
        );

        let error = require_user_agent_id(&json!({"list": []}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("create a User identity"));
    }

    #[test]
    fn adds_no_match_and_pagination_tips() {
        let mut no_match = json!({"services": [], "hasMore": false});
        add_flow_metadata(&mut no_match, &args());
        assert!(no_match.get("action").is_none());
        assert_eq!(no_match["tip"], TIP_NO_MATCH);

        let mut more = json!({"services": [{}, {}], "hasMore": true});
        let mut fuzzy_args = args();
        fuzzy_args.service_id = None;
        add_flow_metadata(&mut more, &fuzzy_args);
        assert!(more.get("action").is_none());
        assert_eq!(more["tip"], TIP_MORE);

        more["hasMore"] = json!(false);
        add_flow_metadata(&mut more, &fuzzy_args);
        assert_eq!(more["tip"], TIP_NO_MORE);
    }

    #[test]
    fn precise_search_handles_offline_and_existing_subscription() {
        let mut offline = json!({
            "services": [{
                "asp": {"onlineStatus": 0},
                "subscribedInfo": {"isActive": true, "jobId": "job-offline"}
            }],
            "hasMore": false
        });
        add_flow_metadata(&mut offline, &args());
        assert!(offline.get("action").is_none());
        assert_eq!(offline["tip"], TIP_OFFLINE);

        let mut subscribed = json!({
            "services": [{
                "asp": {"onlineStatus": 1},
                "subscribedInfo": {
                    "isActive": true,
                    "jobId": " job-123 ",
                    "deviceList": ["device-1"]
                }
            }],
            "hasMore": false
        });
        add_flow_metadata(&mut subscribed, &args());
        assert_eq!(subscribed["phase"], PHASE_SUBSCRIPTION_VALIDATION);
        assert_eq!(subscribed["decision"], "blocked");
        assert_eq!(subscribed["reason"], "duplicate_subscription");
        assert_eq!(
            subscribed["nextAction"],
            json!([{"id": "restore_subscription", "recommend": true}])
        );
        assert_eq!(
            subscribed["payload"],
            json!({"jobId": "job-123", "active": true})
        );
        assert!(subscribed.get("action").is_none());
        assert!(subscribed.get("tip").is_none());

        subscribed["services"][0]["subscribedInfo"]["isActive"] = json!(false);
        add_flow_metadata(&mut subscribed, &args());
        assert!(subscribed.get("action").is_none());
        assert_eq!(subscribed["tip"], TIP_CONFIRM);
        for key in ["phase", "decision", "reason", "nextAction", "payload"] {
            assert!(subscribed.get(key).is_none());
        }
    }

    #[test]
    fn active_subscription_payload_requires_active_subscription_job_id() {
        assert_eq!(
            active_subscription_payload(&json!({
                "subscribedInfo": {"isActive": true, "jobId": "job-1"}
            })),
            Some(json!({"jobId": "job-1", "active": true}))
        );
        assert_eq!(
            active_subscription_payload(&json!({
                "subscribedInfo": {"isActive": false, "jobId": "job-1"}
            })),
            None
        );
        assert_eq!(
            active_subscription_payload(&json!({
                "subscribedInfo": {"isActive": true}
            })),
            None
        );
    }

    #[test]
    fn treats_only_online_status_one_as_online() {
        for offline_status in [
            json!(0),
            json!(2),
            json!("1"),
            json!("0"),
            json!("2"),
            Value::Null,
            json!(true),
        ] {
            let service = json!({"asp": {"onlineStatus": offline_status}});
            assert!(service_is_offline(&service));
        }

        assert!(service_is_offline(&json!({"asp": {}})));
        assert!(service_is_offline(&json!({})));
        assert!(!service_is_offline(&json!({"asp": {"onlineStatus": 1}})));
    }
}
