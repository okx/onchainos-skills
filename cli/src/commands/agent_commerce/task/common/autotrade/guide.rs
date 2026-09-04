//! Locally persisted, guide-driven copy-trading contract.
//!
//! A service Guide is provider-supplied text. The subscribing Agent reads that
//! exact Guide together with the user's matching Consent and a saved Signal at
//! delivery time. No second semantic contract is derived or persisted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::grants::job_id_is_safe;

pub const GUIDE_VERSION: u32 = 1;
pub const GUIDE_CONSENT_VERSION: u32 = 1;
const MAX_GUIDE_CHARS: usize = 48 * 1024;
#[derive(Clone, Debug)]
pub struct GuideDraft {
    pub source: String,
    pub source_hash: String,
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
    pub created_at: u64,
}

/// A Guide-driven Consent deliberately contains no platform-defined business
/// fields. Its values are the user's explicit answers to the matching Guide.
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

pub fn parse_draft(source: Option<&str>, source_hash: Option<&str>) -> Result<Option<GuideDraft>> {
    let Some(source) = source else {
        return Ok(None);
    };
    if source.trim().is_empty() || source.chars().count() > MAX_GUIDE_CHARS {
        bail!("--service-guide must be between 1 and {MAX_GUIDE_CHARS} characters")
    }
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
    }))
}

/// Consent is Guide-defined, so the CLI preserves its fields without imposing a
/// platform schema. Credentials remain prohibited from local Consent records.
pub fn validate_consent_values(values: &BTreeMap<String, Value>) -> Result<()> {
    for key in values.keys() {
        if field_key_is_sensitive(key) {
            bail!("credentials must not be stored in Guide Consent: {key}")
        }
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
    let (file, _source): (GuideFile, &str) = parse_markdown_document("guide", &raw)?;
    validate_file(&file)?;
    if file.job_id != job_id {
        bail!("service guide job id mismatch")
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
    validate_consent_values(&values)?;
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
    let mut consent =
        read_guide_consent(job_id)?.context("prepared Guide Consent is not available")?;
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

fn load_guide_and_consent(job_id: &str) -> Result<(GuideFile, GuideConsentFile)> {
    let guide = load_guide(job_id)?;
    let consent =
        load_active_consent(job_id)?.context("active Guide Consent is not available locally")?;
    Ok((guide, consent))
}

/// A delivery may enter the Guide-direct execution lifecycle only when its
/// locally persisted Guide and active Guide Consent are both available. The
/// Guide source hash is creation metadata only: users may update their local
/// Guide or Consent without re-binding the other document. Any missing, stale,
/// or unreadable record is deliberately a signal-only condition, not an
/// execution error.
pub fn has_active_execution_contract(job_id: &str) -> bool {
    load_guide_and_consent(job_id).is_ok()
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
    Ok(())
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

/// A Guide contract is the provider's verbatim Guide text, represented by its
/// exact source hash. Consent is bound to that hash before activation.
fn guide_contract_hash(guide: &GuideFile) -> Result<String> {
    Ok(guide.source_hash.clone())
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

    #[test]
    fn supplied_hash_must_match_the_exact_guide_content() {
        let error = parse_draft(Some("guide A"), Some(&"0".repeat(64))).unwrap_err();
        assert!(error.to_string().contains("does not match --service-guide"));
    }

    #[test]
    fn consent_rejects_credential_like_keys_without_a_platform_schema() {
        let values = serde_json::from_value(serde_json::json!({"apiKey": "not-allowed"})).unwrap();
        assert!(validate_consent_values(&values).is_err());
    }

    #[test]
    fn guide_and_consent_form_the_active_execution_contract() {
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

        let source = "Follow the saved Signal using only the user's confirmed Consent.";
        let draft = parse_draft(Some(source), None).unwrap().unwrap();
        let guide = draft
            .clone()
            .into_file("job-guide-plain", "svc-guide", None);
        write_guide(&guide, &draft.source).unwrap();
        write_prepared_consent(
            "job-guide-plain",
            &guide,
            serde_json::from_value(serde_json::json!({"followEnabled": true})).unwrap(),
            60,
        )
        .unwrap();
        activate_prepared_consent("job-guide-plain").unwrap();
        assert!(has_active_execution_contract("job-guide-plain"));
        assert_eq!(consent_snapshot("job-guide-plain").status, "active");

        let guide_path = guide_path("job-guide-plain").unwrap();
        let edited_guide = std::fs::read_to_string(&guide_path).unwrap().replace(
            source,
            "Follow the saved Signal using the updated local Guide.",
        );
        std::fs::write(&guide_path, edited_guide).unwrap();

        let mut consent = read_guide_consent("job-guide-plain").unwrap().unwrap();
        consent
            .values
            .insert("followEnabled".to_string(), serde_json::Value::Bool(false));
        write_guide_consent(&consent).unwrap();

        assert!(has_active_execution_contract("job-guide-plain"));

        std::env::remove_var("ONCHAINOS_HOME");
        std::fs::remove_dir_all(home).ok();
    }
}
