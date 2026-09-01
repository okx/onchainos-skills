//! Locally persisted, guide-driven copy-trading contract.
//!
//! A service Guide is provider-supplied text, but it never becomes shell code.
//! Its companion semantic declaration selects one of the already-supported
//! execution tools and declaratively maps confirmed consent and signal values to
//! that tool's parameters.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::grants::job_id_is_safe;
use super::tooling::ExecutionTool;

pub const GUIDE_VERSION: u32 = 1;
pub const GUIDE_CONSENT_VERSION: u32 = 1;
const MAX_GUIDE_CHARS: usize = 48 * 1024;
const MAX_FIELDS: usize = 64;
const MAX_BINDINGS: usize = 96;
const GUIDE_SIGNAL_RESOLUTION_VERSION: u32 = 1;
const MAX_SIGNAL_VALUES_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideField {
    pub key: String,
    #[serde(default)]
    pub required: bool,
    #[serde(rename = "type", default)]
    pub value_type: Option<GuideValueType>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuideValueType {
    Boolean,
    Integer,
    Decimal,
    String,
    Array,
    Object,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideBinding {
    pub parameter: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub map: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideCondition {
    pub source: String,
    pub equals: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideExecution {
    pub tool_id: ExecutionTool,
    pub operation: String,
    /// Name of the Guide-bound execution parameter that represents the exact
    /// authorized size. This is a parameter role, not a platform Consent key.
    pub authorization_parameter: String,
    #[serde(default)]
    pub bindings: Vec<GuideBinding>,
    #[serde(default)]
    pub conditions: Vec<GuideCondition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideSemantics {
    #[serde(default)]
    pub consent_fields: Vec<GuideField>,
    #[serde(default)]
    pub signal_fields: Vec<GuideField>,
    pub execution: GuideExecution,
}

#[derive(Clone, Debug)]
pub struct GuideDraft {
    pub source: String,
    pub source_hash: String,
    pub semantics: GuideSemantics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideFile {
    pub version: u32,
    pub job_id: String,
    pub service_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_agent_id: Option<String>,
    pub source_hash: String,
    pub semantics: GuideSemantics,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionIntent {
    pub guide_hash: String,
    pub intent_hash: String,
    pub tool_id: String,
    pub operation: String,
    pub authorization_amount: Option<String>,
    pub eligible: bool,
    pub parameters: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmet_conditions: Vec<String>,
}

/// A Guide-driven Consent deliberately contains no product-specific fields.
/// Every value is keyed by the matching Guide's declared `consentFields`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideConsentFile {
    pub version: u32,
    pub job_id: String,
    pub guide_hash: String,
    pub lifecycle: GuideConsentLifecycle,
    #[serde(default)]
    pub values: BTreeMap<String, Value>,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GuideConsentLifecycle {
    Prepared,
    Active,
    Aborted,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideConsentSnapshot {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guide_hash: Option<String>,
}

/// A schema-validated extraction of one saved Signal. The raw Signal remains
/// the source of truth; this record only stores the fields the Guide declares
/// after the model has interpreted a non-JSON Signal.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GuideSignalResolution {
    version: u32,
    job_id: String,
    delivery_id: String,
    guide_hash: String,
    saved_signal_hash: String,
    values: BTreeMap<String, Value>,
    intent_hash: String,
    created_at: u64,
}

pub fn parse_draft(
    source: Option<&str>,
    source_hash: Option<&str>,
    semantics_json: Option<&str>,
) -> Result<Option<GuideDraft>> {
    match (source, semantics_json) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            bail!("--service-guide and --autotrade-guide-semantics-json must be supplied together")
        }
        (Some(source), Some(semantics_json)) => {
            if source.trim().is_empty() || source.chars().count() > MAX_GUIDE_CHARS {
                bail!("--service-guide must be between 1 and {MAX_GUIDE_CHARS} characters")
            }
            let semantics: GuideSemantics = serde_json::from_str(semantics_json)
                .context("--autotrade-guide-semantics-json must be valid JSON")?;
            validate_semantics(&semantics)?;
            let computed_hash = sha256_hex(source.as_bytes());
            let supplied_hash = source_hash.unwrap_or(&computed_hash).trim();
            let source_hash = supplied_hash
                .strip_prefix("sha256:")
                .unwrap_or(supplied_hash)
                .to_ascii_lowercase();
            if !is_sha256_hex(&source_hash) {
                bail!("--service-guide-hash must be a lowercase SHA-256 hex digest")
            }
            if source_hash != computed_hash {
                bail!("--service-guide-hash does not match --service-guide")
            }
            Ok(Some(GuideDraft {
                source: source.to_string(),
                source_hash,
                semantics,
            }))
        }
    }
}

/// Validate the user-confirmed values that will be persisted to Consent. Field
/// names and requirements are entirely Guide-defined; the platform supplies no
/// business-field allowlist here.
pub fn validate_consent_values(
    semantics: &GuideSemantics,
    values: &BTreeMap<String, Value>,
) -> Result<()> {
    validate_semantics(semantics)?;
    let declared = semantics
        .consent_fields
        .iter()
        .map(|field| field.key.as_str())
        .collect::<BTreeSet<_>>();
    for key in values.keys() {
        if !declared.contains(key.as_str()) {
            bail!("Guide Consent contains an undeclared field: {key}")
        }
        if field_key_is_sensitive(key) {
            bail!("credentials must not be stored in Guide Consent: {key}")
        }
    }
    for field in &semantics.consent_fields {
        let value = values.get(&field.key);
        if field.required && value.is_none() {
            bail!("required Guide consent field is missing: {}", field.key)
        }
        if let Some(value) = value {
            validate_value_type(value, field.value_type, "consent", &field.key)?;
        }
    }
    Ok(())
}

/// Validate the fields extracted from a saved Signal. The Signal itself may be
/// plain text, Markdown, or JSON; only this small typed projection is persisted
/// and used for Guide bindings.
pub fn validate_signal_values(
    semantics: &GuideSemantics,
    values: &BTreeMap<String, Value>,
) -> Result<()> {
    validate_semantics(semantics)?;
    let declared = semantics
        .signal_fields
        .iter()
        .map(|field| field.key.as_str())
        .collect::<BTreeSet<_>>();
    for key in values.keys() {
        if !declared.contains(key.as_str()) {
            bail!("Guide Signal contains an undeclared field: {key}")
        }
        if field_key_is_sensitive(key) {
            bail!("credentials must not be stored in Guide Signal values: {key}")
        }
    }
    for field in &semantics.signal_fields {
        let value = values.get(&field.key);
        if field.required && value.is_none() {
            bail!("required Guide signal field is missing: {}", field.key)
        }
        if let Some(value) = value {
            validate_value_type(value, field.value_type, "signal", &field.key)?;
        }
    }
    if serde_json::to_vec(values)?.len() > MAX_SIGNAL_VALUES_BYTES {
        bail!("Guide Signal values are too large")
    }
    Ok(())
}

impl GuideDraft {
    pub fn into_file(
        self,
        job_id: &str,
        service_id: &str,
        provider_agent_id: Option<&str>,
    ) -> GuideFile {
        GuideFile {
            version: GUIDE_VERSION,
            job_id: job_id.to_string(),
            service_id: service_id.to_string(),
            provider_agent_id: provider_agent_id.map(ToOwned::to_owned),
            source_hash: self.source_hash,
            semantics: self.semantics,
            created_at: now_secs(),
        }
    }
}

pub fn guide_path(job_id: &str) -> Result<PathBuf> {
    if !job_id_is_safe(job_id) {
        bail!("invalid job id")
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("guide")
        .join(format!("{job_id}.md")))
}

pub fn write_guide(file: &GuideFile, source: &str) -> Result<()> {
    validate_file(file)?;
    if sha256_hex(source.as_bytes()) != file.source_hash {
        bail!("service guide content does not match its hash")
    }
    crate::home::write_secure(
        &guide_path(&file.job_id)?,
        render_markdown("guide", file, source)?.as_bytes(),
    )?;
    Ok(())
}

pub fn load_guide(job_id: &str) -> Result<GuideFile> {
    let raw = std::fs::read_to_string(guide_path(job_id)?)
        .context("service guide is not available locally")?;
    let (file, source): (GuideFile, &str) = parse_markdown_document("guide", &raw)?;
    validate_file(&file)?;
    if file.job_id != job_id {
        bail!("service guide job id mismatch")
    }
    if sha256_hex(source.as_bytes()) != file.source_hash {
        bail!("local service guide content does not match its hash")
    }
    Ok(file)
}

pub fn render_markdown<T: Serialize>(kind: &str, metadata: &T, body: &str) -> Result<String> {
    let metadata = serde_json::to_string_pretty(metadata)?;
    Ok(format!(
        "<!-- onchainos-autotrade:{kind}\n{metadata}\n-->\n\n{body}"
    ))
}

pub fn parse_markdown<T: DeserializeOwned>(kind: &str, raw: &str) -> Result<T> {
    Ok(parse_markdown_document(kind, raw)?.0)
}

fn parse_markdown_document<'a, T: DeserializeOwned>(
    kind: &str,
    raw: &'a str,
) -> Result<(T, &'a str)> {
    let prefix = format!("<!-- onchainos-autotrade:{kind}\n");
    let rest = raw
        .strip_prefix(&prefix)
        .context("local autotrade document header is invalid")?;
    let (metadata, body) = rest
        .split_once("\n-->\n")
        .context("local autotrade document metadata is invalid")?;
    let body = body.strip_prefix('\n').unwrap_or(body);
    Ok((
        serde_json::from_str(metadata).context("local autotrade document metadata is invalid")?,
        body,
    ))
}

pub fn consent_path(job_id: &str) -> Result<PathBuf> {
    if !job_id_is_safe(job_id) {
        bail!("invalid job id")
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("consent")
        .join(format!("{job_id}.md")))
}

pub fn write_prepared_consent(
    job_id: &str,
    guide: &GuideFile,
    values: BTreeMap<String, Value>,
    ttl_sec: u64,
) -> Result<()> {
    validate_consent_values(&guide.semantics, &values)?;
    let now = now_secs();
    let file = GuideConsentFile {
        version: GUIDE_CONSENT_VERSION,
        job_id: job_id.to_string(),
        guide_hash: guide_contract_hash(guide)?,
        lifecycle: GuideConsentLifecycle::Prepared,
        values,
        created_at: now,
        expires_at: now.saturating_add(ttl_sec),
    };
    write_guide_consent(&file)
}

pub fn activate_prepared_consent(job_id: &str) -> Result<()> {
    let mut consent = read_guide_consent(job_id)?.context("prepared Guide Consent is not available")?;
    if consent.lifecycle != GuideConsentLifecycle::Prepared {
        bail!("Guide Consent is not prepared")
    }
    consent.lifecycle = GuideConsentLifecycle::Active;
    write_guide_consent(&consent)
}

pub fn abort_prepared_consent(job_id: &str) {
    let Ok(Some(mut consent)) = read_guide_consent(job_id) else {
        return;
    };
    if consent.lifecycle == GuideConsentLifecycle::Prepared {
        consent.lifecycle = GuideConsentLifecycle::Aborted;
        let _ = write_guide_consent(&consent);
    }
}

pub fn consent_snapshot(job_id: &str) -> GuideConsentSnapshot {
    match load_active_consent(job_id) {
        Ok(Some(consent)) => GuideConsentSnapshot {
            status: "active",
            guide_hash: Some(consent.guide_hash),
        },
        _ => GuideConsentSnapshot {
            status: "unavailable",
            guide_hash: None,
        },
    }
}

fn write_guide_consent(file: &GuideConsentFile) -> Result<()> {
    if file.version != GUIDE_CONSENT_VERSION
        || !job_id_is_safe(&file.job_id)
        || !is_sha256_hex(&file.guide_hash)
    {
        bail!("Guide Consent metadata is invalid")
    }
    crate::home::write_secure(
        &consent_path(&file.job_id)?,
        render_markdown(
            "consent",
            file,
            "# Service Consent\n\nValues in this document are defined exclusively by the matching Service Guide.\n",
        )?
        .as_bytes(),
    )?;
    Ok(())
}

fn read_guide_consent(job_id: &str) -> Result<Option<GuideConsentFile>> {
    let path = consent_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).context("Guide Consent is not readable")?;
    let consent: GuideConsentFile = parse_markdown("consent", &raw)?;
    if consent.version != GUIDE_CONSENT_VERSION
        || consent.job_id != job_id
        || !is_sha256_hex(&consent.guide_hash)
    {
        bail!("Guide Consent metadata is invalid")
    }
    Ok(Some(consent))
}

fn load_active_consent(job_id: &str) -> Result<Option<GuideConsentFile>> {
    let Some(consent) = read_guide_consent(job_id)? else {
        return Ok(None);
    };
    if consent.lifecycle != GuideConsentLifecycle::Active || consent.expires_at <= now_secs() {
        return Ok(None);
    }
    Ok(Some(consent))
}

fn safe_delivery_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.'))
}

fn signal_resolution_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid Guide resolution identity")
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("guide-resolution")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn read_signal_hash(saved_signal_path: &str) -> Result<String> {
    let raw = std::fs::read(saved_signal_path).context("saved subscription signal is not readable")?;
    Ok(sha256_hex(&raw))
}

fn load_guide_and_consent(job_id: &str) -> Result<(GuideFile, GuideConsentFile)> {
    let guide = load_guide(job_id)?;
    let consent = load_active_consent(job_id)?.context("active Guide Consent is not available locally")?;
    if consent.guide_hash != guide_contract_hash(&guide)? {
        bail!("service guide and consent do not match")
    }
    Ok((guide, consent))
}

/// Persist one Guide-declared interpretation of a saved Signal. This accepts a
/// typed field projection, not a JSON requirement on the raw signal file.
pub fn resolve_execution_intent(
    job_id: &str,
    delivery_id: &str,
    saved_signal_path: &str,
    values: BTreeMap<String, Value>,
) -> Result<ExecutionIntent> {
    let (guide, consent) = load_guide_and_consent(job_id)?;
    validate_signal_values(&guide.semantics, &values)?;
    let signal = Value::Object(values.clone().into_iter().collect());
    let intent = build_intent(&guide, &consent, &signal)?;
    let resolution = GuideSignalResolution {
        version: GUIDE_SIGNAL_RESOLUTION_VERSION,
        job_id: job_id.to_string(),
        delivery_id: delivery_id.to_string(),
        guide_hash: guide_contract_hash(&guide)?,
        saved_signal_hash: read_signal_hash(saved_signal_path)?,
        values,
        intent_hash: intent.intent_hash.clone(),
        created_at: now_secs(),
    };
    let path = signal_resolution_path(job_id, delivery_id)?;
    crate::home::write_secure(&path, &serde_json::to_vec_pretty(&resolution)?)?;
    Ok(intent)
}

/// Rebuild a previously resolved intent and prove it still belongs to the same
/// Guide, active Consent, and exact saved Signal bytes before a direct claim.
pub fn load_resolved_execution_intent(
    job_id: &str,
    delivery_id: &str,
    saved_signal_path: &str,
) -> Result<ExecutionIntent> {
    let path = signal_resolution_path(job_id, delivery_id)?;
    let resolution: GuideSignalResolution = serde_json::from_slice(
        &std::fs::read(&path).context("Guide Signal has not been resolved")?,
    )
    .context("Guide Signal resolution is invalid")?;
    if resolution.version != GUIDE_SIGNAL_RESOLUTION_VERSION
        || resolution.job_id != job_id
        || resolution.delivery_id != delivery_id
        || !is_sha256_hex(&resolution.guide_hash)
        || !is_sha256_hex(&resolution.saved_signal_hash)
        || !is_sha256_hex(&resolution.intent_hash)
    {
        bail!("Guide Signal resolution metadata is invalid")
    }
    if resolution.saved_signal_hash != read_signal_hash(saved_signal_path)? {
        bail!("saved Signal changed after Guide resolution")
    }
    let (guide, consent) = load_guide_and_consent(job_id)?;
    if resolution.guide_hash != guide_contract_hash(&guide)? {
        bail!("Guide Signal resolution does not match the current Guide")
    }
    validate_signal_values(&guide.semantics, &resolution.values)?;
    let signal = Value::Object(resolution.values.into_iter().collect());
    let intent = build_intent(&guide, &consent, &signal)?;
    if intent.intent_hash != resolution.intent_hash {
        bail!("Guide Signal resolution no longer matches the execution intent")
    }
    Ok(intent)
}

fn build_intent(
    guide: &GuideFile,
    consent: &GuideConsentFile,
    signal: &Value,
) -> Result<ExecutionIntent> {
    validate_consent_values(&guide.semantics, &consent.values)?;
    let guide_hash = guide_contract_hash(guide)?;
    let signal_values = signal
        .as_object()
        .context("Guide Signal values must be a JSON object")?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    validate_signal_values(&guide.semantics, &signal_values)?;

    let mut unmet_conditions = Vec::new();
    for condition in &guide.semantics.execution.conditions {
        let actual = resolve_source(&condition.source, signal, &consent.values)?;
        if actual.as_ref() != Some(&condition.equals) {
            unmet_conditions.push(condition.source.clone());
        }
    }
    let mut parameters = BTreeMap::new();
    if unmet_conditions.is_empty() {
        for binding in &guide.semantics.execution.bindings {
            let value = resolve_source(&binding.source, signal, &consent.values)?
                .with_context(|| format!("Guide binding source is missing: {}", binding.source))?;
            let value = if binding.map.is_empty() {
                value
            } else {
                let key = value
                    .as_str()
                    .context("Guide binding map sources must be strings")?;
                binding
                    .map
                    .get(key)
                    .cloned()
                    .with_context(|| format!("Guide binding map has no value for {key}"))?
            };
            parameters.insert(binding.parameter.clone(), value);
        }
    }
    let authorization_amount = if unmet_conditions.is_empty() {
        let value = parameters
            .get(&guide.semantics.execution.authorization_parameter)
            .context("Guide authorization parameter is missing")?;
        Some(decimal_parameter(value, &guide.semantics.execution.authorization_parameter)?)
    } else {
        None
    };
    let intent_hash = hash_intent(
        &guide_hash,
        guide.semantics.execution.tool_id.token(),
        &guide.semantics.execution.operation,
        &parameters,
        authorization_amount.as_deref(),
        &unmet_conditions,
    )?;
    Ok(ExecutionIntent {
        guide_hash,
        intent_hash,
        tool_id: guide.semantics.execution.tool_id.token().to_string(),
        operation: guide.semantics.execution.operation.clone(),
        authorization_amount,
        eligible: unmet_conditions.is_empty(),
        parameters,
        unmet_conditions,
    })
}

fn validate_file(file: &GuideFile) -> Result<()> {
    if file.version > GUIDE_VERSION
        || !job_id_is_safe(&file.job_id)
        || file.service_id.trim().is_empty()
    {
        bail!("service guide metadata is invalid")
    }
    if !is_sha256_hex(&file.source_hash) {
        bail!("service guide hash is invalid")
    }
    validate_semantics(&file.semantics)
}

fn validate_semantics(semantics: &GuideSemantics) -> Result<()> {
    if semantics.consent_fields.len() > MAX_FIELDS || semantics.signal_fields.len() > MAX_FIELDS {
        bail!("Guide declares too many fields")
    }
    let consent = validate_fields(&semantics.consent_fields, "consent")?;
    let signal = validate_fields(&semantics.signal_fields, "signal")?;
    if semantics.execution.operation.is_empty()
        || semantics.execution.operation.len() > 64
        || !semantics
            .execution
            .operation
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        bail!("Guide execution operation is invalid")
    }
    if semantics.execution.bindings.len() > MAX_BINDINGS {
        bail!("Guide declares too many execution bindings")
    }
    let mut parameters = BTreeSet::new();
    for binding in &semantics.execution.bindings {
        if !field_key_is_safe(&binding.parameter) || !parameters.insert(&binding.parameter) {
            bail!("Guide execution binding parameter is invalid")
        }
        validate_source(&binding.source, &consent, &signal)?;
    }
    for condition in &semantics.execution.conditions {
        validate_source(&condition.source, &consent, &signal)?;
    }
    if !parameters
        .iter()
        .any(|parameter| parameter.as_str() == semantics.execution.authorization_parameter)
    {
        bail!("Guide authorizationParameter must name one execution binding")
    }
    Ok(())
}

fn validate_fields<'a>(fields: &'a [GuideField], label: &str) -> Result<BTreeSet<&'a str>> {
    let mut names = BTreeSet::new();
    for field in fields {
        if !field_key_is_safe(&field.key) || !names.insert(field.key.as_str()) {
            bail!("Guide {label} field is invalid: {}", field.key)
        }
    }
    Ok(names)
}

