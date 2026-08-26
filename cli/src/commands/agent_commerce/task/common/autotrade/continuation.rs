//! Short-lived state for incomplete auto-trade configuration.
//!
//! This record is deliberately separate from consent and pending/A2A decisions:
//! it remembers which mode the user selected and the explicit values collected
//! so far, but it never authorizes or executes a trade. A successful
//! `autotrade-consent-set` write consumes it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::amount::Decimal;
use super::consent::{ConsentFile, ConsentMode, MarginMode, OrderPolicy, QUOTE_WHITELIST};
use super::grants::job_id_is_safe;
use super::trade_kit::TradeEnvironment;

const VERSION: u32 = 4;
const TTL_SECS: u64 = 30 * 60;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SelectedMode {
    Auto,
    Manual,
}

impl SelectedMode {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "manual" => Ok(Self::Manual),
            _ => anyhow::bail!("--mode must be one of: auto | manual"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    PreDelivery,
    Delivery,
    SubscriptionRestore,
}

impl Origin {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pre-delivery" => Ok(Self::PreDelivery),
            "delivery" => Ok(Self::Delivery),
            "subscription-restore" => Ok(Self::SubscriptionRestore),
            _ => anyhow::bail!(
                "--origin must be one of: pre-delivery | delivery | subscription-restore"
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsentContinuation {
    version: u32,
    pub continuation_id: String,
    pub job_id: String,
    pub agent_id: String,
    pub selected_mode: SelectedMode,
    #[serde(default)]
    pub mode_confirmed: bool,
    pub origin: Origin,
    pub signal_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_delivery_id: Option<String>,
    pub required_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_amount_u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap_u: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_environment: Option<TradeEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_mode: Option<MarginMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_policy: Option<OrderPolicy>,
    created_at: u64,
    expires_at: u64,
}

impl ConsentContinuation {
    pub fn missing_fields(&self) -> Vec<String> {
        let mut missing: Vec<String> = self
            .required_fields
            .iter()
            .filter(|field| match field.as_str() {
                "tradeAmount" | "cap" | "quote"
                    if self.selected_mode == SelectedMode::Manual =>
                {
                    false
                }
                "tradeAmount" => self.trade_amount_u.is_none(),
                "cap" => self.cap_u.is_none(),
                "quote" => self.quote_token.is_none(),
                "environment" => self.trade_environment.is_none(),
                "marginMode" => self.margin_mode.is_none(),
                "orderPolicy" => self.order_policy.is_none(),
                _ => true,
            })
            .cloned()
            .collect();
        if self.origin == Origin::SubscriptionRestore && !self.mode_confirmed {
            missing.insert(0, "mode".to_string());
        }
        missing
    }
}

#[derive(Debug)]
pub struct StartBinding<'a> {
    pub job_id: &'a str,
    pub agent_id: &'a str,
    pub selected_mode: SelectedMode,
    pub mode_confirmed: bool,
    pub origin: Origin,
    pub signal_type: &'a str,
    pub original_delivery_id: Option<&'a str>,
    /// Fields selected from the untrusted ASP description. They control only
    /// which user-authored values remain missing; they never supply values or
    /// grant authorization. Valid only for subscription restoration.
    pub required_fields: Option<&'a [String]>,
    /// Trusted existing policy used only to prefill an upgrade/repair attempt.
    /// The continuation still requires an explicit mode confirmation and never
    /// treats these values as a new authorization by itself.
    pub seed_consent: Option<&'a ConsentFile>,
}

#[derive(Debug, Default)]
pub struct ExplicitValues<'a> {
    pub trade_amount_u: Option<&'a str>,
    pub cap_u: Option<&'a str>,
    pub quote_token: Option<&'a str>,
    pub trade_environment: Option<&'a str>,
    pub margin_mode: Option<&'a str>,
    pub order_policy: Option<&'a str>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationResult {
    pub continuation_id: String,
    pub job_id: String,
    pub agent_id: String,
    pub selected_mode: SelectedMode,
    pub mode_confirmed: bool,
    pub origin: Origin,
    pub signal_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_delivery_id: Option<String>,
    pub required_fields: Vec<String>,
    pub missing_fields: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub validation_errors: Vec<ValidationError>,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_command: Option<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub field: String,
    pub code: String,
    pub message: String,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn continuation_path(job_id: &str) -> anyhow::Result<PathBuf> {
    if !job_id_is_safe(job_id) {
        anyhow::bail!("invalid job id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("consent-continuation")
        .join(format!("{job_id}.json")))
}

fn new_id() -> String {
    format!("atc_{}", uuid::Uuid::new_v4().simple())
}

pub fn continuation_id_is_safe(value: &str) -> bool {
    value.len() == 36
        && value.starts_with("atc_")
        && value[4..].chars().all(|c| c.is_ascii_hexdigit())
}

fn default_required_fields(mode: SelectedMode, origin: Origin) -> Vec<String> {
    match (mode, origin) {
        (SelectedMode::Auto, Origin::PreDelivery | Origin::Delivery) => {
            ["tradeAmount", "cap", "quote"]
                .into_iter()
                .map(str::to_string)
                .collect()
        }
        (SelectedMode::Auto, Origin::SubscriptionRestore) => Vec::new(),
        (SelectedMode::Manual, Origin::Delivery) => vec!["tradeAmount".to_string()],
        (SelectedMode::Manual, Origin::PreDelivery | Origin::SubscriptionRestore) => Vec::new(),
    }
}

fn normalize_required_fields(values: &[String]) -> anyhow::Result<Vec<String>> {
    let mut normalized = Vec::new();
    for value in values {
        let field = match value.as_str() {
            "tradeAmount" | "trade_amount" | "amount" => "tradeAmount",
            "cap" => "cap",
            "quote" | "quoteToken" | "quote_token" => "quote",
            "environment" | "tradeEnvironment" | "trade_environment" => "environment",
            "marginMode" | "margin_mode" => "marginMode",
            "orderPolicy" | "order_policy" => "orderPolicy",
            _ => anyhow::bail!(
                "--required-field must be one of: tradeAmount | cap | quote | environment | marginMode | orderPolicy"
            ),
        };
        if !normalized.iter().any(|existing| existing == field) {
            normalized.push(field.to_string());
        }
    }
    Ok(normalized)
}

fn validate_binding(binding: &StartBinding<'_>) -> anyhow::Result<()> {
    if !job_id_is_safe(binding.job_id) {
        anyhow::bail!("invalid job id");
    }
    if binding.agent_id.is_empty()
        || !binding
            .agent_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        anyhow::bail!("invalid agent id");
    }
    if binding.signal_type.is_empty()
        || !binding
            .signal_type
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        anyhow::bail!("invalid signal type");
    }
    match (binding.origin, binding.original_delivery_id) {
        (Origin::PreDelivery | Origin::SubscriptionRestore, Some(_)) => {
            anyhow::bail!(
                "--delivery-id cannot be used with --origin pre-delivery or subscription-restore"
            )
        }
        (Origin::Delivery, Some(value)) if !value.trim().is_empty() => {}
        (Origin::Delivery, _) => {
            anyhow::bail!("--delivery-id is required with --origin delivery")
        }
        _ => {}
    }
    match (binding.origin, binding.required_fields) {
        (Origin::SubscriptionRestore, Some(fields)) => {
            normalize_required_fields(fields)?;
        }
        (Origin::SubscriptionRestore, None) => {
            anyhow::bail!("--required-field binding is required for subscription restoration")
        }
        (_, Some(_)) => {
            anyhow::bail!("--required-field is valid only with --origin subscription-restore")
        }
        (_, None) => {}
    }
    match (binding.origin, binding.seed_consent) {
        (Origin::SubscriptionRestore, Some(consent)) => {
            if consent.job_id != binding.job_id || consent.mode == ConsentMode::Decline {
                anyhow::bail!("restore seed consent does not match the requested authorization");
            }
        }
        (Origin::SubscriptionRestore, None) => {}
        (_, Some(_)) => {
            anyhow::bail!("seed consent is valid only for subscription restoration")
        }
        (_, None) => {}
    }
    Ok(())
}

fn parse_positive(value: &str, flag: &str) -> anyhow::Result<String> {
    let parsed =
        Decimal::parse(value).map_err(|_| anyhow::anyhow!("{flag} is not a valid decimal"))?;
    if parsed.is_zero() {
        anyhow::bail!("{flag} must be greater than 0");
    }
    Ok(parsed.to_plain_string())
}

fn normalize_quote(value: &str) -> anyhow::Result<String> {
    let value = value.to_ascii_lowercase();
    if !QUOTE_WHITELIST.contains(&value.as_str()) {
        anyhow::bail!("--quote must be one of: usdc | usdt");
    }
    Ok(value)
}

fn read_live(job_id: &str) -> anyhow::Result<Option<ConsentContinuation>> {
    let path = continuation_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| anyhow::anyhow!("consent continuation is unreadable"))?;
    let file: ConsentContinuation = serde_json::from_str(&raw)
        .map_err(|_| anyhow::anyhow!("consent continuation is unreadable"))?;
    if file.version > VERSION || file.job_id != job_id {
        anyhow::bail!("consent continuation is unreadable");
    }
    if file.expires_at <= now_secs() {
        let _ = std::fs::remove_file(path);
        return Ok(None);
    }
    Ok(Some(file))
}

