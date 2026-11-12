//! Stateless service operations. The Agent keeps IDs and unsigned approvals; the provider owns orders.
use super::{
    cancel_request, current_wallet, expiry, id_valid, load_login, query, request_http,
    service_success, status, valid_sub_id, HttpInput, BASE,
};
use crate::commands::payment::addr::parse_recipient_addr;
use crate::commands::sink::CodedError;
use crate::output;
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};
use tokio::process::Command as Process;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseKind {
    Subscription,
    OneTime,
    AutoRenew,
}
impl PurchaseKind {
    fn name(self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::OneTime => "one_time",
            Self::AutoRenew => "auto_renew",
        }
    }
}

pub enum Command {
    Login {
        warning: bool,
        wallet: Option<String>,
    },
    Create {
        kind: PurchaseKind,
        product: u64,
        key: String,
    },
    Prepare {
        id: String,
        kind: PurchaseKind,
    },
    Pay {
        id: String,
        approval: PaymentApproval,
        confirmed: bool,
    },
    Cancel {
        id: String,
        confirmed: bool,
        warning: bool,
        wallet: Option<String>,
    },
}

trait Io {
    async fn http(&mut self, input: HttpInput) -> Result<Value>;
    async fn child(&mut self, args: Vec<String>) -> Result<Value>;
    async fn exact_payment(&mut self, payload: &str, wallet: &str) -> Result<Value>;
    async fn period_payment(&mut self, entry: &Value, wallet: &str, url: &str) -> Result<Value>;
    fn wallet(&self) -> Result<String>;
}
struct Live;
impl Io for Live {
    async fn http(&mut self, input: HttpInput) -> Result<Value> {
        request_http(input).await
    }
    async fn child(&mut self, args: Vec<String>) -> Result<Value> {
        let output = tokio::time::timeout(
            Duration::from_secs(90),
            Process::new(std::env::current_exe()?)
                .args(&args)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| anyhow!("newsliquid_signer_outcome_unknown"))?
        .map_err(|_| anyhow!("newsliquid_signer_unavailable"))?;
        let body: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| anyhow!("newsliquid_signer_invalid_response"))?;
        child_result(output.status.code(), body)
    }
    async fn exact_payment(&mut self, payload: &str, wallet: &str) -> Result<Value> {
        tokio::time::timeout(
            Duration::from_secs(90),
            crate::commands::payment::dispatcher::sign_pay_payload(payload, Some(wallet), None),
        )
        .await
        .map_err(|_| anyhow!("newsliquid_signer_outcome_unknown"))?
        .map_err(exact_signing_error)
    }
    async fn period_payment(&mut self, entry: &Value, wallet: &str, url: &str) -> Result<Value> {
        let (chain_index, chain_id, payer) =
            crate::commands::payment::payment_flow::resolve_chain_and_payer(entry, Some(wallet))
                .await?;
        let signed = tokio::time::timeout(
            Duration::from_secs(90),
            crate::payment::subscription::sign::sign_subscribe(
                &chain_index,
                chain_id,
                &payer,
                entry,
            ),
        )
        .await
        .map_err(|_| anyhow!("newsliquid_signer_outcome_unknown"))?
        .map_err(crate::commands::payment::payment_flow::map_pre_signing_allowance_error)?;
        let (name, header) =
            crate::commands::payment::subscription::build_subscription_payment_header(
                entry,
                Some(url),
                &signed.payload,
            )?;
        Ok(
            json!({"paymentHeaderName":name,"paymentHeaderValue":header,"payload":signed.payload,"subId":signed.sub_id,"chainIndex":signed.chain_index}),
        )
    }
    fn wallet(&self) -> Result<String> {
        current_wallet()
    }
}
fn exact_signing_error(error: anyhow::Error) -> anyhow::Error {
    let mut error = crate::commands::payment::payment_flow::map_pre_signing_allowance_error(error);
    if let Some(coded) = error.downcast_mut::<CodedError>() {
        if coded.code == "permit2_allowance_insufficient" {
            coded.message = "Token allowance is insufficient; confirm the bounded amount before continuing the original order.".into();
            return error;
        }
    }
    if error
        .downcast_ref::<CodedError>()
        .is_some_and(|coded| coded.code == "approval_verification_unsupported")
    {
        return error;
    }
    anyhow!("newsliquid_signer_failed; inspect the original operation and wallet state")
}