fn validate_source(source: &str, consent: &BTreeSet<&str>, signal: &BTreeSet<&str>) -> Result<()> {
    let (kind, key) = source
        .split_once('.')
        .context("Guide source must be consent.<field> or signal.<field>")?;
    let valid = match kind {
        "consent" => consent.contains(key),
        "signal" => signal.contains(key),
        _ => false,
    };
    if !valid {
        bail!("Guide source is not declared: {source}")
    }
    Ok(())
}

fn field_key_is_safe(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && key.len() <= 64
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn field_key_is_sensitive(key: &str) -> bool {
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

fn resolve_source(
    source: &str,
    signal: &Value,
    values: &BTreeMap<String, Value>,
) -> Result<Option<Value>> {
    let (kind, key) = source.split_once('.').context("Guide source is invalid")?;
    Ok(match kind {
        "consent" => values.get(key).cloned(),
        "signal" => signal_value(signal, key),
        _ => None,
    })
}

fn signal_value(signal: &Value, key: &str) -> Option<Value> {
    signal
        .as_object()
        .and_then(|fields| fields.get(key))
        .cloned()
}

fn decimal_parameter(value: &Value, name: &str) -> Result<String> {
    let raw = value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_number().map(ToString::to_string))
        .with_context(|| format!("Guide authorization parameter {name} must be numeric"))?;
    let value = super::amount::Decimal::parse(&raw)
        .with_context(|| format!("Guide authorization parameter {name} must be a positive decimal"))?;
    if value.is_zero() {
        bail!("Guide authorization parameter {name} must be positive")
    }
    Ok(value.to_plain_string())
}

fn hash_intent(
    guide_hash: &str,
    tool_id: &str,
    operation: &str,
    parameters: &BTreeMap<String, Value>,
    authorization_amount: Option<&str>,
    unmet_conditions: &[String],
) -> Result<String> {
    let canonical = serde_jcs::to_vec(&serde_json::json!({
        "guideHash": guide_hash,
        "toolId": tool_id,
        "operation": operation,
        "parameters": parameters,
        "authorizationAmount": authorization_amount,
        "unmetConditions": unmet_conditions,
    }))?;
    Ok(sha256_hex(&canonical))
}

/// A Guide contract comprises its verbatim provider text and the declarative
/// field/binding semantics used to interpret that text. The source hash alone
/// is intentionally retained for validating the Markdown body, but it is not
/// sufficient to authorize execution because a changed semantic mapping could
/// turn identical text into different tool parameters.
fn guide_contract_hash(guide: &GuideFile) -> Result<String> {
    let canonical = serde_jcs::to_vec(&serde_json::json!({
        "sourceHash": guide.source_hash,
        "semantics": guide.semantics,
    }))?;
    Ok(sha256_hex(&canonical))
}

fn validate_value_type(
    value: &Value,
    expected: Option<GuideValueType>,
    kind: &str,
    key: &str,
) -> Result<()> {
    let matches = match expected {
        None => true,
        Some(GuideValueType::Boolean) => value.is_boolean(),
        Some(GuideValueType::Integer) => value.as_i64().is_some() || value.as_u64().is_some(),
        Some(GuideValueType::Decimal) => {
            value.is_number() || value.as_str().is_some_and(|raw| raw.parse::<f64>().is_ok())
        }
        Some(GuideValueType::String) => value.is_string(),
        Some(GuideValueType::Array) => value.is_array(),
        Some(GuideValueType::Object) => value.is_object(),
    };
    if !matches {
        bail!("Guide {kind} field {key} has an unexpected type")
    }
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn sha256_hex(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hex::encode(hasher.finalize())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn semantics() -> GuideSemantics {
        serde_json::from_value(serde_json::json!({
            "consentFields": [{"key": "strategyArmed", "required": true, "type": "boolean"}],
            "signalFields": [{"key": "contractCode", "required": true, "type": "string"}, {"key": "directionCode", "required": true, "type": "string"}, {"key": "entryUnits", "required": true, "type": "decimal"}],
            "execution": {
                "toolId": "trade_kit",
                "operation": "place_order",
                "authorizationParameter": "size",
                "conditions": [{"source": "consent.strategyArmed", "equals": true}],
                "bindings": [
                    {"parameter": "instrument", "source": "signal.contractCode"},
                    {"parameter": "side", "source": "signal.directionCode", "map": {"OPEN_LONG": "buy", "OPEN_SHORT": "sell"}},
                    {"parameter": "size", "source": "signal.entryUnits"}
                ]
            }
        })).unwrap()
    }

    #[test]
    fn guide_semantics_define_dynamic_consent_and_signal_mapping() {
        let guide = GuideFile {
            version: GUIDE_VERSION,
            job_id: "job-1".to_string(),
            service_id: "svc-1".to_string(),
            provider_agent_id: None,
            source_hash: "0".repeat(64),
            semantics: semantics(),
            created_at: 0,
        };
        let consent = GuideConsentFile {
            version: GUIDE_CONSENT_VERSION,
            job_id: "job-1".to_string(),
            guide_hash: "0".repeat(64),
            lifecycle: GuideConsentLifecycle::Active,
            values: serde_json::from_value(serde_json::json!({"strategyArmed": true})).unwrap(),
            created_at: 0,
            expires_at: u64::MAX,
        };
        let intent = build_intent(
            &guide,
            &consent,
            &serde_json::json!({"contractCode": "BTC-USDT-SWAP", "directionCode": "OPEN_LONG", "entryUnits": "2.5"}),
        )
        .unwrap();
        assert!(intent.eligible);
        assert_eq!(intent.parameters["instrument"], "BTC-USDT-SWAP");
        assert_eq!(intent.parameters["side"], "buy");
        assert_eq!(intent.authorization_amount.as_deref(), Some("2.5"));
    }

    #[test]
    fn mapped_signal_value_must_be_declared_by_the_service_guide() {
        let guide = GuideFile {
            version: GUIDE_VERSION,
            job_id: "job-1".to_string(),
            service_id: "svc-1".to_string(),
            provider_agent_id: None,
            source_hash: "0".repeat(64),
            semantics: semantics(),
            created_at: 0,
        };
        let consent = GuideConsentFile {
            version: GUIDE_CONSENT_VERSION,
            job_id: "job-1".to_string(),
            guide_hash: "0".repeat(64),
            lifecycle: GuideConsentLifecycle::Active,
            values: serde_json::from_value(serde_json::json!({"strategyArmed": true})).unwrap(),
            created_at: 0,
            expires_at: u64::MAX,
        };
        let error = build_intent(
            &guide,
            &consent,
            &serde_json::json!({"contractCode": "BTC-USDT-SWAP", "directionCode": "WAIT", "entryUnits": "2.5"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Guide binding map has no value"));
    }

    #[test]
    fn supplied_hash_must_match_the_exact_guide_content() {
        let semantics = serde_json::to_string(&semantics()).unwrap();
        let error = parse_draft(Some("guide A"), Some(&"0".repeat(64)), Some(&semantics))
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("does not match --service-guide"));
    }

    #[test]
    fn guide_contract_hash_binds_the_semantic_mapping() {
        let mut guide = GuideFile {
            version: GUIDE_VERSION,
            job_id: "job-1".to_string(),
            service_id: "svc-1".to_string(),
            provider_agent_id: None,
            source_hash: "0".repeat(64),
            semantics: semantics(),
            created_at: 0,
        };
        let original = guide_contract_hash(&guide).unwrap();
        guide.semantics.execution.bindings[2].parameter = "differentSize".to_string();
        assert_ne!(guide_contract_hash(&guide).unwrap(), original);
    }

    #[test]
    fn plain_text_signal_is_resolved_by_guide_declared_values() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("guide_plain_text_signal");
        if home.exists() {
            std::fs::remove_dir_all(&home).ok();
        }
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &home);

        let source = "Read the signal text and extract the declared contract, direction, and units.";
        let semantics_json = serde_json::to_string(&semantics()).unwrap();
        let draft = parse_draft(Some(source), None, Some(&semantics_json))
            .unwrap()
            .unwrap();
        let guide = draft
            .clone()
            .into_file("job-guide-plain", "svc-guide", None);
        write_guide(&guide, &draft.source).unwrap();
        write_prepared_consent(
            "job-guide-plain",
            &guide,
            serde_json::from_value(serde_json::json!({"strategyArmed": true})).unwrap(),
            60,
        )
        .unwrap();
        activate_prepared_consent("job-guide-plain").unwrap();

        let saved_signal = home.join("signal.md");
        std::fs::write(
            &saved_signal,
            "BTC perpetual: open long; use two and a half contracts.",
        )
        .unwrap();
        let values = serde_json::from_value(serde_json::json!({
            "contractCode": "BTC-USDT-SWAP",
            "directionCode": "OPEN_LONG",
            "entryUnits": "2.5"
        }))
        .unwrap();
        let intent = resolve_execution_intent(
            "job-guide-plain",
            "msg:plain-text",
            saved_signal.to_str().unwrap(),
            values,
        )
        .unwrap();
        assert!(intent.eligible);
        assert_eq!(intent.parameters["instrument"], "BTC-USDT-SWAP");

        let reloaded = load_resolved_execution_intent(
            "job-guide-plain",
            "msg:plain-text",
            saved_signal.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(reloaded.intent_hash, intent.intent_hash);

        std::fs::write(&saved_signal, "changed after resolution").unwrap();
        assert!(load_resolved_execution_intent(
            "job-guide-plain",
            "msg:plain-text",
            saved_signal.to_str().unwrap(),
        )
        .is_err());

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }
}
