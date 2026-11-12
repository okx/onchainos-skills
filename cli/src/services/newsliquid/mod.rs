//! NewsLiquid service adapter. Secrets stay in the existing credential store.
use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::account::resolve_account_address_for_chain;
use crate::commands::payment::{addr::parse_recipient_addr, http_carrier};
use crate::{keyring_store, wallet_store};

mod operations;
pub(crate) use operations::{call, check_params, require_service};
mod flow;
pub use flow::PurchaseKind;

pub(crate) const ID: &str = "newsliquid";

const BASE: &str = "https://x402-okx.newsliquid.com";
const MAX_BYTES: usize = 8 * 1024 * 1024;
const CANCEL_SECONDS: u64 = 300;

#[derive(Deserialize, Serialize)]
struct Login {
    wallet: String,
    token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Value,
}

pub struct HttpInput {
    pub method: String,
    pub path: String,
    pub wallet: Option<String>,
    pub body: Option<Value>,
    pub headers: HashMap<String, String>,
}

pub fn current_wallet() -> Result<String> {
    let wallets = wallet_store::load_wallets()?.ok_or_else(|| anyhow!("wallet_login_required"))?;
    let address = resolve_account_address_for_chain(&wallets, "196")?;
    Ok(parse_recipient_addr(&address, 196)?.0)
}

fn credential_key(wallet: &str) -> Result<String> {
    let address = parse_recipient_addr(wallet, 196)?.0;
    Ok(format!("newsliquid:{}", address.to_ascii_lowercase()))
}

fn expiry(value: &Value) -> Result<i64> {
    if let Some(seconds) = value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
    {
        return Ok(if seconds > 100_000_000_000 {
            seconds / 1000
        } else {
            seconds
        });
    }
    let text = value
        .as_str()
        .ok_or_else(|| anyhow!("invalid_newsliquid_expiry"))?;
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|date| date.timestamp())
        .map_err(|_| anyhow!("invalid_newsliquid_expiry"))
}

fn load_login(wallet: &str) -> Result<Option<Login>> {
    let map = keyring_store::read_blob()?;
    let Some(value) = map.get(&credential_key(wallet)?) else {
        return Ok(None);
    };
    let login: Login =
        serde_json::from_str(value).map_err(|_| anyhow!("newsliquid_credentials_corrupted"))?;
    if !login.wallet.eq_ignore_ascii_case(wallet) {
        bail!("newsliquid_wallet_mismatch");
    }
    Ok(Some(login))
}

fn load_valid_login(wallet: &str) -> Result<Login> {
    load_login(wallet)?
        .filter(|login| {
            !login.token.is_empty()
                && expiry(&login.expires_at).unwrap_or(0) > chrono::Utc::now().timestamp()
        })
        .ok_or_else(|| anyhow!("newsliquid_login_required"))
}

/// Explicit credential output for user-managed clients; ordinary results stay redacted.
fn token() -> Result<Value> {
    let wallet = current_wallet()?;
    let login = load_valid_login(&wallet)?;
    Ok(json!({"wallet":wallet,"token":login.token,"expiresAt":login.expires_at}))
}

fn store_login(wallet: &str, login: Login) -> Result<Value> {
    if !login.wallet.eq_ignore_ascii_case(wallet) || login.token.is_empty() {
        bail!("newsliquid_login_identity_mismatch");
    }
    if expiry(&login.expires_at)? <= chrono::Utc::now().timestamp() {
        bail!("newsliquid_login_expired");
    }
    let metadata = json!({"wallet": wallet, "expiresAt": login.expires_at, "authenticated": true});
    let value = serde_json::to_string(&login).map_err(|_| anyhow!("newsliquid_store_failed"))?;
    let _lock = CREDENTIAL_UPDATE
        .lock()
        .map_err(|_| anyhow!("newsliquid_credentials_busy"))?;
    keyring_store::set(&credential_key(wallet)?, &value)?;
    Ok(metadata)
}

