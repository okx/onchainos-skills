//! Stateless checks for native service responses. Never requests, signs or stores anything.
use alloy_primitives::U256;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};

use crate::commands::payment::addr::parse_recipient_addr;
use crate::commands::sink::CodedError;
use crate::payment::subscription::sign::commitment_cap;

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("payment_field_missing:{field}"))
}

fn amount(value: &Value) -> Result<U256> {
    let raw = value
        .as_str()
        .ok_or_else(|| anyhow!("invalid_payment_integer"))?;
    crate::validators::validate_non_negative_integer(raw, "payment amount")?;
    U256::from_str_radix(raw, 10).map_err(|_| anyhow!("payment_integer_out_of_range"))
}

fn number(value: &Value, max: u64) -> Result<u64> {
    value
        .as_u64()
        .filter(|n| *n <= max)
        .ok_or_else(|| anyhow!("invalid_payment_number"))
}

fn address(value: &str) -> Result<String> {
    Ok(parse_recipient_addr(value, 196)?.0.to_ascii_lowercase())
}

fn same_address(actual: &Value, expected: &str) -> Result<()> {
    if address(
        actual
            .as_str()
            .ok_or_else(|| anyhow!("payment_address_missing"))?,
    )? != address(expected)?
    {
        return Err(CodedError::new(
            "payment_identity_mismatch",
            None,
            "payment_identity_mismatch",
        )
        .into());
    }
    Ok(())
}

fn require(condition: bool, message: &'static str) -> Result<()> {
    if !condition {
        return Err(CodedError::new(message, None, message).into());
    }
    Ok(())
}

fn decode(raw: &str) -> Result<Value> {
    let engine = base64::engine::general_purpose::GeneralPurpose::new(
        &base64::alphabet::URL_SAFE,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    let bytes = B64
        .decode(raw)
        .or_else(|_| engine.decode(raw))
        .map_err(|_| anyhow!("invalid_payment_header"))?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("invalid_payment_header"))
}