fn child_result(code: Option<i32>, mut body: Value) -> Result<Value> {
    if code == Some(2) && body["confirming"] == true {
        let scene = body["scene"].as_str();
        if scene.is_some() {
            return Err(CodedError::new("approval_verification_unsupported",None,"This service operation cannot continue this wallet verification scene. No service payment was submitted; do not use --force to bypass it.").with_data(json!({"scene":scene,"next":body["next"]})).into());
        }
        return Err(output::CliConfirming {
            message: body["message"]
                .as_str()
                .unwrap_or("Wallet confirmation required")
                .into(),
            next: body["next"].as_str().unwrap_or("").into(),
            scene: None,
        }
        .into());
    }
    if !matches!(code, Some(0)) || body["ok"] != true {
        if body["errorCode"] == "approval_verification_unsupported" {
            return Err(CodedError::new("approval_verification_unsupported",None,"The internal payment signer requires unsupported email verification. No service payment submission was made.").into());
        }
        bail!("newsliquid_signer_failed; inspect the original operation and wallet state");
    }
    Ok(body["data"].take())
}
fn http_input(
    method: &str,
    path: String,
    wallet: Option<&str>,
    body: Option<Value>,
    headers: HashMap<String, String>,
) -> HttpInput {
    HttpInput {
        method: method.into(),
        path,
        wallet: wallet.map(str::to_owned),
        body,
        headers,
    }
}
fn data(result: &Value) -> Result<Value> {
    if !service_success(result) {
        return Err(CodedError::new(
            "newsliquid_service_rejected",
            None,
            "Service operation rejected; inspect the original response.",
        )
        .with_data(result.clone())
        .into());
    }
    Ok(result["data"]
        .get("data")
        .unwrap_or(&result["data"])
        .clone())
}
fn warning(error: &anyhow::Error) -> bool {
    error.downcast_ref::<output::CliConfirming>().is_some()
}
fn confirming(mut error: anyhow::Error, next: String) -> anyhow::Error {
    if let Some(w) = error.downcast_mut::<output::CliConfirming>() {
        w.next = next;
    }
    error
}
fn checked_wallet<I: Io>(io: &I, wallet: &str) -> Result<()> {
    if !io.wallet()?.eq_ignore_ascii_case(wallet) {
        bail!("newsliquid_wallet_changed");
    }
    Ok(())
}

