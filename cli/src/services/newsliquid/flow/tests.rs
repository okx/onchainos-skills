use super::*;
use std::collections::VecDeque;
const WALLET: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ASSET: &str = "0xdddddddddddddddddddddddddddddddddddddddd";
const MERCHANT: &str = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const CONTRACT: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PERMIT2: &str = "0xcccccccccccccccccccccccccccccccccccccccc";

#[test]
fn exact_payment_output_must_bind_the_original_wallet_and_header() {
    let good = json!({"scheme":"exact","header_name":"PAYMENT-SIGNATURE","wallet":WALLET});
    verify_exact(&good, WALLET).unwrap();
    for field in ["scheme", "header_name", "wallet"] {
        let mut bad = good.clone();
        bad[field] = json!("different");
        assert!(verify_exact(&bad, WALLET).is_err());
    }
    assert!(verify_exact(&json!({"authorization_header":"fixture"}), WALLET).is_err());
}

struct Fake {
    responses: VecDeque<Result<Value>>,
    signed: VecDeque<Result<Value>>,
    requests: Vec<HttpInput>,
    children: Vec<Vec<String>>,
    wallet: String,
    exact_wallets: Vec<String>,
    period_wallets: Vec<String>,
}
impl Fake {
    fn new() -> Self {
        Self {
            responses: VecDeque::new(),
            signed: VecDeque::new(),
            requests: vec![],
            children: vec![],
            wallet: WALLET.into(),
            exact_wallets: vec![],
            period_wallets: vec![],
        }
    }
}
impl Io for Fake {
    async fn http(&mut self, input: HttpInput) -> Result<Value> {
        self.requests.push(input);
        self.responses.pop_front().expect("unexpected HTTP")
    }
    async fn child(&mut self, args: Vec<String>) -> Result<Value> {
        self.children.push(args);
        self.signed.pop_front().expect("unexpected signer")
    }
    async fn exact_payment(&mut self, payload: &str, wallet: &str) -> Result<Value> {
        self.exact_wallets.push(wallet.to_owned());
        let challenge: Value = serde_json::from_slice(&B64.decode(payload).unwrap()).unwrap();
        assert_eq!(
            challenge["resource"]["url"],
            format!("{BASE}/billing/orders/order-1/pay")
        );
        let result = self.signed.pop_front().expect("unexpected exact signer")?;
        if result["assembleFixture"] == true {
            return crate::commands::payment::payment_flow::pay_with_header_json(
                &crate::commands::payment::payment_flow::PaymentProof::Eip3009 {
                    signature: "test-signature".into(),
                    authorization: json!({"from":wallet}),
                    session_cert: None,
                },
                &challenge["accepts"][0],
                &challenge["resource"],
            );
        }
        Ok(result)
    }
    async fn period_payment(&mut self, _entry: &Value, wallet: &str, _url: &str) -> Result<Value> {
        self.period_wallets.push(wallet.into());
        self.signed.pop_front().expect("unexpected period signer")
    }
    fn wallet(&self) -> Result<String> {
        Ok(self.wallet.clone())
    }
}
fn response(data: Value) -> Value {
    json!({"ok":true,"httpStatus":200,"data":{"success":true,"data":data}})
}
fn entry(renewal: bool) -> Value {
    json!({"scheme":"period","network":"eip155:196","asset":ASSET,"payTo":MERCHANT,"amount":"5000000","extra":{"periodMode":1,"periodSec":0,"maxPeriods":if renewal{12}else{1},"amountPerPeriod":"5000000","startAt":2000000000u64,"initialCharge":{"periodCount":if renewal{0}else{1},"totalAmount":if renewal{"0"}else{"5000000"}},"contracts":{"subscription":CONTRACT,"permit2":PERMIT2},"facilitator":MERCHANT,"plan":{"tier":3,"id":"max"}}})
}
fn order(renewal: bool) -> Value {
    json!({"wallet":WALLET,"orderId":"order-1","kind":if renewal{"auto_renew"}else{"subscription"},"productId":3,"quote":{"network":"eip155:196","asset":ASSET,"payTo":MERCHANT,"amount":"5000000","maxPeriods":if renewal{12}else{1},"startAt":2000000000u64,"autoRenew":renewal},"expiresAt":chrono::Utc::now().timestamp()+300,"paymentStatus":"unpaid"})
}
fn authority() -> Value {
    json!({"subscriptionContract":CONTRACT,"permit2Contract":PERMIT2,"reservedAmount":"0","permit2Allowance":"115792089237316195423570985008687907853269984665640564039457584007913129639935"})
}
fn signed(order: &Value, entry: &Value) -> Value {
    let extra = &entry["extra"];
    json!({"paymentHeaderName":"PAYMENT-SIGNATURE","paymentHeaderValue":"payment-proof-fixture","payload":{"terms":{"payer":WALLET,"merchant":MERCHANT,"token":ASSET,"amountPerPeriod":extra["amountPerPeriod"],"periodMode":extra["periodMode"],"periodSec":extra["periodSec"],"maxPeriods":extra["maxPeriods"],"startAt":extra["startAt"],"initialChargePeriods":extra["initialCharge"]["periodCount"],"initialChargeAmount":extra["initialCharge"]["totalAmount"]},"permit":{"spender":CONTRACT,"details":{"token":ASSET,"amount":order["allowance"]["permitAllowanceCapAmount"],"expiration":2100000000u64}}}})
}
struct Environment {
    root: std::path::PathBuf,
    old: Option<std::ffi::OsString>,
    old_store: Option<std::ffi::OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}