fn snapshot(input: &Value, wallet: &str, now: i64) -> Result<Value> {
    let order = &input["order"];
    let quote = &order["quote"];
    let id = text(order, "orderId")?;
    require(
        !id.is_empty()
            && id.len() <= 160
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "invalid_order_id",
    )?;
    require(order["paymentStatus"] == "unpaid", "order_not_payable")?;
    require(super::expiry(&order["expiresAt"])? > now, "order_expired")?;
    if order.get("wallet").is_some() {
        same_address(&order["wallet"], wallet)?;
    }
    let kind = text(input, "kind")?;
    if order.get("kind").is_some() {
        require(order["kind"] == kind, "order_kind_mismatch")?;
    }
    let challenge = &input["challenge"];
    let url = format!("{}/billing/orders/{id}/pay", super::BASE);
    require(
        challenge["resource"]["url"] == url,
        "payment_resource_mismatch",
    )?;
    let entries = challenge["accepts"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(|| anyhow!("ambiguous_payment_offer"))?;
    let entry = &entries[0];
    for field in ["network", "asset", "payTo", "amount"] {
        require(
            entry.get(field).is_some() && entry[field] == quote[field],
            "payment_quote_mismatch",
        )?;
    }
    require(
        quote["network"] == "eip155:196",
        "unsupported_payment_network",
    )?;
    address(text(quote, "asset")?)?;
    address(text(quote, "payTo")?)?;
    require(
        amount(&quote["amount"])? > U256::ZERO,
        "invalid_payment_amount",
    )?;
    let mut allowance = Value::Null;
    let renewal_terms = order
        .get("autoRenewTerms")
        .or_else(|| quote.get("autoRenewTerms"))
        .cloned()
        .unwrap_or(Value::Null);
    if kind == "one_time" {
        require(entry["scheme"] == "exact", "quota_package_requires_exact")?;
    } else {
        require(
            matches!(kind, "subscription" | "auto_renew") && entry["scheme"] == "period",
            "period_required",
        )?;
        let extra = &entry["extra"];
        let count = if kind == "auto_renew" { 12 } else { 1 };
        require(
            number(&extra["maxPeriods"], u32::MAX as u64)? == count
                && number(&quote["maxPeriods"], u32::MAX as u64)? == count,
            "payment_periods_mismatch",
        )?;
        require(
            number(&extra["periodMode"], 255)? == 1 && number(&extra["periodSec"], u64::MAX)? == 0,
            "calendar_month_required",
        )?;
        require(
            extra["amountPerPeriod"] == quote["amount"],
            "period_amount_mismatch",
        )?;
        address(text(extra, "facilitator")?)?;
        number(&extra["plan"]["tier"], 255)?;
        let initial = &extra["initialCharge"];
        let first = number(&initial["periodCount"], u32::MAX as u64)?;
        if kind == "auto_renew" {
            let start = number(&quote["startAt"], i64::MAX as u64)? as i64;
            require(
                start > now + 1200
                    && super::expiry(&input["planServiceEnd"])? == start
                    && extra["startAt"] == quote["startAt"],
                "renewal_start_mismatch",
            )?;
            require(
                quote["autoRenew"] == true
                    && first == 0
                    && amount(&initial["totalAmount"])? == U256::ZERO,
                "renewal_initial_charge_mismatch",
            )?;
            text(&renewal_terms, "text")?;
            if order.get("autoRenewTerms").is_some() && quote.get("autoRenewTerms").is_some() {
                require(
                    order["autoRenewTerms"] == quote["autoRenewTerms"],
                    "renewal_terms_mismatch",
                )?;
            }
        } else {
            require(
                first == 1 && initial["totalAmount"] == quote["amount"],
                "single_month_initial_charge_mismatch",
            )?;
        }
        let start = number(extra.get("startAt").unwrap_or(&json!(0)), i64::MAX as u64)? as i64;
        require(
            start == 0 || chrono::DateTime::from_timestamp(start, 0).is_some(),
            "invalid_period_start",
        )?;
        if kind == "subscription" {
            require(
                quote.get("startAt").cloned().unwrap_or(json!(0)) == json!(start),
                "single_month_start_mismatch",
            )?;
        }
        let status = &input["allowance"];
        same_address(
            &extra["contracts"]["subscription"],
            text(status, "subscriptionContract")?,
        )?;
        same_address(
            &extra["contracts"]["permit2"],
            text(status, "permit2Contract")?,
        )?;
        // Use the same parser and arithmetic as the signer, with range checks before narrowing.
        amount(&status["reservedAmount"])?;
        amount(&initial["totalAmount"])?;
        let (commitment, cap) = commitment_cap(
            text(initial, "totalAmount")?,
            count as u32,
            first as u32,
            text(extra, "amountPerPeriod")?,
            text(status, "reservedAmount")?,
        )?;
        let available = amount(&status["permit2Allowance"])?;
        if available < cap {
            return Err(CodedError::new("permit2_allowance_insufficient", None, "Confirm a bounded allowance before retrying the original order.")
                .with_data(json!({"stage":"before_signing","asset":entry["asset"],"requiredAmount":cap.to_string(),"availableAmount":available.to_string(),"permit2Contract":status["permit2Contract"]})).into());
        }
        allowance = json!({"cap":cap.to_string(),"commitment":commitment.to_string(),"reservedAmount":status["reservedAmount"],"spender":status["subscriptionContract"],"permit2Contract":status["permit2Contract"]});
    }
    Ok(
        json!({"wallet":address(wallet)?,"orderId":id,"kind":kind,"quote":quote,"accepted":entry,"resource":challenge["resource"],"expiresAt":order["expiresAt"],"autoRenewTerms":renewal_terms,"allowance":allowance}),
    )
}

fn verify_signed(approval: &Value, signed: &Value, now: i64) -> Result<()> {
    let entry = &approval["accepted"];
    let wallet = text(approval, "wallet")?;
    let exact = entry["scheme"] == "exact";
    let header = if exact {
        "authorization_header"
    } else {
        "paymentHeaderValue"
    };
    let header_name = if exact {
        "header_name"
    } else {
        "paymentHeaderName"
    };
    require(
        signed[header_name] == "PAYMENT-SIGNATURE",
        "unexpected_payment_header",
    )?;
    let body = decode(text(signed, header)?)?;
    require(
        body["accepted"] == *entry && body["resource"]["url"] == approval["resource"]["url"],
        "signed_offer_mismatch",
    )?;
    if exact {
        require(signed["scheme"] == "exact", "signed_scheme_mismatch")?;
        same_address(&signed["wallet"], wallet)?;
        if let Some(auth) = body["payload"].get("authorization") {
            same_address(&auth["from"], wallet)?;
            same_address(&auth["to"], text(entry, "payTo")?)?;
            require(
                amount(&auth["value"])? == amount(&entry["amount"])?,
                "signed_amount_mismatch",
            )?;
        } else {
            let auth = &body["payload"]["permit2Authorization"];
            same_address(&auth["from"], wallet)?;
            same_address(&auth["permitted"]["token"], text(entry, "asset")?)?;
            same_address(&auth["witness"]["to"], text(entry, "payTo")?)?;
            require(
                amount(&auth["permitted"]["amount"])? == amount(&entry["amount"])?,
                "signed_amount_mismatch",
            )?;
        }
        return Ok(());
    }
    let payload = &signed["payload"];
    let terms = &payload["terms"];
    let permit = &payload["permit"];
    let extra = &entry["extra"];
    for field in ["amountPerPeriod", "maxPeriods", "periodMode", "periodSec"] {
        require(terms[field] == extra[field], "signed_terms_mismatch")?;
    }
    require(
        terms["startAt"] == extra.get("startAt").cloned().unwrap_or(json!(0))
            && terms["initialChargePeriods"] == extra["initialCharge"]["periodCount"]
            && terms["initialChargeAmount"] == extra["initialCharge"]["totalAmount"],
        "signed_initial_charge_mismatch",
    )?;
    same_address(&terms["payer"], wallet)?;
    same_address(&terms["merchant"], text(entry, "payTo")?)?;
    same_address(&terms["token"], text(entry, "asset")?)?;
    same_address(&terms["facilitator"], text(extra, "facilitator")?)?;
    same_address(&permit["details"]["token"], text(entry, "asset")?)?;
    same_address(&permit["spender"], text(&approval["allowance"], "spender")?)?;
    require(
        amount(&permit["details"]["amount"])? == amount(&approval["allowance"]["cap"])?
            && terms["planTier"] == extra["plan"]["tier"]
            && terms["changeEffectiveAt"] == 0
            && terms["changeFromSubId"] == format!("0x{}", "0".repeat(64)),
        "signed_cap_or_plan_mismatch",
    )?;
    let start = extra["startAt"].as_i64().unwrap_or(0).max(now);
    let end = chrono::DateTime::from_timestamp(start, 0)
        .and_then(|date| {
            date.checked_add_months(chrono::Months::new(
                extra["maxPeriods"].as_u64().unwrap_or(0) as u32,
            ))
        })
        .ok_or_else(|| anyhow!("invalid_service_window"))?
        .timestamp();
    require(
        super::expiry(&permit["details"]["expiration"])? >= end,
        "signed_permit_window_too_short",
    )?;
    require(
        body["payload"]["terms"] == *terms && body["payload"]["permitSingle"] == *permit,
        "signed_header_mismatch",
    )
}

pub(crate) fn validate_payment(input: &Value, wallet: &str, stage: &str) -> Result<Value> {
    validate_at(input, wallet, stage, chrono::Utc::now().timestamp()).map_err(|error| {
        if error.downcast_ref::<CodedError>().is_some() {
            error
        } else {
            anyhow!("payment_validation_failed; missing or invalid order/challenge/proof fields")
        }
    })
}

fn validate_at(input: &Value, wallet: &str, stage: &str, now: i64) -> Result<Value> {
    let current = snapshot(input, wallet, now)?;
    if stage == "prepare" {
        return Ok(current);
    }
    let approved = input["approved"].get("data").unwrap_or(&input["approved"]);
    require(*approved == current, "payment_terms_changed")?;
    if stage == "after-sign" {
        verify_signed(&current, &input["signed"], now)?;
    } else {
        require(stage == "before-sign", "invalid_payment_validation_stage")?;
        // Return only the exact unsigned material just checked. Callers need not
        // independently select or reconstruct an accepts entry for the signer.
        return Ok(
            json!({"valid":true,"orderId":current["orderId"],"accepted":current["accepted"],
            "payload":B64.encode(serde_json::to_vec(&input["challenge"])?),"cap":current["allowance"]["cap"],"url":current["resource"]["url"]}),
        );
    }
    Ok(json!({"valid":true,"orderId":current["orderId"]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: i64 = 1_800_000_000;
    const WALLET: &str = "0x1111111111111111111111111111111111111111";
    const ASSET: &str = "0x2222222222222222222222222222222222222222";
    const MERCHANT: &str = "0x3333333333333333333333333333333333333333";
    const CONTRACT: &str = "0x4444444444444444444444444444444444444444";

    fn fixture(period: bool) -> Value {
        let quote = json!({"network":"eip155:196","asset":ASSET,"payTo":MERCHANT,"amount":"1000000","maxPeriods":1});
        let mut entry = quote.clone();
        entry["scheme"] = json!(if period { "period" } else { "exact" });
        entry["extra"] = json!({"maxPeriods":1,"periodMode":1,"periodSec":0,"amountPerPeriod":"1000000","startAt":0,
            "initialCharge":{"periodCount":1,"totalAmount":"1000000"},"facilitator":CONTRACT,"plan":{"tier":1,"id":"monthly"},
            "contracts":{"subscription":CONTRACT,"permit2":ASSET}});
        json!({"kind":if period { "subscription" } else { "one_time" },
            "order":{"orderId":"order-1","wallet":WALLET,"paymentStatus":"unpaid","expiresAt":NOW+3600,"quote":quote},
            "challenge":{"resource":{"url":format!("{}/billing/orders/order-1/pay",super::super::BASE)},"accepts":[entry]},
            "allowance":{"reservedAmount":"0","permit2Allowance":"999999999999","subscriptionContract":CONTRACT,"permit2Contract":ASSET,"tokenBalance":"0"}})
    }

    #[test]
    fn exact_checks_candidates_wallet_order_and_fresh_terms() {
        let input = fixture(false);
        let approved = validate_at(&input, WALLET, "prepare", NOW).unwrap();
        assert_eq!(approved["wallet"], WALLET);
        let mut fresh = input.clone();
        fresh["approved"] = json!({"ok":true,"data":approved});
        assert!(validate_at(&fresh, WALLET, "before-sign", NOW).is_ok());
        let mut multiple = input.clone();
        multiple["challenge"]["accepts"]
            .as_array_mut()
            .unwrap()
            .push(input["challenge"]["accepts"][0].clone());
        assert!(validate_at(&multiple, WALLET, "prepare", NOW).is_err());
        assert!(validate_at(&input, MERCHANT, "prepare", NOW).is_err());
        assert!(validate_at(&input, WALLET, "prepare", NOW + 4000).is_err());
        fresh["order"]["quote"]["amount"] = json!("2000000");
        fresh["challenge"]["accepts"][0]["amount"] = json!("2000000");
        assert!(validate_at(&fresh, WALLET, "before-sign", NOW).is_err());
    }

    #[test]
    fn cap_reuses_signer_arithmetic_and_returns_bounded_approval_guidance() {
        let mut input = fixture(true);
        input["allowance"]["reservedAmount"] = json!("1267650600228229401496703205376");
        input["allowance"]["permit2Allowance"] =
            json!("1461501637330902918203684832716283019655932542975");
        let approved = validate_at(&input, WALLET, "prepare", NOW).unwrap();
        assert_eq!(
            approved["allowance"]["cap"],
            "1267650600228229401496704205376"
        );
        input["allowance"]["permit2Allowance"] = json!("0");
        let err = validate_at(&input, WALLET, "prepare", NOW).unwrap_err();
        let data = err
            .downcast_ref::<CodedError>()
            .unwrap()
            .data
            .as_ref()
            .unwrap();
        assert_eq!(data["stage"], "before_signing");
        assert_eq!(data["requiredAmount"], approved["allowance"]["cap"]);
    }

    #[test]
    fn malformed_missing_and_overflow_values_stop_before_signing() {
        for value in [
            json!("1.2"),
            json!("-1"),
            json!(true),
            json!(""),
            json!(format!("{}0", U256::MAX)),
        ] {
            let mut input = fixture(true);
            input["order"]["quote"]["amount"] = value.clone();
            input["challenge"]["accepts"][0]["amount"] = value;
            assert!(validate_at(&input, WALLET, "prepare", NOW).is_err());
        }
        let mut input = fixture(true);
        input["challenge"]["accepts"][0]["extra"]["maxPeriods"] = json!(u32::MAX as u64 + 2);
        assert!(validate_at(&input, WALLET, "prepare", NOW).is_err());
        input["order"].as_object_mut().unwrap().remove("quote");
        assert!(validate_at(&input, WALLET, "prepare", NOW).is_err());
    }

    #[test]
    fn renewal_start_must_match_the_current_plan_expiry() {
        let mut input = fixture(true);
        input["kind"] = json!("auto_renew");
        input["planServiceEnd"] = json!(NOW + 86400);
        input["order"]["quote"]["maxPeriods"] = json!(12);
        input["order"]["quote"]["startAt"] = json!(NOW + 86400);
        input["order"]["quote"]["autoRenew"] = json!(true);
        input["order"]["autoRenewTerms"] = json!({"text":"Twelve monthly periods"});
        let extra = &mut input["challenge"]["accepts"][0]["extra"];
        extra["maxPeriods"] = json!(12);
        extra["startAt"] = json!(NOW + 86400);
        extra["initialCharge"] = json!({"periodCount":0,"totalAmount":"0"});
        assert!(validate_at(&input, WALLET, "prepare", NOW).is_ok());
        input["planServiceEnd"] = json!(NOW + 172800);
        assert!(validate_at(&input, WALLET, "prepare", NOW).is_err());
    }

    #[test]
    fn signed_identity_is_checked_without_returning_proof() {
        let mut input = fixture(false);
        input["approved"] = validate_at(&input, WALLET, "prepare", NOW).unwrap();
        let header=B64.encode(serde_json::to_vec(&json!({"accepted":input["challenge"]["accepts"][0],"resource":input["challenge"]["resource"],"payload":{"signature":"FAKE_SIGNATURE_CANARY","authorization":{"from":WALLET,"to":MERCHANT,"value":"1000000"}}})).unwrap());
        input["signed"] = json!({"scheme":"exact","wallet":WALLET,"header_name":"PAYMENT-SIGNATURE","authorization_header":header});
        let out = validate_at(&input, WALLET, "after-sign", NOW).unwrap();
        assert!(!out.to_string().contains("FAKE_SIGNATURE_CANARY"));
        assert!(out.get("signed").is_none());
        input["signed"]["wallet"] = json!(MERCHANT);
        assert!(validate_at(&input, WALLET, "after-sign", NOW).is_err());
    }

    #[test]
    fn period_native_proof_and_window_are_checked_without_narrowing_timestamps() {
        let mut input = fixture(true);
        input["approved"] = validate_at(&input, WALLET, "prepare", NOW).unwrap();
        let terms = json!({"payer":WALLET,"merchant":MERCHANT,"token":ASSET,"facilitator":CONTRACT,
            "amountPerPeriod":"1000000","maxPeriods":1,"periodMode":1,"periodSec":0,"startAt":0,
            "initialChargePeriods":1,"initialChargeAmount":"1000000","planTier":1,
            "changeEffectiveAt":0,"changeFromSubId":format!("0x{}","0".repeat(64))});
        let permit = json!({"spender":CONTRACT,"details":{"token":ASSET,"amount":"1000000","expiration":NOW+40*86400}});
        let header = json!({"accepted":input["challenge"]["accepts"][0],"resource":input["challenge"]["resource"],
            "payload":{"terms":terms,"permitSingle":permit,"termsSignature":"FAKE_TERMS_SIGNATURE","permitSingleSignature":"FAKE_PERMIT_SIGNATURE"}});
        input["signed"] = json!({"paymentHeaderName":"PAYMENT-SIGNATURE","paymentHeaderValue":B64.encode(serde_json::to_vec(&header).unwrap()),
            "payload":{"terms":terms,"permit":permit}});
        assert!(validate_at(&input, WALLET, "after-sign", NOW).is_ok());
        for field in ["cap", "terms", "expiration"] {
            let mut changed = input.clone();
            match field {
                "cap" => {
                    changed["signed"]["payload"]["permit"]["details"]["amount"] = json!("999999")
                }
                "terms" => changed["signed"]["payload"]["terms"]["periodSec"] = json!(1),
                _ => changed["signed"]["payload"]["permit"]["details"]["expiration"] = json!(NOW),
            }
            assert!(validate_at(&changed, WALLET, "after-sign", NOW).is_err());
        }
        for start in [u64::MAX, i64::MAX as u64, (NOW + 86400) as u64] {
            let mut changed = fixture(true);
            changed["challenge"]["accepts"][0]["extra"]["startAt"] = json!(start);
            assert!(validate_at(&changed, WALLET, "prepare", NOW).is_err());
        }
    }
}