fn signature(signed: &Value) -> Result<&str> {
    signed["signature"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("wallet_signature_missing"))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PaymentApproval {
    wallet: String,
    order_id: String,
    kind: PurchaseKind,
    quote: Value,
    accepted: Value,
    allowance: Value,
    expires_at: Value,
    auto_renew_terms: Value,
    requires_confirmation: bool,
}

async fn login<I: Io>(
    io: &mut I,
    warning_confirmed: bool,
    expected_wallet: Option<&str>,
) -> Result<Value> {
    let wallet = io.wallet()?;
    if warning_confirmed {
        checked_wallet(
            io,
            expected_wallet.ok_or_else(|| anyhow!("wallet_warning_requires_original_wallet"))?,
        )?;
    }
    if let Some(login) = load_login(&wallet)? {
        if expiry(&login.expires_at)? > chrono::Utc::now().timestamp() {
            return Ok(json!({"wallet":wallet,"expiresAt":login.expires_at,"reused":true}));
        }
    }
    let challenge = data(
        &io.http(http_input(
            "POST",
            "/auth/challenge".into(),
            None,
            Some(json!({"wallet":wallet,"chainId":196})),
            HashMap::new(),
        ))
        .await?,
    )?;
    let message = challenge["message"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid_login_challenge"))?;
    if !message.contains("x402-okx.newsliquid.com")
        || !message.to_lowercase().contains(&wallet.to_lowercase())
        || expiry(&challenge["expiresAt"])? <= chrono::Utc::now().timestamp()
    {
        bail!("challenge_identity_or_expiry_mismatch");
    }
    let mut args = vec![
        "wallet",
        "sign-message",
        "--type",
        "personal",
        "--chain",
        "xlayer",
        "--from",
        &wallet,
        "--message",
        message,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if warning_confirmed {
        args.push("--force".into());
    }
    let signed = match io.child(args).await {
        Ok(v) => v,
        Err(e) if warning(&e) => return Err(confirming(e, format!("After user confirmation, run onchainos service call --service newsliquid --operation login --params '{}'", json!({"confirmWalletWarning":true,"wallet":wallet})))),
        Err(e) => return Err(e),
    };
    checked_wallet(io, &wallet)?;
    let body = data(
        &io.http(http_input(
            "POST",
            "/auth/login".into(),
            Some(&wallet),
            Some(json!({"challengeId":challenge["challengeId"],"signature":signature(&signed)?})),
            HashMap::new(),
        ))
        .await?,
    )?;
    if !body["wallet"]
        .as_str()
        .is_some_and(|w| w.eq_ignore_ascii_case(&wallet))
        || expiry(&body["expiresAt"])? <= chrono::Utc::now().timestamp()
    {
        bail!("login_identity_or_expiry_mismatch");
    }
    Ok(json!({"wallet":wallet,"expiresAt":body["expiresAt"],"reused":false}))
}

async fn create<I: Io>(io: &mut I, kind: PurchaseKind, product: u64, key: &str) -> Result<Value> {
    if !id_valid(key) {
        bail!("invalid_operation_key");
    }
    let wallet = io.wallet()?;
    // The provider's Idempotency-Key, retained by the Agent, identifies this creation attempt.
    io.http(http_input(
        "POST",
        "/billing/orders".into(),
        Some(&wallet),
        Some(json!({"kind":kind.name(),"productId":product})),
        HashMap::from([("Idempotency-Key".into(), key.into())]),
    ))
    .await
}

fn validate_terms(order: &Value, entry: &Value) -> Result<()> {
    let quote = &order["quote"];
    for field in ["network", "asset", "payTo", "amount"] {
        if quote.get(field).is_none() || entry[field] != quote[field] {
            bail!("payment_quote_mismatch:{field}");
        }
    }
    if quote["network"] != "eip155:196" {
        bail!("unsupported_payment_network");
    }
    if order["kind"] == "one_time" {
        if entry["scheme"] != "exact" {
            bail!("quota_package_requires_exact");
        }
        return Ok(());
    }
    let extra = &entry["extra"];
    let expected = if order["kind"] == "auto_renew" { 12 } else { 1 };
    if entry["scheme"] != "period"
        || quote["maxPeriods"] != expected
        || extra["maxPeriods"] != expected
    {
        bail!("payment_periods_mismatch");
    }
    // This service promises calendar-month billing. Unknown/short fixed periods
    // are rejected, never rewritten to manufacture a compatible quote.
    if extra["periodMode"] != 1 || extra["periodSec"] != 0 {
        bail!("monthly_cadence_required");
    }
    if extra["amountPerPeriod"] != quote["amount"] {
        bail!("payment_period_amount_mismatch");
    }
    let initial = &extra["initialCharge"];
    if order["kind"] == "subscription"
        && (initial["periodCount"] != 1 || initial["totalAmount"] != quote["amount"])
    {
        bail!("single_month_initial_charge_mismatch");
    }
    if order["kind"] == "auto_renew" {
        // The shared signer treats zero as an immediate start, not future renewal.
        let valid_start = quote["startAt"].as_i64().is_some_and(|start| {
            start > chrono::Utc::now().timestamp()
                && chrono::DateTime::from_timestamp(start, 0).is_some()
        });
        if !valid_start
            || quote["autoRenew"] != true
            || extra["startAt"] != quote["startAt"]
            || initial["periodCount"].as_u64().unwrap_or(0) != 0
            || initial["totalAmount"].as_str().unwrap_or("0") != "0"
        {
            bail!("renewal_start_or_initial_charge_mismatch");
        }
    }
    Ok(())
}
fn integer(value: &Value) -> Result<BigUint> {
    let s = value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 78 && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| anyhow!("invalid_minimal_amount"))?;
    let n =
        BigUint::parse_bytes(s.as_bytes(), 10).ok_or_else(|| anyhow!("invalid_minimal_amount"))?;
    if n.bits() > 256 {
        bail!("minimal_amount_out_of_range");
    }
    Ok(n)
}
async fn allowance<I: Io>(io: &mut I, entry: &Value, wallet: &str) -> Result<Value> {
    let asset = entry["asset"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid_payment_asset"))?;
    let status = io
        .child(
            vec![
                "payment",
                "subscription",
                "allowance-status",
                "--token",
                asset,
                "--chain",
                "xlayer",
                "--from",
                wallet,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
        .await?;
    for (declared, authority) in [
        ("subscription", "subscriptionContract"),
        ("permit2", "permit2Contract"),
    ] {
        let a = status[authority]
            .as_str()
            .ok_or_else(|| anyhow!("payment_contract_missing"))?;
        parse_recipient_addr(a, 196)?;
        if !entry["extra"]["contracts"][declared]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(a))
        {
            bail!("payment_contract_mismatch");
        }
    }
    let extra = &entry["extra"];
    let initial = &extra["initialCharge"];
    let count = extra["maxPeriods"]
        .as_u64()
        .ok_or_else(|| anyhow!("invalid_period_count"))?;
    let initial_count = initial["periodCount"].as_u64().unwrap_or(0);
    let initial_amount = if initial["totalAmount"].is_null() {
        BigUint::from(0u8)
    } else {
        integer(&initial["totalAmount"])?
    };
    let future = count
        .checked_sub(initial_count)
        .ok_or_else(|| anyhow!("invalid_initial_period_count"))?;
    let commitment = integer(&extra["amountPerPeriod"])? * BigUint::from(future) + initial_amount;
    let reserved = if status["reservedAmount"].is_null() {
        BigUint::from(0u8)
    } else {
        integer(&status["reservedAmount"])?
    };
    let cap = &reserved + &commitment;
    if commitment == BigUint::from(0u8) || cap.bits() > 160 {
        bail!("invalid_payment_commitment_or_cap");
    }
    let preview = json!({"permitAllowanceCapAmount":cap.to_string(),"newCommitmentAmount":commitment.to_string(),"reservedAmount":reserved.to_string(),"spender":status["subscriptionContract"],"permit2Contract":status["permit2Contract"]});
    if integer(&status["permit2Allowance"])? < cap {
        return Err(CodedError::new("permit2_allowance_insufficient",None,"Token allowance is insufficient; approve only the confirmed bounded amount, then prepare the original order.").with_data(json!({"stage":"before_signing","allowance":preview,"permit2Allowance":status["permit2Allowance"]})).into());
    }
    Ok(preview)
}
fn verify_signed(order: &Value, entry: &Value, signed: &Value, wallet: &str) -> Result<()> {
    let terms = &signed["payload"]["terms"];
    let permit = &signed["payload"]["permit"];
    let extra = &entry["extra"];
    for (field, expected) in [
        ("amountPerPeriod", extra["amountPerPeriod"].clone()),
        ("maxPeriods", extra["maxPeriods"].clone()),
        ("periodMode", extra["periodMode"].clone()),
        ("periodSec", extra["periodSec"].clone()),
        ("startAt", extra.get("startAt").cloned().unwrap_or(json!(0))),
        (
            "initialChargePeriods",
            extra["initialCharge"]
                .get("periodCount")
                .cloned()
                .unwrap_or(json!(0)),
        ),
        (
            "initialChargeAmount",
            extra["initialCharge"]
                .get("totalAmount")
                .cloned()
                .unwrap_or(json!("0")),
        ),
    ] {
        if terms[field] != expected {
            bail!("signed_subscription_terms_mismatch:{field}");
        }
    }
    for (actual, expected) in [
        (&terms["payer"], wallet),
        (&terms["merchant"], entry["payTo"].as_str().unwrap_or("")),
        (&terms["token"], entry["asset"].as_str().unwrap_or("")),
        (
            &permit["spender"],
            order["allowance"]["spender"].as_str().unwrap_or(""),
        ),
        (
            &permit["details"]["token"],
            entry["asset"].as_str().unwrap_or(""),
        ),
    ] {
        if !actual
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(expected))
        {
            bail!("signed_payment_identity_mismatch");
        }
    }
    if integer(&permit["details"]["amount"])?
        != integer(&order["allowance"]["permitAllowanceCapAmount"])?
    {
        bail!("signed_allowance_cap_mismatch");
    }
    let start = extra["startAt"]
        .as_u64()
        .unwrap_or(0)
        .max(chrono::Utc::now().timestamp() as u64);
    let date = chrono::DateTime::from_timestamp(start as i64, 0)
        .ok_or_else(|| anyhow!("invalid_period_start"))?;
    let months = extra["maxPeriods"]
        .as_u64()
        .ok_or_else(|| anyhow!("invalid_period_count"))?;
    let end = date
        .checked_add_months(chrono::Months::new(months as u32))
        .ok_or_else(|| anyhow!("invalid_period_window"))?
        .timestamp() as u64;
    let expiration = permit["details"]["expiration"]
        .as_u64()
        .or_else(|| {
            permit["details"]["expiration"]
                .as_str()
                .and_then(|s| s.parse().ok())
        })
        .ok_or_else(|| anyhow!("invalid_permit_expiration"))?;
    if expiration < end {
        bail!("signed_permit_window_too_short");
    }
    Ok(())
}

async fn read_order<I: Io>(io: &mut I, wallet: &str, id: &str) -> Result<Value> {
    if !id_valid(id) {
        bail!("invalid_order_id");
    }
    let order = data(
        &io.http(http_input(
            "GET",
            format!("/billing/orders/{id}"),
            Some(wallet),
            None,
            HashMap::new(),
        ))
        .await?,
    )?;
    if order["orderId"] != id {
        bail!("service_order_identity_mismatch");
    }
    if order.get("wallet").is_some()
        && !order["wallet"]
            .as_str()
            .is_some_and(|w| w.eq_ignore_ascii_case(wallet))
    {
        bail!("service_order_wallet_mismatch");
    }
    Ok(order)
}

fn payable(order: &Value) -> Result<()> {
    // Unknown, processing, paid and authorized states must never cause a fresh signature.
    if order["paymentStatus"] != "unpaid" {
        bail!("order_not_payable; query the same order instead of signing again");
    }
    if expiry(&order["expiresAt"])? <= chrono::Utc::now().timestamp() {
        bail!("order_expired");
    }
    Ok(())
}

async fn prepare_payment<I: Io>(io: &mut I, id: &str, kind: PurchaseKind) -> Result<Value> {
    Ok(prepare_with_challenge(io, id, kind).await?.0)
}

async fn prepare_with_challenge<I: Io>(
    io: &mut I,
    id: &str,
    kind: PurchaseKind,
) -> Result<(Value, Option<String>)> {
    let wallet = io.wallet()?;
    let mut order = read_order(io, &wallet, id).await?;
    payable(&order)?;
    if order.get("kind").is_some() && order["kind"] != kind.name() {
        bail!("service_order_kind_mismatch");
    }
    order["kind"] = json!(kind.name());
    let r = io
        .http(http_input(
            "POST",
            format!("/billing/orders/{id}/pay"),
            Some(&wallet),
            None,
            HashMap::new(),
        ))
        .await?;
    if r["httpStatus"] != 402 {
        return Ok((r, None));
    }
    let encoded = r["paymentRequired"]
        .as_str()
        .ok_or_else(|| anyhow!("original_order_challenge_missing"))?;
    let challenge: Value = serde_json::from_slice(
        &B64.decode(encoded)
            .map_err(|_| anyhow!("invalid_payment_challenge"))?,
    )
    .map_err(|_| anyhow!("invalid_payment_challenge"))?;
    let entries = challenge["accepts"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(|| anyhow!("ambiguous_payment_offer"))?;
    validate_terms(&order, &entries[0])?;
    if entries[0]["scheme"] == "exact"
        && challenge["resource"]["url"] != format!("{BASE}/billing/orders/{id}/pay")
    {
        bail!("payment_resource_mismatch_or_missing");
    }
    let allowance = if entries[0]["scheme"] == "period" {
        allowance(io, &entries[0], &wallet).await?
    } else {
        Value::Null
    };
    let approval = PaymentApproval {
        wallet: wallet.clone(),
        order_id: id.into(),
        kind,
        quote: order["quote"].clone(),
        accepted: entries[0].clone(),
        allowance: allowance.clone(),
        expires_at: order["expiresAt"].clone(),
        auto_renew_terms: order["autoRenewTerms"].clone(),
        requires_confirmation: true,
    };
    checked_wallet(io, &wallet)?;
    Ok((serde_json::to_value(approval)?, Some(encoded.to_owned())))
}

fn outcome_unknown(error: anyhow::Error, id: &str, stage: &str) -> anyhow::Error {
    if error.downcast_ref::<CodedError>().is_some_and(|e| {
        e.code == "permit2_allowance_insufficient"
            && e.data
                .as_ref()
                .is_some_and(|d| d["stage"] == "before_signing")
    }) {
        return error;
    }
    CodedError::new("newsliquid_operation_outcome_unknown", None,
        "Stop this attempt. Query the same order or subscription; do not automatically sign or submit again.")
        .with_data(json!({"id":id,"stage":stage,"retry":"query-only"})).into()
}

async fn submit_payment<I: Io>(io: &mut I, id: &str, approval: PaymentApproval) -> Result<Value> {
    checked_wallet(io, &approval.wallet)?;
    if !approval.requires_confirmation
        || approval.order_id != id
        || expiry(&approval.expires_at)? <= chrono::Utc::now().timestamp()
    {
        bail!("payment_approval_identity_or_expiry_mismatch");
    }
    // Always fetch the order and challenge again. Never sign caller-supplied payloads.
    let (fresh, encoded) = prepare_with_challenge(io, id, approval.kind).await?;
    let current: PaymentApproval =
        serde_json::from_value(fresh).map_err(|_| anyhow!("order_no_longer_requires_payment"))?;
    if serde_json::to_value(&current)? != serde_json::to_value(&approval)? {
        bail!("payment_terms_changed; show fresh terms and obtain confirmation again");
    }
    let entry = &current.accepted;
    checked_wallet(io, &current.wallet)?;
    let signed = if entry["scheme"] == "period" {
        io.period_payment(
            entry,
            &current.wallet,
            &format!("{BASE}/billing/orders/{id}/pay"),
        )
        .await
    } else {
        let payload = encoded
            .as_deref()
            .ok_or_else(|| anyhow!("fresh_payment_challenge_missing"))?;
        io.exact_payment(payload, &current.wallet).await
    }
    .map_err(|e| outcome_unknown(e, id, "signing"))?;
    let order = json!({"allowance":current.allowance});
    let header = if entry["scheme"] == "period" {
        verify_signed(&order, entry, &signed, &current.wallet)
            .map_err(|e| outcome_unknown(e, id, "signing"))?;
        if signed["paymentHeaderName"] != "PAYMENT-SIGNATURE" {
            return Err(outcome_unknown(
                anyhow!("unexpected_payment_header"),
                id,
                "signing",
            ));
        }
        signed["paymentHeaderValue"].as_str()
    } else {
        verify_exact(&signed, &current.wallet).map_err(|e| outcome_unknown(e, id, "signing"))?;
        signed["authorization_header"].as_str()
    }
    .ok_or_else(|| outcome_unknown(anyhow!("signed_payment_header_missing"), id, "signing"))?;
    checked_wallet(io, &current.wallet).map_err(|e| outcome_unknown(e, id, "signing"))?;
    io.http(http_input(
        "POST",
        format!("/billing/orders/{id}/pay"),
        Some(&current.wallet),
        None,
        HashMap::from([("PAYMENT-SIGNATURE".into(), header.into())]),
    ))
    .await
    .map_err(|e| outcome_unknown(e, id, "submitting"))
}

fn verify_exact(signed: &Value, wallet: &str) -> Result<()> {
    if signed["scheme"] != "exact"
        || signed["header_name"] != "PAYMENT-SIGNATURE"
        || !signed["wallet"]
            .as_str()
            .is_some_and(|w| w.eq_ignore_ascii_case(wallet))
    {
        bail!("signed_payment_identity_or_header_mismatch");
    }
    Ok(())
}

async fn cancel<I: Io>(
    io: &mut I,
    id: &str,
    warning_confirmed: bool,
    expected_wallet: Option<&str>,
) -> Result<Value> {
    if !valid_sub_id(id) {
        bail!("invalid_sub_id");
    }
    let wallet = io.wallet()?;
    if warning_confirmed {
        checked_wallet(
            io,
            expected_wallet.ok_or_else(|| anyhow!("wallet_warning_requires_original_wallet"))?,
        )?;
    }
    let quota = data(
        &io.http(http_input(
            "GET",
            "/billing/quota".into(),
            Some(&wallet),
            None,
            HashMap::new(),
        ))
        .await?,
    )?;
    let renewal = &quota["autoRenew"];
    if !renewal["subId"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case(id))
    {
        bail!("renewal_not_owned_by_current_wallet");
    }
    if renewal["status"] != "active" {
        bail!("renewal_not_active; query current subscription state");
    }
    let subscriptions = data(
        &io.http(http_input(
            "GET",
            "/billing/subscriptions".into(),
            Some(&wallet),
            None,
            HashMap::new(),
        ))
        .await?,
    )?;
    let rows = subscriptions
        .as_array()
        .or_else(|| subscriptions["subscriptions"].as_array())
        .ok_or_else(|| anyhow!("invalid_subscriptions_response"))?;
    let record = rows
        .iter()
        .find(|r| {
            r["subId"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(id))
        })
        .ok_or_else(|| anyhow!("subscription_quote_missing"))?;
    let authority = io
        .child(
            vec![
                "payment",
                "subscription",
                "allowance-status",
                "--token",
                record["quote"]["asset"]
                    .as_str()
                    .ok_or_else(|| anyhow!("subscription_asset_missing"))?,
                "--chain",
                "xlayer",
                "--from",
                &wallet,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
        .await?;
    let contract = authority["subscriptionContract"]
        .as_str()
        .ok_or_else(|| anyhow!("subscription_contract_missing"))?;
    let typed = cancel_request(id, contract)?;
    let mut args = vec![
        "wallet".into(),
        "sign-message".into(),
        "--type".into(),
        "eip712".into(),
        "--chain".into(),
        "xlayer".into(),
        "--from".into(),
        wallet.clone(),
        "--message".into(),
        typed.to_string(),
    ];
    if warning_confirmed {
        args.push("--force".into());
    }
    let signed = match io.child(args).await {
        Ok(v)=>v,
        Err(e) if warning(&e)=>return Err(confirming(e,format!("After user confirmation, run onchainos service call --service newsliquid --operation cancel-renewal --id {id} --confirmed --params '{}'",json!({"confirmWalletWarning":true,"wallet":wallet})))),
        Err(e)=>return Err(outcome_unknown(e,id,"signing")),
    };
    let proof = signature(&signed).map_err(|e| outcome_unknown(e, id, "signing"))?;
    checked_wallet(io, &wallet).map_err(|e| outcome_unknown(e, id, "signing"))?;
    let mut auth = typed["message"].clone();
    auth["signature"] = json!(proof);
    io.http(http_input(
        "POST",
        format!("/billing/subscriptions/{id}/cancel"),
        Some(&wallet),
        Some(json!({"cancelAuth":auth})),
        HashMap::new(),
    ))
    .await
    .map_err(|e| outcome_unknown(e, id, "submitting"))
}

pub(super) async fn request(
    operation: &str,
    id: Option<String>,
    params: Value,
    confirmed: bool,
) -> Result<Value> {
    if matches!(
        operation,
        "auth-challenge"
            | "auth-login"
            | "billing-orders"
            | "billing-orders-id-pay"
            | "billing-subscriptions-id-cancel"
    ) {
        bail!("use_dedicated_service_command");
    }
    if !matches!(
        operation,
        "twitter-watch-add" | "twitter-watch-delete" | "billing-orders-id-reconcile"
    ) {
        return query(operation, id, params).await;
    }
    if matches!(operation, "twitter-watch-add" | "twitter-watch-delete") && !confirmed {
        bail!("explicit_monitor_confirmation_required");
    }
    if !params.is_object() {
        bail!("invalid_request_params");
    }
    let entry: Value = serde_json::from_str(include_str!("endpoints.json"))?;
    let mut path = entry[operation]["path"]
        .as_str()
        .ok_or_else(|| anyhow!("unknown_operation"))?
        .to_owned();
    if path.contains(":id") {
        let id = id
            .filter(|id| id_valid(id))
            .ok_or_else(|| anyhow!("invalid_order_id"))?;
        path = path.replace(":id", &id);
    }
    let wallet = current_wallet()?;
    let body = if operation == "billing-orders-id-reconcile" {
        if params.as_object().unwrap().is_empty() {
            None
        } else {
            bail!("reconcile_has_no_body")
        }
    } else {
        Some(params)
    };
    request_http(http_input(
        "POST",
        path,
        Some(&wallet),
        body,
        HashMap::new(),
    ))
    .await
}
pub(super) async fn run(command: Command) -> Result<Value> {
    match &command {
        Command::Pay {
            confirmed: false, ..
        } => bail!("explicit_payment_confirmation_required"),
        Command::Cancel {
            confirmed: false, ..
        } => bail!("explicit_cancellation_confirmation_required"),
        _ => {}
    }
    let mut io = Live;
    if !matches!(command, Command::Login { .. })
        && !status()?["authenticated"].as_bool().unwrap_or(false)
    {
        bail!("newsliquid_login_required");
    }
    match command {
        Command::Login { warning, wallet } => login(&mut io, warning, wallet.as_deref()).await,
        Command::Create { kind, product, key } => create(&mut io, kind, product, &key).await,
        Command::Prepare { id, kind } => prepare_payment(&mut io, &id, kind).await,
        Command::Pay { id, approval, .. } => submit_payment(&mut io, &id, approval).await,
        Command::Cancel {
            id,
            warning,
            wallet,
            ..
        } => cancel(&mut io, &id, warning, wallet.as_deref()).await,
    }
}

#[cfg(test)]
mod tests;