impl Environment {
    fn new(name: &str) -> Self {
        let guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test_tmp")
            .join(format!("native-flow-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let old = std::env::var_os("ONCHAINOS_HOME");
        let old_store = std::env::var_os("ONCHAINOS_CREDENTIAL_STORE");
        std::env::set_var("ONCHAINOS_HOME", &root);
        std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", "file");
        Self {
            root,
            old,
            old_store,
            _guard: guard,
        }
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        if let Some(v) = self.old.take() {
            std::env::set_var("ONCHAINOS_HOME", v)
        } else {
            std::env::remove_var("ONCHAINOS_HOME")
        };
        if let Some(v) = self.old_store.take() {
            std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", v)
        } else {
            std::env::remove_var("ONCHAINOS_CREDENTIAL_STORE")
        };
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn exact_order() -> Value {
    let mut o = order(false);
    o["kind"] = json!("one_time");
    o
}
fn exact_entry() -> Value {
    let mut e = entry(false);
    e["scheme"] = json!("exact");
    e
}
fn challenge(io: &mut Fake, o: Value, e: Value) {
    io.responses.push_back(Ok(response(o)));
    io.responses.push_back(Ok(json!({"ok":false,"httpStatus":402,"paymentRequired":B64.encode(json!({"x402Version":2,"resource":{"url":format!("{BASE}/billing/orders/order-1/pay"),"mimeType":"application/json","description":"6551 order"},"accepts":[e]}).to_string())})));
}
async fn exact_approval(io: &mut Fake) -> PaymentApproval {
    challenge(io, exact_order(), exact_entry());
    serde_json::from_value(
        prepare_payment(io, "order-1", PurchaseKind::OneTime)
            .await
            .unwrap()
            .clone(),
    )
    .unwrap()
}
fn no_state_files(env: &Environment) {
    assert!(
        std::fs::read_dir(&env.root).unwrap().next().is_none(),
        "stateless flows must not create files or directories"
    );
}

#[tokio::test]
async fn create_uses_provider_idempotency_without_local_order_files() {
    let env = Environment::new("create-stateless");
    let mut io = Fake::new();
    io.responses.push_back(Err(anyhow!("creation timeout")));
    assert!(
        create(&mut io, PurchaseKind::Subscription, 3, "same-attempt")
            .await
            .is_err()
    );
    io.responses.push_back(Ok(response(order(false))));
    create(&mut io, PurchaseKind::Subscription, 3, "same-attempt")
        .await
        .unwrap();
    assert_eq!(io.requests.len(), 2);
    assert!(io
        .requests
        .iter()
        .all(|r| r.headers["Idempotency-Key"] == "same-attempt"));
    no_state_files(&env);
}

#[tokio::test]
async fn prepare_fetches_service_order_without_signing_or_writing_state() {
    let env = Environment::new("prepare-stateless");
    let mut io = Fake::new();
    let a = exact_approval(&mut io).await;
    assert_eq!(a.order_id, "order-1");
    assert_eq!(a.wallet, WALLET);
    assert_eq!(io.requests[0].method, "GET");
    assert_eq!(io.requests[0].path, "/billing/orders/order-1");
    assert!(io.exact_wallets.is_empty() && io.period_wallets.is_empty());
    let output = serde_json::to_string(&a).unwrap();
    assert!(!output.contains("payment-proof"));
    no_state_files(&env);
}

#[tokio::test]
async fn paid_authorized_processing_and_unknown_orders_never_sign() {
    let env = Environment::new("order-status");
    for status in ["paid", "authorized", "processing", "failed", "unknown", ""] {
        let mut io = Fake::new();
        let mut o = exact_order();
        o["paymentStatus"] = json!(status);
        io.responses.push_back(Ok(response(o)));
        assert!(prepare_payment(&mut io, "order-1", PurchaseKind::OneTime)
            .await
            .is_err());
        assert_eq!(io.requests.len(), 1);
        assert!(io.exact_wallets.is_empty());
    }
    no_state_files(&env);
}

#[tokio::test]
async fn changed_terms_and_wrong_wallet_reject_before_signing() {
    let env = Environment::new("approval-binding");
    let mut io = Fake::new();
    let a = exact_approval(&mut io).await;
    io.wallet = MERCHANT.into();
    assert!(submit_payment(&mut io, "order-1", a.clone())
        .await
        .unwrap_err()
        .to_string()
        .contains("wallet_changed"));
    assert_eq!(io.requests.len(), 2);
    io.wallet = WALLET.into();
    let mut o = exact_order();
    let mut e = exact_entry();
    o["quote"]["amount"] = json!("6000000");
    e["amount"] = json!("6000000");
    challenge(&mut io, o, e);
    assert!(submit_payment(&mut io, "order-1", a)
        .await
        .unwrap_err()
        .to_string()
        .contains("payment_terms_changed"));
    assert!(io.exact_wallets.is_empty());
    no_state_files(&env);
}

#[tokio::test]
async fn exact_timeout_returns_query_only_diagnosis_and_binds_wallet() {
    let env = Environment::new("exact-timeout");
    let mut io = Fake::new();
    let a = exact_approval(&mut io).await;
    challenge(&mut io, exact_order(), exact_entry());
    io.signed.push_back(Ok(json!({"scheme":"exact","header_name":"PAYMENT-SIGNATURE","wallet":WALLET,"authorization_header":"exact-proof-fixture"})));
    io.responses
        .push_back(Err(anyhow!("submit timeout exact-proof-fixture")));
    let error = submit_payment(&mut io, "order-1", a).await.unwrap_err();
    let coded = error.downcast_ref::<CodedError>().unwrap();
    assert_eq!(coded.code, "newsliquid_operation_outcome_unknown");
    assert_eq!(coded.data.as_ref().unwrap()["retry"], "query-only");
    assert_eq!(coded.data.as_ref().unwrap()["stage"], "submitting");
    assert!(!error.to_string().contains("exact-proof-fixture"));
    assert_eq!(io.exact_wallets, [WALLET]);
    assert_eq!(io.requests.last().unwrap().wallet.as_deref(), Some(WALLET));
    // The Agent follows query-only. A later processing status is not a new payment.
    let mut o = exact_order();
    o["paymentStatus"] = json!("processing");
    io.responses.push_back(Ok(response(o)));
    assert!(prepare_payment(&mut io, "order-1", PurchaseKind::OneTime)
        .await
        .is_err());
    assert_eq!(io.exact_wallets.len(), 1);
    no_state_files(&env);
}

#[tokio::test]
async fn period_uses_shared_signer_without_subscription_cache() {
    let env = Environment::new("period-stateless");
    let mut io = Fake::new();
    let o = order(true);
    let e = entry(true);
    challenge(&mut io, o.clone(), e.clone());
    io.signed.push_back(Ok(authority()));
    let prepared = prepare_payment(&mut io, "order-1", PurchaseKind::AutoRenew)
        .await
        .unwrap();
    let a: PaymentApproval = serde_json::from_value(prepared.clone()).unwrap();
    challenge(&mut io, o.clone(), e.clone());
    io.signed.push_back(Ok(authority()));
    let mut signing_order = o;
    signing_order["allowance"] = prepared["allowance"].clone();
    io.signed.push_back(Ok(signed(&signing_order, &e)));
    io.responses
        .push_back(Ok(response(json!({"paymentStatus":"authorized"}))));
    submit_payment(&mut io, "order-1", a).await.unwrap();
    assert_eq!(io.period_wallets, [WALLET]);
    assert!(io.children.iter().all(|a| a[2] == "allowance-status"));
    no_state_files(&env);
}

#[tokio::test]
async fn bounded_allowance_failure_remains_proven_before_signing() {
    let env = Environment::new("allowance-stateless");
    let e = entry(true);
    let mut io = Fake::new();
    let mut low = authority();
    low["permit2Allowance"] = json!("0");
    io.signed.push_back(Ok(low));
    let error = allowance(&mut io, &e, WALLET).await.unwrap_err();
    let coded = error.downcast_ref::<CodedError>().unwrap();
    assert_eq!(coded.code, "permit2_allowance_insufficient");
    assert_eq!(coded.data.as_ref().unwrap()["stage"], "before_signing");
    assert!(io.period_wallets.is_empty());
    no_state_files(&env);
}

#[tokio::test]
async fn renewal_invalid_start_stops_before_allowance_or_signing() {
    let _env = Environment::new("renewal-invalid-start");
    let now = chrono::Utc::now().timestamp();
    for start in [
        None,
        Some(Value::Null),
        Some(json!(0)),
        Some(json!(-1)),
        Some(json!(now - 1)),
        Some(json!(now)),
        Some(json!((now + 3600).to_string())),
        Some(json!(true)),
        Some(json!(i64::MAX)),
        Some(json!(u64::MAX)),
    ] {
        let mut io = Fake::new();
        let mut o = order(true);
        let mut e = entry(true);
        if let Some(start) = start {
            o["quote"]["startAt"] = start.clone();
            e["extra"]["startAt"] = start;
        } else {
            o["quote"].as_object_mut().unwrap().remove("startAt");
            e["extra"].as_object_mut().unwrap().remove("startAt");
        }
        challenge(&mut io, o, e);
        let error = prepare_payment(&mut io, "order-1", PurchaseKind::AutoRenew)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("renewal_start_or_initial_charge_mismatch"));
        assert!(io.children.is_empty());
        assert!(io.period_wallets.is_empty() && io.exact_wallets.is_empty());
        assert_eq!(io.requests.len(), 2);
    }
}

#[tokio::test]
async fn renewal_future_start_is_preserved_and_must_match_the_challenge() {
    let _env = Environment::new("renewal-future-start");
    let start = chrono::Utc::now().timestamp() + 3600;
    let mut o = order(true);
    let mut e = entry(true);
    o["quote"]["startAt"] = json!(start);
    e["extra"]["startAt"] = json!(start);
    let mut io = Fake::new();
    challenge(&mut io, o.clone(), e.clone());
    io.signed.push_back(Ok(authority()));
    let prepared = prepare_payment(&mut io, "order-1", PurchaseKind::AutoRenew)
        .await
        .unwrap();
    assert_eq!(prepared["quote"]["startAt"], start);
    assert_eq!(prepared["accepted"]["extra"]["startAt"], start);
    assert!(io.period_wallets.is_empty());

    e["extra"]["startAt"] = json!(start + 1);
    assert!(validate_terms(&o, &e).is_err());
    let o = order(false);
    let mut e = entry(false);
    e["extra"]["startAt"] = json!(0);
    validate_terms(&o, &e).unwrap();
}

#[tokio::test]
async fn login_warning_rechecks_wallet_and_fresh_challenge_without_file() {
    let env = Environment::new("login-stateless");
    let mut io = Fake::new();
    let c = json!({"challengeId":"challenge-1","message":format!("Sign in x402-okx.newsliquid.com for {WALLET}; no payment authority"),"expiresAt":chrono::Utc::now().timestamp()+300});
    io.responses.push_back(Ok(response(c.clone())));
    io.signed.push_back(Err(output::CliConfirming {
        message: "Wallet warning".into(),
        next: "confirm".into(),
        scene: None,
    }
    .into()));
    let error = login(&mut io, false, None).await.unwrap_err();
    assert!(warning(&error));
    assert!(error
        .downcast_ref::<output::CliConfirming>()
        .unwrap()
        .next
        .contains(WALLET));
    io.wallet = MERCHANT.into();
    assert!(login(&mut io, true, Some(WALLET)).await.is_err());
    io.wallet = WALLET.into();
    let mut fresh = c;
    fresh["challengeId"] = json!("challenge-2");
    io.responses.push_back(Ok(response(fresh)));
    io.signed
        .push_back(Ok(json!({"signature":"login-proof-fixture"})));
    io.responses.push_back(Ok(response(
        json!({"wallet":WALLET,"expiresAt":chrono::Utc::now().timestamp()+300}),
    )));
    login(&mut io, true, Some(WALLET)).await.unwrap();
    assert_eq!(
        io.requests.last().unwrap().body.as_ref().unwrap()["challengeId"],
        "challenge-2"
    );
    assert!(io.children.last().unwrap().contains(&"--force".into()));
    no_state_files(&env);
}

fn cancel_reads(io: &mut Fake, id: &str) {
    io.responses.push_back(Ok(response(
        json!({"autoRenew":{"subId":id,"status":"active"}}),
    )));
    io.responses
        .push_back(Ok(response(json!([{"subId":id,"quote":{"asset":ASSET}}]))));
    io.signed.push_back(Ok(authority()));
}
#[tokio::test]
async fn cancellation_checks_provider_ownership_and_does_not_write_history() {
    let env = Environment::new("cancel-stateless");
    let mut io = Fake::new();
    let id = format!("0x{}", "2".repeat(64));
    io.responses.push_back(Ok(response(
        json!({"autoRenew":{"subId":"other","status":"active"}}),
    )));
    assert!(cancel(&mut io, &id, false, None).await.is_err());
    assert!(io.children.is_empty());
    cancel_reads(&mut io, &id);
    io.signed
        .push_back(Ok(json!({"signature":"cancel-proof-fixture"})));
    io.responses
        .push_back(Ok(response(json!({"subId":id,"state":"cancelled"}))));
    cancel(&mut io, &id, false, None).await.unwrap();
    assert_eq!(
        io.requests.last().unwrap().path,
        format!("/billing/subscriptions/{id}/cancel")
    );
    no_state_files(&env);
}

#[test]
fn monthly_cadence_and_signed_identity_checks_are_preserved() {
    let mut o = order(true);
    let e = entry(true);
    validate_terms(&o, &e).unwrap();
    let mut short = e.clone();
    short["extra"]["periodMode"] = json!(0);
    short["extra"]["periodSec"] = json!(1);
    assert!(validate_terms(&o, &short).is_err());
    o["allowance"] = json!({"permitAllowanceCapAmount":"60000000","spender":CONTRACT});
    let good = signed(&o, &e);
    verify_signed(&o, &e, &good, WALLET).unwrap();
    for field in [
        "periodMode",
        "periodSec",
        "maxPeriods",
        "amountPerPeriod",
        "startAt",
    ] {
        let mut bad = good.clone();
        bad["payload"]["terms"][field] = json!(9);
        assert!(verify_signed(&o, &e, &bad, WALLET).is_err());
    }
}

#[tokio::test]
async fn payment_and_cancel_need_explicit_consent_before_wallet_access() {
    let approval = PaymentApproval {
        wallet: WALLET.into(),
        order_id: "order-1".into(),
        kind: PurchaseKind::OneTime,
        quote: json!({}),
        accepted: json!({}),
        allowance: Value::Null,
        expires_at: json!(0),
        auto_renew_terms: Value::Null,
        requires_confirmation: true,
    };
    assert!(run(Command::Pay {
        id: "order-1".into(),
        approval,
        confirmed: false
    })
    .await
    .unwrap_err()
    .to_string()
    .contains("explicit_payment_confirmation_required"));
    assert!(run(Command::Cancel {
        id: "id".into(),
        confirmed: false,
        warning: false,
        wallet: None
    })
    .await
    .unwrap_err()
    .to_string()
    .contains("explicit_cancellation_confirmation_required"));
}

#[tokio::test]
async fn exact_preserves_complete_server_challenge_and_real_header_encoding() {
    let env = Environment::new("exact-envelope");
    let mut io = Fake::new();
    let a = exact_approval(&mut io).await;
    challenge(&mut io, exact_order(), exact_entry());
    io.signed.push_back(Ok(json!({"assembleFixture":true})));
    io.responses
        .push_back(Ok(response(json!({"paymentStatus":"paid"}))));
    submit_payment(&mut io, "order-1", a).await.unwrap();
    let header = &io.requests.last().unwrap().headers["PAYMENT-SIGNATURE"];
    let envelope: Value = serde_json::from_slice(&B64.decode(header).unwrap()).unwrap();
    assert_eq!(envelope["x402Version"], 2);
    assert_eq!(
        envelope["resource"]["url"],
        format!("{BASE}/billing/orders/order-1/pay")
    );
    assert_eq!(envelope["resource"]["description"], "6551 order");
    assert_eq!(envelope["payload"]["authorization"]["from"], WALLET);
    no_state_files(&env);
}

#[tokio::test]
async fn missing_exact_resource_stops_before_signing() {
    let env = Environment::new("missing-resource");
    let mut io = Fake::new();
    io.responses.push_back(Ok(response(exact_order())));
    io.responses.push_back(Ok(json!({"ok":false,"httpStatus":402,"paymentRequired":B64.encode(json!({"accepts":[exact_entry()]}).to_string())})));
    assert!(prepare_payment(&mut io, "order-1", PurchaseKind::OneTime)
        .await
        .unwrap_err()
        .to_string()
        .contains("payment_resource_mismatch_or_missing"));
    assert!(io.exact_wallets.is_empty());
    no_state_files(&env);
}