pub fn load_for_resume(
    job_id: &str,
    agent_id: &str,
    continuation_id: &str,
) -> anyhow::Result<ConsentContinuation> {
    if !continuation_id_is_safe(continuation_id) {
        anyhow::bail!("invalid consent continuation id");
    }
    let file = read_live(job_id)?
        .ok_or_else(|| anyhow::anyhow!("no live consent continuation for this job"))?;
    if file.agent_id != agent_id {
        anyhow::bail!("consent continuation agent does not match");
    }
    if continuation_id != file.continuation_id {
        anyhow::bail!("consent continuation id does not match");
    }
    Ok(file)
}

pub fn load_live_for_job(
    job_id: &str,
    agent_id: &str,
) -> anyhow::Result<Option<ConsentContinuation>> {
    let Some(file) = read_live(job_id)? else {
        return Ok(None);
    };
    if file.agent_id != agent_id {
        anyhow::bail!("consent continuation agent does not match");
    }
    Ok(Some(file))
}

pub fn start_or_update(
    start: Option<StartBinding<'_>>,
    job_id: &str,
    agent_id: &str,
    continuation_id: Option<&str>,
    selected_mode: Option<SelectedMode>,
    values: ExplicitValues<'_>,
) -> anyhow::Result<ContinuationResult> {
    if start
        .as_ref()
        .is_some_and(|binding| binding.job_id != job_id || binding.agent_id != agent_id)
    {
        anyhow::bail!("consent continuation binding does not match the requested job and agent");
    }
    let existing = read_live(job_id)?;
    let (file, is_new) = match (existing, start) {
        (Some(file), Some(binding)) => {
            validate_binding(&binding)?;
            if file.agent_id != binding.agent_id
                || file.selected_mode != binding.selected_mode
                || file.origin != binding.origin
                || file.signal_type != binding.signal_type
                || file.original_delivery_id.as_deref() != binding.original_delivery_id
            {
                anyhow::bail!("a different live consent continuation already exists for this job");
            }
            let continuation_id = continuation_id.ok_or_else(|| {
                anyhow::anyhow!(
                    "--continuation-id is required because a live continuation already exists"
                )
            })?;
            if continuation_id != file.continuation_id {
                anyhow::bail!("consent continuation id does not match");
            }
            (file, false)
        }
        (Some(file), None) => {
            if file.agent_id != agent_id {
                anyhow::bail!("consent continuation agent does not match");
            }
            let continuation_id = continuation_id.ok_or_else(|| {
                anyhow::anyhow!("--continuation-id is required when resuming a continuation")
            })?;
            if continuation_id != file.continuation_id {
                anyhow::bail!("consent continuation id does not match");
            }
            (file, false)
        }
        (None, Some(binding)) => {
            validate_binding(&binding)?;
            if continuation_id.is_some() {
                anyhow::bail!("--continuation-id cannot create a new continuation");
            }
            let now = now_secs();
            let seed = binding.seed_consent;
            (
                ConsentContinuation {
                    version: VERSION,
                    continuation_id: new_id(),
                    job_id: binding.job_id.to_string(),
                    agent_id: binding.agent_id.to_string(),
                    selected_mode: binding.selected_mode,
                    mode_confirmed: binding.mode_confirmed,
                    origin: binding.origin,
                    signal_type: binding.signal_type.to_string(),
                    original_delivery_id: binding.original_delivery_id.map(str::to_string),
                    required_fields: match binding.required_fields {
                        Some(fields) => normalize_required_fields(fields)?,
                        None => default_required_fields(binding.selected_mode, binding.origin),
                    },
                    trade_amount_u: seed.and_then(|consent| consent.trade_amount_u.clone()),
                    cap_u: seed.and_then(|consent| consent.cap_u.clone()),
                    quote_token: seed.and_then(|consent| consent.quote_token.clone()),
                    trade_environment: seed.and_then(|consent| consent.trade_environment),
                    margin_mode: seed.and_then(|consent| consent.margin_mode),
                    order_policy: seed.and_then(|consent| consent.order_policy),
                    created_at: now,
                    expires_at: now.saturating_add(TTL_SECS),
                },
                true,
            )
        }
        (None, None) => anyhow::bail!("no live consent continuation for this job"),
    };

    // Persist a newly selected mode/binding before validating optional values.
    // A bad amount/cap/quote must not erase the user's A/B choice, while those
    // invalid values themselves must never enter the record.
    if is_new {
        write_record(&file)?;
    }

    let mut base = file.clone();
    if let Some(selected_mode) = selected_mode {
        if base.origin != Origin::SubscriptionRestore {
            anyhow::bail!("--mode can update only a subscription-restore continuation");
        }
        base.selected_mode = selected_mode;
        base.mode_confirmed = true;
        // Persist the safe user-selected mode even if a value supplied in the
        // same reply fails validation. Invalid values themselves remain absent.
        write_record(&base)?;
    }
    let mut candidate = base.clone();
    let mut validation_errors = Vec::new();
    if let Some(value) = values.trade_amount_u {
        match parse_positive(value, "--trade-amount") {
            Ok(value) => candidate.trade_amount_u = Some(value),
            Err(error) => validation_errors.push(ValidationError {
                field: "tradeAmount".to_string(),
                code: "invalid_amount".to_string(),
                message: error.to_string(),
            }),
        }
    }
    if let Some(value) = values.cap_u {
        match parse_positive(value, "--cap") {
            Ok(value) => candidate.cap_u = Some(value),
            Err(error) => validation_errors.push(ValidationError {
                field: "cap".to_string(),
                code: "invalid_cap".to_string(),
                message: error.to_string(),
            }),
        }
    }
    if let Some(value) = values.quote_token {
        match normalize_quote(value) {
            Ok(value) => candidate.quote_token = Some(value),
            Err(error) => validation_errors.push(ValidationError {
                field: "quote".to_string(),
                code: "invalid_quote".to_string(),
                message: error.to_string(),
            }),
        }
    }
    if let Some(value) = values.trade_environment {
        match TradeEnvironment::parse(value) {
            Ok(value) if value.is_explicit() => candidate.trade_environment = Some(value),
            Ok(_) | Err(_) => validation_errors.push(ValidationError {
                field: "environment".to_string(),
                code: "invalid_environment".to_string(),
                message: "--environment must be one of: live | demo".to_string(),
            }),
        }
    }
    if let Some(value) = values.margin_mode {
        match MarginMode::parse(value) {
            Ok(value) => candidate.margin_mode = Some(value),
            Err(error) => validation_errors.push(ValidationError {
                field: "marginMode".to_string(),
                code: "invalid_margin_mode".to_string(),
                message: error.to_string(),
            }),
        }
    }
    if let Some(value) = values.order_policy {
        match OrderPolicy::parse(value) {
            Ok(value) => candidate.order_policy = Some(value),
            Err(error) => validation_errors.push(ValidationError {
                field: "orderPolicy".to_string(),
                code: "invalid_order_policy".to_string(),
                message: error.to_string(),
            }),
        }
    }
    let file = if validation_errors.is_empty() {
        write_record(&candidate)?;
        candidate
    } else {
        base
    };

    let missing_fields = file.missing_fields();
    let complete = validation_errors.is_empty() && missing_fields.is_empty();
    let consent_command = complete.then(|| {
        let environment = file
            .trade_environment
            .map(|value| format!(" --environment {}", value.as_str()))
            .unwrap_or_default();
        let margin_mode = file
            .margin_mode
            .map(|value| format!(" --margin-mode {}", value.as_str()))
            .unwrap_or_default();
        let order_policy = file
            .order_policy
            .map(|value| format!(" --order-policy {}", value.as_str()))
            .unwrap_or_default();
        match file.selected_mode {
        SelectedMode::Auto => {
            let amount = file
                .trade_amount_u
                .as_deref()
                .map(|value| format!(" --trade-amount {value}"))
                .unwrap_or_default();
            let cap = file
                .cap_u
                .as_deref()
                .map(|value| format!(" --cap {value}"))
                .unwrap_or_default();
            let quote = file
                .quote_token
                .as_deref()
                .map(|value| format!(" --quote {value}"))
                .unwrap_or_default();
            format!(
                "onchainos agent autotrade-consent-set --job-id {} --agent-id {} --mode auto{amount}{cap}{quote}{environment}{margin_mode}{order_policy}",
                file.job_id, file.agent_id
            )
        }
        SelectedMode::Manual => {
            let amount = file
                .trade_amount_u
                .as_deref()
                .map(|value| format!(" --trade-amount {value}"))
                .unwrap_or_default();
            let quote = file
                .quote_token
                .as_deref()
                .map(|value| format!(" --quote {value}"))
                .unwrap_or_default();
            format!(
                "onchainos agent autotrade-consent-set --job-id {} --agent-id {} --mode manual{amount}{quote}{environment}{margin_mode}{order_policy}",
                file.job_id, file.agent_id
            )
        }
        }
    });

    Ok(ContinuationResult {
        continuation_id: file.continuation_id,
        job_id: file.job_id,
        agent_id: file.agent_id,
        selected_mode: file.selected_mode,
        mode_confirmed: file.mode_confirmed,
        origin: file.origin,
        signal_type: file.signal_type,
        original_delivery_id: file.original_delivery_id,
        required_fields: file.required_fields,
        missing_fields,
        validation_errors,
        complete,
        consent_command,
    })
}

