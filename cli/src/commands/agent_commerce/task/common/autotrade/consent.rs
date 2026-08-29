//! Buyer-side auto-trade **consent** record (product revision 2026-07-17).
//!
//! One file per `jobId` at `<onchainos_home>/autotrade/consent/<jobId>.json`,
//! written **whole** in one shot. The consent record is the client-side gate that
//! sits AFTER the exact-Active backend subscription gate. The model-driven session
//! uses this policy when handling an inbound signal:
//!
//! - no record (first time) ⇒ ask the user a three-way decision, then remember it
//! - `Auto` + amount ≤ `capU` ⇒ auto-execute (execution card)
//! - `Auto` + amount > `capU` ⇒ re-ask (over-cap)
//! - `Manual` ⇒ do not auto-execute; ask a bounded one-shot execute/skip decision
//! - `Decline` ⇒ the previous delivery was skipped; a later signal has no authorization
//!
//! Key granularity is **per-job** (product call, 2026-07-17): consent authorizes
//! THIS subscription, not the ASP as a whole (a subscription's `jobId` is stable
//! across renewals, so this does not re-prompt on renew). The per-trade `capU` is a
//! single global buy-side cap in quote-stablecoin units (PRD: USDT); sells are never cap-gated (L1,
//! WBW-13715 — a sell is naturally bounded by the position held).
//!
//! Supersedes the per-venue `grants.rs` cap model: the consent record is now the
//! single source of truth exposed through `autotrade-grant-check`, so the
//! Skill-selected execution tool enforces the same client-side cap.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::user_lang::Lang;
use super::amount::Decimal;
use super::grants::job_id_is_safe;
use super::trade_kit::TradeEnvironment;
use crate::commands::agent_commerce::task::common::config::SubscriptionTradePath;

/// The current consent-file schema version.
pub const CONSENT_VERSION: u32 = 5;

pub type DynamicConsentSettings = BTreeMap<String, Value>;

const MAX_DYNAMIC_SETTINGS_BYTES: usize = 16 * 1024;
const MAX_DYNAMIC_SETTING_COUNT: usize = 64;
const MAX_DYNAMIC_SETTING_KEY_LEN: usize = 64;
const MAX_DYNAMIC_SETTING_STRING_LEN: usize = 1024;
const MAX_DYNAMIC_SETTING_DEPTH: usize = 4;
const MAX_DYNAMIC_SETTING_COLLECTION_LEN: usize = 64;

const RESERVED_CONSENT_FIELDS: [&str; 16] = [
    "version",
    "jobId",
    "mode",
    "capU",
    "tradeAmountU",
    "quoteToken",
    "tradeEnvironment",
    "marginMode",
    "orderPolicy",
    "authMode",
    "serviceGuideHash",
    "requiredFields",
    "extra",
    "createdAt",
    "expiresAt",
    "status",
];

/// Stable, product-defined fields that remain flat in the consent object. New,
/// service-specific fields belong under `extra` instead of being added here.
const KNOWN_FLAT_SETTING_FIELDS: [&str; 18] = [
    "tradeAmountMode",
    "tradeAmountRatio",
    "tradeAmountBasis",
    "leverageMode",
    "leverage",
    "maxLeverage",
    "takeProfitRatio",
    "stopLossRatio",
    "slippage",
    "maxAutoSlippage",
    "gasLevel",
    "mevProtection",
    "orderSize",
    "sellShares",
    "orderType",
    // Compatibility aliases written by earlier 4.8.4-beta builds.
    "tradeAmountType",
    "tradeAmountPercent",
    "serviceGuideHash",
];

const EXTRA_FIELD_TYPES: [&str; 7] = [
    "boolean", "integer", "decimal", "string", "enum", "array", "object",
];
const EXTRA_FIELD_METADATA: [&str; 9] = [
    "label",
    "type",
    "value",
    "description",
    "constraints",
    "options",
    "appliesWhen",
    "confirmedAt",
    "unit",
];

fn dynamic_setting_key_is_safe(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && key.len() <= MAX_DYNAMIC_SETTING_KEY_LEN
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !RESERVED_CONSENT_FIELDS.contains(&key)
}

pub fn validate_dynamic_setting_name(key: &str) -> anyhow::Result<()> {
    if !KNOWN_FLAT_SETTING_FIELDS.contains(&key) && !dynamic_setting_key_is_safe(key) {
        anyhow::bail!("invalid or reserved dynamic consent setting key: {key}");
    }
    if dynamic_setting_key_is_sensitive(key) {
        anyhow::bail!("credentials must not be stored in consent settings: {key}");
    }
    Ok(())
}

fn bounded_nonempty_text(value: &Value, field: &str, max_chars: usize) -> anyhow::Result<()> {
    let Some(value) = value.as_str() else {
        anyhow::bail!("{field} must be a string");
    };
    if value.trim().is_empty() {
        anyhow::bail!("{field} must not be empty");
    }
    if value.chars().count() > max_chars || value.chars().any(char::is_control) {
        anyhow::bail!("{field} is invalid or too long");
    }
    Ok(())
}

fn validate_string_or_string_array(value: &Value, field: &str) -> anyhow::Result<()> {
    match value {
        Value::String(_) => bounded_nonempty_text(value, field, 64),
        Value::Array(values) if !values.is_empty() => {
            if values.len() > MAX_DYNAMIC_SETTING_COLLECTION_LEN {
                anyhow::bail!("{field} contains too many values");
            }
            for value in values {
                bounded_nonempty_text(value, field, 64)?;
            }
            Ok(())
        }
        _ => anyhow::bail!("{field} must be a string or non-empty string array"),
    }
}

fn validate_extra_field_value(field_type: &str, value: &Value, field: &str) -> anyhow::Result<()> {
    if value.is_null() {
        anyhow::bail!("extra.{field}.value is required and must not be null");
    }
    let valid = match field_type {
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "decimal" => value
            .as_str()
            .is_some_and(|raw| Decimal::parse(raw).is_ok()),
        "string" | "enum" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    };
    if !valid {
        anyhow::bail!("extra.{field}.value does not match type {field_type}");
    }
    validate_dynamic_setting_value(value, 0)
}

fn validate_extra_constraints(
    field: &str,
    field_type: &str,
    value: &Value,
    constraints: &serde_json::Map<String, Value>,
) -> anyhow::Result<()> {
    for key in constraints.keys() {
        if !matches!(
            key.as_str(),
            "min" | "max" | "minExclusive" | "maxExclusive" | "minLength" | "maxLength"
        ) {
            anyhow::bail!("unsupported extra.{field}.constraints field: {key}");
        }
    }
    if matches!(field_type, "integer" | "decimal") {
        let actual = decimal_setting_value(value, &format!("extra.{field}.value"))?;
        for key in ["min", "max", "minExclusive", "maxExclusive"] {
            let Some(limit) = constraints.get(key) else {
                continue;
            };
            let limit = decimal_setting_value(limit, &format!("extra.{field}.constraints.{key}"))?;
            let valid = match key {
                "min" => limit.le(&actual),
                "max" => actual.le(&limit),
                "minExclusive" => limit.le(&actual) && limit != actual,
                "maxExclusive" => actual.le(&limit) && actual != limit,
                _ => unreachable!(),
            };
            if !valid {
                anyhow::bail!("extra.{field}.value violates {key}");
            }
        }
    } else if constraints
        .keys()
        .any(|key| matches!(key.as_str(), "min" | "max" | "minExclusive" | "maxExclusive"))
    {
        anyhow::bail!("numeric constraints require integer or decimal type for extra.{field}");
    }

    let length = match value {
        Value::String(value) => Some(value.chars().count() as u64),
        Value::Array(value) => Some(value.len() as u64),
        Value::Object(value) => Some(value.len() as u64),
        _ => None,
    };
    for key in ["minLength", "maxLength"] {
        let Some(limit) = constraints.get(key) else {
            continue;
        };
        let Some(limit) = limit.as_u64() else {
            anyhow::bail!("extra.{field}.constraints.{key} must be a non-negative integer");
        };
        let Some(length) = length else {
            anyhow::bail!("length constraints require string, array, or object type for extra.{field}");
        };
        let valid = if key == "minLength" {
            length >= limit
        } else {
            length <= limit
        };
        if !valid {
            anyhow::bail!("extra.{field}.value violates {key}");
        }
    }
    Ok(())
}

fn validate_extra_field(field: &str, value: &Value) -> anyhow::Result<()> {
    if !dynamic_setting_key_is_safe(field) || dynamic_setting_key_is_sensitive(field) {
        anyhow::bail!("invalid or reserved extra consent field: {field}");
    }
    let Value::Object(metadata) = value else {
        anyhow::bail!("extra.{field} must be an object");
    };
    for key in metadata.keys() {
        if !EXTRA_FIELD_METADATA.contains(&key.as_str()) {
            anyhow::bail!("unsupported extra.{field} metadata field: {key}");
        }
    }
    let label = metadata
        .get("label")
        .ok_or_else(|| anyhow::anyhow!("extra.{field}.label is required"))?;
    bounded_nonempty_text(label, &format!("extra.{field}.label"), 128)?;
    let field_type = metadata
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("extra.{field}.type is required"))?;
    if !EXTRA_FIELD_TYPES.contains(&field_type) {
        anyhow::bail!(
            "extra.{field}.type must be one of: {}",
            EXTRA_FIELD_TYPES.join(" | ")
        );
    }
    let confirmed_value = metadata
        .get("value")
        .ok_or_else(|| anyhow::anyhow!("extra.{field}.value is required"))?;
    validate_extra_field_value(field_type, confirmed_value, field)?;

    if let Some(description) = metadata.get("description") {
        if !description.is_null() {
            bounded_nonempty_text(description, &format!("extra.{field}.description"), 1024)?;
        }
    }
    if let Some(unit) = metadata.get("unit") {
        if !unit.is_null() {
            bounded_nonempty_text(unit, &format!("extra.{field}.unit"), 64)?;
        }
    }
    if let Some(constraints) = metadata.get("constraints") {
        if !constraints.is_null() {
            let Value::Object(constraints) = constraints else {
                anyhow::bail!("extra.{field}.constraints must be an object or null");
            };
            validate_extra_constraints(field, field_type, confirmed_value, constraints)?;
        }
    }
    if let Some(options) = metadata.get("options") {
        if !options.is_null() {
            let Value::Array(options) = options else {
                anyhow::bail!("extra.{field}.options must be an array or null");
            };
            if options.is_empty() || options.len() > MAX_DYNAMIC_SETTING_COLLECTION_LEN {
                anyhow::bail!("extra.{field}.options must be a non-empty bounded array");
            }
            if field_type != "enum" {
                anyhow::bail!("extra.{field}.options is only valid for enum fields");
            }
            if options.iter().any(|option| !option.is_string()) {
                anyhow::bail!("extra.{field}.options entries must be strings");
            }
            if !options.iter().any(|option| option == confirmed_value) {
                anyhow::bail!("extra.{field}.value must be listed in options");
            }
        }
    }
    if let Some(applies_when) = metadata.get("appliesWhen") {
        if !applies_when.is_null() {
            let Value::Object(applies_when) = applies_when else {
                anyhow::bail!("extra.{field}.appliesWhen must be an object or null");
            };
            for key in applies_when.keys() {
                if !matches!(key.as_str(), "venue" | "operation") {
                    anyhow::bail!("unsupported extra.{field}.appliesWhen field: {key}");
                }
            }
            for (key, value) in applies_when {
                validate_string_or_string_array(
                    value,
                    &format!("extra.{field}.appliesWhen.{key}"),
                )?;
            }
        }
    }
    if let Some(confirmed_at) = metadata.get("confirmedAt") {
        if !confirmed_at.is_null() && confirmed_at.as_u64().is_none() {
            anyhow::bail!("extra.{field}.confirmedAt must be a non-negative integer or null");
        }
    }
    Ok(())
}