// Serializes updates within this process; persistence remains the existing credential store.
static CREDENTIAL_UPDATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn invalidate_failed_login(wallet: &str, failed_token: &str) -> Result<()> {
    let _lock = CREDENTIAL_UPDATE
        .lock()
        .map_err(|_| anyhow!("newsliquid_credentials_busy"))?;
    if load_login(wallet)?.is_some_and(|login| login.token == failed_token) {
        keyring_store::delete(&credential_key(wallet)?)?;
    }
    Ok(())
}

pub fn status() -> Result<Value> {
    let wallet = current_wallet()?;
    let login = load_login(&wallet)?;
    let valid = match login.as_ref() {
        Some(login) => expiry(&login.expires_at)? > chrono::Utc::now().timestamp(),
        None => false,
    };
    Ok(json!({"wallet": wallet, "authenticated": valid,
        "expiresAt": login.map(|login| login.expires_at)}))
}

fn id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn allowed_path(method: &str, path: &str) -> bool {
    if !matches!(method, "GET" | "POST")
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('#')
        || path.contains("..")
    {
        return false;
    }
    let plain = path.split('?').next().unwrap_or("");
    let catalog: Value =
        serde_json::from_str(include_str!("endpoints.json")).expect("checked-in endpoint catalog");
    catalog.as_object().unwrap().values().any(|entry| {
        if entry["method"].as_str() != Some(method) {
            return false;
        }
        let template = entry["path"].as_str().unwrap();
        let Some((prefix, suffix)) = template.split_once(":id") else {
            return plain == template;
        };
        let Some(id) = plain
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
        else {
            return false;
        };
        id_valid(id)
    })
}

fn redact(value: &mut Value, token: Option<&str>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if matches!(
                    key.as_str(),
                    "token"
                        | "signature"
                        | "cancelAuth"
                        | "PAYMENT-SIGNATURE"
                        | "authorization_header"
                        | "paymentHeaderValue"
                        | "termsSignature"
                        | "permitSignature"
                        | "permitSingleSignature"
                        | "accessHeaderValue"
                        | "sessionCert"
                ) {
                    *value = json!("[redacted]");
                } else {
                    redact(value, token);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact(value, token);
            }
        }
        Value::String(value) => {
            if let Some(token) = token.filter(|token| !token.is_empty()) {
                *value = value.replace(token, "[redacted]");
            }
        }
        _ => {}
    }
}

fn signature_values(value: &Value, values: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "signature" | "termsSignature" | "permitSignature" | "permitSingleSignature"
                ) {
                    if let Some(value) = value.as_str() {
                        values.push(value.to_owned());
                    }
                } else {
                    signature_values(value, values);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                signature_values(item, values);
            }
        }
        _ => {}
    }
}