fn write_record(file: &ConsentContinuation) -> anyhow::Result<()> {
    let body = serde_json::to_vec_pretty(file)?;
    crate::home::write_secure(&continuation_path(&file.job_id)?, &body)?;
    Ok(())
}

/// Remove a continuation after consent is written, paused, or explicitly skipped.
pub fn clear(job_id: &str) {
    if let Ok(path) = continuation_path(job_id) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn cancel(job_id: &str, agent_id: &str, continuation_id: &str) -> anyhow::Result<()> {
    if !continuation_id_is_safe(continuation_id) {
        anyhow::bail!("invalid consent continuation id");
    }
    let Some(file) = read_live(job_id)? else {
        return Ok(());
    };
    if file.agent_id != agent_id {
        anyhow::bail!("consent continuation agent does not match");
    }
    if continuation_id != file.continuation_id {
        anyhow::bail!("consent continuation id does not match");
    }
    clear(job_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_home<F: FnOnce()>(f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("continuation-test-{}", uuid::Uuid::new_v4()));
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn auto_mode_survives_partial_updates_and_builds_bounded_command() {
        with_home(|| {
            let start = StartBinding {
                job_id: "job-1",
                agent_id: "7",
                selected_mode: SelectedMode::Auto,
                mode_confirmed: true,
                origin: Origin::Delivery,
                signal_type: "spot",
                original_delivery_id: Some("delivery-1"),
                required_fields: None,
                seed_consent: None,
            };
            let first = start_or_update(
                Some(start),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues {
                    trade_amount_u: Some("10.00"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert_eq!(first.selected_mode, SelectedMode::Auto);
            assert_eq!(first.missing_fields, ["cap", "quote"]);
            assert!(!first.complete);

            let completed = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                None,
                ExplicitValues {
                    cap_u: Some("20"),
                    quote_token: Some("USDC"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert!(completed.complete);
            assert_eq!(
                completed.original_delivery_id.as_deref(),
                Some("delivery-1")
            );
            assert_eq!(
                completed.consent_command.as_deref(),
                Some(
                    "onchainos agent autotrade-consent-set --job-id job-1 --agent-id 7 --mode auto --trade-amount 10 --cap 20 --quote usdc"
                )
            );
        });
    }

    #[test]
    fn continuation_rejects_cross_job_id_but_accepts_amount_above_cap() {
        with_home(|| {
            let start = StartBinding {
                job_id: "job-1",
                agent_id: "7",
                selected_mode: SelectedMode::Auto,
                mode_confirmed: true,
                origin: Origin::PreDelivery,
                signal_type: "spot",
                original_delivery_id: None,
                required_fields: None,
                seed_consent: None,
            };
            let first = start_or_update(
                Some(start),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();
            assert!(load_for_resume("job-2", "7", &first.continuation_id).is_err());
            let completed = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                None,
                ExplicitValues {
                    trade_amount_u: Some("21"),
                    cap_u: Some("20"),
                    quote_token: Some("usdt"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert!(completed.complete);
            assert!(completed.validation_errors.is_empty());
            assert!(completed.missing_fields.is_empty());

            let resumed = load_for_resume("job-1", "7", &first.continuation_id).unwrap();
            assert_eq!(resumed.trade_amount_u.as_deref(), Some("21"));
            assert_eq!(resumed.cap_u.as_deref(), Some("20"));
            assert_eq!(resumed.quote_token.as_deref(), Some("usdt"));
        });
    }

    #[test]
    fn first_amount_above_cap_is_persisted_as_configuration() {
        with_home(|| {
            let result = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Auto,
                    mode_confirmed: true,
                    origin: Origin::Delivery,
                    signal_type: "spot",
                    original_delivery_id: Some("delivery-1"),
                    required_fields: None,
                    seed_consent: None,
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues {
                    trade_amount_u: Some("100"),
                    cap_u: Some("50"),
                    quote_token: Some("usdt"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();

            assert!(result.complete);
            assert!(result.validation_errors.is_empty());
            assert!(result.missing_fields.is_empty());
            let persisted = load_for_resume("job-1", "7", &result.continuation_id).unwrap();
            assert_eq!(persisted.selected_mode, SelectedMode::Auto);
            assert_eq!(persisted.origin, Origin::Delivery);
            assert_eq!(
                persisted.original_delivery_id.as_deref(),
                Some("delivery-1")
            );
            assert_eq!(persisted.trade_amount_u.as_deref(), Some("100"));
            assert_eq!(persisted.cap_u.as_deref(), Some("50"));
            assert_eq!(persisted.quote_token.as_deref(), Some("usdt"));
        });
    }

    #[test]
    fn manual_pre_delivery_needs_no_trade_amount_and_cancel_consumes_state() {
        with_home(|| {
            let result = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Manual,
                    mode_confirmed: true,
                    origin: Origin::PreDelivery,
                    signal_type: "spot",
                    original_delivery_id: None,
                    required_fields: None,
                    seed_consent: None,
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();
            assert!(result.complete);
            cancel("job-1", "7", &result.continuation_id).unwrap();
            assert!(load_for_resume("job-1", "7", &result.continuation_id).is_err());
            cancel("job-1", "7", &result.continuation_id).unwrap();
        });
    }

    #[test]
    fn subscription_restore_requires_only_description_selected_fields() {
        with_home(|| {
            let required = vec!["tradeAmount".to_string(), "quote".to_string()];
            let first = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Auto,
                    mode_confirmed: false,
                    origin: Origin::SubscriptionRestore,
                    signal_type: "spot",
                    original_delivery_id: None,
                    required_fields: Some(&required),
                    seed_consent: None,
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();
            assert_eq!(first.required_fields, ["tradeAmount", "quote"]);
            assert_eq!(first.missing_fields, ["mode", "tradeAmount", "quote"]);
            assert!(!first.complete);

            let completed = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                Some(SelectedMode::Auto),
                ExplicitValues {
                    trade_amount_u: Some("12.5"),
                    quote_token: Some("USDT"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert!(completed.complete);
            assert_eq!(
                completed.consent_command.as_deref(),
                Some(
                    "onchainos agent autotrade-consent-set --job-id job-1 --agent-id 7 --mode auto --trade-amount 12.5 --quote usdt"
                )
            );
        });
    }

    #[test]
    fn subscription_restore_collects_complete_trade_kit_settings() {
        with_home(|| {
            let required = vec![
                "tradeAmount".to_string(),
                "cap".to_string(),
                "quote".to_string(),
                "environment".to_string(),
                "marginMode".to_string(),
                "orderPolicy".to_string(),
            ];
            let first = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Auto,
                    mode_confirmed: false,
                    origin: Origin::SubscriptionRestore,
                    signal_type: "perp",
                    original_delivery_id: None,
                    required_fields: Some(&required),
                    seed_consent: None,
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();
            assert_eq!(
                first.missing_fields,
                [
                    "mode",
                    "tradeAmount",
                    "cap",
                    "quote",
                    "environment",
                    "marginMode",
                    "orderPolicy"
                ]
            );

            let completed = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                Some(SelectedMode::Auto),
                ExplicitValues {
                    trade_amount_u: Some("10"),
                    cap_u: Some("100"),
                    quote_token: Some("usdt"),
                    trade_environment: Some("demo"),
                    margin_mode: Some("cross"),
                    order_policy: Some("signal_price_limit"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert!(completed.complete);
            assert_eq!(
                completed.consent_command.as_deref(),
                Some(
                    "onchainos agent autotrade-consent-set --job-id job-1 --agent-id 7 --mode auto --trade-amount 10 --cap 100 --quote usdt --environment demo --margin-mode cross --order-policy signal_price_limit"
                )
            );
        });
    }

    #[test]
    fn subscription_restore_can_switch_to_manual_without_missing_auto_fields() {
        with_home(|| {
            let required = vec!["tradeAmount".to_string(), "cap".to_string()];
            let first = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Auto,
                    mode_confirmed: false,
                    origin: Origin::SubscriptionRestore,
                    signal_type: "spot",
                    original_delivery_id: None,
                    required_fields: Some(&required),
                    seed_consent: None,
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();

            let completed = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                Some(SelectedMode::Manual),
                ExplicitValues::default(),
            )
            .unwrap();
            assert!(completed.complete);
            assert!(completed.missing_fields.is_empty());
            assert_eq!(
                completed.consent_command.as_deref(),
                Some(
                    "onchainos agent autotrade-consent-set --job-id job-1 --agent-id 7 --mode manual"
                )
            );
        });
    }

    #[test]
    fn subscription_restore_prefills_trusted_legacy_consent_and_builds_full_reauthorization() {
        with_home(|| {
            super::super::consent::write_consent_policy_with_settings(
                "job-1",
                ConsentMode::Auto,
                Some("100"),
                Some("10"),
                Some("usdt"),
                Some(TradeEnvironment::Live),
                None,
                None,
                3600,
            )
            .unwrap();
            let mut seed = super::super::consent::load_consent("job-1")
                .unwrap()
                .unwrap();
            seed.version = super::super::consent::CONSENT_VERSION - 1;
            let required = vec!["environment".to_string(), "orderPolicy".to_string()];
            let first = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Auto,
                    mode_confirmed: false,
                    origin: Origin::SubscriptionRestore,
                    signal_type: "spot",
                    original_delivery_id: None,
                    required_fields: Some(&required),
                    seed_consent: Some(&seed),
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();
            assert_eq!(first.missing_fields, ["mode", "orderPolicy"]);

            let completed = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                Some(SelectedMode::Auto),
                ExplicitValues {
                    order_policy: Some("market"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert!(completed.complete);
            let command = completed.consent_command.unwrap();
            assert!(command.contains("--mode auto"));
            assert!(command.contains("--trade-amount 10"));
            assert!(command.contains("--cap 100"));
            assert!(command.contains("--quote usdt"));
            assert!(command.contains("--environment live"));
            assert!(command.contains("--order-policy market"));
        });
    }

    #[test]
    fn restore_mode_confirmation_survives_an_invalid_value_in_the_same_reply() {
        with_home(|| {
            let required = vec!["tradeAmount".to_string()];
            let first = start_or_update(
                Some(StartBinding {
                    job_id: "job-1",
                    agent_id: "7",
                    selected_mode: SelectedMode::Auto,
                    mode_confirmed: false,
                    origin: Origin::SubscriptionRestore,
                    signal_type: "spot",
                    original_delivery_id: None,
                    required_fields: Some(&required),
                    seed_consent: None,
                }),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();

            let invalid = start_or_update(
                None,
                "job-1",
                "7",
                Some(&first.continuation_id),
                Some(SelectedMode::Auto),
                ExplicitValues {
                    trade_amount_u: Some("bad"),
                    ..ExplicitValues::default()
                },
            )
            .unwrap();
            assert!(invalid.mode_confirmed);
            assert_eq!(invalid.missing_fields, ["tradeAmount"]);
            assert_eq!(invalid.validation_errors.len(), 1);

            let persisted = load_for_resume("job-1", "7", &first.continuation_id).unwrap();
            assert!(persisted.mode_confirmed);
            assert_eq!(persisted.selected_mode, SelectedMode::Auto);
            assert!(persisted.trade_amount_u.is_none());
        });
    }

    #[test]
    fn existing_continuation_rejects_start_or_resume_without_exact_id() {
        with_home(|| {
            let start = StartBinding {
                job_id: "job-1",
                agent_id: "7",
                selected_mode: SelectedMode::Manual,
                mode_confirmed: true,
                origin: Origin::PreDelivery,
                signal_type: "spot",
                original_delivery_id: None,
                required_fields: None,
                seed_consent: None,
            };
            let first = start_or_update(
                Some(start),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .unwrap();

            let duplicate = StartBinding {
                job_id: "job-1",
                agent_id: "7",
                selected_mode: SelectedMode::Manual,
                mode_confirmed: true,
                origin: Origin::PreDelivery,
                signal_type: "spot",
                original_delivery_id: None,
                required_fields: None,
                seed_consent: None,
            };
            assert!(start_or_update(
                Some(duplicate),
                "job-1",
                "7",
                None,
                None,
                ExplicitValues::default(),
            )
            .is_err());
            assert!(
                start_or_update(None, "job-1", "7", None, None, ExplicitValues::default(),)
                    .is_err()
            );
            assert!(cancel("job-1", "7", "atc_00000000000000000000000000000000").is_err());
            assert!(load_for_resume("job-1", "7", &first.continuation_id).is_ok());
        });
    }
}