fn validate_extra_settings(value: &Value, allow_removals: bool) -> anyhow::Result<()> {
    let Value::Object(fields) = value else {
        anyhow::bail!("extra must be a JSON object");
    };
    if fields.len() > MAX_DYNAMIC_SETTING_COUNT {
        anyhow::bail!("extra contains too many fields");
    }
    for (field, value) in fields {
        if allow_removals && value.is_null() {
            if !dynamic_setting_key_is_safe(field) || dynamic_setting_key_is_sensitive(field) {
                anyhow::bail!("invalid or reserved extra consent field: {field}");
            }
            continue;
        }
        validate_extra_field(field, value)?;
    }
    Ok(())
}

fn dynamic_setting_key_is_sensitive(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "password",
        "passphrase",
        "privatekey",
        "secretkey",
        "apikey",
        "accesstoken",
        "refreshtoken",
        "credential",
        "jwt",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn validate_dynamic_setting_value(value: &Value, depth: usize) -> anyhow::Result<()> {
    if depth > MAX_DYNAMIC_SETTING_DEPTH {
        anyhow::bail!("dynamic consent setting exceeds maximum nesting depth");
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        Value::String(value) => {
            if value.chars().count() > MAX_DYNAMIC_SETTING_STRING_LEN {
                anyhow::bail!("dynamic consent setting string is too long");
            }
            if value.chars().any(char::is_control) {
                anyhow::bail!("dynamic consent setting contains control characters");
            }
            Ok(())
        }
        Value::Array(values) => {
            if values.len() > MAX_DYNAMIC_SETTING_COLLECTION_LEN {
                anyhow::bail!("dynamic consent setting array is too large");
            }
            for value in values {
                validate_dynamic_setting_value(value, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            if values.len() > MAX_DYNAMIC_SETTING_COLLECTION_LEN {
                anyhow::bail!("dynamic consent setting object is too large");
            }
            for (key, value) in values {
                if !dynamic_setting_key_is_safe(key) || dynamic_setting_key_is_sensitive(key) {
                    anyhow::bail!("invalid dynamic consent setting key: {key}");
                }
                validate_dynamic_setting_value(value, depth + 1)?;
            }
            Ok(())
        }
    }
}

fn validate_dynamic_settings_impl(
    settings: &DynamicConsentSettings,
    allow_removals: bool,
) -> anyhow::Result<()> {
    if settings.len() > MAX_DYNAMIC_SETTING_COUNT {
        anyhow::bail!("too many dynamic consent settings");
    }
    if serde_json::to_vec(settings)?.len() > MAX_DYNAMIC_SETTINGS_BYTES {
        anyhow::bail!("dynamic consent settings are too large");
    }
    for (key, value) in settings {
        if key == "extra" {
            if value.is_null() {
                continue;
            }
            validate_extra_settings(value, allow_removals)?;
            continue;
        }
        if key == "requiredFields" {
            let Value::Array(fields) = value else {
                anyhow::bail!("requiredFields must be an array");
            };
            if fields.len() > MAX_DYNAMIC_SETTING_COUNT {
                anyhow::bail!("requiredFields contains too many fields");
            }
            for field in fields {
                let Some(field) = field.as_str() else {
                    anyhow::bail!("requiredFields entries must be strings");
                };
                validate_required_field_name(field)?;
            }
            continue;
        }
        if !KNOWN_FLAT_SETTING_FIELDS.contains(&key.as_str()) {
            anyhow::bail!("unknown top-level consent setting {key}; put service-specific fields under extra");
        }
        validate_dynamic_setting_name(key)?;
        validate_dynamic_setting_value(value, 0)?;
        if value.is_null() {
            continue;
        }
        match key.as_str() {
            "tradeAmountMode" => match value.as_str() {
                Some("fixed_amount" | "available_balance_ratio") => {}
                _ => anyhow::bail!(
                    "tradeAmountMode must be one of: fixed_amount | available_balance_ratio"
                ),
            },
            "tradeAmountRatio" => {
                let ratio = decimal_setting_value(value, "tradeAmountRatio")?;
                if ratio.is_zero() || !ratio.le(&Decimal::parse("1")?) {
                    anyhow::bail!("tradeAmountRatio must be greater than 0 and at most 1");
                }
            }
            "tradeAmountBasis" => match value.as_str() {
                Some("notional" | "margin") => {}
                _ => anyhow::bail!("tradeAmountBasis must be one of: notional | margin"),
            },
            "leverageMode" => match value.as_str() {
                Some("keep_account" | "fixed" | "signal_capped") => {}
                _ => anyhow::bail!(
                    "leverageMode must be one of: keep_account | fixed | signal_capped"
                ),
            },
            "leverage" | "maxLeverage" | "orderSize" | "sellShares" => {
                let amount = decimal_setting_value(value, key)?;
                if amount.is_zero() {
                    anyhow::bail!("{key} must be greater than 0");
                }
            }
            "mevProtection" if !value.is_boolean() => {
                anyhow::bail!("mevProtection must be a boolean")
            }
            "orderType" => {
                bounded_nonempty_text(value, "orderType", 32)?;
            }
            "serviceGuideHash" => {
                let valid = value.as_str().is_some_and(|value| {
                    value.strip_prefix("sha256:").is_some_and(|digest| {
                        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                });
                if !valid {
                    anyhow::bail!("serviceGuideHash must be sha256:<64 lowercase-or-uppercase hex characters>");
                }
            }
            "tradeAmountType" => match value.as_str() {
                Some("fixed" | "percentage") => {}
                _ => anyhow::bail!("tradeAmountType must be one of: fixed | percentage"),
            },
            "tradeAmountPercent" => {
                let percentage = decimal_setting_value(value, "tradeAmountPercent")?;
                if percentage.is_zero() || !percentage.le(&Decimal::parse("100")?) {
                    anyhow::bail!("tradeAmountPercent must be greater than 0 and at most 100");
                }
            }
            "takeProfitRatio" | "stopLossRatio" => {
                let ratio = decimal_setting_value(value, key)?;
                if ratio.is_zero() {
                    anyhow::bail!("{key} must be greater than 0");
                }
            }
            _ => {}
        }
    }
    if settings.contains_key("tradeAmountMode") && settings.contains_key("tradeAmountType") {
        anyhow::bail!("use tradeAmountMode; do not combine it with legacy tradeAmountType");
    }
    if settings.contains_key("tradeAmountRatio") && settings.contains_key("tradeAmountPercent") {
        anyhow::bail!("use tradeAmountRatio; do not combine it with legacy tradeAmountPercent");
    }
    match settings.get("leverageMode").and_then(Value::as_str) {
        Some("fixed") if !settings.contains_key("leverage") => {
            anyhow::bail!("leverage is required when leverageMode=fixed")
        }
        Some("signal_capped") if !settings.contains_key("maxLeverage") => {
            anyhow::bail!("maxLeverage is required when leverageMode=signal_capped")
        }
        Some("keep_account")
            if settings.contains_key("leverage") || settings.contains_key("maxLeverage") =>
        {
            anyhow::bail!("leverage and maxLeverage are not valid when leverageMode=keep_account")
        }
        None if settings.contains_key("leverage") || settings.contains_key("maxLeverage") => {
            anyhow::bail!("leverageMode is required when leverage or maxLeverage is present")
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_dynamic_settings(settings: &DynamicConsentSettings) -> anyhow::Result<()> {
    validate_dynamic_settings_impl(settings, false)
}

fn validate_dynamic_setting_updates(settings: &DynamicConsentSettings) -> anyhow::Result<()> {
    validate_dynamic_settings_impl(settings, true)
}

pub fn validate_required_field_name(field: &str) -> anyhow::Result<()> {
    let core = matches!(
        field,
        "mode"
            | "tradeAmount"
            | "tradeAmountU"
            | "cap"
            | "quote"
            | "environment"
            | "marginMode"
            | "orderPolicy"
            | "authMode"
    );
    if core || KNOWN_FLAT_SETTING_FIELDS.contains(&field) {
        return Ok(());
    }
    if let Some(extra) = field.strip_prefix("extra.") {
        return validate_dynamic_setting_name(extra);
    }
    anyhow::bail!("unknown required consent field: {field}")
}

pub fn dynamic_setting_present(settings: &DynamicConsentSettings, field: &str) -> bool {
    if let Some(extra_field) = field.strip_prefix("extra.") {
        return settings
            .get("extra")
            .and_then(Value::as_object)
            .is_some_and(|extra| extra.contains_key(extra_field));
    }
    settings.get(field).is_some_and(|value| !value.is_null())
}

fn decimal_setting_value(value: &Value, field: &str) -> anyhow::Result<Decimal> {
    let raw = value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_number().map(ToString::to_string))
        .ok_or_else(|| anyhow::anyhow!("{field} must be a decimal string or number"))?;
    Decimal::parse(&raw).map_err(|_| anyhow::anyhow!("{field} must be a valid decimal"))
}

pub fn dynamic_decimal_setting(
    settings: &DynamicConsentSettings,
    field: &str,
) -> anyhow::Result<Option<Decimal>> {
    settings
        .get(field)
        .map(|value| decimal_setting_value(value, field))
        .transpose()
}

pub fn uses_percentage_amount(settings: &DynamicConsentSettings) -> bool {
    settings.get("tradeAmountMode").and_then(Value::as_str)
        == Some("available_balance_ratio")
        || settings.get("tradeAmountType").and_then(Value::as_str) == Some("percentage")
}

pub fn validate_amount_policy(
    trade_amount_u: Option<&str>,
    settings: &DynamicConsentSettings,
) -> anyhow::Result<()> {
    match settings.get("tradeAmountMode").and_then(Value::as_str) {
        Some("fixed_amount") if trade_amount_u.is_none() => {
            anyhow::bail!("tradeAmountU is required when tradeAmountMode=fixed_amount")
        }
        Some("available_balance_ratio") if !settings.contains_key("tradeAmountRatio") => {
            anyhow::bail!(
                "tradeAmountRatio is required when tradeAmountMode=available_balance_ratio"
            )
        }
        None if settings.contains_key("tradeAmountRatio") => {
            anyhow::bail!(
                "tradeAmountMode=available_balance_ratio is required when tradeAmountRatio is present"
            )
        }
        _ => {}
    }
    match settings.get("tradeAmountType").and_then(Value::as_str) {
        Some("fixed") if trade_amount_u.is_none() => {
            anyhow::bail!("tradeAmountU is required when tradeAmountType=fixed")
        }
        Some("percentage") if !settings.contains_key("tradeAmountPercent") => {
            anyhow::bail!("tradeAmountPercent is required when tradeAmountType=percentage")
        }
        None if settings.contains_key("tradeAmountPercent") => {
            anyhow::bail!(
                "tradeAmountType=percentage is required when tradeAmountPercent is present"
            )
        }
        _ => Ok(()),
    }
}

pub fn parse_dynamic_settings_json(
    input: Option<&str>,
    flag: &str,
) -> anyhow::Result<DynamicConsentSettings> {
    let Some(input) = input else {
        return Ok(DynamicConsentSettings::new());
    };
    if input.len() > MAX_DYNAMIC_SETTINGS_BYTES {
        anyhow::bail!("{flag} is too large");
    }
    let value: Value =
        serde_json::from_str(input).map_err(|_| anyhow::anyhow!("{flag} must be a JSON object"))?;
    let Value::Object(values) = value else {
        anyhow::bail!("{flag} must be a JSON object");
    };
    let settings = values.into_iter().collect::<DynamicConsentSettings>();
    validate_dynamic_setting_updates(&settings)?;
    Ok(settings)
}

pub(crate) fn merge_dynamic_settings(
    target: &mut DynamicConsentSettings,
    updates: &DynamicConsentSettings,
) {
    for (key, value) in updates {
        if value.is_null() {
            target.remove(key);
        } else if key == "extra" {
            let mut merged = target
                .get(key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(updates) = value.as_object() {
                for (field, update) in updates {
                    if update.is_null() {
                        merged.remove(field);
                    } else {
                        merged.insert(field.clone(), update.clone());
                    }
                }
            }
            if merged.is_empty() {
                target.remove(key);
            } else {
                target.insert(key.clone(), Value::Object(merged));
            }
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
}

/// Versioned, trusted metadata for a saved Active-subscription delivery.
///
/// The deliverable itself remains untrusted market data. This record only proves
/// which local artifact and stable delivery id were admitted by the CLI, so a
/// user-decision relay can safely resume after the original model turn ended.
pub const DELIVERY_CONTEXT_VERSION: u32 = 2;

fn legacy_subscription_trade_path() -> SubscriptionTradePath {
    SubscriptionTradePath::LegacyWrapper
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryContext {
    pub version: u32,
    pub job_id: String,
    pub agent_id: String,
    pub provider_agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_session_key: Option<String>,
    pub delivery_id: String,
    pub saved_path: String,
    pub deliverable_type: String,
    pub received_at_ms: u64,
    /// Execution path is fixed when the delivery is first admitted. Missing
    /// values belong to v1 contexts and deliberately retain legacy behavior.
    #[serde(default = "legacy_subscription_trade_path")]
    pub execution_path: SubscriptionTradePath,
}

fn bounded_json_after_marker(raw: &str) -> Option<serde_json::Value> {
    const MARKER: &str = "[ACTIONABLE_TRADING_SIGNAL]";
    let tail = raw.split_once(MARKER).map(|(_, tail)| tail).unwrap_or(raw);
    let start = tail.find('{')?;
    serde_json::Deserializer::from_str(&tail[start..])
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

fn short_display(value: &str, max: usize) -> String {
    let flattened = value
        .chars()
        .filter(|character| !character.is_control())
        .take(max)
        .collect::<String>();
    if value.chars().filter(|character| !character.is_control()).count() > max {
        format!("{flattened}…")
    } else {
        flattened
    }
}

/// Render only bounded, canonical fields from a trusted delivery context and
/// its untrusted artifact. Raw provider prose is deliberately never copied
/// into a decision card.
pub fn delivery_decision_summary(context: &DeliveryContext, lang: Lang) -> String {
    let raw = std::fs::read_to_string(&context.saved_path)
        .ok()
        .map(|value| value.chars().take(64 * 1024).collect::<String>());
    let signal = raw.as_deref().and_then(bounded_json_after_marker);
    let field = |pointer: &str| {
        signal
            .as_ref()
            .and_then(|value| value.pointer(pointer))
            .and_then(|value| match value {
                serde_json::Value::String(value) => Some(value.clone()),
                serde_json::Value::Number(value) => Some(value.to_string()),
                _ => None,
            })
            .filter(|value| !value.trim().is_empty())
            .map(|value| short_display(value.trim(), 128))
    };
    let signal_id = field("/signalId");
    let signal_type = field("/signalType").unwrap_or_else(|| context.deliverable_type.clone());
    let side = field("/params/side");
    let amount = field("/params/amount");
    let amount_unit = field("/params/amountUnit");
    let quote = field("/params/quoteCurrency");
    let chain = field("/params/chainIndex");
    let token = field("/params/tokenAddress");
    let file_name = std::path::Path::new(&context.saved_path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| short_display(value, 96));

    let mut lines = match lang {
        Lang::Zh => vec!["[对应交付物]".to_string()],
        Lang::En => vec!["[Deliverable for this decision]".to_string()],
    };
    let mut push = |zh: &str, en: &str, value: Option<String>| {
        if let Some(value) = value {
            lines.push(match lang {
                Lang::Zh => format!("{zh}: {value}"),
                Lang::En => format!("{en}: {value}"),
            });
        }
    };
    push(
        "交付 ID",
        "Delivery ID",
        Some(short_display(&context.delivery_id, 128)),
    );
    push("信号 ID", "Signal ID", signal_id);
    push("信号类型", "Signal type", Some(signal_type));
    push("方向", "Side", side);
    let amount_display = amount.map(|amount| {
        [Some(amount), amount_unit, quote]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
    });
    push("信号金额", "Signal amount", amount_display);
    push("链", "Chain", chain);
    push("Token", "Token", token);
    if signal.is_none() {
        push("文件", "File", file_name);
    }
    lines.join("\n")
}

pub fn pending_delivery_decision_summary(job_id: &str, lang: Lang) -> Option<String> {
    load_pending_delivery_context(job_id)
        .ok()
        .flatten()
        .map(|context| delivery_decision_summary(&context, lang))
}

/// Result of atomically binding an outstanding user decision to a delivery.
/// A different pending delivery is never overwritten: doing so could let a
/// decision reply authorize the wrong signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryActivation {
    Activated(DeliveryContext),
    AlreadyPending(DeliveryContext),
    Conflict(DeliveryContext),
}

/// How the buyer wants this subscription's Active signals handled.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConsentMode {
    /// Auto-execute within `capU` (buy-side); over-cap re-asks.
    Auto,
    /// Legacy on-disk value. It is treated as notify-only and never authorizes
    /// a per-delivery execution decision.
    Manual,
    /// Do not execute; notify only.
    Decline,
}

/// User-selected credential source for Trade Kit commands.
///
/// This stores only the source choice. OAuth tokens and API-key material remain
/// owned by Trade Kit and are never copied into the consent file.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TradeKitAuthMode {
    OAuth,
    ApiKey,
}

impl TradeKitAuthMode {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "oauth" => Ok(Self::OAuth),
            "api_key" | "api-key" => Ok(Self::ApiKey),
            _ => anyhow::bail!("auth mode must be one of: oauth | api_key"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OAuth => "oauth",
            Self::ApiKey => "api_key",
        }
    }
}

/// User-authorized margin mode for Trade Kit derivative orders.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MarginMode {
    Cross,
    Isolated,
}

impl MarginMode {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "cross" => Ok(Self::Cross),
            "isolated" => Ok(Self::Isolated),
            _ => anyhow::bail!("margin mode must be one of: cross | isolated"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cross => "cross",
            Self::Isolated => "isolated",
        }
    }
}

/// User-authorized policy for turning a signal entry into an order.
///
/// This is intentionally distinct from the execution bridge's `ExecutionMode`
/// (`auto` / `manual` / `one_time`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrderPolicy {
    Market,
    SignalPriceLimit,
}

impl OrderPolicy {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "market" => Ok(Self::Market),
            "signal_price_limit" => Ok(Self::SignalPriceLimit),
            _ => anyhow::bail!(
                "order policy must be one of: market | signal_price_limit"
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::SignalPriceLimit => "signal_price_limit",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConsentFile {
    /// Versions newer than [`CONSENT_VERSION`] are rejected as unreadable.
    pub version: u32,
    pub job_id: String,
    pub mode: ConsentMode,
    /// Per-trade buy-side cap in quote-stablecoin units (PRD copy denominates in
    /// USDT). Present (and enforced) only for `Auto`; `None`/absent for
    /// `Manual` / `Decline`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap_u: Option<String>,
    /// Fixed quote-currency amount used for each parsed-text copy trade. This is
    /// deliberately separate from `capU`: the amount says what to trade, while
    /// the cap remains the maximum the auto-consent permits. Older consent files
    /// predate this field and deserialize it as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_amount_u: Option<String>,
    /// The stablecoin the buyer pays dex buys with / receives on sells
    /// (`usdc` | `usdt`, lowercase alias). Captured from the consent reply
    /// ("A 每笔100 用USDC"); absent ⇒ the default, USDT (PRD denomination).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_token: Option<String>,
    /// User-authorized Trade Kit target. Older records predate this field and
    /// remain valid for non-Trade-Kit routes; a Trade Kit execution must fail
    /// closed until the user chooses `live` or `demo` once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_environment: Option<TradeEnvironment>,
    /// User-confirmed Trade Kit margin mode. It is absent for products where a
    /// margin mode does not apply and for older records that require one-time
    /// restoration before derivative execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_mode: Option<MarginMode>,
    /// User-confirmed order construction policy. Older records deserialize it
    /// as absent and must never silently fall back to a market order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_policy: Option<OrderPolicy>,
    /// User-selected Trade Kit credential source. Older consent files predate
    /// this field and must ask once before their next Trade Kit execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<TradeKitAuthMode>,
    /// User-confirmed, tool-specific execution settings. These fields remain
    /// flat in the JSON document and are preserved even when this CLI version
    /// does not know their business semantics. Core authorization fields above
    /// are reserved and can never be shadowed here.
    #[serde(default, flatten)]
    pub dynamic_settings: DynamicConsentSettings,
    /// seconds since epoch.
    pub created_at: u64,
    /// seconds since epoch.
    pub expires_at: u64,
}

/// Read-only consent context exposed to the model-driven subscription session.
///
/// This deliberately contains only previously persisted policy. Conversation
/// text and `serviceDescription` are never converted into authorization here.
/// A missing/expired record is `NotSet`; a corrupt record is `Unreadable` so the
/// model can fail closed instead of treating it as a first-time prompt.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsentSnapshotStatus {
    NotSet,
    Active,
    Unreadable,
}

impl ConsentSnapshotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotSet => "not_set",
            Self::Active => "active",
            Self::Unreadable => "unreadable",
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConsentSnapshot {
    pub status: ConsentSnapshotStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ConsentMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap_u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_amount_u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_environment: Option<TradeEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_mode: Option<MarginMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_policy: Option<OrderPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<TradeKitAuthMode>,
    /// Complete validated tool-specific business settings, flattened into the
    /// model-facing snapshot just as they are in the consent file.
    #[serde(flatten)]
    pub dynamic_settings: DynamicConsentSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl ConsentSnapshot {
    fn not_set() -> Self {
        Self {
            status: ConsentSnapshotStatus::NotSet,
            version: None,
            mode: None,
            cap_u: None,
            trade_amount_u: None,
            quote_token: None,
            trade_environment: None,
            margin_mode: None,
            order_policy: None,
            auth_mode: None,
            dynamic_settings: DynamicConsentSettings::new(),
            created_at: None,
            expires_at: None,
        }
    }

    fn unreadable() -> Self {
        Self {
            status: ConsentSnapshotStatus::Unreadable,
            ..Self::not_set()
        }
    }
}

/// The default quote stablecoin (PRD denominates the whole flow in USDT).
pub const DEFAULT_QUOTE: &str = "usdt";
/// Quote stablecoins a consent may select. Whitelisted so the "per-trade cap in
/// stablecoin dollars" semantics can't be bent onto an arbitrary token.
pub const QUOTE_WHITELIST: [&str; 2] = ["usdc", "usdt"];

/// The quote stablecoin alias in effect for `job_id`: the consent record's
/// choice, else [`DEFAULT_QUOTE`]. Always lowercase, always whitelisted.
pub fn quote_token(job_id: &str) -> String {
    load_consent(job_id)
        .ok()
        .flatten()
        .and_then(|c| c.quote_token)
        .filter(|q| QUOTE_WHITELIST.contains(&q.as_str()))
        .unwrap_or_else(|| DEFAULT_QUOTE.to_string())
}

// ── Stable, process-level failure reasons (never echo file content) ──
pub const CONSENT_UNREADABLE: &str = "consent_unreadable";
pub const CONSENT_VERSION_TOO_NEW: &str = "consent_version_too_new";
pub const CONSENT_JOB_MISMATCH: &str = "consent_job_mismatch";

fn persisted_required_fields_are_complete(file: &ConsentFile) -> bool {
    let Some(fields) = file
        .dynamic_settings
        .get("requiredFields")
        .and_then(Value::as_array)
    else {
        return true;
    };
    fields.iter().all(|field| {
        let Some(field) = field.as_str() else {
            return false;
        };
        match field {
            "mode" => true,
            "tradeAmount" | "tradeAmountU" => file.trade_amount_u.is_some(),
            "cap" => file.cap_u.is_some(),
            "quote" => file.quote_token.is_some(),
            "environment" => file.trade_environment.is_some(),
            "marginMode" => file.margin_mode.is_some(),
            "orderPolicy" => file.order_policy.is_some(),
            "authMode" => file.auth_mode.is_some(),
            other => dynamic_setting_present(&file.dynamic_settings, other),
        }
    })
}

fn inferred_extra_type(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "decimal",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "string",
    }
}

/// Earlier local builds stored every guide-defined setting at the top level.
/// Move only those unknown fields into typed `extra` entries in memory so the
/// unpublished schema refinement does not discard an already user-confirmed
/// value. Stable fields and their compatibility aliases remain flat.
fn migrate_legacy_flat_settings(file: &mut ConsentFile) {
    let legacy_keys = file
        .dynamic_settings
        .keys()
        .filter(|key| {
            !KNOWN_FLAT_SETTING_FIELDS.contains(&key.as_str())
                && !matches!(key.as_str(), "requiredFields" | "extra")
        })
        .cloned()
        .collect::<Vec<_>>();
    if legacy_keys.is_empty() {
        return;
    }
    let mut extra = file
        .dynamic_settings
        .get("extra")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in legacy_keys {
        if let Some(mut value) = file.dynamic_settings.remove(&key) {
            let field_type = inferred_extra_type(&value);
            if value.as_number().is_some_and(|number| {
                number.as_i64().is_none() && number.as_u64().is_none()
            }) {
                value = Value::String(value.to_string());
            }
            extra.entry(key.clone()).or_insert_with(|| {
                serde_json::json!({
                    "label": key,
                    "type": field_type,
                    "value": value,
                })
            });
        }
    }
    file.dynamic_settings
        .insert("extra".to_string(), Value::Object(extra));
}

/// A hard consent read failure (present-but-broken record). Distinct from the
/// normal "no record / declined / manual / auto" states carried by
/// [`ConsentDecision`]. Fails closed at the pipeline (degrade → notify).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentError(pub &'static str);

impl std::fmt::Display for ConsentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ConsentError {}

/// The client-side consent verdict for one inbound Active signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDecision {
    /// No record yet — ask the three-way decision.
    FirstTime,
    /// `Decline` (C = "don't execute THIS one") — re-ask the three-way on every signal (auto
    /// stays off; C only rejects the current signal, it is not a persistent decline).
    Declined,
    /// `Manual` (B = "execute this one, then ask every time") — re-ask the three-way on every
    /// signal (auto stays off).
    Manual,
    /// `Auto`, and this action is allowed to auto-execute (sell, or buy within cap).
    AutoAllow,
    /// `Auto`, but a buy exceeds `capU` (or `Auto` with no cap on a buy) — re-ask.
    AutoOverCap,
}

/// `<onchainos_home>/autotrade/consent/<jobId>.json`. Caller MUST have
/// charset-checked `job_id` first (path-traversal defense).
fn consent_path(job_id: &str) -> Result<PathBuf, ConsentError> {
    let home = crate::home::onchainos_home().map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    Ok(home
        .join("autotrade")
        .join("consent")
        .join(format!("{job_id}.json")))
}

/// Current wall-clock time in seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load + validate the consent record for `job_id`.
///
/// Returns `Ok(None)` when there is no record OR the record has **expired**
/// (both mean execution is not configured). Returns `Err` only for a
/// present-but-broken record (unreadable / version-too-new / job mismatch), which
/// the pipeline fails closed on. `Ok(Some(_))` is a live, valid record.
pub fn load_consent(job_id: &str) -> Result<Option<ConsentFile>, ConsentError> {
    if !job_id_is_safe(job_id) {
        // Charset failure is handled by the pipeline's entry guard; be defensive.
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    let path = consent_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    let mut file: ConsentFile =
        serde_json::from_str(&raw).map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    if file.version > CONSENT_VERSION {
        return Err(ConsentError(CONSENT_VERSION_TOO_NEW));
    }
    if file.job_id != job_id {
        return Err(ConsentError(CONSENT_JOB_MISMATCH));
    }
    migrate_legacy_flat_settings(&mut file);
    if validate_dynamic_settings(&file.dynamic_settings).is_err()
        || file.dynamic_settings.values().any(Value::is_null)
        || validate_amount_policy(file.trade_amount_u.as_deref(), &file.dynamic_settings)
            .is_err()
        || !persisted_required_fields_are_complete(&file)
    {
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    if file
        .trade_environment
        .is_some_and(|environment| !environment.is_explicit())
    {
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    // Expired ⇒ execution is no longer configured, not a hard error.
    if file.expires_at <= now_secs() {
        return Ok(None);
    }
    Ok(Some(file))
}

/// Return a bounded, serialization-safe view of the current local policy.
/// Errors are represented explicitly and never expose file content or paths.
pub fn consent_snapshot(job_id: &str) -> ConsentSnapshot {
    match load_consent(job_id) {
        Ok(Some(file)) => ConsentSnapshot {
            status: ConsentSnapshotStatus::Active,
            version: Some(file.version),
            mode: Some(file.mode),
            cap_u: file.cap_u,
            trade_amount_u: file.trade_amount_u,
            quote_token: file.quote_token,
            trade_environment: file.trade_environment,
            margin_mode: file.margin_mode,
            order_policy: file.order_policy,
            auth_mode: file.auth_mode,
            dynamic_settings: file.dynamic_settings,
            created_at: Some(file.created_at),
            expires_at: Some(file.expires_at),
        },
        Ok(None) => ConsentSnapshot::not_set(),
        Err(_) => ConsentSnapshot::unreadable(),
    }
}

/// The client-side consent verdict for one inbound Active signal.
///
/// `buy_amount` is `Some` only for a cap-relevant **buy** (dex buy / defi deposit /
/// polymarket buy — spend in quote-stablecoin units); `None` for sells and no-spend
/// actions, which are never cap-gated. The mode dispatch (first-time / declined /
/// manual / auto) is independent of `buy_amount`; only the `Auto` cap check uses it.
pub fn evaluate_consent(
    job_id: &str,
    buy_amount: Option<&Decimal>,
) -> Result<ConsentDecision, ConsentError> {
    let Some(file) = load_consent(job_id)? else {
        return Ok(ConsentDecision::FirstTime);
    };
    match file.mode {
        ConsentMode::Decline => Ok(ConsentDecision::Declined),
        ConsentMode::Manual => Ok(ConsentDecision::Manual),
        ConsentMode::Auto => match buy_amount {
            // Sell / no-spend under Auto ⇒ always allowed (never cap-gated).
            None => Ok(ConsentDecision::AutoAllow),
            // Buy under Auto ⇒ enforce the per-trade cap. A missing/unparseable cap
            // on an Auto buy fails closed to a re-ask (never an uncapped auto-buy).
            Some(amount) => {
                let within = file
                    .cap_u
                    .as_deref()
                    .and_then(|c| Decimal::parse(c).ok())
                    .map(|cap| amount.le(&cap))
                    .unwrap_or(false);
                Ok(if within {
                    ConsentDecision::AutoAllow
                } else {
                    ConsentDecision::AutoOverCap
                })
            }
        },
    }
}

/// Persist a consent record for `job_id` (release-mode; replaces the debug-only
/// `grants::write_grant` seeding). `cap_u` is optional and validated when
/// present for `Auto`, and must be absent for `Manual` / `Decline`.
pub fn write_consent(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    quote: Option<&str>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    write_consent_with_trade_amount(job_id, mode, cap_u, None, quote, ttl_sec)
}

/// Persist consent together with an optional fixed per-trade amount.
///
/// `trade_amount_u = None` preserves an existing configured amount, matching
/// the existing `quote = None` behavior. This is important for cap-only rewrites
/// after an over-cap decision: raising the cap must not silently erase the
/// subscription's configured execution amount. An explicitly supplied amount
/// must be a positive decimal.
pub fn write_consent_with_trade_amount(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    trade_amount_u: Option<&str>,
    quote: Option<&str>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    write_consent_policy(
        job_id,
        mode,
        cap_u,
        trade_amount_u,
        quote,
        None,
        ttl_sec,
    )
}

/// Persist consent and optionally replace the user-authorized Trade Kit
/// environment. An omitted environment preserves an existing choice so cap,
/// amount, quote, and renewal rewrites cannot silently clear it.
pub fn write_consent_policy(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    trade_amount_u: Option<&str>,
    quote: Option<&str>,
    trade_environment: Option<TradeEnvironment>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    write_consent_policy_with_settings(
        job_id,
        mode,
        cap_u,
        trade_amount_u,
        quote,
        trade_environment,
        None,
        None,
        None,
        ttl_sec,
    )
}

/// Persist the complete local execution policy. Omitted Trade Kit settings
/// preserve an existing choice, which makes cap/amount changes safe and lets
/// older callers remain source-compatible.
pub fn write_consent_policy_with_settings(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    trade_amount_u: Option<&str>,
    quote: Option<&str>,
    trade_environment: Option<TradeEnvironment>,
    margin_mode: Option<MarginMode>,
    order_policy: Option<OrderPolicy>,
    auth_mode: Option<TradeKitAuthMode>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    write_consent_policy_with_dynamic_settings(
        job_id,
        mode,
        cap_u,
        trade_amount_u,
        quote,
        trade_environment,
        margin_mode,
        order_policy,
        auth_mode,
        None,
        ttl_sec,
    )
}

/// Persist the complete local execution policy and merge user-confirmed,
/// tool-specific settings. `null` values remove an existing dynamic setting;
/// all other values remain flat in the consent JSON and model snapshot.
pub fn write_consent_policy_with_dynamic_settings(
    job_id: &str,
    mode: ConsentMode,
    cap_u: Option<&str>,
    trade_amount_u: Option<&str>,
    quote: Option<&str>,
    trade_environment: Option<TradeEnvironment>,
    margin_mode: Option<MarginMode>,
    order_policy: Option<OrderPolicy>,
    auth_mode: Option<TradeKitAuthMode>,
    dynamic_updates: Option<&DynamicConsentSettings>,
    ttl_sec: u64,
) -> anyhow::Result<()> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    if ttl_sec == 0 {
        anyhow::bail!("--ttl-sec must be > 0");
    }
    let cap_u = match mode {
        ConsentMode::Auto => cap_u
            .map(|cap| {
                let parsed = Decimal::parse(cap)
                    .map_err(|_| anyhow::anyhow!("--cap is not a valid decimal"))?;
                if parsed.is_zero() {
                    anyhow::bail!("--cap must be greater than 0");
                }
                Ok(cap.to_string())
            })
            .transpose()?,
        ConsentMode::Manual | ConsentMode::Decline => {
            if cap_u.is_some() {
                anyhow::bail!("--cap is only valid with --mode auto");
            }
            None
        }
    };
    // Best-effort compatibility read. This intentionally retains the historical
    // behavior of allowing a valid new write to replace a broken old record.
    let existing = load_consent(job_id).ok().flatten();
    let trade_amount_u = match trade_amount_u {
        Some(amount) => {
            let parsed = Decimal::parse(amount)
                .map_err(|_| anyhow::anyhow!("--trade-amount is not a valid decimal"))?;
            if parsed.is_zero() {
                anyhow::bail!("--trade-amount must be greater than 0");
            }
            Some(amount.to_string())
        }
        None => existing.as_ref().and_then(|c| c.trade_amount_u.clone()),
    };
    // Quote preference: explicit value must be whitelisted; absent ⇒ KEEP the
    // existing record's choice (an over-cap raise re-writes the record and must
    // not silently reset a stored preference back to the default).
    let quote_token = match quote {
        Some(q) => {
            let q = q.to_ascii_lowercase();
            if !QUOTE_WHITELIST.contains(&q.as_str()) {
                anyhow::bail!("--quote must be one of: usdc | usdt");
            }
            Some(q)
        }
        None => existing.as_ref().and_then(|c| c.quote_token.clone()),
    };
    if trade_environment.is_some_and(|environment| !environment.is_explicit()) {
        anyhow::bail!("trade environment must be live or demo");
    }
    let trade_environment = trade_environment.or_else(|| {
        existing
            .as_ref()
            .and_then(|consent| consent.trade_environment)
    });
    let margin_mode = margin_mode.or_else(|| existing.as_ref().and_then(|c| c.margin_mode));
    let order_policy = order_policy.or_else(|| existing.as_ref().and_then(|c| c.order_policy));
    let auth_mode = auth_mode.or_else(|| existing.as_ref().and_then(|c| c.auth_mode));
    let mut dynamic_settings = existing
        .as_ref()
        .map(|consent| consent.dynamic_settings.clone())
        .unwrap_or_default();
    if let Some(updates) = dynamic_updates {
        validate_dynamic_setting_updates(updates)?;
        merge_dynamic_settings(&mut dynamic_settings, updates);
    }
    validate_dynamic_settings(&dynamic_settings)?;
    validate_amount_policy(trade_amount_u.as_deref(), &dynamic_settings)?;

    let created_at = now_secs();
    let file = ConsentFile {
        version: CONSENT_VERSION,
        job_id: job_id.to_string(),
        mode,
        cap_u,
        trade_amount_u,
        quote_token,
        trade_environment,
        margin_mode,
        order_policy,
        auth_mode,
        dynamic_settings,
        created_at,
        expires_at: created_at + ttl_sec,
    };
    if !persisted_required_fields_are_complete(&file) {
        anyhow::bail!("requiredFields contains a setting without a confirmed value");
    }

    let path = consent_path(job_id).map_err(|d| anyhow::anyhow!("{}", d.0))?;
    let body = serde_json::to_string_pretty(&file)?;
    crate::home::write_secure(&path, body.as_bytes())?;
    Ok(())
}

/// Update only the Trade Kit environment on an existing live consent record.
/// This is used when an older policy first reaches a Trade Kit delivery: the
/// user's one-time environment choice must not rewrite amount, cap, quote,
/// mode, or expiry.
pub fn write_trade_environment(
    job_id: &str,
    trade_environment: TradeEnvironment,
) -> anyhow::Result<ConsentFile> {
    if !trade_environment.is_explicit() {
        anyhow::bail!("trade environment must be live or demo");
    }
    let mut file = load_consent(job_id)
        .map_err(|error| anyhow::anyhow!(error.0))?
        .ok_or_else(|| anyhow::anyhow!("no live consent"))?;
    file.version = CONSENT_VERSION;
    file.trade_environment = Some(trade_environment);
    let path = consent_path(job_id).map_err(|error| anyhow::anyhow!(error.0))?;
    let body = serde_json::to_string_pretty(&file)?;
    crate::home::write_secure(&path, body.as_bytes())?;
    Ok(file)
}

/// Partially update user-confirmed Trade Kit execution settings without
/// rewriting mode, amount, cap, quote, timestamps, or expiry.
pub fn write_trade_settings(
    job_id: &str,
    trade_environment: Option<TradeEnvironment>,
    margin_mode: Option<MarginMode>,
    order_policy: Option<OrderPolicy>,
    auth_mode: Option<TradeKitAuthMode>,
) -> anyhow::Result<ConsentFile> {
    write_trade_settings_with_dynamic(
        job_id,
        trade_environment,
        margin_mode,
        order_policy,
        auth_mode,
        None,
    )
}

pub fn write_trade_settings_with_dynamic(
    job_id: &str,
    trade_environment: Option<TradeEnvironment>,
    margin_mode: Option<MarginMode>,
    order_policy: Option<OrderPolicy>,
    auth_mode: Option<TradeKitAuthMode>,
    dynamic_updates: Option<&DynamicConsentSettings>,
) -> anyhow::Result<ConsentFile> {
    if trade_environment.is_none()
        && margin_mode.is_none()
        && order_policy.is_none()
        && auth_mode.is_none()
        && dynamic_updates.is_none_or(DynamicConsentSettings::is_empty)
    {
        anyhow::bail!("at least one consent setting is required");
    }
    if trade_environment.is_some_and(|environment| !environment.is_explicit()) {
        anyhow::bail!("trade environment must be live or demo");
    }
    let mut file = load_consent(job_id)
        .map_err(|error| anyhow::anyhow!(error.0))?
        .ok_or_else(|| anyhow::anyhow!("no live consent"))?;
    file.version = CONSENT_VERSION;
    if let Some(value) = trade_environment {
        file.trade_environment = Some(value);
    }
    if let Some(value) = margin_mode {
        file.margin_mode = Some(value);
    }
    if let Some(value) = order_policy {
        file.order_policy = Some(value);
    }
    if let Some(value) = auth_mode {
        file.auth_mode = Some(value);
    }
    if let Some(updates) = dynamic_updates {
        validate_dynamic_setting_updates(updates)?;
        merge_dynamic_settings(&mut file.dynamic_settings, updates);
        validate_dynamic_settings(&file.dynamic_settings)?;
    }
    validate_amount_policy(file.trade_amount_u.as_deref(), &file.dynamic_settings)?;
    if !persisted_required_fields_are_complete(&file) {
        anyhow::bail!("requiredFields contains a setting without a confirmed value");
    }
    let path = consent_path(job_id).map_err(|error| anyhow::anyhow!(error.0))?;
    let body = serde_json::to_string_pretty(&file)?;
    crate::home::write_secure(&path, body.as_bytes())?;
    Ok(file)
}

fn delivery_id_is_safe(delivery_id: &str) -> bool {
    !delivery_id.is_empty()
        && delivery_id.len() <= 96
        && delivery_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':'))
}

fn delivery_context_path(job_id: &str, delivery_id: &str) -> anyhow::Result<PathBuf> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    if !delivery_id_is_safe(delivery_id) {
        anyhow::bail!("invalid delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("delivery-context")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn pending_delivery_path(job_id: &str) -> anyhow::Result<PathBuf> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("pending")
        .join(format!("{job_id}.json")))
}

/// Register a delivery admitted by the exact-Active subscription path.
///
/// This runs before the model sees the route prompt. A later hidden consent
/// command can therefore activate only a delivery id that the CLI registered;
/// model-supplied paths can never create a trusted continuation context.
#[allow(clippy::too_many_arguments)]
pub fn register_delivery_context(
    job_id: &str,
    agent_id: &str,
    provider_agent_id: &str,
    origin_session_key: Option<&str>,
    delivery_id: &str,
    saved_path: &str,
    deliverable_type: &str,
    received_at_ms: u64,
) -> anyhow::Result<DeliveryContext> {
    register_delivery_context_with_path(
        job_id,
        agent_id,
        provider_agent_id,
        origin_session_key,
        delivery_id,
        saved_path,
        deliverable_type,
        received_at_ms,
        SubscriptionTradePath::LegacyWrapper,
    )
}

/// Register a newly admitted delivery with its resolved, immutable execution
/// path. Runtime feature-switch changes apply only to later deliveries.
#[allow(clippy::too_many_arguments)]
pub fn register_delivery_context_with_path(
    job_id: &str,
    agent_id: &str,
    provider_agent_id: &str,
    origin_session_key: Option<&str>,
    delivery_id: &str,
    saved_path: &str,
    deliverable_type: &str,
    received_at_ms: u64,
    execution_path: SubscriptionTradePath,
) -> anyhow::Result<DeliveryContext> {
    let path = delivery_context_path(job_id, delivery_id)?;
    if path.exists() {
        let existing = load_delivery_context(job_id, delivery_id)?;
        if existing.agent_id != agent_id
            || existing.provider_agent_id != provider_agent_id
            || existing.delivery_id != delivery_id
            || existing.saved_path != saved_path
            || existing.deliverable_type != deliverable_type
        {
            anyhow::bail!("delivery context identity mismatch");
        }
        return Ok(existing);
    }
    let context = DeliveryContext {
        version: DELIVERY_CONTEXT_VERSION,
        job_id: job_id.to_string(),
        agent_id: agent_id.to_string(),
        provider_agent_id: provider_agent_id.to_string(),
        origin_session_key: origin_session_key.map(str::to_string),
        delivery_id: delivery_id.to_string(),
        saved_path: saved_path.to_string(),
        deliverable_type: deliverable_type.to_string(),
        received_at_ms,
        execution_path,
    };
    let body = serde_json::to_vec_pretty(&context)?;
    crate::home::write_secure(&path, &body)?;
    Ok(context)
}

pub fn load_delivery_context(job_id: &str, delivery_id: &str) -> anyhow::Result<DeliveryContext> {
    let path = delivery_context_path(job_id, delivery_id)?;
    let raw = std::fs::read(&path)?;
    let context: DeliveryContext = serde_json::from_slice(&raw)?;
    if !(1..=DELIVERY_CONTEXT_VERSION).contains(&context.version)
        || context.job_id != job_id
        || context.delivery_id != delivery_id
    {
        anyhow::bail!("delivery context mismatch");
    }
    Ok(context)
}

/// Bind a delivery decision to one previously registered delivery. This pointer
/// is what makes a reply recoverable when it arrives in a fresh model session.
pub fn activate_delivery_context(
    job_id: &str,
    delivery_id: &str,
) -> anyhow::Result<DeliveryContext> {
    let context = load_delivery_context(job_id, delivery_id)?;
    let path = pending_delivery_path(job_id)?;
    let body = serde_json::to_vec_pretty(&context)?;
    crate::home::write_secure(&path, &body)?;
    Ok(context)
}

/// Atomically bind the first delivery awaiting consent without replacing a
/// different outstanding delivery. This is the production entry point for
/// manual decisions; `activate_delivery_context` remains available for migration and
/// focused tests that intentionally replace the pointer.
pub fn activate_delivery_context_exclusive(
    job_id: &str,
    delivery_id: &str,
) -> anyhow::Result<DeliveryActivation> {
    let context = load_delivery_context(job_id, delivery_id)?;
    let path = pending_delivery_path(job_id)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("pending delivery path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    let body = serde_json::to_vec_pretty(&context)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(&body)?;
            file.flush()?;
            Ok(DeliveryActivation::Activated(context))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let pending = load_pending_delivery_context(job_id)?.ok_or_else(|| {
                anyhow::anyhow!("pending delivery disappeared during activation")
            })?;
            if pending.delivery_id == delivery_id {
                Ok(DeliveryActivation::AlreadyPending(pending))
            } else {
                Ok(DeliveryActivation::Conflict(pending))
            }
        }
        Err(error) => Err(error.into()),
    }
}

/// Load the exact delivery bound to the outstanding user decision.
pub fn load_pending_delivery_context(job_id: &str) -> anyhow::Result<Option<DeliveryContext>> {
    let path = pending_delivery_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path)?;
    let context: DeliveryContext = serde_json::from_slice(&raw)?;
    if !(1..=DELIVERY_CONTEXT_VERSION).contains(&context.version) || context.job_id != job_id {
        anyhow::bail!("pending delivery context mismatch");
    }
    Ok(Some(context))
}

/// Remove the delivery continuation pointer when the policy is paused/reset.
pub fn clear_pending_signal(job_id: &str) {
    if !job_id_is_safe(job_id) {
        return;
    }
    if let Ok(home) = crate::home::onchainos_home() {
        let path = home
            .join("autotrade")
            .join("pending")
            .join(format!("{job_id}.json"));
        let _ = std::fs::remove_file(path);
    }
}

/// Clear the continuation pointer only when it still refers to the delivery
/// that just reached a terminal outcome. This avoids removing a newer prompt
/// if cleanup races with another admitted signal.
pub fn clear_pending_delivery(job_id: &str, delivery_id: &str) {
    if load_pending_delivery_context(job_id)
        .ok()
        .flatten()
        .is_some_and(|context| context.delivery_id == delivery_id)
    {
        clear_pending_signal(job_id);
    }
}
/// Pause auto copy-trade for this job: delete the consent record so `evaluate_consent`
/// falls back to `FirstTime`. Later signals are saved and reported as not executed until
/// the user explicitly restores or updates the policy. Best-effort (already-absent is
/// fine). Grant file + pending signal are cleared by the caller.
pub fn clear_consent(job_id: &str) {
    if let Ok(path) = consent_path(job_id) {
        let _ = std::fs::remove_file(path);
    }
}

// ── Plugin-install approval (per job + plugin) ──────────────────────────────
//
// A copy-trade command that needs a plugin (e.g. `polymarket-plugin`) must NOT be
// silently installed inside the headless sub session: compliance requires the first
// install to be user-confirmed, and a sub-side `okx-dapp-discovery` install prompt is
// invisible to the human. Instead the pipeline defers with a plugin-install decision;
// once the user approves and the user session installs the plugin, this marker is
// written so subsequent signals for the same (job, plugin) execute without re-asking.

/// `<onchainos_home>/autotrade/plugin-approved/<jobId>/<plugin>`.
fn plugin_approved_path(job_id: &str, plugin: &str) -> Result<PathBuf, ConsentError> {
    if !job_id_is_safe(job_id) {
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    let safe_plugin: String = plugin
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_plugin.is_empty() {
        return Err(ConsentError(CONSENT_UNREADABLE));
    }
    let home = crate::home::onchainos_home().map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    Ok(home
        .join("autotrade")
        .join("plugin-approved")
        .join(job_id)
        .join(safe_plugin))
}

/// True when the user has approved installing `plugin` for `job_id`.
pub fn plugin_approved(job_id: &str, plugin: &str) -> bool {
    plugin_approved_path(job_id, plugin)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Record the user's approval to install `plugin` for `job_id` (best-effort marker).
pub fn write_plugin_approved(job_id: &str, plugin: &str) -> Result<(), ConsentError> {
    let path = plugin_approved_path(job_id, plugin)?;
    crate::home::write_secure(&path, b"1").map_err(|_| ConsentError(CONSENT_UNREADABLE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    /// Set ONCHAINOS_HOME to an isolated temp dir for the duration of a test.
    fn with_home<F: FnOnce()>(f: F) {
        // macOS may deny access to the process-wide temporary directory in
        // hardened test environments. Keep these fixtures under Cargo's local
        // target directory instead, which is already known to be writable.
        // Recover a poisoned mutex too, so one failed fixture does not turn all
        // subsequent tests into unrelated `PoisonError` failures.
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_root = std::env::current_dir()
            .expect("current working directory")
            .join("target")
            .join("consent-test-home");
        std::fs::create_dir_all(&temp_root).expect("create consent test temp root");
        let tmp = tempfile::tempdir_in(temp_root).expect("create consent test home");
        std::env::set_var("ONCHAINOS_HOME", tmp.path());
        f();
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn no_record_is_first_time() {
        with_home(|| {
            assert_eq!(
                evaluate_consent("job1", Some(&dec("10"))).unwrap(),
                ConsentDecision::FirstTime
            );
            assert_eq!(
                consent_snapshot("job1"),
                ConsentSnapshot {
                    status: ConsentSnapshotStatus::NotSet,
                    version: None,
                    mode: None,
                    cap_u: None,
                    trade_amount_u: None,
                    quote_token: None,
                    trade_environment: None,
                    margin_mode: None,
                    order_policy: None,
                    auth_mode: None,
                    dynamic_settings: DynamicConsentSettings::new(),
                    created_at: None,
                    expires_at: None,
                }
            );
        });
    }

    #[test]
    fn snapshot_exposes_only_persisted_policy() {
        with_home(|| {
            write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                Some("USDC"),
                3600,
            )
            .unwrap();

            let snapshot = consent_snapshot("job1");
            assert_eq!(snapshot.status, ConsentSnapshotStatus::Active);
            assert_eq!(snapshot.mode, Some(ConsentMode::Auto));
            assert_eq!(snapshot.cap_u.as_deref(), Some("50"));
            assert_eq!(snapshot.trade_amount_u.as_deref(), Some("12.5"));
            assert_eq!(snapshot.quote_token.as_deref(), Some("usdc"));
            assert_eq!(snapshot.trade_environment, None);
            assert_eq!(snapshot.margin_mode, None);
            assert_eq!(snapshot.order_policy, None);
            assert!(snapshot.created_at.is_some());
            assert!(snapshot.expires_at.is_some());
        });
    }

    #[test]
    fn stable_settings_remain_flat_and_extra_reaches_the_snapshot() {
        with_home(|| {
            let settings = parse_dynamic_settings_json(
                Some(
                    r#"{"leverageMode":"fixed","leverage":"2","mevProtection":true,"slippage":"0.5","extra":{"maxConcurrentPositions":{"label":"Maximum concurrent positions","type":"integer","value":3}}}"#,
                ),
                "--settings-json",
            )
            .unwrap();
            write_consent_policy_with_dynamic_settings(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                Some("USDC"),
                None,
                None,
                None,
                None,
                Some(&settings),
                3600,
            )
            .unwrap();

            let snapshot_json = serde_json::to_value(consent_snapshot("job1")).unwrap();
            assert_eq!(snapshot_json["leverage"], serde_json::json!("2"));
            assert_eq!(snapshot_json["mevProtection"], serde_json::json!(true));
            assert_eq!(snapshot_json["slippage"], serde_json::json!("0.5"));
            assert_eq!(
                snapshot_json["extra"]["maxConcurrentPositions"]["value"],
                serde_json::json!(3)
            );
            assert!(snapshot_json.get("dynamicSettings").is_none());

            let raw = std::fs::read_to_string(consent_path("job1").unwrap()).unwrap();
            let persisted: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(persisted["leverage"], serde_json::json!("2"));
            assert_eq!(
                persisted["extra"]["maxConcurrentPositions"]["type"],
                serde_json::json!("integer")
            );
            assert!(persisted.get("dynamicSettings").is_none());
        });
    }

    #[test]
    fn extra_requires_label_type_value_and_validates_optional_metadata() {
        for raw in [
            r#"{"extra":{"foo":{"type":"integer","value":1}}}"#,
            r#"{"extra":{"foo":{"label":"Foo","value":1}}}"#,
            r#"{"extra":{"foo":{"label":"Foo","type":"integer"}}}"#,
            r#"{"extra":{"foo":{"label":"Foo","type":"integer","value":"1"}}}"#,
        ] {
            assert!(parse_dynamic_settings_json(Some(raw), "--settings-json").is_err());
        }

        let settings = parse_dynamic_settings_json(
            Some(
                r#"{"extra":{"orderUrgency":{"label":"Order urgency","type":"enum","value":"fast","description":null,"unit":null,"constraints":null,"options":["normal","fast"],"appliesWhen":{"venue":"hyperliquid","operation":["open","close"]},"confirmedAt":1787900000}}}"#,
            ),
            "--settings-json",
        )
        .unwrap();
        assert!(dynamic_setting_present(&settings, "extra.orderUrgency"));

        let precise_identifier = parse_dynamic_settings_json(
            Some(
                r#"{"extra":{"strategyInstanceId":{"label":"Strategy instance ID","type":"string","value":"90071992547409931234567890"}}}"#,
            ),
            "--settings-json",
        )
        .unwrap();
        assert_eq!(
            precise_identifier["extra"]["strategyInstanceId"]["value"],
            serde_json::json!("90071992547409931234567890")
        );
    }

    #[test]
    fn extra_updates_merge_without_dropping_existing_fields() {
        with_home(|| {
            let first = parse_dynamic_settings_json(
                Some(
                    r#"{"extra":{"maxConcurrentPositions":{"label":"Maximum concurrent positions","type":"integer","value":3}}}"#,
                ),
                "--settings-json",
            )
            .unwrap();
            write_consent_policy_with_dynamic_settings(
                "job1",
                ConsentMode::Manual,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&first),
                3600,
            )
            .unwrap();
            let second = parse_dynamic_settings_json(
                Some(
                    r#"{"extra":{"orderUrgency":{"label":"Order urgency","type":"enum","value":"fast","options":["normal","fast"]}}}"#,
                ),
                "--settings-json",
            )
            .unwrap();
            write_trade_settings_with_dynamic("job1", None, None, None, None, Some(&second))
                .unwrap();

            let stored = load_consent("job1").unwrap().unwrap();
            assert!(dynamic_setting_present(
                &stored.dynamic_settings,
                "extra.maxConcurrentPositions"
            ));
            assert!(dynamic_setting_present(
                &stored.dynamic_settings,
                "extra.orderUrgency"
            ));
        });
    }

    #[test]
    fn legacy_v5_unknown_flat_setting_migrates_to_typed_extra_in_memory() {
        with_home(|| {
            let raw = serde_json::json!({
                "version": 5,
                "jobId": "job1",
                "mode": "manual",
                "legacyRiskBucket": 3,
                "createdAt": now_secs(),
                "expiresAt": now_secs() + 3600,
            });
            crate::home::write_secure(
                &consent_path("job1").unwrap(),
                serde_json::to_string_pretty(&raw).unwrap().as_bytes(),
            )
            .unwrap();

            let stored = load_consent("job1").unwrap().unwrap();
            assert_eq!(stored.version, 5);
            assert_eq!(
                stored.dynamic_settings["extra"]["legacyRiskBucket"],
                serde_json::json!({
                    "label": "legacyRiskBucket",
                    "type": "integer",
                    "value": 3,
                })
            );
            assert!(!stored.dynamic_settings.contains_key("legacyRiskBucket"));
        });
    }

    #[test]
    fn required_fields_and_service_guide_hash_are_validated_and_persisted() {
        with_home(|| {
            let settings = parse_dynamic_settings_json(
                Some(
                    r#"{"serviceGuideHash":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","requiredFields":["tradeAmountMode","tradeAmountBasis","extra.maxConcurrentPositions"],"tradeAmountMode":"fixed_amount","tradeAmountBasis":"margin","extra":{"maxConcurrentPositions":{"label":"Maximum concurrent positions","type":"integer","value":3}}}"#,
                ),
                "--settings-json",
            )
            .unwrap();
            write_consent_policy_with_dynamic_settings(
                "job1",
                ConsentMode::Auto,
                Some("100"),
                Some("10"),
                Some("USDT"),
                None,
                None,
                None,
                None,
                Some(&settings),
                3600,
            )
            .unwrap();
            let snapshot = serde_json::to_value(consent_snapshot("job1")).unwrap();
            assert_eq!(snapshot["requiredFields"][0], "tradeAmountMode");
            assert_eq!(snapshot["requiredFields"][1], "tradeAmountBasis");
            assert_eq!(snapshot["tradeAmountBasis"], "margin");
            assert_eq!(snapshot["serviceGuideHash"], settings["serviceGuideHash"]);
        });
    }

    #[test]
    fn dynamic_settings_cannot_shadow_core_or_store_credentials() {
        for raw in [
            r#"{"authMode":"oauth"}"#,
            r#"{"apiKey":"secret"}"#,
            r#"{"createdAt":1}"#,
        ] {
            assert!(parse_dynamic_settings_json(Some(raw), "--settings-json").is_err());
        }
    }

    #[test]
    fn canonical_percentage_and_risk_overrides_are_validated() {
        let settings = parse_dynamic_settings_json(
            Some(
                r#"{"tradeAmountMode":"available_balance_ratio","tradeAmountRatio":"0.125","takeProfitRatio":"20","stopLossRatio":"5"}"#,
            ),
            "--settings-json",
        )
        .unwrap();
        validate_amount_policy(None, &settings).unwrap();
        assert!(uses_percentage_amount(&settings));
        assert_eq!(
            dynamic_decimal_setting(&settings, "tradeAmountRatio")
                .unwrap()
                .unwrap()
                .to_plain_string(),
            "0.125"
        );

        let missing_type = parse_dynamic_settings_json(
            Some(r#"{"tradeAmountRatio":"0.25"}"#),
            "--settings-json",
        )
        .unwrap();
        assert!(validate_amount_policy(None, &missing_type).is_err());
        assert!(parse_dynamic_settings_json(
            Some(r#"{"tradeAmountMode":"available_balance_ratio","tradeAmountRatio":"1.01"}"#),
            "--settings-json",
        )
        .is_err());
        assert!(parse_dynamic_settings_json(
            Some(r#"{"takeProfitRatio":"0"}"#),
            "--settings-json",
        )
        .is_err());
    }

    #[test]
    fn null_dynamic_setting_removes_an_existing_value() {
        with_home(|| {
            let initial = parse_dynamic_settings_json(
                Some(r#"{"leverageMode":"fixed","leverage":"2","slippage":"0.5"}"#),
                "--settings-json",
            )
            .unwrap();
            write_consent_policy_with_dynamic_settings(
                "job1",
                ConsentMode::Manual,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&initial),
                3600,
            )
            .unwrap();
            let removal = parse_dynamic_settings_json(
                Some(r#"{"slippage":null}"#),
                "--settings-json",
            )
            .unwrap();
            write_trade_settings_with_dynamic("job1", None, None, None, None, Some(&removal))
                .unwrap();

            let stored = load_consent("job1").unwrap().unwrap();
            assert!(!stored.dynamic_settings.contains_key("slippage"));
            assert_eq!(stored.dynamic_settings["leverage"], serde_json::json!("2"));
        });
    }

    #[test]
    fn null_extra_entry_removes_only_the_named_dynamic_field() {
        with_home(|| {
            let initial = parse_dynamic_settings_json(
                Some(
                    r#"{"extra":{"maxConcurrentPositions":{"label":"Maximum concurrent positions","type":"integer","value":3},"orderUrgency":{"label":"Order urgency","type":"enum","value":"fast","options":["normal","fast"]}}}"#,
                ),
                "--settings-json",
            )
            .unwrap();
            write_consent_policy_with_dynamic_settings(
                "job1",
                ConsentMode::Manual,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&initial),
                3600,
            )
            .unwrap();

            let removal = parse_dynamic_settings_json(
                Some(r#"{"extra":{"orderUrgency":null}}"#),
                "--settings-json",
            )
            .unwrap();
            write_trade_settings_with_dynamic("job1", None, None, None, None, Some(&removal))
                .unwrap();

            let stored = load_consent("job1").unwrap().unwrap();
            assert!(dynamic_setting_present(
                &stored.dynamic_settings,
                "extra.maxConcurrentPositions"
            ));
            assert!(!dynamic_setting_present(
                &stored.dynamic_settings,
                "extra.orderUrgency"
            ));
        });
    }

    #[test]
    fn trade_environment_upgrade_preserves_existing_policy_and_snapshot_exposes_it() {
        with_home(|| {
            write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                Some("USDC"),
                3600,
            )
            .unwrap();
            let before = load_consent("job1").unwrap().unwrap();

            let upgraded = write_trade_environment("job1", TradeEnvironment::Demo).unwrap();
            assert_eq!(upgraded.version, CONSENT_VERSION);
            assert_eq!(upgraded.mode, before.mode);
            assert_eq!(upgraded.cap_u, before.cap_u);
            assert_eq!(upgraded.trade_amount_u, before.trade_amount_u);
            assert_eq!(upgraded.quote_token, before.quote_token);
            assert_eq!(upgraded.created_at, before.created_at);
            assert_eq!(upgraded.expires_at, before.expires_at);
            assert_eq!(upgraded.trade_environment, Some(TradeEnvironment::Demo));
            assert_eq!(
                consent_snapshot("job1").trade_environment,
                Some(TradeEnvironment::Demo)
            );

            write_consent("job1", ConsentMode::Auto, Some("75"), None, 1800).unwrap();
            let rewritten = load_consent("job1").unwrap().unwrap();
            assert_eq!(rewritten.cap_u.as_deref(), Some("75"));
            assert_eq!(rewritten.trade_environment, Some(TradeEnvironment::Demo));
        });
    }

    #[test]
    fn trade_settings_update_preserves_policy_and_snapshot_exposes_all_settings() {
        with_home(|| {
            write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                Some("USDC"),
                3600,
            )
            .unwrap();
            let before = load_consent("job1").unwrap().unwrap();

            let updated = write_trade_settings(
                "job1",
                Some(TradeEnvironment::Live),
                Some(MarginMode::Cross),
                Some(OrderPolicy::SignalPriceLimit),
                Some(TradeKitAuthMode::OAuth),
            )
            .unwrap();
            assert_eq!(updated.mode, before.mode);
            assert_eq!(updated.cap_u, before.cap_u);
            assert_eq!(updated.trade_amount_u, before.trade_amount_u);
            assert_eq!(updated.quote_token, before.quote_token);
            assert_eq!(updated.created_at, before.created_at);
            assert_eq!(updated.expires_at, before.expires_at);

            let snapshot = consent_snapshot("job1");
            assert_eq!(snapshot.trade_environment, Some(TradeEnvironment::Live));
            assert_eq!(snapshot.margin_mode, Some(MarginMode::Cross));
            assert_eq!(snapshot.order_policy, Some(OrderPolicy::SignalPriceLimit));
            assert_eq!(snapshot.auth_mode, Some(TradeKitAuthMode::OAuth));
        });
    }

    #[test]
    fn legacy_consent_without_auth_mode_is_readable_and_can_be_upgraded() {
        with_home(|| {
            let path = consent_path("job1").unwrap();
            let now = now_secs();
            let legacy = serde_json::json!({
                "version": 3,
                "jobId": "job1",
                "mode": "auto",
                "capU": "50",
                "tradeAmountU": "10",
                "quoteToken": "usdt",
                "tradeEnvironment": "live",
                "marginMode": "cross",
                "orderPolicy": "market",
                "createdAt": now,
                "expiresAt": now + 3600
            });
            crate::home::write_secure(&path, serde_json::to_string_pretty(&legacy).unwrap().as_bytes())
                .unwrap();

            let legacy_snapshot = consent_snapshot("job1");
            assert_eq!(legacy_snapshot.version, Some(3));
            assert_eq!(legacy_snapshot.auth_mode, None);

            let upgraded = write_trade_settings(
                "job1",
                None,
                None,
                None,
                Some(TradeKitAuthMode::OAuth),
            )
            .unwrap();
            assert_eq!(upgraded.version, CONSENT_VERSION);
            assert_eq!(upgraded.auth_mode, Some(TradeKitAuthMode::OAuth));
            assert_eq!(upgraded.trade_environment, Some(TradeEnvironment::Live));
            assert_eq!(upgraded.margin_mode, Some(MarginMode::Cross));
            assert_eq!(upgraded.order_policy, Some(OrderPolicy::Market));
        });
    }

    #[test]
    fn snapshot_marks_broken_policy_unreadable() {
        with_home(|| {
            let path = consent_path("job1").unwrap();
            crate::home::write_secure(&path, b"not-json").unwrap();
            assert_eq!(
                consent_snapshot("job1").status,
                ConsentSnapshotStatus::Unreadable
            );
        });
    }

    #[test]
    fn quote_preference_defaults_persists_and_survives_cap_rewrite() {
        with_home(|| {
            // No record ⇒ the default (PRD denominates in USDT).
            assert_eq!(quote_token("job1"), "usdt");
            // Explicit choice is stored, case-normalized.
            write_consent("job1", ConsentMode::Auto, Some("50"), Some("USDC"), 3600).unwrap();
            assert_eq!(quote_token("job1"), "usdc");
            // Over-cap raise re-writes the record WITHOUT --quote ⇒ choice survives.
            write_consent("job1", ConsentMode::Auto, Some("200"), None, 3600).unwrap();
            assert_eq!(quote_token("job1"), "usdc");
            // Whitelist: arbitrary tokens can't bend the cap semantics.
            assert!(
                write_consent("job1", ConsentMode::Auto, Some("50"), Some("dai"), 3600).is_err()
            );
            // Manual (B) can carry a preference too.
            write_consent("job1", ConsentMode::Manual, None, Some("usdt"), 3600).unwrap();
            assert_eq!(quote_token("job1"), "usdt");
        });
    }

    #[test]
    fn trade_amount_is_optional_backward_compatible_and_survives_legacy_rewrite() {
        with_home(|| {
            // A v1 file written before tradeAmountU existed remains readable.
            let created_at = now_secs();
            let legacy = serde_json::json!({
                "version": CONSENT_VERSION,
                "jobId": "job1",
                "mode": "auto",
                "capU": "50",
                "createdAt": created_at,
                "expiresAt": created_at + 3600
            });
            let path = consent_path("job1").unwrap();
            crate::home::write_secure(&path, legacy.to_string().as_bytes()).unwrap();
            assert_eq!(load_consent("job1").unwrap().unwrap().trade_amount_u, None);

            write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("12.5"),
                None,
                3600,
            )
            .unwrap();
            assert_eq!(
                load_consent("job1")
                    .unwrap()
                    .unwrap()
                    .trade_amount_u
                    .as_deref(),
                Some("12.5")
            );

            // Existing callers use write_consent (no amount argument). A cap-only
            // rewrite must preserve the fixed amount rather than erasing it.
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 3600).unwrap();
            let saved = load_consent("job1").unwrap().unwrap();
            assert_eq!(saved.cap_u.as_deref(), Some("100"));
            assert_eq!(saved.trade_amount_u.as_deref(), Some("12.5"));
        });
    }

    #[test]
    fn write_consent_validates_fixed_trade_amount() {
        with_home(|| {
            for invalid in ["", "abc", "0"] {
                assert!(write_consent_with_trade_amount(
                    "job1",
                    ConsentMode::Auto,
                    Some("50"),
                    Some(invalid),
                    None,
                    3600,
                )
                .is_err());
            }
            assert!(write_consent_with_trade_amount(
                "job1",
                ConsentMode::Auto,
                Some("50"),
                Some("0.01"),
                None,
                3600,
            )
            .is_ok());
        });
    }

    #[test]
    fn delivery_context_round_trips_and_activates_exact_delivery() {
        with_home(|| {
            let saved = register_delivery_context(
                "job1",
                "1506",
                "8779",
                Some("job:job1:my:1506:to:8779"),
                "msg:abc123",
                "/tmp/signal.txt",
                "text",
                1234,
            )
            .unwrap();
            assert_eq!(saved.delivery_id, "msg:abc123");
            assert_eq!(saved.execution_path, SubscriptionTradePath::LegacyWrapper);
            assert_eq!(
                saved.origin_session_key.as_deref(),
                Some("job:job1:my:1506:to:8779")
            );
            assert_eq!(load_pending_delivery_context("job1").unwrap(), None);

            let active = activate_delivery_context("job1", "msg:abc123").unwrap();
            assert_eq!(active, saved);
            assert_eq!(load_pending_delivery_context("job1").unwrap(), Some(saved));
            assert!(activate_delivery_context("job1", "msg:missing").is_err());
        });
    }

    #[test]
    fn delivery_execution_path_is_pinned_and_v1_contexts_remain_legacy() {
        with_home(|| {
            let direct = register_delivery_context_with_path(
                "job1",
                "1506",
                "8779",
                None,
                "msg:direct",
                "/tmp/direct.txt",
                "text",
                1234,
                SubscriptionTradePath::AgentDirect,
            )
            .unwrap();
            assert_eq!(direct.execution_path, SubscriptionTradePath::AgentDirect);

            let replay = register_delivery_context_with_path(
                "job1",
                "1506",
                "8779",
                Some("a-new-session-must-not-change-the-path"),
                "msg:direct",
                "/tmp/direct.txt",
                "text",
                9999,
                SubscriptionTradePath::LegacyWrapper,
            )
            .unwrap();
            assert_eq!(replay.execution_path, SubscriptionTradePath::AgentDirect);
            assert_eq!(replay.received_at_ms, 1234);

            let v1 = serde_json::json!({
                "version": 1,
                "jobId": "job1",
                "agentId": "1506",
                "providerAgentId": "8779",
                "deliveryId": "msg:v1",
                "savedPath": "/tmp/v1.txt",
                "deliverableType": "text",
                "receivedAtMs": 1234
            });
            let path = delivery_context_path("job1", "msg:v1").unwrap();
            crate::home::write_secure(&path, v1.to_string().as_bytes()).unwrap();
            assert_eq!(
                load_delivery_context("job1", "msg:v1")
                    .unwrap()
                    .execution_path,
                SubscriptionTradePath::LegacyWrapper
            );
        });
    }

    #[test]
    fn exclusive_activation_never_overwrites_a_different_pending_delivery() {
        with_home(|| {
            for delivery_id in ["delivery-1", "delivery-2"] {
                register_delivery_context(
                    "job1",
                    "1506",
                    "8779",
                    Some("job:job1:my:1506:to:8779"),
                    delivery_id,
                    &format!("/tmp/{delivery_id}.txt"),
                    "text",
                    1234,
                )
                .unwrap();
            }
            assert!(matches!(
                activate_delivery_context_exclusive("job1", "delivery-1").unwrap(),
                DeliveryActivation::Activated(_)
            ));
            assert!(matches!(
                activate_delivery_context_exclusive("job1", "delivery-1").unwrap(),
                DeliveryActivation::AlreadyPending(_)
            ));
            let conflict =
                activate_delivery_context_exclusive("job1", "delivery-2").unwrap();
            assert!(matches!(
                conflict,
                DeliveryActivation::Conflict(ref pending)
                    if pending.delivery_id == "delivery-1"
            ));
            assert_eq!(
                load_pending_delivery_context("job1")
                    .unwrap()
                    .unwrap()
                    .delivery_id,
                "delivery-1"
            );
            clear_pending_delivery("job1", "delivery-2");
            assert!(load_pending_delivery_context("job1").unwrap().is_some());
            clear_pending_delivery("job1", "delivery-1");
            assert!(matches!(
                activate_delivery_context_exclusive("job1", "delivery-2").unwrap(),
                DeliveryActivation::Activated(_)
            ));
        });
    }

    #[test]
    fn decline_maps_to_declined_decision() {
        with_home(|| {
            // A Decline record maps to ConsentDecision::Declined; the delivery
            // pipeline saves the artifact and reports a terminal skip.
            write_consent("job1", ConsentMode::Decline, None, None, 3600).unwrap();
            assert_eq!(
                evaluate_consent("job1", None).unwrap(),
                ConsentDecision::Declined
            );
        });
    }

    #[test]
    fn manual_surfaces_command() {
        with_home(|| {
            write_consent("job1", ConsentMode::Manual, None, None, 3600).unwrap();
            assert_eq!(
                evaluate_consent("job1", Some(&dec("10"))).unwrap(),
                ConsentDecision::Manual
            );
        });
    }

    #[test]
    fn clear_consent_pauses_to_unconfigured_state() {
        with_home(|| {
            // Auto authorized within cap → auto-executes.
            write_consent("job1", ConsentMode::Auto, Some("10"), None, 3600).unwrap();
            assert_eq!(
                evaluate_consent("job1", Some(&dec("5"))).unwrap(),
                ConsentDecision::AutoAllow
            );
            // Pause ("暂停自动跟单"): clearing the consent record reverts to
            // FirstTime, which the delivery pipeline handles as a saved skip.
            clear_consent("job1");
            assert!(load_consent("job1").unwrap().is_none());
            assert_eq!(
                evaluate_consent("job1", Some(&dec("5"))).unwrap(),
                ConsentDecision::FirstTime
            );
        });
    }

    #[test]
    fn auto_within_cap_allows_and_over_cap_reasks() {
        with_home(|| {
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 3600).unwrap();
            // buy within cap ⇒ allow (boundary inclusive)
            assert_eq!(
                evaluate_consent("job1", Some(&dec("100"))).unwrap(),
                ConsentDecision::AutoAllow
            );
            // buy over cap ⇒ re-ask
            assert_eq!(
                evaluate_consent("job1", Some(&dec("100.01"))).unwrap(),
                ConsentDecision::AutoOverCap
            );
            // sell (no buy amount) ⇒ always allowed under Auto
            assert_eq!(
                evaluate_consent("job1", None).unwrap(),
                ConsentDecision::AutoAllow
            );
        });
    }

    #[test]
    fn expired_record_is_first_time() {
        with_home(|| {
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 1).unwrap();
            // Force expiry into the past.
            let path = consent_path("job1").unwrap();
            let mut file: ConsentFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            file.expires_at = 1;
            std::fs::write(&path, serde_json::to_string(&file).unwrap()).unwrap();
            assert_eq!(
                evaluate_consent("job1", Some(&dec("10"))).unwrap(),
                ConsentDecision::FirstTime
            );
        });
    }

    #[test]
    fn version_too_new_and_job_mismatch_fail_closed() {
        with_home(|| {
            write_consent("job1", ConsentMode::Auto, Some("100"), None, 3600).unwrap();
            let path = consent_path("job1").unwrap();
            let good: ConsentFile =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

            let mut newer = good.clone();
            newer.version = CONSENT_VERSION + 1;
            std::fs::write(&path, serde_json::to_string(&newer).unwrap()).unwrap();
            assert_eq!(
                load_consent("job1").unwrap_err(),
                ConsentError(CONSENT_VERSION_TOO_NEW)
            );

            let mut configured = good.clone();
            configured.trade_environment = Some(TradeEnvironment::Configured);
            std::fs::write(&path, serde_json::to_string(&configured).unwrap()).unwrap();
            assert_eq!(
                load_consent("job1").unwrap_err(),
                ConsentError(CONSENT_UNREADABLE)
            );

            let mut mism = good;
            mism.job_id = "other".into();
            std::fs::write(&path, serde_json::to_string(&mism).unwrap()).unwrap();
            assert_eq!(
                load_consent("job1").unwrap_err(),
                ConsentError(CONSENT_JOB_MISMATCH)
            );
        });
    }

    #[test]
    fn write_consent_validates_cap_and_mode() {
        with_home(|| {
            // Auto permits no cap; a supplied cap must be positive and parseable.
            assert!(write_consent("job1", ConsentMode::Auto, None, None, 3600).is_ok());
            assert!(write_consent("job1", ConsentMode::Auto, Some("abc"), None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Auto, Some("0"), None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Auto, Some("50"), None, 3600).is_ok());
            // Manual / Decline must NOT carry a cap.
            assert!(write_consent("job1", ConsentMode::Manual, Some("50"), None, 3600).is_err());
            assert!(write_consent("job1", ConsentMode::Manual, None, None, 3600).is_ok());
            assert!(write_consent("job1", ConsentMode::Decline, None, None, 3600).is_ok());
            // ttl must be > 0.
            assert!(write_consent("job1", ConsentMode::Decline, None, None, 0).is_err());
        });
    }

    #[test]
    fn plugin_approved_marker_is_per_job_and_plugin() {
        with_home(|| {
            assert!(!plugin_approved("job1", "polymarket-plugin"));
            write_plugin_approved("job1", "polymarket-plugin").unwrap();
            assert!(plugin_approved("job1", "polymarket-plugin"));
            // Independent per (job, plugin): a different plugin or job is NOT approved.
            assert!(!plugin_approved("job1", "hyperliquid-plugin"));
            assert!(!plugin_approved("job2", "polymarket-plugin"));
        });
    }
}