/// Only exact error fields can classify a business 402. News text and arbitrary
/// messages are untrusted content, not permission to enter a purchase flow.
fn response_result(
    status: u16,
    path: &str,
    wallet: Option<&str>,
    body: Value,
    retry: Option<String>,
    required: Option<String>,
) -> Value {
    let path = path.split('?').next().unwrap_or("");
    let mut result = json!({"ok": (200..300).contains(&status), "httpStatus":status, "data":body,
        "retryAfter":retry, "paymentRequired":required,
        "requestContext":{"origin":BASE,"path":path,"wallet":wallet}});
    if status == 402 {
        let order_payment = path
            .strip_prefix("/billing/orders/")
            .and_then(|tail| tail.strip_suffix("/pay"))
            .is_some_and(|id| !id.is_empty() && !id.contains('/'));
        let kind = if order_payment
            && result["paymentRequired"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        {
            "payment_required"
        } else {
            let mut quota = false;
            let mut subscription = false;
            for object in [&result["data"], &result["data"]["data"]] {
                for field in ["error", "code"] {
                    let value = &object[field];
                    let code = value.as_str().or_else(|| value["code"].as_str());
                    quota |= code == Some("quota_insufficient");
                    subscription |= code == Some("subscription_required");
                }
            }
            match (quota, subscription) {
                (true, false) if !order_payment => "quota_insufficient",
                (false, true) if !order_payment => "subscription_required",
                _ => "unknown_402",
            }
        };
        result["failureKind"] = json!(kind);
    }
    result
}

fn with_credential_cleanup(mut result: Value, cleanup: Result<()>) -> Value {
    if cleanup.is_err() {
        result["credentialInvalidationFailed"] = json!(true);
    }
    result
}

pub async fn request_http(input: HttpInput) -> Result<Value> {
    if !allowed_path(&input.method, &input.path) {
        bail!("newsliquid_endpoint_not_allowed");
    }
    if input.headers.keys().any(|key| {
        !matches!(
            key.to_ascii_lowercase().as_str(),
            "idempotency-key" | "payment-signature"
        )
    }) {
        bail!("newsliquid_header_not_allowed");
    }
    let path = input.path.split('?').next().unwrap_or("");
    let public = matches!(
        path,
        "/healthz" | "/billing/offers" | "/auth/challenge" | "/auth/login"
    );
    let wallet = if public && path != "/auth/login" {
        None
    } else {
        Some(current_wallet()?)
    };
    if let Some(expected) = input.wallet.as_deref() {
        if !wallet
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        {
            bail!("newsliquid_wallet_changed_before_request");
        }
    }
    let login = if public {
        None
    } else {
        Some(load_valid_login(wallet.as_deref().unwrap())?)
    };
    let token = login.as_ref().map(|login| login.token.as_str());
    let mut private_values: Vec<String> = input
        .headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("payment-signature"))
        .map(|(_, value)| value.clone())
        .collect();
    if let Some(body) = input.body.as_ref() {
        signature_values(body, &mut private_values);
    }
    // ApiClient carries OKX-specific credentials and payment retries. Do not
    // forward those to a third party; reuse its carrier-independent builder.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| anyhow!("newsliquid_transport_failed"))?;
    let url = format!("{BASE}{}", input.path);
    let params = input.body.as_ref().and_then(Value::as_object);
    if input.body.is_some() && params.is_none() {
        bail!("newsliquid_body_must_be_object");
    }
    let mut request = if let Some(params) = params {
        http_carrier::build_typed_request(&client, &input.method, &url, params, &[])
            .map_err(|_| anyhow!("invalid_newsliquid_request_params"))?
    } else {
        http_carrier::build_request(&client, &input.method, &url, &[], &[])
    };
    request = request.header("Accept", "application/json");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    for (key, value) in input.headers {
        request = request.header(key, value);
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow!("newsliquid_transport_outcome_unknown"))?;
    let status = response.status().as_u16();
    let cleanup = if status == 401 && !public {
        invalidate_failed_login(wallet.as_deref().unwrap(), token.unwrap())
    } else {
        Ok(())
    };
    let retry = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let required = response
        .headers()
        .get("PAYMENT-REQUIRED")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("newsliquid_response_incomplete"))?
    {
        if bytes.len() + chunk.len() > MAX_BYTES {
            bail!("newsliquid_response_too_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut body: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        if path == "/auth/login" || !private_values.is_empty() {
            json!({"error":"non_json_response"})
        } else {
            json!({"error":"non_json_response","bodyText":String::from_utf8_lossy(&bytes)})
        }
    });
    let mut new_token = None;
    if path == "/auth/login" && (200..300).contains(&status) && body["success"] != false {
        let data = body.get("data").unwrap_or(&body);
        let login: Login = serde_json::from_value(data.clone())
            .map_err(|_| anyhow!("newsliquid_login_response_invalid"))?;
        new_token = Some(login.token.clone());
        store_login(wallet.as_deref().unwrap(), login)?;
    }
    redact(&mut body, new_token.as_deref().or(token));
    let mut result = with_credential_cleanup(
        response_result(
            status,
            &input.path,
            wallet.as_deref(),
            body,
            retry,
            required,
        ),
        cleanup,
    );
    redact(&mut result, new_token.as_deref().or(token));
    for value in private_values {
        redact(&mut result, Some(&value));
    }
    if let Some(text) = result["data"]["bodyText"].as_str() {
        result["data"]["bodyText"] = json!(text.chars().take(4096).collect::<String>());
    }
    Ok(result)
}

fn service_success(result: &Value) -> bool {
    result["ok"] == true
        && result["data"]["success"] != false
        && result["data"]["data"]["success"] != false
        && result["data"].get("error").is_none_or(Value::is_null)
        && result["data"]["data"]
            .get("error")
            .is_none_or(Value::is_null)
}

/// Account totals, not an exhausted individual quota category, drive the
/// post-query purchase notice. Reuse exact decimal arithmetic from funding.
fn quota_state(result: &Value) -> &'static str {
    if !service_success(result) {
        return "unknown";
    }
    let body = result["data"].get("data").unwrap_or(&result["data"]);
    match body["totalRemaining"]
        .as_str()
        .and_then(|value| crate::funding::readable_shortfall(value, "0"))
    {
        Some(value) if value == "0" => "exhausted",
        Some(_) => "available",
        None => "unknown",
    }
}

async fn query_followup<F, Fut>(operation: &str, mut result: Value, mut request: F) -> Value
where
    F: FnMut(HttpInput) -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let known_limit = matches!(
        result["failureKind"].as_str(),
        Some("quota_insufficient" | "subscription_required")
    );
    let information_query = operation != "healthz" && !operation.starts_with("billing-");
    if operation == "billing-quota" || known_limit || information_query && service_success(&result)
    {
        let quota = if operation == "billing-quota" {
            result.clone()
        } else if let Some(wallet) = result["requestContext"]["wallet"].as_str() {
            tokio::time::timeout(
                Duration::from_secs(5),
                request(HttpInput {
                    method: "GET".into(),
                    path: "/billing/quota".into(),
                    wallet: Some(wallet.into()),
                    body: None,
                    headers: HashMap::new(),
                }),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(|| json!({"ok":false,"error":"quota_check_failed"}))
        } else {
            json!({"ok":false,"error":"quota_wallet_context_missing"})
        };
        let state = quota_state(&quota);
        result["quotaState"] = json!(state);
        result["quotaCheck"] = quota;
        if state == "exhausted" || known_limit {
            // Public catalog read only. Never create/sign/pay an order here.
            result["purchaseOptions"] = tokio::time::timeout(
                Duration::from_secs(5),
                request(HttpInput {
                    method: "GET".into(),
                    path: "/billing/offers".into(),
                    wallet: None,
                    body: None,
                    headers: HashMap::new(),
                }),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(|| json!({"ok":false,"error":"purchase_catalog_unavailable"}));
        }
    }
    result
}

/// Read-only service queries: no login mutation, payment proof or arbitrary URL.
pub async fn query(operation: &str, id: Option<String>, params: Value) -> Result<Value> {
    if matches!(
        operation,
        "auth-login"
            | "auth-challenge"
            | "billing-orders"
            | "billing-orders-id-pay"
            | "billing-orders-id-reconcile"
            | "billing-subscriptions-id-cancel"
            | "twitter-watch-add"
            | "twitter-watch-delete"
    ) {
        bail!("newsliquid_query_requires_native_flow");
    }
    let catalog: Value = serde_json::from_str(include_str!("endpoints.json"))?;
    let entry = catalog
        .get(operation)
        .ok_or_else(|| anyhow!("unknown_newsliquid_operation"))?;
    let mut path = entry["path"].as_str().unwrap().to_owned();
    if path.contains(":id") {
        let id = id
            .as_deref()
            .ok_or_else(|| anyhow!("newsliquid_id_required"))?;
        if !id_valid(id) {
            bail!("invalid_newsliquid_id");
        }
        path = path.replace(":id", id);
    }
    let method = entry["method"].as_str().unwrap().to_owned();
    let result = request_http(HttpInput {
        method,
        path,
        wallet: None,
        body: Some(params),
        headers: HashMap::new(),
    })
    .await?;
    Ok(query_followup(operation, result, request_http).await)
}

fn valid_sub_id(sub_id: &str) -> bool {
    sub_id.len() == 66 && sub_id.starts_with("0x") && hex::decode(&sub_id[2..]).is_ok()
}

pub fn cancel_request(sub_id: &str, contract: &str) -> Result<Value> {
    if !valid_sub_id(sub_id) {
        bail!("invalid_newsliquid_sub_id");
    }
    let contract = parse_recipient_addr(contract, 196)?.0;
    let nonce = crate::payment::subscription::sign::random_bytes32_hex();
    let deadline = chrono::Utc::now().timestamp() as u64 + CANCEL_SECONDS;
    let domain = crate::payment::subscription::eip712::SubDomain {
        chain_id: 196,
        verifying_contract: &contract,
    };
    Ok(
        crate::payment::subscription::eip712::build_cancel_auth_typed_data(
            0, sub_id, 0, &nonce, deadline, &domain,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_cleanup_failure_preserves_unauthorized_response() {
        let original = response_result(
            401,
            "/billing/quota",
            Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            json!({"error":"expired_session"}),
            Some("5".into()),
            None,
        );
        let result = with_credential_cleanup(
            original.clone(),
            Err(anyhow!("credential-store-private-diagnostic")),
        );
        assert_eq!(result["credentialInvalidationFailed"], true);
        assert_eq!(result["ok"], false);
        assert_eq!(result["httpStatus"], 401);
        assert_eq!(result["data"], original["data"]);
        assert_eq!(result["requestContext"], original["requestContext"]);
        assert_eq!(result["retryAfter"], "5");
        assert!(!result
            .to_string()
            .contains("credential-store-private-diagnostic"));
        assert_eq!(with_credential_cleanup(original.clone(), Ok(())), original);
    }

    #[tokio::test]
    async fn query_exhaustion_and_402_share_read_only_purchase_options() {
        let wallet = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let catalog =
            json!({"subscriptions":[{"name":"Max"}],"oneTimePackages":[{"name":"Quota"}]});
        for (status, body, total, expected_state, expected_reads) in [
            (
                200,
                json!({"data":[{"text":"last news"}],"usage":{"quota":"0"}}),
                "0",
                "exhausted",
                2,
            ),
            (
                200,
                json!({"data":[{"text":"last news"}],"usage":{"quota":"0"}}),
                "1",
                "available",
                1,
            ),
            (
                402,
                json!({"error":"quota_insufficient"}),
                "0",
                "exhausted",
                2,
            ),
            (
                402,
                json!({"error":"subscription_required"}),
                "1",
                "available",
                2,
            ),
        ] {
            let original = response_result(
                status,
                "/news_search",
                Some(wallet),
                body.clone(),
                None,
                None,
            );
            let calls = std::cell::RefCell::new(Vec::new());
            let result = query_followup("news-search", original, |input| {
                calls.borrow_mut().push(input.path.clone());
                assert_eq!(input.method, "GET");
                assert!(input.headers.is_empty());
                let reply = if input.path == "/billing/quota" {
                    assert_eq!(input.wallet.as_deref(), Some(wallet));
                    json!({"data":{"totalRemaining":total,"dailyRefresh":{"remaining":"0"}}})
                } else {
                    assert_eq!(input.path, "/billing/offers");
                    assert!(input.wallet.is_none());
                    catalog.clone()
                };
                std::future::ready(Ok(response_result(
                    200,
                    &input.path,
                    input.wallet.as_deref(),
                    reply,
                    None,
                    None,
                )))
            })
            .await;
            assert_eq!(result["data"], body);
            assert_eq!(result["ok"], status == 200);
            assert_eq!(result["quotaState"], expected_state);
            assert_eq!(calls.borrow().len(), expected_reads);
            if expected_reads == 2 {
                assert_eq!(result["purchaseOptions"]["data"], catalog);
            } else {
                assert!(result.get("purchaseOptions").is_none());
            }
        }
    }

    #[tokio::test]
    async fn unknown_402_and_failed_quota_checks_do_not_offer_payment_or_replay() {
        let original = response_result(
            402,
            "/twitter_search",
            None,
            json!({"error":"Payment Required"}),
            None,
            None,
        );
        let result = query_followup("twitter-search", original.clone(), |_| {
            panic!("an unknown 402 must not trigger follow-up requests");
            #[allow(unreachable_code)]
            std::future::ready(Ok(json!({})))
        })
        .await;
        assert_eq!(result, original);
        let original = response_result(
            200,
            "/twitter_search",
            Some("wallet"),
            json!({"data":[]}),
            None,
            None,
        );
        let calls = std::cell::Cell::new(0);
        let result = query_followup("twitter-search", original, |input| {
            calls.set(calls.get() + 1);
            assert_eq!(input.path, "/billing/quota");
            std::future::ready(Err(anyhow!("synthetic quota failure")))
        })
        .await;
        assert_eq!(calls.get(), 1);
        assert_eq!(result["quotaState"], "unknown");
        assert_eq!(result["ok"], true);
        assert!(result.get("purchaseOptions").is_none());
    }

    #[test]
    fn post_query_notice_uses_total_available_quota_with_exact_zero() {
        for (body, expected) in [
            (
                json!({"data":{"totalRemaining":"0.000","dailyRefresh":{"remaining":"0"}}}),
                "exhausted",
            ),
            (
                json!({"totalRemaining":"1","dailyRefresh":{"remaining":"0"},"subscription":{"remaining":"1"}}),
                "available",
            ),
            (
                json!({"totalRemaining":"0.00000000000000000001"}),
                "available",
            ),
            (json!({"dailyRefresh":{"remaining":"0"}}), "unknown"),
            (json!({"totalRemaining":"invalid"}), "unknown"),
            (json!({"success":false,"totalRemaining":"0"}), "unknown"),
            (json!({"error":null,"totalRemaining":"0"}), "exhausted"),
        ] {
            let result = response_result(200, "/billing/quota", None, body, None, None);
            assert_eq!(quota_state(&result), expected);
        }
        assert_eq!(
            quota_state(&response_result(
                401,
                "/billing/quota",
                None,
                json!({"totalRemaining":"0"}),
                None,
                None
            )),
            "unknown"
        );
    }
    #[test]
    fn business_402_requires_an_exact_unambiguous_error_field() {
        let wallet = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for (body, expected) in [
            (json!({"error":"quota_insufficient"}), "quota_insufficient"),
            (
                json!({"data":{"error":{"code":"subscription_required"}}}),
                "subscription_required",
            ),
            (json!({"message":"quota_insufficient"}), "unknown_402"),
            (
                json!({"data":{"items":[{"text":"subscription_required"}]}}),
                "unknown_402",
            ),
            (
                json!({"error":"quota_insufficient", "data":{"code":"subscription_required"}}),
                "unknown_402",
            ),
            (
                json!({"error":"Client error 402 Payment Required"}),
                "unknown_402",
            ),
        ] {
            let result = response_result(
                402,
                "/twitter_user_tweets?private-query=omitted",
                Some(wallet),
                body.clone(),
                Some("5".into()),
                None,
            );
            assert_eq!(result["failureKind"], expected);
            assert_eq!(result["data"], body);
            assert_eq!(result["httpStatus"], 402);
            assert_eq!(result["retryAfter"], "5");
            assert_eq!(result["requestContext"]["origin"], BASE);
            assert_eq!(result["requestContext"]["wallet"], wallet);
            assert_eq!(result["requestContext"]["path"], "/twitter_user_tweets");
            assert!(!result["requestContext"]
                .to_string()
                .contains("private-query"));
        }
    }

    #[test]
    fn payment_challenge_is_only_recognized_on_an_original_order_pay_path() {
        for (status, path, header, expected) in [
            (
                402,
                "/billing/orders/order-1/pay",
                Some("challenge".into()),
                Some("payment_required"),
            ),
            (
                402,
                "/billing/orders/order-1/pay",
                None,
                Some("unknown_402"),
            ),
            (
                402,
                "/billing/orders/order-1/pay",
                Some("".into()),
                Some("unknown_402"),
            ),
            (
                402,
                "/twitter_search",
                Some("challenge".into()),
                Some("unknown_402"),
            ),
            (200, "/twitter_search", None, None),
        ] {
            let result = response_result(status, path, None, json!({}), None, header);
            assert_eq!(result["failureKind"].as_str(), expected);
            assert_eq!(result["ok"], status == 200);
        }
    }

    #[test]
    fn scope_and_path_cannot_escape_service() {
        assert!(allowed_path("GET", "/billing/quota"));
        assert!(allowed_path("POST", "/billing/orders/order-1/pay"));
        for path in [
            "//attacker.example",
            "/billing/orders/../pay",
            "/billing/quota#other",
            "/unknown",
        ] {
            assert!(!allowed_path("GET", path));
        }
        assert!(!allowed_path("DELETE", "/billing/quota"));
    }
    #[test]
    fn wallet_keys_are_scoped_and_case_normalized() {
        let a = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            credential_key(a).unwrap(),
            credential_key(&a.to_uppercase().replacen("0X", "0x", 1)).unwrap()
        );
        assert_ne!(
            credential_key(a).unwrap(),
            credential_key("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()
        );
        assert!(credential_key("../other").is_err());
    }
    #[test]
    fn status_metadata_never_contains_token() {
        let mut body = json!({"data":{"token":"private-token","text":"private-token echoed","signature":"sig"}});
        redact(&mut body, Some("private-token"));
        assert!(!body.to_string().contains("private-token"));
        assert_eq!(body["data"]["signature"], "[redacted]");
    }
    #[test]
    fn canonical_cancel_builder_has_short_window() {
        let before = chrono::Utc::now().timestamp() as u64;
        let data = cancel_request(
            &format!("0x{}", "a".repeat(64)),
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap();
        assert_eq!(data["primaryType"], "CancelAuth");
        assert_eq!(data["message"]["action"], 0);
        assert!(data["message"]["deadline"].as_u64().unwrap() <= before + CANCEL_SECONDS + 1);
    }
    #[test]
    fn credentials_roundtrip_scopes_wallets_and_never_writes_plaintext() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test_tmp/newsliquid-keyring");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let old_home = std::env::var_os("ONCHAINOS_HOME");
        let old_store = std::env::var_os("ONCHAINOS_CREDENTIAL_STORE");
        std::env::set_var("ONCHAINOS_HOME", &root);
        std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", "file");
        let a = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let b = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut wallets: wallet_store::WalletsJson = serde_json::from_value(json!({
            "selectedAccountId":"a",
            "accountsMap":{
                "a":{"addressList":[{"address":a,"chainIndex":"196"}]},
                "b":{"addressList":[{"address":b,"chainIndex":"196"}]}
            }
        }))
        .unwrap();
        wallet_store::save_wallets(&wallets).unwrap();
        assert_eq!(
            token().unwrap_err().to_string(),
            "newsliquid_login_required"
        );
        keyring_store::set("access_token", "wallet-credential-fixture").unwrap();
        let metadata = store_login(
            a,
            Login {
                wallet: a.into(),
                token: "service-token-a-fixture".into(),
                expires_at: json!(chrono::Utc::now().timestamp() + 300),
            },
        )
        .unwrap();
        store_login(
            b,
            Login {
                wallet: b.into(),
                token: "service-token-b-fixture".into(),
                expires_at: json!(chrono::Utc::now().timestamp() + 300),
            },
        )
        .unwrap();
        assert!(!metadata.to_string().contains("service-token-a-fixture"));
        assert_eq!(
            load_login(a).unwrap().unwrap().token,
            "service-token-a-fixture"
        );
        let credential = token().unwrap();
        assert_eq!(credential["wallet"], a);
        assert_eq!(credential["token"], "service-token-a-fixture");
        assert_eq!(credential["expiresAt"], metadata["expiresAt"]);
        assert!(status().unwrap().get("token").is_none());
        wallets.selected_account_id = "b".into();
        wallet_store::save_wallets(&wallets).unwrap();
        assert_eq!(token().unwrap()["token"], "service-token-b-fixture");
        wallets.selected_account_id = "a".into();
        wallet_store::save_wallets(&wallets).unwrap();
        let encrypted = std::fs::read(root.join("keyring.enc")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("service-token-a-fixture"));
        assert!(!root.join("newsliquid").exists());
        store_login(
            a,
            Login {
                wallet: a.into(),
                token: "fresh-service-token-a-fixture".into(),
                expires_at: json!(chrono::Utc::now().timestamp() + 300),
            },
        )
        .unwrap();
        invalidate_failed_login(a, "service-token-a-fixture").unwrap();
        assert_eq!(
            load_login(a).unwrap().unwrap().token,
            "fresh-service-token-a-fixture"
        );
        invalidate_failed_login(a, "fresh-service-token-a-fixture").unwrap();
        assert!(load_login(a).unwrap().is_none());
        assert_eq!(
            token().unwrap_err().to_string(),
            "newsliquid_login_required"
        );
        for expires_at in [json!(0), json!("invalid-expiry")] {
            let invalid = serde_json::to_string(&Login {
                wallet: a.into(),
                token: "expired-token-fixture".into(),
                expires_at,
            })
            .unwrap();
            keyring_store::set(&credential_key(a).unwrap(), &invalid).unwrap();
            assert_eq!(
                token().unwrap_err().to_string(),
                "newsliquid_login_required"
            );
        }
        assert_eq!(
            load_login(b).unwrap().unwrap().token,
            "service-token-b-fixture"
        );
        assert_eq!(
            keyring_store::get("access_token").unwrap(),
            "wallet-credential-fixture"
        );
        match old_home {
            Some(value) => std::env::set_var("ONCHAINOS_HOME", value),
            None => std::env::remove_var("ONCHAINOS_HOME"),
        };
        match old_store {
            Some(value) => std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", value),
            None => std::env::remove_var("ONCHAINOS_CREDENTIAL_STORE"),
        };
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn read_only_queries_cannot_sign_mutate_or_accept_secret_headers() {
        assert!(
            query("billing-orders-id-pay", Some("id-1".into()), json!({}))
                .await
                .is_err()
        );
        assert!(query("twitter-watch-add", None, json!({"username":"test"}))
            .await
            .is_err());
        assert!(request_http(HttpInput {
            method: "GET".into(),
            path: "/healthz".into(),
            wallet: None,
            body: None,
            headers: HashMap::from([("Authorization".into(), "Bearer fixture".into())])
        })
        .await
        .is_err());
    }

    #[test]
    fn reflected_input_signatures_are_redacted_from_error_text() {
        let mut values = Vec::new();
        signature_values(
            &json!({"cancelAuth":{"signature":"proof-fixture"}}),
            &mut values,
        );
        let mut reply =
            json!({"data":{"error":"rejected proof-fixture"},"retryAfter":"proof-fixture"});
        for value in values {
            redact(&mut reply, Some(&value));
        }
        assert!(!reply.to_string().contains("proof-fixture"));
    }
}
